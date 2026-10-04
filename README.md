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

### Candidate-directed counterexample repair

`ingraph-repair` starts from host seeds (or random hosts), and permits both
adding and deleting edges. Its default `--method counted` minimizes the
**exact number of distinct monochromatic copies** of the candidate, with
tabu moves to leave local minima. It computes every one-edge score change
in one traversal of monochromatic and one-wrong-edge copies. Unlike `seek`,
this is a heuristic search, not an exhaustive universality test.

```sh
target/release/graphy ingraph-repair 14 output/batches/all14 \
    --seed-file output/somes-14.txt --free-seeds \
    --restarts 16 --seconds 10 --jobs 4 --rng-seed 20260926 \
    > output/repair14.csv 2> output/repair14.log
```

Budgets apply **per restart per candidate**. `--steps` caps flips; `--seconds 0`
disables the clock limit for reproducible step-limited traces. Seeds are
deduplicated up to isomorphism and complement, then relabelled at each restart.
`--free-seeds` prefers seeds avoiding the candidate in at least one colour,
falling back to the full pool if none do. Every fourth restart is random;
`--random-every 0` uses only supplied seeds. `--perturb k` changes exactly k
distinct edges before a walk. Default builds currently support n <= 16.

CSV output streams an `outcome` row per restart and stops that candidate after a hit.
`refuter` is filled **only** after both colours pass an exact absence check.
`saved_host` holds the best scored host for `counted`, the last host for
`learned`, or the seed for an unsuccessful `neighbourhood` search. Labels
are arbitrary, not minimum-decimal. `best_copies` is an exact score, not a
distance to a refuter; blank means no score was completed. `walk_seed` is
the derived internal RNG seed, not the original `--rng-seed` argument.
Clock limits are soft around exact existence calls; counted traversals
check the clock internally and discard incomplete counts.
For `counted`, `checks` is twice the number of attempted score evaluations,
plus two final absence checks on success; an interrupted evaluation may not
have reached its second colour. For `neighbourhood`, `flips` counts distinct
visited non-root states, not the distance of the final host from the seed.

Counted walks now default to `--moves all`: a flip need not touch a current
monochromatic copy. Improving single flips always do touch one, but allowing
other edges can make escape moves much cheaper. `--moves focused` retains
the old restriction. `--tabu` is the number of subsequent flips an edge is
excluded for, unless changing it would beat the walk's best score. `--noise`
is the probability of a random allowed single flip when the best allowed
single flip is non-improving; it is not a probability applied at every step.

`--pair-every 8` optionally scores **all pairs of distinct edges** at every
eighth non-improving decision. It chooses a pair only if its exact joint
delta beats the best allowed single flip, respecting tabu/aspiration. Pairs
equivalent to just relabelling or complementing the current host are skipped.
Both edges count against `--steps`. This costs an extra traversal through
two-wrong-edge copies; it is disabled by default (`--pair-every 0`), and can
reduce the number of flips substantially at the same wall-clock budget.
Pair choices consider all edges regardless of `--moves`.

`--penalty-every 1` enables experimental adaptive copy penalties. At each
weighted stall it increases the extra weight of up to `--penalty-batch 64`
distinct violated labelled copies **per colour**, by `--penalty-step 1`.
Moves minimize the exact raw delta plus the extra weighted delta. The memory
holds at most `--penalty-cap 4096` copy masks, with FIFO eviction; weights
are halved every `--penalty-decay 64` updates (zero disables decay). A copy
keeps its weight when satisfied, so returning to it becomes less attractive.
Penalty updates are cumulative non-improving decisions, not elapsed time.
This is a bounded heuristic, not a completeness guarantee. It currently
requires `--method counted --pair-every 0`, and is **off by default**.
Archives, tabu aspiration, `best_copies`, and the acceptance gate always use
the **unweighted** count. The same flags work in `refine_repair.py`.

`--archive 8 --archive-slack 32` keeps a bounded sample of distinct
isomorphism/complement classes within 32 copies of the best visited score.
Lower scores take priority; ties use a deterministic per-walk sampling rank.
The best host is always saved separately. Archive records have
`record=checkpoint`, `status=checkpoint`, and no `refuter`; their scores are
complete exact counts. They stream with the outcome when a restart finishes.
`--archive 0` restores outcome-only output. Canonicalization is used only to
compare classes, never to change a live walk's labels. Archive size does not
grow with walk length.

