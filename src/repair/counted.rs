//! Exact local objective and all one-edge deltas in one embedding traversal.
//! Enumerate copies with <=1 wrong-coloured edge in each colour. Monochromatic
//! copies contribute to `makes` on every edge; one-wrong copies contribute to
//! `breaks` on their sole wrong edge. No host symmetry pruning is allowed here.
//! Optional pair scoring visits <=2 wrong edges and accumulates the exact
//! interaction correction to the sum of the two single-edge deltas.

use super::*;
use crate::base::{index, rev_index};

#[derive(Debug, Default)]
struct Score {
    copies: u64,
    colors: [u64; 2],
    makes: Vec<u64>,
    breaks: Vec<u64>,
    pairs: Vec<i64>,
    witnesses: std::collections::BTreeSet<BitNum>,
}

impl Score {
    fn delta(&self, edge: usize) -> i64 {
        self.breaks[edge] as i64 - self.makes[edge] as i64
    }

    fn pair_delta(&self, a: usize, b: usize) -> i64 {
        self.delta(a) + self.delta(b) + self.pairs[index(a, b)]
    }
}

struct Counter {
    size: usize,
    order: Vec<usize>,
    adjacent: [u32; MAX_SIZE],
    pred: [usize; MAX_SIZE],
    // Each edge-set copy occurs this many times after pattern-twin pruning.
    multiplicity: u64,
}

struct Search<'a, const WRONG: u32> {
    counter: &'a Counter,
    host_adj: [u32; MAX_SIZE],
    domains: [[u32; MAX_SIZE]; 3],
    required: [u32; MAX_SIZE],
    images: [usize; MAX_SIZE],
    score: &'a mut Score,
    timer: Instant,
    deadline: Option<Duration>,
    nodes: u64,
    witness_limit: usize,
}

impl Counter {
    fn new(pattern: &Graph) -> Self {
        let mut adjacent = [0u32; MAX_SIZE];
        for edge in bits(pattern.bits()) {
            let (a, b) = rev_index(edge);
            adjacent[a] |= 1 << b;
            adjacent[b] |= 1 << a;
        }
        let order: Vec<_> = build_sorted_row(pattern)
            .into_iter()
            .filter(|&(degree, _)| degree != 0)
            .map(|(_, v)| v)
            .collect();
        let mut pred = [ABSENT; MAX_SIZE];
        let mut chain_length = [1u64; MAX_SIZE];
        let mut twin_factor = 1;
        for (i, &v) in order.iter().enumerate() {
            for &u in order[..i].iter().rev() {
                let other = !((1u32 << u) | (1 << v));
                if adjacent[v] & other == adjacent[u] & other {
                    pred[v] = u;
                    chain_length[v] = chain_length[u] + 1;
                    twin_factor *= chain_length[v];
                    break;
                }
            }
        }
        let active_automorphisms = crate::tools::count_symmetries(pattern)
            / crate::tools::factorial(pattern.size - order.len());
        assert_eq!(active_automorphisms % twin_factor as u128, 0);
        let multiplicity = active_automorphisms as u64 / twin_factor;
        Self {
            size: pattern.size,
            order,
            adjacent,
            pred,
            multiplicity,
        }
    }

    // A timed-out enumeration yields NO score, not a partial lower score.
    fn score(&self, host: Graph, timer: Instant, deadline: Option<Duration>) -> Option<Score> {
        self.evaluate::<1>(host, timer, deadline)
    }

    fn evaluate<const WRONG: u32>(
        &self,
        host: Graph,
        timer: Instant,
        deadline: Option<Duration>,
    ) -> Option<Score> {
        self.evaluate_collect::<WRONG>(host, timer, deadline, 0)
    }

