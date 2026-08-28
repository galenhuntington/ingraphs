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
Dedupe with graphy canon afterwards:

  ./scripts/orbitals.py 14 40 51 | ./target/release/graphy canon 14 /dev/stdin | sort -un
"""

import sys
from itertools import combinations, permutations

n_target = int(sys.argv[1])
emin, emax = int(sys.argv[2]), int(sys.argv[3])
MAX_ORBITS = 18
MAX_LAYERS = 5
MAX_PARTS = 2

def idx(a, b):
    if a > b: a, b = b, a
    return b*(b-1)//2 + a

emitted = set()

def emit_orbit_unions(nverts, group_perms):
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
                for p in group_perms:
                    px, py = p[x], p[y]
                    if px > py: px, py = py, px
                    if (px, py) not in o: stack.append((px, py))
            for e in o: orbit_of[e] = len(orbits)
            orbits.append(sorted(o))
    if len(orbits) > MAX_ORBITS: return
    sizes = [len(o) for o in orbits]
    nums = [sum(1 << idx(a, b) for (a, b) in o) for o in orbits]
    # meet-in-the-middle would be overkill; orbit counts are small
    for pick in range(1, 1 << len(orbits)):
        e = 0
        for i in range(len(orbits)):
            if pick >> i & 1: e += sizes[i]
        if emin <= e <= emax:
            g = 0
            for i in range(len(orbits)):
                if pick >> i & 1: g |= nums[i]
            if g not in emitted:
                emitted.add(g)
                print(g)

groups_done = set()

def close_group(nverts, gens):
    ident = tuple(range(nverts))
    group = {ident}
    frontier = [ident]
    while frontier:
        q = frontier.pop()
        for g in gens:
            ng = tuple(g[q[i]] for i in range(nverts))
            if ng not in group:
                group.add(ng)
                frontier.append(ng)
        if len(group) > 20000: return None
    key = frozenset(group)
    if key in groups_done: return None
    groups_done.add(key)
    return sorted(group)

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

# block of subset-layers of [k] + independent parts (sizes 1..7)
for k in range(3, 6):
    kinds = sk_layer_kinds(k)
    layer_sizes = [s for s, _ in kinds]
    for nblock in range(0, n_target + 1):
        block_comps = list(compositions(layer_sizes, nblock, 0)) \
            if nblock > 0 else [[]]
        for bc in block_comps:
            if len(bc) > MAX_LAYERS: continue
            if nblock == 0 and k > 3: continue  # parts-only: do once
            layers = [kinds[i] for i in bc]
            base = []
            for li, (_, els) in enumerate(layers):
                base += [(li, el) for el in els]
            base_index = {v: i for i, v in enumerate(base)}
            bgen_sets = block_perm_generators(k, layers, base_index, base) \
                if nblock > 0 else [[]]
            # independent parts filling the rest
            rest = n_target - nblock
            for pc in compositions(list(range(1, 8)), rest, 0) if rest else [[]]:
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
                            grp = close_group(n_target, allg) \
                                if allg else None
                            if grp: emit_orbit_unions(n_target, grp)
                        return
                    for gs in pgen_choices[i]:
                        rec(i + 1, gens + gs)
                rec(0, [])
