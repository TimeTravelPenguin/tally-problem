use std::sync::{Arc, Mutex};

use iced::keyboard::{self, Key, key::Named};
use iced::widget::{
    button, column, container, operation, responsive, row, scrollable, space, text, text_input,
    tooltip,
};
use iced::{Border, Color, Element, Event, Fill, Subscription, Task, Theme, event, mouse};
use tally_problem::{Action, SearchError, SearchProgress, SearchSession};

use crate::focus;
use crate::model::{Form, PreparedSearch, Solution, Step};

const PAGE: &str = "planner-page";
const SEARCH_BATCH: usize = 1_024;
const VIDEO_URL: &str = "https://www.youtube.com/watch?v=AT9wAQSV5_4";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Target,
    Start,
    ResetIndex,
    Solve,
    ResetForm,
    Results,
    Video,
}

impl Focus {
    fn id(self) -> &'static str {
        match self {
            Self::Target => "target-input",
            Self::Start => "start-input",
            Self::ResetIndex => "reset-input",
            Self::Solve => "solve-button",
            Self::ResetForm => "reset-button",
            Self::Results => "results-panel",
            Self::Video => "video-button",
        }
    }

    fn reveal_id(self) -> &'static str {
        match self {
            Self::Target => "target-field",
            Self::Start => "start-field",
            Self::ResetIndex => "reset-field",
            Self::Solve | Self::ResetForm | Self::Results | Self::Video => self.id(),
        }
    }

    fn help(self) -> Option<&'static str> {
        match self {
            Self::Target => Some(
                "The value to reach, using one or more decimal digits. Leading zeros set the counter width: 0012 uses four digits. Larger counters may take longer to solve.",
            ),
            Self::Start => Some(
                "The counter's current digits. Leave blank to start at zero, or enter the same number of digits as the target, including leading zeros.",
            ),
            Self::ResetIndex => Some(
                "The current knob position (0–9). It determines which digits the next forward reset pushes; it is not the number of reset ticks.",
            ),
            Self::Solve | Self::ResetForm | Self::Results | Self::Video => None,
        }
    }

    fn activation(self) -> Option<Message> {
        match self {
            Self::Solve => Some(Message::SolvePressed),
            Self::ResetForm => Some(Message::ResetForm),
            Self::Video => Some(Message::OpenVideo),
            Self::Target | Self::Start | Self::ResetIndex | Self::Results => None,
        }
    }
}

struct Job {
    session: Arc<Mutex<SearchSession>>,
    prepared: PreparedSearch,
}

pub struct Planner {
    form: Form,
    focus: Option<Focus>,
    focus_revision: u64,
    job: Option<Job>,
    generation: u64,
    visited_states: usize,
    solution: Option<Solution>,
    error: Option<String>,
    video_error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    TargetChanged(String),
    StartChanged(String),
    ResetIndexChanged(String),
    Solve,
    SolvePressed,
    ResetForm,
    OpenVideo,
    Progress(u64, Result<SearchProgress, SearchError>),
    KeyPressed(Key, keyboard::Modifiers),
    PointerPressed,
    InputFocused(u64, &'static str, bool),
    RevealFocus(u64, Option<f32>),
}

impl Planner {
    pub fn new() -> (Self, Task<Message>) {
        (
            Self {
                form: Form::default(),
                focus: Some(Focus::Target),
                focus_revision: 0,
                job: None,
                generation: 0,
                visited_states: 0,
                solution: None,
                error: None,
                video_error: None,
            },
            operation::focus(Focus::Target.id()),
        )
    }

    pub fn subscription(&self) -> Subscription<Message> {
        event::listen_with(|event, status, _window| match event {
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                repeat,
                ..
            }) if status == event::Status::Ignored
                && (!repeat
                    || !matches!(key, Key::Named(Named::Enter | Named::Space | Named::Tab))) =>
            {
                Some(Message::KeyPressed(key, modifiers))
            }

            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(iced::touch::Event::FingerPressed { .. }) => {
                Some(Message::PointerPressed)
            }

            _ => None,
        })
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TargetChanged(value) => {
                self.form.target = value;
                self.changed(Focus::Target);
            }

            Message::StartChanged(value) => {
                self.form.start = value;
                self.changed(Focus::Start);
            }

            Message::ResetIndexChanged(value) => {
                self.form.reset_index = value;
                self.changed(Focus::ResetIndex);
            }

