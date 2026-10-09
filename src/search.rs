use std::{cmp::Reverse, collections::BinaryHeap};

mod diagram;

use diagram::{Diagram, EMPTY, NodeId, insert, push};

use rustc_hash::FxHashMap as HashMap;
use thiserror::Error;

use crate::TallyCounter;

/// Invalid search input or insufficient storage for the explored patterns.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SearchError {
    #[error("Invalid digit count: {0}. Value should be greater than 0.")]
    InvalidDigitCount(usize),
    #[error("Invalid target length: expected {expected}, got {actual}")]
    InvalidTargetLength { expected: usize, actual: usize },
    #[error("Invalid target digit: {0}. Value should be in the range 0-9.")]
    InvalidTargetDigit(u8),
    /// Retained for compatibility; the symbolic solver has no numeric width limit.
    #[error("Counter length too large: {0}")]
    CounterLengthTooLarge(usize),
    #[error("The optimal sequence is too long to represent")]
    CostTooLarge,
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
    /// More work remains. Each visited state group may represent many concrete
    /// counter states. The field name is retained for API compatibility.
    InProgress {
        visited_states: usize,
    },
    Complete(SearchResult),
}

/// Observations of search work, without an estimated total or completion time.
///
/// Diagram work is a cumulative count of approximate node visits during set
/// operations and compaction. It is useful for comparing successive samples,
/// but does not represent concrete counter states or a fixed duration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchStatistics {
    /// Nonempty state groups explored, matching `SearchProgress::visited_states`.
    pub visited_groups: usize,
    /// Increment cost of the latest queued group processed.
    pub increment_layer: u64,
    /// Reset cost within the latest increment layer; can decrease at a new layer.
    pub reset_ticks: u64,
    /// Diagram nodes at the latest sample; compaction can reduce this count.
    pub diagram_nodes: usize,
    /// Cumulative approximate diagram work, including compaction and cache clears.
    pub diagram_work: u64,
    /// Queued state groups at the latest sample, whose processing may create more.
    pub queued_groups: usize,
}

/// An exact incremental solver. Search state is owned independently of the
/// original counter and released on completion or failure.
#[derive(Debug)]
pub struct SearchSession {
    state: SessionState,
    statistics: SearchStatistics,
}

#[derive(Debug)]
enum SessionState {
    Searching(Box<ActiveSearch>),
    Complete(SearchResult),
    Failed(SearchError),
}

#[derive(Debug)]
struct ActiveSearch {
    initial: TallyCounter,
    diagram: Diagram,
    settled: [NodeId; 10],
    regions: HashMap<(Cost, u8), NodeId>,
    pending: HashMap<(Cost, u8), NodeId>,
    queue: BinaryHeap<Reverse<(Cost, u8)>>,
    visited_states: usize,
    cache_cost: Option<Cost>,
    collection_threshold: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Cost {
    // Field order is important: derived Ord is lexicographic.
    increments: u64,
    reset_ticks: u64,
}

impl Cost {
    const ZERO: Self = Self {
        increments: 0,
        reset_ticks: 0,
    };

    fn increment(self) -> Result<Self, SearchError> {
        Ok(Self {
            increments: self
                .increments
                .checked_add(1)
                .ok_or(SearchError::CostTooLarge)?,
            ..self
        })
    }

    fn reset(self) -> Result<Self, SearchError> {
        Ok(Self {
            reset_ticks: self
                .reset_ticks
                .checked_add(1)
                .ok_or(SearchError::CostTooLarge)?,
            ..self
        })
    }
}

/// Find a sequence that first minimizes increments, then total reset ticks.
///
/// The final reset index is unconstrained. The target must contain one decimal
/// digit per counter digit, including leading zeros. A symbolic reverse search
/// shares sets of digit combinations instead of allocating every possible state.
/// Storage depends on the patterns explored; difficult inputs can still require
/// exponential work. There is no numeric counter-width limit.
pub fn search(counter: &TallyCounter, target: &[u8]) -> Result<SearchResult, SearchError> {
    let mut session = SearchSession::new(counter, target)?;

    loop {
        if let SearchProgress::Complete(result) = session.advance(usize::MAX)? {
            return Ok(result);
        }
    }
}

impl SearchSession {
    /// Validate the target and prepare its compact search representation.
    /// An already displayed target completes without search buffers.
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

