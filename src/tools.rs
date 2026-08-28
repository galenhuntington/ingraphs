/*!
    Permutations and basic operations on them.

    This is extracted from a larger module in another project.
*/

use crate::base::{BitNum,Graph,Bits,rev_hi_index};
use crate::perm::{Perm,all_perms};
use crate::enumerate;
use std::cmp::Reverse;
use std::time::SystemTime;
use utc_dt::UTCDatetime;
use std::collections::{BTreeMap,BTreeSet};

#[inline]
pub fn factorial(n: usize) -> usize {
    (1..=n).product()
}

#[inline]
pub fn one_bits(ct: usize) -> BitNum { (1 << ct) - 1 }

pub fn timestamp() -> String {
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    let ts = UTCDatetime::from(now);
    ts.as_iso_datetime(6).to_string()
}

pub fn degree_row(gr: &Graph) -> Vec<usize> {
    (0..gr.size).map(|i| gr.degree_of(i)).collect()
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
    if edges == 0 { return 1 }
    rev_hi_index(hi_bit_ix(edges)) + 1
}

pub fn infer_graph(edges: BitNum) -> Graph {
    Graph::from_bits(infer_size(edges), edges)
}

// Decode nauty's graph6 format; the pair ordering matches ours,
// but bits are packed high-to-low within 6-bit chars
pub fn parse_graph6(line: &str) -> BitNum {
    let bytes = line.as_bytes();
    let n = (bytes[0] - 63) as usize;
    assert!(n <= 16, "graph6 too large (or multi-byte size): {}", line);
    let tri = Graph::triangle(n);
    assert_eq!(bytes.len(), 1 + tri.div_ceil(6), "Bad graph6 line: {}", line);
    let mut edges: BitNum = 0;
    for k in 0..tri {
        let c = (bytes[1 + k / 6] - 63) as BitNum;
        edges |= ((c >> (5 - k % 6)) & 1) << k;
    }
    edges
}

