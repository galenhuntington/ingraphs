//! Incremental SAT separation: every stored mask is a FULL labelled copy of F.
//! Outside-block assignments are temporary assumptions, never permanent units.
//! SAT is provisional until both colours pass the graph oracle. Local UNSAT
//! rules out only the printed anchor/free-edge subcube, not the candidate.

use super::*;
use counted::Counter;
use rustsat::solvers::{
    ControlSignal, GetInternalStats, PhaseLit, Solve, SolveIncremental, SolverResult, Terminate,
};
use rustsat::types::{Lit, TernaryVal};
use rustsat_cadical::{CaDiCaL, Limit};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum BlockMode {
    /// Half the block from current-copy support, then uniformly fill the rest
    Directed,
    /// Uniform edge subset, without consulting current-copy support
    Random,
}

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Host order
    size: usize,
    /// Decimal candidate F (isolates implicit)
    candidate: BitNum,
    /// Decimal host seeds, in their current labels
    #[arg(long, value_delimiter = ',')]
    seeds: Vec<BitNum>,
    /// Additional seeds: decimal or graph6, FIRST CSV column (not repair CSVs)
    #[arg(long)]
    seed_file: Vec<String>,
    /// Blocks per run, sharing clauses and solver learning
    #[arg(long, default_value_t = 16)]
    blocks: usize,
    /// Free edges per block; zero frees ALL host edges
    #[arg(long, default_value_t = 32)]
    block_size: usize,
    /// Exact decimal free-edge mask, overriding block-size and block-mode
    #[arg(long)]
    free_edges: Option<BitNum>,
    #[arg(long, value_enum, default_value_t = BlockMode::Directed)]
    block_mode: BlockMode,
    /// Seconds per block including witness collection; zero disables
    #[arg(long, default_value_t = 5.0)]
    seconds: f64,
    /// Maximum SAT calls per block (zero makes no calls)
    #[arg(long, default_value_t = 10000)]
    rounds: usize,
    /// Distinct witnesses collected per colour per oracle call
    #[arg(long, default_value_t = 64)]
    batch: usize,
    /// Maximum permanent copy masks (two clauses each); stops the run at the cap
    #[arg(long, default_value_t = 200000)]
    max_constraints: usize,
    /// Conflict limit per SAT call; zero disables. Interrupted is never UNSAT
    #[arg(long, default_value_t = 0)]
    conflicts: i32,
    /// Recreate the solver every kth block; zero retains it for the whole run
    #[arg(long, default_value_t = 0)]
    reset_every: usize,
    /// Independently relabel the selected seed before each block
    #[arg(long)]
    relabel: bool,
    /// Enable CaDiCaL's optional bounded variable addition preprocessing
    #[arg(long)]
    factor: bool,
    #[arg(long, default_value_t = 0)]
    rng_seed: u64,
    /// Independent stream index, for externally scheduled parallel runs
    #[arg(long, default_value_t = 0)]
    restart: usize,
    /// Save each collected CNF plus fixed-edge units; files must not exist
    #[arg(long)]
    dump_dir: Option<PathBuf>,
}

#[derive(Clone, Debug)]
struct Config {
    seconds: f64,
    rounds: usize,
    batch: usize,
    max_constraints: usize,
    conflicts: i32,
}

/// All binding-specific operations live here. No raw FFI or solver internals.
struct Engine {
    solver: CaDiCaL<'static, 'static>,
    variables: usize,
    masks: Vec<BitNum>,
    seen: HashSet<BitNum>,
}

enum Answer {
    Model(BitNum),
    Unsat(BitNum), // variables participating in the fixed-assignment core
    Interrupted,
}

fn literal(edge: usize, value: bool) -> Lit {
    Lit::new(edge as u32, !value)
}

impl Engine {
    fn new(variables: usize, seed: u64, factor: bool) -> Self {
        let mut solver = CaDiCaL::default();
        solver
            .set_option("seed", (seed % i32::MAX as u64) as i32)
            .unwrap();
        solver.set_option("factor", i32::from(factor)).unwrap();
        // Protect all application variables, including those not yet in clauses,
        // against collisions with factor's extension variables.
        if variables != 0 {
            solver.declare_more_variables(variables as u32);
        }
        Self {
            solver,
            variables,
            masks: Vec::new(),
            seen: HashSet::new(),
        }
    }

