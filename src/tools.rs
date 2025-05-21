/**
    Permutations and basic operations on them.

    This is extracted from a larger module in another project.
*/

use crate::base::{BitNum,Graph,Bits,rev_hi_index};
use crate::perm::{Perm,all_perms};
use crate::enumerate;
use std::time::SystemTime;
use utc_dt::UTCDatetime;
use fix_fn::fix_fn;
use std::collections::BTreeSet;

#[inline]
pub fn factorial(n: usize) -> usize {
    (1..=n).product()
}

#[inline]
pub fn one_bits(ct: usize) -> BitNum { (1 << ct) - 1 }

pub fn timestamp() -> String {
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    let ts = UTCDatetime::from(now);
    format!("{}", ts.as_iso_datetime(6))
}

pub fn degree_row(gr: &Graph) -> Vec<usize> {
    let mut row = Vec::with_capacity(gr.size);
    for i in 0..gr.size {
        row.push(gr.degree_of(i));
    }
    row
}

pub fn sorted_degree_row(gr: &Graph) -> Vec<usize> {
    let mut row = degree_row(gr);
    row.sort();
    row
}

#[inline]
pub fn hi_bit_ix(n: BitNum) -> usize {
    ((BitNum::BITS - 1) - n.leading_zeros()) as usize
}

pub fn infer_size(edges: BitNum) -> usize {
    rev_hi_index(hi_bit_ix(edges)) + 1
}

pub fn infer_graph(edges: BitNum) -> Graph {
    Graph::from_bits(infer_size(edges), edges)
}

pub fn read_graphs<B: Bits>(sz: usize, path: &str) -> impl Iterator<Item=B> {
    use std::fs::File;
    use std::io::{BufReader,BufRead};
    let file = File::open(path).unwrap();
    let reader = BufReader::new(file);
    reader.lines().map(move |line| {
        let line = line.unwrap();
        let line = line.split_once(',').map_or(line.as_str(), |x| x.0);
        let edges = line.parse::<BitNum>().unwrap();
        B::from_bits(sz, edges)
    })
}

pub fn count_symmetries_slow(gr: &Graph) -> usize {
    let mut count = 0;
    for perm in all_perms(gr.size) {
        let g = gr.unrenumber(&perm);
        if g.edges == gr.edges { count += 1 }
    }
    count
}

pub fn naive_find_best(gr: &Graph) -> Graph {
    let mut best: BitNum = !0;
    for perm in all_perms(gr.size) {
        let g = gr.unrenumber(&perm);
        best = best.min(g.bits());
    }
    Graph::from_bits(gr.size, best)
}

pub fn count_symmetries(gr: &Graph) -> usize {
    // Partition by degree count
    let mut degs: Vec<Vec<usize>> = vec![Vec::new(); gr.size];
    for pt in 0..gr.size { degs[gr.degree_of(pt)].push(pt); }
    let degs = degs;
    fn go(gr: &Graph, degs: &Vec<Vec<usize>>, deg: usize, perm: &Perm) -> usize {
        let vec = &degs[deg];
        all_perms(vec.len()).map(|p| {
            let mut pn = Perm::identity(gr.size);
            for (i, &pt) in vec.iter().enumerate() {
                pn.vec[pt] = vec[p.vec[i]];
            }
            let p2 = perm * pn;
            if deg >= gr.size - 2 {
                let g = gr.unrenumber(&p2);
                if g.edges == gr.edges { 1 } else { 0 }
            } else {
                go(gr, degs, deg + 1, &p2)
            }
        }).sum()
    }
    go(&gr, &degs, 1, &Perm::identity(gr.size))
        * factorial(degs[0].len()) * factorial(degs[gr.size - 1].len())
}

const UNFILLED: usize = 0xfffff;

pub trait IIResult {
    fn from_perm(perm: &Perm) -> Self;
    fn failure() -> Self;
}

impl IIResult for bool {
    fn from_perm(_perm: &Perm) -> bool { true }
    fn failure() -> bool { false }
}

impl IIResult for Option<Perm> {
    fn from_perm(perm: &Perm) -> Self { Some(perm.clone()) }
    fn failure() -> Self { None }
}

