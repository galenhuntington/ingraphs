//! Whole-vertex repair regions. Only the induced graph on the untouched
//! vertices is fixed; isomorphic (or complementary) fixed graphs define
//! equivalent feasibility problems. Deduplication does not relabel any live
//! SAT clause or assert that a timed-out region is infeasible.

use super::*;
use crate::canon;

#[derive(Clone, Copy, Debug)]
pub(super) struct VertexBlock {
    pub anchor: Graph,
    pub vertices: u32,
    pub free: BitNum,
    pub fixed_key: BitNum,
}

pub(super) fn star_mask(n: usize, vertices: u32) -> BitNum {
    Graph::from_fn(n, |u, v| vertices & ((1 << u) | (1 << v)) != 0).bits()
}

fn fixed_graph(host: Graph, vertices: u32) -> Graph {
    let kept: Vec<_> = (0..host.size)
        .filter(|v| vertices & (1 << v) == 0)
        .collect();
    Graph::from_fn(kept.len(), |u, v| host.has_edge(kept[u], kept[v]))
}

pub(super) fn region(anchor: Graph, vertices: u32) -> VertexBlock {
    let fixed = fixed_graph(anchor, vertices);
    let key = canon::key(&fixed).min(canon::key(&fixed.complement()));
    VertexBlock {
        anchor,
        vertices,
        free: star_mask(anchor.size, vertices),
        fixed_key: key.graph(fixed.size).bits(),
    }
}

impl VertexBlock {
    pub fn class(&self) -> (u32, BitNum) {
        // The fixed-core order is essential when expansion changes its size.
        (self.vertices.count_ones(), self.fixed_key)
    }
}

/// Relax one more whole vertex after a genuine local UNSAT. Prefer a vertex
/// incident to many failed assumptions, breaking ties randomly. A full core
/// gives no vertex preference; an empty core must never trigger expansion.
pub(super) fn expand(
    previous: VertexBlock,
    core: BitNum,
    maximum: usize,
    attempted: &HashSet<(u32, BitNum)>,
    rng: &mut StdRng,
) -> Option<VertexBlock> {
    if previous.vertices.count_ones() as usize >= maximum || core == 0 {
        return None;
    }
    assert_eq!(previous.free & core, 0);
    let mut vertices: Vec<_> = (0..previous.anchor.size)
        .filter(|v| previous.vertices & (1 << v) == 0)
        .collect();
    vertices.shuffle(rng);
    // Stable sort preserves the random tie order.
    vertices.sort_by_key(|v| {
        std::cmp::Reverse((star_mask(previous.anchor.size, 1 << v) & core).count_ones())
    });
    for v in vertices {
        if star_mask(previous.anchor.size, 1 << v) & core == 0 {
            continue;
        }
        let next = region(previous.anchor, previous.vertices | (1 << v));
        if !attempted.contains(&next.class()) {
            return Some(next);
        }
    }
    None
}

