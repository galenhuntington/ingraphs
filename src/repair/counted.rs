//! Exact local objective and all one-edge deltas in one embedding traversal.
//! Enumerate copies with <=1 wrong-coloured edge in each colour. Monochromatic
//! copies contribute to `makes` on every edge; one-wrong copies contribute to
//! `breaks` on their sole wrong edge. No host symmetry pruning is allowed here.

use super::*;
use crate::base::{index, rev_index};

#[derive(Debug)]
struct Score {
    copies: u64,
    makes: Vec<u64>,
    breaks: Vec<u64>,
}

struct Counter {
    size: usize,
    order: Vec<usize>,
    adjacent: [u32; MAX_SIZE],
    pred: [usize; MAX_SIZE],
    // Each edge-set copy occurs this many times after pattern-twin pruning.
    multiplicity: u64,
}

struct Search<'a> {
    counter: &'a Counter,
    host_adj: [u32; MAX_SIZE],
    domains: [u32; MAX_SIZE],
    full_domains: [u32; MAX_SIZE],
    required: [u32; MAX_SIZE],
    images: [usize; MAX_SIZE],
    score: &'a mut Score,
    timer: Instant,
    deadline: Option<Duration>,
    nodes: u64,
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
        let variables = Graph::triangle(self.size);
        let mut score = Score {
            copies: 0,
            makes: vec![0; variables],
            breaks: vec![0; variables],
        };
        for host in [host, host.complement()] {
            let mut host_adj = [0u32; MAX_SIZE];
            for edge in bits(host.bits()) {
                let (a, b) = rev_index(edge);
                host_adj[a] |= 1 << b;
                host_adj[b] |= 1 << a;
            }
            let mut domains = [0u32; MAX_SIZE];
            let mut full_domains = [0u32; MAX_SIZE];
            for &v in &self.order {
                for (w, row) in host_adj[..self.size].iter().enumerate() {
                    // At most one incident pattern edge may be missing.
                    if row.count_ones() + 1 >= self.adjacent[v].count_ones() {
                        domains[v] |= 1 << w;
                    }
                    if row.count_ones() >= self.adjacent[v].count_ones() {
                        full_domains[v] |= 1 << w;
                    }
                }
            }
            let mut search = Search {
                counter: self,
                host_adj,
                domains,
                full_domains,
                required: [0; MAX_SIZE],
                images: [ABSENT; MAX_SIZE],
                score: &mut score,
                timer,
                deadline,
                nodes: 0,
            };
            if !search.go(0, 0, 0, 0) {
                return None;
            }
        }
        assert_eq!(score.copies % self.multiplicity, 0);
        score.copies /= self.multiplicity;
        for value in score.makes.iter_mut().chain(&mut score.breaks) {
            debug_assert_eq!(*value % self.multiplicity, 0);
            *value /= self.multiplicity;
        }
        Some(score)
    }
}

impl Search<'_> {
    // false means interrupted; otherwise this subtree has been fully counted.
    fn go(&mut self, depth: usize, used: u32, edge_set: BitNum, wrong: BitNum) -> bool {
        self.nodes += 1;
        if self.nodes & 4095 == 1 && self.deadline.is_some_and(|d| self.timer.elapsed() >= d) {
            return false;
        }
        if depth == self.counter.order.len() {
            if wrong == 0 {
                self.score.copies += 1;
                for edge in bits(edge_set) {
                    self.score.makes[edge] += 1;
                }
            } else {
                self.score.breaks[wrong.trailing_zeros() as usize] += 1;
            }
            return true;
        }
        let v = self.counter.order[depth];
        // Once the wrong edge has been placed, its endpoints are both used;
        // all later vertices need their entire pattern degree in this colour.
        let mut choices = if wrong == 0 {
            self.domains[v]
        } else {
            self.full_domains[v]
        } & !used;
        if wrong != 0 {
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
            if missing.count_ones() > u32::from(wrong == 0) {
                continue;
            }
            let next_wrong = if missing == 0 {
                wrong
            } else {
                1 << index(w, missing.trailing_zeros() as usize)
            };
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

pub(super) fn walk(pattern: &Graph, start: Graph, config: &Config, rng: &mut StdRng) -> Outcome {
    let timer = Instant::now();
    let deadline = (config.seconds > 0.0).then(|| Duration::from_secs_f64(config.seconds));
    let counter = Counter::new(pattern);
    let mut host = start;
    let mut best_host = start;
    let mut best = None;
    let mut flips = 0;
    let mut checks = 0;
    let mut oracle_time = Duration::ZERO;
    let variables = Graph::triangle(start.size);
    let mut tabu_until = vec![0u64; variables];
    let mut next_progress = Duration::from_secs(10);
    let status = loop {
        let before = Instant::now();
        let score = counter.score(host, timer, deadline);
        oracle_time += before.elapsed();
        checks += 2;
        let Some(score) = score else {
            break "time-limit";
        };
        if best.is_none_or(|b| score.copies < b) {
            best = Some(score.copies);
            best_host = host;
        }
        if score.copies == 0 {
            // Keep the existing, separately implemented absence oracle as the
            // acceptance gate even though the full count is itself exact.
            let row = build_sorted_row(pattern);
            assert!(find_subgraph_ss(pattern, &row, &host).is_none());
            assert!(find_subgraph_ss(pattern, &row, &host.complement()).is_none());
            checks += 2;
            break "refuted";
        }
        if flips == config.steps {
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
        let delta = |v: usize| score.breaks[v] as i64 - score.makes[v] as i64;
        let mut choices: Vec<_> = (0..variables)
            .filter(|&v| {
                score.makes[v] != 0
                    && (tabu_until[v] <= flips
                        || (score.copies as i64 + delta(v)) < best.unwrap() as i64)
            })
            .collect();
        if choices.is_empty() {
            choices = (0..variables).filter(|&v| score.makes[v] != 0).collect();
        }
        let min_delta = choices.iter().map(|&v| delta(v)).min().unwrap();
        let random = min_delta >= 0 && rng.gen_bool(config.noise);
        if !random {
            choices.retain(|&v| delta(v) == min_delta);
        }
        let edge = *choices.choose(rng).unwrap();
        host.edges.0.0 ^= 1 << edge;
        flips += 1;
        tabu_until[edge] = flips.saturating_add(config.tabu as u64);
    };
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
            assert_eq!(score.copies, brute(pattern, host));
            for v in 0..10 {
                let next = Graph::from_bits(5, host.bits() ^ (1 << v));
                assert_eq!(
                    score.copies as i64 + score.breaks[v] as i64 - score.makes[v] as i64,
                    brute(pattern, next) as i64
                );
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
    }
}