    fn evaluate_collect<const WRONG: u32>(
        &self,
        host: Graph,
        timer: Instant,
        deadline: Option<Duration>,
        witnesses_per_color: usize,
    ) -> Option<Score> {
        assert!(WRONG <= 2);
        let variables = Graph::triangle(self.size);
        let mut score = Score {
            copies: 0,
            colors: [0; 2],
            makes: vec![0; variables],
            breaks: vec![0; variables],
            pairs: if WRONG == 2 {
                vec![0; Graph::triangle(variables)]
            } else {
                Vec::new()
            },
            witnesses: std::collections::BTreeSet::new(),
        };
        for (color, host) in [host, host.complement()].into_iter().enumerate() {
            let before = score.copies;
            let witness_limit = score.witnesses.len() + witnesses_per_color;
            let mut host_adj = [0u32; MAX_SIZE];
            for edge in bits(host.bits()) {
                let (a, b) = rev_index(edge);
                host_adj[a] |= 1 << b;
                host_adj[b] |= 1 << a;
            }
            let mut domains = [[0u32; MAX_SIZE]; 3];
            for &v in &self.order {
                for (w, row) in host_adj[..self.size].iter().enumerate() {
                    for remaining in 0..=WRONG {
                        if row.count_ones() + remaining as u32 >= self.adjacent[v].count_ones() {
                            domains[remaining as usize][v] |= 1 << w;
                        }
                    }
                }
            }
            let mut search = Search::<WRONG> {
                counter: self,
                host_adj,
                domains,
                required: [0; MAX_SIZE],
                images: [ABSENT; MAX_SIZE],
                score: &mut score,
                timer,
                deadline,
                nodes: 0,
                witness_limit,
            };
            if !search.go(0, 0, 0, 0) {
                return None;
            }
            score.colors[color] = score.copies - before;
        }
        assert_eq!(score.copies % self.multiplicity, 0);
        score.copies /= self.multiplicity;
        for value in &mut score.colors {
            assert_eq!(*value % self.multiplicity, 0);
            *value /= self.multiplicity;
        }
        for value in score.makes.iter_mut().chain(&mut score.breaks) {
            debug_assert_eq!(*value % self.multiplicity, 0);
            *value /= self.multiplicity;
        }
        for value in &mut score.pairs {
            debug_assert_eq!(*value % self.multiplicity as i64, 0);
            *value /= self.multiplicity as i64;
        }
        Some(score)
    }
}

