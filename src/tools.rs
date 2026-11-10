/*!
    Permutations and basic operations on them.

    This is extracted from a larger module in another project.
*/

use crate::base::{BitNum,Graph,Bits,rev_hi_index,MAX_SIZE,ZERO,bit};
use crate::perm::{Perm,all_perms};
use crate::enumerate;
use std::time::SystemTime;
use utc_dt::UTCDatetime;
use std::collections::{BTreeMap,BTreeSet};

#[inline]
pub fn factorial(n: usize) -> u128 {
    (1..=n).try_fold(1u128, |product, k|
        product.checked_mul(u128::try_from(k).unwrap())).expect("factorial overflow")
}

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
    assert!(n <= MAX_SIZE, "graph6 too large (or multi-byte size): {}", line);
    let tri = Graph::triangle(n);
    assert_eq!(bytes.len(), 1 + tri.div_ceil(6), "Bad graph6 line: {}", line);
    let mut edges: BitNum = ZERO;
    for k in 0..tri {
        let c = BitNum::from(bytes[1 + k / 6] - 63);
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
    let mut best: BitNum = !ZERO;
    for perm in all_perms(gr.size) {
        let g = gr.unrenumber(&perm);
        best = best.min(g.bits());
    }
    Graph::from_bits(gr.size, best)
}

// Backtracking count of color- and adjacency-preserving bijections
struct AutCount<'a> {
    verts: &'a [usize],
    adj: &'a [u32; MAX_SIZE],
    color: &'a [usize],
    img: [usize; MAX_SIZE],
    used: u32,
}

impl AutCount<'_> {
    fn go(&mut self, i: usize) -> u128 {
        if i == self.verts.len() { return 1 }
        let v = self.verts[i];
        let mut total = 0u128;
        for k in 0..self.verts.len() {
            let c = self.verts[k];
            if self.used & (1 << c) != 0 { continue }
            if self.color[c] != self.color[v] { continue }
            let ok = self.verts[..i].iter().all(|&w|
                self.adj[v] >> w & 1 == self.adj[c] >> self.img[w] & 1);
            if ok {
                self.img[v] = c;
                self.used |= 1 << c;
                total = total.checked_add(self.go(i + 1)).expect("automorphism count overflow");
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
pub fn count_symmetries(gr: &Graph) -> u128 {
    assert!(gr.size <= MAX_SIZE, "unsupported graph order");
    if gr.size <= 2 { return factorial(gr.size) }
    let n = gr.size;
    let adj = adj_masks(gr);
    let mut alive: u32 = (1 << n) - 1;
    let mut color = vec![0usize; n];
    let mut next_color = 1;
    let mut mult: u128 = 1;

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
                mult = mult.checked_mul(factorial(vs.len())).expect("automorphism count overflow");
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

    let mut quot_adj = [0u32; MAX_SIZE];
    for &v in &verts { quot_adj[v] = adj[v] & alive }
    let mut ac = AutCount {
        verts: &verts,
        adj: &quot_adj,
        color: &color,
        img: [usize::MAX; MAX_SIZE],
        used: 0,
    };
    mult.checked_mul(ac.go(0)).expect("automorphism count overflow")
}

// Kept as compatibility entry points. Bulk callers should retain the prepared
// matcher objects rather than repeatedly supplying only a degree/order row.
pub use crate::matcher::IIResult;
use crate::matcher::{PreparedColours, PreparedHost, PreparedPattern};

// Also used by the automorphism counter above.
fn adj_masks(gr: &Graph) -> [u32; MAX_SIZE] {
    let mut adj = [0u32; MAX_SIZE];
    let mut bits = gr.bits();
    while bits != 0 {
        let (a, b) = crate::base::rev_index(bits.trailing_zeros() as usize);
        bits &= bits - 1;
        adj[a] |= 1 << b;
        adj[b] |= 1 << a;
    }
    adj
}

pub fn isso_inner<T: IIResult>(sub: &Graph, sub_sorted: &[(usize, usize)], sup: &Graph) -> T {
    PreparedPattern::with_row(sub, sub_sorted).search(&PreparedHost::new(sup))
}

pub fn build_sorted_row(gr: &Graph) -> Vec<(usize, usize)> {
    crate::matcher::sorted_row(gr)
}

pub fn ingraph_check(sup: &Graph, sub_sorted: &[(usize, usize)], sub: &Graph) -> bool {
    PreparedColours::new(sup).covers(&PreparedPattern::with_row(sub, sub_sorted))
}

pub fn noncovers<B: Bits + Copy, V: Iterator<Item=B>>(sups: V, sub: &Graph)
        -> impl Iterator<Item=B> {
    let pattern = PreparedPattern::new(sub);
    let min_edges = Graph::triangle(sub.size).div_ceil(2);
    sups.filter(
        move |sup| sup.bits().count_ones() as usize >= min_edges
            && !PreparedColours::new(&Graph::from_bits(sub.size, sup.bits())).covers(&pattern))
}

pub fn is_subgraph_of(sub: &Graph, sup: &Graph) -> bool {
    PreparedPattern::new(sub).is_subgraph_of(&PreparedHost::new(sup))
}

pub fn find_subgraph_ss(sub: &Graph, sub_sorted: &[(usize, usize)], sup: &Graph) -> Option<Graph> {
    isso_inner::<Option<Perm>>(sub, sub_sorted, sup).map(|p| sub.unrenumber(&p))
}

pub fn find_subgraph_of(sub: &Graph, sup: &Graph) -> Option<Graph> {
    PreparedPattern::new(sub).find_subgraph(&PreparedHost::new(sup))
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
    for edge in 0 .. Graph::triangle(gr.size) {
        let mask = bit(edge);
        let val = if extend { base | mask } else { base & !mask };
        let gr = Graph::from_bits(gr.size, val);
        if base == val { continue }
        let gr = enumerate::to_best(&gr);
        if seen.contains(&gr.bits()) { continue }
        seen.insert(gr.bits());
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
            assert_eq!(count_symmetries(&gr) as usize, count_symmetries_slow(&gr), "{}", gr);
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
                assert_eq!(count_symmetries(&gr) as usize, count_symmetries_slow(&gr), "{}", gr);
            }
        }
    }
}
