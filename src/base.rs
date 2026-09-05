/*!
    Representation of an undirected graph.
*/

use crate::perm::*;
use std::fmt;

type Pair = (usize, usize);

#[cfg(not(any(feature = "bitnum-u64", feature = "bitnum-u128", feature = "bitnum-u256")))]
compile_error!("select exactly one BitNum feature: bitnum-u64, bitnum-u128, or bitnum-u256");

#[cfg(any(
    all(feature = "bitnum-u64", feature = "bitnum-u128"),
    all(feature = "bitnum-u64", feature = "bitnum-u256"),
    all(feature = "bitnum-u128", feature = "bitnum-u256"),
))]
compile_error!("BitNum features are mutually exclusive; select exactly one");

#[cfg(feature = "bitnum-u64")]
pub type BitNum = u64;
#[cfg(all(not(feature = "bitnum-u64"), feature = "bitnum-u128"))]
pub type BitNum = u128;
#[cfg(all(not(any(feature = "bitnum-u64", feature = "bitnum-u128")), feature = "bitnum-u256"))]
pub type BitNum = ethnum::U256;

#[cfg(feature = "bitnum-u64")]
pub const MAX_SIZE: usize = 11;
#[cfg(all(not(feature = "bitnum-u64"), feature = "bitnum-u128"))]
pub const MAX_SIZE: usize = 16;
#[cfg(all(not(any(feature = "bitnum-u64", feature = "bitnum-u128")), feature = "bitnum-u256"))]
pub const MAX_SIZE: usize = 23;

#[cfg(any(feature = "bitnum-u64", feature = "bitnum-u128"))]
pub const BITNUM_ZERO: BitNum = 0;
#[cfg(all(not(any(feature = "bitnum-u64", feature = "bitnum-u128")), feature = "bitnum-u256"))]
pub const BITNUM_ZERO: BitNum = BitNum::ZERO;
#[cfg(any(feature = "bitnum-u64", feature = "bitnum-u128"))]
pub const BITNUM_ONE: BitNum = 1;
#[cfg(all(not(any(feature = "bitnum-u64", feature = "bitnum-u128")), feature = "bitnum-u256"))]
pub const BITNUM_ONE: BitNum = BitNum::ONE;

#[cfg(any(feature = "bitnum-u64", feature = "bitnum-u128"))]
const fn bitnum_get(value: BitNum, i: usize) -> bool { value & (1 << i) != 0 }
#[cfg(all(not(any(feature = "bitnum-u64", feature = "bitnum-u128")), feature = "bitnum-u256"))]
const fn bitnum_get(value: BitNum, i: usize) -> bool {
    let (hi, lo) = value.into_words();
    if i < 128 { lo & (1 << i) != 0 } else { hi & (1 << (i - 128)) != 0 }
}

#[cfg(any(feature = "bitnum-u64", feature = "bitnum-u128"))]
const fn bitnum_set(value: BitNum, i: usize) -> BitNum { value | (1 << i) }
#[cfg(all(not(any(feature = "bitnum-u64", feature = "bitnum-u128")), feature = "bitnum-u256"))]
const fn bitnum_set(value: BitNum, i: usize) -> BitNum {
    let (mut hi, mut lo) = value.into_words();
    if i < 128 { lo |= 1 << i } else { hi |= 1 << (i - 128) }
    BitNum::from_words(hi, lo)
}

#[cfg(any(feature = "bitnum-u64", feature = "bitnum-u128"))]
const fn bitnum_unset(value: BitNum, i: usize) -> BitNum { value & !(1 << i) }
#[cfg(all(not(any(feature = "bitnum-u64", feature = "bitnum-u128")), feature = "bitnum-u256"))]
const fn bitnum_unset(value: BitNum, i: usize) -> BitNum {
    let (mut hi, mut lo) = value.into_words();
    if i < 128 { lo &= !(1 << i) } else { hi &= !(1 << (i - 128)) }
    BitNum::from_words(hi, lo)
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Ord, PartialOrd)]
pub struct BitVec (pub BitNum);

