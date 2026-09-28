//! Candidate-directed host repair. The default uses exact copy counts and
//! tabu moves; alternatives learn NAE constraints or search a bounded edit ball.
//!
//! A hyperedge is the edge set of one labelled copy of F; it must contain both
//! colours in a refuter. The learned variant remembers some of these constraints.
//! Hosts are NEVER canonicalized during a walk: constraints fix edge labels.
//! A satisfied learned bank is only permission to call the full oracle again.

use crate::base::{BitNum, Bits, Graph, MAX_SIZE, random_graph};
use crate::perm::Perm;
use crate::tools::{build_sorted_row, find_subgraph_ss, read_graphs};
use rand::{Rng, SeedableRng, rngs::StdRng, seq::SliceRandom};
use rayon::prelude::*;
use std::collections::HashSet;
use std::io::Write;
use std::time::{Duration, Instant};

mod counted;
mod neighbourhood;
mod penalty;

#[derive(Debug, clap::Args)]
pub struct CountArgs {
    /// Host order (up to this build's MAX_SIZE)
    size: usize,
    /// Decimal candidate F
    candidate: BitNum,
    /// Hosts to score (decimal or graph6, first CSV column)
    path: String,
    /// Seconds per host, zero disables; incomplete counts are never reported
    #[arg(long, default_value_t = 1.0)]
    seconds: f64,
    #[arg(long, default_value_t = 1)]
    jobs: usize,
}

pub fn count_hosts(args: CountArgs) {
    counted::count_hosts(args);
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Method {
    Learned,
    Counted,
    Neighbourhood,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Moves {
    /// Restrict single flips to edges of a current monochromatic copy (old behaviour)
    Focused,
    /// Also allow preparatory flips outside all current monochromatic copies
    All,
}

#[derive(Clone, Debug)]
pub struct Checkpoint {
    pub host: Graph,
    pub copies: u64,
    pub flips: u64,
}

#[derive(Debug, Default)]
pub struct Diagnostics {
    pub archive: Vec<Checkpoint>,
    pub pair_evaluations: u64,
    pub pair_moves: u64,
    pub outside_moves: u64,
    pub best_step: u64,
    pub penalty_updates: u64,
    pub penalty_constraints: usize,
}

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Host order (up to this build's MAX_SIZE)
    size: usize,
    /// Candidate file: decimal or graph6, first CSV column
    path: String,
    /// Exact counts, learned constraints, or copy-directed bounded edit search
    #[arg(long, value_enum, default_value_t = Method::Counted)]
    method: Method,
    /// Tabu tenure in flips (unless a move improves the best score)
    #[arg(long, default_value_t = 7)]
    tabu: usize,
    /// Eligible single-edge moves; improving single flips are always focused anyway
    #[arg(long, value_enum, default_value_t = Moves::All)]
    moves: Moves,
    /// Try exact two-edge scores every kth non-improving decision; zero disables
    #[arg(long, default_value_t = 0)]
    pair_every: u64,
    /// Boost violated-copy penalties every kth weighted stall; zero disables
    #[arg(long, default_value_t = 0)]
    penalty_every: u64,
    /// Extra weight per retained violated copy at a penalty update
    #[arg(long, default_value_t = 1)]
    penalty_step: u32,
    /// Maximum labelled copy masks in penalty memory (FIFO eviction)
    #[arg(long, default_value_t = 4096)]
    penalty_cap: usize,
    /// Halve extra weights every kth penalty update; zero disables
    #[arg(long, default_value_t = 64)]
    penalty_decay: u64,
    /// Maximum distinct violated copies retained per colour per evaluation
    #[arg(long, default_value_t = 64)]
    penalty_batch: usize,
    /// Maximum archived host classes per counted walk; zero disables
    #[arg(long, default_value_t = 8)]
    archive: usize,
    /// Archive only hosts within this many copies of the walk's best score
    #[arg(long, default_value_t = 32)]
    archive_slack: u64,
    /// Maximum distance from the seed for the neighbourhood method
    #[arg(long, default_value_t = 4)]
    depth: usize,
    /// Host seed file, repeatable; smaller-order decimal seeds gain isolates
    #[arg(long)]
    seed_file: Vec<String>,
    /// Prefer seeds avoiding the candidate in at least one colour; fall back to all
    #[arg(long)]
    free_seeds: bool,
    /// Additional decimal host seeds
    #[arg(long, value_delimiter = ',')]
    seeds: Vec<BitNum>,
    /// Independent attempts per candidate (fresh state each time)
    #[arg(long, default_value_t = 8)]
    restarts: usize,
    /// First restart index, for reproducible independently scheduled attempts
    #[arg(long, default_value_t = 0)]
    restart_offset: usize,
    /// Flips per walk, or visited non-root states in neighbourhood search
    #[arg(long, default_value_t = 1_000_000)]
    steps: u64,
    /// Seconds per attempt; zero disables. Exact existence calls have a soft limit
    #[arg(long, default_value_t = 10.0)]
    seconds: f64,
    /// Maximum learned copy masks per walk; memory is O(cap * edges(F))
    #[arg(long, default_value_t = 100_000)]
    max_constraints: usize,
    /// Witness attempts per colour per round, with random vertex labels
    #[arg(long, default_value_t = 32)]
    batch: usize,
    /// Random-move probability when the best available move does not improve the score
    #[arg(long, default_value_t = 0.3)]
    noise: f64,
    /// Distinct edges to flip in a seed before starting a walk
    #[arg(long, default_value_t = 0)]
    perturb: usize,
    /// Every kth walk starts uniformly at random; zero uses seeds only
    #[arg(long, default_value_t = 4)]
    random_every: usize,
    /// Reproducible per-candidate/per-restart randomness; use --seconds 0 for exact traces
    #[arg(long, default_value_t = 0)]
    rng_seed: u64,
    /// Candidates processed concurrently (each walk itself is single-threaded)
    #[arg(long, default_value_t = 1)]
    jobs: usize,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub method: Method,
    pub tabu: usize,
    pub moves: Moves,
    pub pair_every: u64,
    pub penalty_every: u64,
    pub penalty_step: u32,
    pub penalty_cap: usize,
    pub penalty_decay: u64,
    pub penalty_batch: usize,
    pub archive: usize,
    pub archive_slack: u64,
    pub depth: usize,
    pub steps: u64,
    pub seconds: f64,
    pub max_constraints: usize,
    pub batch: usize,
    pub noise: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            method: Method::Counted,
            tabu: 7,
            moves: Moves::All,
            pair_every: 0,
            penalty_every: 0,
            penalty_step: 1,
            penalty_cap: 4096,
            penalty_decay: 64,
            penalty_batch: 64,
            archive: 8,
            archive_slack: 32,
            depth: 4,
            steps: 1_000_000,
            seconds: 10.0,
            max_constraints: 100_000,
            batch: 32,
            noise: 0.3,
        }
    }
}

