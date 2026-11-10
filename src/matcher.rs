//! Exact, non-induced subgraph matching for graphs of the same order.
//!
//! Pad a smaller pattern with isolated vertices before preparing it. Prepared
//! objects own an immutable graph and may be reused/shared between threads;
//! only the small backtracking state is allocated anew for each query.

use crate::base::{Bits, Graph, MAX_SIZE, one_bits};
use crate::perm::Perm;
use std::cmp::Reverse;

const ABSENT: u8 = u8::MAX;
const UNFILLED: usize = usize::MAX;

pub trait IIResult: Sized {
    fn from_perm(perm: Perm) -> Self;
    fn failure() -> Self;
    fn from_mapping(mapping: &[usize]) -> Self {
        Self::from_perm(Perm::new_unchecked(mapping.to_vec()))
    }
}

impl IIResult for bool {
    fn from_perm(_perm: Perm) -> Self {
        true
    }
    fn failure() -> Self {
        false
    }
    // Existence queries need no allocated permutation/witness.
    fn from_mapping(_mapping: &[usize]) -> Self {
        true
    }
}

impl IIResult for Option<Perm> {
    fn from_perm(perm: Perm) -> Self {
        Some(perm)
    }
    fn failure() -> Self {
        None
    }
}

#[derive(Clone, Debug)]
struct Components {
    // Masks in discovery order; sizes sorted largest first for packing.
    masks: [u32; MAX_SIZE],
    sizes: [u8; MAX_SIZE],
    len: usize,
}

impl Components {
    fn new(adj: &[u32; MAX_SIZE], size: usize) -> Self {
        let mut result = Self {
            masks: [0; MAX_SIZE],
            sizes: [0; MAX_SIZE],
            len: 0,
        };
        let mut remaining = (1u32 << size) - 1;
        while remaining != 0 {
            let mut frontier = 1u32 << remaining.trailing_zeros();
            let mut component = frontier;
            while frontier != 0 {
                let v = frontier.trailing_zeros() as usize;
                frontier &= frontier - 1;
                let added = adj[v] & !component;
                component |= added;
                frontier |= added;
            }
            remaining &= !component;
            result.masks[result.len] = component;
            result.sizes[result.len] = component.count_ones() as u8;
            result.len += 1;
        }
        result.sizes[..result.len].sort_unstable_by_key(|&s| Reverse(s));
        result
    }

    fn sizes(&self) -> &[u8] {
        &self.sizes[..self.len]
    }
}

// Necessary, NOT sufficient for a subgraph embedding. Each connected pattern
// component must fit wholly inside one host component. Several may share a
// host component: host edges between their images are allowed (non-induced).
// Largest-first exact bin packing is tiny here (at most MAX_SIZE vertices).
// Singleton components can always fill remaining slots once the others fit.
fn component_sizes_fit(items: &[u8], capacities: &[u8]) -> bool {
    if items.iter().map(|&x| x as usize).sum::<usize>()
        > capacities.iter().map(|&x| x as usize).sum::<usize>()
    {
        return false;
    }
    if items.is_empty() || capacities.len() == 1 {
        return true;
    }
    if capacities.is_empty() || items[0] > capacities[0] {
        return false;
    }
    let mut bins = [0u8; MAX_SIZE];
    bins[..capacities.len()].copy_from_slice(capacities);
    pack_components(items, &mut bins[..capacities.len()])
}

fn pack_components(items: &[u8], bins: &mut [u8]) -> bool {
    let Some((&next, rest)) = items.split_first() else {
        return true;
    };
    if next == 1 {
        return true;
    }
    let mut tried = 0u32;
    for j in 0..bins.len() {
        let capacity = bins[j];
        // Bins with the same residual capacity are interchangeable for this
        // necessary check, even when the actual host components are not.
        if capacity < next || tried & (1 << capacity) != 0 {
            continue;
        }
        tried |= 1 << capacity;
        bins[j] -= next;
        let fits = pack_components(rest, bins);
        bins[j] += next;
        if fits {
            return true;
        }
    }
    false
}

#[derive(Clone, Debug)]
struct GraphData {
    graph: Graph,
    adj: [u32; MAX_SIZE],
    degree: [u8; MAX_SIZE],
    components: Components,
    edges: usize,
}

