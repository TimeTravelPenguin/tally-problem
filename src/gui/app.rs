use std::time::Duration;

use iced::keyboard::{self, Key, key::Named};
use iced::time::Instant;
use iced::widget::{
    button, column, container, operation, progress_bar, responsive, row, scrollable, sensor, space,
    text, text_input, tooltip,
};
use iced::{Border, Color, Element, Event, Fill, Length, Subscription, Task, Theme, event, mouse};
use tally_problem::{Action, SearchProgress, SearchStatistics};

use crate::focus;
use crate::model::{Form, PreparedSearch, Solution, Step};
use crate::progress::{ProgressEstimate, TimingPrediction};
use crate::solver;

const PAGE: &str = "planner-page";
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
                "The value to reach, using one or more decimal digits. \
                Leading zeros set the counter width: 0012 uses four digits. \
                Larger counters may take longer to solve.",
            ),
            Self::Start => Some(
                "The counter's current digits. Leave blank to start at zero, \
                or enter the same number of digits as the target, including leading zeros.",
            ),
            Self::ResetIndex => Some(
                "The current knob position (0–9). It determines which digits the next forward \
                reset pushes; it is not the number of reset ticks.",
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
    _control: solver::Control,
    prepared: PreparedSearch,
    started_at: Instant,
    elapsed: Duration,
    estimate: ProgressEstimate,
    statistics: SearchStatistics,
    reported_at: Option<Instant>,
}

impl Job {
    fn timing_prediction(&self) -> Option<TimingPrediction> {
        // A long batch can outlive the prediction from its preceding report.
        if self.reported_at?.elapsed() > Duration::from_secs(2) {
            return None;
        }

        self.estimate.prediction()
    }
}

pub struct Planner {
    form: Form,
    focus: Option<Focus>,
    focus_revision: u64,
    job: Option<Job>,
    generation: u64,
    visited_states: usize,
    completed_elapsed: Option<Duration>,
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
    Progress(u64, Result<solver::Update, String>),
    SearchFrame(u64, Instant),
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
                completed_elapsed: None,
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
                    Ok(solver::Update {
                        progress: SearchProgress::InProgress { visited_states },
                        statistics,
                        elapsed,
                    }) => {
                        self.visited_states = visited_states;

                        if let Some(job) = &mut self.job {
                            job.statistics = statistics;
                            job.reported_at = Some(Instant::now());
                            job.estimate.update(statistics, elapsed);
                        }
                    }

                    Ok(solver::Update {
                        progress: SearchProgress::Complete(result),
                        ..
                    }) => {
                        if let Some(job) = self.job.take() {
                            let elapsed = job.elapsed.max(job.started_at.elapsed());

                            match job.prepared.finish(result) {
                                Ok(solution) => {
                                    self.solution = Some(solution);
                                    self.completed_elapsed = Some(elapsed);
                                }

                                Err(error) => self.error = Some(error),
                            }
                        }
                    }

