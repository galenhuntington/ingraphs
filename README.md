This is code for my article [Dense Universal
Ingraphs](https://galen.xyz/ingraphs/).  It provides several tools
for working on this problem.  It is written in Rust with some C,
and is quite rough, but usable.

In the `proofs` directory are instructions for verifying the claims
in the article.

## Problem

A universal ingraph for _n_ is a graph G such that for any graph H
on _n_ vertices, G is a subgraph of either H or the complement of H.

For example, a universal ingraph for 6 is the five-vertex graph
looking like ☐_.

The goal is to find universal ingraphs with the most number of edges.

## Code

This code has a smorgasbord of operations aimed towards finding and
analyzing ingraphs.  Some of it could be better documented.

Graphs are represented by decimal numbers (perhaps not the best system
but adequate).

### Building and native canonicalization

`cargo build --release` now builds a pinned, bundled nauty 2.9.3 backend
for internal isomorphism keys. Native builds on POSIX systems require a
C11 compiler, `sh`, and the usual configure utilities. Cargo configures
the bundled headers in its build directory and links the C code statically;
there is no make step, libclang/bindgen requirement, download during the
build, or dependency on the separately built `nauty-prune` directory.

For a binary tuned to the build machine:

```sh
cargo build --release --features native
```

Release C compilation already uses `-O3`; `native` adds `-march=native` to
the C backend. Such binaries may not run on older/different CPUs. This
feature does not change Rust's target CPU; set `RUSTFLAGS='-C target-cpu=native'`
too if desired. `CC` and `CFLAGS` retain their normal `cc`-crate meanings.
The actual C command is recorded in
`target/release/build/graphy-*/out/nauty/compiler.txt`.

`cargo build --release --no-default-features` selects the pure-Rust
canonical-key fallback and needs no C toolchain; this is also the current
option for cross-compilation or non-POSIX targets. The existing `u64`
feature can be combined with either backend.

`successors` normalizes every input graph into an exact native key, so its
input may use graphy's minimum-decimal labels, arbitrary decimal labels,
or graph6 from `geng`/`labelg`. It stops checking an extension at its first
missing retraction. Output still uses minimum-decimal CSV, but its ordering
may change. For chained calls, `--internal-labels` skips conversion back to
minimum-decimal form on intermediate output; the next call accepts those
labels directly. This avoids making the legacy canonicalizer the output
bottleneck. Omit the flag on the last stage if minimum-decimal output is wanted.
`--max` retains the older, minimum-decimal-order-based path and conflicts
with `--internal-labels`.
`canon`, `enumerate`, `extend`, and `retract` retain their original semantics.
The minimum-decimal canonicalizer now tries a degree-ordered relabeling as
an initial upper bound (only if its integer is smaller), before the unchanged
exact search. This greatly reduces conversion costs for many nauty-labeled
graphs with isolated vertices; it does not change the minimum sought.

`seek()` and the search phase of `ingraph-seek` use exact native keys too,
while keeping working graph labels separate. Only a returned witness is
converted to minimum-decimal form. The matcher-only seed phase is unchanged.
For repeatable bounded experiments:

```sh
RAYON_NUM_THREADS=1 target/release/graphy ingraph-seek 14 output/batches/all14 \
    --bailout 3000 --rng-seed 20260924
```

A fixed seed and one worker give the same traversal with either key backend.
Multiple workers remain scheduling-dependent. The bailout is a soft cap on
distinct cached states (concurrent insertions can overshoot slightly); hitting
it now cancels sibling branches. **`None` from a bounded run is inconclusive,**
not a universality certificate. `--bailout 0` still skips the search.

Orbital generation streams without retaining emitted graphs. A useful
heuristic-library pipeline is:

```sh
python3 scripts/orbitals.py 14 32 45 --family all --max-orbits 25 --graph6 \
    | labelg -q | uniqg -cq > output/orbitals14.g6
```

`--family all` includes layered actions as well as cyclic ones; a smaller
orbit cap can therefore recover graphs requiring a higher cyclic cap. It
can still take a long time and is not exhaustive. `uniqg -c` trusts already
canonical input and uses SHA-256 fingerprints; for exact full-key comparisons
use `awk '!x[$0]++'` after `labelg`. Both deduplicators have growing memory
use, even though their output streams. They are not the exhaustive search's
visited-state implementation, which always uses exact packed graph keys.

## Some graphs

Here are numeric representations of some graphs mentioned in the
article:

| Graph | Number |
| :--- | ---: |
| 2 DUI | 1 |
| 3 DUI | 3 |
| 5 DUI | 13 |
| 6 DUI | 94 |
| 7 DUI | 1118 |
| 8 DUI | 3448 |
| 9X | 101752 |
| 9 DUI | 36280 |
| 9 DUI | 36216 |
| 9 DUI | 101736 |
| 9 DUI | 101744 |
| 10 DUI | 2202040 |
| 10 DUI | 6395248 |
| 11 DUI | 6732736 |
| 12 DUI | 816167872 |
| 13 DUI | 206974663616 |
| 13 DUI | 207515663232 |

### 14 DUI candidates

These are the 26 candidates for a 14-DUI, with their properties:

The two refutations found on 2026-09-17, generator commands, and further
mathematical observations are recorded in [the research report](research/2026-09-17.md).

| Graph | Vertices | Planar | \|⁠Sym⁠\| | ⊃ 11-DUI | ⊃ 12-DUI | ⊃ 2nd 13-DUI |
| ---: | ---: | :---: | ---: | :---: | :---: | :---: |
| 208052533120 | 10 | | 4 | 💎 | | |
| 208052598656 | 10 | | 4 | 💎 | | ✅ |
| 208069916672 | 10 | | 4 | 💎 | ✅ | |
| 208090921984 | 10 | | 2 | 💎 | ✅ | |
| 208095050752 | 10 | | 4 | 💎 | | |
| 209697274880 | 10 | | 1 | 💎 | ✅ | |
| 210234143744 | 10 | | 2 | 💎 | ✅ | |
| 482968729600 | 10 | | 8 | 💎 | | |
| 35391346720704 | 11 | ✅ | 4 | | | |
| 35391348817856 | 11 | ✅ | 4 | | | |
| 35391350912960 | 11 | ✅ | 2 | | | |
| 35391350913984 | 11 | ✅ | 2 | | | |
| 35391361432512 | 11 | | 4 | 💎 | | |
| 35391363529600 | 11 | | 16 | | | |
| 35392164576128 | 11 | ✅ | 4 | | | |
| 35392424655744 | 11 | ✅ | 4 | | | |
| 35599401008000 | 11 | ✅ | 8 | | | |
| 105760090896320 | 11 | ✅ | 6 | | | |
| 105760090897344 | 11 | ✅ | 6 | | | |
| 105760090929024 | 11 | | 12 | | | |
| 105760090930048 | 11 | | 12 | | | |
| 105760095090560 | 11 | ✅ | 12 | | | |
| 105760105609088 | 11 | | 24 | | | |
| 105760642480000 | 11 | | 4 | | | |
| 105761168831296 | 11 | ✅ | 8 | | | |
