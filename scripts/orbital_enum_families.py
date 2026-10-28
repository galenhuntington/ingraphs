"""Finite covers of four equal even cycles, using a square-root permutation.

See research/2026-10-27.md for the coverage proof. This is NOT a restriction
to self-complementary graphs: every defect mask is eventually enumerated.
"""

from dataclasses import dataclass
from functools import lru_cache
import hashlib
from itertools import permutations, product
import json
import math

from orbitals import edge_orbits, idx

VERSION = "four-even-cycles-v1"


def permute_mask(mask, permutation):
    result = 0
    while mask:
        bit = mask & -mask
        result |= 1 << permutation[bit.bit_length() - 1]
        mask ^= bit
    return result


@dataclass(frozen=True)
class Cube:
    defect: int
    base: int
    flips: tuple

    @property
    def text(self):
        return "\n".join(map(str, (self.base,) + self.flips)) + "\n"

    @property
    def generated(self):
        return 1 << len(self.flips)


@lru_cache(None)
def geometry(cycle_length):
    # n=8 is a small exhaustive control; n=16 is the production family.
    if cycle_length not in (2, 4):
        raise ValueError("supported cycle lengths are 2 and 4")
    length = 2 * cycle_length
    n = 2 * length
    q = tuple(length * (v // length) + (v % length + 1) % length for v in range(n))
    cells = edge_orbits(n, [q])
    pairs = [(a, b) for b in range(n) for a in range(b)]
    owners = {e: i for i, cell in enumerate(cells) for e in range(len(pairs)) if cell >> e & 1}
    base, halves = 0, []
    for cell in cells:
        a, b = pairs[(cell & -cell).bit_length() - 1]
        sequence, seen = [], set()
        while idx(a, b) not in seen:
            e = idx(a, b)
            sequence.append(e)
            seen.add(e)
            a, b = q[a], q[b]
        assert len(sequence) % 2 == 0
        assert sum(1 << e for e in sequence) == cell
        base |= sum(1 << e for e in sequence[1::2])
        halves.append(sum(1 << e for e in sequence[::2]))

    # A safe SUBGROUP of the full normalizer of q^2. It normalizes q too.
    actions = set()
    units = [u for u in range(length) if math.gcd(u, length) == 1]
    for u, a, b, swap in product(units, range(length), range(length), range(2)):
        p = tuple(length * ((v // length) ^ swap)
                  + (u * (v % length) + (a if v < length else b)) % length for v in range(n))
        action = []
        for cell in cells:
            x, y = pairs[(cell & -cell).bit_length() - 1]
            action.append(owners[idx(p[x], p[y])])
        actions.add(tuple(action))
    return n, q, cells, base, tuple(halves), tuple(sorted(actions))


@lru_cache(None)
def cubes(cycle_length=4):
    _, _, cells, initial, halves, actions = geometry(cycle_length)
    seen, result = set(), []
    full = (1 << len(cells)) - 1
    for defect in range(full + 1):
        if defect in seen:
            continue
        seen.update(permute_mask(defect, p) for p in actions)
        base = initial
        for i, half in enumerate(halves):
            if defect >> i & 1:
                base ^= half
        # Complement flips every orientation. Applying q flips exactly the
        # orientations of nondefective cells. With both kinds present, these
        # operations independently fix one orientation of EACH kind to zero.
        anchors = set()
        for subset in (defect, full ^ defect):
            if subset:
                anchors.add((subset & -subset).bit_length() - 1)
        flips = tuple(cell for i, cell in enumerate(cells) if i not in anchors)
        result.append(Cube(defect, base, flips))
    assert len(seen) == full + 1
    return tuple(sorted(result, key=lambda cube: (cube.defect.bit_count(), cube.defect)))


def manifest(cycle_length=4):
    n, _, _, _, _, _ = geometry(cycle_length)
    plan = {"version": VERSION, "n": n, "cycle_length": cycle_length,
            "cubes": [(c.defect, c.text) for c in cubes(cycle_length)]}
    digest = hashlib.sha256(json.dumps(plan, sort_keys=True).encode()).hexdigest()
    return plan, digest


@lru_cache(None)
def normalizer_coloring_classes(cycle_length=4):
    """Burnside count under the FULL ordinary cyclic normalizer + complement.

    This is an upper bound on graph isomorphism/complement classes, not a
    claim that different normalizer orbits are always nonisomorphic graphs.
    The generator uses a smaller, simpler subgroup of this normalizer.
    """
    geometry(cycle_length)  # Validate the supported orders.
    k, n = cycle_length, 4*cycle_length
    g = tuple(k*(v//k)+(v%k+1)%k for v in range(n))
    cells = edge_orbits(n,[g])
    pairs = [(a,b) for b in range(n) for a in range(b)]
    owner = {e:i for i,c in enumerate(cells) for e in range(len(pairs)) if c>>e&1}
    representatives = [pairs[(c&-c).bit_length()-1] for c in cells]
    actions = set()
    for sigma in permutations(range(4)):
        for shifts in product(range(k),repeat=4):
            for u in range(k):
                if math.gcd(u,k)!=1:
                    continue
                p = tuple(k*sigma[v//k]+(u*(v%k)+shifts[v//k])%k for v in range(n))
                actions.add(tuple(owner[idx(p[a],p[b])] for a,b in representatives))
    fixed = 0
    for p in actions:
        seen,lengths = set(),[]
        for i in range(len(p)):
            if i in seen:
                continue
            j,length = i,0
            while j not in seen:
                seen.add(j)
                j,length = p[j],length+1
            lengths.append(length)
        fixed += 1<<len(lengths)
        if all(length%2==0 for length in lengths):
            fixed += 1<<len(lengths)
    assert fixed % (2*len(actions)) == 0
    return fixed // (2*len(actions))
