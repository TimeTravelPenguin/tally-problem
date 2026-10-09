# The Tally Puzzle: Mathematics, Bounds, Search, and Progress

> [!IMPORTANT]
> Please note that this document is an in-depth summary of the work I put into optimising
> this task. It was written by GPT 6.1 Sol.
>
> While experimenting, I was continuously bouncing ideas off AI and having it look into
> verifying claims while I worked. The results of this document are what followed.

This document defines the puzzle implemented by Tally Puzzle Optimizer, proves
the bounds used by the application, and explains its exact solver and progress
estimates. The mechanical inspiration is [OskarPuzzle's video][video]. The rules
below are the mathematical model implemented in this repository; the proofs
follow from those rules.

The central distinction is between the **cost of a solution** and the **work
needed to discover it**. An increment lower bound is a statement about every
legal solution. A runtime prediction is an empirical estimate about a particular
search on a particular computer. The application uses both, with different
meanings and different guarantees.

## Contents

- [Formal problem](#formal-problem)
- [Reset structure and reachability](#reset-structure-and-reachability)
- [A linear-time increment lower bound](#a-linear-time-increment-lower-bound)
- [Mandatory final increments](#mandatory-final-increments)
- [Worst-case increment bounds](#worst-case-increment-bounds)
- [The exact solver](#the-exact-solver)
- [Progress and timing estimates](#progress-and-timing-estimates)
- [Verification and implementation map](#verification-and-implementation-map)
- [References](#references)

## Formal problem

### Displays, width, and internal state

Let the counter width be $`n \ge 1`$, and let the decimal digit alphabet be

```math
\mathcal D=\{0,1,\ldots,9\}.
```

A display is a fixed-width vector

```math
x=(x_0,x_1,\ldots,x_{n-1})\in\mathcal D^n,
```

ordered from most significant to least significant. Its numeric value is

```math
V(x)=\sum_{j=0}^{n-1}x_j10^{n-1-j},
\qquad M=10^n.
```

For a digit $`d\in\mathcal D`$, word notation $`d^n`$ denotes the uniform display
$`(d,d,\ldots,d)`$ with $`n`$ repetitions.

Leading zeros are part of the display and its width. For example,
$`\mathtt{0012}`$ is a different-width puzzle from $`\mathtt{12}`$.
The implementation stores digit vectors directly; this numeric notation does
not require the complete value to fit a machine integer.

The complete state also includes a reset index:

```math
z=(x,q),\qquad q\in\mathbb Z_{10}.
```

The reset index is an internal knob position. It is neither an extra displayed
counter digit nor a count of operations. Two states can have identical displays
and different future reset behavior.

### Legal operations

An increment adds one to the display, carries from the right, and wraps at its
width. It preserves the reset index:

```math
\mathrm{Inc}(x,q)=
\left(\mathrm{digits}_n((V(x)+1)\bmod M),q\right).
```

Here $`\mathrm{digits}_n`$ includes leading zeros. Thus the display after
incrementing $`\mathtt{9999}`$ is $`\mathtt{0000}`$.

For a forward reset tick, define the digit map

```math
\phi_q(d)=
\begin{cases}
(q+1)\bmod 10,&d=q,\\
d,&d\ne q.
\end{cases}
```

Apply this same map to every wheel:

```math
F_q(x)=\bigl(\phi_q(x_0),\ldots,\phi_q(x_{n-1})\bigr).
```

The complete forward transition is

```math
\mathrm{Forward}(x,q)=\bigl(F_q(x),(q+1)\bmod 10\bigr).
```

Wheels equal to the **old** reset index move to its successor. Other wheels stay
where they are. A backward tick moves only the knob:

```math
\mathrm{Backward}(x,q)=\bigl(x,(q-1)\bmod 10\bigr).
```

For example,

```math
(\mathtt{0077},0)
\xrightarrow{\mathrm{Forward}}
(\mathtt{1177},1)
\xrightarrow{\mathrm{Backward}}
(\mathtt{1177},0).
```

### Grouped operations and modular arithmetic

Grouped increments have the same fixed-width meaning as repeated button presses:

```math
\mathrm{Inc}^k(x,q)=
\left(\mathrm{digits}_n((V(x)+k)\bmod M),q\right).
```

Backward turns can be reduced modulo ten without affecting the display:

```math
\mathrm{Backward}^k(x,q)=\bigl(x,(q-k)\bmod10\bigr).
```

For grouped forward turns, let $`q'=(q+k)\bmod10`$. Before a full revolution,
$`0\le k\lt 10`$, a wheel moves exactly when its old value lies in the swept arc:

```math
\mathrm{Forward}^k(x,q)=(y,q'),\qquad
y_j=
\begin{cases}
q',&(x_j-q)\bmod10\lt k,\\
x_j,&\text{otherwise}.
\end{cases}
```

After a full revolution, all wheels have been gathered and move with the knob,
so for $`k\ge10`$,

```math
\mathrm{Forward}^k(x,q)=\bigl((q')^n,q'\bigr).
```

These formulas let `TallyCounter` apply counted operations without iterating
every tick. They also show why the knob index alone is insufficient for
normalizing arbitrary forward/backward sequences. In particular,

```math
\mathrm{Forward}^{10}(x,q)=(q^n,q)
```

can change the display even though the index returns to its original value.
A forward tick followed by a backward tick gives $`(F_q(x),q)`$, which is
generally not the original state. A reverse-search preimage is consequently not
the same operation as turning the knob backwards.

There are still some exact reductions within an uninterrupted group. A backward
group of at least ten ticks can lose a full revolution. A forward group of at
least twenty ticks can lose a full revolution while retaining its gathering
effect. Hence an optimal sequence needs no backward group longer than nine
ticks or forward group longer than nineteen ticks. The solver finds the optimum
through state costs and then groups its witness; it does not assume that all
mixed reset sequences can be simplified by index arithmetic alone.

### Optimization objective

The input specifies an initial display $`s`$, its reset index $`q_s`$, and a target
display $`t`$ of the same width. The final reset index is unconstrained:

```math
\mathcal T=\{(t,q):q\in\mathbb Z_{10}\}.
```

For a legal sequence $`\pi`$, let $`i(\pi)`$ count increments and $`r(\pi)`$ count
individual forward and backward reset ticks. Its cost is

```math
C(\pi)=(i(\pi),r(\pi)).
```

The solver minimizes this pair in lexicographic order:

```math
(i_1,r_1)\lt _{\mathrm{lex}}(i_2,r_2)
\iff i_1\lt i_2\ \text{or}\ (i_1=i_2\ \text{and}\ r_1\lt r_2).
```

Thus increments have absolute priority. Among all sequences with the fewest
increments, the solver chooses one with the fewest total reset ticks.
The edge costs are

```math
w_{\mathrm{Inc}}=(1,0),\qquad
w_{\mathrm{Forward}}=w_{\mathrm{Backward}}=(0,1).
```

Grouping consecutive operations changes presentation, not cost. An
`Increment(k)` action represents $`k`$ individual increments; analogous reset
actions represent $`k`$ knob ticks. Minimizing the number of printed rows or
changes of direction is a different objective and is not used here.

Write $`C^*_{q_s}(s,t)`$ for the optimal pair. Write $`c(s,t)`$ for its primary
increment count. The latter does not depend on the initial reset index, as shown
below, but the secondary reset cost generally does.

## Reset structure and reachability

### Equality cannot be split by resets

Every forward reset applies one common digit map to all positions. Therefore

```math
x_j=x_k\ \Longrightarrow\ \phi_q(x_j)=\phi_q(x_k).
```

Backward resets preserve the entire display. Any sequence consisting only of
resets can merge distinct wheel values, but cannot separate equal ones. This
applies to every pair of positions, including neighboring wheels.

Increments break this symmetry because decimal carry acts differently on
different positions. That distinction is the basis of the increment potential
proved in the next section.

### A forward tick always leaves a digit missing

The image of $`\phi_q`$ omits its old index:

```math
q\notin\phi_q(\mathcal D).
```

Existing occurrences of $`q`$ move away, and no other digit maps to $`q`$.
Consequently, no display containing all decimal digits can be the immediate
result of a forward tick. This holds even for a tick that changes no wheel:
such a tick starts from a display already missing $`q`$.

Backward ticks do not change the display, so they can occur while all digits are
present. They cannot create such a display from a different one.

### Uniform displays and arbitrary initial knob positions

A full forward revolution gathers all wheels at the final knob index.
Starting at $`q`$, after ten forward ticks the display is uniformly $`q`$ and the
knob has returned to $`q`$. Each wheel is engaged when the knob passes its original
digit and then moves with every remaining tick, which proves this gathering
property for every starting digit. Continue by

```math
\delta=(d-q)\bmod 10
```

ticks to obtain the uniform display $`d^n`$. This construction uses at most
$`10+9=19`$ forward ticks and no increments, from any initial display.

In particular, every starting state can reach the zero display using resets
alone. The mathematical construction uses legal knob turns; it does not use the
library's separate `hard_reset` convenience method.

Backward ticks can also reposition the knob to any chosen index without changing
the display, using at most nine ticks. When only increment cost is considered,
knob repositioning is free. A solution available from one initial knob position
can therefore be used from any other with the same number of increments. This
proves that $`c(s,t)`$ is independent of $`q_s`$.

### A constructive upper bound for an individual puzzle

Let

```math
U_n=\underbrace{11\ldots1}_{n\ \mathrm{digits}}
=\frac{10^n-1}{9},\qquad T=V(t).
```

Choose

```math
d=\left\lfloor\frac{T}{U_n}\right\rfloor,\qquad
k=T-dU_n.
```

Because $`0\le T\le 9U_n`$, the uniform digit satisfies $`d\in\mathcal D`$.
Reach $`d^n`$ by resets, then increment $`k`$ times. The final display has value
$`dU_n+k=T`$, with

```math
0\le k\le U_n-1.
```

Direct increments from the initial display also give a legal solution with

```math
D=(V(t)-V(s))\bmod M.
```

Hence

```math
0\le c(s,t)\le\min(D,k)\le\frac{10^n-10}{9}.
```

For $`t=\mathtt{9876}`$, the uniform construction uses
$`U_4=1111`$, $`d=8`$, and $`k=988`$. This is a reachability proof and an upper bound,
not an optimal solution: the exact solver reaches that target from zeros using
only six increments.

The current solver does not use this uniform construction as its returned
answer, a queue priority, or a timing prior. It is useful as a feasible incumbent
for a future branch-and-bound method, provided both cost objectives are handled
correctly.

## A linear-time increment lower bound

### Weighted unequal boundaries

For each neighboring pair, define an inequality indicator and a weight:

```math
e_j(x)=
\begin{cases}
1,&x_j\ne x_{j+1},\\
0,&x_j=x_{j+1},
\end{cases}
\qquad w_j=n-j-1,
\qquad 0\le j\le n-2.
```

An unequal boundary near the left has more weight because producing it involves
carry through more positions to its right. We cannot simply add every boundary:
adjacent boundaries share a wheel and can share the same work.

Instead, choose a set of boundaries with no shared wheel. Define

```math
B(x)=\max\left\{
\sum_{j\in A}w_j:
A\subseteq\{0,\ldots,n-2\},\quad
e_j(x)=1\ \text{for every}\ j\in A,\quad
|j-k|\ge2\ \text{for distinct}\ j,k\in A
\right\}.
```

This is the maximum-weight matching of available edges in a path. For
$`x=\mathtt{9876}`$, the available weights are $`3,2,1`$; choosing the first and last
gives $`B(x)=4`$.

### Linear-time computation

Let $`P_j`$ be the best score using boundaries from position $`j`$ onward, with

```math
P_{n-1}=P_n=0.
```

The backward recurrence is

```math
P_j=
\begin{cases}
\max(P_{j+1},w_j+P_{j+2}),&e_j(x)=1,\\
P_{j+1},&e_j(x)=0.
\end{cases}
\qquad B(x)=P_0.
```

The alternatives skip the boundary or select it and skip its neighbor.
Only the next two scores are needed, giving $`\mathcal O(n)`$ time and
$`\mathcal O(1)`$ auxiliary score storage. The implementation uses `u128` for
absolute scores and converts their difference to `u64` only afterwards. This
avoids falsely rejecting a small difference between large scores and avoids
width-dependent overflow in a browser's narrower address space.

### Proof that resets cannot increase the score

A reset cannot turn an equal pair into an unequal one. Thus the set of available
boundaries after a reset is a subset of the previous set. Every matching
available afterwards was available before with the same weights, proving

```math
B(\mathrm{Forward}(x,q)_{\mathrm{display}})\le B(x),
\qquad B(\mathrm{Backward}(x,q)_{\mathrm{display}})=B(x).
```

### Proof that one increment increases the score by at most one

Consider the carry pattern of an increment.

**No carry:** only the least significant digit changes. The only boundary whose
availability can change has weight one, so the best matching can gain at most
one.

**A nonempty suffix of nines:** let $`j`$ be the rightmost position whose digit is
not nine. It increases by one, and all positions after it change from nine to
zero. Every boundary strictly inside that suffix is equal before and after.
The boundary immediately after $`j`$ is unequal before and after, since
$`x_j\in\{0,\ldots,8\}`$ and $`x_j+1\in\{1,\ldots,9\}`$.

The only possibly newly available boundary is immediately before $`j`$. If an
optimal new matching does not use that boundary, it was already available
before. If it does, replace that boundary by the old boundary immediately after
$`j`$. This replacement is valid: the new matching could not contain the adjacent
edge to its left, and there are no available edges farther right inside the
equal suffix that could conflict with the replacement. The weights differ by
exactly one:

```math
w_{j-1}-w_j=(n-j)-(n-j-1)=1.
```

The old display therefore had a matching with score at least the new optimum
minus one. If $`j`$ is the first position, there is no preceding boundary to add.

**Full wraparound:** all nines become all zeros, so both scores are zero.

Together these cases prove

```math
B(\mathrm{Inc}(x,q)_{\mathrm{display}})\le B(x)+1.
```

### The bound and its consistency

Along a solution with $`i`$ increments, resets never increase the score and each
increment increases it by at most one. Therefore

```math
B(t)\le B(s)+i,
\qquad
h_B(s,t)=\max(0,B(t)-B(s))\le c(s,t).
```

This is an admissible lower bound from **any** initial display. It is not
necessary to start from zeros. It can be weak when the initial display already
has substantial boundary structure.

For a fixed target, the forward-search heuristic

```math
h_t(x)=\max(0,B(t)-B(x))
```

satisfies the consistency inequality

```math
h_t(x)\le w_I(a)+h_t(a(x)),
```

where $`w_I(a)`$ is the increment component of the action's cost.
For reverse search towards a fixed initial display, the corresponding heuristic
is $`h_s(x)=\max(0,B(x)-B(s))`$ and satisfies the analogous inequality on inverse
edges. These facts make the potential suitable for a correctly designed A*
search over concrete states. They do not, by themselves, justify changing the
settling rules of the grouped solver described later.

Examples from zero displays:

| Target          | Guaranteed minimum $`h_B`$ | Exact minimum increments |
| --------------- | -----------------------: | -----------------------: |
| $`\mathtt{1000}`$ |                      $`3`$ |                      $`3`$ |
| $`\mathtt{0101}`$ |                      $`4`$ |                      $`4`$ |
| $`\mathtt{9876}`$ |                      $`4`$ |                      $`6`$ |

The final row illustrates the difference between a proved underestimate and an
exact answer.

## Mandatory final increments

### The last display missing a digit

Say that a display is complete when every decimal digit occurs at least once,
including leading zeros. Define

```math
\mathrm{Complete}(x)
\iff\{x_0,\ldots,x_{n-1}\}=\mathcal D.
```

Suppose the target is complete. Set

```math
U=\max\left\{v\in\{0,\ldots,T\}:
\neg\mathrm{Complete}(\mathrm{digits}_n(v))\right\},
\qquad u=\mathrm{digits}_n(U),
\qquad H=T-U.
```

The set is nonempty because the zero display is missing other digits. The target
itself is complete, so $`U\lt T`$ and $`H\gt 0`$. By maximality, every value in the interval

```math
U+1,U+2,\ldots,T
```

has a complete display. None can be entered by a forward reset. Backward resets
can change the knob but not move between display values. Thus entering and
crossing this whole interval forces consecutive increments from $`u`$ to $`t`$.

This interval is a normal numeric interval without wraparound. A path that wraps
earlier still has to pass through $`u`$ on its final approach to the target.

### Finding the predecessor without decrementing repeatedly

For each possible excluded digit $`d\in\mathcal D`$, find the largest fixed-width
display at or below the target that avoids $`d`$:

1. Keep the longest possible target prefix up to its first occurrence of $`d`$.
2. Lower that position to the largest allowed smaller digit. If impossible,
   backtrack to the nearest earlier position that can be lowered legally.
3. Fill the remaining suffix with the largest allowed digit: nine, except that
   excluding nine requires eight.
4. If no position can be lowered, this excluded digit has no feasible predecessor
   at the specified width and value range. Skip it.
5. Choose the largest candidate across all excluded digits.

The target prefix before its first occurrence of $`d`$ already avoids $`d`$.
Keeping as much of it as possible maximizes the candidate lexicographically;
lowering the chosen position by as little as possible and maximizing the suffix
then gives the greatest candidate for that excluded digit. Every display missing
a digit belongs to at least one of these ten cases, so maximizing over the cases
gives exactly $`u`$.

For equal widths, lexicographic comparison of digit vectors agrees with numeric
comparison. This procedure therefore needs no full-value integer encoding. It
takes $`\mathcal O(10n)=\mathcal O(n)`$ digit operations and $`\mathcal O(n)`$
additional storage. Decimal subtraction computes $`H`$ directly, rather than
decrementing through the interval.

### Exact decomposition, including both cost objectives

There are two cases for the initial value $`S=V(s)`$.

**The initial display lies in the interval:** if $`U\le S\le T`$, direct increments
reach the target with cost

```math
C^*_{q_s}(s,t)=(T-S,0).
```

To see optimality, read any successful path backwards from the target. Display
changes while the display is complete must be inverse increments. A path must
therefore encounter $`s`$ after at least $`T-S`$ increments, unless it continues
back to $`u`$, which would require at least $`H\ge T-S`$ increments. Direct
increments attain the bound with no resets. This also covers $`s=u`$ and $`s=t`$.

**The initial display lies outside the interval:** every successful path must
pass through $`u`$ before its final increment stretch. Backward reset ticks in
that stretch do not affect the display and can be omitted, since the final knob
position is unrestricted. Therefore, writing the optimal cost to $`u`$ as
$`C^*_{q_s}(s,u)=(i_u,r_u)`$,

```math
C^*_{q_s}(s,t)=(i_u+H,r_u).
```

Every path has a prefix to some state with display $`u`$, whose cost is at least
the optimal cost to the unconstrained-knob target $`u`$. Appending exactly $`H`$
increments to an optimal such prefix attains that lower bound. Adding the same
constant increment cost preserves lexicographic comparisons, so the reduction
preserves the secondary reset optimum as well as the primary increment optimum.

The implementation solves the reduced target $`u`$, then appends one grouped
increment action. If the previous action was also an increment, it merges the
counts. It keeps the **original initial reset index** throughout the prefix
search.

### A stronger per-puzzle lower bound

For a complete target, the implemented lower bound is

```math
h(s,t)=
\begin{cases}
T-S,&U\le S\le T,\\
H+\max(0,B(u)-B(s)),&S\lt U\ \text{or}\ S\gt T.
\end{cases}
```

For a target that is not complete, the bound is simply $`h_B(s,t)`$.
Outside the interval, the decomposition forces both the prefix's potential
increase and the full final increment stretch. Inside it, the direct distance
is the exact optimum, not just a lower bound.

An equivalent complete-target formulation is

```math
h(s,t)=\min\left(D,H+h_B(s,u)\right),
\qquad D=(T-S)\bmod M.
```

Outside the interval, the direct increment path must also visit $`u`$, and its
prefix must pay at least the potential increase. Inside the interval,
$`D=T-S\le H`$ and $`B(s)\ge B(u)`$ is not required: the minimum still selects the
direct distance. The piecewise implementation avoids representing the full
modulus and avoids overflowing an unused, potentially enormous $`H`$ for an
initial display very close to the target.

### Worked examples and computational benefit

From twelve zeros to $`t=\mathtt{012345678999}`$,

```math
u=\mathtt{012345678888},\qquad
H=111,\qquad B(u)=32,\qquad B(s)=0,
```

so

```math
h(s,t)=111+32=143.
```

This is a guaranteed minimum, not a claim that the optimal solution has exactly
that many increments. The solver still searches for an optimal prefix to $`u`$.

Starting instead at $`\mathtt{912345678888}`$ with reset index nine, one forward
tick reaches $`u`$ and the forced suffix completes the puzzle:

```math
(\mathtt{912345678888},9)
\xrightarrow{\mathrm{Forward}}
(\mathtt{012345678888},0)
\xrightarrow{\mathrm{Inc}^{111}}
(\mathtt{012345678999},0).
```

The cost is $`(111,1)`$. The forced interval requires all increments, and at least
one reset is needed because a direct increment path from the starting numeric
value would wrap and require more increments. Thus both components are optimal.

The same reasoning works with much longer suffixes. For a prefix containing
digits zero through eight and a suffix of $`m`$ nines,

```math
H=\underbrace{11\ldots1}_{m\ \mathrm{digits}}
=\frac{10^m-1}{9}.
```

Before this shortcut, a reverse search traversed the increment layers in that
stretch. After it, preprocessing uses linear digit scans and emits one counted
action. A benchmark starting exactly at $`u`$ with five trailing eights and ending
with five trailing nines went from $`111111`$ explored groups to zero, returning
the same `Increment` action with count $`11111`$.

This optimization is specific to a proved forced stretch. It does not make every
large counter easy, and the remaining prefix search can still be expensive.

## Worst-case increment bounds

### Independence of the starting display in the universal bound

Define

```math
W(n)=\max_{s,t\in\mathcal D^n}c(s,t).
```

Every start can reach the zero display without increments and can reposition the
knob without increments. Therefore

```math
c(s,t)\le c(0^n,t)
\quad\text{and}\quad
W(n)=\max_{t\in\mathcal D^n}c(0^n,t).
```

Equality holds because the zero display is itself among the possible starts.
This does **not** say that all starts have identical difficulty for a given
target, nor that reset costs are independent of the start. It says the worst
increment count over all start/target pairs is attained by a zero start.

The constructive upper bound gives

```math
W(n)\le U_n-1=\frac{10^n-10}{9}.
```

### A quadratic lower bound

Choose a target whose neighboring digits always differ, such as an alternating
zero/one display. All boundary weights are available. The optimal disjoint
selection takes alternating boundaries from the left:

```math
B(t)=(n-1)+(n-3)+(n-5)+\cdots
=\left\lfloor\frac{n^2}{4}\right\rfloor.
```

One can derive this sum from the matching recurrence, or pair each competing
adjacent selection with the higher-weight edge on its left. Since $`B(0^n)=0`$,
the potential bound proves

```math
W(n)\ge\left\lfloor\frac{n^2}{4}\right\rfloor.
```

This is a worst-case bound. Individual puzzles can require no increments at all.

### An exponential lower bound for longer counters

For $`n\ge10`$, put $`m=n-9`$ and choose

```math
t=\mathtt{012345678}\underbrace{\mathtt{99\ldots9}}_{m\ \mathrm{digits}}.
```

The fixed prefix contains digits zero through eight. The largest preceding
display missing a digit is

```math
u=\mathtt{012345678}\underbrace{\mathtt{88\ldots8}}_{m\ \mathrm{digits}}.
```

With the fixed prefix, the only digit that can be missing is nine. The largest
suffix without nine is all eights. Any candidate with a smaller prefix is
numerically below this one, so it cannot give a closer missing-digit predecessor.

The zero start lies below $`u`$ and hence must traverse the full forced interval:

```math
c(0^n,t)\ge H
=\frac{10^m-1}{9}
=\frac{10^{n-9}-1}{9}.
```

Thus

```math
\max\left(
\left\lfloor\frac{n^2}{4}\right\rfloor,
\frac{10^{n-9}-1}{9}
\right)
\le W(n)\le\frac{10^n-10}{9},\qquad n\ge10.
```

The two exponential expressions differ only by a fixed factor as width grows,
which establishes

```math
W(n)=\Theta(10^n).
```

This is the growth of the **number of increments in the worst optimal solution**.
It is not a proof of an exponential runtime lower bound for every algorithm.
Counted actions can describe huge increment stretches compactly, and the forced
stretch optimization finds some such sequences without visiting each increment.
No general closed-form formula for $`W(n)`$ is derived here.

### Exact small cases and the six-increment claim

Exhaustive enumeration of the increment-cost graph gives the following exact
small cases:

| Width $`n`$ | Worst minimum increment count $`W(n)`$ |
| --------: | -----------------------------------: |
|       $`1`$ |                                  $`0`$ |
|       $`2`$ |                                  $`1`$ |
|       $`3`$ |                                  $`3`$ |
|       $`4`$ |                                  $`6`$ |
|       $`5`$ |                                 $`11`$ |
|       $`6`$ |                                 $`17`$ |
|       $`7`$ |                                 $`25`$ |
|       $`8`$ |                                 $`34`$ |

These are finite enumeration results, not an extrapolated formula. The
verification graph has one vertex per display. Knob position can be omitted for
this primary-cost calculation because free backward repositioning allows any
digit merge $`F_d`$ to be chosen. Add edges

```math
x\xrightarrow{0}F_d(x)\quad(d\in\mathcal D),
\qquad
x\xrightarrow{1}\mathrm{Inc}(x)_{\mathrm{display}}.
```

Starting at $`0^n`$, a complete zero–one breadth-first search puts zero-cost
relaxations at the front of a deque and unit-cost relaxations at the back. Once
all distances are finalized, their maximum is $`W(n)`$. This quotient graph is
exact for increment counts: each digit merge can be realized by knob
repositioning followed by a forward tick, with no increments; conversely every
legal display-changing reset is one of those merges.

For the four-wheel enumeration, the distribution is:

| Minimum increments | Number of targets |
| -----------------: | ----------------: |
|                $`0`$ |              $`10`$ |
|                $`1`$ |              $`90`$ |
|                $`2`$ |             $`540`$ |
|                $`3`$ |             $`900`$ |
|                $`4`$ |            $`2460`$ |
|                $`5`$ |            $`4320`$ |
|                $`6`$ |            $`1680`$ |

The counts sum to $`10^4`$, and none needs more than six increments. Together with
the zero-start reduction, this establishes the six-increment guarantee from any
four-wheel start. The target $`\mathtt{9876}`$ from $`\mathtt{0000}`$ needs six, so
the universal bound is sharp. The repository also has regression checks for its
optimal cost pair $`(6,148)`$ and the five-wheel example
$`\mathtt{00000}\to\mathtt{98765}`$ with cost $`(11,267)`$.

## The exact solver

The current solver is an exact **reverse symbolic Dijkstra search**. It represents sets of displays with a canonical decision diagram, merges candidates with the same remaining cost and reset index, and settles each concrete state only at its smallest lexicographic cost. Mathematical bounds support direct solutions, forced-tail compression, and a user-visible lower bound. They do not currently change the priority queue into A*.

The implementation is divided between [`search.rs`](../src/search.rs), which manages costs and the frontier; [`diagram.rs`](../src/search/diagram.rs), which represents and transforms sets; and [`bounds.rs`](../src/search/bounds.rs), which computes the mathematical shortcuts and increment bounds.

### State, cost, and the effective target

Use the displays, reset rules, and lexicographic cost defined above. The
unreduced graph has $`10^{n+1}`$ concrete states. For the preimage notation below,
write $`\mathcal I(x)`$ for the increment's display component,
$`\mathcal F_q(x)=F_q(x)`$, and $`f_q=\phi_q`$.

A remaining path has cost $`c=(i,r)`$. Every operation increases the full
lexicographic cost, including resets that use no increments. Costs add
componentwise, and order is preserved by adding the same nonnegative cost to
both sides. These properties and the finite concrete state space support
Dijkstra's settling argument. The shortest-path foundation is
[Dijkstra's method][dijkstra]; the exact grouping and puzzle-specific reduction
are justified below.

Let $`z_s=(s,q_s)`$ be the original initial state. Let $`t_\ast`$ be the effective
search target and $`H`$ the compressed final increment count. Normally
$`t_\ast=t`$ and $`H=0`$. When a forced tail is removed, $`t_\ast=u`$ and
$`H=V(t)-V(u)`$. The original initial display and reset index remain unchanged.

The searched goal set is

```math
\mathcal G_\ast=\{(t_\ast,q):q\in\mathbb Z_{10}\}.
```

Seeding all ten goal indices implements the unrestricted final reset index.
The extra tail adds the same cost $`(H,0)`$ to every searched path, preserving
both optimization objectives.

### Initialization and proven shortcuts

`SearchSession::new` first validates the target's length and digits. If the initial display already equals the target, it returns an empty sequence without allocating a diagram.

For a target containing all ten digits, `forced_predecessor` finds the greatest same-width display $`u\lt t`$ that omits at least one digit. It makes one candidate for each excluded digit: retain the longest permitted prefix, lower a digit when necessary, and fill the remaining suffix with the largest permitted digit. The maximum candidate is $`u`$. There are only ten excluded digits, so this construction takes linear time in the width, rather than enumerating the values between $`u`$ and $`t`$.

If

```math
u\le s\le t,
```

the session completes with the direct increment count $`V(t)-V(s)`$ and zero resets. This interval test happens before converting the full tail length to `u64`: the tail can be too large to represent even when the initial display is only one increment from the target.

Otherwise, the session searches to $`u`$ and records $`H=V(t)-V(u)`$ for later appending. The comparisons use equal-length digit slices, and subtraction uses decimal borrowing. Neither requires converting the whole display to a machine integer.

There is also a one-increment shortcut, applied to the effective search target. If one increment reaches $`t_\ast`$ and makes at least one neighboring pair unequal that was initially equal, zero increments are impossible: resets cannot split an equal pair. The direct prefix is therefore optimal with cost $`(1,0)`$. Combined with the tail, the returned sequence is a single increment group with count $`H+1`$, checked for overflow.

Otherwise, the solver constructs a singleton diagram for $`t_\ast`$, shares that root across all ten goal indices, and begins reverse search.

### Pending groups, settled sets, and exact cost regions

The frontier is grouped by the key $`(c,q)`$, not by individual displays. Three families of display sets play different roles:

| Mathematical set | Implementation           | Meaning                                                                                                                           |
| ---------------- | ------------------------ | --------------------------------------------------------------------------------------------------------------------------------- |
| $`P_{c,q}`$        | `pending[(cost, reset)]` | Candidate displays with a known path of cost $`c`$ to the effective goal. Their shortest cost has not necessarily been established. |
| $`S_q`$            | `settled[reset]`         | Displays already accepted at their smallest remaining cost for reset index $`q`$.                                                   |
| $`R_{c,q}`$        | `regions[(cost, reset)]` | Newly accepted displays whose exact optimal remaining cost is $`c`$; retained for reconstruction.                                   |

All these sets are diagram roots. The binary heap contains group keys, ordered by $`c`$ and then by the numeric reset index. The reset-index tie break does not introduce another optimization objective.

Whenever another predecessor set $`A`$ arrives for an already pending key, the solver performs

```math
P_{c,q}\leftarrow P_{c,q}\cup A.
```

It keeps one heap entry for that pending key. This union is exact: it combines alternatives with the same remaining cost and index without replacing them by a representative display or an approximation.

When the smallest key $`(c,q)`$ is popped, the solver removes its pending entry and computes

```math
A=P_{c,q}\setminus S_q.
```

An empty result is discarded. A nonempty result becomes $`R_{c,q}=A`$ and increments the visited-group counter. The solver then checks whether

```math
q=q_s\quad\text{and}\quad s\in A.
```

If so, $`c`$ is the optimal prefix cost and reconstruction can begin. Otherwise, it updates

```math
S_q\leftarrow S_q\cup A
```

and expands the group's predecessors.

The winning region is recorded before the success check; it does not need to be added to the settled union because the search ends immediately. All regions with smaller costs needed for its replay have already been recorded.

This is Dijkstra's settling argument applied to an exact set representation. All incoming paths to a candidate at a smaller cost are processed before it. Every inverse edge produces a strictly larger full cost, so a popped key cannot later receive a new contribution from a smaller unprocessed key. Removing settled displays excludes paths that reach an already accepted state at a worse cost. Grouping changes storage and the amount of work done together, not the shortest-path criterion.

### Reverse expansion uses preimages

A reverse edge asks which states would reach the current set after one **forward** operation. It is not an attempt to physically run a noninvertible reset backwards.

For a display set $`A`$, define

```math
\mathcal I^{-1}(A)=\{x:\mathcal I(x)\in A\},
\qquad
\mathcal F_p^{-1}(A)=\{x:\mathcal F_p(x)\in A\}.
```

For a popped group $`(c,q,A)`$, the three expansions are

```math
\begin{aligned}
P_{c+(1,0),q}
&\leftarrow P_{c+(1,0),q}\cup\mathcal I^{-1}(A),\\
P_{c+(0,1),q-1}
&\leftarrow P_{c+(0,1),q-1}\cup\mathcal F_{q-1}^{-1}(A),\\
P_{c+(0,1),q+1}
&\leftarrow P_{c+(0,1),q+1}\cup A.
\end{aligned}
```

All index arithmetic is modulo ten. A forward-reset predecessor has index $`q-1`$, because its forward tick produces index $`q`$. A backward-reset predecessor has index $`q+1`$, because its backward tick produces index $`q`$; its display is unchanged.

Increment is a bijection on fixed-width displays, so its preimage is subtraction by one modulo $`10^n`$. A forward reset can have many predecessors. The diagram computes their full set without choosing a single inverse.

### Canonical least-significant-first decision diagrams

The diagram is a digit trie whose identical subtrees are shared in a directed acyclic graph. Each nonterminal node stores ten child identifiers, one per digit. There are two reserved identifiers:

```math
\mathbf 0=\varnothing,
\qquad
\mathbf 1=\{\varepsilon\},
```

where $`\varepsilon`$ is the empty remaining word. The empty identifier can be used at any remaining depth. A terminal is reached only after consuming the whole display.

Although public digit vectors are most significant first, a root reads the **least significant digit first**. If a node has children $`A_0,\ldots,A_9`$, its represented words are

```math
N(A_0,\ldots,A_9)
=\bigcup_{a=0}^{9}\{a\}\times A_a,
```

in that least-significant-first word order. A singleton for display $`\mathtt{123}`$ has a root branch for $`3`$, followed by $`2`$, followed by $`1`$, then the terminal. `contains` consequently traverses the supplied display in reverse.

`intern` gives an identical child array the same node identifier. It hashes the array to locate a collision chain, then compares complete arrays before accepting an existing node. Hash equality alone never establishes set equality. An all-empty array becomes the empty identifier without allocating a node.

Children are created before parents and have smaller identifiers. Nonempty child structure also determines depth, so no separate level key is needed for interning. The implementation shares equal subtrees; it does not erase a digit level merely because all ten branches are equal.

#### Union and difference

For roots of the same remaining width, Boolean set operations are componentwise:

```math
\begin{aligned}
N(A_0,\ldots,A_9)\cup N(B_0,\ldots,B_9)
&=N(A_0\cup B_0,\ldots,A_9\cup B_9),\\
N(A_0,\ldots,A_9)\setminus N(B_0,\ldots,B_9)
&=N(A_0\setminus B_0,\ldots,A_9\setminus B_9).
\end{aligned}
```

Identity cases resolve empty sets and equal roots immediately. Other cases use an explicit postorder stack and operation caches keyed by node pairs. Child results are available before their parent is interned. This avoids call-stack depth proportional to the number of wheels.

Each previously unresolved node pair examines ten branch pairs. The amount of work depends on the pairs encountered and on newly interned nodes; sharing can make it much smaller than operating on the represented concrete displays. It is not generally linear in the number of wheels, and intermediate results can grow substantially.

#### Increment preimage and the carry spine

Let $`J(A)=\mathcal I^{-1}(A)`$. At a nonterminal node,

```math
J\bigl(N(A_0,\ldots,A_9)\bigr)
=N\bigl(A_1,A_2,\ldots,A_9,J(A_0)\bigr).
```

An input digit from zero through eight becomes its successor with no carry, so it reuses the corresponding existing subtree. Input digit nine becomes zero and carries into the higher digits, requiring the transformed zero branch. Only that branch propagates a carry.

The base cases are

```math
J(\mathbf 0)=\mathbf 0,
\qquad
J(\mathbf 1)=\mathbf 1.
```

The terminal case discards a carry beyond the most significant wheel and thereby implements fixed-width wraparound. For example, the increment preimage of the all-zero display is the all-nine display.

The implementation follows the zero-child carry spine until a terminal, an empty node, or a cached result, then rebuilds it upwards with an explicit stack. One call follows at most one width-length spine; the other nine branches reuse existing subtrees. It does not transform every branch of the represented set.

#### Forward-reset preimage

Let $`K_q(A)=\mathcal F_q^{-1}(A)`$. Then

```math
K_q\bigl(N(A_0,\ldots,A_9)\bigr)
=N\bigl(K_q(A_{f_q(0)}),\ldots,K_q(A_{f_q(9)})\bigr).
```

The same map applies independently at every wheel. An output digit $`q`$ has no predecessor because a forward tick removes that digit completely. The traversal therefore skips the old output-$`q`$ branch. Input digits $`q`$ and $`(q+1)\bmod 10`$ share the transformed successor branch; all other input digits use their corresponding output branches.

For a target consisting entirely of the successor digit, every wheel can previously have been either the old index or its successor. This gives $`2^n`$ concrete predecessors, but the diagram can represent them as a shared chain with two equal nonempty branches at each level. This example illustrates the benefit of sharing, not a bound on every reset preimage.

Reset transformations use an iterative postorder traversal cached by node and reset index. The backward-reset preimage needs no diagram transformation because it changes no displayed digit.

### Reconstructing an actual optimal sequence

The solver retains exact remaining-cost regions instead of a parent pointer for
every concrete state. It starts at $`z_s`$ with the accepted prefix cost $`c`$ and
tries actual forward operations at each replay position.

For the current complete state $`z=(x,q)`$, an operation $`a`$ of cost $`w(a)`$ gives
the successor $`a(z)=(x',q')`$. That successor is eligible when

```math
x'\in R_{c-w(a),\,q'}.
```

This tests the successor display in the region keyed by its reset index and
remaining cost. A component must be positive before its cost is subtracted.
After selecting the successor, replay replaces the current state by
$`z\leftarrow a(z)`$ and the remaining cost by $`c\leftarrow c-w(a)`$.

The concrete preference order is increment first, then forward reset, then backward reset. The first eligible successor is taken, its operation is appended, and the remaining cost is decreased. This order chooses among equal-cost witnesses; it does not minimize the number of reported groups or introduce another objective.

An eligible successor exists because the accepted state has an optimal path and its first operation leads to a state at exactly the reduced cost. Every reduced cost is strictly smaller, so the necessary region has already been settled. Repeating reaches the effective target at cost $`(0,0)`$.

Consecutive operations of the same kind are combined with checked addition. Forward and backward reset groups remain separate. If a tail was compressed, a single increment group of size $`H`$ is appended and merged with a final prefix increment group when appropriate.

Prefix reconstruction still executes its witness one unit operation at a time and performs concrete counter updates and diagram membership tests. A grouped output alone does not make a very long prefix cheap to reconstruct. The forced tail is different: it is appended directly, without replaying its potentially enormous number of individual increments inside the solver.

### Cache and arena lifecycle

Operation caches store derived unions, differences, reset preimages, and increment preimages. They are cleared when the processed **full cost pair** changes, not merely when the increment count changes. Groups at the same cost can share these cached computations. Clearing those maps leaves the node arena and its canonical interning table intact, so roots remain valid.

The arena also retains nodes that are no longer reachable from useful roots. Before processing a group, the solver compacts it when the allocated nonterminal count reaches its collection threshold. The initial threshold is $`32768`$ nodes. After collection, the threshold is the greater of $`32768`$ and three times the retained node count; it is a collection policy, not a hard memory limit.

The retained roots are exactly those referenced by the settled sets, pending groups, and reconstruction regions. Because children have smaller identifiers, compaction can mark reachable nodes in a descending arena scan and rebuild them in ascending order without recursion. Every retained root is then remapped to its new identifier. Operation caches are discarded because their old identifiers are invalid after remapping.

This removes unreachable intermediates, but it cannot remove live regions required for eventual replay. A difficult search can therefore continue to grow despite collection. Collection itself requires scans and replacement storage, so its cost contributes to the measured work.

After completion or failure, `SearchSession` replaces the active search with a lightweight terminal outcome. Diagram storage, frontier maps, caches, and the heap are released. Subsequent `advance` calls return the stored result or error; the latest statistics remain available without retaining the search buffers.

### Complexity and what sharing does not guarantee

The concrete state space is exponential in the width. Symbolic storage can exploit repeated suffix sets and digitwise reset structure, but there is no general polynomial-time or polynomial-space guarantee for this solver.

If $`Q`$ is the maximum number of pending group keys and $`G`$ the number of popped groups, heap operations have the usual logarithmic dependence on $`Q`$. That is only one part of the work. Diagram union and difference depend on the node pairs encountered; reset preimages depend on reachable transformed nodes; increment preimages follow carry spines; compaction scans arena storage; and reconstruction follows a concrete optimal witness. Hash-table operations and interning also affect actual runtime.

The following distinctions are essential:

- A group is a set of displays at one cost and reset index. Its cardinality can be enormous even when its root is compact.
- Two groups can have very different diagram-operation costs.
- Popping a group can create new groups, so the current queue length is not a remaining-work total.
- Allocated diagram nodes include unreachable intermediates awaiting collection; their count can decrease during compaction.
- The cumulative diagram-work counter is an approximate, saturating count of internal operations. It survives cache clears and compaction, but it is neither a concrete-state count nor a fixed amount of elapsed time.

The public `visited_states` name is retained for compatibility and counts nonempty groups accepted after subtracting settled displays. `advance` budgets count popped groups, including groups that become empty. A group budget is consequently not a time budget.

### Numeric limits and error semantics

Displayed values remain digit vectors, so the implementation imposes no limit requiring the whole value to fit a machine integer. Leading zeros remain significant. This does not remove limits on action counts, costs, arena identifiers, or available memory.

Both components of a searched cost and every reported action count use `u64`, with maximum

```math
L=2^{64}-1.
```

Costs are not converted into a weighted scalar. Checked addition prevents increments, reset ticks, merged action counts, and the appended tail from wrapping silently.

For a forced-tail search, enqueueing also checks

```math
i+H\le L.
```

Over-budget increment branches are omitted and an overflow flag is remembered. The solver continues exploring representable branches, allowing a valid solution at the last representable increment layer. If the frontier later exhausts with that flag set, it reports `CostTooLarge`. Reset-cost additions are checked as well and report overflow rather than wrapping. Direct solutions check their differences and combined counts before returning.

Decimal subtraction permits arbitrarily wide equal-length displays when their difference fits `u64`. High place values that cannot fit are harmless if their difference digits are zero; a nonzero unrepresentable contribution yields `CostTooLarge`.

Absolute weighted-bound scores use `u128`. Their maximum for width $`n`$ is at most $`\lfloor n^2/4\rfloor`$, which fits that type for the supported address spaces. The implementation subtracts scores before converting the nonnegative difference to `u64`. Large absolute scores therefore do not falsely reject a small required increment bound.

Diagram identifiers use `u32`; exhaustion of representable identifiers is reported as `AllocationFailed`. Fallible growth of the diagram arena, frontier storage, traversal stacks, and output vectors maps reservation failures to the same error. These checks describe the allocations explicitly managed by the solver, rather than promising recovery from every possible host allocation failure.

Target validation errors distinguish an empty counter, mismatched width, and digits outside the decimal range. In the mathematical graph, every valid decimal target is reachable by incrementing around the fixed-width cycle. `NotFound` remains an API outcome for an exhausted frontier; it is not a claim that some ordinary valid decimal target lies outside that cycle.

### Library execution and responsive interfaces

`search` is synchronous: it repeatedly advances a session until completion. `SearchSession::new` performs validation and initialization, `advance` performs a bounded number of group pops, and `statistics` observes the latest snapshot. A zero budget observes progress without processing a group. These methods do not create threads or automatically yield during a costly diagram operation.

The GUI puts initialization and advancement outside its event thread. Its Iced task is the delivery mechanism for results from a dedicated native thread or a browser Web Worker; merely making a CPU-heavy function asynchronous would not provide this separation.

The two backends currently use batches of $`256`$ popped groups and report progress after approximately $`50`$ milliseconds of measured solver time between reports. Completion and errors bypass that reporting throttle. Neither the batch size nor the reporting interval guarantees a response every $`50`$ milliseconds: one symbolic operation or batch can take longer.

On native builds, dropping the control handle sets a cancellation flag. The worker checks it, and receiver closure, between batches; cancellation does not join the worker on the UI thread. On browser builds, dropping the handle detaches callbacks, terminates the dedicated worker, and closes the update stream, including during worker initialization. The browser waits for a `ready` handshake before sending its request. The UI also tags updates by job generation so an older job cannot overwrite a newer one.

Each update pairs a search status, statistics from the same completed batch, and backend-measured elapsed time. Native timing uses `Instant`; browser timing uses the worker's monotonic performance clock, with a finite nondecreasing fallback. Solver setup is included. Thread or worker startup, browser WASM loading, and update delivery are outside that solver-time measurement.

The displayed increment layer includes $`H`$ when a tail was compressed. Reset ticks describe the latest processed cost pair and can decrease when the search moves to a larger increment layer. The GUI's mathematical lower-bound label is separate from its empirical timing estimate. A solution-cost bound does not determine a diagram-work total or an ETA.

### Why this is not currently A*

The current heap orders remaining searched cost $`c`$, then reset index. It does not order a sum of searched cost and a heuristic. `increment_lower_bound` is used for the guaranteed user-facing bound, while the proven forced-tail reduction and direct-path checks modify the searched problem before Dijkstra begins.

For a possible reverse A* design, a heuristic would need to bound the cost **from the original initial state to the current reverse-search state**. For a symbolic group $`A`$ at index $`q`$, an admissible scalar increment bound would need to satisfy

```math
h(A,q)\le\min_{x\in A}
\left\{\text{minimum increments from }z_s\text{ to }(x,q)\right\}.
```

A potential priority could then combine $`c`$ with $`(h(A,q),0)`$. This is a design requirement, not an implemented algorithm or a sufficient correctness proof on its own.

A single group can contain states with different heuristic values. A bound for an arbitrary representative can overestimate the bound for other group members. Even a conservative group minimum does not automatically justify settling every member using the current Dijkstra rule. A safe implementation would need to establish suitable grouping or partitioning, per-state optimality when settling, a sound stopping condition, and the availability of exact regions for reconstruction. The existing full-puzzle lower bound is not a substitute for that group analysis.

Until such an implementation and proof are provided, the optimization guarantees come from reverse Dijkstra, exact diagram operations, and the separately proven shortcuts described above.

## Progress and timing estimates

The GUI separates facts about the puzzle from predictions about execution time. The minimum-increment bound is a mathematical statement about a solution's cost. The current increment layer is a measured search milestone. The percentage and ETA are empirical predictions based on an estimated amount of diagram work and the worker's measured speed. Neither a bound on increments nor the number of visited state groups supplies a known total amount of search work.

The implementation is in [the progress estimator](../src/gui/progress.rs), with telemetry defined by [the search session](../src/search.rs) and displayed by [the application](../src/gui/app.rs). These estimates do not change the solver's ordering, transitions, or optimality.

### What the observations measure

The notation in this section is local: $`t`$ is elapsed time, and $`W`$ is work,
rather than the target display or the worst-case function $`W(n)`$. For one
accepted worker report, write:

- $`t`$ for elapsed worker time in seconds;
- $`W`$ for cumulative approximate diagram work;
- $`G`$ for visited nonempty symbolic state groups;
- $`Q`$ for currently queued groups.

A symbolic group may represent many concrete digit displays. Different groups may require very different amounts of traversal and allocation, so $`G`$ is not used as a fixed-cost unit.

The diagram-work counter records operation entry and traversal, interning and collision checks, cache-entry clearing, and approximate compaction scans. It survives cache clearing and diagram compaction, and saturates rather than overflowing. It is a work proxy, not an exact instruction count or a fixed conversion into seconds.

The other telemetry fields have different meanings:

| Observation     | Meaning                                                                                                             |
| --------------- | ------------------------------------------------------------------------------------------------------------------- |
| Visited groups  | Cumulative count of nonempty groups actually explored.                                                              |
| Increment layer | Increment component of the latest processed cost, including any compressed forced increment tail.                   |
| Reset ticks     | Reset component of that latest cost; it can decrease when the search moves to another increment layer.              |
| Diagram nodes   | Currently allocated nonterminal arena nodes, including nodes awaiting collection; compaction can reduce this count. |
| Diagram work    | Cumulative approximate work, independent of the current arena size.                                                 |
| Queued groups   | Current queue size; processing these entries may create additional groups.                                          |

In particular, neither the node count nor the queue size is monotonic, and the current queue is not the entire remaining search.

### Worker time and visible elapsed time

Both backends pair progress and statistics from the same completed batch with worker-local elapsed time.

On desktop, timing starts inside the search thread immediately before constructing the search session. It includes solver initialization and subsequent execution/reporting overhead, but excludes thread startup and GUI preparation.

In the browser, timing starts inside the worker's request handler before request decoding, counter construction, and search-session initialization. It includes those setup steps and subsequent execution/reporting overhead. It excludes worker creation, bootstrap and WASM loading, the readiness handshake, inbound request delivery, and delivery of each response to the GUI. The preferred clock is Performance; the Date fallback is clamped to finite, nondecreasing elapsed values.

Each backend advances at most $`256`$ queued groups per batch, including entries that turn out to be empty. Progress is normally reported after a batch when at least approximately $`0.05`$ seconds have passed since the previous report; completion bypasses that throttle. This is a group budget, not a wall-time deadline. A complicated group or batch can take much longer.

The visible elapsed clock is separate. It starts in the application before estimating the bound and launching the backend, so it also includes startup/loading and message latency. Redraw messages keep it moving independently of backend reports. The completed elapsed value is captured when completion reaches the application, before replaying the result into displayed steps. The estimator uses worker time $`t`$, not this visible UI elapsed value.

### The input-dependent work prior

Before measurement, the estimator optionally chooses an initial total-work prior $`W_0`$. This is a table-based empirical calibration, not a mathematically proved work bound.

Let the target's displayed, most-significant-first digits be $`d_1,\ldots,d_n`$. Let its maximal adjacent equal-digit runs have lengths $`\ell_1,\ldots,\ell_m`$. Define:

```math
E=\sum_{j=1}^{m}\min(\ell_j,2),
```

```math
\nu=\left|\{d_1,\ldots,d_n\}\right|,
```

```math
b=\sum_{i=1}^{n-1}\mathbf{1}[d_i\ne d_{i+1}].
```

Here $`E`$ is the effective width, $`\nu`$ is digit diversity, and $`b`$ counts boundaries between adjacent runs. Capping each run at two representatives reflects the sharing seen for long repeated runs, without making each extra repeated digit contribute exponential predicted work.

The prior is unavailable unless all of the following hold:

- Starting and target widths match, with $`1\le n\le7`$.
- Target digits are decimal digits.
- Every starting digit is equal to the first starting digit.
- The target differs from the starting display.
- The target has at least two distinct digits, $`\nu\ge2`$.

The one-wheel case passes the width check but always has $`\nu=1`$, so it never receives a prior. Wider puzzles and mixed starting digits still use the solver; they simply have no calibrated percentage or ETA.

For remaining inputs, the base scales are:

```math
\beta(E)=
\begin{cases}
20\,000 & E=2,\\
190\,000 & E=3,\\
1\,650\,000 & E=4,\\
10\,200\,000 & E=5,\\
61\,000\,000 & E=6,\\
366\,000\,000 & E=7.
\end{cases}
```

The following rules are evaluated in order.

**1.** A target with one boundary and effective width at most three gets the special repeated-run prior:

```math
W_0=30\,000+50\,000\max(n-2,0)
\qquad\text{if }b=1\text{ and }E\le3.
```

**2.** Otherwise, a target with two distinct digits gets:

```math
W_0=0.65\,\beta(E)
\qquad\text{if }\nu=2.
```

This includes alternating targets and some repeated-run targets. It takes precedence over the next repeated-run rejection.

**3.** For $`\nu\ge3`$, adjacent repeated digits are not calibrated: the prior is unavailable unless $`b+1=n`$.

**4.** For the remaining targets, define the ascending fraction:

```math
A=\sum_{i=1}^{n-1}\mathbf{1}[d_i\lt d_{i+1}],
\qquad
a=\frac{A}{b}.
```

The growth multiplier and prior are:

```math
g=
\begin{cases}
1+0.3\max(E-4,0) & a\ge0.75,\\
1 & a\lt 0.75,
\end{cases}
\qquad
W_0=\beta(E)\,g.
```

For example, the descending target $`\mathtt{9876543}`$ gets $`W_0=366\,000\,000`$; the ascending target $`\mathtt{1234567}`$ gets $`W_0=695\,400\,000`$; and the alternating target $`\mathtt{9090909}`$ gets $`W_0=237\,900\,000`$. A target such as $`\mathtt{0000009}`$ gets the special prior $`W_0=280\,000`$.

The rounded scales were calibrated from bounded release searches over varied, alternating, ascending, and descending patterns. The seven-wheel scale extrapolates the roughly sixfold increase between five and six wheels; seven-wheel cases, including $`\mathtt{9876543}`$ and $`\mathtt{9090909}`$, were held out when checking the prediction.

This policy does not model the value of the uniform starting digit or the initial reset index. Both can affect the actual search. Eligibility is therefore a deliberately limited heuristic policy, not a guarantee of accuracy for every accepted input.

### Accepting samples and revising the prior

An update is ignored if worker elapsed time or cumulative diagram work moves backward relative to the previous accepted update:

```math
t_{\mathrm{new}}\lt t_{\mathrm{old}}
\quad\text{or}\quad
W_{\mathrm{new}}\lt W_{\mathrm{old}}.
```

Equality is allowed. The estimator does not independently reject a decreasing visited-group count; valid session reports supply its monotonic count, and the subsequent differences use saturating subtraction.

The sample deque starts with $`(t,W,G)=(0,0,0)`$. An accepted update adds a sample only when its elapsed time is at least $`0.05`$ seconds after the most recent stored sample. At most eight samples are retained; when a ninth is added, the oldest is discarded. These are spaced observations rather than a fixed-rate sampling schedule. Latest telemetry and elapsed time are still updated when a report arrives too soon to add a sample.

Let $`(t_f,W_f,G_f)`$ be the oldest retained sample. With current telemetry, define:

```math
\Delta G=\max(G-G_f,0),
\qquad
\Delta W=\max(W-W_f,0).
```

When $`\Delta G\gt 0`$, the estimator allows for two additional queued waves at the observed work per group:

```math
W_{\mathrm{queue}} =
2Q\,\frac{\Delta W}{\Delta G},
```

```math
\widehat W =
\max(W_0,\;W+W_{\mathrm{queue}}).
```

The multiplier of two is empirical. The queue term revises a total-work prediction; it does not assert that the queue covers all remaining work.

The revised total is available only while:

```math
\widehat W\le1.4W_0.
```

A larger revision withdraws the current total-work prediction. A later update may restore it if the queue-derived revision again fits this limit. Revised totals are recomputed rather than forced to increase monotonically, and never go below $`W_0`$.

A separate rule is permanent for that estimator instance: once observed work reaches the original prior,

```math
W\ge W_0,
```

timing predictions are disabled for the remainder of the run. This check uses the original prior even if the queue previously increased the revised total. The implementation does not invent a shrinking tail to keep showing a high percentage after its prior has been exhausted.

### Rate selection, ETA, and percentage

A prediction requires every startup threshold:

```math
t\ge0.25\text{ seconds},
\qquad
G\ge128,
\qquad
W\ge100\,000,
\qquad
|\mathcal S|\ge4,
```

where $`\mathcal S`$ is the retained sample deque. A valid revised total must also be available, and the original prior must not have been exhausted.

For consecutive stored samples, collect rates only when both elapsed time and work increased:

```math
r_j =
\frac{W_j-W_{j-1}}{t_j-t_{j-1}}
\qquad
\text{for }t_j\gt t_{j-1}\text{ and }W_j\gt W_{j-1}.
```

At least three positive rates are required. Sorting the $`k`$ rates in ascending order, the implementation selects:

```math
r_{\mathrm{median}}=r_{\lfloor k/2\rfloor},
```

using zero-based indexing. For an even number of rates this is the upper middle value, not the average of the two middle values. With eight samples there can be at most seven interval rates.

The overall rate is:

```math
r_{\mathrm{overall}}=\frac{W}{t}.
```

The prediction is withdrawn unless the median is finite and positive and all stability checks hold:

```math
r_{\max}\le4r_{\min},
```

```math
0.4r_{\mathrm{overall}}
\le r_{\mathrm{median}}
\le2.5r_{\mathrm{overall}}.
```

These comparisons allow equality at each boundary. The rate used for the ETA is:

```math
r_{\mathrm{used}} =
\min(r_{\mathrm{median}},\;1.1r_{\mathrm{overall}}).
```

A recent slowdown can therefore increase the remaining time promptly. A burst of cheap recent work cannot raise the selected rate more than ten percent above the overall rate.

Estimated remaining time in seconds is:

```math
\tau =
\frac{\widehat W-W}{r_{\mathrm{used}}}.
```

The prediction is unavailable if $`\tau`$ is nonfinite, negative, or greater than one day:

```math
0\le\tau\le86\,400.
```

Finally, the displayed percentage is based on predicted time, not simply the proportion of observed work:

```math
p =
\mathrm{clamp}
\left(
100\,\frac{t}{t+\tau},
\;0,\;99
\right).
```

Under a stable rate this resembles a work fraction. With slowing execution or a larger revised total, both $`\tau`$ and $`p`$ change: the ETA may increase and the percentage may move backward. The percentage is a current prediction, not a monotonic count of completed tasks.

### Displaying predictions and actual milestones

The application imposes an additional freshness gate: a timing prediction is displayed only if the most recently received report is at most two seconds old. If a long batch delays reports, the UI switches to activity even though the worker remains busy.

With a prediction, a redraw sensor updates visible elapsed time approximately once a second. With uncertain timing, its cadence is approximately $`0.05`$ seconds so the activity bar remains animated. These redraws do not generate new worker samples. In particular, the ETA is recomputed on reports and does not count down independently between them. Its displayed duration has a minimum of one second and is otherwise formatted in whole seconds.

When timing is unavailable, the GUI still shows:

- Visible elapsed time.
- An activity bar without a completion percentage.
- “Preparing the search…” before the first report.
- The actual current increment-layer milestone after reports begin.
- Visited state groups.
- The mathematical minimum-increment bound when it is available and positive.

The minimum-increment statement has units of action ticks; the increment layer describes where the ordered search currently is. Dividing one by the other would not produce a justified work percentage: each layer can contain very different numbers and shapes of symbolic groups.

Only a successfully found and replayed solution produces the completed bar with $`100\%`$. An exhausted or failed search displays its error rather than claiming success. While a prediction exists during execution, its cap remains $`99\%`$.

### A recorded checkpoint and the limits of the estimator

The regression fixture for the release search $`\mathtt{0000000}\to\mathtt{9876543}`$ records total worker time of approximately $`6.622`$ seconds. At a checkpoint around $`t=1.303`$ seconds, it records $`W=86\,174\,793`$ units of diagram work. Replaying the retained measurements gives approximately:

```math
r_{\mathrm{median}} =
52.365\times10^6\text{ work units/second},
```

```math
\tau\approx5.344\text{ seconds},
\qquad
p\approx19.6\%.
```

The recorded actual remaining time was about $`5.319`$ seconds. This is a useful regression against the former early high-percentage display. It is one recorded case, not a timing guarantee or an accuracy interval for other counters, machines, native builds, or browsers.

The confidence gates are transparent heuristics. They do not prove that the predicted remaining work exists, that future work will cost the same per unit, or that an ETA is statistically calibrated. Memory pressure, cache behavior, compaction, the starting state, and target structure can change runtime sharply. A stable measured rate can still accompany an inaccurate work prior.

The fallback is therefore part of the intended behavior. Unsupported widths and shapes, insufficient observations, unstable rates, large queue revisions, exhausted priors, or stale reports suppress the percentage and ETA while preserving actual search milestones and elapsed time. A new run creates a new estimator with fresh samples and no previous prediction.

## Verification and implementation map

### What the mathematical framework currently changes

| Mathematical fact                                      | Current use                                                                    | Guarantee or limitation                                                                                                   |
| ------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------- |
| Resets cannot split equal wheels.                      | Proves a direct one-increment shortcut when it creates a new unequal boundary. | Exact solution with no resets, including a merged forced tail when applicable.                                            |
| Weighted boundary potential.                           | `increment_lower_bound` and the GUI's minimum-increment label.                 | Guaranteed underestimate; not currently a frontier priority or timing total.                                              |
| A forward reset omits its old index.                   | Removes the maximal mandatory final increment interval before search.          | Exact reduction of both cost components; avoids traversing that interval.                                                 |
| Resets act digitwise and can have many inverse states. | Decision-diagram reset preimages and subtree sharing.                          | Exact set operations can represent exponentially many predecessors compactly; arbitrary sets need not remain compact.     |
| Lexicographic costs are additive and ordered.          | Reverse Dijkstra, exact cost regions, and constant tail offset.                | Increment optimality first, reset optimality second.                                                                      |
| Uniform displays are reachable without increments.     | General reachability and increment upper-bound proof.                          | A useful possible incumbent for future search pruning; not currently used as the returned construction or progress total. |
| Measured work patterns and throughput.                 | Conditional percentage and ETA.                                                | Empirical prediction with explicit fallback; not a proof about remaining computation.                                     |

An increment bound can certify that a returned solution is increment-optimal
when its increment count meets the bound. It still says nothing by itself about
whether its reset count is minimal among all increment-optimal solutions. That
secondary claim comes from the exact shortest-path search or the direct-path
proof, not from the scalar bound alone.

### Evidence and regression coverage

The general statements about resets, the potential, the forced interval, and
the analytic worst-case bounds have proofs above. The small-case table was
checked separately by exhaustive zero–one breadth-first search of the quotient
graph through width $`n=8`$. The quotient intentionally ignores reset cost; it
does not establish the secondary component of an optimal sequence.

The retained automated tests check several different kinds of behavior:

- Counter batching against independent one-tick simulation, including carry,
  wraparound, and wheel engagement.
- Diagram union, difference, and preimages against explicit sets; fingerprint
  collision safety; iterative traversal of wide diagrams; and collection that
  preserves retained roots.
- The symbolic solver's full cost pairs against independent Bellman–Ford
  distances for every two-wheel target from more than one initial state.
- Every four-wheel increment and forward-reset transition against the potential
  inequalities, including full wraparound.
- Missing-digit predecessor maximality against direct backwards enumeration for
  selected complete targets, including leading zeros and prefix backtracking.
- Direct and reduced forced-tail searches, original knob positions, replay,
  merged groups, huge tails, and a start close to a target whose full forced
  interval exceeds the action-count limit.
- A forced tail of length $`2^{64}-1`$ that still has a valid reset-only prefix,
  ensuring overflow pruning does not discard that representable solution; and
  an input needing more increments than the representation can hold.
- Bounded search sessions, final statistics, memory release, and repeated reads
  of terminal outcomes.
- Timing warmup, uncertainty, stale reports, queue revisions, slowdowns, prior
  exhaustion, and the recorded checkpoint described above.

These checks support the implementation and catch regressions; finite tests
alone do not replace the proofs for arbitrary width. Conversely, a mathematical
proof of the model does not promise that any physical counter follows the same
rules or that every reachable puzzle fits the library's count and memory limits.

To run the retained tests:

```sh
cargo test --all-features --locked
```

### Finding the implementation

| Source                                                                                | Responsibility                                                                                                                            |
| ------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| [Library API](../src/lib.rs)                                                          | Public counter, action, result, session, statistics, and lower-bound exports.                                                             |
| [Counter model](../src/counter.rs)                                                    | Validated digit vectors, decimal carry, wraparound, and grouped reset operations.                                                         |
| [Mathematical bounds](../src/search/bounds.rs)                                        | Weighted matching recurrence, greatest missing-digit predecessor, and checked decimal differences.                                        |
| [Search and reconstruction](../src/search.rs)                                         | Input validation, direct shortcuts, forced-tail reduction, cost ordering, grouped frontier, exact regions, reconstruction, and lifecycle. |
| [Decision diagram](../src/search/diagram.rs)                                          | Canonical nodes, set operations, inverse transitions, cache management, compaction, and approximate work accounting.                      |
| [Progress model](../src/gui/progress.rs)                                              | Mathematical bound label data, empirical work prior, sample rates, prediction gates, and ETA.                                             |
| [GUI application](../src/gui/app.rs)                                                  | Job generations, report freshness, visible elapsed time, progress views, and completed result handling.                                   |
| [GUI model](../src/gui/model.rs)                                                      | Form validation, preparation, and grouped-action replay into result rows.                                                                 |
| [Solver facade](../src/gui/solver.rs)                                                 | Shared cancellable background-task interface and paired update snapshots.                                                                 |
| [Native backend](../src/gui/solver_native.rs)                                         | Dedicated thread, bounded batches, cancellation checks, and worker-local time.                                                            |
| [Browser backend](../src/gui/solver_wasm.rs)                                          | Worker creation and termination, startup handshake, message decoding, and exact integer transport.                                        |
| [Worker implementation](../src/gui/worker.rs)                                         | Browser-side search execution, elapsed clock, and encoded progress/results.                                                               |
| [Worker bootstrap](../solver-worker.js)                                               | Load the worker WASM module and start its handler.                                                                                        |
| [Playback state](../src/gui/playback.rs) and [rendering](../src/gui/playback_view.rs) | Apply real ticks, animate their exact states, and keep wide displays readable.                                                            |
| [CLI](../src/bin/cli.rs)                                                              | Parse inputs, run the synchronous solver, and print grouped replay states in a table.                                                     |

The module documentation and source are the authority for implementation policy
values that may change, such as collection thresholds, batch sizes, work-prior
scales, and confidence gates. The mathematical results depend on the stated
counter rules and objective, rather than those tuning choices.

## References

- [OskarPuzzle's inspiration video][video]. The proofs in this document concern
  the explicitly defined project model rather than unstated physical details.
- E. W. Dijkstra, [_A note on two problems in connexion with graphs_][dijkstra].
  The project adapts shortest-path ordering to additive lexicographic costs and
  exact sets of counter states; the puzzle-specific bounds and reduction are
  proved above.

[video]: https://www.youtube.com/watch?v=AT9wAQSV5_4
[dijkstra]: https://link.springer.com/article/10.1007/BF01386390
