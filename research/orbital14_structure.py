#!/usr/bin/env python3
"""Independently reconstruct and check the October 14 S3-family refuter.

The four triples are aligned by index. No external packages or graphy needed.
--emit writes the decimal graph; the structural/verification report is stderr.
"""

import argparse
from collections import Counter
from itertools import combinations
import sys

from refuter15_structure import automorphisms
from verify_refutations import adjacency, contains
from orbitals import edge_orbits

N = 14
CANDIDATE = 482968729600
HOST = 1220745975446480454358073344
CANONICAL = 2117466373777211149778943


def recipe():
    a,b,c,d = (0,1,2),(3,4,5),(6,7,8),(9,10,11)
    edges = list(combinations(c+d,2))  # C union D is K6; A union B is independent.
    edges.extend(zip(a,d))
    edges.extend(zip(b,c))
    for left,right in ((a,c),(b,d)):
        edges.extend((u,v) for i,u in enumerate(left) for j,v in enumerate(right) if i != j)
    edges.extend((12,v) for v in c)
    edges.extend((13,v) for v in a+c+d)
    return sum(1 << (max(u,v)*(max(u,v)-1)//2+min(u,v)) for u,v in edges)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--emit",action="store_true")
    args = parser.parse_args()
    assert recipe() == HOST
    full = (1 << 91)-1
    for host in (HOST,CANONICAL):
        assert not contains(N,CANDIDATE,host)[0]
        assert not contains(N,CANDIDATE,host^full)[0]
    group = automorphisms(N,HOST)
    adj = adjacency(N,HOST)
    degrees = Counter(a.bit_count() for a in adj)
    independent, clique = (0,1,2,3,4,5,12), (6,7,8,9,10,11,13)
    assert all(not adj[u] >> v & 1 for u,v in combinations(independent,2))
    assert all(adj[u] >> v & 1 for u,v in combinations(clique,2))
    assert len(group) == 6
    assert len(edge_orbits(N,group)) == 25
    cyclic_minimum = min(len(edge_orbits(N,[p])) for p in group)
    assert cyclic_minimum == 31
    print(f"Verified n={N} F={CANDIDATE} H={HOST}; {HOST.bit_count()} edges, "
          f"degrees={dict(sorted(degrees.items()))}, |Aut|=6, "
          f"25 full-group pair orbits; minimum cyclic pair orbits={cyclic_minimum}",file=sys.stderr)
    if args.emit:
        print(HOST)


if __name__ == "__main__":
    main()
