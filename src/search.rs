use std::{cmp::Reverse, collections::BinaryHeap};

use thiserror::Error;

use crate::TallyCounter;

/// Invalid search input or a search space that cannot be represented or allocated.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SearchError {
    #[error("Invalid digit count: {0}. Value should be greater than 0.")]
    InvalidDigitCount(usize),
    #[error("Invalid target length: expected {expected}, got {actual}")]
    InvalidTargetLength { expected: usize, actual: usize },
    #[error("Invalid target digit: {0}. Value should be in the range 0-9.")]
    InvalidTargetDigit(u8),
    #[error("Counter length too large: {0}")]
    CounterLengthTooLarge(usize),
    #[error("Unable to allocate memory for the search")]
    AllocationFailed,
}

/// A group of consecutive operations of the same kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Increment(u64),
    ResetForward(u64),
    ResetBackward(u64),
}

impl Action {
    /// Apply every operation in this group to a counter.
    pub fn apply(self, counter: &mut TallyCounter) {
        match self {
            Self::Increment(ticks) => counter.increment_by(ticks),
            Self::ResetForward(ticks) => counter.reset_forward_by(ticks),
            Self::ResetBackward(ticks) => counter.reset_backward_by(ticks),
        }
    }
}

/// The outcome of searching for a displayed value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchResult {
    /// An optimal sequence, with consecutive operations grouped by kind.
    /// The sequence is empty when the counter already displays the target.
    Found(Vec<Action>),
    NotFound,
}

/// The current outcome of a bounded search step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchProgress {
    /// More work remains. The count includes non-stale states visited so far.
    InProgress {
        visited_states: usize,
    },
    Complete(SearchResult),
}

/// An incremental solver that can yield between bounded portions of work.
///
/// The search has the same objectives and input requirements as [`search`].
/// It owns its search state, so the initial counter can be changed or dropped.
#[derive(Debug)]
pub struct SearchSession {
    state: SessionState,
}

#[derive(Debug)]
enum SessionState {
    Searching(ActiveSearch),
    Complete(SearchResult),
    Failed(SearchError),
}

#[derive(Debug)]
struct ActiveSearch {
    digit_count: usize,
    value_states: usize,
    start: usize,
    target_value: usize,
    distance: Vec<Cost>,
    parent: Vec<usize>,
    action: Vec<Option<Action>>,
    queue: BinaryHeap<Reverse<(Cost, usize)>>,
    visited_states: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Cost {
    // Field order is important: derived Ord is lexicographic.
    increments: usize,
    reset_ticks: usize,
}

const INF: Cost = Cost {
    increments: usize::MAX,
    reset_ticks: usize::MAX,
};

const NO_PARENT: usize = usize::MAX;

/// Find a sequence that first minimizes increments, then total reset ticks.
///
/// The final reset index is unconstrained. The target must contain one decimal
/// digit per counter digit, including leading zeros. Search storage grows as
/// `10^(digit_count + 1)`; unrepresentable widths and allocation failures are
/// reported as errors. An already displayed target needs no search allocation.
pub fn search(counter: &TallyCounter, target: &[u8]) -> Result<SearchResult, SearchError> {
    let mut session = SearchSession::new(counter, target)?;

    loop {
        if let SearchProgress::Complete(result) = session.advance(usize::MAX)? {
            return Ok(result);
        }
    }
}

impl SearchSession {
    /// Validate the target and allocate the search state.
    ///
    /// An already displayed target completes without allocating search buffers.
    pub fn new(counter: &TallyCounter, target: &[u8]) -> Result<Self, SearchError> {
        let digit_count = counter.values().len();

        if digit_count == 0 {
            return Err(SearchError::InvalidDigitCount(digit_count));
        }

        if target.len() != digit_count {
            return Err(SearchError::InvalidTargetLength {
                expected: digit_count,
                actual: target.len(),
            });
        }

        if let Some(&invalid_digit) = target.iter().find(|&&digit| digit > 9) {
            return Err(SearchError::InvalidTargetDigit(invalid_digit));
        }

        let exponent = digit_count
            .try_into()
            .map_err(|_| SearchError::CounterLengthTooLarge(digit_count))?;

        let value_states = 10usize
            .checked_pow(exponent)
            .ok_or(SearchError::CounterLengthTooLarge(digit_count))?;

        let state_count = value_states
            .checked_mul(10)
            .ok_or(SearchError::CounterLengthTooLarge(digit_count))?;

        if counter.values() == target {
            return Ok(Self {
                state: SessionState::Complete(SearchResult::Found(Vec::new())),
            });
        }

        let start = encode_state(counter.values(), counter.reset_index());
        let target_value = encode_values(target);

        let mut distance = filled_buffer(INF, state_count)?;
        let mut parent = filled_buffer(NO_PARENT, state_count)?;
        let action = filled_buffer(None, state_count)?;
        let mut queue = BinaryHeap::new();

        distance[start] = Cost {
            increments: 0,
            reset_ticks: 0,
        };

        parent[start] = start;
        queue
            .try_reserve(1)
            .map_err(|_| SearchError::AllocationFailed)?;

        queue.push(Reverse((distance[start], start)));

        Ok(Self {
            state: SessionState::Searching(ActiveSearch {
                digit_count,
                value_states,
                start,
                target_value,
                distance,
                parent,
                action,
                queue,
                visited_states: 0,
            }),
        })
    }

