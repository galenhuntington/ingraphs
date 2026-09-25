#!/usr/bin/env python3
"""Bounded neighborhoods in the n=14, 6+6+2 refuter template.

A and B induce complementary six-vertex graphs, their cross graph is
3-regular, and two independent twins join all A and no B. Two moves preserve
this 45-edge template: toggle an A edge and its corresponding B edge, or
switch a checkerboard 2x2 in the cross graph. Depth two can change up to eight
edges. This is a heuristic neighborhood, not an exhaustive template search.

Input: decimal seed hosts, one per line (CSV first columns also accepted).
Output streams; the explicitly capped BFS keeps labeled states. Deduplicate
isomorphs externally, e.g. --graph6 | labelg -q | uniqg -cq.
"""

import argparse
from collections import deque
from itertools import combinations, permutations
import sys

from refuter_variants import edge_index, graph6


def edge(bits, a, b):
    return (bits >> edge_index(a, b)) & 1


def normalize_seed(bits):
    """Recognize the template in any labeling; return a fixed-part labeling.

    This is NOT graph canonicalization. Use the first valid twin pair and
    first complement isomorphism; different recognitions can yield different
    neighborhoods under the paired-edge move.
    """
    if not 0 <= bits < 1 << 91 or bits.bit_count() != 45:
        raise ValueError("a seed must be a 45-edge graph on 14 vertices")
    for x, y in combinations(range(14), 2):
        if edge(bits, x, y):
            continue
        a = [v for v in range(14) if v not in (x, y) and edge(bits, x, v)]
        if len(a) != 6 or any(edge(bits, x, v) != edge(bits, y, v)
                             for v in range(14) if v not in (x, y)):
            continue
        b = [v for v in range(14) if v not in (*a, x, y)]
        if any(sum(edge(bits, u, v) for v in b) != 3 for u in a):
            continue
        if any(sum(edge(bits, u, v) for u in a) != 3 for v in b):
            continue
        for order in permutations(b):
            if all(edge(bits, a[i], a[j]) != edge(bits, order[i], order[j])
                   for i, j in combinations(range(6), 2)):
                vertices = a + list(order) + [x, y]
                return sum(edge(bits, vertices[i], vertices[j]) << edge_index(i, j)
                           for i, j in combinations(range(14), 2))
    raise ValueError("seed does not fit the 6+6+2 template")


def neighbors(bits):
    """Input uses the fixed-part labeling returned by normalize_seed."""
    for a, b in combinations(range(6), 2):
        yield bits ^ (1 << edge_index(a, b)) ^ (1 << edge_index(a + 6, b + 6))
    for a, b in combinations(range(6), 2):
        for c, d in combinations(range(6, 12), 2):
            ac, ad = edge(bits, a, c), edge(bits, a, d)
            if ac != ad and edge(bits, b, c) == ad and edge(bits, b, d) == ac:
                yield bits ^ sum(1 << edge_index(u, v)
                                 for u in (a, b) for v in (c, d))


def explore(seeds, depth=2, max_graphs=10_000):
    if depth < 0 or max_graphs < 1:
        raise ValueError("depth must be nonnegative and max_graphs positive")
    seen = set()
    queue = deque()
    for seed in seeds:
        bits = normalize_seed(seed)
        if bits not in seen:
            seen.add(bits)
            yield bits
            if len(seen) >= max_graphs:
                return
            queue.append((bits, 0))
    while queue:
        bits, distance = queue.popleft()
        if distance >= depth:
            continue
        for other in neighbors(bits):
            if other in seen:
                continue
            seen.add(other)
            yield other
            if len(seen) >= max_graphs:
                return
            # Leaves need no queue entry: the cap bounds both retained sets.
            if distance + 1 < depth:
                queue.append((other, distance + 1))


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("seeds")
    parser.add_argument("--depth", type=int, default=2)
    parser.add_argument("--max-graphs", type=int, default=10_000)
    parser.add_argument("--graph6", action="store_true")
    args = parser.parse_args()
    if args.depth < 0 or args.max_graphs < 1:
        parser.error("depth must be nonnegative and max-graphs positive")
    with open(args.seeds) as source:
        seeds = [int(line.split(",", 1)[0]) for line in source
                 if line.strip() and not line.lstrip().startswith("#")]
    count = 0
    try:
        for bits in explore(seeds, args.depth, args.max_graphs):
            print(graph6(14, bits) if args.graph6 else bits)
            count += 1
    except ValueError as error:
        parser.error(str(error))
    print(f"Emitted {count} labeled hosts (cap {args.max_graphs}, depth {args.depth}); "
          "not exhaustive over the template", file=sys.stderr)


if __name__ == "__main__":
    main()