pub fn read_graphs<B: Bits>(sz: usize, path: &str) -> impl Iterator<Item=B> {
    use std::fs::File;
    use std::io::{BufReader,BufRead};
    let file = File::open(path)
        .unwrap_or_else(|e| panic!("Error reading {}: {}", path, e));
    let reader = BufReader::new(file);
    reader.lines().map(move |line| {
        let line = line.unwrap();
        let line = line.split_once(',').map_or(line.as_str(), |x| x.0);
        let edges = if line.as_bytes().first().is_some_and(|b| b.is_ascii_digit()) {
            line.parse::<BitNum>().unwrap()
        } else {
            parse_graph6(line)
        };
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

// Backtracking count of color- and adjacency-preserving bijections
struct AutCount<'a> {
    verts: &'a [usize],
    adj: &'a [u32; 16],
    color: &'a [usize],
    img: [usize; 16],
    used: u32,
}

impl AutCount<'_> {
    fn go(&mut self, i: usize) -> usize {
        if i == self.verts.len() { return 1 }
        let v = self.verts[i];
        let mut total = 0;
        for k in 0..self.verts.len() {
            let c = self.verts[k];
            if self.used & (1 << c) != 0 { continue }
            if self.color[c] != self.color[v] { continue }
            let ok = self.verts[..i].iter().all(|&w|
                self.adj[v] >> w & 1 == self.adj[c] >> self.img[w] & 1);
            if ok {
                self.img[v] = c;
                self.used |= 1 << c;
                total += self.go(i + 1);
                self.used &= !(1 << c);
            }
        }
        total
    }
}

// |Aut| via twin collapsing: vertices with identical neighborhoods (twin
// classes) contribute a full symmetric group, exactly; collapse them to one
// colored representative and count the (small) quotient's automorphisms by
// refinement plus backtracking.
pub fn count_symmetries(gr: &Graph) -> usize {
    if gr.size <= 2 { return gr.size }
    let n = gr.size;
    let adj = adj_masks(gr);
    let mut alive: u32 = (1 << n) - 1;
    let mut color = vec![0usize; n];
    let mut next_color = 1;
    let mut mult: usize = 1;

    // collapse twin classes (open: same neighborhood; closed: same closed
    // neighborhood) until none remain; classes of equal prior color, size,
    // and type stay interchangeable, so they share a fresh color
    loop {
        let mut changed = false;
        'types: for closed in [false, true] {
            let mut groups: BTreeMap<(usize, u32), Vec<usize>> = BTreeMap::new();
            for v in 0..n {
                if alive & (1 << v) == 0 { continue }
                let mut key = adj[v] & alive;
                if closed { key |= 1 << v }
                groups.entry((color[v], key)).or_default().push(v);
            }
            let mut sigs: BTreeMap<(usize, usize), usize> = BTreeMap::new();
            for ((c, _), vs) in groups {
                if vs.len() < 2 { continue }
                mult *= factorial(vs.len());
                for &v in &vs[1..] { alive &= !(1 << v) }
                let nc = *sigs.entry((c, vs.len())).or_insert_with(|| {
                    next_color += 1;
                    next_color - 1
                });
                color[vs[0]] = nc;
                changed = true;
            }
            if changed { break 'types }
        }
        if !changed { break }
    }

    // color refinement (1-WL) on the quotient
    let vs: Vec<usize> = (0..n).filter(|v| alive & (1 << *v) != 0).collect();
    loop {
        let before: BTreeSet<_> = vs.iter().map(|&v| color[v]).collect();
        let mut sigs: BTreeMap<(usize, Vec<usize>), Vec<usize>> = BTreeMap::new();
        for &v in &vs {
            let mut ns: Vec<usize> = vs.iter()
                .filter(|&&u| adj[v] >> u & 1 == 1)
                .map(|&u| color[u]).collect();
            ns.sort();
            sigs.entry((color[v], ns)).or_default().push(v);
        }
        if sigs.len() == before.len() { break }
        let base = next_color;
        for (i, (_, cl)) in sigs.into_iter().enumerate() {
            for v in cl { color[v] = base + i }
        }
        next_color = base + n;
    }

    // most-constrained-first: smaller color classes early
    let mut class_size = BTreeMap::new();
    for &v in &vs { *class_size.entry(color[v]).or_insert(0usize) += 1 }
    let mut verts = vs;
    verts.sort_by_key(|&v| (class_size[&color[v]], color[v], v));

    let mut quot_adj = [0u32; 16];
    for &v in &verts { quot_adj[v] = adj[v] & alive }
    let mut ac = AutCount {
        verts: &verts,
        adj: &quot_adj,
        color: &color,
        img: [usize::MAX; 16],
        used: 0,
    };
    mult * ac.go(0)
}

const UNFILLED: usize = 0xfffff;

pub trait IIResult {
    fn from_perm(perm: Perm) -> Self;
    fn failure() -> Self;
}

impl IIResult for bool {
    fn from_perm(_perm: Perm) -> bool { true }
    fn failure() -> bool { false }
}

impl IIResult for Option<Perm> {
    fn from_perm(perm: Perm) -> Self { Some(perm) }
    fn failure() -> Self { None }
}

// Neighbor masks, indexed by vertex
fn adj_masks(gr: &Graph) -> [u32; 16] {
    let mut adj = [0u32; 16];
    let mut bits = gr.bits();
    while bits != 0 {
        let i = bits.trailing_zeros() as usize;
        bits &= bits - 1;
        let (a, b) = crate::base::rev_index(i);
        adj[a] |= 1 << b;
        adj[b] |= 1 << a;
    }
    adj
}

struct Isso<'a> {
    sub_sorted: &'a [(usize, usize)],
    sub_adj: [u32; 16],
    sup_adj: [u32; 16],
    sup_deg: [usize; 16],
    // sup slots in descending-degree order, tried in this order
    sup_order: [usize; 16],
    // for each sub vertex, the sup slots its already-placed neighbors occupy
    req: [u32; 16],
    // sup slot -> sub vertex
    vec: [usize; 16],
    // sub vertex -> its sup slot (valid once assigned)
    img: [usize; 16],
    // interchangeable predecessor in matching order, else UNFILLED; el's
    // image is forced above its predecessor's, killing twin permutations
    twin_pred: [usize; 16],
    // interchangeable earlier (in sup_order) sup slot: while it is unused,
    // this slot is redundant to try
    sup_twin_pred: [usize; 16],
    used: u32,
    size: usize,
}

