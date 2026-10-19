//! SAT over an arbitrary partition of all unordered vertex pairs. Each cell
//! gets one colour variable; splitting cells relaxes the family. Full graph
//! witnesses are projected by SET UNION (not parity) to NAE constraints.
//! Family UNSAT is not universality, nor a separately checked UNSAT proof.

use super::*;

#[derive(Debug, clap::Args)]
pub struct Args {
    size: usize,
    candidate: BitNum,
    /// One nonzero decimal edge mask per line; disjoint and covering ALL pairs
    #[arg(long)]
    partition: PathBuf,
    /// Total search seconds including witness collection; zero disables
    #[arg(long, default_value_t = 30.0)]
    seconds: f64,
    #[arg(long, default_value_t = 100000)]
    rounds: usize,
    /// Full graph witnesses per colour per oracle call (projections are deduplicated)
    #[arg(long, default_value_t = 64)]
    batch: usize,
    /// Maximum distinct projected constraints, two clauses each
    #[arg(long, default_value_t = 500000)]
    max_constraints: usize,
    #[arg(long, default_value_t = 0)]
    conflicts: i32,
    #[arg(long)]
    factor: bool,
    #[arg(long, default_value_t = 0)]
    rng_seed: u64,
    /// Save the collected projected CNF, mapping, and full witnesses; never overwrite
    #[arg(long)]
    dump_cnf: Option<PathBuf>,
}

#[derive(Clone, Debug)]
struct Partition {
    cells: Vec<BitNum>,
    edge_variable: Vec<usize>,
}

impl Partition {
    fn new(size: usize, mut cells: Vec<BitNum>) -> Self {
        let edges = Graph::triangle(size);
        let full = ((1 as BitNum) << edges) - 1;
        cells.sort_unstable(); // Cell order is not part of the family identity.
        let mut covered = 0;
        let mut edge_variable = vec![0; edges];
        for (variable, &cell) in cells.iter().enumerate() {
            assert!(cell != 0 && cell & !full == 0, "invalid partition cell");
            assert_eq!(cell & covered, 0, "overlapping partition cells");
            for edge in bits(cell) {
                edge_variable[edge] = variable;
            }
            covered |= cell;
        }
        assert_eq!(covered, full, "partition must cover ALL vertex pairs");
        Self {
            cells,
            edge_variable,
        }
    }

    fn project(&self, copy: BitNum) -> BitNum {
        bits(copy).fold(0, |mask, e| mask | (1 << self.edge_variable[e]))
    }

    fn decode(&self, assignment: BitNum) -> BitNum {
        assert_eq!(assignment >> self.cells.len(), 0);
        bits(assignment).fold(0, |host, v| host | self.cells[v])
    }

    fn text(&self) -> String {
        self.cells
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("|")
    }
}

#[derive(Debug)]
struct Outcome {
    status: &'static str,
    last: Graph,
    last_copies: Option<u64>,
    rounds: usize,
    checks: usize,
    witnesses: usize,
    elapsed: Duration,
    sat_time: Duration,
    oracle_time: Duration,
}