    fn add_copy(&mut self, mask: BitNum) -> bool {
        assert_eq!(mask >> self.variables, 0);
        if !self.seen.insert(mask) {
            return false;
        }
        let clause: Vec<_> = bits(mask).map(|e| literal(e, true)).collect();
        self.solver.add_clause_ref(&clause).unwrap();
        let opposite: Vec<_> = clause.iter().map(|&lit| !lit).collect();
        self.solver.add_clause_ref(&opposite).unwrap();
        self.masks.push(mask);
        true
    }

    fn phases(&mut self, host: BitNum) {
        for e in 0..self.variables {
            self.solver
                .phase_lit(literal(e, host & (1 << e) != 0))
                .unwrap();
        }
    }

    fn solve(
        &mut self,
        host: BitNum,
        free: BitNum,
        timer: Instant,
        deadline: Option<Duration>,
        conflicts: i32,
    ) -> Answer {
        let assumptions: Vec<_> = (0..self.variables)
            .filter(|e| free & (1 << e) == 0)
            .map(|e| literal(e, host & (1 << e) != 0))
            .collect();
        self.solver.attach_terminator(move || {
            if deadline.is_some_and(|d| timer.elapsed() >= d) {
                ControlSignal::Terminate
            } else {
                ControlSignal::Continue
            }
        });
        self.solver
            .set_limit(Limit::Conflicts(if conflicts == 0 {
                -1
            } else {
                conflicts
            }))
            .unwrap();
        let result = self.solver.solve_assumps(&assumptions).unwrap();
        self.solver.detach_terminator();
        match result {
            SolverResult::Sat => {
                let mut model = 0;
                for e in 0..self.variables {
                    let value = match self.solver.lit_val(literal(e, true)).unwrap() {
                        TernaryVal::True => true,
                        TernaryVal::False => false,
                        TernaryVal::DontCare => host & (1 << e) != 0,
                    };
                    if value {
                        model |= 1 << e;
                    }
                }
                assert_eq!((model ^ host) & !free, 0, "SAT model violates a fixed edge");
                Answer::Model(model)
            }
            SolverResult::Unsat => {
                let core = self
                    .solver
                    .core()
                    .unwrap()
                    .iter()
                    .fold(0, |acc, lit| acc | (1 << lit.var().idx()));
                assert_eq!(core & free, 0);
                Answer::Unsat(core)
            }
            SolverResult::Interrupted => Answer::Interrupted,
        }
    }

    fn dump(&self, path: &Path, pattern: &Graph, host: BitNum, free: BitNum) {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .expect("cannot create CNF (existing files are never overwritten)");
        writeln!(
            file,
            "c n={} candidate={} anchor={} free_edges={}",
            pattern.size,
            pattern.bits(),
            host,
            free
        )
        .unwrap();
        writeln!(
            file,
            "c variable 1+index(a,b) is red iff true; full copy clauses, fixed units"
        )
        .unwrap();
        let fixed = self.variables - free.count_ones() as usize;
        writeln!(
            file,
            "p cnf {} {}",
            self.variables,
            2 * self.masks.len() + fixed
        )
        .unwrap();
        for &mask in &self.masks {
            for sign in [1, -1] {
                for e in bits(mask) {
                    write!(file, "{} ", sign * (e as i32 + 1)).unwrap();
                }
                writeln!(file, "0").unwrap();
            }
        }
        for e in 0..self.variables {
            if free & (1 << e) == 0 {
                writeln!(file, "{} 0", literal(e, host & (1 << e) != 0).to_ipasir()).unwrap();
            }
        }
    }
}