/// Enumerate once, retain original labels, then shuffle equivalence classes
/// (not vertex subsets, which would over-sample classes with many embeddings).
pub(super) fn vertex_blocks(
    seeds: &[Graph],
    vertices: usize,
    rng: &mut StdRng,
) -> (Vec<VertexBlock>, usize) {
    assert!(!seeds.is_empty());
    let n = seeds[0].size;
    assert!(vertices <= n && seeds.iter().all(|h| h.size == n));
    let mut subsets: Vec<u32> = (0..1u32 << n)
        .filter(|s| s.count_ones() as usize == vertices)
        .collect();
    let labelled = subsets.len() * seeds.len();
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for &anchor in seeds {
        subsets.shuffle(rng);
        for &selected in &subsets {
            let block = region(anchor, selected);
            if seen.insert(block.class()) {
                result.push(block);
            }
        }
    }
    result.shuffle(rng);
    (result, labelled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stars_free_exactly_incident_edges() {
        for n in 1..=MAX_SIZE {
            for k in 0..=n {
                let selected = (1 << k) - 1;
                let mask = star_mask(n, selected);
                assert_eq!(
                    mask.count_ones() as usize,
                    Graph::triangle(n) - Graph::triangle(n - k)
                );
                for v in 1..n {
                    for u in 0..v {
                        assert_eq!(mask & (1 << crate::base::index(u, v)) != 0, u < k || v < k);
                    }
                }
            }
        }
    }

    #[test]
    fn fixed_core_dedup_is_exact_up_to_complement() {
        let host = Graph::from_bits(5, 613); // C5
        let mut rng = StdRng::seed_from_u64(743);
        let relabelled = host.renumber(&Perm::random(&mut rng, 5));
        let (blocks, labelled) = vertex_blocks(&[host, relabelled, host.complement()], 2, &mut rng);
        assert_eq!(labelled, 30);
        assert_eq!(blocks.len(), 1); // Every three-vertex core is P3 or its complement.
        let b = blocks[0];
        assert_eq!(b.free, star_mask(5, b.vertices));
        assert_eq!(b.vertices.count_ones(), 2);
        assert_eq!(fixed_graph(b.anchor, b.vertices).size, 3);
        let (whole, _) = vertex_blocks(&[host, relabelled], 5, &mut rng);
        assert_eq!(whole.len(), 1);
        assert_eq!(whole[0].free, 1023);
        assert_eq!(whole[0].fixed_key, 0);
    }

    #[test]
    fn deduplicated_regions_preserve_exhaustive_feasibility() {
        // All four-vertex anchors, all two-vertex rewiring regions. Compare
        // full enumeration with their fixed-core representatives independently
        // for every four-vertex candidate, not just the selector's own masks.
        let seeds: Vec<_> = (0..64).map(|h| Graph::from_bits(4, h)).collect();
        let mut rng = StdRng::seed_from_u64(23);
        let (blocks, labelled) = vertex_blocks(&seeds, 2, &mut rng);
        assert_eq!(labelled, 384);
        assert_eq!(blocks.len(), 1); // K2 and its complement have equivalent extensions.
        let b = blocks[0];
        for f in 0..64 {
            let pattern = Graph::from_bits(4, f);
            let valid = |h| super::super::tests::naive_refutes(&pattern, Graph::from_bits(4, h));
            let global = (0..64).any(valid);
            let representative = (0..64).any(|h| (h ^ b.anchor.bits()) & !b.free == 0 && valid(h));
            assert_eq!(global, representative);
        }
    }

    #[test]
    fn expansion_hits_core_preserves_anchor_and_never_repeats_a_class() {
        let mut rng = StdRng::seed_from_u64(89);
        let original = region(Graph::from_bits(6, 1734), 1);
        let core = (1 << crate::base::index(2, 3)) | (1 << crate::base::index(2, 4));
        let mut seen = HashSet::new();
        let next = expand(original, core, 3, &seen, &mut rng).unwrap();
        // Vertex 2 is the only one hitting both failed assumptions.
        assert_eq!(next.vertices, 1 | (1 << 2));
        assert_eq!(next.anchor, original.anchor);
        assert_eq!(next.free & original.free, original.free);
        assert_eq!(next.free & core, core);
        seen.insert(next.class());
        while let Some(next) = expand(original, core, 3, &seen, &mut rng) {
            assert!(seen.insert(next.class()));
        }
        assert!(expand(original, 0, 3, &HashSet::new(), &mut rng).is_none());
        assert!(expand(original, core, 1, &HashSet::new(), &mut rng).is_none());
    }

    #[test]
    fn multiple_core_classes_match_independent_extension_search() {
        let mut rng = StdRng::seed_from_u64(283);
        let seeds: Vec<_> = (0..12).map(|_| random_graph(&mut rng, 5)).collect();
        for k in [1, 2] {
            let (representatives, _) = vertex_blocks(&seeds, k, &mut rng);
            assert!(representatives.len() > 1);
            for f in [3, 7, 13, 19, 31, 59, 613] {
                let pattern = Graph::from_bits(5, f);
                let refuters: Vec<_> = (0..1024)
                    .filter(|&h| {
                        super::super::tests::naive_refutes(&pattern, Graph::from_bits(5, h))
                    })
                    .collect();
                let feasible = |b: VertexBlock| {
                    refuters
                        .iter()
                        .any(|h| (h ^ b.anchor.bits()) & !b.free == 0)
                };
                for &host in &seeds {
                    for selected in (0..32u32).filter(|s| s.count_ones() as usize == k) {
                        let b = region(host, selected);
                        let representative = *representatives
                            .iter()
                            .find(|r| r.class() == b.class())
                            .unwrap();
                        assert_eq!(feasible(b), feasible(representative));
                    }
                }
            }
        }
    }
}
