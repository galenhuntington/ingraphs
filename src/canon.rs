//! Exact isomorphism keys for fixed-order caches, separate from the public
//! least-decimal representation in `enumerate`. These keys are not persistent
//! identifiers: backend/version/options may change their labeling.

use crate::base::{BitNum, Bits, Graph, MAX_SIZE};
use std::collections::BTreeSet;

/// An exact packed canonical graph, NOT a hash. Only compare keys for the
/// same vertex count. Fixed-order callers keep that count outside the cache
/// to avoid increasing the memory used by every visited state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key(BitNum);

impl Key {
    pub fn graph(self, size: usize) -> Graph {
        Graph::from_bits(size, self.0)
    }
}

#[cfg(feature = "nauty")]
unsafe extern "C" {
    fn graphy_canon(n: u32, rows: *const u32, out: *mut u32) -> std::ffi::c_int;
}

pub fn key(gr: &Graph) -> Key {
    assert!(
        gr.size <= MAX_SIZE && gr.size <= 32,
        "unsupported graph order"
    );
    assert_eq!(
        gr.bits() >> Graph::triangle(gr.size),
        0,
        "graph has edges outside its vertex count"
    );
    Key(canonical_bits(gr))
}

#[cfg(feature = "nauty")]
fn canonical_bits(gr: &Graph) -> BitNum {
    let mut rows = [0u32; 32];
    let mut out = [0u32; 32];
    for v in 1..gr.size {
        for u in 0..v {
            if gr.has_edge_raw(u, v) {
                rows[u] |= 1 << v;
                rows[v] |= 1 << u;
            }
        }
    }
    // SAFETY: validated n <= 32; both arrays have 32 aligned u32 entries,
    // remain alive, and do not alias. The shim retains no pointers and all
    // linked nauty objects use TLS. No nauty struct layout crosses the ABI.
    let status = unsafe { graphy_canon(gr.size as u32, rows.as_ptr(), out.as_mut_ptr()) };
    assert_eq!(
        status, 0,
        "nauty canonicalization failed with status {status}"
    );
    let mut bits = 0;
    for v in 1..gr.size {
        for u in 0..v {
            bits |= (((out[u] >> v) & 1) as BitNum) << crate::base::raw_index(u, v);
        }
    }
    bits
}

#[cfg(not(feature = "nauty"))]
fn canonical_bits(gr: &Graph) -> BitNum {
    if gr.size == 0 {
        return 0;
    }
    crate::enumerate::to_best(gr).bits()
}

/// One-edge extensions/retractions, deduplicated with internal keys.
/// `tools::bump` retains its least-decimal interface for other callers.
pub fn bump(gr: &Graph, extend: bool) -> BTreeSet<Key> {
    let mut keys = BTreeSet::new();
    for b in 0..Graph::triangle(gr.size) {
        let bit = (1 as BitNum) << b;
        if (gr.bits() & bit == 0) == extend {
            keys.insert(key(&Graph::from_bits(gr.size, gr.bits() ^ bit)));
        }
    }
    keys
}

/// Deduplicate with keys but retain the first ORIGINAL labeling of each
/// neighbor, in increasing toggled-bit order. This makes seeded seek's root
/// order and subsequent traversal independent of the canonical-key backend.
pub fn bump_representatives(gr: &Graph, extend: bool) -> Vec<Graph> {
    let mut keys = BTreeSet::new();
    let mut representatives = Vec::new();
    for b in 0..Graph::triangle(gr.size) {
        let bit = (1 as BitNum) << b;
        if (gr.bits() & bit == 0) == extend {
            let next = Graph::from_bits(gr.size, gr.bits() ^ bit);
            if keys.insert(key(&next)) { representatives.push(next); }
        }
    }
    representatives
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{base::random_graph, enumerate, perm::Perm};
    use rand::{SeedableRng, rngs::StdRng};
    use std::collections::BTreeMap;

    #[test]
    fn exhaustive_six_vertex_partition_matches_legacy() {
        let mut forward = BTreeMap::new();
        let mut backward = BTreeMap::new();
        for bits in 0..(1 << 15) {
            let g = Graph::from_bits(6, bits);
            let native = key(&g);
            let legacy = enumerate::to_best(&g).bits();
            if let Some(previous) = forward.insert(native, legacy) {
                assert_eq!(previous, legacy, "false canonical-key collision");
            }
            if let Some(previous) = backward.insert(legacy, native) {
                assert_eq!(previous, native, "isomorphic graphs got different keys");
            }
        }
        assert_eq!(forward.len(), 156);
        assert_eq!(backward.len(), 156);
    }

    #[test]
    fn relabeling_idempotence_and_edge_counts_at_every_order() {
        let mut rng = StdRng::seed_from_u64(20260924);
        for n in 0..=MAX_SIZE {
            for _ in 0..40 {
                let gr = random_graph(&mut rng, n);
                let canon = key(&gr);
                let perm = Perm::random(&mut rng, n);
                assert_eq!(canon, key(&gr.renumber(&perm)));
                assert_eq!(canon, key(&canon.graph(n)));
                assert_eq!(gr.edge_count(), canon.graph(n).edge_count());
            }
        }
    }

    #[test]
    fn concurrent_calls_match_serial() {
        let mut rng = StdRng::seed_from_u64(1234);
        let cases: Vec<_> = (0..512)
            .map(|i| {
                let gr = random_graph(&mut rng, i % (MAX_SIZE + 1));
                (gr, key(&gr))
            })
            .collect();
        std::thread::scope(|scope| {
            for worker in 0..8 {
                let cases = &cases;
                scope.spawn(move || {
                    for i in 0..cases.len() {
                        let (gr, expected) = &cases[(31 * i + worker) % cases.len()];
                        assert_eq!(key(gr), *expected);
                    }
                });
            }
        });
    }

    #[test]
    #[should_panic(expected = "unsupported graph order")]
    fn rejects_unsupported_order_before_ffi() {
        key(&Graph::from_bits(MAX_SIZE + 1, 0));
    }

    #[test]
    #[should_panic(expected = "edges outside")]
    fn rejects_out_of_range_edges() {
        key(&Graph::from_bits(3, 8));
    }
}
