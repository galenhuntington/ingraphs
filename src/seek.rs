use crate::base::{Graph,Bits,BitNum};
use crate::tools;
use crate::enumerate;
use dashmap::DashSet;
use rayon::prelude::*;
use rand::{Rng,thread_rng,rngs};
use std::sync::atomic::{AtomicBool,Ordering};

struct Fixed<'a> {
    gr: &'a Graph,
    seen: &'a DashSet<BitNum>,
    row: &'a Vec<(usize, usize)>,
    limit: usize,
    rng: rngs::ThreadRng,
    quit: &'a AtomicBool,
    // Size limitations only seem to slow things down.
    // top_size: u32,
    // bit_mask: BitNum,
}

fn recurse(fixed: &mut Fixed, ce: Graph) -> Option<Graph> {
    // eprintln!("recurse: {} {:?}", grc, grc);
    match tools::find_subgraph_ss(fixed.gr, fixed.row, &ce.complement()) {
        Some(grtw) => {
            // if ce.bits().count_ones() == fixed.top_size { return None }
            let hi = tools::hi_bit_ix(grtw.bits()) + 1;
            // let skew = ce.bits() as usize % hi;
            let skew = fixed.rng.gen_range(0..hi);
            for b in 0..hi {
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
                let grnext = enumerate::to_best(&grnext);
                let seen = fixed.seen;
                if fixed.quit.load(Ordering::Relaxed) { return None }
                if !seen.insert(grnext.bits()) { continue }
                if seen.len() >= fixed.limit { return None }
                if seen.len().is_multiple_of(100_000) {
                    eprint!("\n Checked {}\r", seen.len());
                }
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
    let seen = &DashSet::new();
    let quit = &AtomicBool::new(false);
    /*
    for grm in tools::bump(&gr, false) {
        let grm = Graph::from_bits(gr.size, grm);
        eprintln!("seek: {} / {} {}", grm, grm.show_bits(), tools::count_symmetries(&grm));
    }
    */
    let rets = tools::bump(gr, false);
    // let rets: Vec<_> = rets.iter().cloned().rev().collect();
    let res = rets.par_iter().find_map_any(|grm| {
        let grm = Graph::from_bits(gr.size, *grm);
        // eprintln!("seek: {:?} / {}", grm, tools::count_symmetries(&grm));
        recurse(&mut Fixed {
                gr,
                seen,
                row: &tools::build_sorted_row(gr),
                limit,
                rng: thread_rng(),
                quit,
                // top_size: (Graph::triangle(gr.size) as u32 + 1) / 2,
                // bit_mask: (1 << Graph::triangle(gr.size)) - 1,
            },
            grm,
        )
    });
    (res, seen.len())
}

pub fn seek_full(gr: &Graph) -> (Option<Graph>, usize) { seek(gr, usize::MAX) }