impl<const WRONG: u32> Search<'_, WRONG> {
    // false means interrupted; otherwise this subtree has been fully counted.
    fn go(&mut self, depth: usize, used: u32, edge_set: BitNum, wrong: BitNum) -> bool {
        self.nodes += 1;
        if self.nodes & 4095 == 1 && self.deadline.is_some_and(|d| self.timer.elapsed() >= d) {
            return false;
        }
        if depth == self.counter.order.len() {
            if wrong == 0 {
                self.score.copies += 1;
                if WRONG != 0 {
                    for edge in bits(edge_set) {
                        self.score.makes[edge] += 1;
                    }
                }
                if self.score.witnesses.len() < self.witness_limit {
                    self.score.witnesses.insert(edge_set);
                }
            } else if wrong.count_ones() == 1 {
                self.score.breaks[wrong.trailing_zeros() as usize] += 1;
            }
            if WRONG == 2 {
                // A monochromatic copy was subtracted twice by two makes;
                // a one-wrong copy was added by its sole wrong flip, but a
                // simultaneous right flip destroys it; a two-wrong copy is
                // created only by the pair. Hence +1, -1, +1 respectively.
                match wrong.count_ones() {
                    0 => {
                        for b in bits(edge_set) {
                            for a in bits(edge_set & ((1 << b) - 1)) {
                                self.score.pairs[index(a, b)] += 1;
                            }
                        }
                    }
                    1 => {
                        let a = wrong.trailing_zeros() as usize;
                        for b in bits(edge_set & !wrong) {
                            self.score.pairs[index(a, b)] -= 1;
                        }
                    }
                    2 => {
                        let a = wrong.trailing_zeros() as usize;
                        let b = (wrong & (wrong - 1)).trailing_zeros() as usize;
                        self.score.pairs[index(a, b)] += 1;
                    }
                    _ => unreachable!(),
                }
            }
            return true;
        }
        let v = self.counter.order[depth];
        // Already missing edges have both endpoints placed, so later vertices
        // may lose at most the remaining wrong-edge budget in their degree.
        let remaining = WRONG - wrong.count_ones();
        let mut choices = self.domains[remaining as usize][v] & !used;
        if remaining == 0 {
            let mut r = self.required[v];
            while r != 0 {
                let u = r.trailing_zeros() as usize;
                r &= r - 1;
                choices &= self.host_adj[u];
            }
        }
        let pred = self.counter.pred[v];
        if pred != ABSENT {
            choices &= !((1 << (self.images[pred] + 1)) - 1);
        }
        while choices != 0 {
            let w = choices.trailing_zeros() as usize;
            let wb = 1 << w;
            choices &= choices - 1;
            let required = self.required[v];
            let missing = required & !self.host_adj[w];
            if missing.count_ones() > remaining {
                continue;
            }
            let mut next_wrong = wrong;
            let mut missing_edges = missing;
            while missing_edges != 0 {
                let u = missing_edges.trailing_zeros() as usize;
                missing_edges &= missing_edges - 1;
                next_wrong |= 1 << index(w, u);
            }
            let mut next_edges = edge_set;
            let mut r = required;
            while r != 0 {
                let u = r.trailing_zeros() as usize;
                r &= r - 1;
                next_edges |= 1 << index(u, w);
            }
            self.images[v] = w;
            let mut neighbours = self.counter.adjacent[v];
            while neighbours != 0 {
                let u = neighbours.trailing_zeros() as usize;
                neighbours &= neighbours - 1;
                self.required[u] |= wb;
            }
            if !self.go(depth + 1, used | wb, next_edges, next_wrong) {
                return false;
            }
            let mut neighbours = self.counter.adjacent[v];
            while neighbours != 0 {
                let u = neighbours.trailing_zeros() as usize;
                neighbours &= neighbours - 1;
                self.required[u] &= !wb;
            }
        }
        true
    }
}

pub(super) fn count_hosts(args: CountArgs) {
    assert!((1..=MAX_SIZE).contains(&args.size));
    assert!(args.jobs > 0 && args.seconds.is_finite() && args.seconds >= 0.0);
    assert_eq!(args.candidate >> Graph::triangle(args.size), 0);
    let pattern = Graph::from_bits(args.size, args.candidate);
    let counter = Counter::new(&pattern);
    let hosts: Vec<Graph> = read_graphs(args.size, &args.path).collect();
    let deadline = (args.seconds > 0.0).then(|| Duration::from_secs_f64(args.seconds));
    println!("n,candidate,host,status,edges,red_copies,blue_copies,total_copies,seconds");
    std::io::stdout().flush().unwrap();
    rayon::ThreadPoolBuilder::new()
        .num_threads(args.jobs)
        .build()
        .unwrap()
        .install(|| {
            hosts.par_iter().for_each(|&host| {
                assert_eq!(host.bits() >> Graph::triangle(args.size), 0);
                let timer = Instant::now();
                let score = counter.evaluate::<0>(host, timer, deadline);
                let (status, red, blue, total) = match score {
                    None => ("time-limit", String::new(), String::new(), String::new()),
                    Some(score) => {
                        let status = if score.copies == 0 {
                            let row = build_sorted_row(&pattern);
                            assert!(find_subgraph_ss(&pattern, &row, &host).is_none());
                            assert!(find_subgraph_ss(&pattern, &row, &host.complement()).is_none());
                            "refuted"
                        } else {
                            "counted"
                        };
                        (
                            status,
                            score.colors[0].to_string(),
                            score.colors[1].to_string(),
                            score.copies.to_string(),
                        )
                    }
                };
                let mut out = std::io::stdout().lock();
                writeln!(
                    out,
                    "{},{},{},{},{},{},{},{},{:.6}",
                    args.size,
                    args.candidate,
                    host.bits(),
                    status,
                    host.edge_count(),
                    red,
                    blue,
                    total,
                    timer.elapsed().as_secs_f64()
                )
                .unwrap();
                out.flush().unwrap();
            });
        });
}