    /// Pop at most `max_states` queue entries, including stale entries.
    ///
    /// A zero budget observes the current state without doing search work.
    /// Completion releases the search buffers and subsequent calls return the
    /// same result. A failed step also retains its error for subsequent calls.
    pub fn advance(&mut self, max_states: usize) -> Result<SearchProgress, SearchError> {
        let progress = match &mut self.state {
            SessionState::Searching(search) => search.advance(max_states),
            SessionState::Complete(result) => {
                return Ok(SearchProgress::Complete(result.clone()));
            }

            SessionState::Failed(error) => return Err(error.clone()),
        };

        match &progress {
            Ok(SearchProgress::Complete(result)) => {
                self.state = SessionState::Complete(result.clone());
            }

            Err(error) => {
                self.state = SessionState::Failed(error.clone());
            }

            Ok(SearchProgress::InProgress { .. }) => {}
        };

        progress
    }
}

impl ActiveSearch {
    fn advance(&mut self, max_states: usize) -> Result<SearchProgress, SearchError> {
        for _ in 0..max_states {
            let Some(Reverse((cost, state))) = self.queue.pop() else {
                return Ok(SearchProgress::Complete(SearchResult::NotFound));
            };

            // Stale entries consume the budget as well, keeping each step bounded.
            if cost != self.distance[state] {
                continue;
            }

            self.visited_states += 1;

            let value = state / 10;
            let reset = state % 10;

            // The first target popped is optimal in both objectives.
            if value == self.target_value {
                return Ok(SearchProgress::Complete(SearchResult::Found(reconstruct(
                    state,
                    self.start,
                    &self.parent,
                    &self.action,
                )?)));
            }

            let next = reset_forward_one(state, self.digit_count);

            relax(
                state,
                next,
                Action::ResetForward(1),
                Cost {
                    increments: cost.increments,
                    reset_ticks: cost.reset_ticks + 1,
                },
                &mut self.distance,
                &mut self.parent,
                &mut self.action,
                &mut self.queue,
            )?;

            let next_reset = (reset + 9) % 10;
            let next = value * 10 + next_reset;

            relax(
                state,
                next,
                Action::ResetBackward(1),
                Cost {
                    increments: cost.increments,
                    reset_ticks: cost.reset_ticks + 1,
                },
                &mut self.distance,
                &mut self.parent,
                &mut self.action,
                &mut self.queue,
            )?;

            let next_value = if value + 1 == self.value_states {
                0
            } else {
                value + 1
            };

            let next = next_value * 10 + reset;

            relax(
                state,
                next,
                Action::Increment(1),
                Cost {
                    increments: cost.increments + 1,
                    reset_ticks: cost.reset_ticks,
                },
                &mut self.distance,
                &mut self.parent,
                &mut self.action,
                &mut self.queue,
            )?;
        }

        if self.queue.is_empty() {
            return Ok(SearchProgress::Complete(SearchResult::NotFound));
        }

        Ok(SearchProgress::InProgress {
            visited_states: self.visited_states,
        })
    }
}

fn filled_buffer<T: Clone>(value: T, len: usize) -> Result<Vec<T>, SearchError> {
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(len)
        .map_err(|_| SearchError::AllocationFailed)?;

    buffer.resize(len, value);

    Ok(buffer)
}

#[allow(clippy::too_many_arguments)]
fn relax(
    current: usize,
    next: usize,
    next_action: Action,
    next_cost: Cost,
    distance: &mut [Cost],
    parent: &mut [usize],
    action: &mut [Option<Action>],
    queue: &mut BinaryHeap<Reverse<(Cost, usize)>>,
) -> Result<(), SearchError> {
    if next_cost >= distance[next] {
        return Ok(());
    }

    queue
        .try_reserve(1)
        .map_err(|_| SearchError::AllocationFailed)?;

    distance[next] = next_cost;
    parent[next] = current;
    action[next] = Some(next_action);
    queue.push(Reverse((next_cost, next)));

    Ok(())
}

fn encode_values(values: &[u8]) -> usize {
    values
        .iter()
        .fold(0usize, |acc, &digit| acc * 10 + digit as usize)
}

fn encode_state(values: &[u8], reset_index: u8) -> usize {
    encode_values(values) * 10 + reset_index as usize
}

fn reset_forward_one(state: usize, digit_count: usize) -> usize {
    let value = state / 10;
    let reset = (state % 10) as u8;
    let next_reset = (reset + 1) % 10;

    let mut next_value = value;
    let mut place = 1usize;

    for _ in 0..digit_count {
        let digit = ((value / place) % 10) as u8;

        if digit == reset {
            if reset == 9 {
                next_value -= 9 * place;
            } else {
                next_value += place;
            }
        }

        place *= 10;
    }

    next_value * 10 + next_reset as usize
}

fn reconstruct(
    mut state: usize,
    start: usize,
    parent: &[usize],
    action: &[Option<Action>],
) -> Result<Vec<Action>, SearchError> {
    let mut result = Vec::new();

    while state != start {
        combine_action(&mut result, action[state].unwrap())?;
        state = parent[state];
    }

    result.reverse();

    Ok(result)
}

fn combine_action(result: &mut Vec<Action>, action: Action) -> Result<(), SearchError> {
    match (result.last_mut(), action) {
        (Some(Action::Increment(total)), Action::Increment(ticks))
        | (Some(Action::ResetForward(total)), Action::ResetForward(ticks))
        | (Some(Action::ResetBackward(total)), Action::ResetBackward(ticks)) => {
            *total += ticks;
        }

        (_, action) => {
            result
                .try_reserve(1)
                .map_err(|_| SearchError::AllocationFailed)?;

            result.push(action);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found_actions(counter: &TallyCounter, target: &[u8]) -> Vec<Action> {
        match search(counter, target).unwrap() {
            SearchResult::Found(actions) => actions,
            SearchResult::NotFound => panic!("Every decimal target is reachable by incrementing"),
        }
    }

    fn action_cost(actions: &[Action]) -> (usize, usize) {
        actions.iter().fold((0, 0), |acc, action| match action {
            Action::Increment(ticks) => (acc.0 + *ticks as usize, acc.1),
            Action::ResetForward(ticks) | Action::ResetBackward(ticks) => {
                (acc.0, acc.1 + *ticks as usize)
            }
        })
    }

    #[test]
    fn invalid_targets_return_errors() {
        let counter = TallyCounter::new(2).unwrap();

        assert_eq!(
            search(&counter, &[1]),
            Err(SearchError::InvalidTargetLength {
                expected: 2,
                actual: 1,
            })
        );
        assert_eq!(
            search(&counter, &[1, 2, 3]),
            Err(SearchError::InvalidTargetLength {
                expected: 2,
                actual: 3,
            })
        );
        assert_eq!(
            search(&counter, &[0, 10]),
            Err(SearchError::InvalidTargetDigit(10))
        );
    }

    #[test]
    fn runtime_widths_preserve_leading_zeros_and_replay_actions() {
        for target in [vec![7], vec![0, 7], vec![0, 0, 7]] {
            let initial = TallyCounter::new(target.len()).unwrap();
            let actions = found_actions(&initial, &target);
            let mut counter = initial;

            for action in &actions {
                action.apply(&mut counter);
            }

            assert_eq!(counter.values(), target);
            assert!(actions.windows(2).all(|pair| {
                std::mem::discriminant(&pair[0]) != std::mem::discriminant(&pair[1])
            }));
        }
    }

    #[test]
    fn already_matched_target_has_no_actions() {
        let mut counter = TallyCounter::new(3).unwrap();
        counter.set_values(vec![0, 1, 2]).unwrap();
        counter.set_reset_index(7).unwrap();

        assert_eq!(
            search(&counter, &[0, 1, 2]),
            Ok(SearchResult::Found(Vec::new()))
        );
    }

    #[test]
    fn session_respects_zero_and_single_entry_budgets() {
        let counter = TallyCounter::new(2).unwrap();
        let mut session = SearchSession::new(&counter, &[1, 2]).unwrap();

        assert_eq!(
            session.advance(0),
            Ok(SearchProgress::InProgress { visited_states: 0 })
        );
        assert_eq!(
            session.advance(1),
            Ok(SearchProgress::InProgress { visited_states: 1 })
        );
        assert_eq!(
            session.advance(0),
            Ok(SearchProgress::InProgress { visited_states: 1 })
        );
    }

    #[test]
    fn stale_queue_entries_consume_the_session_budget() {
        let mut counter = TallyCounter::new(2).unwrap();
        counter.set_values(vec![5, 0]).unwrap();

        let mut session = SearchSession::new(&counter, &[5, 1]).unwrap();
        let SessionState::Searching(search) = &mut session.state else {
            panic!("An unmatched target needs a search");
        };

        // This entry sorts before the start but no longer matches its distance.
        search.queue.push(Reverse((
            Cost {
                increments: 0,
                reset_ticks: 0,
            },
            0,
        )));

        assert_eq!(
            session.advance(1),
            Ok(SearchProgress::InProgress { visited_states: 0 })
        );
        assert_eq!(
            session.advance(1),
            Ok(SearchProgress::InProgress { visited_states: 1 })
        );
    }

    #[test]
    fn bounded_sessions_preserve_the_full_search_result() {
        for (values, reset_index, target) in [
            (vec![0, 0], 0, vec![9, 8]),
            (vec![7, 2], 5, vec![0, 3]),
            (vec![0, 0, 0], 9, vec![1, 2, 3]),
        ] {
            let mut counter = TallyCounter::new(values.len()).unwrap();
            counter.set_values(values).unwrap();
            counter.set_reset_index(reset_index).unwrap();

            let expected = search(&counter, &target).unwrap();

            for budget in [1, 7, 64] {
                let mut session = SearchSession::new(&counter, &target).unwrap();
                let mut previous_visited = 0;

                loop {
                    match session.advance(budget).unwrap() {
                        SearchProgress::InProgress { visited_states } => {
                            assert!(visited_states >= previous_visited);
                            assert!(visited_states - previous_visited <= budget);
                            previous_visited = visited_states;
                        }

                        SearchProgress::Complete(result) => {
                            assert_eq!(result, expected);
                            break;
                        }
                    }
                }

                assert!(matches!(session.state, SessionState::Complete(_)));
                assert_eq!(
                    session.advance(0),
                    Ok(SearchProgress::Complete(expected.clone()))
                );
                assert_eq!(
                    session.advance(1),
                    Ok(SearchProgress::Complete(expected.clone()))
                );
            }
        }
    }

    #[test]
    fn already_matched_sessions_are_complete_without_search_buffers() {
        let counter = TallyCounter::new(3).unwrap();
        let mut session = SearchSession::new(&counter, &[0, 0, 0]).unwrap();

        assert!(matches!(session.state, SessionState::Complete(_)));
        assert_eq!(
            session.advance(0),
            Ok(SearchProgress::Complete(SearchResult::Found(Vec::new())))
        );
    }

    #[test]
    fn actions_combine_every_kind() {
        let mut actions = Vec::new();

        for action in [
            Action::Increment(2),
            Action::Increment(3),
            Action::ResetForward(1),
            Action::ResetForward(4),
            Action::ResetBackward(2),
            Action::ResetBackward(5),
            Action::Increment(1),
        ] {
            combine_action(&mut actions, action).unwrap();
        }

        assert_eq!(
            actions,
            vec![
                Action::Increment(5),
                Action::ResetForward(5),
                Action::ResetBackward(7),
                Action::Increment(1),
            ]
        );
    }

    #[test]
    fn unrepresentable_and_unallocatable_searches_return_errors() {
        let oversized_width = usize::BITS as usize;
        let counter = TallyCounter::new(oversized_width).unwrap();

        assert_eq!(
            search(&counter, &vec![0; oversized_width]),
            Err(SearchError::CounterLengthTooLarge(oversized_width))
        );
        assert_eq!(
            filled_buffer(INF, usize::MAX),
            Err(SearchError::AllocationFailed)
        );
    }

    #[test]
    fn increments_have_priority_and_reset_ticks_break_ties() {
        let counter = TallyCounter::new(2).unwrap();

        assert_eq!(
            found_actions(&counter, &[9, 9]),
            vec![Action::ResetForward(9)]
        );
        assert_eq!(found_actions(&counter, &[0, 1]), vec![Action::Increment(1)]);
    }

    // Bellman-Ford uses the counter's public operations as an independent
    // transition model, rather than the search's encoded-state arithmetic.
    fn reference_costs(initial: &TallyCounter) -> Vec<(usize, usize)> {
        let mut distance = vec![(usize::MAX, usize::MAX); 1_000];
        let initial_value = initial.values()[0] as usize * 10 + initial.values()[1] as usize;
        let initial_state = initial_value * 10 + initial.reset_index() as usize;
        distance[initial_state] = (0, 0);

        loop {
            let mut changed = false;

            for state in 0..distance.len() {
                let cost = distance[state];

                if cost.0 == usize::MAX {
                    continue;
                }

                let display = state / 10;
                let mut counter = TallyCounter::new(2).unwrap();
                counter
                    .set_values(vec![(display / 10) as u8, (display % 10) as u8])
                    .unwrap();

                counter.set_reset_index((state % 10) as u8).unwrap();

                for action in [
                    Action::Increment(1),
                    Action::ResetForward(1),
                    Action::ResetBackward(1),
                ] {
                    let mut next_counter = counter.clone();
                    action.apply(&mut next_counter);

                    let next_display =
                        next_counter.values()[0] as usize * 10 + next_counter.values()[1] as usize;
                    let next_state = next_display * 10 + next_counter.reset_index() as usize;
                    let next_cost = match action {
                        Action::Increment(_) => (cost.0 + 1, cost.1),
                        Action::ResetForward(_) | Action::ResetBackward(_) => (cost.0, cost.1 + 1),
                    };

                    if next_cost < distance[next_state] {
                        distance[next_state] = next_cost;
                        changed = true;
                    }
                }
            }

            if !changed {
                break;
            }
        }

        distance
    }

    #[test]
    fn every_two_digit_target_matches_independent_optimal_costs() {
        for (values, reset_index) in [(vec![0, 0], 0), (vec![7, 2], 5)] {
            let mut initial = TallyCounter::new(2).unwrap();
            initial.set_values(values).unwrap();
            initial.set_reset_index(reset_index).unwrap();

            let reference = reference_costs(&initial);

            for display in 0..100 {
                let target = [(display / 10) as u8, (display % 10) as u8];
                let actions = found_actions(&initial, &target);
                let expected = *reference[display * 10..display * 10 + 10]
                    .iter()
                    .min()
                    .unwrap();

                assert_eq!(action_cost(&actions), expected, "Target {target:?}");

                let mut counter = initial.clone();

                for action in actions {
                    action.apply(&mut counter);
                }

                assert_eq!(counter.values(), target);
            }
        }
    }
}