Use `record=outcome` when counting attempts or comparing flip totals.
Checkpoint/seed rows have zero timing and evaluation totals to avoid double
counting; an archive row's `flips` says when that host was first retained.
Diagnostics include `pair_evaluations`, `pair_moves`, `outside_moves`
(edge flips outside the current copy support), `best_step`, `penalty_updates`,
and `penalty_constraints` (final memory size, not an exact total copy count).

Continue from the best hosts **for each candidate**, without mixing them up:

```sh
python3 scripts/refine_repair.py output/repair14.csv \
    --candidates output/batches/all14 --keep 8 \
    --restarts 8 --seconds 30 --perturb 4 --jobs 4 \
    --rng-seed 20260927 > output/repair14-refined.csv 2> output/repair14-refined.log
```

Here `--keep` counts exact **isomorphism/complement classes**, not labelled
rows. The helper deduplicates before truncating, using
`graphy canon --internal-labels --complement` (ordinary `canon` is unchanged).
Selected input seeds are copied into the output as `record=seed`, so a later
short/perturbed run cannot silently discard the input's best checkpoint.
Pass multiple old CSVs to recover diversity that older runs discarded.
`--prepare-only` writes this selected pool (and any cross-hits), without
launching walks; the resulting CSV is directly resumable.
`--max-score` is an absolute copy-count ceiling on eligible seeds;
`--archive-slack` is a relative allowance within each new walk.

The helper schedules **individual restarts**, including for a single selected
`--candidate NUMBER`, so `--jobs 6` can use six cores in a focused campaign.
The Rust command itself still parallelizes candidates. `--restart-offset`
preserves restart identities when splitting a campaign, provided the seed
pool and other parameters stay fixed. Time-limited traces still depend on
machine load. The helper defaults to `--tabu 3 --noise 0.2`; the Rust command
retains defaults 7 and 0.3 respectively.

Selected input seeds, input certificates, and newly found witnesses are
checked against the other selected
candidates with the separate existence matcher. Cross-hits are emitted as
ordinary verified refutations with `record=cross-refutation`. Future jobs
for a refuted candidate are skipped; already running jobs finish (at most
`jobs-1` extra attempts). `--no-share` disables cross-checking. Neither
filtering nor sharing edits your survivor files.

Candidate-specific `free-close` outputs can be screened and imported too:

```sh
python3 scripts/free_close_seeds.py 14 --per-layer 32 --keep 16 --jobs 4 \
    --rng-seed 20260928 > output/free-close14-seeds.csv 2> output/free-close14-seeds.log
python3 scripts/refine_repair.py output/free-close14-seeds.csv \
    --keep 16 --restarts 4 --seconds 5 --perturb 0 --jobs 4 \
    > output/free-close14-repair.csv 2> output/free-close14-repair.log
```

The importer defaults to `output/runs14/F-*/found-*.csv`, restricted to
`output/batches/all14`. It samples each edge-count layer separately, dedups
exact isomorphism/complement classes, and **rechecks** both colour counts.
Only fully scored, genuinely F-free hosts are retained, ranked by total
copies; the old boolean column is not trusted as a certificate. `--per-layer 0`
scores all raw hosts; `--keep 0` exports all completed valid scores. The
default counting timeout is 0.25 seconds **per host**, not per candidate.
The `record=free-close` rows include original source paths and both colour
counts; their timing fields describe screening, not repair attempts.
Use a free-close-only input for a seed experiment: mixing it immediately
with low-scoring old checkpoints may discard every new seed at `--keep`.

For stand-alone exact scoring, without near-miss or flip-delta enumeration:

```sh
target/release/graphy ingraph-count 14 482968729600 HOSTS.txt --seconds 1 --jobs 4
```

The output has red, blue and total copy counts, or blank counts on timeout.
The budget is per host; zero disables it. Zero totals also pass the separate
absence matcher before receiving `status=refuted`. Parallel output order
is unspecified. Neither command claims that a low copy count is an edit
distance, nor that a maximum-density F-free host is necessarily a good seed.

Alternative methods:

- `--method learned` accumulates labelled-copy NAE constraints, then flips
  edges to repair that sampled bank. `--max-constraints` bounds memory;
  satisfying the bank is **not** success without the full absence checks.
- `--method neighbourhood --depth 4` branches on edges of actual surviving
  copies to search the whole radius-four edit ball. `--steps` now caps
  visited non-root states. `neighbourhood-exhausted` rules out only that
  ball, not all refuters; any other limit is inconclusive even within it.

Neither normal exhaustion nor a positive `best_copies` proves universality.
Independently check returned certificates with:

```sh
awk -F, 'NR == 1 || $4 == "refuted"' output/repair14-refined.csv > output/repair14-certs.csv
python3 research/verify_refutations.py output/repair14-certs.csv
```

The verifier intentionally rejects an empty certificate file. Controls,
measurements, near-misses, and longer-run suggestions are in
[the September 26 research note](research/2026-09-26.md). The archive,
escape-move and pair-scoring follow-up is in
[the September 27 note](research/2026-09-27.md). Adaptive penalties and
screening the old free-close seeds are covered in
[the September 28 note](research/2026-09-28.md).

### Incremental SAT block repair (optional)

`ingraph-sat` uses RustSAT **0.7.5**, pinned to its bundled **CaDiCaL 2.2.1**.
It is a separate experimental command; existing repair commands and default
builds do not require SAT. Building the optional feature needs C/C++ compilers
and **libclang** for bindgen, but no system CaDiCaL or Python SAT package.
If your shell already supplies a working libclang, use
`cargo build -r --features native,sat` directly. A fallback for the Nix toolchain:

```sh
nix-shell -p rustPlatform.bindgenHook --run 'env -u NIX_ENFORCE_NO_NATIVE cargo build -r --features native,sat'
```

The Nix hook supplies libclang and its header-search environment. Merely finding
the host system's libclang may fail when it is loaded by a Nix-built executable.
The explicit unset allows the requested `native` optimization; otherwise the Nix
compiler wrapper may discard `-march=native`. A correctly configured
`LIBCLANG_PATH` also avoids needing the temporary shell.
On other toolchains, install the normal libclang development package and use
`cargo build -r --features sat` (set `LIBCLANG_PATH` if it is not discovered).
`native` optimizes Nauty; optionally set `CXXFLAGS=-march=native` to also optimize
CaDiCaL for this CPU. Such binaries are not portable to older CPUs.

A focused example, using the two four-copy n=15 seeds:

```sh
python3 scripts/sat_repair.py research/repair15-seeds-2026-09-28-followup.csv \
    --candidate 105898625070080 --keep 2 --restarts 2 --jobs 2 \
    --blocks 8 --block-size 48 --seconds 5 --max-constraints 500000 \
    --rng-seed 20261002 > output/sat15.csv 2> output/sat15.log
```

The Python helper only selects candidate-specific seeds and schedules independent
Rust processes. It accepts earlier repair CSVs and its own output. Within each
process, successive blocks share clauses and solver learning. Results flush
after every block. On Ctrl-C or SIGTERM the helper terminates its active children.
It does not automatically cross-check hits against other candidates.

Important parameter meanings:

- `--block-size K` frees K edges, fixing all other edges to a chosen seed.
  This is **not** a radius-K search: it explores all assignments of that particular
  edge subset. Zero frees all edges. Seeds are cycled across blocks.
  `--free-edges MASK` instead prescribes the exact decimal edge subset, useful
  for replaying a logged block or checking a known feasible repair region.
- `--block-mode directed` greedily hits current copies, then fills roughly half
  the block from their edge support and the remainder uniformly. It permits
  both colours to change. `random` samples the entire block uniformly.
- `--block-mode vertex --vertices R` frees **every edge incident to R selected
  vertices**, ignoring `--block-size`. The other n-R vertices retain their
  induced graph. Four vertices free 46/50/54 edges at n=14/15/16 respectively.
  The selector enumerates vertex subsets of the seed pool, deduplicates by
  isomorphism **and complementation of that fixed induced graph**, then shuffles
  the distinct classes. It tries at most one representative of each class per
  run, even after a timeout; additional restarts can revisit them. Without seeds,
  it makes one random seed for this catalogue. Catalogue preparation is outside
  the per-block timer.