// Keep a bounded sample of distinct near-best classes. Score has priority;
// ties use a stable per-walk pseudo-random rank, without consuming move RNG.
// This needs no unbounded set of every host/class previously encountered.
struct Archive {
    capacity: usize,
    slack: u64,
    salt: u64,
    entries: Vec<(crate::canon::Key, u64, Checkpoint)>,
}

fn host_key(host: &Graph) -> crate::canon::Key {
    crate::canon::key(host).min(crate::canon::key(&host.complement()))
}

impl Archive {
    fn new(config: &Config, start: Graph) -> Self {
        Self {
            capacity: config.archive,
            slack: config.archive_slack,
            salt: stream_seed(0x61726368697665, start.bits(), 0),
            entries: Vec::new(),
        }
    }

    fn offer(&mut self, host: Graph, copies: u64, best: u64, flips: u64) {
        if self.capacity == 0 {
            return;
        }
        let ceiling = best.saturating_add(self.slack);
        self.entries.retain(|(_, _, c)| c.copies <= ceiling);
        if copies > ceiling {
            return;
        }
        if self.entries.len() == self.capacity && copies > self.entries.last().unwrap().2.copies {
            return;
        }
        let key = host_key(&host);
        if self.entries.iter().any(|(k, _, _)| *k == key) {
            return;
        }
        let priority = stream_seed(self.salt, key.graph(host.size).bits(), 0);
        self.entries.push((
            key,
            priority,
            Checkpoint {
                host,
                copies,
                flips,
            },
        ));
        self.entries.sort_by_key(|(k, p, c)| (c.copies, *p, *k));
        self.entries.truncate(self.capacity);
    }

    fn finish(self, best_host: Graph) -> Vec<Checkpoint> {
        if self.entries.is_empty() {
            return Vec::new();
        }
        let best_key = host_key(&best_host);
        self.entries
            .into_iter()
            .filter(|(k, _, _)| *k != best_key)
            .map(|(_, _, c)| c)
            .collect()
    }
}

fn single_choices(score: &Score, tabu: &[u64], flips: u64, best: u64, moves: Moves) -> Vec<usize> {
    let focused = |v: usize| moves == Moves::All || score.makes[v] != 0;
    let mut choices: Vec<_> = (0..score.makes.len())
        .filter(|&v| {
            focused(v) && (tabu[v] <= flips || score.copies as i64 + score.delta(v) < best as i64)
        })
        .collect();
    if choices.is_empty() {
        choices = (0..score.makes.len()).filter(|&v| focused(v)).collect();
    }
    choices
}

fn pair_choices(
    score: &Score,
    host: Graph,
    tabu: &[u64],
    flips: u64,
    best: u64,
    single_delta: i64,
) -> Vec<(usize, usize)> {
    let mut choices = Vec::new();
    let mut pair_best = single_delta;
    let mut current_key = None;
    for b in 1..score.makes.len() {
        for a in 0..b {
            let delta = score.pair_delta(a, b);
            if delta >= single_delta || delta > pair_best {
                continue;
            }
            if (tabu[a] > flips || tabu[b] > flips) && score.copies as i64 + delta >= best as i64 {
                continue;
            }
            // In particular, swapping two almost-twin vertex labels can be a
            // two-edge move. Do not spend the pair budget on an isomorphic or
            // complementary version of the CURRENT host. Only delta=0 can do
            // this, so most moves need no canonicalization at all.
            if delta == 0 {
                let key = *current_key.get_or_insert_with(|| host_key(&host));
                let next = Graph::from_bits(host.size, host.bits() ^ (1 << a) ^ (1 << b));
                if host_key(&next) == key {
                    continue;
                }
            }
            if delta < pair_best {
                pair_best = delta;
                choices.clear();
            }
            if delta == pair_best {
                choices.push((a, b));
            }
        }
    }
    choices
}

