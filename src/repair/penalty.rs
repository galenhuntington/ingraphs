//! Bounded extra weights on labelled NAE copy constraints. This biases the
//! walk, never the exact unweighted count, archive ranking, or acceptance gate.

use super::{BitNum, bits};
use std::collections::HashMap;

const MAX_WEIGHT: u32 = 1_000_000;

#[derive(Clone, Copy)]
struct Entry {
    mask: BitNum,
    weight: u32,
}

pub(super) struct Penalties {
    entries: Vec<Entry>,
    indices: HashMap<BitNum, usize>,
    capacity: usize,
    next_evict: usize,
    pub updates: u64,
}

impl Penalties {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            indices: HashMap::new(),
            capacity,
            next_evict: 0,
            updates: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn boost(&mut self, masks: impl IntoIterator<Item = BitNum>, step: u32, decay: u64) {
        assert!(self.capacity > 0 && (1..=MAX_WEIGHT).contains(&step));
        self.updates += 1;
        if decay != 0 && self.updates % decay == 0 {
            // The ring's next eviction is the oldest entry. Restore age order
            // before filtering, so compaction preserves FIFO eviction too.
            if self.entries.len() == self.capacity {
                self.entries.rotate_left(self.next_evict);
            }
            for entry in &mut self.entries {
                entry.weight /= 2;
            }
            self.entries.retain(|entry| entry.weight != 0);
            self.indices.clear();
            for (i, entry) in self.entries.iter().enumerate() {
                self.indices.insert(entry.mask, i);
            }
            self.next_evict = 0;
        }
        for mask in masks {
            assert!(mask.count_ones() >= 2);
            if let Some(&i) = self.indices.get(&mask) {
                self.entries[i].weight =
                    self.entries[i].weight.saturating_add(step).min(MAX_WEIGHT);
            } else if self.entries.len() < self.capacity {
                self.indices.insert(mask, self.entries.len());
                self.entries.push(Entry { mask, weight: step });
            } else {
                let i = self.next_evict;
                self.indices.remove(&self.entries[i].mask);
                self.entries[i] = Entry { mask, weight: step };
                self.indices.insert(mask, i);
                self.next_evict = (i + 1) % self.capacity;
            }
        }
    }

    pub fn deltas(&self, host: BitNum, variables: usize) -> Vec<i64> {
        let mut deltas = vec![0; variables];
        for &Entry { mask, weight } in &self.entries {
            let red = mask & host;
            let blue = mask & !host;
            let weight = weight as i64;
            if red == 0 || blue == 0 {
                for e in bits(mask) {
                    deltas[e] -= weight;
                }
            } else {
                if red.count_ones() == 1 {
                    deltas[red.trailing_zeros() as usize] += weight;
                }
                if blue.count_ones() == 1 {
                    deltas[blue.trailing_zeros() as usize] += weight;
                }
            }
        }
        deltas
    }

    #[cfg(test)]
    fn cost(&self, host: BitNum) -> i64 {
        self.entries
            .iter()
            .filter(|e| e.mask & host == 0 || e.mask & !host == 0)
            .map(|e| e.weight as i64)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_deltas_match_recounting_including_two_edge_constraints() {
        let mut memory = Penalties::new(100);
        for mask in 0..64 as BitNum {
            if mask.count_ones() >= 2 {
                memory.boost([mask], (mask % 5 + 1) as u32, 0);
            }
        }
        for host in 0..64 {
            let old = memory.cost(host);
            for (e, delta) in memory.deltas(host, 6).into_iter().enumerate() {
                assert_eq!(delta, memory.cost(host ^ (1 << e)) - old);
            }
        }
    }

    #[test]
    fn bounded_eviction_decay_and_determinism() {
        let mut a = Penalties::new(3);
        let mut b = Penalties::new(3);
        for i in 0..500 {
            let mask = [3, 5, 6, 7, 9][i % 5];
            a.boost([mask], 1, 7);
            b.boost([mask], 1, 7);
            assert!(a.len() <= 3);
            assert_eq!(a.indices.len(), a.len());
            for (j, entry) in a.entries.iter().enumerate() {
                assert_eq!(a.indices[&entry.mask], j);
            }
            assert_eq!(a.deltas(9, 4), b.deltas(9, 4));
        }
        let mut memory = Penalties::new(3);
        memory.boost([3], 1, 2);
        memory.boost([5], 1, 2);
        assert_eq!(memory.len(), 1); // the weight-one old entry was smoothed away
        memory.boost([5], MAX_WEIGHT, 0);
        memory.boost([5], MAX_WEIGHT, 0);
        assert_eq!(memory.cost(0), MAX_WEIGHT as i64);
        let mut ring = Penalties::new(3);
        for mask in [3, 5, 6, 9] {
            ring.boost([mask], 4, 5);
        }
        // At update 5, 5 is the oldest surviving constraint even though its
        // vector index was 1 before smoothing; it must be evicted next.
        ring.boost([10], 4, 5);
        assert!(!ring.indices.contains_key(&5));
        assert!(ring.indices.contains_key(&9));
    }
}
