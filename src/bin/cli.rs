//! Command-line entry point for solving and displaying a tally puzzle.
//!
//! The target's digits define the counter width, including leading zeros.
//! Optional starting digits and a reset index configure its initial state.
//! The CLI runs the library solver, prints the two optimization costs, and
//! replays grouped actions into a table of resulting values and reset indices.

use std::{error::Error, process::ExitCode, str::FromStr};

use clap::Parser;
use comfy_table::{Cell, CellAlignment, Table, presets::UTF8_FULL_CONDENSED};
use tally_problem::{Action, SearchResult, TallyCounter, search};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Find a tally counter sequence minimizing increments, then reset ticks"
)]
struct Cli {
    /// Target digits, including any leading zeros (e.g. 9876 or 0012)
    #[arg(value_name = "TARGET")]
    target: Digits,

    /// Initial digits; defaults to zeros with the same width as TARGET
    #[arg(long, value_name = "DIGITS")]
    start: Option<Digits>,

    /// Initial reset knob position
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=9))]
    reset_index: u8,
}

/// Parsed decimal digits that retain the supplied width and leading zeros.
#[derive(Debug, Clone)]
struct Digits(Vec<u8>);

impl FromStr for Digits {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() || !value.bytes().all(|digit| digit.is_ascii_digit()) {
            return Err("expected a nonempty string of digits in the range 0-9".to_owned());
        }

        Ok(Self(value.bytes().map(|digit| digit - b'0').collect()))
    }
}

/// Parse CLI arguments and report runtime errors with a failing exit code.
fn main() -> ExitCode {
    let cli = Cli::parse();

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");

            ExitCode::FAILURE
        }
    }
}

/// Configure the counter, solve it, and print costs followed by the replay table.
fn run(cli: Cli) -> Result<(), Box<dyn Error>> {
    let target = cli.target.0;
    let mut counter = TallyCounter::new(target.len())?;

    if let Some(start) = cli.start {
        counter.set_values(start.0)?;
    }

    counter.set_reset_index(cli.reset_index)?;

    match search(&counter, &target)? {
        SearchResult::Found(actions) => {
            let increments: u64 = actions
                .iter()
                .filter_map(|action| match action {
                    Action::Increment(ticks) => Some(*ticks),
                    _ => None,
                })
                .sum();

            let reset_ticks: u64 = actions
                .iter()
                .filter_map(|action| match action {
                    Action::ResetForward(ticks) | Action::ResetBackward(ticks) => Some(*ticks),
                    _ => None,
                })
                .sum();

            println!("Minimum increments: {increments}");
            println!("Reset ticks: {reset_ticks}");
            println!();
            print_actions(&counter, &actions);
        }

        SearchResult::NotFound => println!("Target is unreachable."),
    }

    Ok(())
}

/// Replay each grouped action and print the counter state after that action.
///
/// Consecutive resets share a label while keeping their directions on separate
/// aligned rows, making changes of direction easier to follow.
fn print_actions(initial: &TallyCounter, actions: &[Action]) {
    let mut counter = initial.clone();
    let mut table = Table::new();
    table
        .load_style(UTF8_FULL_CONDENSED)
        .set_header(["Action", "Direction", "Count", "Value", "Reset index"])
        .add_row([
            Cell::new("Start"),
            Cell::new(""),
            Cell::new("-"),
            Cell::new(format_values(counter.values())),
            Cell::new(counter.reset_index()),
        ]);

    let mut was_last_reset = false;

    for &action in actions {
        action.apply(&mut counter);

        let (label, direction, ticks) = action_details(action);
        let is_reset = matches!(action, Action::ResetForward(_) | Action::ResetBackward(_));
        let label = if is_reset && was_last_reset {
            ""
        } else {
            label
        };

        table.add_row([
            Cell::new(label),
            Cell::new(direction),
            Cell::new(format!("x{ticks}")),
            Cell::new(format_values(counter.values())),
            Cell::new(counter.reset_index()),
        ]);

        was_last_reset = is_reset;
    }

    for column in table.column_iter_mut().skip(1) {
        column.set_cell_alignment(CellAlignment::Right);
    }

    println!("{table}");
}

fn action_details(action: Action) -> (&'static str, &'static str, u64) {
    match action {
        Action::Increment(ticks) => ("Increment", "", ticks),
        Action::ResetForward(ticks) => ("Reset", "Forward", ticks),
        Action::ResetBackward(ticks) => ("Reset", "Backward", ticks),
    }
}

/// Format the display without losing any leading zeros.
fn format_values(values: &[u8]) -> String {
    values
        .iter()
        .map(|&digit| char::from(b'0' + digit))
        .collect()
}
