use tally_problem::*;

fn main() {
    let counter = TallyCounter::<4>::new();
    let target = [9, 8, 7, 6];

    if let Some(actions) = search(&counter, target) {
        let increments = actions
            .iter()
            .filter(|action| matches!(action, Action::Increment(_)))
            .count();

        println!("Minimum increments: {increments}");

        let normalized_actions = normalize_actions(actions);
        print_actions(&counter, &normalized_actions);
    } else {
        println!("Target is unreachable.");
    }
}

fn print_actions<const N: usize>(initial: &TallyCounter<N>, actions: &[Action]) {
    let mut counter = initial.clone();

    let mut was_last_reset = false;

    for &action in actions {
        perform_action(&mut counter, action);

        if !was_last_reset {
            println!("Reset");
        }

        let label_width = 13;
        let count_width = 5;

        let description = match action {
            Action::Increment(ticks) => {
                let count = format!("x{ticks}");
                format!(
                    "{label:<label_width$}{count:>count_width$}",
                    label = "Increment",
                )
            }
            Action::ResetForward(ticks) => {
                let count = format!("x{ticks}");

                format!(
                    "{label:>label_width$}{count:>count_width$}",
                    label = "Forward"
                )
            }
            Action::ResetBackward(ticks) => {
                let count = format!("x{ticks}");

                format!(
                    "{label:>label_width$}{count:>count_width$}",
                    label = "Backward"
                )
            }
        };

        let values = format!("{values:?}", values = counter.values());
        println!("{description:<10}{values:>width$}", width = 3 * N + 3);

        was_last_reset = matches!(action, Action::ResetForward(_) | Action::ResetBackward(_));
    }
}

fn normalize_actions(actions: impl IntoIterator<Item = Action>) -> Vec<Action> {
    let mut result = Vec::new();

    for action in actions {
        match (result.last_mut(), action) {
            (Some(Action::Increment(a)), Action::Increment(b)) => {
                *a += b;
            }

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

fn perform_action<const N: usize>(counter: &mut TallyCounter<N>, action: Action) {
    match action {
        Action::Increment(ticks) => {
            counter.increment_by(ticks);
        }

        Action::ResetForward(ticks) => {
            counter.reset_forward_by(ticks);
        }

        Action::ResetBackward(ticks) => {
            counter.reset_backward_by(ticks);
        }
    }
}
