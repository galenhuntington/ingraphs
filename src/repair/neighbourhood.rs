//! Bounded edit search, branching only on an actual monochromatic copy.
//! If H' is a refuter, every such copy in H must contain an edge of H xor H'.
//! Therefore this branching covers the whole Hamming ball, not just some
//! chosen copies' neighbourhood. Never flip an edge twice along a branch.

use super::*;

struct Search<'a> {
    pattern: &'a Graph,
    row: Vec<(usize, usize)>,
    config: &'a Config,
    rng: &'a mut StdRng,
    timer: Instant,
    seen: HashSet<BitNum>,
    checks: u64,
    oracle_time: Duration,
}

impl Search<'_> {
    fn go(
        &mut self,
        host: Graph,
        changed: BitNum,
        depth: usize,
    ) -> Result<Option<Graph>, &'static str> {
        if self.config.seconds > 0.0 && self.timer.elapsed().as_secs_f64() >= self.config.seconds {
            return Err("time-limit");
        }
        if self.seen.contains(&host.bits()) {
            return Ok(None);
        }
        if self.seen.len() as u64 > self.config.steps {
            return Err("step-limit");
        }
        // Remaining depth is a function of H xor start, so duplicate hosts
        // always have the SAME depth budget. Plain exact-key memoization is safe.
        self.seen.insert(host.bits());
        let before = Instant::now();
        let mut copy = find_subgraph_ss(self.pattern, &self.row, &host);
        self.checks += 1;
        if copy.is_none() {
            copy = find_subgraph_ss(self.pattern, &self.row, &host.complement());
            self.checks += 1;
        }
        self.oracle_time += before.elapsed();
        let Some(copy) = copy else {
            return Ok(Some(host));
        };
        if depth == 0 {
            return Ok(None);
        }
        let mut choices: Vec<_> = bits(copy.bits() & !changed).collect();
        choices.shuffle(self.rng);
        for edge in choices {
            let bit = 1 << edge;
            let next = Graph::from_bits(host.size, host.bits() ^ bit);
            if let Some(hit) = self.go(next, changed | bit, depth - 1)? {
                return Ok(Some(hit));
            }
        }
        Ok(None)
    }
}

pub(super) fn walk(pattern: &Graph, start: Graph, config: &Config, rng: &mut StdRng) -> Outcome {
    let mut search = Search {
        pattern,
        row: build_sorted_row(pattern),
        config,
        rng,
        timer: Instant::now(),
        seen: HashSet::new(),
        checks: 0,
        oracle_time: Duration::ZERO,
    };
    let result = search.go(start, 0, config.depth.min(Graph::triangle(start.size)));
    let (status, host) = match result {
        Ok(Some(host)) => ("refuted", host),
        Ok(None) => ("neighbourhood-exhausted", start),
        Err(reason) => (reason, start),
    };
    Outcome {
        host,
        status,
        flips: search.seen.len().saturating_sub(1) as u64,
        checks: search.checks,
        constraints: 0,
        unsatisfied: 0,
        elapsed: search.timer.elapsed(),
        oracle_time: search.oracle_time,
        best_copies: (status == "refuted").then_some(0),
        diagnostics: Diagnostics::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhausted_balls_agree_with_brute_force() {
        let mut rng = StdRng::seed_from_u64(816);
        let config = Config {
            method: Method::Neighbourhood,
            depth: 2,
            tabu: 7,
            steps: 10_000,
            seconds: 0.0,
            max_constraints: 100,
            batch: 1,
            noise: 0.3,
            ..Config::default()
        };
        for _ in 0..80 {
            let pattern = random_graph(&mut rng, 4);
            if pattern.edge_count() < 2 {
                continue;
            }
            let start = random_graph(&mut rng, 4);
            let refutes = |host: Graph| {
                crate::perm::all_perms(4).all(|p| {
                    let copy = pattern.renumber(&p);
                    !copy.is_subgraph_of(&host) && !copy.is_subgraph_of(&host.complement())
                })
            };
            let possible = (0..64).any(|h: BitNum| {
                (h ^ start.bits()).count_ones() <= 2 && refutes(Graph::from_bits(4, h))
            });
            let result = walk(&pattern, start, &config, &mut rng);
            assert_eq!(result.status == "refuted", possible);
            if possible {
                assert!(refutes(result.host));
                assert!((result.host.bits() ^ start.bits()).count_ones() <= 2);
            } else {
                assert_eq!(result.status, "neighbourhood-exhausted");
            }
        }
    }
}