impl GraphData {
    fn new(graph: &Graph) -> Self {
        assert!(graph.size <= MAX_SIZE, "unsupported graph order");
        assert_eq!(
            graph.bits() & !one_bits(Graph::triangle(graph.size)),
            0,
            "graph has edges outside its stated order"
        );
        let mut adj = [0u32; MAX_SIZE];
        let mut bits = graph.bits();
        while bits != 0 {
            let (a, b) = crate::base::rev_index(bits.trailing_zeros() as usize);
            bits &= bits - 1;
            adj[a] |= 1 << b;
            adj[b] |= 1 << a;
        }
        Self::from_adj(*graph, adj)
    }

    fn from_adj(graph: Graph, adj: [u32; MAX_SIZE]) -> Self {
        let mut degree = [0; MAX_SIZE];
        for v in 0..graph.size {
            degree[v] = adj[v].count_ones() as u8;
        }
        let components = Components::new(&adj, graph.size);
        Self {
            graph,
            adj,
            degree,
            components,
            edges: graph.edge_count(),
        }
    }

    fn complement(&self) -> Self {
        let mut adj = [0; MAX_SIZE];
        let all = (1u32 << self.graph.size) - 1;
        for (v, mask) in adj[..self.graph.size].iter_mut().enumerate() {
            *mask = all & !((1 << v) | self.adj[v]);
        }
        Self::from_adj(self.graph.complement(), adj)
    }

    // Previous interchangeable vertex in the given matching order. Swapping
    // twins is an automorphism, so we may enforce ascending images for each
    // pattern twin chain and fill host twin classes in order.
    fn twin_predecessors(&self, order: impl Iterator<Item = usize>) -> [u8; MAX_SIZE] {
        let mut result = [ABSENT; MAX_SIZE];
        let mut earlier = [0; MAX_SIZE];
        for (i, v) in order.enumerate() {
            for &u in earlier[..i].iter().rev() {
                if self.degree[u] != self.degree[v] {
                    continue;
                }
                let both = !((1u32 << u) | (1 << v));
                if self.adj[u] & both == self.adj[v] & both {
                    result[v] = u as u8;
                    break;
                }
            }
            earlier[i] = v;
        }
        result
    }

    // Preserve the existing search order: components densest first (ties by
    // increasing size, then discovery order), greedy connected order within
    // each component. Isolates necessarily come last.
    fn sorted_row(&self) -> Vec<(usize, usize)> {
        let mut components = [(0u32, 0u32, 0u32); MAX_SIZE];
        for (i, &mask) in self.components.masks[..self.components.len]
            .iter()
            .enumerate()
        {
            let edges2 = (0..self.graph.size)
                .filter(|v| mask & (1 << v) != 0)
                .map(|v| self.degree[v] as u32)
                .sum();
            components[i] = (mask, edges2, mask.count_ones());
        }
        components[..self.components.len]
            .sort_by(|a, b| (b.1 * a.2).cmp(&(a.1 * b.2)).then(a.2.cmp(&b.2)));
        let mut row = Vec::with_capacity(self.graph.size);
        for &(mask, _, _) in &components[..self.components.len] {
            let mut placed = 0u32;
            for _ in 0..mask.count_ones() {
                let best = (0..self.graph.size)
                    .filter(|v| mask & (1 << v) != 0 && placed & (1 << v) == 0)
                    .max_by_key(|&v| {
                        (
                            (self.adj[v] & placed).count_ones(),
                            self.degree[v],
                            Reverse(v),
                        )
                    })
                    .unwrap();
                row.push((self.degree[best] as usize, best));
                placed |= 1 << best;
            }
        }
        row
    }
}

// Historical order-only API, without computing unused twin predecessors.
pub(crate) fn sorted_row(graph: &Graph) -> Vec<(usize, usize)> {
    GraphData::new(graph).sorted_row()
}

/// Reusable candidate-side graph, masks, component sizes, search order and twins.
#[derive(Clone, Debug)]
pub struct PreparedPattern {
    data: GraphData,
    row: Vec<(usize, usize)>,
    twin_pred: [u8; MAX_SIZE],
}

impl PreparedPattern {
    pub fn new(graph: &Graph) -> Self {
        let data = GraphData::new(graph);
        let row = data.sorted_row();
        Self::from_data(data, row)
    }