pub trait Bits {
    fn bits(&self) -> BitNum;
    fn from_bits(size: usize, bits: BitNum) -> Self;
    fn show_bits(&self) -> String {
        let mut b = 1;
        let mut a = 0;
        let mut val = self.bits();
        let mut vec = Vec::new();
        loop {
            vec.push(if val & BITNUM_ONE == BITNUM_ONE { '1' } else { '0' });
            val >>= 1;
            if val == BITNUM_ZERO { break }
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
    #[inline] pub const fn new() -> Self { BitVec(BITNUM_ZERO) }
    #[inline] pub const fn set(&mut self, i: usize) { self.0 = bitnum_set(self.0, i) }
    #[inline] pub const fn unset(&mut self, i: usize) { self.0 = bitnum_unset(self.0, i) }
    #[inline] pub const fn get(&self, i: usize) -> bool { bitnum_get(self.0, i) }
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
    let mut vecs = [BITNUM_ZERO; MAX_SIZE];
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
    /*
*/
    vecs
};

impl Graph {
    pub fn new(size: usize, edges: Triangle ) -> Self {
        assert!(size <= MAX_SIZE, "graph size {size} exceeds configured maximum {MAX_SIZE}");
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
    pub const fn triangle(sz: usize) -> usize { sz*(sz-1)/2 }
    pub fn from_fn(size: usize, f: impl Fn(usize, usize) -> bool) -> Self {
        assert!(size <= MAX_SIZE, "graph size {size} exceeds configured maximum {MAX_SIZE}");
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
        let vec = ((BITNUM_ONE << Graph::triangle(self.size)) - BITNUM_ONE) ^ self.edges.0.0;
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
        assert!(size <= MAX_SIZE, "graph size {size} exceeds configured maximum {MAX_SIZE}");
        Graph { size, edges: Triangle(BitVec(bits)) }
    }
}

pub fn random_graph(rng: &mut impl rand::Rng, size: usize) -> Graph {
    #[cfg(any(feature = "bitnum-u64", feature = "bitnum-u128"))]
    let bits = rng.gen_range(BITNUM_ZERO..(1 << Graph::triangle(size)));
    #[cfg(all(not(any(feature = "bitnum-u64", feature = "bitnum-u128")), feature = "bitnum-u256"))]
    let bits = {
        let value = BitNum::from_words(rng.r#gen(), rng.r#gen());
        value & ((BITNUM_ONE << Graph::triangle(size)) - BITNUM_ONE)
    };
    Graph::from_bits(size, bits)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::testers::*;
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
        let gr = Graph::from_bits(10, BITNUM_ZERO);
        assert_eq!(gr.edges.show_bits(), "0");
        let gr = Graph::from_bits(10, BitNum::from(0b1100101u8));
        assert_eq!(gr.edges.show_bits(), "1_100_10_1");
    }

    const TOP_BIT: BitVec = {
        let mut bits = BitVec::new();
        bits.set(Graph::triangle(MAX_SIZE) - 1);
        bits
    };

    #[test]
    fn test_configured_size() {
        #[cfg(feature = "bitnum-u64")]
        assert_eq!((BitNum::BITS, MAX_SIZE), (64, 11));
        #[cfg(feature = "bitnum-u128")]
        assert_eq!((BitNum::BITS, MAX_SIZE), (128, 16));
        #[cfg(feature = "bitnum-u256")]
        assert_eq!((BitNum::BITS, MAX_SIZE), (256, 23));
        assert!(Graph::triangle(MAX_SIZE) <= BitNum::BITS as usize);
        assert!(Graph::triangle(MAX_SIZE + 1) > BitNum::BITS as usize);
    }

    #[test]
    fn test_max_size_boundary() {
        let top = Graph::triangle(MAX_SIZE) - 1;
        assert!(TOP_BIT.get(top));
        assert_eq!(TOP_BIT.0.count_ones(), 1);
        let complete = Graph::from_fn(MAX_SIZE, |_, _| true);
        assert_eq!(complete.edge_count(), Graph::triangle(MAX_SIZE));
        assert_eq!(complete.degree_of(MAX_SIZE - 1), MAX_SIZE - 1);
        assert_eq!(complete.complement().edge_count(), 0);
    }

    #[cfg(feature = "bitnum-u256")]
    #[test]
    fn test_u256_decimal_round_trip() {
        let value: BitNum = BITNUM_ONE << 200usize | BitNum::from(12345u16);
        assert_eq!(value.to_string().parse::<BitNum>().unwrap(), value);
    }

    #[test]
    fn test_edge_vec() {
        for i in 0..MAX_SIZE {
            for j in 0..MAX_SIZE {
                if i != j {
                    assert!(bitnum_get(EDGE_VECS[i], index(i, j)));
                }
            }
            assert_eq!(EDGE_VECS[i].count_ones() as usize, MAX_SIZE - 1);
        }
    }
}