impl Config {
    fn validate_penalties(&self) {
        if self.penalty_every == 0 {
            return;
        }
        assert!(
            matches!(self.method, Method::Counted),
            "penalties require --method counted"
        );
        assert_eq!(
            self.pair_every, 0,
            "penalties currently require --pair-every 0"
        );
        assert!(
            self.penalty_cap > 0 && self.penalty_batch > 0,
            "penalty-cap and penalty-batch must be positive"
        );
        assert!(
            (1..=1_000_000).contains(&self.penalty_step),
            "penalty-step must be in 1..=1000000"
        );
    }
}

#[derive(Debug)]
pub struct Outcome {
    pub host: Graph,
    pub status: &'static str,
    pub flips: u64,
    /// Colour searches/evaluations, including relabelled batch searches.
    pub checks: u64,
    pub constraints: usize,
    pub unsatisfied: usize,
    pub elapsed: Duration,
    pub oracle_time: Duration,
    /// Exact score at the saved host (counted method, or zero for a found refuter).
    pub best_copies: Option<u64>,
    pub diagnostics: Diagnostics,
}

const ABSENT: usize = usize::MAX;

/// Incremental counts and O(1) membership in the list of violated constraints.
/// Every constraint has the same arity, namely |E(F)|, which must be >= 2.
struct Bank {
    host: BitNum,
    arity: u32,
    masks: Vec<BitNum>,
    seen: HashSet<BitNum>,
    red: Vec<u32>,
    occurrences: Vec<Vec<usize>>,
    bad: Vec<usize>,
    bad_pos: Vec<usize>,
    // Number of currently mixed constraints made monochromatic by each flip.
    breaks: Vec<usize>,
    makes: Vec<usize>,
}