    // Compatibility for callers supplying the historical cached row. New
    // bulk callers should retain a PreparedPattern instead of just the row.
    pub(crate) fn with_row(graph: &Graph, row: &[(usize, usize)]) -> Self {
        let data = GraphData::new(graph);
        assert_eq!(row.len(), graph.size, "incomplete matching order");
        let mut seen = 0u32;
        let mut isolated = false;
        for &(degree, v) in row {
            assert!(
                v < graph.size && seen & (1 << v) == 0,
                "invalid matching order"
            );
            assert_eq!(degree, data.degree[v] as usize, "stale matching order");
            assert!(!isolated || degree == 0, "isolates must come last");
            isolated |= degree == 0;
            seen |= 1 << v;
        }
        Self::from_data(data, row.to_vec())
    }

    fn from_data(data: GraphData, row: Vec<(usize, usize)>) -> Self {
        let twin_pred = data.twin_predecessors(row.iter().map(|&(_, v)| v));
        Self {
            data,
            row,
            twin_pred,
        }
    }

    pub fn graph(&self) -> &Graph {
        &self.data.graph
    }
    pub fn sorted_row(&self) -> &[(usize, usize)] {
        &self.row
    }

    /// Exact existence query. No permutation/witness allocation on success.
    pub fn is_subgraph_of(&self, host: &PreparedHost) -> bool {
        self.search(host)
    }

    /// A copy using host labels, if one exists (not necessarily induced).
    pub fn find_subgraph(&self, host: &PreparedHost) -> Option<Graph> {
        self.search::<Option<Perm>>(host)
            .map(|p| self.graph().unrenumber(&p))
    }

    fn feasible(&self, host: &PreparedHost) -> bool {
        assert_eq!(
            self.graph().size,
            host.graph().size,
            "pad the pattern with isolates to the host order before preparing it"
        );
        self.data.edges <= host.data.edges
            && (host.data.components.len == 1
                || component_sizes_fit(self.data.components.sizes(), host.data.components.sizes()))
    }

    pub(crate) fn search<T: IIResult>(&self, host: &PreparedHost) -> T {
        if !self.feasible(host) {
            return T::failure();
        }
        self.search_feasible(host)
    }

    fn search_feasible<T: IIResult>(&self, host: &PreparedHost) -> T {
        let mut state = Search {
            row: &self.row,
            sub_adj: self.data.adj,
            sup_adj: host.data.adj,
            sup_degree: host.data.degree.map(usize::from),
            sup_order: host.order.map(usize::from),
            twin_pred: self.twin_pred.map(|p| {
                if p == ABSENT {
                    UNFILLED
                } else {
                    usize::from(p)
                }
            }),
            sup_twin_pred: host.twin_pred.map(|p| {
                if p == ABSENT {
                    UNFILLED
                } else {
                    usize::from(p)
                }
            }),
            size: self.graph().size,
            req: [0; MAX_SIZE],
            vec: [UNFILLED; MAX_SIZE],
            img: [UNFILLED; MAX_SIZE],
            used: 0,
        };
        if self.graph().size == 0 || state.go(0) {
            T::from_mapping(&state.vec[..self.graph().size])
        } else {
            T::failure()
        }
    }
}

/// Reusable host-side graph, masks, components, degree order and twins.
#[derive(Clone, Debug)]
pub struct PreparedHost {
    data: GraphData,
    order: [u8; MAX_SIZE],
    twin_pred: [u8; MAX_SIZE],
}

impl PreparedHost {
    pub fn new(graph: &Graph) -> Self {
        Self::from_data(GraphData::new(graph))
    }

    fn from_data(data: GraphData) -> Self {
        let size = data.graph.size;
        let mut order = [0; MAX_SIZE];
        for (v, entry) in order[..size].iter_mut().enumerate() {
            *entry = v as u8;
        }
        // Numeric tie-breaking also makes the host twin-prefix rule compatible
        // with ascending images along pattern twin chains.
        order[..size].sort_unstable_by_key(|&v| (Reverse(data.degree[v as usize]), v));
        let twin_pred = data.twin_predecessors(order[..size].iter().map(|&v| v as usize));
        Self {
            data,
            order,
            twin_pred,
        }
    }

    pub fn graph(&self) -> &Graph {
        &self.data.graph
    }

