#!/usr/bin/env python3
"""Generate nearby hosts from successful counterexamples (not proof certificates).

Examples:
  python3 scripts/refuter_variants.py 14 seeds.txt --mode flips --distance 2
  python3 scripts/refuter_variants.py 14 seeds.txt --mode switch --graph6 \
    | labelg -q | sort -u | graphy cull 14 output/batches/all14 /dev/stdin

Output has one representative of each *labelled* complement pair. For fast
isomorphism deduplication, use --graph6 and nauty's labelg as above. Its labels
differ from graphy's canonical numbers; graphy can read either format.
"""

import argparse
from itertools import combinations
import sys


def edge_index(a, b):
    a, b = sorted((a, b))
    return b * (b - 1) // 2 + a


# Graphy's first edge is the low bit; graph6 puts it at the high end of
# each sextet. Reverse six bits once per table entry, not once per edge.
_GRAPH6_SEXTETS = tuple(chr(63 + int(f"{value:06b}"[::-1], 2))
                       for value in range(64))


def graph6(n, bits):
    if not 1 <= n <= 62:
        raise ValueError("single-byte graph6 requires 1 <= n <= 62")
    length = n * (n - 1) // 2
    return chr(n + 63) + "".join(
        _GRAPH6_SEXTETS[(bits >> start) & 63]
        for start in range(0, length, 6)
    )


def variants(n, bits, mode, distance=1):
    """Include the seed; all operations preserve the vertex count.

    flips: toggle up to distance arbitrary edges.
    switch: toggle the cut between S and its complement, for every S.
    rewire: replace the neighborhood of one vertex arbitrarily.
    clone: replace one vertex with a true or false twin of another.

    These operations need not preserve refutation. Every output must be checked.
    """
    yield bits
    length = n * (n - 1) // 2
    if mode == "flips":
        for d in range(1, distance + 1):
            for edges in combinations(range(length), d):
                yield bits ^ sum(1 << e for e in edges)
    elif mode == "switch":
        # Fix vertex n-1 outside S: S and its complement give the same cut.
        stars = [sum(1 << edge_index(v, u) for u in range(n) if u != v)
                 for v in range(n - 1)]
        cut = 0
        for step in range(1, 1 << (n - 1)):
            cut ^= stars[(step & -step).bit_length() - 1]
            yield bits ^ cut
    elif mode == "rewire":
        for v in range(n):
            edges = [1 << edge_index(v, u) for u in range(n) if u != v]
            base = bits & ~sum(edges)
            neighborhood = 0
            yield base
            for step in range(1, 1 << (n - 1)):
                neighborhood ^= edges[(step & -step).bit_length() - 1]
                yield base | neighborhood
    elif mode == "clone":
        for v in range(n):
            star = sum(1 << edge_index(v, u) for u in range(n) if u != v)
            for source in range(n):
                if source == v:
                    continue
                neighborhood = sum(
                    1 << edge_index(v, u) for u in range(n)
                    if u not in (v, source) and bits >> edge_index(source, u) & 1
                )
                clone = (bits & ~star) | neighborhood
                yield clone
                yield clone | (1 << edge_index(v, source))
    else:
        raise ValueError(f"unknown mode: {mode}")


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("n", type=int)
    parser.add_argument("seeds", help="decimal graph numbers, one per line; CSV first columns accepted")
    parser.add_argument("--mode", choices=("flips", "switch", "rewire", "clone"),
                        default="flips")
    parser.add_argument("--distance", type=int, default=1,
                        help="maximum number of edge flips (default: 1)")
    parser.add_argument("--graph6", action="store_true")
    parser.add_argument("--min-edges", type=int, default=0,
                        help="minimum edges in the sparser member of the complement pair")
    args = parser.parse_args()
    if not 1 <= args.n <= 62:
        parser.error("n must be between 1 and 62")
    length = args.n * (args.n - 1) // 2
    if not 0 <= args.distance <= length:
        parser.error("distance must lie between 0 and the number of possible edges")
    if not 0 <= args.min_edges <= length // 2:
        parser.error("min-edges must lie between 0 and half the possible edges")
    full = (1 << length) - 1
    seen = set()
    with open(args.seeds) as source:
        for line_number, line in enumerate(source, 1):
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            seed = int(line.split(",", 1)[0])
            if not 0 <= seed <= full:
                parser.error(f"seed on line {line_number} does not fit n={args.n}")
            for bits in variants(args.n, seed, args.mode, args.distance):
                # Edge count first gives the sparse member; numeric tie break
                # handles exactly half-full graphs, including self-complements.
                complement = full ^ bits
                if (complement.bit_count(), complement) < (bits.bit_count(), bits):
                    bits = complement
                if bits.bit_count() < args.min_edges or bits in seen:
                    continue
                seen.add(bits)
                print(graph6(args.n, bits) if args.graph6 else bits)
    print(f"Emitted {len(seen)} labelled complement representatives", file=sys.stderr)


if __name__ == "__main__":
    main()