            Message::Solve => return self.solve(),
            Message::SolvePressed => {
                let task = self.solve();

                if self.job.is_some() {
                    return Task::batch([self.focus(Focus::ResetForm), task]);
                }

                return task;
            }

            Message::ResetForm => {
                let was_searching = self.job.is_some();
                self.cancel();
                self.solution = None;
                self.error = None;

                if !was_searching {
                    self.form = Form::default();
                }

                return self.focus(Focus::Target);
            }

            Message::OpenVideo => {
                // Open synchronously while handling the user's click or key press.
                self.video_error = webbrowser::open(VIDEO_URL)
                    .err()
                    .map(|error| format!("Unable to open the video: {error}"));

                return self.focus(Focus::Video);
            }

            Message::Progress(generation, progress) => {
                if generation != self.generation || self.job.is_none() {
                    return Task::none();
                }

                match progress {
                    Ok(SearchProgress::InProgress { visited_states }) => {
                        self.visited_states = visited_states;

                        if let Some(job) = &self.job {
                            return search_task(Arc::clone(&job.session), generation);
                        }
                    }

                    Ok(SearchProgress::Complete(result)) => {
                        if let Some(job) = self.job.take() {
                            match job.prepared.finish(result) {
                                Ok(solution) => self.solution = Some(solution),
                                Err(error) => self.error = Some(error),
                            }
                        }
                    }

                    Err(error) => {
                        self.job = None;
                        self.error = Some(error.to_string());
                    }
                }
            }

            Message::KeyPressed(key, modifiers) => {
                return self.key_pressed(key, modifiers);
            }

            Message::PointerPressed => {
                self.focus = None;
                self.focus_revision += 1;
                let revision = self.focus_revision;

                return Task::batch(
                    [Focus::Target, Focus::Start, Focus::ResetIndex]
                        .into_iter()
                        .map(|field| {
                            operation::is_focused(field.id()).map(move |focused| {
                                Message::InputFocused(revision, field.id(), focused)
                            })
                        }),
                );
            }

            Message::InputFocused(revision, id, true) if revision == self.focus_revision => {
                self.focus = [Focus::Target, Focus::Start, Focus::ResetIndex]
                    .into_iter()
                    .find(|field| field.id() == id);
            }

            Message::InputFocused(_, _, _) => {}
            Message::RevealFocus(revision, Some(offset)) if revision == self.focus_revision => {
                return operation::scroll_to(
                    PAGE,
                    operation::AbsoluteOffset {
                        x: None,
                        y: Some(offset),
                    },
                );
            }