    /// Build the other colour directly from the cached adjacency masks.
    pub fn complement(&self) -> Self {
        Self::from_data(self.data.complement())
    }
}

/// Both colours of a library graph. Prepare once, then test many candidates.
#[derive(Clone, Debug)]
pub struct PreparedColours {
    host: PreparedHost,
    complement: PreparedHost,
}

impl PreparedColours {
    pub fn new(graph: &Graph) -> Self {
        let host = PreparedHost::new(graph);
        let complement = host.complement();
        Self { host, complement }
    }

    pub fn graph(&self) -> &Graph {
        self.host.graph()
    }

    /// Does either colour contain the pattern? Structural impossibilities are
    /// checked before searching. Among feasible colours, try the denser one
    /// first (complement first on ties). This is only an ordering heuristic:
    /// a refutation still requires exact absence in BOTH colours.
    pub fn covers(&self, pattern: &PreparedPattern) -> bool {
        self.covers_ordered(pattern, self.host.data.edges > self.complement.data.edges)
    }

    fn covers_ordered(&self, pattern: &PreparedPattern, host_first: bool) -> bool {
        let host_ok = pattern.feasible(&self.host);
        let complement_ok = pattern.feasible(&self.complement);
        match (host_ok, complement_ok) {
            (false, false) => false,
            (true, false) => pattern.search_feasible(&self.host),
            (false, true) => pattern.search_feasible(&self.complement),
            (true, true) => {
                let (first, second) = if host_first {
                    (&self.host, &self.complement)
                } else {
                    (&self.complement, &self.host)
                };
                pattern.search_feasible::<bool>(first) || pattern.search_feasible(second)
            }
        }
    }
}

struct Search<'a> {
    // Keep the compact library representation, but copy its small arrays into
    // the search frame and widen indices once. No graph information is rebuilt;
    // the recursive hot loop then has the original matcher's local data layout.
    row: &'a [(usize, usize)],
    sub_adj: [u32; MAX_SIZE],
    sup_adj: [u32; MAX_SIZE],
    sup_degree: [usize; MAX_SIZE],
    sup_order: [usize; MAX_SIZE],
    twin_pred: [usize; MAX_SIZE],
    sup_twin_pred: [usize; MAX_SIZE],
    size: usize,
    // For each pattern vertex, host slots occupied by its placed neighbours.
    req: [u32; MAX_SIZE],
    vec: [usize; MAX_SIZE], // host slot -> pattern vertex
    img: [usize; MAX_SIZE], // pattern vertex -> host slot, valid once placed
    used: u32,
}