fn bits(mut mask: BitNum) -> impl Iterator<Item = usize> {
    std::iter::from_fn(move || {
        if mask == 0 {
            return None;
        }
        let v = mask.trailing_zeros() as usize;
        mask &= mask - 1;
        Some(v)
    })
}

impl Bank {
    fn new(host: Graph, arity: usize) -> Self {
        assert!(arity >= 2);
        let variables = Graph::triangle(host.size);
        Self {
            host: host.bits(),
            arity: arity as u32,
            masks: Vec::new(),
            seen: HashSet::new(),
            red: Vec::new(),
            occurrences: vec![Vec::new(); variables],
            bad: Vec::new(),
            bad_pos: Vec::new(),
            breaks: vec![0; variables],
            makes: vec![0; variables],
        }
    }

    fn is_bad(&self, count: u32) -> bool {
        count == 0 || count == self.arity
    }

    // At most two variables can break a mixed NAE constraint (two when arity=2).
    fn critical(&self, mask: BitNum, red: u32) -> BitNum {
        let mut critical = 0;
        if red == 1 {
            critical |= mask & self.host;
        }
        if red == self.arity - 1 {
            critical |= mask & !self.host;
        }
        critical
    }

    fn add(&mut self, mask: BitNum) -> bool {
        assert_eq!(mask.count_ones(), self.arity);
        assert_eq!(mask >> self.occurrences.len(), 0);
        if !self.seen.insert(mask) {
            return false;
        }
        let id = self.masks.len();
        let red = (mask & self.host).count_ones();
        for edge in bits(mask) {
            self.occurrences[edge].push(id);
        }
        for edge in bits(self.critical(mask, red)) {
            self.breaks[edge] += 1;
        }
        self.masks.push(mask);
        self.red.push(red);
        self.bad_pos.push(ABSENT);
        if self.is_bad(red) {
            self.mark_bad(id);
        }
        true
    }

    fn mark_bad(&mut self, id: usize) {
        debug_assert_eq!(self.bad_pos[id], ABSENT);
        self.bad_pos[id] = self.bad.len();
        self.bad.push(id);
        for v in bits(self.masks[id]) {
            self.makes[v] += 1;
        }
    }

    fn mark_good(&mut self, id: usize) {
        let pos = self.bad_pos[id];
        debug_assert_ne!(pos, ABSENT);
        self.bad.swap_remove(pos);
        if pos < self.bad.len() {
            self.bad_pos[self.bad[pos]] = pos;
        }
        self.bad_pos[id] = ABSENT;
        for v in bits(self.masks[id]) {
            self.makes[v] -= 1;
        }
    }

    fn flip(&mut self, edge: usize) {
        let was_red = self.host & (1 << edge) != 0;
        // Remove the old contributions before changing the assignment.
        for &id in &self.occurrences[edge] {
            for v in bits(self.critical(self.masks[id], self.red[id])) {
                self.breaks[v] -= 1;
            }
        }
        self.host ^= 1 << edge;
        for i in 0..self.occurrences[edge].len() {
            let id = self.occurrences[edge][i];
            let was_bad = self.is_bad(self.red[id]);
            self.red[id] = if was_red {
                self.red[id] - 1
            } else {
                self.red[id] + 1
            };
            let now_bad = self.is_bad(self.red[id]);
            if was_bad && !now_bad {
                self.mark_good(id);
            }
            if !was_bad && now_bad {
                self.mark_bad(id);
            }
            for v in bits(self.critical(self.masks[id], self.red[id])) {
                self.breaks[v] += 1;
            }
        }
    }

    fn choose(&self, rng: &mut StdRng, noise: f64) -> usize {
        let mask = self.masks[*self.bad.choose(rng).unwrap()];
        let score = |v: usize| self.breaks[v] as i64 - self.makes[v] as i64;
        let best = bits(mask).map(score).min().unwrap();
        // Focused min-conflicts, with random steps at local minima/plateaux.
        let random = best >= 0 && rng.gen_bool(noise);
        let mut choice = 0;
        let mut ties = 0;
        for v in bits(mask) {
            if random || score(v) == best {
                ties += 1;
                if rng.gen_range(0..ties) == 0 {
                    choice = v;
                }
            }
        }
        choice
    }
}