                    Err(error) => {
                        self.job = None;
                        self.error = Some(error);
                    }
                }
            }

            Message::SearchFrame(generation, now) => {
                if generation == self.generation
                    && let Some(job) = &mut self.job
                {
                    job.elapsed = job
                        .elapsed
                        .max(now.saturating_duration_since(job.started_at));
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
        self.completed_elapsed = None;
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

        let started_at = Instant::now();
        let estimate = ProgressEstimate::new(&prepared.counter, &prepared.target);
        let (control, task) = solver::start(prepared.counter.clone(), prepared.target.clone());

        self.job = Some(Job {
            _control: control,
            prepared,
            started_at,
            elapsed: Duration::ZERO,
            estimate,
            statistics: SearchStatistics::default(),
            reported_at: None,
        });

        let generation = self.generation;

        task.map(move |progress| Message::Progress(generation, progress))
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
            } else if let Some(job) = &self.job {
                self.searching(job, compact)
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

    fn searching(&self, job: &Job, compact: bool) -> Element<'_, Message> {
        let generation = self.generation;
        let prediction = job.timing_prediction();
        let interval = if prediction.is_some() { 1_000 } else { 50 };
        // The sensor's redraw timer keeps elapsed time and fallback activity
        // independent of slow solver batches, on both desktop and browser.
        let elapsed = sensor(
            text(format!("Elapsed {}", elapsed_label(job.elapsed)))
                .size(14)
                .style(text::secondary),
        )
        .key((generation, job.elapsed.as_millis() / interval, interval))
        .delay(Duration::from_millis(interval as u64))
        .on_show(move |_| Message::SearchFrame(generation, Instant::now()));
        let estimate_label = prediction.map_or_else(
            || "Timing estimate uncertain".to_owned(),
            |prediction| format!("Estimated {:.0}%", prediction.percent),
        );
        let estimate = tooltip(
            text(estimate_label).size(14).style(text::secondary),
            container(
                text(
                    "Timing estimates adapt to measured diagram work and search timings. \
                    The percentage can move backwards as more work is discovered. \
                    When the prediction is uncertain, the bar shows activity instead. \
                    100% means the search has finished.",
                )
                .size(13),
            )
            .width(280),
            tooltip::Position::Top,
        )
        .gap(8)
        .padding(10)
        .snap_within_viewport(true)
        .style(container::rounded_box);
        let timing: Element<'_, Message> = if compact {
            column![elapsed, estimate].spacing(6).into()
        } else {
            row![elapsed, space().width(Fill), estimate].into()
        };

        let bar = prediction.map_or_else(
            || activity_bar(job.elapsed),
            |prediction| search_bar(prediction.percent, false),
        );
        let milestone = if job.reported_at.is_some() {
            let increments = job.statistics.increment_layer;

            format!(
                "Checking paths with {increments} {}",
                if increments == 1 {
                    "increment"
                } else {
                    "increments"
                },
            )
        } else {
            "Preparing the search…".to_owned()
        };
        let mut content = column![
            text("Looking for the best path…").size(22),
            bar,
            timing,
            text(milestone).size(14),
            text(format!("{} state groups explored", self.visited_states)).style(text::secondary),
        ]
        .spacing(12);

        if let Some(prediction) = prediction {
            content = content.push(
                text(format!(
                    "Estimated remaining {}",
                    elapsed_label(prediction.remaining.max(Duration::from_secs(1))),
                ))
                .size(13)
                .style(text::secondary),
            );
        }

        card(
            content.push(
                text("You can cancel or change an input at any time.")
                    .size(13)
                    .style(text::secondary),
            ),
            false,
        )
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

        let mut results = column![].spacing(20);

        if let Some(elapsed) = self.completed_elapsed {
            results = results.push(
                column![
                    row![
                        text("Complete · 100%").size(14).style(text::success),
                        space().width(Fill),
                        text(format!("Elapsed {}", elapsed_label(elapsed)))
                            .size(14)
                            .style(text::secondary),
                    ],
                    search_bar(100.0, true),
                ]
                .spacing(8),
            );
        }

        results
            .push(statistics)
            .push(
                container(card(steps, self.focus == Some(Focus::Results))).id(Focus::Results.id()),
            )
            .into()
    }
}

fn elapsed_label(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    let minutes = seconds / 60;

    if minutes < 60 {
        format!("{minutes}:{:02}", seconds % 60)
    } else {
        format!("{}:{:02}:{:02}", minutes / 60, minutes % 60, seconds % 60)
    }
}

fn search_bar(percent: f32, completed: bool) -> Element<'static, Message> {
    progress_bar(0.0..=100.0, percent)
        .length(Fill)
        .girth(8)
        .style(move |theme: &Theme| {
            let palette = theme.extended_palette();

            progress_bar::Style {
                background: palette.background.strong.color.into(),
                bar: if completed {
                    palette.success.base.color
                } else {
                    palette.primary.base.color
                }
                .into(),
                border: Border {
                    radius: 4.0.into(),
                    ..Border::default()
                },
            }
        })
        .into()
}