        if counter.values() == target {
            return Ok(Self {
                state: SessionState::Complete(SearchResult::Found(Vec::new())),
                statistics: SearchStatistics::default(),
            });
        }

        // Resets cannot split an equal pair of wheels. If one increment makes
        // the target and creates such a boundary, at least one increment is
        // necessary and this zero-reset sequence is already globally optimal.
        let mut incremented = counter.clone();
        incremented.increment();
        let creates_boundary = counter
            .values()
            .windows(2)
            .zip(target.windows(2))
            .any(|(initial, target)| initial[0] == initial[1] && target[0] != target[1]);

        if incremented.values() == target && creates_boundary {
            return Ok(Self {
                state: SessionState::Complete(SearchResult::Found(vec![Action::Increment(1)])),
                statistics: SearchStatistics::default(),
            });
        }

        let mut diagram = Diagram::default();
        let target_root = diagram.singleton(target)?;
        let mut search = ActiveSearch {
            initial: counter.clone(),
            diagram,
            settled: [EMPTY; 10],
            regions: HashMap::default(),
            pending: HashMap::default(),
            queue: BinaryHeap::new(),
            visited_states: 0,
            cache_cost: None,
            collection_threshold: 32_768,
        };

        // Any final reset index is acceptable.
        for reset in 0..10 {
            search.enqueue(Cost::ZERO, reset, target_root)?;
        }

        Ok(Self {
            statistics: search.statistics(),
            state: SessionState::Searching(Box::new(search)),
        })
    }

    /// Read the latest work snapshot without advancing the search.
    ///
    /// After completion or failure, retains the final snapshot from immediately
    /// before search buffers were released. No diagram storage is retained.
    pub fn statistics(&self) -> SearchStatistics {
        self.statistics
    }

    /// Process at most `max_states` queued state groups, including empty groups.
    ///
    /// A group shares digit patterns and may contain many concrete states; its
    /// processing time varies with its diagram size. Run CPU work on a worker
    /// thread for a responsive interface. A zero budget only observes progress.
    /// Completion releases search buffers and subsequent calls retain the result.
    pub fn advance(&mut self, max_states: usize) -> Result<SearchProgress, SearchError> {
        let progress = match &mut self.state {
            SessionState::Searching(search) => {
                let progress = search.advance(max_states);
                self.statistics = search.statistics();

                progress
            }

            SessionState::Complete(result) => return Ok(SearchProgress::Complete(result.clone())),
            SessionState::Failed(error) => return Err(error.clone()),
        };

        match &progress {
            Ok(SearchProgress::Complete(result)) => {
                self.state = SessionState::Complete(result.clone())
            }
            Err(error) => self.state = SessionState::Failed(error.clone()),
            Ok(SearchProgress::InProgress { .. }) => {}
        }

        progress
    }
}

impl ActiveSearch {
    fn statistics(&self) -> SearchStatistics {
        let cost = self.cache_cost.unwrap_or(Cost::ZERO);

        SearchStatistics {
            visited_groups: self.visited_states,
            increment_layer: cost.increments,
            reset_ticks: cost.reset_ticks,
            diagram_nodes: self.diagram.node_count(),
            diagram_work: self.diagram.work(),
            queued_groups: self.queue.len(),
        }
    }

