//! Form validation and display data shared by the desktop and browser GUIs.
//!
//! [`Form::prepare`] preserves decimal text, including leading zeros, while
//! converting a valid form into the library's counter and target types. After
//! the background search finishes, [`PreparedSearch::finish`] replays its
//! grouped actions to produce the values, totals, and reset groups shown by the
//! UI and sequence player. This module has no dependency on Iced widgets.

use tally_problem::{Action, SearchResult, TallyCounter};

#[cfg(test)]
use tally_problem::search;

/// Editable text kept independently of parsing so incomplete input stays visible.
#[derive(Debug, Clone)]
pub struct Form {
    /// Decimal target text; its length determines the counter width.
    pub target: String,
    /// Decimal initial digits, or an empty string to start with all zeros.
    pub start: String,
    /// A single decimal digit specifying the initial reset knob position.
    pub reset_index: String,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            target: "9876".to_owned(),
            start: String::new(),
            reset_index: "0".to_owned(),
        }
    }
}

/// Per-field messages for both inline validation and focusing the first error.
///
/// A missing message means that field is valid. Width matching is checked only
/// after the target itself is valid, avoiding misleading secondary errors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Validation {
    pub target: Option<String>,
    pub start: Option<String>,
    pub reset_index: Option<String>,
}

impl Validation {
    /// Whether all three fields are ready to prepare a search.
    pub fn is_valid(&self) -> bool {
        self.target.is_none() && self.start.is_none() && self.reset_index.is_none()
    }
}

impl Form {
    /// Validate ASCII decimal digits, matching widths, and the reset index.
    ///
    /// The target must contain at least one digit; the starting value may be
    /// blank. There is no fixed limit on the number of counter digits.
    pub fn validate(&self) -> Validation {
        let mut validation = Validation::default();

        if self.target.is_empty() {
            validation.target = Some("Enter a target value.".to_owned());
        } else if !is_ascii_digits(&self.target) {
            validation.target = Some("Use only digits from 0 to 9.".to_owned());
        }

        if !self.start.is_empty() {
            if !is_ascii_digits(&self.start) {
                validation.start = Some("Use only digits from 0 to 9, or leave blank.".to_owned());
            } else if validation.target.is_none() && self.start.len() != self.target.len() {
                validation.start = Some(format!(
                    "Enter {} digits to match the target, or leave blank.",
                    self.target.len()
                ));
            }
        }

        if self.reset_index.len() != 1 || !is_ascii_digits(&self.reset_index) {
            validation.reset_index = Some("Enter one digit from 0 to 9.".to_owned());
        }

        validation
    }

    /// Convert valid text into solver input, or return errors attached to fields.
    ///
    /// A blank starting value creates a zeroed counter of the target's width.
    /// No search runs here; the application passes the prepared data to its
    /// background solver.
    pub fn prepare(&self) -> Result<PreparedSearch, Validation> {
        let validation = self.validate();

        if !validation.is_valid() {
            return Err(validation);
        }

        let target = parse_digits(&self.target);
        let mut counter = TallyCounter::new(target.len()).map_err(|error| Validation {
            target: Some(error.to_string()),
            ..Validation::default()
        })?;

        if !self.start.is_empty() {
            counter
                .set_values(parse_digits(&self.start))
                .map_err(|error| Validation {
                    start: Some(error.to_string()),
                    ..Validation::default()
                })?;
        }

        counter
            .set_reset_index(self.reset_index.as_bytes()[0] - b'0')
            .map_err(|error| Validation {
                reset_index: Some(error.to_string()),
                ..Validation::default()
            })?;

        Ok(PreparedSearch { counter, target })
    }
}

/// Validated solver input retained until the search result can be replayed.
#[derive(Debug, Clone)]
pub struct PreparedSearch {
    pub counter: TallyCounter,
    pub target: Vec<u8>,
}