/// No UNSAT/proof claim is ever returned: only a verified two-colour absence,
/// or the named finite limit. Embedding calls are exact and not interruptible;
/// the time limit is consequently soft by at most one call.
pub fn walk(pattern: &Graph, start: Graph, config: &Config, rng: &mut StdRng) -> Outcome {
    config.validate_penalties();
    assert_eq!(pattern.size, start.size);
    assert!((1..=MAX_SIZE).contains(&pattern.size));
    let variables = Graph::triangle(pattern.size);
    assert_eq!(pattern.bits() >> variables, 0);
    assert_eq!(start.bits() >> variables, 0);
    assert!(config.seconds.is_finite() && config.seconds >= 0.0);
    assert!((0.0..=1.0).contains(&config.noise));
    assert!(config.batch > 0 && config.max_constraints > 0);
    if matches!(config.method, Method::Counted) && pattern.edge_count() >= 2 {
        return counted::walk(pattern, start, config, rng);
    }
    if matches!(config.method, Method::Neighbourhood) && pattern.edge_count() >= 2 {
        return neighbourhood::walk(pattern, start, config, rng);
    }
    let timer = Instant::now();
    let deadline = (config.seconds > 0.0).then(|| Duration::from_secs_f64(config.seconds));
    let row = build_sorted_row(pattern);
    let mut flips = 0;
    let mut checks = 0;
    let mut oracle_time = Duration::ZERO;
    if pattern.edge_count() < 2 {
        return Outcome {
            host: start,
            status: "trivial-universal",
            flips,
            checks,
            constraints: 0,
            unsatisfied: 0,
            elapsed: timer.elapsed(),
            oracle_time,
            best_copies: None,
            diagnostics: Diagnostics::default(),
        };
    }
    let mut bank = Bank::new(start, pattern.edge_count());
    let mut next_progress = Duration::from_secs(10);
    let status = 'search: loop {
        if deadline.is_some_and(|d| timer.elapsed() >= d) {
            break "time-limit";
        }
        if timer.elapsed() >= next_progress {
            eprintln!(
                "repair F={}: {:.1}s, flips={}, checks={}, constraints={}, violated={}",
                pattern.bits(),
                timer.elapsed().as_secs_f64(),
                flips,
                checks,
                bank.masks.len(),
                bank.bad.len()
            );
            next_progress = timer.elapsed() + Duration::from_secs(10);
        }
        if bank.bad.is_empty() {
            // Learn from both colours, not just the colour of the latest flip.
            let mut found = false;
            for complement in [false, true] {
                let mut host = Graph::from_bits(start.size, bank.host);
                if complement {
                    host = host.complement();
                }
                for attempt in 0..config.batch {
                    if deadline.is_some_and(|d| timer.elapsed() >= d) {
                        break 'search "time-limit";
                    }
                    let before = Instant::now();
                    let permutation = if attempt == 0 {
                        Perm::identity(start.size)
                    } else {
                        Perm::random(rng, start.size)
                    };
                    let relabelled = host.renumber(&permutation);
                    let copy = find_subgraph_ss(pattern, &row, &relabelled)
                        .map(|copy| copy.unrenumber(&permutation));
                    checks += 1;
                    oracle_time += before.elapsed();
                    let Some(copy) = copy else {
                        break;
                    };
                    found = true;
                    // Map each witness back to the ORIGINAL labelled host.
                    // Random labels diversify witnesses, but do not change H
                    // or the meaning of any constraint already in the bank.
                    assert_eq!(copy.bits() & !host.bits(), 0);
                    if bank.masks.len() == config.max_constraints {
                        break 'search "constraint-limit";
                    }
                    bank.add(copy.bits());
                }
            }
            if !found {
                break "refuted";
            }
            assert!(
                !bank.bad.is_empty(),
                "oracle witness must violate its new constraint"
            );
        }
        // Still check a final assignment after the last permitted flip.
        if flips == config.steps {
            break "step-limit";
        }
        let edge = bank.choose(rng, config.noise);
        bank.flip(edge);
        flips += 1;
    };
    Outcome {
        host: Graph::from_bits(start.size, bank.host),
        status,
        flips,
        checks,
        constraints: bank.masks.len(),
        unsatisfied: bank.bad.len(),
        elapsed: timer.elapsed(),
        oracle_time,
        best_copies: None,
        diagnostics: Diagnostics::default(),
    }
}

