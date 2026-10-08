use std::{cmp::Reverse, collections::BinaryHeap};

use crate::TallyCounter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Increment(u64),
    ResetForward(u64),
    ResetBackward(u64),
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

pub fn search<const N: usize>(counter: &TallyCounter<N>, target: [u8; N]) -> Option<Vec<Action>> {
    let value_states = 10usize.checked_pow(N as u32)?;
    let state_count = value_states.checked_mul(10)?;

    let start = encode_state(counter.values(), counter.reset_index());
    let target_value = encode_values(&target);

    let mut distance = vec![INF; state_count];
    let mut parent = vec![NO_PARENT; state_count];
    let mut action = vec![None; state_count];

    let mut queue = BinaryHeap::new();

    distance[start] = Cost {
        increments: 0,
        reset_ticks: 0,
    };

    parent[start] = start;

    queue.push(Reverse((distance[start], start)));

    while let Some(Reverse((cost, state))) = queue.pop() {
        // Ignore stale queue entries.
        if cost != distance[state] {
            continue;
        }

        let value = state / 10;
        let reset = state % 10;

        // Since Dijkstra pops states in optimal cost order, the first
        // target state encountered is optimal in both objectives.
        if value == target_value {
            return Some(combine_actions(reconstruct(state, start, &parent, &action)));
        }

        // ------------------------------------------------------------
        // Reset forward: (0 increments, 1 reset tick)
        // ------------------------------------------------------------

        let next = reset_forward_one::<N>(state);

        relax(
            state,
            next,
            Action::ResetForward(1),
            Cost {
                increments: cost.increments,
                reset_ticks: cost.reset_ticks + 1,
            },
            &mut distance,
            &mut parent,
            &mut action,
            &mut queue,
        );

        // ------------------------------------------------------------
        // Reset backward: (0 increments, 1 reset tick)
        // ------------------------------------------------------------

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
            &mut distance,
            &mut parent,
            &mut action,
            &mut queue,
        );

        // ------------------------------------------------------------
        // Increment: (1 increment, 0 reset ticks)
        // ------------------------------------------------------------

        let next_value = if value + 1 == value_states {
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
            &mut distance,
            &mut parent,
            &mut action,
            &mut queue,
        );
    }

    None
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
) {
    if next_cost >= distance[next] {
        return;
    }

    distance[next] = next_cost;
    parent[next] = current;
    action[next] = Some(next_action);

    queue.push(Reverse((next_cost, next)));
}

fn encode_values<const N: usize>(values: &[u8; N]) -> usize {
    values
        .iter()
        .fold(0usize, |value, &digit| value * 10 + digit as usize)
}

fn encode_state<const N: usize>(values: &[u8; N], reset_index: u8) -> usize {
    encode_values(values) * 10 + reset_index as usize
}

fn reset_forward_one<const N: usize>(state: usize) -> usize {
    let value = state / 10;
    let reset = (state % 10) as u8;

    let next_reset = (reset + 1) % 10;

    let mut next_value = value;
    let mut place = 1usize;

    for _ in 0..N {
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
) -> Vec<Action> {
    let mut result = Vec::new();

    while state != start {
        result.push(action[state].unwrap());
        state = parent[state];
    }

    result.reverse();
    result
}

fn combine_actions(actions: Vec<Action>) -> Vec<Action> {
    let mut result = Vec::with_capacity(actions.len());

    for action in actions {
        match (result.last_mut(), action) {
            (Some(Action::ResetForward(a)), Action::ResetForward(b)) => {
                *a += b;
            }

            (Some(Action::ResetBackward(a)), Action::ResetBackward(b)) => {
                *a += b;
            }

            (_, action) => result.push(action),
        }
    }

    result
}
