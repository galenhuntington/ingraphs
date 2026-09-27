#!/usr/bin/env python3
"""Independent exact copy counts, for auditing small/near-free repair hosts.

Counts distinct edge sets, not injections. No graphy, nauty, automorphism
division, or pattern-twin pruning. Can be very slow on copy-rich hosts.
"""

import argparse
from verify_refutations import adjacency


def copies(n, pattern, host):
    sub, sup = adjacency(n, pattern), adjacency(n, host)
    active = [v for v in range(n) if sub[v]]
    domains = [sum(1 << w for w in range(n) if sup[w].bit_count() >= sub[v].bit_count())
               for v in range(n)]
    edges = [(a, b) for b in range(n) for a in range(b) if sub[a] >> b & 1]
    image = [-1] * n
    found = set()

    def search(vertices, available):
        if not vertices:
            mask = 0
            for a, b in edges:
                u, v = sorted((image[a], image[b]))
                mask |= 1 << (v * (v - 1) // 2 + u)
            found.add(mask)
            return
        v = min(vertices, key=lambda u: (available[u].bit_count(), -sub[u].bit_count(), u))
        choices = available[v]
        rest = [u for u in vertices if u != v]
        while choices:
            bit = choices & -choices
            choices ^= bit
            w = bit.bit_length() - 1
            next_domains = available.copy()
            for u in rest:
                domain = available[u] & ~bit
                if sub[v] >> u & 1:
                    domain &= sup[w]
                if not domain:
                    break
                next_domains[u] = domain
            else:
                image[v] = w
                search(rest, next_domains)

    search(active, domains)
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("n", type=int)
    parser.add_argument("candidate", type=int)
    parser.add_argument("host", type=int)
    args = parser.parse_args()
    if args.n < 1:
        parser.error("n must be positive")
    full = (1 << (args.n * (args.n - 1) // 2)) - 1
    red = copies(args.n, args.candidate, args.host)
    blue = copies(args.n, args.candidate, args.host ^ full)
    print(f"red={len(red)}, blue={len(blue)}, total={len(red) + len(blue)}")
    if len(red) + len(blue) <= 20:
        print("red masks:", *sorted(red))
        print("blue masks:", *sorted(blue))


if __name__ == "__main__":
    main()