            Message::RevealFocus(_, _) => {}
        }

        Task::none()
    }

    fn cancel(&mut self) {
        self.generation += 1;
        self.job = None;
        self.visited_states = 0;
    }

    fn changed(&mut self, field: Focus) {
        self.cancel();
        self.focus = Some(field);
        self.focus_revision += 1;
        self.solution = None;
        self.error = None;
    }

    fn solve(&mut self) -> Task<Message> {
        if self.job.is_some() {
            return Task::none();
        }

        let prepared = match self.form.prepare() {
            Ok(prepared) => prepared,
            Err(validation) => {
                let field = if validation.target.is_some() {
                    Focus::Target
                } else if validation.start.is_some() {
                    Focus::Start
                } else {
                    Focus::ResetIndex
                };

                return self.focus(field);
            }
        };

        self.cancel();
        self.solution = None;
        self.error = None;

        let session = match SearchSession::new(&prepared.counter, &prepared.target) {
            Ok(session) => Arc::new(Mutex::new(session)),
            Err(error) => {
                self.error = Some(error.to_string());

                return Task::none();
            }
        };

        self.job = Some(Job {
            session: Arc::clone(&session),
            prepared,
        });

        search_task(session, self.generation)
    }

    fn focus(&mut self, focus: Focus) -> Task<Message> {
        self.focus = Some(focus);
        self.focus_revision += 1;
        let revision = self.focus_revision;

        // Focusing an ID that is not a text input also unfocuses all text inputs.
        // Buttons receive their keyboard activation and visible ring from this state.
        Task::batch([
            operation::focus(focus.id()),
            focus::reveal(focus.reveal_id())
                .map(move |offset| Message::RevealFocus(revision, offset)),
        ])
    }

    fn focus_order(&self) -> Vec<Focus> {
        let mut controls = vec![Focus::Target, Focus::Start, Focus::ResetIndex];

        if self.form.validate().is_valid() && self.job.is_none() {
            controls.push(Focus::Solve);
        }

        controls.push(Focus::ResetForm);

        if self.solution.is_some() {
            controls.push(Focus::Results);
        }

        controls.push(Focus::Video);

        controls
    }

    fn key_pressed(&mut self, key: Key, modifiers: keyboard::Modifiers) -> Task<Message> {
        match key.as_ref() {
            Key::Named(Named::Tab) => {
                let order = self.focus_order();
                let index = self
                    .focus
                    .and_then(|focus| order.iter().position(|&item| item == focus));
                let next = match (index, modifiers.shift()) {
                    (Some(index), true) => (index + order.len() - 1) % order.len(),
                    (Some(index), false) => (index + 1) % order.len(),
                    (None, true) => order.len() - 1,
                    (None, false) => 0,
                };

                return self.focus(order[next]);
            }

            Key::Named(Named::Enter | Named::Space) => {
                if let Some(message) = self.focus.and_then(Focus::activation) {
                    return self.update(message);
                }
            }

            Key::Named(Named::PageDown | Named::ArrowDown) => {
                return operation::scroll_by(PAGE, operation::AbsoluteOffset { x: 0.0, y: 240.0 });
            }

            Key::Named(Named::PageUp | Named::ArrowUp) => {
                return operation::scroll_by(PAGE, operation::AbsoluteOffset { x: 0.0, y: -240.0 });
            }

            Key::Named(Named::Home) => {
                return operation::snap_to(PAGE, operation::RelativeOffset::START);
            }

            Key::Named(Named::End) => return operation::snap_to_end(PAGE),
            Key::Named(Named::Escape) if self.job.is_some() => {
                return self.update(Message::ResetForm);
            }

            _ => {}
        }

        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        responsive(move |size| {
            let compact = size.width < 620.0;
            let validation = self.form.validate();
            let target = field(
                "Target value", "9876", &self.form.target, Focus::Target,
                Message::TargetChanged, validation.target.as_deref(),
                "One or more digits. Leading zeros count.",
            );
            let start = field(
                "Starting value", "Leave blank for zero", &self.form.start, Focus::Start,
                Message::StartChanged, validation.start.as_deref(),
                "The same number of digits as your target.",
            );
            let reset = field(
                "Reset index", "0", &self.form.reset_index, Focus::ResetIndex,
                Message::ResetIndexChanged, validation.reset_index.as_deref(),
                "One digit, from 0 to 9.",
            );
            let secondary_fields: Element<'_, Message> = if compact {
                column![start, reset].spacing(18).into()
            } else {
                row![container(start).width(Fill), container(reset).width(Fill)].spacing(20).into()
            };

            let solve_focused = self.focus == Some(Focus::Solve);
            let reset_focused = self.focus == Some(Focus::ResetForm);
            let solve = button(
                container(text(if self.job.is_some() { "Finding your sequence…" } else { "Find sequence" }).size(16))
                    .center_x(Fill),
            )
            .padding([14, 20])
            .width(Fill)
            .on_press_maybe((validation.is_valid() && self.job.is_none()).then_some(Message::SolvePressed))
            .style(move |theme, status| focused_button(theme, status, solve_focused, true));
            let reset_button = button(if self.job.is_some() { "Cancel" } else { "Reset form" })
                .padding([14, 20])
                .on_press(Message::ResetForm)
                .style(move |theme, status| focused_button(theme, status, reset_focused, false));
            let solve = container(solve).id(Focus::Solve.id()).width(Fill);
            let reset_button = container(reset_button).id(Focus::ResetForm.id());
            let controls: Element<'_, Message> = if compact {
                column![solve, reset_button.width(Fill)].spacing(10).into()
            } else {
                row![solve, reset_button].spacing(12).into()
            };

            let form = card(column![
                text("Set up your counter").size(22),
                target,
                secondary_fields,
                controls,
                text("Tab to move · Shift + Tab to go back · Enter to solve").size(12).style(text::secondary),
            ].spacing(20), false);

            let header = column![
                text("Tally Puzzle Optimizer").size(if compact { 30 } else { 40 }).center(),
                text("Find the fewest increments with the shortest reset path.")
                    .size(15).style(text::secondary).center(),
            ].spacing(12).align_x(iced::Center).width(Fill);

            let results = if let Some(solution) = &self.solution {
                self.results(solution, compact)
            } else if let Some(error) = &self.error {
                card(column![text("Unable to find a sequence").size(20), text(error).style(text::danger)].spacing(12), false)
            } else if self.job.is_some() {
                card(column![
                    text("Looking for the best path…").size(22),
                    text(format!("{} states explored", self.visited_states)).style(text::secondary),
                    text("You can cancel or change an input at any time.").size(13).style(text::secondary),
                ].spacing(12), false)
            } else {
                card(column![
                    text("Your sequence will appear here").size(22),
                    text("Each step shows the resulting value. Reset moves stay grouped, so they are easy to follow.")
                        .size(14).style(text::secondary),
                ].spacing(12), false)
            };

            let content = column![header, form, results, self.about(),
                text("Made for the tally counter puzzle · Runs entirely in your browser")
                    .size(12).style(text::secondary).center().width(Fill),
            ].spacing(28).max_width(840);

            let page = container(container(content).width(Fill).max_width(840))
                .center_x(Fill)
                .padding(if compact { [28, 16] } else { [52, 32] });

            scrollable(page).id(PAGE).width(Fill).height(Fill).into()
        }).into()
    }

    fn about(&self) -> Element<'_, Message> {
        let video_focused = self.focus == Some(Focus::Video);
        let video = button(text("Inspired by OskarPuzzle’s video").size(14))
            .padding([12, 16])
            .on_press(Message::OpenVideo)
            .style(move |theme, status| focused_button(theme, status, video_focused, false));
        let mut content = column![
            text("About the puzzle").size(22),
            text("Reach a chosen value on a mechanical tally counter using increments and forward or backward turns of the reset knob.")
                .size(14).style(text::secondary),
            text("Increment adds one, carrying across digits and wrapping to zero when the counter is full.")
                .size(14).style(text::secondary),
            text("Forward reset turns the knob forwards and pushes digit wheels along as it passes them. Backward reset moves only the knob.")
                .size(14).style(text::secondary),
            text("The optimizer finds the fewest increments, then the fewest reset ticks among those solutions.")
                .size(14).style(text::secondary),
            container(video).id(Focus::Video.id()),
        ]
        .spacing(14);

        if let Some(error) = &self.video_error {
            content = content.push(text(error).size(13).style(text::danger));
        }

        card(content, false)
    }

    fn results<'a>(&'a self, solution: &'a Solution, compact: bool) -> Element<'a, Message> {
        let stats = [
            stat("Minimum increments", solution.increments.to_string(), false),
            stat("Reset ticks", solution.reset_ticks.to_string(), false),
            stat("Target reached", solution.target.clone(), true),
        ];
        let statistics: Element<'_, Message> = if compact {
            column(stats).spacing(12).into()
        } else {
            row(stats).spacing(12).into()
        };

        let step_count = solution.steps.len();
        let mut steps = column![
            row![
                text("Your sequence").size(24),
                space().width(Fill),
                text(format!(
                    "{step_count} {}",
                    if step_count == 1 { "step" } else { "steps" }
                ))
                .size(13)
                .style(text::secondary)
            ]
            .align_y(iced::Center),
            text("Scroll to follow every step. Page Up / Page Down also work.")
                .size(13)
                .style(text::secondary),
            step_row(
                "Start",
                "Initial state",
                &solution.start,
                solution.initial_reset_index,
                None,
                compact
            ),
        ]
        .spacing(12);

        if solution.steps.is_empty() {
            steps = steps.push(
                text("You are already at your target. No moves needed.").style(text::success),
            );
        }

        for (index, step) in solution.steps.iter().enumerate() {
            if step.starts_reset_group {
                steps = steps.push(text("RESET MOVES").size(11).style(text::secondary));
            }

            steps = steps.push(solution_step(index + 1, step, compact));
        }

        column![
            statistics,
            container(card(steps, self.focus == Some(Focus::Results))).id(Focus::Results.id()),
        ]
        .spacing(20)
        .into()
    }
}