impl Isso<'_> {
    fn go(&mut self, i: usize) -> bool {
        let (el_deg, el) = self.sub_sorted[i];
        if el_deg == 0 {
            // the rest are isolated; drop them into free slots
            let mut j = 0;
            for k in i..self.size {
                while self.used & (1 << j) != 0 { j += 1 }
                self.vec[j] = self.sub_sorted[k].1;
                self.used |= 1 << j;
            }
            return true;
        }
        let req_el = self.req[el];
        // symmetry breaking: only slots above an interchangeable predecessor's
        let above = match self.twin_pred[el] {
            UNFILLED => 0,
            p => self.img[p] + 1,
        };
        for jx in 0..self.size {
            let j = self.sup_order[jx];
            if j < above { continue }
            let jb = 1u32 << j;
            if self.used & jb != 0 { continue }
            match self.sup_twin_pred[j] {
                UNFILLED => {},
                p => if self.used & (1 << p) == 0 { continue },
            }
            if el_deg > self.sup_deg[j] { continue }
            // every placed neighbor of el must sit on a sup neighbor of j
            if req_el & !self.sup_adj[j] != 0 { continue }
            self.vec[j] = el;
            self.img[el] = j;
            self.used |= jb;
            let mut nbrs = self.sub_adj[el];
            while nbrs != 0 {
                let u = nbrs.trailing_zeros() as usize;
                nbrs &= nbrs - 1;
                self.req[u] |= jb;
            }
            if i == self.size - 1 || self.go(i + 1) { return true }
            let mut nbrs = self.sub_adj[el];
            while nbrs != 0 {
                let u = nbrs.trailing_zeros() as usize;
                nbrs &= nbrs - 1;
                self.req[u] &= !jb;
            }
            self.used &= !jb;
            self.vec[j] = UNFILLED;
        }
        false
    }
}

pub fn isso_inner<T: IIResult>(sub: &Graph, sub_sorted: &[(usize, usize)], sup: &Graph) -> T {
    let size = sub.size;
    let sup_adj = adj_masks(sup);
    let mut sup_deg = [0; 16];
    for j in 0..size { sup_deg[j] = sup_adj[j].count_ones() as usize }
    let mut sup_order = [0; 16];
    for j in 0..size { sup_order[j] = j }
    sup_order[..size].sort_unstable_by_key(|&j| Reverse(sup_deg[j]));
    let sub_adj = adj_masks(sub);
    // chain interchangeable vertices (swapping them is an automorphism)
    // in matching order; correcting swaps always terminate, so demanding
    // ascending images along each chain loses no embeddings
    let mut twin_pred = [UNFILLED; 16];
    for i in 1..size {
        let el = sub_sorted[i].1;
        for k in (0..i).rev() {
            let u = sub_sorted[k].1;
            let both = !((1u32 << el) | (1 << u));
            if sub_adj[el] & both == sub_adj[u] & both {
                twin_pred[el] = u;
                break;
            }
        }
    }
    let mut sup_twin_pred = [UNFILLED; 16];
    for jx in 1..size {
        let j = sup_order[jx];
        for kx in (0..jx).rev() {
            let u = sup_order[kx];
            // order is degree-sorted and twins share a degree
            if sup_deg[u] != sup_deg[j] { break }
            let both = !((1u32 << j) | (1 << u));
            if sup_adj[j] & both == sup_adj[u] & both {
                sup_twin_pred[j] = u;
                break;
            }
        }
    }
    let mut st = Isso {
        sub_sorted,
        sub_adj,
        sup_adj,
        sup_deg,
        sup_order,
        req: [0; 16],
        vec: [UNFILLED; 16],
        img: [UNFILLED; 16],
        twin_pred,
        sup_twin_pred,
        used: 0,
        size,
    };
    if st.go(0) {
        T::from_perm(Perm::new_unchecked(st.vec[..size].to_vec()))
    } else {
        T::failure()
    }
}

