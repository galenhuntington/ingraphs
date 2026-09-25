#!/usr/bin/env python3
"""Generate unions of edge-orbitals of small group actions — the family
that has produced the fertile counterexamples: circulants (orbital
unions of Z_n), and the n=14 one-off refuters (orbital unions of
subgroups of S_4 x (part groups) acting on layered subsets of a 4-set
plus independent parts).

Vertex set = a block of subset-layers of [k] (i-subsets, and for k=4 the
3 perfect pairings) acted on diagonally, plus independent parts each
carrying its own group (symmetric, cyclic, dihedral).  For the block,
the diagonal group ranges over S_k and cyclic/dihedral subgroups (plus a
pair-complementation extension); every union of the resulting edge
orbits with edge count in [min,max] is emitted as a graphy number.
The optional cyclic family uses one permutation of each cycle type,
including full circulants and coupled rotations of several blocks.
Actions exceeding --max-orbits (default 18) are skipped, so generation
is not exhaustive over all graphs with symmetries.

Output streams without keeping a set of emitted graphs. For fast
isomorphism deduplication of a heuristic refuter library, use:
  python3 scripts/orbitals.py 14 35 45 --family cyclic --graph6 \
    | labelg -q | uniqg -cq > output/cyclic14.g6

uniqg stores SHA-256 fingerprints; use awk '!x[$0]++' after labelg if
exact comparisons are required. Both retain a growing deduplication cache.

graphy reads graph6 directly. To get graphy's canonical decimal labels,
use graphy canon on the deduplicated library (or just on useful refuters).
The original three positional arguments still select the layered family.
"""

import argparse
from itertools import combinations

from refuter_variants import graph6

MAX_ORBITS = 18
MAX_LAYERS = 5
MAX_PARTS = 2

def idx(a, b):
    if a > b: a, b = b, a
    return b*(b-1)//2 + a

def edge_orbits(nverts, generators):
    """Unordered-pair orbits, using generators without closing the group.

    Traversing the generator edges reaches exactly the generated-group orbit:
    an inverse permutation is a positive power of that permutation. The group
    itself can be enormous even when this traversal has only C(n,2) states.
    """
    orbit_of = {}
    orbits = []
    for b in range(1, nverts):
        for a in range(b):
            if (a, b) in orbit_of: continue
            o = set()
            stack = [(a, b)]
            while stack:
                (x, y) = stack.pop()
                if (x, y) in o: continue
                o.add((x, y))
                for p in generators:
                    px, py = p[x], p[y]
                    if px > py: px, py = py, px
                    if (px, py) not in o: stack.append((px, py))
            for e in o: orbit_of[e] = len(orbits)
            orbits.append(sorted(o))
    return tuple(sorted(sum(1 << idx(a, b) for a, b in o) for o in orbits))


def orbit_unions(orbits, emin, emax):
    """Enumerate unions, pruning branches outside the requested edge range."""
    orbits = sorted(orbits, key=int.bit_count, reverse=True)
    sizes = [o.bit_count() for o in orbits]
    remaining = [0] * (len(orbits) + 1)
    for i in range(len(orbits) - 1, -1, -1):
        remaining[i] = remaining[i + 1] + sizes[i]

    def visit(i, bits, edges):
        if edges > emax or edges + remaining[i] < emin:
            return
        if i == len(orbits):
            yield bits
            return
        yield from visit(i + 1, bits, edges)
        yield from visit(i + 1, bits | orbits[i], edges + sizes[i])

    yield from visit(0, 0, 0)

def apply_elem(p, el):
    if isinstance(el, frozenset):
        return frozenset(apply_elem(p, x) for x in el)
    return p[el]

def sk_layer_kinds(k):
    kinds = []
    for i in range(0, k+1):
        els = [frozenset(c) for c in combinations(range(k), i)]
        kinds.append((len(els), els))
    if k == 4:
        pairings = []
        for b in range(1, 4):
            rest = [x for x in range(1, 4) if x != b]
            pairings.append(frozenset([frozenset([0, b]), frozenset(rest)]))
        kinds.append((3, pairings))
    return kinds

def block_perm_generators(k, layers, base_index, base):
    """generator sets for the diagonal action on the block: full S_k,
       cyclic, dihedral; each optionally extended by 2-subset
       complementation"""
    def lift(p):
        return {v: base_index[(li, apply_elem(p, el))]
                for v, (li, el) in enumerate(base)}
    def to_tuple(m, extra=None):
        return tuple((extra or {}).get(v, m[v]) for v in range(len(base)))
    swap = tuple([1, 0] + list(range(2, k)))
    cyc = tuple(list(range(1, k)) + [0])
    flip = tuple([(-i) % k for i in range(k)])
    gsets = {
        'sym': [lift(swap), lift(cyc)],
        'cyc': [lift(cyc)],
        'dih': [lift(cyc), lift(flip)],
    }
    # complementation on any 2-subsets layer of k=4 (commutes with S_4)
    comp = {}
    if k == 4:
        for v, (li, el) in enumerate(base):
            if isinstance(el, frozenset) and len(el) == 2 \
                    and all(isinstance(x, int) for x in el):
                cel = frozenset(range(4)) - el
                comp[v] = base_index[(li, cel)]
    out = []
    for name, gs in gsets.items():
        out.append([to_tuple(m) for m in gs])
        if comp:
            out.append([to_tuple(m) for m in gs]
                       + [tuple(comp.get(v, v) for v in range(len(base)))])
    return out