impl Search<'_> {
    fn go(&mut self, i: usize) -> bool {
        let size = self.size;
        let (degree, v) = self.row[i];
        if degree == 0 {
            let mut j = 0;
            for k in i..size {
                while self.used & (1 << j) != 0 {
                    j += 1;
                }
                self.vec[j] = self.row[k].1;
                self.used |= 1 << j;
            }
            return true;
        }
        let required = self.req[v];
        let above = match self.twin_pred[v] {
            UNFILLED => 0,
            p => self.img[p] + 1,
        };
        for jx in 0..size {
            let j = self.sup_order[jx];
            if j < above {
                continue;
            }
            let jb = 1u32 << j;
            if self.used & jb != 0 {
                continue;
            }
            match self.sup_twin_pred[j] {
                UNFILLED => {}
                p => {
                    if self.used & (1 << p) == 0 {
                        continue;
                    }
                }
            }
            if degree > self.sup_degree[j] {
                continue;
            }
            if required & !self.sup_adj[j] != 0 {
                continue;
            }
            self.vec[j] = v;
            self.img[v] = j;
            self.used |= jb;
            let mut neighbours = self.sub_adj[v];
            while neighbours != 0 {
                let u = neighbours.trailing_zeros() as usize;
                neighbours &= neighbours - 1;
                self.req[u] |= jb;
            }
            if i == size - 1 || self.go(i + 1) {
                return true;
            }
            let mut neighbours = self.sub_adj[v];
            while neighbours != 0 {
                let u = neighbours.trailing_zeros() as usize;
                neighbours &= neighbours - 1;
                self.req[u] &= !jb;
            }
            self.used &= !jb;
            self.vec[j] = UNFILLED;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::{ZERO, bit, random_graph};
    use crate::tools;
    use rand::{Rng, SeedableRng, rngs::StdRng};

    // Opt-in corpus benchmark. Query times exclude preparation and printing;
    // all modes must return exactly the same first refuter for every candidate.
    // See research/2026-11-09.md for environment and example commands.
    #[test]
    #[ignore = "requires GRAPHY_MATCH_N, GRAPHY_MATCH_CANDIDATES, GRAPHY_MATCH_HOSTS"]
    fn benchmark_prepared_library() {
        use std::{env, hint::black_box, mem::size_of, time::Instant};
        let n = env::var("GRAPHY_MATCH_N").unwrap().parse().unwrap();
        let limit = env::var("GRAPHY_MATCH_LIMIT")
            .ok()
            .map(|x| x.parse().unwrap())
            .unwrap_or(usize::MAX);
        let repeats = env::var("GRAPHY_MATCH_REPEATS")
            .ok()
            .map(|x| x.parse().unwrap())
            .unwrap_or(3);
        let start = Instant::now();
        let patterns: Vec<_> =
            tools::read_graphs::<Graph>(n, &env::var("GRAPHY_MATCH_CANDIDATES").unwrap())
                .take(limit)
                .map(|g| PreparedPattern::new(&g))
                .collect();
        let hosts: Vec<_> =
            tools::read_graphs::<Graph>(n, &env::var("GRAPHY_MATCH_HOSTS").unwrap())
                .map(|g| PreparedColours::new(&g))
                .collect();
        eprintln!(
            "prepared {} candidates and {} hosts in {:?}; {} inline bytes per two-colour host",
            patterns.len(),
            hosts.len(),
            start.elapsed(),
            size_of::<PreparedColours>()
        );
        let mut expected = None;
        for iteration in 0..repeats {
            // Alternate timing order to reduce warmup bias.
            for dense_first in if iteration % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            } {
                let start = Instant::now();
                let results: Vec<_> = patterns
                    .iter()
                    .map(|pattern| {
                        hosts.iter().position(|host| {
                            !black_box(host.covers_ordered(
                                pattern,
                                dense_first && host.host.data.edges > host.complement.data.edges,
                            ))
                        })
                    })
                    .collect();
                eprintln!(
                    "iteration={iteration}, dense_first={dense_first}, seconds={:.6}, survivors={}",
                    start.elapsed().as_secs_f64(),
                    results.iter().filter(|r| r.is_none()).count()
                );
                if let Some(ref expected) = expected {
                    assert_eq!(&results, expected);
                } else {
                    expected = Some(results);
                }
            }
        }
    }

    // Independent labelled-bin assignment, including singletons and without
    // symmetry pruning or the largest-first assumption.
    fn brute_pack(items: &[u8], bins: &mut [u8]) -> bool {
        let Some((&item, rest)) = items.split_first() else {
            return true;
        };
        for j in 0..bins.len() {
            if item > bins[j] {
                continue;
            }
            bins[j] -= item;
            let fits = brute_pack(rest, bins);
            bins[j] += item;
            if fits {
                return true;
            }
        }
        false
    }

    fn partitions(n: u8, limit: u8) -> Vec<Vec<u8>> {
        if n == 0 {
            return vec![vec![]];
        }
        (1..=n.min(limit))
            .rev()
            .flat_map(|first| {
                partitions(n - first, first)
                    .into_iter()
                    .map(move |mut tail| {
                        tail.insert(0, first);
                        tail
                    })
            })
            .collect()
    }

    #[test]
    fn packing_agrees_with_brute_force_on_small_partitions() {
        for n in 0..=12.min(MAX_SIZE as u8) {
            let parts = partitions(n, n);
            for items in &parts {
                for bins in &parts {
                    assert_eq!(
                        component_sizes_fit(items, bins),
                        brute_pack(items, &mut bins.clone()),
                        "items={items:?}, capacities={bins:?}"
                    );
                }
            }
        }
        assert!(!component_sizes_fit(&[10, 7, 1], &[14, 4]));
        // All items fit individually and total capacity is sufficient, but
        // neither bin can hold two items. A largest-component test is not enough.
        assert!(!component_sizes_fit(&[6, 6, 6], &[9, 9]));
        assert!(component_sizes_fit(&[4, 3, 2, 1], &[7, 3]));
        // First-fit decreasing fails here; the exact packing search must backtrack.
        assert!(component_sizes_fit(&[6, 5, 3, 2, 2, 2], &[10, 10]));
        assert!(!component_sizes_fit(&[2, 1], &[2]));
    }

    fn check_pair(sub: Graph, sup: Graph) {
        let pattern = PreparedPattern::new(&sub);
        let colours = PreparedColours::new(&sup);
        let expected = tools::naive_is_subgraph_of(&sub, &sup);
        let expected_complement = tools::naive_is_subgraph_of(&sub, &sup.complement());
        assert_eq!(
            pattern.is_subgraph_of(&colours.host),
            expected,
            "{sub:?} / {sup:?}"
        );
        assert_eq!(colours.covers(&pattern), expected || expected_complement);
        assert_eq!(
            PreparedColours::new(&sup.complement()).covers(&pattern),
            expected || expected_complement
        );
        let found = pattern.find_subgraph(&colours.host);
        assert_eq!(found.is_some(), expected);
        if let Some(copy) = found {
            assert_eq!(copy.bits() & !sup.bits(), 0);
            assert_eq!(copy.edge_count(), sub.edge_count());
            assert!(tools::naive_is_subgraph_of(&sub, &copy));
        }
        // Compatibility API preserves the prepared API's exact witness.
        assert_eq!(
            tools::find_subgraph_ss(&sub, pattern.sorted_row(), &sup),
            found
        );
    }

    #[test]
    fn exhaustive_labelled_small_graphs_match_permutation_oracle() {
        for n in 0..=4 {
            for sub in 0..(1u64 << Graph::triangle(n)) {
                for sup in 0..(1u64 << Graph::triangle(n)) {
                    check_pair(Graph::from_u64(n, sub), Graph::from_u64(n, sup));
                }
            }
        }
    }

    #[test]
    fn random_relabelled_graphs_match_permutation_oracle() {
        let mut rng = StdRng::seed_from_u64(20261109);
        for _ in 0..150 {
            let n = rng.gen_range(5..=8);
            let sub = random_graph(&mut rng, n).renumber(&Perm::random(&mut rng, n));
            let sup = random_graph(&mut rng, n).renumber(&Perm::random(&mut rng, n));
            check_pair(sub, sup);
        }
    }

    #[test]
    fn packing_rejects_before_embedding_search() {
        // Two connected 5-vertex paths cannot both fit into a 7-vertex host
        // component; neither can use its other, 3-vertex component.
        let sub = Graph::from_fn(10, |a, b| a + 1 == b && b != 5);
        let sup = Graph::from_fn(10, |a, b| (a < 7) == (b < 7));
        let pattern = PreparedPattern::new(&sub);
        let host = PreparedHost::new(&sup);
        assert_eq!(pattern.data.components.sizes(), &[5, 5]);
        assert_eq!(host.data.components.sizes(), &[7, 3]);
        assert!(!pattern.feasible(&host));
        assert!(!pattern.is_subgraph_of(&host));
        assert!(pattern.find_subgraph(&host).is_none());
    }

    #[test]
    fn components_can_share_a_host_and_packing_is_not_sufficient() {
        let two_edges = Graph::from_fn(4, |a, b| (a, b) == (0, 1) || (a, b) == (2, 3));
        check_pair(two_edges, Graph::from_bits(4, one_bits(6)));
        let triangle = Graph::from_u64(4, 7); // plus one isolated vertex
        let square = Graph::from_fn(4, |a, b| b == a + 1 || (a, b) == (0, 3));
        let pattern = PreparedPattern::new(&triangle);
        let host = PreparedHost::new(&square);
        assert!(pattern.feasible(&host));
        assert!(!pattern.is_subgraph_of(&host));
        check_pair(triangle, square);
    }

    #[test]
    fn preparation_owns_graphs_and_preserves_complements_at_maximum_width() {
        let n = MAX_SIZE;
        let mut graph = Graph::from_bits(n, bit(Graph::triangle(n) - 1));
        let saved = graph;
        let pattern = PreparedPattern::new(&graph);
        let host = PreparedHost::new(&graph);
        graph = Graph::from_bits(n, ZERO);
        assert_eq!(*pattern.graph(), saved);
        assert_eq!(*host.graph(), saved);
        assert_ne!(*pattern.graph(), graph);
        let direct = PreparedHost::new(&saved.complement());
        let complement = host.complement();
        assert_eq!(complement.data.adj, direct.data.adj);
        assert_eq!(
            complement.data.components.sizes(),
            direct.data.components.sizes()
        );
        assert_eq!(complement.data.degree, direct.data.degree);
        assert_eq!(complement.order, direct.order);
        assert_eq!(complement.twin_pred, direct.twin_pred);
        assert!(pattern.is_subgraph_of(&host));
        assert!(pattern.is_subgraph_of(&complement));
        assert_eq!(pattern.find_subgraph(&host).unwrap().bits().count_ones(), 1);
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PreparedPattern>();
        assert_send_sync::<PreparedHost>();
        assert_send_sync::<PreparedColours>();
    }

    #[test]
    fn twin_heavy_graphs_embed_after_relabelling_at_every_order() {
        let mut rng = StdRng::seed_from_u64(2311);
        for n in 1..=MAX_SIZE {
            let graphs = [
                Graph::from_fn(n, |_, _| true),
                Graph::from_fn(n, |_, _| false),
                Graph::from_fn(n, |a, _| a == 0),
                Graph::from_fn(n, |a, b| (a < n / 2) == (b < n / 2)),
                Graph::from_fn(n, |a, b| (a < n / 2) != (b < n / 2)),
                Graph::from_fn(n, |a, b| a / 3 == b / 3),
            ];
            for host in graphs {
                let sub = Graph::from_bits(n, host.bits() & random_graph(&mut rng, n).bits());
                let sub = sub.renumber(&Perm::random(&mut rng, n));
                let host = host.renumber(&Perm::random(&mut rng, n));
                let pattern = PreparedPattern::new(&sub);
                let prepared = PreparedHost::new(&host);
                assert!(
                    pattern.is_subgraph_of(&prepared),
                    "n={n}, sub={sub:?}, host={host:?}"
                );
                let copy = pattern.find_subgraph(&prepared).unwrap();
                assert_eq!(copy.bits() & !host.bits(), 0);
                assert_eq!(copy.edge_count(), sub.edge_count());
            }
        }
    }

    #[test]
    #[should_panic(expected = "pad the pattern")]
    fn unequal_orders_are_not_silently_truncated() {
        PreparedPattern::new(&Graph::from_u64(3, 1))
            .is_subgraph_of(&PreparedHost::new(&Graph::from_u64(4, 1)));
    }

    #[test]
    #[should_panic(expected = "outside its stated order")]
    fn edges_outside_order_are_rejected() {
        PreparedPattern::new(&Graph::from_u64(3, 8));
    }

    #[test]
    #[should_panic(expected = "stale matching order")]
    fn stale_legacy_rows_are_rejected() {
        PreparedPattern::with_row(&Graph::from_u64(3, 1), &[(0, 0), (0, 1), (0, 2)]);
    }

    #[cfg(feature = "u256")]
    #[test]
    fn eighteen_vertex_slow_query_is_a_root_packing_rejection() {
        let sub = Graph::from_bits(18, "1329268563080305633988669656996007944".parse().unwrap());
        let sup = Graph::from_bits(
            18,
            "615220134502691169874691659634381667188743"
                .parse()
                .unwrap(),
        );
        let mut rng = StdRng::seed_from_u64(1818);
        for _ in 0..20 {
            let sub = sub.renumber(&Perm::random(&mut rng, 18));
            let sup = sup.renumber(&Perm::random(&mut rng, 18));
            let pattern = PreparedPattern::new(&sub);
            let colours = PreparedColours::new(&sup);
            assert_eq!(pattern.data.components.sizes(), &[10, 7, 1]);
            assert_eq!(colours.host.data.components.sizes(), &[14, 4]);
            assert!(!pattern.feasible(&colours.host));
            assert!(pattern.find_subgraph(&colours.host).is_none());
            let copy = pattern.find_subgraph(&colours.complement).unwrap();
            assert_eq!(copy.bits() & !sup.complement().bits(), 0);
            assert_eq!(copy.edge_count(), 18);
            assert!(colours.covers(&pattern));
        }
    }
}
