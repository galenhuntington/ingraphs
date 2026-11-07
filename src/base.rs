/*!
    Representation of an undirected graph.
*/

use crate::perm::*;
use std::fmt;

type Pair = (usize, usize);

#[cfg(all(feature = "u64", feature = "u256"))]
compile_error!("features `u64` and `u256` are mutually exclusive; build each binary separately");

#[cfg(all(feature = "u64", not(feature = "u256")))]
pub const MAX_SIZE: usize = 11;
#[cfg(not(any(feature = "u64", feature = "u256")))]
pub const MAX_SIZE: usize = 16;
#[cfg(feature = "u256")]
pub const MAX_SIZE: usize = 23;

// Packed edge sets (and SAT-variable sets), not vertex masks. Construct masks
// with bit()/one_bits()/BitVec, never with an inferred integer `1 << index`:
// U256's mixed primitive operators would perform that shift in u128 first.
#[cfg(all(feature = "u64", not(feature = "u256")))]
pub type BitNum = u64;
#[cfg(not(any(feature = "u64", feature = "u256")))]
pub type BitNum = u128;
#[cfg(feature = "u256")]
pub type BitNum = ethnum::U256;

#[cfg(not(feature = "u256"))]
pub const ZERO: BitNum = 0;
#[cfg(feature = "u256")]
pub const ZERO: BitNum = BitNum::ZERO;

#[cfg(not(feature = "u256"))]
pub const ONE: BitNum = 1;
#[cfg(feature = "u256")]
pub const ONE: BitNum = BitNum::ONE;

#[inline]
pub const fn bit(index: usize) -> BitNum { BitVec::bit(index) }

#[inline]
pub fn one_bits(count: usize) -> BitNum { BitVec::low_mask(count) }

#[derive(Debug, PartialEq, Eq, Clone, Copy, Ord, PartialOrd)]
pub struct BitVec (pub BitNum);

pub trait Bits {
    fn bits(&self) -> BitNum;
    fn from_bits(size: usize, bits: BitNum) -> Self;
    fn from_u64(size: usize, bits: u64) -> Self where Self: Sized { Self::from_bits(size, BitNum::from(bits)) }
    fn show_bits(&self) -> String {
        let mut b = 1;
        let mut a = 0;
        let mut val = self.bits();
        let mut vec = Vec::new();
        loop {
            vec.push(if val & ONE == ONE { '1' } else { '0' });
            val >>= 1;
            if val == ZERO { break }
            a += 1;
            if a == b { vec.push('_'); b += 1; a = 0; }
        }
        vec.reverse();
        String::from_iter(vec)
    }
}

impl Bits for BitNum {
    fn bits(&self) -> BitNum { *self }
    fn from_bits(_size: usize, bits: BitNum) -> Self { bits }
}

impl BitVec {
    #[inline] pub const fn new() -> Self { BitVec(ZERO) }

    /// One packed bit, checked even in optimized builds. Vertex masks use u32
    /// separately; this API is for the build's full edge/SAT-variable width.
    #[inline]
    pub const fn bit(index: usize) -> BitNum {
        assert!(index < BitNum::BITS as usize, "packed bit index out of range");
        #[cfg(not(feature = "u256"))]
        { ONE << index }
        #[cfg(feature = "u256")]
        {
            if index < 128 { BitNum::from_words(0, 1u128 << index) }
            else { BitNum::from_words(1u128 << (index - 128), 0) }
        }
    }

    /// The low `count` bits, including the empty and full-width masks.
    #[inline]
    pub fn low_mask(count: usize) -> BitNum {
        assert!(count <= BitNum::BITS as usize, "packed mask width out of range");
        if count == BitNum::BITS as usize { !ZERO }
        else { bit(count) - ONE }
    }

    #[inline]
    pub fn toggle(&mut self, index: usize) { self.0 ^= bit(index); }
}

#[cfg(not(feature = "u256"))]
impl BitVec {
    #[inline] pub const fn get(&self, i: usize) -> bool { self.0 & bit(i) != ZERO }
    #[inline] pub const fn set(&mut self, i: usize) { self.0 |= bit(i) }
    #[inline] pub const fn unset(&mut self, i: usize) { self.0 &= !bit(i) }
}

// Own implementations needed so these are const, for EDGE_VECS.
#[cfg(feature = "u256")]
impl BitVec {
    #[inline]
    pub const fn get(&self, i: usize) -> bool {
        assert!(i < BitNum::BITS as usize, "packed bit index out of range");
        let (hi, lo) = self.0.into_words();
        if i < 128 { lo & (1u128 << i) != 0 } else { hi & (1u128 << (i - 128)) != 0 }
    }
    #[inline]
    pub const fn set(&mut self, i: usize) {
        assert!(i < BitNum::BITS as usize, "packed bit index out of range");
        let (mut hi, mut lo) = self.0.into_words();
        if i < 128 { lo |= 1u128 << i } else { hi |= 1u128 << (i - 128) }
        self.0 = BitNum::from_words(hi, lo);
    }
    #[inline]
    pub const fn unset(&mut self, i: usize) {
        assert!(i < BitNum::BITS as usize, "packed bit index out of range");
        let (mut hi, mut lo) = self.0.into_words();
        if i < 128 { lo &= !(1u128 << i) } else { hi &= !(1u128 << (i - 128)) }
        self.0 = BitNum::from_words(hi, lo);
    }
}

