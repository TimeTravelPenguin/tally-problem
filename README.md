# Tally Puzzle Optimizer

[Try the solver here](https://timetravelpenguin.github.io/tally-problem/) · Inspired by [OskarPuzzle's video][video]

Reach a chosen value on a mechanical tally counter using increments and turns of the reset
knob. Forward turns can move the digit wheels; backward turns reposition the knob without
changing the displayed value, but it repositions the reset index internally (see the video
below for a physical intuition). Find the fewest increments, then the fewest reset ticks.

<details>

<summary>A longer example</summary>

Below is an example of running the CLI app to solve for the target value `9876`, as
proposed in [OskarPuzzle's video][video].

```sh
cargo run -- 9876
```

Gives the result:

```
Minimum increments: 6
Reset ticks: 148

┌───────────┬───────────┬───────┬───────┬─────────────┐
│ Action    ┆ Direction ┆ Count ┆ Value ┆ Reset index │
╞═══════════╪═══════════╪═══════╪═══════╪═════════════╡
│ Start     ┆           ┆     - ┆  0000 ┆           0 │
│ Reset     ┆   Forward ┆    x8 ┆  8888 ┆           8 │
│ Increment ┆           ┆    x2 ┆  8890 ┆           8 │
│ Reset     ┆  Backward ┆    x9 ┆  8890 ┆           9 │
│           ┆   Forward ┆    x8 ┆  8877 ┆           7 │
│           ┆  Backward ┆    x9 ┆  8877 ┆           8 │
│           ┆   Forward ┆    x2 ┆  0077 ┆           0 │
│           ┆  Backward ┆    x3 ┆  0077 ┆           7 │
│           ┆   Forward ┆    x2 ┆  0099 ┆           9 │
│ Increment ┆           ┆    x2 ┆  0101 ┆           9 │
│ Reset     ┆  Backward ┆    x8 ┆  0101 ┆           1 │
│           ┆   Forward ┆    x8 ┆  0909 ┆           9 │
│ Increment ┆           ┆    x1 ┆  0910 ┆           9 │
│ Reset     ┆  Backward ┆    x8 ┆  0910 ┆           1 │
│           ┆   Forward ┆    x7 ┆  0980 ┆           8 │
│           ┆  Backward ┆    x8 ┆  0980 ┆           0 │
│           ┆   Forward ┆    x7 ┆  7987 ┆           7 │
│           ┆  Backward ┆    x8 ┆  7987 ┆           9 │
│           ┆   Forward ┆    x7 ┆  7687 ┆           6 │
│           ┆  Backward ┆    x8 ┆  7687 ┆           8 │
│           ┆   Forward ┆    x7 ┆  7657 ┆           5 │
│           ┆  Backward ┆    x8 ┆  7657 ┆           7 │
│           ┆   Forward ┆    x2 ┆  9659 ┆           9 │
│           ┆  Backward ┆    x3 ┆  9659 ┆           6 │
│           ┆   Forward ┆    x1 ┆  9759 ┆           7 │
│ Increment ┆           ┆    x1 ┆  9760 ┆           7 │
│ Reset     ┆   Forward ┆    x1 ┆  9860 ┆           8 │
│           ┆  Backward ┆    x2 ┆  9860 ┆           6 │
│           ┆   Forward ┆    x1 ┆  9870 ┆           7 │
│           ┆  Backward ┆    x7 ┆  9870 ┆           0 │
│           ┆   Forward ┆    x6 ┆  9876 ┆           6 │
└───────────┴───────────┴───────┴───────┴─────────────┘
```

</details>

## How the Solver Works

The solver searches backwards from the target using Dijkstra's shortest-path
method. It prioritizes the fewest increments, then the fewest reset ticks, so
the returned sequence is optimal for both goals.

Rather than storing every possible counter value, it shares groups of digit
patterns in compact decision diagrams and frees unused patterns along the way.
Once it finds a solution, it reconstructs the steps and groups consecutive
actions for display. There is no fixed digit limit, though difficult patterns
can still take a long time.

The interface shows elapsed time and the number of increments currently being
checked. When measured search timings support a useful prediction, it also shows
an estimated percentage and remaining time. Estimates can adjust as more work is
discovered; uncertain cases show an activity bar instead. 100% means the search
has finished.

## Use as a Library

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

## Building

### CLI

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

Use `just build-cli` to build the release executable in `target/release/`.

### Desktop UI

Build and run the native desktop interface:

```sh
just build-desktop
just run-desktop
```

The executable is written to `target/release/tally-gui` (`tally-gui.exe` on Windows).

### Browser UI

To run the web build locally, install [Trunk](https://trunk-rs.github.io/trunk/) and run:

```sh
rustup target add wasm32-unknown-unknown
trunk serve
```

Open `http://127.0.0.1:8080`.

Alternatively, build the release website into `dist/`, optionally choosing its hosting
path:

```sh
just build-web
just build-web /tally-problem/
```

## Acknowledgement of AI Usage

Parts of this project used OpenAI's GPT 6.1 Sol with Ultra "effort".

The core solver and CLI were initially all my own work, using Dijkstra's algorithm to
optimise finding solutions. However, as I grew more curious and required further
optimisation, I relied on AI to quickly iterate new ideas and implementations. So, while
the original work is _mostly_ my own, with AI mostly being for debugging or asking for
asking about code optimisation, the current state of the project is heavily altered by AI,
under my supervision and guidance.

The Iced GUI & Web application was heavily "vibe coded". My primary input was to
explicitly outline what I wanted, providing feedback throughout. Note that since I have
moderate experience creating Iced applications, the majority of work supervised was almost
explicitly instructed.

I have never really let AI build an entire Iced application, preferring to do it myself.
It is a little bit messy, but it did a relatively decent job—though, I still prefer to do
it myself!

[video]: https://www.youtube.com/watch?v=AT9wAQSV5_4
