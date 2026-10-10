#!/usr/bin/env python3
"""Reproduce the October 5 structural analysis; optionally emit orbit-flip neighbours.

No external graph library: enumerate automorphisms by colour refinement and
exact induced adjacency matching. Intended for this particular small graph,
not highly symmetric arbitrary graphs. Labels below are graphy's least-decimal
labels of the new 15-vertex refuter. --depth emits decimal hosts to stdout;
analysis always goes to stderr.
"""

import argparse
from collections import Counter
from itertools import combinations
import math
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from orbitals import edge_orbits
from verify_refutations import adjacency, contains

HOST = 156118014097567454983522103044
CANDIDATE = 106450004178944


def template_recipe():
    """Four aligned triples and three singleton vertices; see October 5 note."""
    a, b, c, d = (1, 3, 6), (2, 4, 7), (9, 8, 5), (12, 13, 14)
    edges = list(combinations(b, 2)) + list(combinations(c, 2))
    for x, y in [(a, b), (b, c), (c, d)]:
        edges.extend(zip(x, y))
    for x, y in [(a, c), (a, d), (b, d)]:
        edges.extend((u, v) for i, u in enumerate(x) for j, v in enumerate(y) if i != j)
    for apex, parts in [(0, (c, d)), (10, (a, c)), (11, (a, b))]:
        edges.extend((apex, v) for part in parts for v in part)
    edges.append((10, 11))
    return sum(1 << (max(u, v) * (max(u, v) - 1) // 2 + min(u, v)) for u, v in edges)


def automorphisms(n, host):
    adj = adjacency(n, host)
    colors = [row.bit_count() for row in adj]
    while True:
        signatures = [(colors[v], tuple(sorted(colors[u] for u in range(n) if adj[v] >> u & 1)))
                      for v in range(n)]
        palette = {s: i for i, s in enumerate(sorted(set(signatures)))}
        refined = [palette[s] for s in signatures]
        if len(set(refined)) == len(set(colors)):
            colors = refined
            break
        colors = refined

    def visit(mapping, used):
        if len(mapping) == n:
            yield tuple(mapping[v] for v in range(n))
            return
        choices = {v: [u for u in range(n) if not used >> u & 1 and colors[v] == colors[u]
                       and all((adj[v] >> a & 1) == (adj[u] >> b & 1) for a, b in mapping.items())]
                   for v in range(n) if v not in mapping}
        v = min(choices, key=lambda v: len(choices[v]))
        for u in choices[v]:
            yield from visit(mapping | {v: u}, used | (1 << u))

    return list(visit({}, 0))


def cycles(perm):
    seen, result = set(), []
    for v in range(len(perm)):
        if v in seen:
            continue
        cycle = []
        while v not in seen:
            seen.add(v)
            cycle.append(v)
            v = perm[v]
        result.append(cycle)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--depth', type=int, default=0, help='emit H plus all flips of up to this many whole pair orbits')
    parser.add_argument('--delete-one', action='store_true', help='emit every 14-vertex deletion of each emitted host')
    args = parser.parse_args()
    if not 0 <= args.depth <= 4:
        parser.error('this bounded exploration supports depth 0..4')
    n, host = 15, HOST
    assert template_recipe() == host
    full = (1 << 105) - 1
    assert not contains(n, CANDIDATE, host)[0]
    assert not contains(n, CANDIDATE, host ^ full)[0]
    group = automorphisms(n, host)
    assert len(group) == 6
    adj = adjacency(n, host)
    parts = sorted({tuple(sorted({p[v] for p in group})) for v in range(n)})
    orbits = edge_orbits(n, group)
    assert len(orbits) == 31 and all(host & o in (0, o) for o in orbits)
    say = lambda *values: print(*values, file=sys.stderr, flush=True)
    say('host', host, 'edges', host.bit_count(), 'degree histogram', dict(Counter(a.bit_count() for a in adj)))
    for perm in group:
        cs = cycles(perm)
        say('automorphism', cs, 'order', math.lcm(*map(len, cs)), 'pair-orbits', len(edge_orbits(n, [perm])))
    say('vertex orbits', parts)
    say('pair orbits', len(orbits), 'red', sum(bool(host & o) for o in orbits),
        'size histogram', dict(Counter(o.bit_count() for o in orbits)))
    say('equitable quotient rows (neighbours in each vertex orbit):')
    for part in parts:
        say([sum(bool(adj[part[0]] >> v & 1) for v in other) for other in parts])
    say('twins', [(u, v) for u in range(n) for v in range(u)
                  if (adj[u] ^ adj[v]) & ~((1 << u) | (1 << v)) == 0])
    if args.depth or args.delete_one:
        emitted = 0
        for depth in range(args.depth + 1):
            for flipped in combinations(orbits, depth):
                neighbour = host
                for orbit in flipped:
                    neighbour ^= orbit
                if args.delete_one:
                    for removed in range(n):
                        kept = [v for v in range(n) if v != removed]
                        print(sum(((neighbour >> (v * (v-1)//2 + u)) & 1) << (j * (j-1)//2+i)
                                  for j, v in enumerate(kept) for i, u in enumerate(kept[:j])))
                        emitted += 1
                else:
                    print(neighbour)
                    emitted += 1
        say('emitted', emitted, 'labelled orbit-neighbour hosts (not isomorphism-deduplicated)')


if __name__ == '__main__':
    main()