impl Default for BitVec { fn default() -> Self { Self::new() } }

impl Bits for BitVec {
    fn bits(&self) -> BitNum { self.0 }
    fn from_bits(_size: usize, bits: BitNum) -> Self { BitVec(bits) }
}


#[derive(PartialEq, Eq, Debug, Clone, Copy, Ord, PartialOrd)]
pub struct Triangle (pub BitVec);

#[derive(Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord)]
pub struct Graph { pub size: usize, pub edges: Triangle }

impl Triangle {
    #[inline]
    pub const fn empty(_sz: usize) -> Self { Triangle(BitVec::new()) }
    #[inline]
    pub const fn get(&self, (a, b): Pair) -> bool { self.0.get(index(a, b)) }
    #[inline]
    pub const fn set(&mut self, (a, b): Pair) { self.0.set(index(a, b)) }
    #[inline]
    pub const fn unset(&mut self, (a, b): Pair) { self.0.unset(index(a, b)) }
}

impl Bits for Triangle {
    fn bits(&self) -> BitNum { self.0.bits() }
    fn from_bits(_size: usize, bits: BitNum) -> Self { Triangle(BitVec(bits)) }
}

#[inline]
pub const fn raw_index(a: usize, b: usize) -> usize { b*(b-1) / 2 + a }

#[inline]
pub const fn index(a: usize, b: usize) -> usize {
    if a < b { raw_index(a, b) } else { raw_index(b, a) }
}

#[inline]
pub fn rev_hi_index(i: usize) -> usize {
    (((8*i + 1) as f64).sqrt() as usize).div_ceil(2)
}

pub fn rev_index(i: usize) -> Pair {
    let b = rev_hi_index(i);
    let a = i - b*(b-1)/2;
    (a, b)
}

const EDGE_VECS: [BitNum; MAX_SIZE] = {
    let mut vecs = [ZERO; MAX_SIZE];
    let mut i = 0;
    while i < MAX_SIZE {
        let mut tri = Triangle(BitVec::new());
        let mut j = 0;
        while j < MAX_SIZE {
            if i != j { tri.set((i, j)) }
            j += 1;
        }
        vecs[i] = tri.0.0;
        i += 1;
    }
    vecs
};

impl Graph {
    pub fn new(size: usize, edges: Triangle ) -> Self {
        // assert_eq!(edges.0.len(), Graph::triangle(size), "Invalid graph size!");
        Graph { size, edges }
    }
    #[inline]
    pub fn has_edge(&self, a: usize, b: usize) -> bool {
        a != b && self.edges.get((a, b))
    }
    // assume a < b
    #[inline]
    pub fn has_edge_raw(&self, a: usize, b: usize) -> bool {
        self.edges.get((a, b))
    }
    pub const fn triangle(sz: usize) -> usize { sz*sz.wrapping_sub(1)/2 } // sz=0 is dubious but used
    pub fn from_fn(size: usize, f: impl Fn(usize, usize) -> bool) -> Self {
        let mut edges = Triangle(BitVec::new());
        for b in 1..size { for a in 0..b {
            if f(a, b) { edges.set((a, b)) }
        }}
        Graph { size, edges }
    }
    pub fn unrenumber(&self, p: &Perm) -> Self {
        let size = self.size;
        let mut edges = Triangle(BitVec::new());
        for b in 1..size { for a in 0..b {
            if self.edges.get((p.apply(a), p.apply(b))) {
                edges.set((a, b));
            }
        }}
        Graph { size, edges }
    }
    pub fn renumber(&self, p: &Perm) -> Self { self.unrenumber(&p.inverse()) }
    pub fn slow_degree_of(&self, pt: usize) -> usize {
        (0..self.size).filter(|&x| self.has_edge(pt, x)).count()
    }
    // this optimization proved to not help noticeably
    pub fn degree_of(&self, pt: usize) -> usize {
        (self.edges.0.0 & EDGE_VECS[pt]).count_ones() as usize
    }
    pub fn edge_count(&self) -> usize {
        self.edges.0.0.count_ones() as usize
    }
    pub fn complement(&self) -> Self {
        let vec = one_bits(Graph::triangle(self.size)) ^ self.edges.0.0;
        Graph { size: self.size, edges: Triangle(BitVec(vec)) }
    }
    #[inline]
    pub fn is_subgraph_of(&self, other: &Graph) -> bool {
        self.edges.0.0 & !other.edges.0.0 == 0
    }
}

impl fmt::Display for Graph {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let v = (1 .. self.size)
            .flat_map(|b| (0..b)
                .filter(move |a| self.has_edge(*a, b))
                .map(move |a| format!("{}–{}", a, b)))
            .collect::<Vec<_>>();
        write!(f, "{}:[{}]", self.size, v.as_slice().join(" "))
    }
}