    fn advance(&mut self, max_states: usize) -> Result<SearchProgress, SearchError> {
        for _ in 0..max_states {
            let Some(Reverse((cost, reset))) = self.queue.pop() else {
                return Ok(SearchProgress::Complete(SearchResult::NotFound));
            };

            if self.diagram.node_count() >= self.collection_threshold {
                self.collect_diagram()?;
            }

            if self.cache_cost != Some(cost) {
                self.diagram.clear_caches();
                self.cache_cost = Some(cost);
            }

            let root = self.pending.remove(&(cost, reset)).unwrap();
            let root = self
                .diagram
                .difference(root, self.settled[reset as usize])?;

            if root == EMPTY {
                continue;
            }

            self.visited_states = self.visited_states.saturating_add(1);
            insert(&mut self.regions, (cost, reset), root)?;

            if reset == self.initial.reset_index()
                && self.diagram.contains(root, self.initial.values())
            {
                return Ok(SearchProgress::Complete(SearchResult::Found(
                    self.reconstruct(cost)?,
                )));
            }

            self.settled[reset as usize] =
                self.diagram.union(self.settled[reset as usize], root)?;
            let reset_cost = cost.reset()?;
            let previous_reset = (reset + 9) % 10;
            let forward_predecessors = self.diagram.reset_preimage(root, previous_reset)?;
            self.enqueue(reset_cost, previous_reset, forward_predecessors)?;
            self.enqueue(reset_cost, (reset + 1) % 10, root)?;

            let increment_predecessors = self.diagram.increment_preimage(root)?;
            self.enqueue(cost.increment()?, reset, increment_predecessors)?;
        }

        Ok(SearchProgress::InProgress {
            visited_states: self.visited_states,
        })
    }

    fn collect_diagram(&mut self) -> Result<(), SearchError> {
        let roots = self
            .settled
            .iter()
            .chain(self.regions.values())
            .chain(self.pending.values())
            .copied();
        let mapping = self.diagram.collect(roots)?;

        for root in self
            .settled
            .iter_mut()
            .chain(self.regions.values_mut())
            .chain(self.pending.values_mut())
        {
            *root = mapping[*root as usize];
        }

        self.collection_threshold = self.diagram.node_count().saturating_mul(3).max(32_768);

        Ok(())
    }

    fn enqueue(&mut self, cost: Cost, reset: u8, root: NodeId) -> Result<(), SearchError> {
        if root == EMPTY {
            return Ok(());
        }

        let key = (cost, reset);

        if let Some(&previous) = self.pending.get(&key) {
            let merged = self.diagram.union(previous, root)?;
            self.pending.insert(key, merged);
        } else {
            self.queue
                .try_reserve(1)
                .map_err(|_| SearchError::AllocationFailed)?;
            insert(&mut self.pending, key, root)?;
            self.queue.push(Reverse(key));
        }

        Ok(())
    }

    fn reconstruct(&self, mut cost: Cost) -> Result<Vec<Action>, SearchError> {
        let mut counter = self.initial.clone();
        let mut result = Vec::new();

        while cost != Cost::ZERO {
            let mut selected = None;

            if cost.increments > 0 {
                let remaining = Cost {
                    increments: cost.increments - 1,
                    ..cost
                };
                let mut next = counter.clone();
                next.increment();

                if self.includes(remaining, &next) {
                    selected = Some((Action::Increment(1), remaining, next));
                }
            }

            if selected.is_none() && cost.reset_ticks > 0 {
                let remaining = Cost {
                    reset_ticks: cost.reset_ticks - 1,
                    ..cost
                };

                for action in [Action::ResetForward(1), Action::ResetBackward(1)] {
                    let mut next = counter.clone();
                    action.apply(&mut next);

                    if self.includes(remaining, &next) {
                        selected = Some((action, remaining, next));
                        break;
                    }
                }
            }

            let (action, remaining, next) =
                selected.expect("An optimal region has an optimal successor");
            combine_action(&mut result, action)?;
            cost = remaining;
            counter = next;
        }

        Ok(result)
    }

    fn includes(&self, cost: Cost, counter: &TallyCounter) -> bool {
        self.regions
            .get(&(cost, counter.reset_index()))
            .is_some_and(|&root| self.diagram.contains(root, counter.values()))
    }
}