// Stable independent streams, insensitive to scheduling and candidate ordering.
fn stream_seed(seed: u64, candidate: BitNum, restart: usize) -> u64 {
    let wide = candidate as u128;
    let mut x = seed ^ wide as u64 ^ (wide >> 64) as u64;
    x = x.wrapping_add((restart as u64).wrapping_mul(0x9e3779b97f4a7c15));
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

pub fn run(args: Args) {
    assert!(
        (1..=MAX_SIZE).contains(&args.size),
        "unsupported host order"
    );
    assert!(args.jobs > 0 && args.restarts > 0);
    assert!(args.perturb <= Graph::triangle(args.size));
    assert!(args.seconds.is_finite() && args.seconds >= 0.0);
    assert!((0.0..=1.0).contains(&args.noise));
    assert!(args.batch > 0 && args.max_constraints > 0);
    let candidates: Vec<Graph> = read_graphs(args.size, &args.path).collect();
    let mut seeds: Vec<Graph> = args
        .seeds
        .iter()
        .map(|&h| Graph::from_bits(args.size, h))
        .collect();
    for file in &args.seed_file {
        seeds.extend(read_graphs::<Graph>(args.size, file));
    }
    for g in candidates.iter().chain(&seeds) {
        assert_eq!(
            g.bits() >> Graph::triangle(args.size),
            0,
            "graph does not fit host order: {}",
            g.bits()
        );
    }
    // Exact isomorphism/complement dedup ONLY here; a live bank fixes labels.
    let mut seen = HashSet::new();
    seeds.retain(|h| seen.insert(crate::canon::key(h).min(crate::canon::key(&h.complement()))));
    eprintln!(
        "repair: {} candidates, {} seed classes, {} restarts each, {} jobs; heuristic only",
        candidates.len(),
        seeds.len(),
        args.restarts,
        args.jobs
    );
    let config = Config {
        method: args.method,
        tabu: args.tabu,
        moves: args.moves,
        pair_every: args.pair_every,
        penalty_every: args.penalty_every,
        penalty_step: args.penalty_step,
        penalty_cap: args.penalty_cap,
        penalty_decay: args.penalty_decay,
        penalty_batch: args.penalty_batch,
        archive: args.archive,
        archive_slack: args.archive_slack,
        depth: args.depth,
        steps: args.steps,
        seconds: args.seconds,
        max_constraints: args.max_constraints,
        batch: args.batch,
        noise: args.noise,
    };
    config.validate_penalties();
    println!(
        "n,candidate,refuter,status,restart,walk_seed,flips,checks,constraints,violated,seconds,oracle_seconds,start_host,saved_host,distance,best_copies,method,source_seed,record,pair_evaluations,pair_moves,outside_moves,best_step,penalty_updates,penalty_constraints"
    );
    std::io::stdout().flush().unwrap();
    rayon::ThreadPoolBuilder::new()
        .num_threads(args.jobs)
        .build()
        .unwrap()
        .install(|| {
            candidates.par_iter().for_each(|pattern| {
                let mut order: Vec<usize> = (0..seeds.len()).collect();
                if args.free_seeds && !seeds.is_empty() {
                    let row = build_sorted_row(pattern);
                    let free: Vec<_> = order
                        .iter()
                        .copied()
                        .filter(|&i| {
                            find_subgraph_ss(pattern, &row, &seeds[i]).is_none()
                                || find_subgraph_ss(pattern, &row, &seeds[i].complement()).is_none()
                        })
                        .collect();
                    if !free.is_empty() {
                        order = free;
                    }
                    eprintln!(
                        "repair F={}: using {} of {} seed classes after one-colour-free filter",
                        pattern.bits(),
                        order.len(),
                        seeds.len()
                    );
                }
                order.shuffle(&mut StdRng::seed_from_u64(stream_seed(
                    args.rng_seed,
                    pattern.bits(),
                    usize::MAX,
                )));
                let end = args.restart_offset.checked_add(args.restarts).expect("restart index overflow");
                for restart in args.restart_offset..end {
                    let seed = stream_seed(args.rng_seed, pattern.bits(), restart);
                    let mut rng = StdRng::seed_from_u64(seed);
                    let random = seeds.is_empty()
                        || (args.random_every != 0 && (restart + 1) % args.random_every == 0);
                    // Number of seeded attempts before this one. Unlike a mutable
                    // cursor, this also works for independently scheduled restarts.
                    let seed_index = restart - if args.random_every == 0 { 0 } else { restart / args.random_every };
                    let source_seed = if random {
                        None
                    } else {
                        Some(seeds[order[seed_index % order.len()]].bits())
                    };
                    let mut start = if random {
                        random_graph(&mut rng, args.size)
                    } else {
                        let h = seeds[order[seed_index % order.len()]];
                        h.renumber(&Perm::random(&mut rng, args.size))
                    };
                    let mut edges: Vec<usize> = (0..Graph::triangle(args.size)).collect();
                    edges.shuffle(&mut rng);
                    for &edge in &edges[..args.perturb] {
                        start.edges.0.0 ^= 1 << edge;
                    }
                    let outcome = walk(pattern, start, &config, &mut rng);
                    let refuter = if outcome.status == "refuted" {
                        outcome.host.bits().to_string()
                    } else {
                        String::new()
                    };
                    let mut out = std::io::stdout().lock();
                    writeln!(
                        out,
                        "{},{},{},{},{},{},{},{},{},{},{:.6},{:.6},{},{},{},{},{:?},{},outcome,{},{},{},{},{},{}",
                        args.size,
                        pattern.bits(),
                        refuter,
                        outcome.status,
                        restart,
                        seed,
                        outcome.flips,
                        outcome.checks,
                        outcome.constraints,
                        outcome.unsatisfied,
                        outcome.elapsed.as_secs_f64(),
                        outcome.oracle_time.as_secs_f64(),
                        start.bits(),
                        outcome.host.bits(),
                        (start.bits() ^ outcome.host.bits()).count_ones(),
                        outcome
                            .best_copies
                            .map(|x| x.to_string())
                            .unwrap_or_default(),
                        args.method,
                        source_seed.map(|x| x.to_string()).unwrap_or_default(),
                        outcome.diagnostics.pair_evaluations,
                        outcome.diagnostics.pair_moves,
                        outcome.diagnostics.outside_moves,
                        outcome.diagnostics.best_step,
                        outcome.diagnostics.penalty_updates,
                        outcome.diagnostics.penalty_constraints,
                    )
                    .unwrap();
                    for checkpoint in &outcome.diagnostics.archive {
                        // Timing and evaluation totals belong to the outcome row
                        // only. These are seeds, never extra certificates.
                        writeln!(out,
                            "{},{},,checkpoint,{},{},{},0,0,0,0,0,{},{},{},{},{:?},{},checkpoint,0,0,0,{},0,0",
                            args.size, pattern.bits(), restart, seed, checkpoint.flips,
                            start.bits(), checkpoint.host.bits(),
                            (start.bits() ^ checkpoint.host.bits()).count_ones(), checkpoint.copies,
                            args.method, source_seed.map(|x| x.to_string()).unwrap_or_default(), checkpoint.flips,
                        ).unwrap();
                    }
                    out.flush().unwrap();
                    eprintln!(
                        "repair F={} restart={}: {}, {:.2}s, {} flips, {} constraints",
                        pattern.bits(),
                        restart,
                        outcome.status,
                        outcome.elapsed.as_secs_f64(),
                        outcome.flips,
                        outcome.constraints
                    );
                    if outcome.status == "refuted" || outcome.status == "trivial-universal" {
                        break;
                    }
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_method_has_repeatable_step_limited_traces() {
        let pattern = Graph::from_bits(6, 7);
        let start = Graph::from_bits(6, 0);
        for method in [Method::Learned, Method::Counted, Method::Neighbourhood] {
            let config = Config {
                method,
                tabu: 3,
                depth: 3,
                steps: 40,
                seconds: 0.0,
                max_constraints: 1000,
                batch: 4,
                noise: 0.3,
                ..Config::default()
            };
            let a = walk(&pattern, start, &config, &mut StdRng::seed_from_u64(92));
            let b = walk(&pattern, start, &config, &mut StdRng::seed_from_u64(92));
            assert_eq!(
                (
                    a.host,
                    a.status,
                    a.flips,
                    a.checks,
                    a.constraints,
                    a.best_copies
                ),
                (
                    b.host,
                    b.status,
                    b.flips,
                    b.checks,
                    b.constraints,
                    b.best_copies
                )
            );
        }
    }

    fn check_bank(bank: &Bank) {
        let mut bad = Vec::new();
        let mut breaks = vec![0; bank.occurrences.len()];
        let mut makes = vec![0; bank.occurrences.len()];
        for (id, &mask) in bank.masks.iter().enumerate() {
            let red = (mask & bank.host).count_ones();
            assert_eq!(bank.red[id], red);
            if bank.is_bad(red) {
                bad.push(id);
            }
            for (v, count) in breaks.iter_mut().enumerate() {
                let red1 = (mask & (bank.host ^ (1 << v))).count_ones();
                if !bank.is_bad(red) && bank.is_bad(red1) {
                    *count += 1;
                }
                if bank.is_bad(red) && !bank.is_bad(red1) {
                    makes[v] += 1;
                }
            }
        }
        let mut actual = bank.bad.clone();
        actual.sort_unstable();
        assert_eq!(actual, bad);
        assert_eq!(bank.breaks, breaks);
        assert_eq!(bank.makes, makes);
        for (id, &pos) in bank.bad_pos.iter().enumerate() {
            assert_eq!(pos != ABSENT, bank.is_bad(bank.red[id]));
            if pos != ABSENT {
                assert_eq!(bank.bad[pos], id);
            }
        }
    }

    #[test]
    fn incremental_bank_matches_recomputation() {
        let mut rng = StdRng::seed_from_u64(203);
        for arity in 2..=10 {
            let mut bank = Bank::new(random_graph(&mut rng, 5), arity);
            let mut edges: Vec<_> = (0..10).collect();
            for _ in 0..400 {
                edges.shuffle(&mut rng);
                let mask = edges[..arity].iter().fold(0, |acc, &v| acc | (1 << v));
                bank.add(mask);
                assert!(!bank.add(mask));
                bank.flip(rng.gen_range(0..10));
                check_bank(&bank);
            }
        }
    }

    #[test]
    fn small_walks_verified_by_permutations() {
        let config = Config {
            method: Method::Learned,
            tabu: 7,
            depth: 4,
            steps: 500,
            seconds: 0.0,
            max_constraints: 2000,
            batch: 3,
            noise: 0.3,
            ..Config::default()
        };
        let mut rng = StdRng::seed_from_u64(405);
        let mut found = 0;
        for _ in 0..100 {
            let pattern = random_graph(&mut rng, 5);
            let start = random_graph(&mut rng, 5);
            let result = walk(&pattern, start, &config, &mut rng);
            if result.status == "refuted" {
                found += 1;
                for p in crate::perm::all_perms(5) {
                    let copy = pattern.renumber(&p);
                    assert!(!copy.is_subgraph_of(&result.host));
                    assert!(!copy.is_subgraph_of(&result.host.complement()));
                }
            }
        }
        assert!(found > 30);
    }

    #[test]
    fn known_cycle_refuter_and_limits() {
        // R(3,3)=6: a C5 and its complement are triangle-free.
        let pattern = Graph::from_bits(5, 7);
        let cycle = Graph::from_fn(5, |a, b| b == a + 1 || (a == 0 && b == 4));
        let mut config = Config {
            method: Method::Learned,
            tabu: 7,
            depth: 4,
            steps: 0,
            seconds: 0.0,
            max_constraints: 100,
            batch: 1,
            noise: 0.3,
            ..Config::default()
        };
        let mut rng = StdRng::seed_from_u64(4);
        assert_eq!(walk(&pattern, cycle, &config, &mut rng).status, "refuted");
        assert_eq!(
            walk(&pattern, Graph::from_bits(5, 0), &config, &mut rng).status,
            "step-limit"
        );
        config.steps = 10_000;
        assert_eq!(
            walk(&pattern, Graph::from_bits(5, 0), &config, &mut rng).status,
            "refuted"
        );
        config.max_constraints = 1;
        assert_eq!(
            walk(&pattern, Graph::from_bits(5, 0), &config, &mut rng).status,
            "constraint-limit"
        );
        config.seconds = 1e-12;
        assert_eq!(
            walk(&pattern, cycle, &config, &mut rng).status,
            "time-limit"
        );
    }
}
