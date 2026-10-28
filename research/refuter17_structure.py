#!/usr/bin/env python3
"""Reconstruct/analyse the October 27 n=17 refuter; no external graph library.

--verify independently checks the two refutations (potentially slower).
--emit-orbit-unions emits all 1024 colourings of its full-group pair-orbits,
in decimal, including complements/duplicates. Pipe to a u256-enabled checker.
"""

import argparse
from collections import Counter, deque
from itertools import combinations
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/"scripts"))
from orbitals import edge_orbits, idx
from refuter15_structure import automorphisms, cycles
from verify_refutations import adjacency, contains

HOST = 338986856409490997648489224340590907136
CANDIDATES = (221541319920208212992, 221613518320445584384)

# A(i,a0,a1), B(j,b0,b1). The unusual order reproduces the user's least-
# decimal labels, rather than merely an isomorphic copy.
A = {(i,a0,a1): v for i,vs in enumerate(((3,2,1,0),(16,15,14,13)))
     for (a0,a1),v in zip(((0,0),(0,1),(1,0),(1,1)),vs)}
B = {(j,b0,b1): v for j,vs in enumerate(((4,10,5,11),(6,8,7,9)))
     for (b0,b1),v in zip(((0,0),(0,1),(1,0),(1,1)),vs)}
APEX = 12


def recipe():
    edges = set()
    for (x,v),(y,w) in combinations(A.items(),2):
        if x[0] != y[0]:
            edges.add(idx(v,w))
    for (x,v),(y,w) in combinations(B.items(),2):
        if x[0] == y[0]:
            edges.add(idx(v,w))
    for (i,*a),v in A.items():
        for (j,*b),w in B.items():
            if a[j] == b[i]:
                edges.add(idx(v,w))
    edges.update(idx(APEX,v) for v in B.values())
    return sum(1 << e for e in edges)


def complementing_permutation():
    p=list(range(17))
    for key,v in A.items():
        p[v]=B[key]
    for (j,b0,b1),v in B.items():
        p[v]=A[j,1-b0,1-b1]
    return tuple(p)


def cube_labels(adj):
    left,right=set(A.values()),set(B.values())
    vertices=sorted(left|right)
    cross={v:[w for w in vertices if (v in left)!=(w in left) and adj[v]>>w&1] for v in vertices}

    def distances(root):
        distance,queue={root:0},deque([root])
        while queue:
            v=queue.popleft()
            for w in cross[v]:
                if w not in distance:
                    distance[w]=distance[v]+1
                    queue.append(w)
        return distance

    origin=A[0,0,0]
    d=distances(origin)
    basis=[distances(v) for v in cross[origin]]
    labels={v:sum((basis[i][v]==d[v]-1)<<i for i in range(4)) for v in vertices}
    assert len(set(labels.values()))==16
    assert all(((labels[v]^labels[w]).bit_count()==1)==(w in cross[v]) for v in vertices for w in vertices)
    return labels


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verify",action="store_true")
    parser.add_argument("--emit-orbit-unions",action="store_true")
    args=parser.parse_args()
    say=lambda *values: print(*values,file=sys.stderr,flush=True)
    assert recipe()==HOST
    adj=adjacency(17,HOST)
    assert {a.bit_count() for a in adj}=={8}
    anti=complementing_permutation()
    assert all((adj[v]>>w&1)!=(adj[anti[v]]>>anti[w]&1) for v in range(17) for w in range(v))
    group=automorphisms(17,HOST)
    orbits=edge_orbits(17,group)
    assert len(group)==64 and len(orbits)==10
    assert all(HOST&cell in (0,cell) for cell in orbits)
    say("Host",HOST,"edges",HOST.bit_count(),"degrees",dict(Counter(a.bit_count() for a in adj)))
    say("Automorphisms",len(group),"pair-orbits",len(orbits),"minimum cyclic pair-orbits",
        min(len(edge_orbits(17,[p])) for p in group))
    say("Complementing permutation",cycles(anti))
    say("Cross edges form Q4; binary cube labels",{v:f"{label:04b}" for v,label in cube_labels(adj).items()})
    say("Adjacent common-neighbour counts",dict(Counter((adj[v]&adj[w]).bit_count()
        for v in range(17) for w in range(v) if adj[v]>>w&1)))
    if args.verify:
        full=(1<<136)-1
        for candidate in CANDIDATES:
            red,red_nodes=contains(17,candidate,HOST)
            blue,blue_nodes=contains(17,candidate,HOST^full)
            assert not red and not blue
            say("Verified",candidate,"search nodes",red_nodes+blue_nodes)
    if args.emit_orbit_unions:
        host=0
        print(host)
        for step in range(1,1<<len(orbits)):
            host^=orbits[(step&-step).bit_length()-1]
            print(host)


if __name__=="__main__":
    main()