/// A solved sequence with display text and totals for the results and player.
///
/// Values retain the original counter width. Each step represents one grouped
/// action, while the player can expand its tick count incrementally.
#[derive(Debug, Clone)]
pub struct Solution {
    pub target: String,
    pub start: String,
    /// Knob position before any action, independent of the initial digit text.
    pub initial_reset_index: u8,
    pub increments: u64,
    pub reset_ticks: u64,
    pub steps: Vec<Step>,
}

/// A grouped action and the counter state after all of its ticks have been applied.
#[derive(Debug, Clone)]
pub struct Step {
    pub action: Action,
    pub value: String,
    pub reset_index: u8,
    /// True for the first reset action after an increment or at sequence start.
    /// Adjacent forward and backward resets share one displayed reset group.
    pub starts_reset_group: bool,
}

impl PreparedSearch {
    #[cfg(test)]
    pub fn solve(self) -> Result<Solution, String> {
        let result = search(&self.counter, &self.target).map_err(|error| error.to_string())?;

        self.finish(result)
    }

    /// Replay a completed search into display steps, or report an unreachable target.
    ///
    /// Applying the returned actions to the retained initial counter provides
    /// the value and reset index at every reported step, without another search.
    pub fn finish(self, result: SearchResult) -> Result<Solution, String> {
        let actions = match result {
            SearchResult::Found(actions) => actions,
            SearchResult::NotFound => return Err("This target is unreachable.".to_owned()),
        };

        let mut solution = Solution {
            target: format_values(&self.target),
            start: format_values(self.counter.values()),
            initial_reset_index: self.counter.reset_index(),
            increments: 0,
            reset_ticks: 0,
            steps: Vec::with_capacity(actions.len()),
        };

        let mut counter = self.counter;
        let mut was_last_reset = false;

        for action in actions {
            let is_reset = match action {
                Action::Increment(ticks) => {
                    solution.increments += ticks;

                    false
                }

                Action::ResetForward(ticks) | Action::ResetBackward(ticks) => {
                    solution.reset_ticks += ticks;

                    true
                }
            };

            action.apply(&mut counter);
            solution.steps.push(Step {
                action,
                value: format_values(counter.values()),
                reset_index: counter.reset_index(),
                starts_reset_group: is_reset && !was_last_reset,
            });

            was_last_reset = is_reset;
        }

        Ok(solution)
    }
}

fn is_ascii_digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|digit| digit.is_ascii_digit())
}

/// Parse text that has already passed [`Form::validate`].
fn parse_digits(value: &str) -> Vec<u8> {
    value.bytes().map(|digit| digit - b'0').collect()
}