- In vertex mode, `--max-vertices M` optionally expands an UNSAT region by one
  whole vertex, up to M. The failed-assumption core ranks vertices by the number
  of core edges they touch, with random tie-breaking. Each expansion keeps the
  anchor and labels, shares the solver, and consumes another block/time budget.
  Timeouts and other limits **do not** trigger expansion. A full core gives no
  vertex preference; this is not core minimization. Expanded classes are also
  deduplicated, and only one expansion branch is followed from each UNSAT region.
  `--blocks` caps **all attempts**, including expansions; a finite catalogue can
  finish sooner. Completion does not imply every larger region was explored.
- `--seconds` is **per block**, including oracle work, not per restart. The
  limit is cooperative/soft; setup and final independent existence checks are
  not hard-interruptible. Zero disables the clock; `--rounds` still bounds SAT calls.
- `--batch` caps distinct witnesses per colour per oracle call (default 64).
  A capped/interrupted traversal never supplies an exact copy count or absence
  claim. More witnesses strengthen the formula but cost memory and solver work.
- `--max-constraints` caps stored full copy masks, each producing two clauses.
  Hitting the cap stops that process. `--reset-every K` instead allows periodic
  fresh solvers, sacrificing accumulated learning. Default zero retains state.
  This is not a hard total-memory limit: SAT's internally learned clauses also
  use memory.
- `--restarts` in the helper starts fresh independent solver processes; `--jobs`
  bounds concurrency, including for a single candidate. `--restart-offset` and
  `--rng-seed` permit additional streams. `--relabel` diversifies seed labels;
  `--factor` enables optional bounded variable addition. Both default off.

All retained clauses describe **full labelled copies**, never clauses simplified
under a previous block's fixed edges. The fixed values are temporary assumptions.
The solver proposes a colouring, the graph oracle adds clauses for monochromatic
copies, and the loop repeats. A provisional SAT model is not itself a refuter.
Success also passes the separate existence matcher in both colours.

CSV statuses distinguish `refuted`, `block-unsat`, `time-limit`, `sat-limit`,
`round-limit`, and `constraint-limit`. `block-unsat` rules out only the printed
`start_host`/`free_edges` subcube. An unrestricted UNSAT result is deliberately
labelled `unrestricted-unsat-unverified`: this tool does **not** emit checked
universality proofs. Timeouts and resource limits establish no infeasibility.

`best_copies` refers only to fully counted hosts; a blank is unknown, not zero.
`saved_host` is the best such host (or the anchor if none was completely counted);
`last_host` records the last SAT proposal. `constraints` is cumulative per solver,
while `added_constraints`, times, rounds and search statistics are per block.
The exact solver signature, anchor, free-edge mask and failed-assumption variable
mask (`core_edges`) are logged. A failed core need not be minimal. An empty core
on UNSAT gives `unrestricted-unsat-unverified`, even in a restricted query.
Vertex mode adds `free_vertices` (a vertex bit mask), `fixed_core` (an internal
canonical key, compare only at the same remaining order), and `parent_block`
(the UNSAT block that triggered expansion, otherwise blank). Core keys are not
least-decimal forms or stable identifiers across canonicalizer versions.

For a single seed without the Python helper:

```sh
target/release/graphy ingraph-sat 15 105898625070080 \
    --seeds 4554531801452033082124616721190 --blocks 4 --block-size 40 \
    --seconds 5 --dump-dir output/sat-debug > output/sat-debug.csv
```

`--dump-dir` optionally saves the collected DIMACS clauses plus that block's
fixed-edge units for replay. Existing files are never overwritten. These files
are formulas, **not proof certificates**; satisfiability of a collected formula
does not imply absence of uncollected copies. With the helper each task gets its
own subdirectory. Audit any `refuted` rows with `research/verify_refutations.py`,
using the same CSV filtering command as for ordinary repair above.

Tests: `cargo test --features sat`, plus
`python3 -m unittest discover -s scripts -p 'test_sat_repair.py'` after a SAT-enabled
release build. The latter independently checks small returned graphs and
exhaustively checks dumped small CNFs. Experiments and follow-up recommendations
are recorded in [the October 2 note](research/2026-10-02.md).
Whole-vertex blocks, core-guided expansion, fresh-solver comparisons and the
complete two-vertex scans are in [the October 3 note](research/2026-10-03.md).

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
