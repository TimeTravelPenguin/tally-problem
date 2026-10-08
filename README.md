# Tally Counter Puzzle Solver

This repo solves the puzzle presented by [OskarPuzzle's video][OskarPuzzle],
for any size tally counter, and for any target value.

The implementation uses Dijkstra's algorithm to minimise the number of reset movements
after minimising the total increments.

[OskarPuzzle]: https://youtu.be/AT9wAQSV5_4?si=Na-anyUZgdwYqE9G

In the output of the program, a `Forward Reset` is a turn of the reset knob in the direction
that will adjust the displayed digits towards an all-zero value. A `Backward Reset` is a
turn in the opposite direction, such that the wratchet mechanism does not engage. An
`Increment` is a single press of the counter button.

## Example

The output of the program for a 4-digit counter, with a target of `9876` is:

```
Minimum increments: 6
Reset
      Forward   x8   [8, 8, 8, 8]
     Backward   x9   [8, 8, 8, 8]
Increment       x2   [8, 8, 9, 0]
Reset
      Forward   x2   [8, 8, 1, 1]
     Backward   x3   [8, 8, 1, 1]
      Forward   x2   [0, 0, 1, 1]
     Backward   x9   [0, 0, 1, 1]
      Forward   x8   [0, 0, 9, 9]
     Backward   x8   [0, 0, 9, 9]
Increment       x2   [0, 1, 0, 1]
Reset
      Forward   x8   [0, 9, 0, 9]
     Backward   x8   [0, 9, 0, 9]
Increment       x1   [0, 9, 1, 0]
Reset
      Forward   x2   [0, 9, 3, 0]
     Backward   x3   [0, 9, 3, 0]
      Forward   x2   [2, 9, 3, 2]
     Backward   x3   [2, 9, 3, 2]
      Forward   x2   [2, 1, 3, 2]
     Backward   x8   [2, 1, 3, 2]
      Forward   x7   [2, 1, 0, 2]
     Backward   x8   [2, 1, 0, 2]
      Forward   x7   [9, 1, 0, 9]
     Backward   x8   [9, 1, 0, 9]
      Forward   x7   [9, 8, 0, 9]
     Backward   x8   [9, 8, 0, 9]
      Forward   x6   [9, 8, 6, 9]
     Backward   x6   [9, 8, 6, 9]
Increment       x1   [9, 8, 7, 0]
Reset
      Forward   x6   [9, 8, 7, 6]
```