pub(super) fn walk(pattern: &Graph, start: Graph, config: &Config, rng: &mut StdRng) -> Outcome {
    let timer = Instant::now();
    let deadline = (config.seconds > 0.0).then(|| Duration::from_secs_f64(config.seconds));
    let counter = Counter::new(pattern);
    let mut host = start;
    let mut best_host = start;
    let mut best = None;
    let mut diagnostics = Diagnostics::default();
    let mut archive = Archive::new(config, start);
    let weighted = config.penalty_every != 0;
    let mut penalties = penalty::Penalties::new(config.penalty_cap);
    let mut stalled_decisions = 0u64;
    let mut flips = 0;
    let mut checks = 0;
    let mut oracle_time = Duration::ZERO;
    let variables = Graph::triangle(start.size);
    let mut tabu_until = vec![0u64; variables];
    let mut next_progress = Duration::from_secs(10);
    let status = loop {
        let before = Instant::now();
        let score = if weighted {
            counter.evaluate_collect::<1>(host, timer, deadline, config.penalty_batch)
        } else {
            counter.score(host, timer, deadline)
        };
        oracle_time += before.elapsed();
        checks += 2;
        let Some(score) = score else {
            break "time-limit";
        };
        if best.is_none_or(|b| score.copies < b) {
            best = Some(score.copies);
            best_host = host;
            diagnostics.best_step = flips;
        }
        archive.offer(host, score.copies, best.unwrap(), flips);
        if score.copies == 0 {
            // Keep the existing, separately implemented absence oracle as the
            // acceptance gate even though the full count is itself exact.
            let row = build_sorted_row(pattern);
            assert!(find_subgraph_ss(pattern, &row, &host).is_none());
            assert!(find_subgraph_ss(pattern, &row, &host.complement()).is_none());
            checks += 2;
            break "refuted";
        }
        if flips >= config.steps {
            break "step-limit";
        }
        if deadline.is_some_and(|d| timer.elapsed() >= d) {
            break "time-limit";
        }
        if timer.elapsed() >= next_progress {
            eprintln!(
                "counted F={}: {:.1}s, flips={}, copies={}, best={}",
                pattern.bits(),
                timer.elapsed().as_secs_f64(),
                flips,
                score.copies,
                best.unwrap()
            );
            next_progress = timer.elapsed() + Duration::from_secs(10);
        }
        let mut choices = single_choices(&score, &tabu_until, flips, best.unwrap(), config.moves);
        let mut extra = if weighted {
            penalties.deltas(host.bits(), variables)
        } else {
            vec![0; variables]
        };
        let mut min_delta = choices
            .iter()
            .map(|&v| score.delta(v) + extra[v])
            .min()
            .unwrap();
        if min_delta >= 0 {
            stalled_decisions += 1;
        }
        if min_delta >= 0 && weighted && stalled_decisions % config.penalty_every == 0 {
            // Only fully enumerated scores reach here. Remember actual distinct
            // labelled copies, not embedding multiplicities or abstract classes.
            penalties.boost(
                score.witnesses.iter().copied(),
                config.penalty_step,
                config.penalty_decay,
            );
            extra = penalties.deltas(host.bits(), variables);
            min_delta = choices
                .iter()
                .map(|&v| score.delta(v) + extra[v])
                .min()
                .unwrap();
        }
        if min_delta >= 0
            && config.pair_every != 0
            && stalled_decisions % config.pair_every == 0
            && config.steps - flips >= 2
        {
            let before = Instant::now();
            let paired = counter.evaluate::<2>(host, timer, deadline);
            oracle_time += before.elapsed();
            checks += 2;
            diagnostics.pair_evaluations += 1;
            let Some(paired) = paired else {
                break "time-limit";
            };
            assert_eq!(paired.copies, score.copies);
            let pairs = pair_choices(&paired, host, &tabu_until, flips, best.unwrap(), min_delta);
            if let Some(&(a, b)) = pairs.choose(rng) {
                host.edges.0.0 ^= (1 << a) | (1 << b);
                flips += 2;
                tabu_until[a] = flips.saturating_add(config.tabu as u64);
                tabu_until[b] = flips.saturating_add(config.tabu as u64);
                diagnostics.pair_moves += 1;
                diagnostics.outside_moves +=
                    u64::from(score.makes[a] == 0) + u64::from(score.makes[b] == 0);
                continue;
            }
        }
        let random = min_delta >= 0 && rng.gen_bool(config.noise);
        if !random {
            choices.retain(|&v| score.delta(v) + extra[v] == min_delta);
        }
        let edge = *choices.choose(rng).unwrap();
        host.edges.0.0 ^= 1 << edge;
        flips += 1;
        tabu_until[edge] = flips.saturating_add(config.tabu as u64);
        diagnostics.outside_moves += u64::from(score.makes[edge] == 0);
    };
    diagnostics.archive = archive.finish(best_host);
    diagnostics.penalty_updates = penalties.updates;
    diagnostics.penalty_constraints = penalties.len();
    Outcome {
        host: best_host,
        status,
        flips,
        checks,
        constraints: 0,
        unsatisfied: 0,
        elapsed: timer.elapsed(),
        oracle_time,
        best_copies: best,
        diagnostics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brute(pattern: Graph, host: Graph) -> u64 {
        let mut copies = HashSet::new();
        for p in crate::perm::all_perms(pattern.size) {
            let copy = pattern.renumber(&p);
            if copy.is_subgraph_of(&host) || copy.is_subgraph_of(&host.complement()) {
                copies.insert(copy.bits());
            }
        }
        copies.len() as u64
    }

    #[test]
    fn counts_and_every_flip_delta_match_permutations() {
        let mut rng = StdRng::seed_from_u64(732);
        for _ in 0..100 {
            let pattern = random_graph(&mut rng, 5);
            if pattern.edge_count() < 2 {
                continue;
            }
            let counter = Counter::new(&pattern);
            let host = random_graph(&mut rng, 5);
            let score = counter.score(host, Instant::now(), None).unwrap();
            let only = counter.evaluate::<0>(host, Instant::now(), None).unwrap();
            let witnessed = counter
                .evaluate_collect::<1>(host, Instant::now(), None, 3)
                .unwrap();
            assert_eq!(score.copies, only.copies);
            assert_eq!(score.colors, only.colors);
            assert_eq!(score.copies, score.colors.iter().sum());
            assert_eq!(score.copies, witnessed.copies);
            assert!(witnessed.witnesses.len() <= 6);
            let possible: HashSet<_> = crate::perm::all_perms(5)
                .map(|p| pattern.renumber(&p).bits())
                .collect();
            for &mask in &witnessed.witnesses {
                assert!(possible.contains(&mask));
                assert!(mask & host.bits() == 0 || mask & !host.bits() == 0);
            }
            assert_eq!(
                witnessed.witnesses.len(),
                score.colors.iter().map(|&n| n.min(3) as usize).sum()
            );
            let paired = counter.evaluate::<2>(host, Instant::now(), None).unwrap();
            assert_eq!(score.copies, brute(pattern, host));
            assert_eq!(score.copies, paired.copies);
            assert_eq!(score.makes, paired.makes);
            assert_eq!(score.breaks, paired.breaks);
            for v in 0..10 {
                let next = Graph::from_bits(5, host.bits() ^ (1 << v));
                assert_eq!(
                    score.copies as i64 + score.breaks[v] as i64 - score.makes[v] as i64,
                    brute(pattern, next) as i64
                );
                for u in 0..v {
                    let next = Graph::from_bits(5, host.bits() ^ (1 << v) ^ (1 << u));
                    assert_eq!(
                        score.copies as i64 + paired.pair_delta(u, v),
                        brute(pattern, next) as i64,
                        "pattern={}, host={}, pair=({u},{v})",
                        pattern.bits(),
                        host.bits()
                    );
                }
            }
        }
    }

    #[test]
    fn interruption_is_not_a_zero_score() {
        let pattern = Graph::from_bits(5, 7);
        let counter = Counter::new(&pattern);
        assert!(
            counter
                .score(pattern, Instant::now(), Some(Duration::ZERO))
                .is_none()
        );
        assert!(
            counter
                .evaluate::<2>(pattern, Instant::now(), Some(Duration::ZERO))
                .is_none()
        );
        assert!(
            counter
                .evaluate::<0>(pattern, Instant::now(), Some(Duration::ZERO))
                .is_none()
        );
        assert!(
            counter
                .evaluate_collect::<1>(pattern, Instant::now(), Some(Duration::ZERO), 64)
                .is_none()
        );
    }

    #[test]
    fn weighted_walks_keep_exact_scores_and_repeatable_traces() {
        // K6 always has a monochromatic triangle, so this exercises penalties,
        // evictions and smoothing throughout, rather than stopping on a hit.
        let pattern = Graph::from_bits(6, 7);
        let config = Config {
            penalty_every: 1,
            penalty_cap: 7,
            penalty_decay: 3,
            penalty_batch: 4,
            archive: 6,
            steps: 100,
            seconds: 0.0,
            ..Config::default()
        };
        let start = Graph::from_bits(6, 0);
        let a = walk(&pattern, start, &config, &mut StdRng::seed_from_u64(205));
        let b = walk(&pattern, start, &config, &mut StdRng::seed_from_u64(205));
        assert_eq!(
            (a.host, a.flips, a.best_copies),
            (b.host, b.flips, b.best_copies)
        );
        assert_eq!(a.diagnostics.penalty_updates, b.diagnostics.penalty_updates);
        assert!(a.diagnostics.penalty_updates > 3);
        assert!(a.diagnostics.penalty_constraints <= 7);
        assert_eq!(a.status, "step-limit");
        assert_eq!(a.best_copies, Some(brute(pattern, a.host)));
        for checkpoint in a.diagnostics.archive {
            assert_eq!(checkpoint.copies, brute(pattern, checkpoint.host));
        }
    }

    #[test]
    fn outside_moves_can_be_cheaper_and_tabu_still_applies() {
        let score = Score {
            copies: 2,
            makes: vec![1, 1, 0],
            breaks: vec![9, 9, 6],
            pairs: vec![],
            ..Score::default()
        };
        assert_eq!(
            single_choices(&score, &[0; 3], 0, 2, Moves::Focused),
            vec![0, 1]
        );
        let all = single_choices(&score, &[0; 3], 0, 2, Moves::All);
        assert_eq!(all.into_iter().min_by_key(|&v| score.delta(v)), Some(2));
        assert_eq!(
            single_choices(&score, &[0, 0, 7], 0, 2, Moves::All),
            vec![0, 1]
        );
    }

    #[test]
    fn pairs_do_not_just_relabel_the_current_host() {
        let pattern = Graph::from_bits(5, 7);
        let counter = Counter::new(&pattern);
        for h in [1, 3, 7, 45, 613, 771] {
            let host = Graph::from_bits(5, h);
            let score = counter.evaluate::<2>(host, Instant::now(), None).unwrap();
            let choices = pair_choices(&score, host, &[0; 10], 0, score.copies, 100);
            for (a, b) in choices {
                let next = Graph::from_bits(5, h ^ (1 << a) ^ (1 << b));
                assert_ne!(host_key(&host), host_key(&next));
            }
        }
    }

    #[test]
    fn exhaustive_four_vertex_pair_deltas() {
        for f in 0..64u64 {
            if f.count_ones() < 2 {
                continue;
            }
            let pattern = Graph::from_bits(4, f as BitNum);
            let counter = Counter::new(&pattern);
            for h in 0..64 {
                let host = Graph::from_bits(4, h);
                let score = counter.evaluate::<2>(host, Instant::now(), None).unwrap();
                for b in 1..6 {
                    for a in 0..b {
                        let next = Graph::from_bits(4, h ^ (1 << a) ^ (1 << b));
                        assert_eq!(
                            score.copies as i64 + score.pair_delta(a, b),
                            brute(pattern, next) as i64
                        );
                    }
                }
            }
        }
    }

    #[cfg(all(feature = "nauty", not(feature = "u64")))]
    #[test]
    fn frontier_minima_have_cheaper_genuine_pair_escapes() {
        for (f, h, q, focused, all, paired) in [
            (208095050752, 149391117928587412994558940, 2, 10, 8, 3),
            (35391346720704, 1735057707038897413644043761, 5, 19, 14, 9),
            (35391346720704, 240446773475227354961364772, 5, 9, 9, 6),
        ] {
            let pattern = Graph::from_bits(14, f);
            let host = Graph::from_bits(14, h);
            let score = Counter::new(&pattern)
                .evaluate::<2>(host, Instant::now(), None)
                .unwrap();
            assert_eq!(score.copies, q);
            for (scope, expected) in [(Moves::Focused, focused), (Moves::All, all)] {
                let choices = single_choices(&score, &[0; 91], 0, q, scope);
                assert_eq!(
                    q as i64 + choices.iter().map(|&e| score.delta(e)).min().unwrap(),
                    expected
                );
            }
            let pairs = pair_choices(&score, host, &[0; 91], 0, q, all - q as i64);
            assert!(!pairs.is_empty());
            assert!(
                pairs
                    .iter()
                    .all(|&(a, b)| q as i64 + score.pair_delta(a, b) == paired)
            );
        }
    }

    #[test]
    fn archives_are_exact_bounded_diverse_and_do_not_change_walks() {
        let pattern = Graph::from_bits(5, 7);
        let counter = Counter::new(&pattern);
        let config = Config {
            archive: 4,
            archive_slack: 3,
            seconds: 0.0,
            steps: 80,
            pair_every: 1,
            ..Config::default()
        };
        let start = Graph::from_bits(5, 0);
        let mut archive = Archive::new(&config, start);
        for h in 0..1024 {
            let host = Graph::from_bits(5, h);
            let q = counter.score(host, Instant::now(), None).unwrap().copies;
            archive.offer(host, q, 0, h as u64);
            assert!(archive.entries.len() <= 4);
        }
        let entries = archive.finish(start);
        assert_eq!(entries.len(), 4);
        assert_eq!(
            entries
                .iter()
                .map(|c| host_key(&c.host))
                .collect::<HashSet<_>>()
                .len(),
            4
        );
        for c in entries {
            assert_eq!(c.copies, brute(pattern, c.host));
            assert!(c.copies <= 3);
        }
        let a = walk(&pattern, start, &config, &mut StdRng::seed_from_u64(183));
        let b = walk(
            &pattern,
            start,
            &Config {
                archive: 0,
                ..config
            },
            &mut StdRng::seed_from_u64(183),
        );
        assert_eq!(
            (a.host, a.flips, a.status, a.best_copies),
            (b.host, b.flips, b.status, b.best_copies)
        );
        if a.status == "refuted" {
            assert_eq!(brute(pattern, a.host), 0);
        }
    }
}