fn search(
    pattern: &Graph,
    partition: &Partition,
    config: &Config,
    seed: u64,
    factor: bool,
) -> (Outcome, Engine, Vec<BitNum>) {
    let timer = Instant::now();
    let deadline = (config.seconds > 0.0).then(|| Duration::from_secs_f64(config.seconds));
    let expired = || deadline.is_some_and(|d| timer.elapsed() >= d);
    let counter = Counter::new(pattern);
    let mut engine = Engine::new(partition.cells.len(), seed, factor);
    let mut rng = StdRng::seed_from_u64(seed);
    let phases =
        (0..partition.cells.len()).fold(0, |m, i| if rng.gen_bool(0.5) { m | (1 << i) } else { m });
    engine.phases(phases);
    let free = ((1 as BitNum) << partition.cells.len()) - 1;
    let mut full_witnesses = Vec::new(); // One auditable graph witness per projected constraint.
    let mut out = Outcome {
        status: "round-limit",
        last: Graph::from_bits(pattern.size, partition.decode(phases)),
        last_copies: None,
        rounds: 0,
        checks: 0,
        witnesses: 0,
        elapsed: Duration::ZERO,
        sat_time: Duration::ZERO,
        oracle_time: Duration::ZERO,
    };
    out.status = loop {
        let before = Instant::now();
        let (pending, score) = counter.collect(out.last, timer, deadline, config.batch);
        out.oracle_time += before.elapsed();
        out.checks += 1;
        out.witnesses += pending.len();
        out.last_copies = score;
        if score == Some(0) {
            let before = Instant::now();
            validate_refuter(pattern, &out.last);
            out.oracle_time += before.elapsed();
            break "refuted";
        }
        // Check before adding this batch: several full copies in ONE batch
        // may project to the same set, but none may repeat a PREVIOUS clause.
        assert!(
            pending
                .iter()
                .all(|&m| !engine.seen.contains(&partition.project(m))),
            "oracle copy violates an already stored projected constraint"
        );
        for copy in pending {
            assert!(copy & out.last.bits() == 0 || copy & !out.last.bits() == 0);
            if engine.masks.len() < config.max_constraints
                && engine.add_copy(partition.project(copy))
            {
                full_witnesses.push(copy);
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
        let answer = engine.solve(phases, free, timer, deadline, config.conflicts);
        out.sat_time += before.elapsed();
        out.rounds += 1;
        match answer {
            Answer::Interrupted => break if expired() { "time-limit" } else { "sat-limit" },
            Answer::Unsat(core) => {
                assert_eq!(core, 0); // No fixed assignments or seed colours.
                break "family-unsat-unverified";
            }
            Answer::Model(model) => {
                out.last = Graph::from_bits(pattern.size, partition.decode(model));
            }
        }
    };
    out.elapsed = timer.elapsed();
    assert_eq!(engine.masks.len(), full_witnesses.len());
    (out, engine, full_witnesses)
}

fn dump(
    path: &Path,
    pattern: &Graph,
    partition: &Partition,
    engine: &Engine,
    witnesses: &[BitNum],
) {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("cannot create CNF (existing files are never overwritten)");
    writeln!(
        file,
        "c orbital n={} candidate={}; family only, no fixed colours",
        pattern.size,
        pattern.bits()
    )
    .unwrap();
    for (v, cell) in partition.cells.iter().enumerate() {
        writeln!(file, "c variable {} edge_mask {}", v + 1, cell).unwrap();
    }
    writeln!(
        file,
        "p cnf {} {}",
        engine.variables,
        2 * engine.masks.len()
    )
    .unwrap();
    for (&mask, &witness) in engine.masks.iter().zip(witnesses) {
        writeln!(file, "c witness {}", witness).unwrap();
        for sign in [1, -1] {
            for v in bits(mask) {
                write!(file, "{} ", sign * (v as i32 + 1)).unwrap();
            }
            writeln!(file, "0").unwrap();
        }
    }
}

pub fn run(args: Args) {
    assert!(
        (1..=MAX_SIZE).contains(&args.size),
        "unsupported host order"
    );
    assert_eq!(
        args.candidate >> Graph::triangle(args.size),
        0,
        "candidate exceeds host order"
    );
    assert!(args.seconds.is_finite() && args.seconds >= 0.0 && args.conflicts >= 0);
    assert!(args.batch > 0 && args.max_constraints > 0);
    let text = std::fs::read_to_string(&args.partition).expect("cannot read partition file");
    let cells = text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
        .map(|s| s.parse().expect("expected a decimal partition cell"))
        .collect();
    let partition = Partition::new(args.size, cells);
    let pattern = Graph::from_bits(args.size, args.candidate);
    let config = Config {
        seconds: args.seconds,
        rounds: args.rounds,
        batch: args.batch,
        max_constraints: args.max_constraints,
        conflicts: args.conflicts,
    };
    let seed = stream_seed(args.rng_seed, args.candidate, 0);
    eprintln!(
        "Orbital SAT n={} F={}: {} variables, {:.1}s; family UNSAT is NOT universality",
        args.size,
        args.candidate,
        partition.cells.len(),
        args.seconds
    );
    let (out, engine, witnesses) = search(&pattern, &partition, &config, seed, args.factor);
    println!(
        "n,candidate,refuter,status,variables,partition,walk_seed,last_host,last_copies,rounds,checks,witnesses,constraints,seconds,sat_seconds,oracle_seconds,conflicts,decisions,propagations,solver,factor"
    );
    println!(
        "{},{},{},{},{},{},{},{},{},{},{},{},{},{:.6},{:.6},{:.6},{},{},{},{},{}",
        args.size,
        args.candidate,
        if out.status == "refuted" {
            out.last.bits().to_string()
        } else {
            String::new()
        },
        out.status,
        partition.cells.len(),
        partition.text(),
        seed,
        out.last.bits(),
        out.last_copies.map(|q| q.to_string()).unwrap_or_default(),
        out.rounds,
        out.checks,
        out.witnesses,
        engine.masks.len(),
        out.elapsed.as_secs_f64(),
        out.sat_time.as_secs_f64(),
        out.oracle_time.as_secs_f64(),
        engine.solver.conflicts(),
        engine.solver.decisions(),
        engine.solver.propagations(),
        engine.solver.signature(),
        args.factor
    );
    std::io::stdout().flush().unwrap();
    eprintln!(
        "Orbital F={}: {}, {:.3}s (SAT {:.3}s / oracle {:.3}s), {} rounds, {} projected constraints",
        args.candidate,
        out.status,
        out.elapsed.as_secs_f64(),
        out.sat_time.as_secs_f64(),
        out.oracle_time.as_secs_f64(),
        out.rounds,
        engine.masks.len()
    );
    if let Some(path) = args.dump_cnf {
        dump(&path, &pattern, &partition, &engine, &witnesses);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_is_union_and_preserves_monochromaticity() {
        let p = Partition::new(4, vec![56, 1, 6]);
        assert_eq!(p.project(6), 2); // Two edges, ONE variable; not XOR.
        for assignment in 0..8 {
            let host = p.decode(assignment);
            for copy in 0..64 {
                let projected = p.project(copy);
                assert_eq!(copy & host == 0, projected & assignment == 0);
                assert_eq!(copy & !host == 0, projected & !assignment == 0);
            }
        }
    }

    #[test]
    fn all_four_vertex_patterns_match_exhaustive_family_search() {
        let config = Config {
            seconds: 0.0,
            rounds: 10000,
            batch: 3,
            max_constraints: 10000,
            conflicts: 0,
        };
        for cells in [vec![63], vec![1, 6, 56], vec![1, 2, 4, 8, 16, 32]] {
            let p = Partition::new(4, cells);
            for f in 0..64 {
                let pattern = Graph::from_bits(4, f);
                let possible = (0..1 << p.cells.len()).any(|m| {
                    super::super::tests::naive_refutes(&pattern, Graph::from_bits(4, p.decode(m)))
                });
                let (out, engine, witnesses) = search(&pattern, &p, &config, 17, false);
                assert_eq!(out.status == "refuted", possible, "F={f}, partition={p:?}");
                if !possible {
                    assert_eq!(out.status, "family-unsat-unverified");
                }
                for (&mask, &witness) in engine.masks.iter().zip(&witnesses) {
                    assert_eq!(mask, p.project(witness));
                    assert!(
                        crate::perm::all_perms(4)
                            .any(|perm| pattern.renumber(&perm).bits() == witness)
                    );
                }
            }
        }
    }

    #[test]
    fn limits_are_not_unsat() {
        let p = Partition::new(4, vec![1, 2, 4, 8, 16, 32]);
        let pattern = Graph::from_bits(4, 3);
        let config = Config {
            seconds: 0.0,
            rounds: 0,
            batch: 3,
            max_constraints: 100,
            conflicts: 0,
        };
        assert_eq!(
            search(&pattern, &p, &config, 0, false).0.status,
            "round-limit"
        );
        let config = Config {
            rounds: 10000,
            max_constraints: 1,
            ..config
        };
        assert_eq!(
            search(&pattern, &p, &config, 0, false).0.status,
            "constraint-limit"
        );
        let config = Config {
            max_constraints: 100,
            seconds: f64::MIN_POSITIVE,
            ..config
        };
        assert_eq!(
            search(&pattern, &p, &config, 0, false).0.status,
            "time-limit"
        );
    }

    #[test]
    #[should_panic(expected = "overlapping")]
    fn reject_overlap() {
        Partition::new(3, vec![3, 6]);
    }

    #[test]
    #[should_panic(expected = "ALL")]
    fn reject_missing_pairs() {
        Partition::new(3, vec![1, 2]);
    }
}
