#!/usr/bin/env python3
"""Independently verify n,candidate,refuter CSV certificates using only Python.

This uses ordinary injective subgraph search with dynamic minimum-domain
ordering and forward checking. It does not call graphy/nauty or use their
canonicalization or twin symmetry breaking. No claim of speed on arbitrary
inputs is intended. Trailing isolated pattern vertices are implicit up to n.
"""

import argparse
import csv


def adjacency(n, bits):
    if not 0 <= bits < 1 << (n * (n - 1) // 2):
        raise ValueError(f"graph does not fit on {n} vertices: {bits}")
    rows = [0] * n
    for b in range(1, n):
        for a in range(b):
            if bits >> (b * (b - 1) // 2 + a) & 1:
                rows[a] |= 1 << b
                rows[b] |= 1 << a
    return rows


def contains(n, pattern, host):
    sub = adjacency(n, pattern)
    sup = adjacency(n, host)
    active = [v for v in range(n) if sub[v]]
    degrees = [row.bit_count() for row in sub]
    domains = [sum(1 << w for w in range(n) if sup[w].bit_count() >= degrees[v])
               for v in range(n)]
    nodes = 0

    def search(vertices, available):
        nonlocal nodes
        nodes += 1
        if not vertices:
            return True
        v = min(vertices, key=lambda u: (available[u].bit_count(), -degrees[u], u))
        choices = available[v]
        rest = [u for u in vertices if u != v]
        while choices:
            bit = choices & -choices
            choices ^= bit
            w = bit.bit_length() - 1
            next_domains = available.copy()
            viable = True
            for u in rest:
                domain = available[u] & ~bit
                if sub[v] >> u & 1:
                    domain &= sup[w]
                if not domain:
                    viable = False
                    break
                next_domains[u] = domain
            if viable and search(rest, next_domains):
                return True
        return False

    result = search(active, domains)
    return result, nodes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("certificates")
    args = parser.parse_args()
    count = 0
    with open(args.certificates, newline="") as source:
        for row in csv.DictReader(source):
            n, candidate, refuter = (int(row[key]) for key in ("n", "candidate", "refuter"))
            if n < 1:
                raise ValueError("n must be positive")
            complement = ((1 << (n * (n - 1) // 2)) - 1) ^ refuter
            red, red_nodes = contains(n, candidate, refuter)
            blue, blue_nodes = contains(n, candidate, complement)
            if red or blue:
                raise SystemExit(f"INVALID: n={n}, candidate={candidate}, refuter={refuter}")
            print(f"Verified n={n}, candidate={candidate}, refuter={refuter}; "
                  f"search nodes={red_nodes + blue_nodes}", flush=True)
            count += 1
    if not count:
        raise SystemExit("No certificates to check")
    print(f"Verified all {count} certificates")


if __name__ == "__main__":
    main()
