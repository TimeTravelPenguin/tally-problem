//! Exact replay state and scheduling for the solution player.
//!
//! Grouped solver actions stay grouped in memory. Each forward move applies one
//! real counter tick, then exposes the old and new states for animation. Moving
//! backward rebuilds the preceding position from the starting counter because a
//! reset can destroy information and cannot generally be inverted.
//!
//! Counter state changes immediately; animation only interpolates its display.
//! The caller drives [`Playback::advance`] and redraws according to
//! [`Playback::next_frame_delay`]. Rendering belongs to `playback_view`.

use std::time::Duration;

use iced::time::Instant;
use tally_problem::{Action, TallyCounter};

use crate::model::Solution;

const DEFAULT_SPEED: f32 = 3.0;
const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const MAX_TRANSITION: Duration = Duration::from_millis(180);

/// Visual wheel motion, independent of the numerical distance between digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RollDirection {
    /// The old numeral leaves upward and the new numeral enters from below.
    Forward,
    /// The old numeral leaves downward and the new numeral enters from above.
    Backward,
    /// Keep the current numeral centered without rolling.
    Still,
}

/// A borrowed presentation snapshot for the latest single-tick transition.
///
/// Digits and reset indices are exact counter states, rather than interpolated
/// values. The renderer uses `progress` and the separate wheel directions to
/// animate between them. Once settled, both states describe the same position.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Frame<'a> {
    pub(crate) previous_digits: &'a [u8],
    pub(crate) current_digits: &'a [u8],
    pub(crate) previous_reset_index: u8,
    pub(crate) reset_index: u8,
    /// Smoothstep-eased animation progress, bounded to `0.0..=1.0`.
    pub(crate) progress: f32,
    pub(crate) digit_direction: RollDirection,
    pub(crate) reset_direction: RollDirection,
    pub(crate) previous_instruction: &'a str,
    pub(crate) instruction: &'a str,
    pub(crate) completed_ticks: u64,
    pub(crate) total_ticks: u64,
    /// Index of the most recently performed instruction, or `None` at the start.
    pub(crate) active_step: Option<usize>,
    pub(crate) playing: bool,
}

/// Replays grouped operations one tick at a time without expanding the sequence.
///
/// All methods receiving a solution must use the same solution passed to
/// [`Self::new`], unless [`Self::restart`] replaces the playback state. Playback
/// owns the current counter and one previous snapshot, not a history of ticks.
#[derive(Debug)]
pub(crate) struct Playback {
    counter: TallyCounter,
    previous_digits: Vec<u8>,
    previous_reset_index: u8,
    instruction: String,
    previous_instruction: String,
    /// Instruction containing the next tick; may equal the sequence length.
    step_index: usize,
    /// Ticks already applied within `step_index`; zero at a group boundary.
    ticks_in_step: u64,
    step_count: usize,
    active_step: Option<usize>,
    completed_ticks: u64,
    total_ticks: u64,
    playing: bool,
    speed: f32,
    next_tick_at: Option<Instant>,
    transition_started_at: Option<Instant>,
    transition_duration: Duration,
    digit_direction: RollDirection,
    reset_direction: RollDirection,
}

impl Playback {
    /// Start paused at the validated initial state, skipping zero-tick actions.
    pub(crate) fn new(solution: &Solution) -> Self {
        let counter = initial_counter(solution);
        let total_ticks = solution.steps.iter().fold(0_u64, |acc, step| {
            acc.saturating_add(action_ticks(step.action))
        });
        let mut playback = Self {
            previous_digits: counter.values().to_vec(),
            previous_reset_index: counter.reset_index(),
            counter,
            instruction: "Initial state".to_owned(),
            previous_instruction: "Initial state".to_owned(),
            step_index: 0,
            ticks_in_step: 0,
            step_count: solution.steps.len(),
            active_step: None,
            completed_ticks: 0,
            total_ticks,
            playing: false,
            speed: DEFAULT_SPEED,
            next_tick_at: None,
            transition_started_at: None,
            transition_duration: MAX_TRANSITION,
            digit_direction: RollDirection::Still,
            reset_direction: RollDirection::Still,
        };

        playback.skip_empty_steps(solution);

        playback
    }