fn search_task(session: Arc<Mutex<SearchSession>>, generation: u64) -> Task<Message> {
    Task::perform(
        async move {
            // An async function alone cannot yield CPU work on a browser's main thread.
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(0).await;

            session
                .lock()
                .map_err(|_| SearchError::AllocationFailed)?
                .advance(SEARCH_BATCH)
        },
        move |progress| Message::Progress(generation, progress),
    )
}

#[allow(clippy::too_many_arguments)]
fn field<'a>(
    label: &'static str,
    placeholder: &'static str,
    value: &'a str,
    focus: Focus,
    on_input: fn(String) -> Message,
    error: Option<&str>,
    hint: &'static str,
) -> Element<'a, Message> {
    let invalid = error.is_some();
    let input = text_input(placeholder, value)
        .id(focus.id())
        .on_input(on_input)
        .on_submit(Message::Solve)
        .size(20)
        .padding([12, 14])
        .style(move |theme, status| {
            let mut style = text_input::default(theme, status);

            if invalid {
                style.border.color = theme.palette().danger;
            }

            style.border.radius = 8.0.into();

            style
        });

    let help = focus.help().unwrap_or_default();
    let help_icon = container(text("?").size(14).style(text::secondary))
        .center_x(16)
        .center_y(20)
        .style(container::rounded_box);
    let help_tooltip = tooltip(
        help_icon,
        container(text(help).size(13)).width(280),
        tooltip::Position::Top,
    )
    .gap(8)
    .padding(10)
    .snap_within_viewport(true)
    .style(container::rounded_box);
    let content = column![
        row![text(label).size(14), help_tooltip]
            .spacing(2)
            .align_y(iced::Center),
        input,
        text(error.unwrap_or(hint).to_owned())
            .size(12)
            .style(if invalid {
                text::danger
            } else {
                text::secondary
            }),
    ]
    .spacing(8);

    container(content).id(focus.reveal_id()).width(Fill).into()
}