impl Bits for Graph {
    fn bits(&self) -> BitNum { self.edges.bits() }
    fn from_bits(size: usize, bits: BitNum) -> Self {
        Graph { size, edges: Triangle(BitVec(bits)) }
    }
}

pub fn random_graph(rng: &mut impl rand::Rng, size: usize) -> Graph {
    #[cfg(not(feature = "u256"))]
    let bits = rng.gen_range(ZERO..bit(Graph::triangle(size)));
    #[cfg(feature = "u256")]
    let bits = {
        let value = BitNum::from_words(rng.r#gen(), rng.r#gen());
        value & one_bits(Graph::triangle(size))
    };
    Graph::from_bits(size, bits)
}

// Not uniform across unlabelled graphs due to symmetries.
pub fn random_graph_edges(rng: &mut impl rand::Rng, size: usize, edges: usize) -> Graph {
    let top = Graph::triangle(size);
    let mut bits: BitNum = ZERO;
    while edges <= size && bits.count_ones() != edges as u32 {
        let b = rng.gen_range(0..top);
        bits |= bit(b);
    }
    Graph::from_bits(size, bits)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::testers::*;

    #[test]
    fn packed_bits_cover_both_words_and_full_width_masks() {
        let width = BitNum::BITS as usize;
        let mut value = BitVec(ZERO);
        assert_eq!(one_bits(0), ZERO);
        assert_eq!(one_bits(width), !ZERO);
        for i in 0..width {
            assert_eq!(bit(i).count_ones(), 1);
            assert_eq!(bit(i).trailing_zeros() as usize, i);
            assert_eq!(one_bits(i).count_ones() as usize, i);
            value.set(i);
            assert!(value.get(i));
            assert_eq!(value.0, one_bits(i + 1));
        }
        for i in 0..width {
            value.unset(i);
            assert_eq!(value.0, !ZERO ^ one_bits(i + 1));
            value.toggle(i);
            assert!(value.get(i));
            value.toggle(i);
            assert!(!value.get(i));
        }
        assert_eq!(value.0, ZERO);
    }

    #[test]
    fn packed_bit_bounds_are_checked_in_release_too() {
        let width = BitNum::BITS as usize;
        assert!(std::panic::catch_unwind(|| bit(width)).is_err());
        assert!(std::panic::catch_unwind(|| one_bits(width + 1)).is_err());
        assert!(std::panic::catch_unwind(|| BitVec(ZERO).get(width)).is_err());
        assert!(std::panic::catch_unwind(|| BitVec(ZERO).set(width)).is_err());
        assert!(std::panic::catch_unwind(|| BitVec(!ZERO).unset(width)).is_err());
        assert!(std::panic::catch_unwind(|| BitVec(ZERO).toggle(width)).is_err());
    }
    /*
    #[test]
    fn test_fn() {
        assert_eq!(
            Graph::from_fn(10, |_, _| true).edges.0.len(), Graph::triangle(10));
    }
    */
    #[test]
    fn test_renumber() {
        let rng = &mut rand::thread_rng();
        for _ in 1..80_000 {
            let len = rng.gen_range(2..MAX_SIZE);
            let p = Perm::random(rng, len);
            // XXX lopsided distro
            let b = rng.gen_range(1..len);
            let a = rng.gen_range(0..b);
            let pa = p.apply(a);
            let pb = p.apply(b);
            let (pa1, pb1) = if pa < pb { (pa, pb) } else { (pb, pa) };
            let gr = Graph::from_fn(len, |x, y| x==a && y==b);
            let gr2 = Graph::from_fn(len, |x, y| x==pa1 && y==pb1);
            assert_eq!(&gr.renumber(&p), &gr2);
            assert_eq!(gr.edge_count(), 1);
            assert_eq!(gr2.edge_count(), 1);
        }
    }
    #[test]
    fn test_fast_degree() {
        let rng = &mut rand::thread_rng();
        for _ in 0 .. 100_000 {
            let sz = rng.gen_range(1..=MAX_SIZE);
            let gr = random_graph(rng, sz);
            for j in 0 .. sz {
                assert_eq!(gr.slow_degree_of(j), gr.degree_of(j), "{:?}/{}\n", gr, j);
            }
        }
    }
    #[test]
    fn test_show_bits() {
        let gr = Graph::from_u64(10, 0);
        assert_eq!(gr.edges.show_bits(), "0");
        let gr = Graph::from_u64(10, 0b1100101);
        assert_eq!(gr.edges.show_bits(), "1_100_10_1");
    }
    #[test]
    fn test_edge_vec() {
        for i in 0..MAX_SIZE {
            for j in 0..MAX_SIZE {
                if i != j {
                    assert!(BitVec(EDGE_VECS[i]).get(index(i, j)));
                }
            }
            assert_eq!(EDGE_VECS[i].count_ones() as usize, MAX_SIZE - 1);
        }
    }
}