fn combine_action(result: &mut Vec<Action>, action: Action) -> Result<(), SearchError> {
    match (result.last_mut(), action) {
        (Some(Action::Increment(total)), Action::Increment(ticks))
        | (Some(Action::ResetForward(total)), Action::ResetForward(ticks))
        | (Some(Action::ResetBackward(total)), Action::ResetBackward(ticks)) => {
            *total = total.checked_add(ticks).ok_or(SearchError::CostTooLarge)?;
        }

        (_, action) => push(result, action)?,
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
        assert_eq!(session.statistics(), SearchStatistics::default());
    }

    #[test]
    fn statistics_preserve_search_order_and_final_snapshot() {
        let counter = TallyCounter::new(3).unwrap();
        let target = [9, 8, 7];
        let mut session = SearchSession::new(&counter, &target).unwrap();
        let initial = session.statistics();
        assert_eq!(initial.visited_groups, 0);
        assert!(initial.queued_groups > 0);
        assert!(initial.diagram_nodes > 0);
        assert!(initial.diagram_work > 0);

        session.advance(0).unwrap();
        assert_eq!(session.statistics(), initial);
        let mut previous = initial;

        let actions = loop {
            let progress = session.advance(7).unwrap();
            let statistics = session.statistics();
            assert!(statistics.visited_groups >= previous.visited_groups);
            assert!(statistics.diagram_work >= previous.diagram_work);
            assert!(
                (statistics.increment_layer, statistics.reset_ticks)
                    >= (previous.increment_layer, previous.reset_ticks)
            );

            previous = statistics;

            match progress {
                SearchProgress::InProgress { visited_states } => {
                    assert_eq!(statistics.visited_groups, visited_states);
                }

                SearchProgress::Complete(SearchResult::Found(actions)) => break actions,
                SearchProgress::Complete(SearchResult::NotFound) => panic!("Target is reachable"),
            }
        };

        let final_statistics = session.statistics();
        let (increments, reset_ticks) = action_cost(&actions);
        assert_eq!(final_statistics.increment_layer, increments as u64);
        assert_eq!(final_statistics.reset_ticks, reset_ticks as u64);
        assert!(final_statistics.diagram_work > initial.diagram_work);
        assert!(matches!(session.state, SessionState::Complete(_)));

        session.advance(usize::MAX).unwrap();
        assert_eq!(session.statistics(), final_statistics);
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
    fn wide_counters_do_not_need_numeric_encoding_or_recursive_traversal() {
        let mut counter = TallyCounter::new(1_000).unwrap();
        counter.set_reset_index(7).unwrap();
        let mut target = vec![0; 1_000];
        target[999] = 1;

        assert_eq!(found_actions(&counter, &target), vec![Action::Increment(1)]);
        assert_eq!(
            search(&counter, &vec![0; 1_000]),
            Ok(SearchResult::Found(Vec::new()))
        );
    }

    #[test]
    fn compaction_preserves_exact_costs_and_replay() {
        let initial = TallyCounter::new(4).unwrap();
        let target = [9, 8, 7, 6];
        let mut session = SearchSession::new(&initial, &target).unwrap();
        let mut previous_work = session.statistics().diagram_work;
        let actions = loop {
            // Force frequent collection while an unfinished frontier and all
            // reconstruction regions are still retained.
            if let SessionState::Searching(search) = &mut session.state {
                search.collection_threshold = 0;
            }

            let progress = session.advance(64).unwrap();
            let work = session.statistics().diagram_work;
            assert!(work > previous_work);
            previous_work = work;

            if let SearchProgress::Complete(SearchResult::Found(actions)) = progress {
                break actions;
            }
        };

        assert_eq!(action_cost(&actions), (6, 148));
        let mut replay = initial;

        for action in actions {
            action.apply(&mut replay);
        }

        assert_eq!(replay.values(), target);
        assert!(matches!(session.state, SessionState::Complete(_)));
    }

    #[test]
    fn five_digit_search_preserves_the_independent_optimum() {
        let mut counter = TallyCounter::new(5).unwrap();
        let target = [9, 8, 7, 6, 5];
        let actions = found_actions(&counter, &target);

        assert_eq!(action_cost(&actions), (11, 267));

        for action in actions {
            action.apply(&mut counter);
        }

        assert_eq!(counter.values(), target);
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
