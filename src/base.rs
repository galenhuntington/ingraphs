/*!
    Representation of an undirected graph.
*/

use crate::perm::*;
use std::fmt;

type Pair = (usize, usize);
// BitNum can be u64 if graphs' max size is 11
pub const MAX_SIZE: usize = 16;
pub type BitNum = u128;

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
            vec.push(if val & 1 == 1 { '1' } else { '0' });
            val >>= 1;
            if val == 0 { break }
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
    #[inline] pub const fn new() -> Self { BitVec(0) }
    #[inline] pub const fn set(&mut self, i: usize) { self.0 |= 1 << i }
    #[inline] pub const fn unset(&mut self, i: usize) { self.0 &= !(1 << i) }
    #[inline] pub const fn get(&self, i: usize) -> bool { self.0 & (1 << i) != 0 }
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
    pub const fn empty(_sz: usize) -> Self { Triangle(BitVec(0)) }
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
    let mut vecs = [0; MAX_SIZE];
    let mut i = 0;
    while i < MAX_SIZE {
        let mut tri = Triangle(BitVec(0));
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
        let mut edges = Triangle(BitVec(0));
        for b in 1..size { for a in 0..b {
            if f(a, b) { edges.set((a, b)) }
        }}
        Graph { size, edges }
    }
    pub fn unrenumber(&self, p: &Perm) -> Self {
        let size = self.size;
        let mut edges = Triangle(BitVec(0));
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
        let vec = ((1 << Graph::triangle(self.size)) - 1) ^ self.edges.0.0;
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
    Graph::from_bits(size, rng.gen_range(0..(1 << Graph::triangle(size))))
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
        let gr = Graph::from_bits(10, 0);
        assert_eq!(gr.edges.show_bits(), "0");
        let gr = Graph::from_bits(10, 0b1100101);
        assert_eq!(gr.edges.show_bits(), "1_100_10_1");
    }
    #[test]
    fn test_edge_vec() {
        assert_eq!(EDGE_VECS[10], 0x8002001001002007fe00000000000_u128 as BitNum);
    }
}

