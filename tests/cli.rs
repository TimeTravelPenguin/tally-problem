#![cfg(feature = "cli")]

use std::process::{Command, Output};

use tally_problem::{Action, TallyCounter};

fn run_cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tally-problem"))
        .args(args)
        .output()
        .expect("CLI should start")
}

fn successful_output(args: &[&str]) -> String {
    let output = run_cli(args);

    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).unwrap()
}

fn error_output(args: &[&str]) -> String {
    let output = run_cli(args);

    assert!(!output.status.success(), "invalid input should fail");

    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(stderr.contains("error:"), "missing error message: {stderr}");
    assert!(!stderr.contains("panicked"), "CLI panicked: {stderr}");

    stderr
}

fn table_rows(output: &str) -> Vec<[&str; 5]> {
    output
        .lines()
        .filter_map(|line| {
            let cells: Vec<_> = line.split(['|', '│', '┆']).map(str::trim).collect();
            let row = match cells.as_slice() {
                [action, direction, count, value, reset_index] => {
                    [*action, *direction, *count, *value, *reset_index]
                }

                [
                    left_border,
                    action,
                    direction,
                    count,
                    value,
                    reset_index,
                    right_border,
                ] if left_border.is_empty() && right_border.is_empty() => {
                    [*action, *direction, *count, *value, *reset_index]
                }

                _ => return None,
            };

            if row[2] == "-" || row[2].starts_with('x') {
                Some(row)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn help_and_version_describe_the_command() {
    let help = successful_output(&["--help"]);

    assert!(help.contains("TARGET"));
    assert!(help.contains("--start"));
    assert!(help.contains("--reset-index"));

    let version = successful_output(&["--version"]);

    assert!(version.contains("tally-problem"));
    assert!(version.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn default_start_reports_total_ticks_and_grouped_increments() {
    let output = successful_output(&["890"]);

    assert!(output.contains("Minimum increments: 2"));
    assert!(output.contains("Reset ticks: 8"));

    let rows = table_rows(&output);
    let start = rows.iter().find(|row| row[0] == "Start").unwrap();
    let increments: Vec<_> = rows.iter().filter(|row| row[0] == "Increment").collect();

    assert_eq!(*start, ["Start", "", "-", "000", "0"]);
    assert_eq!(increments.len(), 1);
    assert_eq!(*increments[0], ["Increment", "", "x2", "890", "8"]);
}

#[test]
fn explicit_start_and_reset_position_preserve_leading_zeros() {
    let output = successful_output(&["001", "--start", "000", "--reset-index", "4"]);

    assert!(output.contains("Minimum increments: 1"));
    assert!(output.contains("Reset ticks: 0"));

    let rows = table_rows(&output);
    let start = rows.iter().find(|row| row[0] == "Start").unwrap();
    let increment = rows.iter().find(|row| row[0] == "Increment").unwrap();

    assert_eq!(*start, ["Start", "", "-", "000", "4"]);
    assert_eq!(*increment, ["Increment", "", "x1", "001", "4"]);
}

#[test]
fn consecutive_resets_share_a_label_and_report_each_resulting_state() {
    let output = successful_output(&["9876"]);
    let rows = table_rows(&output);

    assert_eq!(rows[0], ["Start", "", "-", "0000", "0"]);
    assert!(output.contains("Direction"));

    let mut counter = TallyCounter::new(4).unwrap();
    let mut was_reset = false;
    let mut reset_groups = 0;
    let mut reset_continuations = 0;
    let mut increments = 0;
    let mut reset_ticks = 0;

    for row in &rows[1..] {
        let ticks: u64 = row[2].strip_prefix('x').unwrap().parse().unwrap();
        let action = match row[1] {
            "" => {
                assert_eq!(row[0], "Increment");
                increments += ticks;
                was_reset = false;

                Action::Increment(ticks)
            }

            direction @ ("Forward" | "Backward") => {
                if was_reset {
                    assert_eq!(row[0], "", "continuing resets should have an empty action");
                    reset_continuations += 1;
                } else {
                    assert_eq!(row[0], "Reset", "each reset group needs a label");
                    reset_groups += 1;
                }

                reset_ticks += ticks;
                was_reset = true;

                if direction == "Forward" {
                    Action::ResetForward(ticks)
                } else {
                    Action::ResetBackward(ticks)
                }
            }

            direction => panic!("Unexpected reset direction: {direction}"),
        };

        action.apply(&mut counter);

        let expected_value: String = counter
            .values()
            .iter()
            .map(|&digit| char::from(b'0' + digit))
            .collect();

        assert_eq!(row[3], expected_value, "incorrect displayed value: {row:?}");
        assert_eq!(row[4], counter.reset_index().to_string());
    }

    assert!(reset_groups > 1, "increments should start new reset groups");
    assert!(reset_continuations > 0, "expected consecutive reset rows");
    assert_eq!(counter.values(), &[9, 8, 7, 6]);
    assert_eq!(increments, 6);
    assert!(output.contains(&format!("Minimum increments: {increments}")));
    assert!(output.contains(&format!("Reset ticks: {reset_ticks}")));
}

#[test]
fn invalid_targets_are_rejected() {
    for target in ["1x", "１２", ""] {
        let stderr = error_output(&[target]);

        assert!(stderr.contains("TARGET"));
    }
}

#[test]
fn invalid_start_digits_are_rejected() {
    for start in ["1x", "１２", ""] {
        let stderr = error_output(&["12", "--start", start]);

        assert!(stderr.contains("--start"));
    }
}

#[test]
fn mismatched_start_width_is_rejected() {
    let stderr = error_output(&["001", "--start", "00"]);

    assert!(stderr.to_lowercase().contains("length"));
}

#[test]
fn out_of_range_reset_position_is_rejected() {
    let stderr = error_output(&["12", "--reset-index", "10"]);

    assert!(stderr.contains("--reset-index"));
}

#[test]
fn wide_counters_are_supported_without_numeric_encoding() {
    let output = successful_output(&["0000000000000000000000000000000000000001"]);

    assert!(output.contains("Minimum increments: 1"));
    assert!(output.contains("Reset ticks: 0"));
}