fn card<'a>(content: impl Into<Element<'a, Message>>, focused: bool) -> Element<'a, Message> {
    container(content)
        .padding(24)
        .width(Fill)
        .style(move |theme: &Theme| {
            let palette = theme.extended_palette();

            container::Style {
                background: Some(palette.background.weak.color.into()),
                border: Border {
                    color: if focused {
                        palette.primary.base.color
                    } else {
                        palette.background.strong.color
                    },
                    width: if focused { 2.0 } else { 1.0 },
                    radius: 16.0.into(),
                },
                ..container::Style::default()
            }
        })
        .into()
}

fn focused_button(
    theme: &Theme,
    status: button::Status,
    focused: bool,
    primary: bool,
) -> button::Style {
    let mut style = if primary {
        button::primary(theme, status)
    } else {
        button::secondary(theme, status)
    };
    style.border.radius = 8.0.into();

    if focused {
        style.border.color = theme.palette().text;
        style.border.width = 2.0;
    }

    style
}

fn stat<'a>(label: &'static str, value: String, success: bool) -> Element<'a, Message> {
    card(
        column![
            text(label).size(13).style(text::secondary),
            text(value).size(32).style(if success {
                text::success
            } else {
                text::default
            }),
        ]
        .spacing(10),
        false,
    )
}

fn solution_step(number: usize, step: &Step, compact: bool) -> Element<'_, Message> {
    let (label, ticks, accent) = match step.action {
        Action::Increment(ticks) => ("Increment", ticks, Theme::CatppuccinMocha.palette().primary),
        Action::ResetForward(ticks) => ("Forward", ticks, Theme::CatppuccinMocha.palette().success),
        Action::ResetBackward(ticks) => ("Backward", ticks, Color::from_rgb8(0xf9, 0xe2, 0xaf)),
    };

    step_row(
        &format!("{number:02}"),
        &format!("{label} ×{ticks}"),
        &step.value,
        step.reset_index,
        Some(accent),
        compact,
    )
}

