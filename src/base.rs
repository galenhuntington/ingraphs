/**
    Representation of an undirected graph.
*/

use crate::perm::*;
use core::ops::Index;
use std::fmt;

type Pair = (usize, usize);
// can be u64 if graphs max size is 11
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

impl Index<usize> for BitVec {
    type Output = bool;
    fn index(&self, i: usize) -> &bool {
        // assert!(i < 64, "Index out of bounds");
        if self.0 & (1 << i) != 0 { &true } else { &false }
    }
}

impl BitVec {
    pub fn new() -> Self { BitVec(0) }
    pub fn set(&mut self, i: usize) { self.0 |= 1 << i }
    pub fn unset(&mut self, i: usize) { self.0 &= !(1 << i) }
    pub fn get(&self, i: usize) -> bool { self.0 & (1 << i) != 0 }
}

impl Bits for BitVec {
    fn bits(&self) -> BitNum { self.0 }
    fn from_bits(_size: usize, bits: BitNum) -> Self { BitVec(bits) }
}


#[derive(PartialEq, Eq, Debug, Clone, Copy, Ord, PartialOrd)]
pub struct Triangle (pub BitVec);

#[derive(Debug, PartialEq, Eq, Clone, PartialOrd, Ord)]
pub struct Graph { pub size: usize, pub edges: Triangle }

impl Triangle {
    pub fn empty(_sz: usize) -> Self { Triangle(BitVec(0)) }
    pub fn get(&self, (a, b): Pair) -> bool { self.0.get(index(a, b)) }
    pub fn set(&mut self, (a, b): Pair) { self.0.set(index(a, b)) }
    pub fn unset(&mut self, (a, b): Pair) { self.0.unset(index(a, b)) }
}

impl Index<Pair> for Triangle {
    type Output = bool;
    fn index(&self, (a, b): Pair) -> &bool { &self.0[index(a, b)] }
}

impl Bits for Triangle {
    fn bits(&self) -> BitNum { self.0.bits() }
    fn from_bits(_size: usize, bits: BitNum) -> Self { Triangle(BitVec(bits)) }
}

/*
impl IntoIterator for Triangle {
    type Item = bool;
    type IntoIter = std::vec::IntoIter<bool>;
    fn into_iter(self) -> Self::IntoIter { self.0.into_iter() }
}
*/

#[inline]
pub fn raw_index(a: usize, b: usize) -> usize { b*(b-1) / 2 + a }

/*
#[inline]
fn mn(x: usize, y: usize) -> Pair { if x < y { (x, y) } else { (y, x) } }
*/

#[inline]
pub fn index(a: usize, b: usize) -> usize {
    if a < b { raw_index(a, b) } else { raw_index(b, a) }
}

#[inline]
pub fn rev_hi_index(i: usize) -> usize {
    (((8*i + 1) as f64).sqrt() as usize + 1) / 2
}

pub fn rev_index(i: usize) -> Pair {
    let b = rev_hi_index(i);
    let a = i - b*(b-1)/2;
    (a, b)
}

const EDGE_VECS: [BitNum; 16] = [
    0x20008004004008020101020844b_u128 as BitNum,
    0x400100080080100402020410895_u128 as BitNum,
    0x800200100100200804040821126_u128 as BitNum,
    0x1000400200200401008081042238_u128 as BitNum,
    0x20008004004008020101020843c0_u128 as BitNum,
    0x4001000800801004020204107c00_u128 as BitNum,
    0x80020010010020080404081f8000_u128 as BitNum,
    0x1000400200200401008080fe00000_u128 as BitNum,
    0x20008004004008020100ff0000000_u128 as BitNum,
    0x400100080080100401ff000000000_u128 as BitNum,
    0x8002001001002007fe00000000000_u128 as BitNum,
    0x10004002002003ff80000000000000_u128 as BitNum,
    0x20008004003ffc0000000000000000_u128 as BitNum,
    0x40010007ffc0000000000000000000_u128 as BitNum,
    0x8001fff80000000000000000000000_u128 as BitNum,
    0xfffe00000000000000000000000000_u128 as BitNum,
];

impl Graph {
    pub fn new(size: usize, edges: Triangle ) -> Self {
        // assert_eq!(edges.0.len(), Graph::triangle(size), "Invalid graph size!");
        Graph { size, edges }
    }
    pub fn has_edge(&self, a: usize, b: usize) -> bool {
        a != b && self.edges[(a, b)]
    }
    // assume a < b
    pub fn has_edge_raw(&self, a: usize, b: usize) -> bool {
        self.edges[(a, b)]
    }
    pub fn triangle(sz: usize) -> usize { sz*(sz-1)/2 }
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
        write!(f, "[{}]", v.as_slice().join(" "))
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
            let len = rng.gen_range(2..11);
            let p = Perm::random(rng, len);
            if p.is_valid() {
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
    }
    #[test]
    fn test_fast_degree() {
        let rng = &mut rand::thread_rng();
        for _ in 0 .. 100_000 {
            let sz = rng.gen_range(1..=11);
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
    /*
    #[test]
    fn test_output() {
        // used to generate the EDGE_VECS
        for i in 0..16 {
            let mut gr = Graph::from_bits(i, 0);
            for j in 0..16 {
                if i != j { gr.edges.set((i, j)) }
            }
            println!("0x{:x},", gr.edges.0.0);
        }
    }
    */
}