/// Greedily hit currently violated copies, fill to half a block from their
/// support, then sample the rest uniformly. Always permit both edge colours
/// when available: freeing only red support cannot undo new blue copies.
fn choose_block(
    host: Graph,
    copies: &[BitNum],
    size: usize,
    mode: BlockMode,
    rng: &mut StdRng,
) -> BitNum {
    let variables = Graph::triangle(host.size);
    let full = host.bits() | host.complement().bits();
    if size == 0 || size >= variables {
        return full;
    }
    let mut edges: Vec<_> = (0..variables).collect();
    edges.shuffle(rng);
    let mut free: BitNum = 0;
    if matches!(mode, BlockMode::Directed) {
        let target = size.div_ceil(2);
        while (free.count_ones() as usize) < target {
            let best = edges
                .iter()
                .copied()
                .filter(|e| free & (1 << e) == 0)
                .max_by_key(|e| {
                    copies
                        .iter()
                        .filter(|&&m| m & free == 0 && m & (1 << e) != 0)
                        .count()
                });
            let Some(e) = best else {
                break;
            };
            if !copies.iter().any(|&m| m & free == 0 && m & (1 << e) != 0) {
                break;
            }
            free |= 1 << e;
        }
        let support = copies.iter().fold(0, |a, &m| a | m);
        for &e in &edges {
            if free.count_ones() as usize == target {
                break;
            }
            if support & (1 << e) != 0 {
                free |= 1 << e;
            }
        }
    }
    for &e in &edges {
        if free.count_ones() as usize == size {
            break;
        }
        free |= 1 << e;
    }
    if matches!(mode, BlockMode::Directed) && size >= 2 {
        for color in [host.bits(), host.complement().bits()] {
            if color != 0 && free & color == 0 {
                let add = *edges.iter().find(|&&e| color & (1 << e) != 0).unwrap();
                let remove = free.trailing_zeros() as usize;
                free = (free ^ (1 << remove)) | (1 << add);
            }
        }
    }
    assert_eq!(free.count_ones() as usize, size);
    free
}

#[derive(Debug)]
struct Outcome {
    status: &'static str,
    free: BitNum,
    core: BitNum,
    host: Graph,
    last: Graph,
    best_copies: Option<u64>,
    initial_copies: Option<u64>,
    rounds: usize,
    checks: usize,
    added: usize,
    elapsed: Duration,
    sat_time: Duration,
    oracle_time: Duration,
    conflicts: usize,
    decisions: usize,
    propagations: usize,
}

fn validate_refuter(pattern: &Graph, host: &Graph) {
    let row = build_sorted_row(pattern);
    assert!(find_subgraph_ss(pattern, &row, host).is_none());
    assert!(find_subgraph_ss(pattern, &row, &host.complement()).is_none());
}

fn block(
    engine: &mut Engine,
    counter: &Counter,
    pattern: &Graph,
    anchor: Graph,
    config: &Config,
    select: impl FnOnce(&[BitNum]) -> BitNum,
) -> Outcome {
    let timer = Instant::now();
    let deadline = (config.seconds > 0.0).then(|| Duration::from_secs_f64(config.seconds));
    let expired = || deadline.is_some_and(|d| timer.elapsed() >= d);
    let initial_constraints = engine.masks.len();
    let counters = (
        engine.solver.conflicts(),
        engine.solver.decisions(),
        engine.solver.propagations(),
    );
    let before = Instant::now();
    let (initial, initial_copies) = counter.collect(anchor, timer, deadline, config.batch);
    let mut oracle_time = before.elapsed();
    let free = select(&initial);
    let mut out = Outcome {
        status: "round-limit",
        free,
        core: 0,
        host: anchor,
        last: anchor,
        best_copies: initial_copies,
        initial_copies,
        rounds: 0,
        checks: 1,
        added: 0,
        elapsed: Duration::ZERO,
        sat_time: Duration::ZERO,
        oracle_time: Duration::ZERO,
        conflicts: 0,
        decisions: 0,
        propagations: 0,
    };
    engine.phases(anchor.bits());
    let mut pending = initial;
    let mut score = initial_copies;
    out.status = loop {
        if score == Some(0) {
            let before = Instant::now();
            validate_refuter(pattern, &out.last);
            oracle_time += before.elapsed();
            out.host = out.last;
            out.best_copies = Some(0);
            break "refuted";
        }
        for &copy in &pending {
            assert!(copy & out.last.bits() == 0 || copy & !out.last.bits() == 0);
            if !engine.seen.contains(&copy) && engine.masks.len() < config.max_constraints {
                engine.add_copy(copy);
            }
        }
        if engine.masks.len() >= config.max_constraints {
            break "constraint-limit";
        }
        if expired() {
            break "time-limit";
        }
        if out.rounds >= config.rounds {
            break "round-limit";
        }
        let before = Instant::now();
        let answer = engine.solve(anchor.bits(), free, timer, deadline, config.conflicts);
        out.sat_time += before.elapsed();
        out.rounds += 1;
        match answer {
            Answer::Interrupted => break if expired() { "time-limit" } else { "sat-limit" },
            Answer::Unsat(core) => {
                out.core = core;
                // Even an unrestricted UNSAT result is not published as a proof:
                // this exploratory command does not produce a checked proof log.
                break if free.count_ones() as usize == engine.variables {
                    "unrestricted-unsat-unverified"
                } else {
                    "block-unsat"
                };
            }
            Answer::Model(model) => {
                out.last = Graph::from_bits(anchor.size, model);
                let before = Instant::now();
                (pending, score) = counter.collect(out.last, timer, deadline, config.batch);
                oracle_time += before.elapsed();
                out.checks += 1;
                // A satisfying model cannot violate any previously stored copy.
                assert!(
                    pending.iter().all(|m| !engine.seen.contains(m)),
                    "oracle found a copy already excluded by the SAT formula"
                );
                if let Some(q) = score {
                    if out.best_copies.is_none_or(|best| q < best) {
                        out.host = out.last;
                        out.best_copies = Some(q);
                    }
                }
            }
        }
    };
    out.added = engine.masks.len() - initial_constraints;
    out.elapsed = timer.elapsed();
    out.oracle_time = oracle_time;
    out.conflicts = engine.solver.conflicts() - counters.0;
    out.decisions = engine.solver.decisions() - counters.1;
    out.propagations = engine.solver.propagations() - counters.2;
    out
}