pub fn isso_inner<T: IIResult>(sub: &Graph, sub_sorted: &Vec<(usize, usize)>, sup: &Graph) -> T {
    let size = sub.size;
    let sup_row = degree_row(sup);
    let mut perm = Perm::new_unsafe(vec![UNFILLED; size]);
    let go = fix_fn!(|go, perm: &mut Perm, i: usize| -> bool {
        let (el_deg, el) = sub_sorted[i];
        'outer: for j in 0..size {
            if perm.vec[j] != UNFILLED { continue }
            if el_deg > sup_row[j] { continue }
            if i > 0 {
                for k in 0..size {
                    let v = perm.vec[k];
                    if v != UNFILLED
                            && sub.has_edge(el, v)
                            && !sup.has_edge(j, k) {
                        continue 'outer
                    }
                }
            }
            perm.vec[j] = el;
            if i == size - 1 || go(perm, i + 1) { return true }
            perm.vec[j] = UNFILLED;
        }
        false
    });
    if go(&mut perm, 0) { T::from_perm(&perm) } else { T::failure() }
}

pub fn build_sorted_row(gr: &Graph) -> Vec<(usize, usize)> {
    let mut row = Vec::with_capacity(gr.size);
    for i in 0..gr.size {
        row.push((gr.degree_of(i), i));
    }
    row.sort();
    row.reverse();
    row
}

pub fn ingraph_check(sup: &Graph, sub_sorted: &Vec<(usize, usize)>, sub: &Graph) -> bool {
    let isup = sup.complement();
    return isso_inner(sub, sub_sorted, &isup) || isso_inner(sub, sub_sorted, sup);
}

pub fn noncovers<B: Bits + Copy, V: Iterator<Item=B>>(sups: V, sub: &Graph)
        -> impl Iterator<Item=B> + use<'_, B, V> {
    let sub_sorted = build_sorted_row(sub);
    let min_edges = (Graph::triangle(sub.size) + 1) / 2;
    sups.filter(
        move |sup| sup.bits().count_ones() as usize >= min_edges
            && !ingraph_check(&Graph::from_bits(sub.size, sup.bits()), &sub_sorted, sub))
}

pub fn is_subgraph_of(sub: &Graph, sup: &Graph) -> bool {
    let sub_sorted = build_sorted_row(sub);
    return isso_inner(sub, &sub_sorted, sup);
}

pub fn find_subgraph_ss(sub: &Graph, sub_sorted: &Vec<(usize, usize)>, sup: &Graph) -> Option<Graph> {
    isso_inner::<Option<Perm>>(sub, sub_sorted, sup).map(|p| sub.unrenumber(&p))
}

pub fn find_subgraph_of(sub: &Graph, sup: &Graph) -> Option<Graph> {
    find_subgraph_ss(sub, &build_sorted_row(sub), sup)
}

pub fn naive_is_subgraph_of(sub: &Graph, sup: &Graph) -> bool {
    for perm in all_perms(sup.size) {
        let sub1 = sub.unrenumber(&perm);
        if sub1.bits() & !sup.bits() == 0 { return true; }
    }
    false
}

pub fn bump(gr: &Graph, extend: bool) -> BTreeSet<BitNum> {
    let mut seen = BTreeSet::new();
    let base = gr.bits();
    for bit in 0 .. Graph::triangle(gr.size) {
        // let val = base & !(1 << bit);
        let val = if extend { base | (1 << bit) } else { base & !(1 << bit) };
        let gr = Graph::from_bits(gr.size, val);
        if base == val { continue }
        let gr = enumerate::to_best(&gr);
        if seen.contains(&gr.bits()) { continue }
        seen.insert(gr.bits());
        // eprintln!("{base} {bit} -> {}: {} ({})", gr.bits(), gr, enumerate::is_best(&gr));
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;
    use crate::base::random_graph;
    #[test]
    fn test_subgraph() {
        let rng = &mut rand::thread_rng();
        for _ in 0 .. 2000 {
            let size = rng.gen_range(1..=9);
            let sup = random_graph(rng, size);
            let mut sub = random_graph(rng, size);
            let sub2 = random_graph(rng, size);
            sub.edges.0.0 &= sub2.edges.0.0;
            let sub_s = find_subgraph_of(&sub, &sup);
            assert_eq!(
                sub_s.is_some(),
                naive_is_subgraph_of(&sub, &sup),
                "size = {}, sub = {}, sup = {}",
                size, sub.bits(), sup.bits());
            if let Some(sub_s) = sub_s {
                assert_eq!(sub.bits().count_ones(), sub_s.bits().count_ones());
                assert_eq!(sub_s.bits() & !sup.bits(), 0, 
                    "size = {}, sub = {}, sup = {}, sub_s = {}",
                    size, sub.bits(), sup.bits(), sub_s.bits());
            }
        }
    }
}

