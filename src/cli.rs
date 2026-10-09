use clap::Parser;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// The target value to reach, represented as a string of digits (e.g., "9876").
    pub target: String,
}