/// Preserve every wheel, including leading zeros, in displayed digit order.
fn format_values(values: &[u8]) -> String {
    values
        .iter()
        .map(|&digit| char::from(b'0' + digit))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(target: &str, start: &str, reset_index: &str) -> Form {
        Form {
            target: target.to_owned(),
            start: start.to_owned(),
            reset_index: reset_index.to_owned(),
        }
    }

    #[test]
    fn target_rejects_empty_nondecimal_and_unicode_digits() {
        for target in ["", "1a", " 12", "１２"] {
            let validation = form(target, "", "0").validate();

            assert!(validation.target.is_some(), "accepted target {target:?}");
        }
    }

    #[test]
    fn wide_targets_prepare_with_zeroed_or_matching_initial_digits() {
        for target in ["12345", "000000012345", "12345678901234567890"] {
            let prepared = form(target, "", "0").prepare().unwrap();

            assert_eq!(prepared.counter.values(), vec![0; target.len()]);
            assert_eq!(format_values(&prepared.target), target);
        }

        let prepared = form("0000012", "0000009", "9").prepare().unwrap();

        assert_eq!(format_values(prepared.counter.values()), "0000009");
        assert_eq!(format_values(&prepared.target), "0000012");
        assert_eq!(prepared.counter.reset_index(), 9);
        assert!(form("0000012", "000009", "9").validate().start.is_some());
    }

    #[test]
    fn validates_each_field_and_prevents_preparing_invalid_input() {
        let invalid = form("12", "a", "10");
        let validation = invalid.validate();

        assert!(validation.target.is_none());
        assert!(validation.start.is_some());
        assert!(validation.reset_index.is_some());
        assert!(!invalid.prepare().unwrap_err().is_valid());
    }

    #[test]
    fn initial_digits_match_target_width_and_allow_blank() {
        for start in ["1", "123", "１２", " "] {
            assert!(form("12", start, "0").validate().start.is_some());
        }

        let prepared = form("0012", "", "9").prepare().unwrap();

        assert_eq!(prepared.counter.values(), [0, 0, 0, 0]);
        assert_eq!(prepared.target, [0, 0, 1, 2]);
        assert_eq!(prepared.counter.reset_index(), 9);
    }

    #[test]
    fn reset_index_requires_one_ascii_digit() {
        for reset_index in ["", "00", "10", "-1", "a", "９", " 0"] {
            assert!(form("12", "", reset_index).validate().reset_index.is_some());
        }

        for reset_index in ["0", "9"] {
            assert!(form("12", "", reset_index).validate().is_valid());
        }
    }

    #[test]
    fn already_displayed_target_has_no_steps() {
        let solution = form("0012", "0012", "8")
            .prepare()
            .unwrap()
            .solve()
            .unwrap();

        assert_eq!(solution.target, "0012");
        assert_eq!(solution.start, "0012");
        assert_eq!(solution.initial_reset_index, 8);
        assert_eq!(solution.increments, 0);
        assert_eq!(solution.reset_ticks, 0);
        assert!(solution.steps.is_empty());
    }

    #[test]
    fn reset_totals_count_ticks_and_replay_grouped_results() {
        let solution = form("22", "", "0").prepare().unwrap().solve().unwrap();

        assert_eq!(solution.increments, 0);
        assert_eq!(solution.reset_ticks, 2);
        assert_eq!(solution.steps.len(), 1);
        assert_eq!(solution.steps[0].action, Action::ResetForward(2));
        assert_eq!(solution.steps[0].value, "22");
        assert_eq!(solution.steps[0].reset_index, 2);
        assert!(solution.steps[0].starts_reset_group);
    }

    #[test]
    fn increments_preserve_leading_zeros_and_starting_reset_position() {
        let solution = form("010", "009", "9").prepare().unwrap().solve().unwrap();

        assert_eq!(solution.start, "009");
        assert_eq!(solution.target, "010");
        assert_eq!(solution.increments, 1);
        assert_eq!(solution.reset_ticks, 0);
        assert_eq!(solution.steps[0].value, "010");
        assert_eq!(solution.steps[0].reset_index, 9);
        assert!(!solution.steps[0].starts_reset_group);
    }

    #[test]
    fn adjacent_reset_directions_share_a_group_until_an_increment() {
        let prepared = form("44", "", "0").prepare().unwrap();
        let solution = prepared
            .finish(SearchResult::Found(vec![
                Action::ResetForward(2),
                Action::ResetBackward(1),
                Action::Increment(1),
                Action::ResetForward(3),
            ]))
            .unwrap();

        assert!(solution.steps[0].starts_reset_group);
        assert!(!solution.steps[1].starts_reset_group);
        assert!(!solution.steps[2].starts_reset_group);
        assert!(solution.steps[3].starts_reset_group);
        assert_eq!(solution.steps[3].value, "44");
        assert_eq!(solution.increments, 1);
        assert_eq!(solution.reset_ticks, 6);
    }

    #[test]
    fn unreachable_search_result_becomes_a_displayable_error() {
        let prepared = form("12", "", "0").prepare().unwrap();

        assert_eq!(
            prepared.finish(SearchResult::NotFound).unwrap_err(),
            "This target is unreachable."
        );
    }
}