def part_generator_sets(m, offset):
    """group choices for an independent part of size m at vertex offset"""
    vs = list(range(offset, offset + m))
    if m == 1: return [[]]
    swap = {vs[0]: vs[1], vs[1]: vs[0]}
    cyc = {vs[i]: vs[(i+1) % m] for i in range(m)}
    flip = {vs[i]: vs[(-i) % m] for i in range(m)}
    out = [[swap, cyc]]          # S_m
    if m >= 3:
        out.append([cyc])        # Z_m
        out.append([cyc, flip])  # D_m
    return out

def compositions(sizes, remaining, start):
    if remaining == 0:
        yield []
        return
    for i in range(start, len(sizes)):
        if sizes[i] <= remaining:
            for rest in compositions(sizes, remaining - sizes[i], i):
                yield [i] + rest

def pad(mapping, nverts):
    return tuple(mapping.get(v, v) for v in range(nverts))

def layered_actions(n_target):
    # Subset-layers of [k] + independent parts (sizes 1..7).
    for k in range(3, 6):
        kinds = sk_layer_kinds(k)
        layer_sizes = [s for s, _ in kinds]
        for nblock in range(0, n_target + 1):
            block_comps = compositions(layer_sizes, nblock, 0)
            for bc in block_comps:
                if len(bc) > MAX_LAYERS: continue
                if nblock == 0 and k > 3: continue
                layers = [kinds[i] for i in bc]
                base = []
                for li, (_, els) in enumerate(layers):
                    base += [(li, el) for el in els]
                base_index = {v: i for i, v in enumerate(base)}
                bgen_sets = block_perm_generators(k, layers, base_index, base) \
                    if nblock > 0 else [[]]
                rest = n_target - nblock
                for pc in compositions(list(range(1, 8)), rest, 0):
                    if len(pc) > MAX_PARTS: continue
                    if len(bc) + len(pc) > MAX_LAYERS: continue
                    part_sizes = [p+1 for p in pc]
                    part_offsets = []
                    off = nblock
                    for m in part_sizes:
                        part_offsets.append((m, off))
                        off += m
                    pgen_choices = [part_generator_sets(m, o)
                                    for (m, o) in part_offsets]
                    def rec(i, gens):
                        if i == len(pgen_choices):
                            for bg in bgen_sets:
                                allg = [pad(dict(enumerate(t)), n_target)
                                        for t in bg] \
                                     + [pad(m, n_target) for m in gens]
                                if allg:
                                    yield allg
                            return
                        for gs in pgen_choices[i]:
                            yield from rec(i + 1, gens + gs)
                    yield from rec(0, [])


def cyclic_actions(n, cycles=None):
    """One permutation of every cycle type, including a full n-cycle.

    Multiple cycles rotate together, so cross-block edges may be matchings
    or shifted matchings, not just complete or empty bipartite graphs. Up to
    relabelling, this covers every cyclic subgroup action on n vertices.
    """
    if cycles is not None and (not cycles or min(cycles) < 1 or sum(cycles) != n):
        raise ValueError("cycle lengths must be positive and sum to n")
    cycle_types = [cycles] if cycles is not None else (
        [part + 1 for part in composition]
        for composition in compositions(list(range(1, n + 1)), n, 0)
    )
    for cycle_type in cycle_types:
        perm = list(range(n))
        offset = 0
        for size in cycle_type:
            for v in range(size):
                perm[offset + v] = offset + (v + 1) % size
            offset += size
        yield [tuple(perm)]


def generate(n, emin, emax, family="layers", max_orbits=MAX_ORBITS, cycles=None):
    """Stream orbital unions, including duplicates between distinct actions.

    Only the small set of orbit partitions is cached here. Downstream tools
    can deduplicate canonical forms without retaining every labeled graph.
    """
    partitions_done = set()
    families = []
    if family in ("layers", "all"):
        families.append(layered_actions(n))
    if family in ("cyclic", "all"):
        families.append(cyclic_actions(n, cycles))
    for actions in families:
        for generators in actions:
            orbits = edge_orbits(n, generators)
            if len(orbits) > max_orbits or orbits in partitions_done:
                continue
            # Different groups with the same pair-orbit partition generate
            # exactly the same graphs; the partition is all we need to cache.
            partitions_done.add(orbits)
            yield from orbit_unions(orbits, emin, emax)


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("n", type=int)
    parser.add_argument("min_edges", type=int)
    parser.add_argument("max_edges", type=int)
    parser.add_argument("--family", choices=("layers", "cyclic", "all"), default="layers")
    parser.add_argument("--max-orbits", type=int, default=MAX_ORBITS)
    parser.add_argument("--cycles", help="restrict --family cyclic to one cycle type, e.g. 6,6,2")
    parser.add_argument("--graph6", action="store_true",
                        help="output graph6 for streaming deduplication with labelg -q | uniqg -cq")
    args = parser.parse_args()
    if not 1 <= args.n <= 62:
        parser.error("n must be between 1 and 62")
    if not 0 <= args.min_edges <= args.max_edges <= args.n * (args.n - 1) // 2:
        parser.error("invalid edge range")
    if args.max_orbits < 0:
        parser.error("max-orbits must be nonnegative")
    cycles = None
    if args.cycles is not None:
        if args.family != "cyclic":
            parser.error("--cycles requires --family cyclic")
        try:
            cycles = list(map(int, args.cycles.split(",")))
        except ValueError:
            parser.error("--cycles must be comma-separated integers")
        if min(cycles) < 1 or sum(cycles) != args.n:
            parser.error("cycle lengths must be positive and sum to n")
    for bits in generate(args.n, args.min_edges, args.max_edges,
                         args.family, args.max_orbits, cycles):
        print(graph6(args.n, bits) if args.graph6 else bits)


if __name__ == "__main__":
    main()