fn activity_bar(elapsed: Duration) -> Element<'static, Message> {
    let phase = (elapsed.as_secs_f64() / 2.4).fract();
    let position = 1.0 - (2.0 * phase - 1.0).abs();
    let leading = (position * 800.0).round() as u16;
    let trailing = 800 - leading;
    let mut bar = row![];

    if leading > 0 {
        bar = bar.push(space().width(Length::FillPortion(leading)));
    }

    bar = bar.push(
        container(space())
            .width(Length::FillPortion(200))
            .height(8)
            .style(|theme: &Theme| container::Style {
                background: Some(theme.extended_palette().primary.base.color.into()),
                border: Border {
                    radius: 4.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            }),
    );

    if trailing > 0 {
        bar = bar.push(space().width(Length::FillPortion(trailing)));
    }

    container(bar)
        .width(Fill)
        .height(8)
        .style(|theme: &Theme| container::Style {
            background: Some(theme.extended_palette().background.strong.color.into()),
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
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

    fn report(progress: SearchProgress) -> solver::Update {
        let visited_groups = match &progress {
            SearchProgress::InProgress { visited_states } => *visited_states,
            SearchProgress::Complete(_) => 0,
        };

        solver::Update {
            progress,
            statistics: SearchStatistics {
                visited_groups,
                increment_layer: 2,
                diagram_nodes: 1,
                diagram_work: visited_groups as u64,
                queued_groups: 1,
                ..SearchStatistics::default()
            },
            elapsed: Duration::ZERO,
        }
    }

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
            Ok(report(SearchProgress::Complete(
                tally_problem::SearchResult::Found(Vec::new()),
            ))),
        ));

        assert!(planner.job.is_none());
        assert!(planner.solution.is_none());
        assert_eq!(planner.form.target, "12");
    }

    #[test]
    fn elapsed_clock_uses_frame_time_without_solver_reports_and_never_moves_backwards() {
        let (mut planner, _) = Planner::new();
        let _ = planner.solve();
        let generation = planner.generation;
        let started_at = planner.job.as_ref().unwrap().started_at;
        let elapsed = Duration::from_secs(65);

        let _ = planner.update(Message::SearchFrame(generation, started_at + elapsed));

        assert_eq!(planner.job.as_ref().unwrap().elapsed, elapsed);
        assert_eq!(planner.visited_states, 0);

        let _ = planner.update(Message::SearchFrame(
            generation,
            started_at + Duration::from_secs(1),
        ));
        let _ = planner.update(Message::Progress(
            generation,
            Ok(report(SearchProgress::InProgress {
                visited_states: 256,
            })),
        ));

        assert_eq!(planner.job.as_ref().unwrap().elapsed, elapsed);
        assert_eq!(planner.visited_states, 256);
    }

    #[test]
    fn restarting_resets_clock_and_old_frames_cannot_update_new_or_finished_jobs() {
        let (mut planner, _) = Planner::new();
        let _ = planner.solve();
        let old_generation = planner.generation;
        let old_start = planner.job.as_ref().unwrap().started_at;
        let old_frame = old_start + Duration::from_secs(120);
        let _ = planner.update(Message::SearchFrame(old_generation, old_frame));
        let _ = planner.update(Message::ResetForm);
        let _ = planner.update(Message::SearchFrame(old_generation, old_frame));

        assert!(planner.job.is_none());

        let _ = planner.solve();
        let generation = planner.generation;
        let _ = planner.update(Message::SearchFrame(old_generation, old_frame));

        assert_eq!(planner.job.as_ref().unwrap().elapsed, Duration::ZERO);
        assert!(
            planner
                .job
                .as_ref()
                .unwrap()
                .estimate
                .prediction()
                .is_none()
        );

        let _ = planner.update(Message::Progress(
            generation,
            Ok(report(SearchProgress::Complete(
                tally_problem::SearchResult::NotFound,
            ))),
        ));
        let _ = planner.update(Message::SearchFrame(generation, old_frame));

        assert!(planner.job.is_none());
        assert!(planner.completed_elapsed.is_none());
        assert_eq!(
            planner.error.as_deref(),
            Some("This target is unreachable.")
        );
    }

    #[test]
    fn uncertain_timing_uses_search_milestones_and_only_completion_finishes_the_bar() {
        let (mut planner, _) = Planner::new();
        planner.form.target = "12".to_owned();
        let _ = planner.solve();
        let generation = planner.generation;
        let started_at = planner.job.as_ref().unwrap().started_at;
        let elapsed = Duration::from_secs(120);
        let _ = planner.update(Message::SearchFrame(generation, started_at + elapsed));

        assert!(
            planner
                .job
                .as_ref()
                .unwrap()
                .estimate
                .prediction()
                .is_none()
        );

        let _ = planner.update(Message::Progress(
            generation,
            Ok(report(SearchProgress::InProgress { visited_states: 32 })),
        ));
        assert_eq!(planner.job.as_ref().unwrap().statistics.increment_layer, 2);
        assert!(
            planner
                .job
                .as_ref()
                .unwrap()
                .estimate
                .prediction()
                .is_none()
        );
        assert!(planner.completed_elapsed.is_none());

        let _ = planner.update(Message::Progress(
            generation,
            Ok(report(SearchProgress::InProgress {
                visited_states: usize::MAX,
            })),
        ));

        assert!(
            planner
                .job
                .as_ref()
                .unwrap()
                .estimate
                .prediction()
                .is_none_or(|prediction| prediction.percent <= 99.0)
        );
        assert!(planner.solution.is_none());

        let _ = planner.update(Message::Progress(
            generation,
            Ok(report(SearchProgress::Complete(
                tally_problem::SearchResult::Found(vec![
                    Action::ResetForward(1),
                    Action::Increment(1),
                ]),
            ))),
        ));

        assert!(planner.job.is_none());
        assert!(planner.solution.is_some());
        assert_eq!(planner.completed_elapsed, Some(elapsed));

        let _ = planner.update(Message::TargetChanged("13".to_owned()));

        assert!(planner.completed_elapsed.is_none());
        assert!(planner.solution.is_none());
    }

    #[test]
    fn stale_reports_cannot_set_a_restarted_jobs_search_milestone() {
        let (mut planner, _) = Planner::new();
        let _ = planner.solve();
        let old_generation = planner.generation;
        let _ = planner.update(Message::ResetForm);
        let _ = planner.solve();
        let _ = planner.update(Message::Progress(
            old_generation,
            Ok(report(SearchProgress::InProgress {
                visited_states: 256,
            })),
        ));
        let job = planner.job.as_ref().unwrap();

        assert_eq!(job.statistics, SearchStatistics::default());
        assert!(job.reported_at.is_none());
        assert!(job.timing_prediction().is_none());
        assert_eq!(planner.visited_states, 0);
    }

    #[test]
    fn failed_search_cannot_show_completed_progress() {
        let (mut planner, _) = Planner::new();
        let _ = planner.solve();
        let generation = planner.generation;
        let _ = planner.update(Message::Progress(
            generation,
            Err("Search stopped".to_owned()),
        ));

        assert!(planner.job.is_none());
        assert!(planner.solution.is_none());
        assert!(planner.completed_elapsed.is_none());
    }

    #[test]
    fn elapsed_labels_handle_minutes_and_hours() {
        for (seconds, expected) in [
            (0, "0:00"),
            (59, "0:59"),
            (60, "1:00"),
            (3_599, "59:59"),
            (3_600, "1:00:00"),
            (3_661, "1:01:01"),
        ] {
            assert_eq!(elapsed_label(Duration::from_secs(seconds)), expected);
        }
    }
}