pub fn run(args: Args) {
    assert!(
        (1..=MAX_SIZE).contains(&args.size),
        "unsupported host order"
    );
    let variables = Graph::triangle(args.size);
    assert_eq!(
        args.candidate >> variables,
        0,
        "candidate exceeds host order"
    );
    if let Some(free) = args.free_edges {
        assert_eq!(free >> variables, 0, "free-edge mask exceeds host order");
    } else {
        assert!(
            args.block_size <= variables,
            "block size exceeds edge count (use 0 for all)"
        );
    }
    assert!(args.blocks > 0 && args.batch > 0 && args.max_constraints > 0);
    assert!(args.seconds.is_finite() && args.seconds >= 0.0 && args.conflicts >= 0);
    let pattern = Graph::from_bits(args.size, args.candidate);
    let mut seeds: Vec<Graph> = args
        .seeds
        .iter()
        .map(|&h| Graph::from_bits(args.size, h))
        .collect();
    for path in &args.seed_file {
        seeds.extend(read_graphs::<Graph>(args.size, path));
    }
    assert!(
        seeds.iter().all(|h| h.bits() >> variables == 0),
        "seed exceeds host order"
    );
    let seed = stream_seed(args.rng_seed, args.candidate, args.restart);
    let mut rng = StdRng::seed_from_u64(seed);
    let mut engine = Engine::new(variables, seed, args.factor);
    let mode = if args.free_edges.is_some() {
        "Prescribed"
    } else {
        match args.block_mode {
            BlockMode::Directed => "Directed",
            BlockMode::Random => "Random",
        }
    };
    eprintln!(
        "SAT: {}, F={}, {} blocks of {} edges, {}, seed={}, factor={}; local UNSAT is not universality",
        engine.solver.signature(),
        args.candidate,
        args.blocks,
        args.free_edges.map_or_else(
            || if args.block_size == 0 {
                variables
            } else {
                args.block_size
            },
            |m| m.count_ones() as usize
        ),
        mode,
        seed,
        args.factor
    );
    let counter = Counter::new(&pattern);
    let config = Config {
        seconds: args.seconds,
        rounds: args.rounds,
        batch: args.batch,
        max_constraints: args.max_constraints,
        conflicts: args.conflicts,
    };
    if let Some(dir) = &args.dump_dir {
        std::fs::create_dir_all(dir).unwrap();
    }
    println!(
        "n,candidate,refuter,status,restart,block,walk_seed,start_host,free_edges,free_size,initial_copies,saved_host,best_copies,last_host,distance,rounds,checks,constraints,added_constraints,seconds,sat_seconds,oracle_seconds,conflicts,decisions,propagations,core_edges,solver,block_mode,factor,record"
    );
    std::io::stdout().flush().unwrap();
    for b in 0..args.blocks {
        if b != 0 && args.reset_every != 0 && b % args.reset_every == 0 {
            engine = Engine::new(variables, seed.wrapping_add(b as u64), args.factor);
        }
        let mut anchor = if seeds.is_empty() {
            random_graph(&mut rng, args.size)
        } else {
            seeds[b % seeds.len()]
        };
        if args.relabel {
            anchor = anchor.renumber(&Perm::random(&mut rng, args.size));
        }
        let out = block(&mut engine, &counter, &pattern, anchor, &config, |copies| {
            args.free_edges.unwrap_or_else(|| {
                choose_block(anchor, copies, args.block_size, args.block_mode, &mut rng)
            })
        });
        let refuter = if out.status == "refuted" {
            out.host.bits().to_string()
        } else {
            String::new()
        };
        println!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{:.6},{:.6},{:.6},{},{},{},{},{},{},{},outcome",
            args.size,
            args.candidate,
            refuter,
            out.status,
            args.restart,
            b,
            seed,
            anchor.bits(),
            out.free,
            out.free.count_ones(),
            out.initial_copies
                .map(|q| q.to_string())
                .unwrap_or_default(),
            out.host.bits(),
            out.best_copies.map(|q| q.to_string()).unwrap_or_default(),
            out.last.bits(),
            (anchor.bits() ^ out.host.bits()).count_ones(),
            out.rounds,
            out.checks,
            engine.masks.len(),
            out.added,
            out.elapsed.as_secs_f64(),
            out.sat_time.as_secs_f64(),
            out.oracle_time.as_secs_f64(),
            out.conflicts,
            out.decisions,
            out.propagations,
            out.core,
            engine.solver.signature(),
            mode,
            args.factor
        );
        std::io::stdout().flush().unwrap();
        eprintln!(
            "SAT F={} restart={} block={}: {}, {:.2}s (SAT {:.2}s / oracle {:.2}s), {} rounds, {} copies, best={:?}",
            args.candidate,
            args.restart,
            b,
            out.status,
            out.elapsed.as_secs_f64(),
            out.sat_time.as_secs_f64(),
            out.oracle_time.as_secs_f64(),
            out.rounds,
            engine.masks.len(),
            out.best_copies
        );
        if let Some(dir) = &args.dump_dir {
            engine.dump(
                &dir.join(format!("block-{b:05}.cnf")),
                &pattern,
                anchor.bits(),
                out.free,
            );
        }
        if matches!(
            out.status,
            "refuted" | "constraint-limit" | "unrestricted-unsat-unverified"
        ) {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unlimited() -> Config {
        Config {
            seconds: 0.0,
            rounds: 10000,
            batch: 4,
            max_constraints: 100000,
            conflicts: 0,
        }
    }

    fn naive_refutes(pattern: &Graph, host: Graph) -> bool {
        crate::perm::all_perms(pattern.size).all(|p| {
            let copy = pattern.renumber(&p);
            !copy.is_subgraph_of(&host) && !copy.is_subgraph_of(&host.complement())
        })
    }

    #[test]
    fn pinned_backend_models_assumptions_and_recovery() {
        let mut engine = Engine::new(3, 9, false);
        assert_eq!(engine.solver.signature(), "cadical-2.2.1");
        assert!(engine.add_copy(3));
        assert!(!engine.add_copy(3));
        assert!(matches!(
            engine.solve(0, 0, Instant::now(), None, 0),
            Answer::Unsat(_)
        ));
        let Answer::Model(model) = engine.solve(0, 2, Instant::now(), None, 0) else {
            panic!()
        };
        assert_eq!(model & 3, 2);
        assert_eq!(
            engine.solver.lit_val(Lit::negative(0)).unwrap(),
            TernaryVal::True
        );
        assert_eq!(
            engine.solver.lit_val(Lit::negative(1)).unwrap(),
            TernaryVal::False
        );
        engine.add_copy(6);
        let Answer::Model(model) = engine.solve(0, 7, Instant::now(), None, 0) else {
            panic!()
        };
        assert_ne!(model & 3, 0);
        assert_ne!(model & 3, 3);
        assert_ne!(model & 6, 0);
        assert_ne!(model & 6, 6);
    }

    #[test]
    fn collected_witnesses_and_complete_scores_match_permutations() {
        let mut rng = StdRng::seed_from_u64(73);
        for _ in 0..80 {
            let pattern = random_graph(&mut rng, 5);
            if pattern.edge_count() == 0 {
                continue;
            }
            let host = random_graph(&mut rng, 5);
            let copies: HashSet<_> = crate::perm::all_perms(5)
                .map(|p| pattern.renumber(&p).bits())
                .filter(|&m| m & host.bits() == 0 || m & !host.bits() == 0)
                .collect();
            let counter = Counter::new(&pattern);
            for cap in [1, 3, 10000] {
                let (found, score) = counter.collect(host, Instant::now(), None, cap);
                assert!(found.len() <= cap * 2);
                assert!(found.iter().all(|m| copies.contains(m)));
                if let Some(q) = score {
                    assert_eq!(q as usize, copies.len());
                }
                if cap == 10000 {
                    assert_eq!(score, Some(copies.len() as u64));
                }
            }
            let (found, score) = counter.collect(host, Instant::now(), Some(Duration::ZERO), 4);
            assert!(found.is_empty());
            assert_eq!(score, None);
        }
    }

    #[test]
    fn incremental_subcubes_match_exhaustive_search() {
        let mut rng = StdRng::seed_from_u64(681);
        for _ in 0..32 {
            let pattern = random_graph(&mut rng, 4);
            if pattern.edge_count() < 2 {
                continue;
            }
            let counter = Counter::new(&pattern);
            // Shared solver across changing anchors: catches accidentally
            // permanent units and block-simplified clauses leaking between calls.
            let mut engine = Engine::new(6, 7, true);
            for _ in 0..8 {
                let anchor = random_graph(&mut rng, 4);
                let free = rng.gen_range(0..64);
                let possible = (0..64).any(|h| {
                    (h ^ anchor.bits()) & !free == 0
                        && naive_refutes(&pattern, Graph::from_bits(4, h))
                });
                let out = block(
                    &mut engine,
                    &counter,
                    &pattern,
                    anchor,
                    &unlimited(),
                    |_| free,
                );
                assert_eq!(out.status == "refuted", possible);
                if possible {
                    assert!(naive_refutes(&pattern, out.host));
                    assert_eq!((out.host.bits() ^ anchor.bits()) & !free, 0);
                } else {
                    assert!(matches!(
                        out.status,
                        "block-unsat" | "unrestricted-unsat-unverified"
                    ));
                }
            }
        }
    }

    #[test]
    fn triangle_ramsey_control_and_limits() {
        let pattern = Graph::from_bits(5, 7);
        let counter = Counter::new(&pattern);
        let mut engine = Engine::new(10, 10, false);
        let anchor = Graph::from_bits(5, 0);
        let out = block(
            &mut engine,
            &counter,
            &pattern,
            anchor,
            &unlimited(),
            |_| 1023,
        );
        assert_eq!(out.status, "refuted"); // a red/blue C5
        let pattern = Graph::from_bits(6, 7);
        let counter = Counter::new(&pattern);
        let mut engine = Engine::new(15, 10, false);
        let anchor = Graph::from_bits(6, 0);
        let out = block(
            &mut engine,
            &counter,
            &pattern,
            anchor,
            &unlimited(),
            |_| 32767,
        );
        assert_eq!(out.status, "unrestricted-unsat-unverified"); // R(3,3)=6
        let mut engine = Engine::new(15, 10, false);
        let config = Config {
            rounds: 0,
            ..unlimited()
        };
        let out = block(&mut engine, &counter, &pattern, anchor, &config, |_| 32767);
        assert_eq!(out.status, "round-limit");
        let config = Config {
            seconds: f64::MIN_POSITIVE,
            ..unlimited()
        };
        let out = block(&mut engine, &counter, &pattern, anchor, &config, |_| 32767);
        assert_eq!(out.status, "time-limit");
        assert_ne!(out.best_copies, Some(0));
    }

    #[test]
    fn block_selection_has_requested_size_and_both_colors() {
        let host = Graph::from_bits(6, 7);
        let mut rng = StdRng::seed_from_u64(47);
        for size in 2..=15 {
            for _ in 0..30 {
                let free = choose_block(host, &[7], size, BlockMode::Directed, &mut rng);
                assert_eq!(free.count_ones() as usize, size);
                assert_ne!(free & host.bits(), 0);
                assert_ne!(free & host.complement().bits(), 0);
            }
        }
    }
}