fn step_row<'a>(
    number: &str,
    action: &str,
    value: &'a str,
    reset_index: u8,
    accent: Option<Color>,
    compact: bool,
) -> Element<'a, Message> {
    let description = row![
        text(number.to_owned())
            .size(12)
            .style(text::secondary)
            .width(40),
        text(action.to_owned())
            .size(16)
            .color(accent.unwrap_or(Theme::CatppuccinMocha.palette().text)),
    ]
    .spacing(8)
    .align_y(iced::Center);
    let value = row![
        text(value).size(22),
        text(format!("Reset index {reset_index}"))
            .size(12)
            .style(text::secondary),
    ]
    .spacing(18)
    .align_y(iced::Center);
    let content: Element<'_, Message> = if compact {
        column![description, value].spacing(10).into()
    } else {
        row![description, space().width(Fill), value]
            .align_y(iced::Center)
            .into()
    };

    container(content)
        .padding([14, 16])
        .width(Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(theme.extended_palette().background.base.color.into()),
            border: Border {
                radius: 10.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_visits_fields_and_buttons_and_shift_tab_reverses() {
        let (mut planner, _) = Planner::new();

        for expected in [
            Focus::Start,
            Focus::ResetIndex,
            Focus::Solve,
            Focus::ResetForm,
            Focus::Video,
            Focus::Target,
        ] {
            let _ = planner.key_pressed(Key::Named(Named::Tab), keyboard::Modifiers::empty());

            assert_eq!(planner.focus, Some(expected));
        }

        let _ = planner.key_pressed(Key::Named(Named::Tab), keyboard::Modifiers::SHIFT);

        assert_eq!(planner.focus, Some(Focus::Video));
    }

    #[test]
    fn keyboard_navigation_reaches_results_then_video() {
        let (mut planner, _) = Planner::new();
        planner.form.target = "00".to_owned();
        planner.solution = Some(
            planner
                .form
                .prepare()
                .unwrap()
                .finish(tally_problem::SearchResult::Found(Vec::new()))
                .unwrap(),
        );
        planner.focus = Some(Focus::ResetForm);

        let _ = planner.key_pressed(Key::Named(Named::Tab), keyboard::Modifiers::empty());

        assert_eq!(planner.focus, Some(Focus::Results));

        let _ = planner.key_pressed(Key::Named(Named::Tab), keyboard::Modifiers::empty());

        assert_eq!(planner.focus, Some(Focus::Video));
        assert!(matches!(
            Focus::Video.activation(),
            Some(Message::OpenVideo)
        ));

        let _ = planner.key_pressed(Key::Named(Named::Tab), keyboard::Modifiers::empty());

        assert_eq!(planner.focus, Some(Focus::Target));
    }

    #[test]
    fn invalid_input_skips_solve_and_enter_focuses_first_error() {
        let (mut planner, _) = Planner::new();
        planner.form.target = "abc".to_owned();
        planner.focus = Some(Focus::ResetIndex);

        let _ = planner.key_pressed(Key::Named(Named::Tab), keyboard::Modifiers::empty());

        assert_eq!(planner.focus, Some(Focus::ResetForm));

        let _ = planner.solve();

        assert_eq!(planner.focus, Some(Focus::Target));
        assert!(planner.job.is_none());
    }

    #[test]
    fn delayed_focus_queries_and_reveals_do_not_override_newer_navigation() {
        let (mut planner, _) = Planner::new();
        let _ = planner.update(Message::PointerPressed);
        let pointer_revision = planner.focus_revision;
        let _ = planner.update(Message::InputFocused(
            pointer_revision,
            Focus::Start.id(),
            true,
        ));

        assert_eq!(planner.focus, Some(Focus::Start));

        let _ = planner.key_pressed(Key::Named(Named::Tab), keyboard::Modifiers::empty());
        let keyboard_revision = planner.focus_revision;
        let _ = planner.update(Message::InputFocused(
            pointer_revision,
            Focus::Target.id(),
            true,
        ));
        let stale_reveal = planner.update(Message::RevealFocus(pointer_revision, Some(320.0)));
        let current_reveal = planner.update(Message::RevealFocus(keyboard_revision, Some(320.0)));

        assert_eq!(planner.focus, Some(Focus::ResetIndex));
        assert_eq!(stale_reveal.units(), 0);
        assert_eq!(current_reveal.units(), 1);

        let _ = planner.update(Message::PointerPressed);
        let _ = planner.update(Message::InputFocused(
            keyboard_revision,
            Focus::Start.id(),
            true,
        ));

        assert_eq!(planner.focus, None);

        let _ = planner.update(Message::TargetChanged("12".to_owned()));
        let _ = planner.update(Message::InputFocused(
            pointer_revision,
            Focus::Start.id(),
            true,
        ));

        assert_eq!(planner.focus, Some(Focus::Target));
    }

    #[test]
    fn changing_inputs_discards_results_and_stale_search_messages() {
        let (mut planner, _) = Planner::new();
        let _ = planner.solve();
        let generation = planner.generation;
        let _ = planner.update(Message::TargetChanged("12".to_owned()));
        let _ = planner.update(Message::Progress(
            generation,
            Ok(SearchProgress::Complete(
                tally_problem::SearchResult::Found(Vec::new()),
            )),
        ));

        assert!(planner.job.is_none());
        assert!(planner.solution.is_none());
        assert_eq!(planner.form.target, "12");
    }
}
