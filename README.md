# Tally Counter Puzzle Solver

Solve the puzzle from [OskarPuzzle's video](https://youtu.be/AT9wAQSV5_4?si=Na-anyUZgdwYqE9G)
with the fewest increments, then the fewest reset ticks.

## CLI

Run the solver with a target value:

```sh
cargo run -- 12
```

The counter starts at zero. Each row shows its value after the action:

```text
Minimum increments: 1
Reset ticks: 1

┌───────────┬───────────┬───────┬───────┬─────────────┐
│ Action    ┆ Direction ┆ Count ┆ Value ┆ Reset index │
╞═══════════╪═══════════╪═══════╪═══════╪═════════════╡
│ Start     ┆           ┆     - ┆    00 ┆           0 │
│ Reset     ┆   Forward ┆    x1 ┆    11 ┆           1 │
│ Increment ┆           ┆    x1 ┆    12 ┆           1 │
└───────────┴───────────┴───────┴───────┴─────────────┘
```

Leading zeros set the counter width. You can also choose the starting value and
reset position, or view all options:

```sh
cargo run -- 0012 --start 0009 --reset-index 9
cargo run -- --help
```

## Library

Find a sequence and apply it to a counter:

```rust
use tally_problem::{SearchResult, TallyCounter, search};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut counter = TallyCounter::new(2)?;
    let target = vec![1, 2];

    if let SearchResult::Found(actions) = search(&counter, &target)? {
        for action in actions {
            action.apply(&mut counter);
        }

        println!("{:?}", counter.values()); // [1, 2]
    }

    Ok(())
}
```
