# Tally Puzzle Optimizer

Reach a chosen value on a mechanical tally counter using increments and turns of
the reset knob. Forward turns can move the digit wheels; backward turns reposition
the knob without changing the displayed value. Find the fewest increments, then
the fewest reset ticks.

Inspired by [OskarPuzzle's video](https://www.youtube.com/watch?v=AT9wAQSV5_4).

## Browser UI

The Iced interface uses Catppuccin Mocha, with validated inputs and a scrollable
sequence of steps. Hover over an input's help marker for an explanation.
Leading zeros determine the counter width. Searches run in the background and can
be cancelled without freezing the interface.
Use Tab / Shift + Tab to move between controls and Enter to solve; Page Up / Page
Down scroll the results.

To run it locally, install [Trunk](https://trunk-rs.github.io/trunk/) and run:

```sh
rustup target add wasm32-unknown-unknown
trunk serve
```

Open `http://127.0.0.1:8080`. For GitHub Pages, set the repository's Pages source to
**GitHub Actions**. The included workflow builds and deploys when you push to `main`.

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
reset index, or view all options:

```sh
cargo run -- 0012 --start 0009 --reset-index 9
cargo run -- --help
```

The exact solver shares digit patterns to reduce search time and memory. There is
no fixed digit limit, though difficult targets can still take a long time.

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