    /// Borrow exact states and evaluate animation progress without advancing.
    pub(crate) fn frame(&self, now: Instant) -> Frame<'_> {
        let linear_progress = self.transition_started_at.map_or(1.0, |started_at| {
            (now.saturating_duration_since(started_at).as_secs_f32()
                / self.transition_duration.as_secs_f32())
            .clamp(0.0, 1.0)
        });
        let progress = linear_progress * linear_progress * (3.0 - 2.0 * linear_progress);

        Frame {
            previous_digits: &self.previous_digits,
            current_digits: self.counter.values(),
            previous_reset_index: self.previous_reset_index,
            reset_index: self.counter.reset_index(),
            progress,
            digit_direction: self.digit_direction,
            reset_direction: self.reset_direction,
            previous_instruction: &self.previous_instruction,
            instruction: &self.instruction,
            completed_ticks: self.completed_ticks,
            total_ticks: self.total_ticks,
            active_step: self.active_step,
            playing: self.playing,
        }
    }

    pub(crate) fn is_playing(&self) -> bool {
        self.playing
    }

    /// Whether every tick has been applied, even if its final animation remains.
    pub(crate) fn is_finished(&self) -> bool {
        self.step_index >= self.step_count
    }

    pub(crate) fn can_previous(&self) -> bool {
        self.active_step.is_some()
    }

    pub(crate) fn can_next(&self) -> bool {
        !self.is_finished()
    }

    /// Automatic playback rate in ticks per second.
    pub(crate) fn speed(&self) -> f32 {
        self.speed
    }

    /// Settle the display and schedule the first tick one interval from `now`.
    ///
    /// Playing a finished sequence restarts it while retaining the chosen speed.
    pub(crate) fn play(&mut self, solution: &Solution, now: Instant) {
        if self.is_finished() {
            self.restart(solution);
        }

        self.settle();

        if self.can_next() {
            self.playing = true;
            self.next_tick_at = Some(now + self.tick_interval());
        }
    }

    /// Pause on the exact state already reached by the current tick.
    pub(crate) fn pause(&mut self) {
        self.playing = false;
        self.next_tick_at = None;
        self.settle();
    }

    /// Replace the replay with a paused initial state, retaining playback speed.
    pub(crate) fn restart(&mut self, solution: &Solution) {
        let speed = self.speed;
        *self = Self::new(solution);
        self.speed = speed;
    }

    /// Set a finite rate clamped to 1–8 ticks per second and settle animation.
    ///
    /// If playing, the next tick is rescheduled from `now` at the new rate.
    pub(crate) fn set_speed(&mut self, speed: f32, now: Instant) {
        if !speed.is_finite() {
            return;
        }

        self.speed = speed.clamp(1.0, 8.0);
        self.settle();

        if self.playing {
            self.next_tick_at = Some(now + self.tick_interval());
        }
    }

    /// Manually move one tick forward and leave automatic playback paused.
    pub(crate) fn next(&mut self, solution: &Solution, now: Instant) {
        self.pause();
        self.perform_tick(solution, now);
    }

    /// Rebuild the preceding state with grouped operations, rather than assuming
    /// resets have an inverse. Work depends on sequence length, not tick count.
    pub(crate) fn previous(&mut self, solution: &Solution, now: Instant) {
        self.pause();

        if !self.can_previous() {
            return;
        }

        let undone_step = self.active_step.unwrap();
        let undone_action = solution.steps[undone_step].action;
        self.begin_transition(now);

        if self.ticks_in_step == 0 {
            self.step_index = (0..self.step_index)
                .rev()
                .find(|&idx| action_ticks(solution.steps[idx].action) > 0)
                .expect("A previous instruction exists");
            self.ticks_in_step = action_ticks(solution.steps[self.step_index].action);
        }

        self.ticks_in_step -= 1;
        self.rebuild_position(solution);
        self.set_directions(undone_action, true);
    }

    /// A delayed or background frame advances at most one tick. It schedules the
    /// next tick from this frame, so returning to the player never causes a burst.
    pub(crate) fn advance(&mut self, solution: &Solution, now: Instant) {
        if !self.playing || self.next_tick_at.is_none_or(|deadline| now < deadline) {
            return;
        }

        self.perform_tick(solution, now);

        if self.playing {
            self.next_tick_at = Some(now + self.tick_interval());
        }
    }

    /// Delay until the next animation redraw or automatic tick, if either exists.
    ///
    /// Transitions request frames about every 16 ms; idle paused playback has no
    /// deadline. A due tick returns a zero delay rather than advancing here.
    pub(crate) fn next_frame_delay(&self, now: Instant) -> Option<Duration> {
        if let Some(started_at) = self.transition_started_at {
            let remaining = self
                .transition_duration
                .saturating_sub(now.saturating_duration_since(started_at));

            if remaining > Duration::ZERO {
                return Some(FRAME_INTERVAL.min(remaining));
            }
        }

        self.next_tick_at
            .filter(|_| self.playing)
            .map(|deadline| deadline.saturating_duration_since(now))
    }

    fn tick_interval(&self) -> Duration {
        Duration::from_secs_f32(1.0 / self.speed)
    }

    fn skip_empty_steps(&mut self, solution: &Solution) {
        while self.step_index < solution.steps.len()
            && action_ticks(solution.steps[self.step_index].action) == 0
        {
            self.step_index += 1;
        }
    }

    fn perform_tick(&mut self, solution: &Solution, now: Instant) {
        if !self.can_next() {
            return;
        }

        let action = solution.steps[self.step_index].action;
        self.begin_transition(now);
        with_ticks(action, 1).apply(&mut self.counter);
        self.ticks_in_step += 1;
        self.completed_ticks = self.completed_ticks.saturating_add(1);
        self.active_step = Some(self.step_index);
        self.instruction = instruction(action, self.ticks_in_step);
        self.set_directions(action, false);

        if self.ticks_in_step == action_ticks(action) {
            self.step_index += 1;
            self.ticks_in_step = 0;
            self.skip_empty_steps(solution);
        }

        if self.is_finished() {
            self.playing = false;
            self.next_tick_at = None;
        }
    }

    /// Apply whole completed groups and the partial current group from scratch.
    fn rebuild_position(&mut self, solution: &Solution) {
        let mut counter = initial_counter(solution);
        let mut completed_ticks = 0_u64;

        for step in &solution.steps[..self.step_index] {
            step.action.apply(&mut counter);
            completed_ticks = completed_ticks.saturating_add(action_ticks(step.action));
        }

        if self.ticks_in_step > 0 {
            with_ticks(solution.steps[self.step_index].action, self.ticks_in_step)
                .apply(&mut counter);
        }

        self.counter = counter;
        self.completed_ticks = completed_ticks.saturating_add(self.ticks_in_step);
        let last_step = if self.ticks_in_step > 0 {
            Some((self.step_index, self.ticks_in_step))
        } else {
            (0..self.step_index).rev().find_map(|idx| {
                let ticks = action_ticks(solution.steps[idx].action);

                (ticks > 0).then_some((idx, ticks))
            })
        };

        if let Some((idx, ticks)) = last_step {
            self.active_step = Some(idx);
            self.instruction = instruction(solution.steps[idx].action, ticks);
        } else {
            self.active_step = None;
            self.instruction = "Initial state".to_owned();
        }
    }

    /// Capture exact starting values before mutation and bound animation length.
    ///
    /// A transition takes at most 180 ms or 72% of a tick interval, leaving it
    /// time to finish before the next automatically scheduled operation.
    fn begin_transition(&mut self, now: Instant) {
        self.previous_digits.copy_from_slice(self.counter.values());
        self.previous_reset_index = self.counter.reset_index();
        self.previous_instruction.clone_from(&self.instruction);
        self.transition_started_at = Some(now);
        self.transition_duration = MAX_TRANSITION.min(self.tick_interval().mul_f32(0.72));
    }

    /// Choose visual directions for replay or rewind, keeping unchanged digits still.
    fn set_directions(&mut self, action: Action, reverse: bool) {
        let forward = if reverse {
            RollDirection::Backward
        } else {
            RollDirection::Forward
        };
        let backward = if reverse {
            RollDirection::Forward
        } else {
            RollDirection::Backward
        };

        self.digit_direction = if self.previous_digits == self.counter.values() {
            RollDirection::Still
        } else {
            forward
        };
        self.reset_direction = match action {
            Action::Increment(_) => RollDirection::Still,
            Action::ResetForward(_) => forward,
            Action::ResetBackward(_) => backward,
        };
    }

    fn settle(&mut self) {
        self.previous_digits.copy_from_slice(self.counter.values());
        self.previous_reset_index = self.counter.reset_index();
        self.previous_instruction.clone_from(&self.instruction);
        self.transition_started_at = None;
        self.digit_direction = RollDirection::Still;
        self.reset_direction = RollDirection::Still;
    }
}