// Matching order for isso_inner: greedy connected order (most already-placed
// neighbors first, then highest degree) so each placement is constrained
// early.  Isolated vertices necessarily land at the end.
// Matching order for isso_inner: connected components densest first (an
// unembeddable dense component then fails before the search wastes time
// embedding easy sparse ones), greedy connected order within a component.
// Isolated vertices have density 0 and land at the end, as isso requires.
pub fn build_sorted_row(gr: &Graph) -> Vec<(usize, usize)> {
    let mut adj = adj_masks(gr);
    // edges above the stated size are ignored (legacy truncation behavior)
    let lim = (1u32 << gr.size) - 1;
    for m in adj.iter_mut() { *m &= lim }
    let mut seen = 0u32;
    let mut comps: Vec<(u32, u32, u32)> = Vec::new();  // (mask, edges2, size)
    for v in 0..gr.size {
        if seen & (1 << v) != 0 { continue }
        let mut mask = 1u32 << v;
        loop {
            let grown = comps_grow(&adj, mask, gr.size);
            if grown == mask { break }
            mask = grown;
        }
        seen |= mask;
        let edges2: u32 = (0..gr.size)
            .filter(|u| mask & (1 << u) != 0)
            .map(|u| adj[u].count_ones())
            .sum();
        comps.push((mask, edges2, mask.count_ones()));
    }
    // densest first: e1/v1 > e2/v2  ⟺  e1*v2 > e2*v1; ties by size ascending
    comps.sort_by(|a, b|
        (b.1 * a.2).cmp(&(a.1 * b.2)).then(a.2.cmp(&b.2)));
    let mut row = Vec::with_capacity(gr.size);
    for (mask, _, _) in comps {
        let mut placed = 0u32;
        for _ in 0..mask.count_ones() {
            let best = (0..gr.size)
                .filter(|v| mask & (1 << v) != 0 && placed & (1 << v) == 0)
                .max_by_key(|&v| (
                    (adj[v] & placed).count_ones(),
                    adj[v].count_ones(),
                    Reverse(v),
                ))
                .unwrap();
            row.push((adj[best].count_ones() as usize, best));
            placed |= 1 << best;
        }
    }
    row
}

fn comps_grow(adj: &[u32; 16], mask: u32, size: usize) -> u32 {
    let mut out = mask;
    for u in 0..size {
        if mask & (1 << u) != 0 { out |= adj[u] }
    }
    out
}

pub fn ingraph_check(sup: &Graph, sub_sorted: &[(usize, usize)], sub: &Graph) -> bool {
    let isup = sup.complement();
    isso_inner(sub, sub_sorted, &isup) || isso_inner(sub, sub_sorted, sup)
}

pub fn noncovers<B: Bits + Copy, V: Iterator<Item=B>>(sups: V, sub: &Graph)
        -> impl Iterator<Item=B> {
    let sub_sorted = build_sorted_row(sub);
    let min_edges = Graph::triangle(sub.size).div_ceil(2);
    sups.filter(
        move |sup| sup.bits().count_ones() as usize >= min_edges
            && !ingraph_check(&Graph::from_bits(sub.size, sup.bits()), &sub_sorted, sub))
}

pub fn is_subgraph_of(sub: &Graph, sup: &Graph) -> bool {
    let sub_sorted = build_sorted_row(sub);
    isso_inner(sub, &sub_sorted, sup)
}

pub fn find_subgraph_ss(sub: &Graph, sub_sorted: &[(usize, usize)], sup: &Graph) -> Option<Graph> {
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
        for _ in 0 .. 1000 {
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
    #[test]
    fn test_count_symmetries() {
        let rng = &mut rand::thread_rng();
        for _ in 0 .. 300 {
            let size = rng.gen_range(2..=9);
            let gr = random_graph(rng, size);
            assert_eq!(count_symmetries(&gr), count_symmetries_slow(&gr), "{}", gr);
        }
    }
    #[test]
    fn test_count_symmetries_symmetric() {
        // twin-heavy and structured families
        for size in 3..=9 {
            let mut grs = vec![
                Graph::from_fn(size, |_, _| true),                    // complete
                Graph::from_fn(size, |_, _| false),                   // empty
                Graph::from_fn(size, |a, b| a == 0 || b == 0),        // star
                Graph::from_fn(size, |a, b| (a < size/2) != (b < size/2)),  // bipartite
                Graph::from_fn(size, |a, b| (a < size/2) == (b < size/2)),  // cliques
                Graph::from_fn(size, |a, b| a + 1 == b || b + 1 == a),      // path
                Graph::from_fn(size, |a, b| (a + 1) % size == b || (b + 1) % size == a), // cycle
                Graph::from_fn(size, |a, b| a % 3 != b % 3),          // tripartite
            ];
            for k in 0..3 {
                grs.push(Graph::from_fn(size, |a, b| (a + b) % 3 == k));
            }
            for gr in grs {
                assert_eq!(count_symmetries(&gr), count_symmetries_slow(&gr), "{}", gr);
            }
        }
    }
}

