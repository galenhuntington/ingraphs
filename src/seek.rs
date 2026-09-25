use crate::base::{Graph,Bits,MAX_SIZE};
use crate::canon;
use crate::tools;
use crate::enumerate;
use dashmap::DashSet;
use rayon::prelude::*;
use rand::{Rng,SeedableRng,thread_rng,rngs::StdRng,prelude::SliceRandom};
use std::sync::atomic::{AtomicBool,Ordering};

struct Fixed<'a> {
    gr: &'a Graph,
    seen: &'a DashSet<canon::Key>,
    row: &'a Vec<(usize, usize)>,
    limit: usize,
    rng: StdRng,
    quit: &'a AtomicBool,
    // Size limitations only seem to slow things down.
    // top_size: u32,
    // bit_mask: BitNum,
}

fn recurse(fixed: &mut Fixed, ce: Graph) -> Option<Graph> {
    if fixed.quit.load(Ordering::Relaxed) { return None }
    // eprintln!("recurse: {} {:?}", grc, grc);
    match tools::find_subgraph_ss(fixed.gr, fixed.row, &ce.complement()) {
        Some(grtw) => {
            // if ce.bits().count_ones() == fixed.top_size { return None }
            let hi = tools::hi_bit_ix(grtw.bits()) + 1;
            // let skew = ce.bits() as usize % hi;
            let skew = fixed.rng.gen_range(0..hi);
            for b in 0..hi {
                if fixed.quit.load(Ordering::Relaxed) { return None }
                // let bit = 1 << b;
                // let bit = 1 << (hi - 1 - b);
                let bit = 1 << { let b = b + skew; if b >= hi { b - hi } else { b } };
                if grtw.bits() & bit == 0 { continue }
                let grnext = ce.bits() | bit;
                // eprintln!("{}: {} -> {}", b, grtw, grnext);
                let grnext = Graph::from_bits(fixed.gr.size, grnext);
                // eprintln!("1 {}: {} -> {}; {}", b, grtw, grnext, fixed.gr);
                if tools::isso_inner(fixed.gr, fixed.row, &grnext) { continue }
                // eprintln!("2 {}: {} -> {}; {}", b, grtw, grnext, fixed.gr);
                let key = canon::key(&grnext);
                let seen = fixed.seen;
                if fixed.quit.load(Ordering::Relaxed) { return None }
                if !seen.insert(key) { continue }
                if seen.len() >= fixed.limit {
                    // A bounded run is inconclusive. Stop all workers instead
                    // of continuing to insert states in sibling branches.
                    fixed.quit.store(true, Ordering::Relaxed);
                    return None
                }
                /*
                if seen.len().is_multiple_of(100_000) {
                    eprint!("\n Checked {}\r", seen.len());
                }
                */
                let ans = recurse(fixed, grnext);
                if ans.is_some() {
                    // find_map_any doesn't actually stop other threads, so
                    // we have to signal others to quit
                    fixed.quit.store(true, Ordering::Relaxed);
                    return ans;
                }
            }
            None
        },
        None => Some(ce),
    }
}

pub fn seek(gr: &Graph, limit: usize) -> (Option<Graph>, usize) {
    seek_seeded(gr, limit, thread_rng().r#gen())
}

/// Reproducible with a fixed seed and a single Rayon worker. Parallel cache
/// races can still change traversal order. None at a finite limit is NOT a
/// universality certificate, just as with the unseeded interface.
pub fn seek_seeded(gr: &Graph, limit: usize, seed: u64) -> (Option<Graph>, usize) {
    assert!(gr.size <= MAX_SIZE, "unsupported graph order");
    if limit == 0 { return (None, 1) }
    let seen = &DashSet::new();
    let quit = &AtomicBool::new(false);
    /*
    for grm in tools::bump(&gr, false) {
        let grm = Graph::from_bits(gr.size, grm);
        eprintln!("seek: {} / {} {}", grm, grm.show_bits(), tools::count_symmetries(&grm));
    }
    */
    let mut rets = canon::bump_representatives(gr, false);
    let mut rng = StdRng::seed_from_u64(seed);
    rets.shuffle(&mut rng);
    let rets: Vec<_> = rets.into_iter().map(|grm| (grm, rng.r#gen::<u64>())).collect();
    let row = tools::build_sorted_row(gr);
    // let rets: Vec<_> = rets.iter().cloned().rev().collect();
    let res = rets.par_iter().find_map_any(|(grm, branch_seed)| {
        // eprintln!("seek: {:?} / {}", grm, tools::count_symmetries(&grm));
        recurse(&mut Fixed {
                gr,
                seen,
                row: &row,
                limit,
                rng: StdRng::seed_from_u64(*branch_seed),
                quit,
                // top_size: (Graph::triangle(gr.size) as u32 + 1) / 2,
                // bit_mask: (1 << Graph::triangle(gr.size)) - 1,
            },
            *grm,
        )
    });
    // Preserve the external least-decimal witness convention, only once.
    (res.map(|g| enumerate::to_best(&g)), seen.len())
}

pub fn seek_full(gr: &Graph) -> (Option<Graph>, usize) { seek(gr, usize::MAX) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhaustive_small_seek_agrees_with_independent_permutation_checks() {
        for workers in [1, 4] {
            let pool = rayon::ThreadPoolBuilder::new().num_threads(workers).build().unwrap();
            pool.install(|| {
                for n in 1..=5 {
                    let mut graphs = Vec::new();
                    enumerate::enumerate_graphs(n, None, |bits|
                        graphs.push(Graph::from_bits(n, bits)));
                    for gr in &graphs {
                        let refutable = graphs.iter().any(|host|
                            !tools::naive_is_subgraph_of(gr, host)
                            && !tools::naive_is_subgraph_of(gr, &host.complement()));
                        let (found, _) = seek_seeded(gr, usize::MAX, 1234);
                        assert_eq!(found.is_some(), refutable, "n={n}, graph={gr}");
                        if let Some(host) = found {
                            assert!(!tools::naive_is_subgraph_of(gr, &host));
                            assert!(!tools::naive_is_subgraph_of(gr, &host.complement()));
                            assert_eq!(host, enumerate::to_best(&host));
                        }
                    }
                }
            });
        }
    }

    #[test]
    fn fixed_seed_is_repeatable_with_one_worker() {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        pool.install(|| {
            let gr = Graph::from_bits(7, 1118);
            assert_eq!(seek_seeded(&gr, 1000, 789), seek_seeded(&gr, 1000, 789));
        });
    }

    #[test]
    fn zero_budget_and_global_bailout() {
        // 94 is universal for n=6, so no worker can find a counterexample.
        let gr = Graph::from_bits(6, 94);
        assert_eq!(seek_seeded(&gr, 0, 123), (None, 1));
        let pool = rayon::ThreadPoolBuilder::new().num_threads(4).build().unwrap();
        let (found, visited) = pool.install(|| seek_seeded(&gr, 1, 123));
        assert!(found.is_none());
        // Workers already between the cancellation check and insertion can
        // overshoot the soft cap by at most one entry per other worker.
        assert!((1..=4).contains(&visited));
        assert_eq!(seek_seeded(&Graph::from_bits(0, 0), usize::MAX, 123), (None, 0));
    }
}