/// Restore a counter from the validated starting digits and reset index.
fn initial_counter(solution: &Solution) -> TallyCounter {
    let mut counter = TallyCounter::new(solution.start.len())
        .expect("A solution retains a validated counter width");
    counter
        .set_values(solution.start.bytes().map(|digit| digit - b'0').collect())
        .expect("A solution retains validated starting digits");
    counter
        .set_reset_index(solution.initial_reset_index)
        .expect("A solution retains a validated reset index");

    counter
}

fn action_ticks(action: Action) -> u64 {
    match action {
        Action::Increment(ticks) | Action::ResetForward(ticks) | Action::ResetBackward(ticks) => {
            ticks
        }
    }
}

fn with_ticks(action: Action, ticks: u64) -> Action {
    match action {
        Action::Increment(_) => Action::Increment(ticks),
        Action::ResetForward(_) => Action::ResetForward(ticks),
        Action::ResetBackward(_) => Action::ResetBackward(ticks),
    }
}

fn instruction(action: Action, tick: u64) -> String {
    let label = match action {
        Action::Increment(_) => "Increment",
        Action::ResetForward(_) => "Reset Forward",
        Action::ResetBackward(_) => "Reset Backward",
    };

    format!("{label} · {tick} of {}", action_ticks(action))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Step;

    fn solution(start: &str, reset_index: u8, actions: &[Action]) -> Solution {
        let mut counter = TallyCounter::new(start.len()).unwrap();
        counter
            .set_values(start.bytes().map(|digit| digit - b'0').collect())
            .unwrap();
        counter.set_reset_index(reset_index).unwrap();
        let mut steps = Vec::new();

        for &action in actions {
            action.apply(&mut counter);
            steps.push(Step {
                action,
                value: counter
                    .values()
                    .iter()
                    .map(|&digit| char::from(b'0' + digit))
                    .collect(),
                reset_index: counter.reset_index(),
                starts_reset_group: false,
            });
        }

        Solution {
            target: counter
                .values()
                .iter()
                .map(|&digit| char::from(b'0' + digit))
                .collect(),
            start: start.to_owned(),
            initial_reset_index: reset_index,
            increments: 0,
            reset_ticks: 0,
            steps,
        }
    }

    #[test]
    fn each_tick_replays_real_operations_with_leading_zeros_and_reset_directions() {
        let solution = solution(
            "009",
            9,
            &[
                Action::Increment(2),
                Action::ResetForward(2),
                Action::ResetBackward(2),
                Action::Increment(3),
            ],
        );
        let mut playback = Playback::new(&solution);
        let now = Instant::now();
        let mut counter = initial_counter(&solution);
        assert!(!playback.is_playing());
        assert_eq!(playback.speed(), 3.0);
        assert_eq!(playback.frame(now).current_digits, &[0, 0, 9]);
        assert_eq!(playback.frame(now).reset_index, 9);

        for (idx, step) in solution.steps.iter().enumerate() {
            for tick in 1..=action_ticks(step.action) {
                let previous_digits = counter.values().to_vec();
                let previous_reset = counter.reset_index();
                with_ticks(step.action, 1).apply(&mut counter);
                playback.next(&solution, now);
                let frame = playback.frame(now);

                assert_eq!(frame.previous_digits, previous_digits);
                assert_eq!(frame.current_digits, counter.values());
                assert_eq!(frame.previous_reset_index, previous_reset);
                assert_eq!(frame.reset_index, counter.reset_index());
                assert_eq!(frame.active_step, Some(idx));
                assert_eq!(frame.instruction, instruction(step.action, tick));
                assert_eq!(frame.progress, 0.0);

                if matches!(step.action, Action::ResetBackward(_)) {
                    assert_eq!(frame.digit_direction, RollDirection::Still);
                    assert_eq!(frame.reset_direction, RollDirection::Backward);
                }
            }
        }

        assert!(playback.is_finished());
        assert!(!playback.is_playing());
        assert_eq!(playback.frame(now).completed_ticks, 9);
        assert_eq!(playback.frame(now).total_ticks, 9);
        assert_eq!(playback.frame(now + MAX_TRANSITION).progress, 1.0);
        assert!(playback.next_frame_delay(now + MAX_TRANSITION).is_none());
        assert_eq!(
            counter.values(),
            solution
                .target
                .bytes()
                .map(|digit| digit - b'0')
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn previous_reconstructs_noninvertible_resets_and_can_cross_group_boundaries() {
        let solution = solution(
            "01",
            0,
            &[
                Action::ResetForward(1),
                Action::ResetBackward(0),
                Action::Increment(2),
                Action::ResetBackward(1),
            ],
        );
        let now = Instant::now();
        let mut playback = Playback::new(&solution);
        let mut frames = vec![(vec![0, 1], 0)];

        while playback.can_next() {
            playback.next(&solution, now);
            let frame = playback.frame(now);
            frames.push((frame.current_digits.to_vec(), frame.reset_index));
        }

        for (digits, reset_index) in frames.into_iter().rev().skip(1) {
            playback.previous(&solution, now);
            let frame = playback.frame(now);

            assert_eq!(frame.current_digits, digits);
            assert_eq!(frame.reset_index, reset_index);
        }

        assert!(!playback.can_previous());
        assert!(playback.can_next());
        assert_eq!(playback.frame(now).instruction, "Initial state");
        assert_eq!(playback.frame(now).digit_direction, RollDirection::Backward);
        assert_eq!(playback.frame(now).reset_direction, RollDirection::Backward);
        playback.previous(&solution, now);
        assert_eq!(playback.frame(now).current_digits, &[0, 1]);
    }

    #[test]
    fn paused_position_is_exact_and_late_frames_never_catch_up_in_a_burst() {
        let solution = solution("000", 7, &[Action::Increment(10)]);
        let mut playback = Playback::new(&solution);
        let now = Instant::now();
        playback.play(&solution, now);
        playback.advance(&solution, now + Duration::from_secs(60));
        assert_eq!(playback.frame(now).completed_ticks, 1);

        playback.pause();
        assert_eq!(playback.frame(now).current_digits, &[0, 0, 1]);
        assert_eq!(playback.frame(now).previous_digits, &[0, 0, 1]);
        assert_eq!(playback.frame(now).progress, 1.0);
        assert!(playback.next_frame_delay(now).is_none());
        playback.advance(&solution, now + Duration::from_secs(120));
        assert_eq!(playback.frame(now).completed_ticks, 1);

        playback.play(&solution, now + Duration::from_secs(120));
        playback.advance(&solution, now + Duration::from_secs(120));
        assert_eq!(playback.frame(now).completed_ticks, 1);
        playback.advance(&solution, now + Duration::from_secs(121));
        assert_eq!(playback.frame(now).completed_ticks, 2);
    }

    #[test]
    fn playback_finishes_and_play_from_the_end_restarts_with_the_selected_speed() {
        let solution = solution("009", 8, &[Action::Increment(1)]);
        let mut playback = Playback::new(&solution);
        let now = Instant::now();
        playback.set_speed(8.0, now);
        playback.play(&solution, now);
        playback.advance(&solution, now + Duration::from_secs(1));

        assert!(playback.is_finished());
        assert!(!playback.is_playing());
        assert_eq!(playback.frame(now).current_digits, &[0, 1, 0]);
        assert!(
            playback
                .next_frame_delay(now + Duration::from_secs(2))
                .is_none()
        );

        playback.play(&solution, now + Duration::from_secs(3));

        assert!(playback.is_playing());
        assert_eq!(playback.frame(now).current_digits, &[0, 0, 9]);
        assert_eq!(playback.frame(now).completed_ticks, 0);
        assert_eq!(playback.speed(), 8.0);
    }

    #[test]
    fn speed_is_finite_and_bounded_and_transitions_finish_before_the_next_tick() {
        let solution = solution("000", 0, &[Action::Increment(4)]);
        let mut playback = Playback::new(&solution);
        let now = Instant::now();

        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            playback.set_speed(invalid, now);
            assert_eq!(playback.speed(), 3.0);
        }

        playback.set_speed(-10.0, now);
        assert_eq!(playback.speed(), 1.0);
        playback.set_speed(100.0, now);
        assert_eq!(playback.speed(), 8.0);
        playback.next(&solution, now);
        assert!(playback.transition_duration < playback.tick_interval());
        assert!(playback.transition_duration <= MAX_TRANSITION);
        assert!(playback.frame(now + Duration::from_millis(40)).progress > 0.0);
        assert!(playback.frame(now + Duration::from_millis(40)).progress < 1.0);
        assert_eq!(playback.next_frame_delay(now), Some(FRAME_INTERVAL));
        assert!(playback.next_frame_delay(now).is_some());
        assert_eq!(
            playback.frame(now + Duration::from_millis(100)).progress,
            1.0
        );
        assert!(
            playback
                .next_frame_delay(now + Duration::from_millis(100))
                .is_none()
        );
    }

    #[test]
    fn empty_and_zero_tick_sequences_stay_at_the_initial_state() {
        let now = Instant::now();

        for actions in [
            Vec::new(),
            vec![
                Action::Increment(0),
                Action::ResetForward(0),
                Action::ResetBackward(0),
            ],
        ] {
            let solution = solution("0012", 8, &actions);
            let mut playback = Playback::new(&solution);
            playback.play(&solution, now);
            playback.next(&solution, now);
            playback.previous(&solution, now);
            playback.advance(&solution, now + Duration::from_secs(1));

            assert!(playback.is_finished());
            assert!(!playback.is_playing());
            assert!(!playback.can_next());
            assert!(!playback.can_previous());
            assert_eq!(playback.frame(now).current_digits, &[0, 0, 1, 2]);
            assert_eq!(playback.frame(now).reset_index, 8);
            assert_eq!(playback.frame(now).total_ticks, 0);
            assert!(playback.next_frame_delay(now).is_none());
        }
    }

    #[test]
    fn wide_counters_and_huge_group_counts_do_not_expand_ticks_or_overflow_totals() {
        let start = "0".repeat(1_000);
        let solution = solution(
            &start,
            7,
            &[Action::Increment(u64::MAX), Action::ResetBackward(u64::MAX)],
        );
        let mut playback = Playback::new(&solution);
        let now = Instant::now();
        assert_eq!(playback.frame(now).total_ticks, u64::MAX);
        assert_eq!(playback.frame(now).current_digits.len(), 1_000);
        playback.next(&solution, now);
        assert_eq!(playback.frame(now).completed_ticks, 1);
        assert_eq!(playback.frame(now).current_digits[999], 1);
        playback.previous(&solution, now);
        assert_eq!(playback.frame(now).current_digits, vec![0; 1_000]);

        // A cursor can still distinguish exact positions after the displayed
        // totals saturate. Rebuilding a huge prefix applies each group once.
        playback.step_index = 1;
        playback.ticks_in_step = 2;
        playback.rebuild_position(&solution);
        playback.previous(&solution, now);
        assert_eq!(playback.frame(now).completed_ticks, u64::MAX);
        assert_eq!(playback.frame(now).active_step, Some(1));
        assert_eq!(playback.frame(now).reset_index, 6);
        assert!(playback.can_previous());
        assert!(playback.can_next());
    }
}
