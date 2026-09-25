//! One-edge extensions all of whose one-edge retractions are in the pool.
use crate::{
    base::{BitNum, Bits, Graph, MAX_SIZE},
    canon,
};
use std::collections::BTreeSet;

pub fn generate(size: usize, pool: impl Iterator<Item = BitNum>) -> impl Iterator<Item = Graph> {
    assert!(size <= MAX_SIZE, "unsupported graph order");
    // Normalize even if input is from geng, labelg, or a different nauty version.
    let pool: BTreeSet<_> = pool
        .map(|bits| canon::key(&Graph::from_bits(size, bits)))
        .collect();
    let extensions: BTreeSet<_> = pool
        .iter()
        .flat_map(|key| canon::bump(&key.graph(size), true))
        .collect();
    extensions.into_iter().filter_map(move |key| {
        let gr = key.graph(size);
        // Stop at the first missing retraction. Repeated isomorphic retractions
        // are harmless; avoid allocating a set for each candidate extension.
        let mut remaining = gr.bits();
        while remaining != 0 {
            let b = remaining.trailing_zeros();
            remaining &= remaining - 1;
            let retraction = Graph::from_bits(size, gr.bits() ^ ((1 as BitNum) << b));
            if !pool.contains(&canon::key(&retraction)) {
                return None;
            }
        }
        Some(gr)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{enumerate, perm::Perm, tools};
    use rand::{SeedableRng, rngs::StdRng};

    fn legacy(size: usize, pool: &[BitNum]) -> BTreeSet<BitNum> {
        let pool: BTreeSet<_> = pool.iter().copied().collect();
        let extensions: BTreeSet<_> = pool
            .iter()
            .flat_map(|&g| tools::bump(&Graph::from_bits(size, g), true))
            .collect();
        extensions
            .into_iter()
            .filter(|&g| {
                tools::bump(&Graph::from_bits(size, g), false)
                    .iter()
                    .all(|r| pool.contains(r))
            })
            .collect()
    }

    #[test]
    fn all_small_levels_and_partial_pools_match_legacy() {
        let mut rng = StdRng::seed_from_u64(20260924);
        for size in 2..=6 {
            let mut levels = vec![Vec::new(); Graph::triangle(size) + 1];
            enumerate::enumerate_graphs(size, None, |bits| {
                levels[bits.count_ones() as usize].push(bits)
            });
            for level in levels {
                for stride in 1..=3 {
                    let pool: Vec<_> = level.iter().copied().step_by(stride).collect();
                    let expected = legacy(size, &pool);
                    let shuffled = pool
                        .iter()
                        .map(|&bits| {
                            let g = Graph::from_bits(size, bits);
                            g.renumber(&Perm::random(&mut rng, size)).bits()
                        })
                        .collect::<Vec<_>>();
                    let actual = generate(size, shuffled.into_iter())
                        .map(|g| enumerate::to_best(&g).bits())
                        .collect();
                    assert_eq!(expected, actual, "n={size}, stride={stride}");
                }
            }
        }
    }

    #[test]
    fn empty_input_and_duplicate_inputs() {
        assert_eq!(generate(0, std::iter::empty()).count(), 0);
        assert_eq!(generate(1, [0, 0].into_iter()).count(), 0);
        assert_eq!(generate(3, [1, 1, 2, 4].into_iter()).count(), 1);
    }
}
