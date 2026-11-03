"""Elementary-abelian translation actions, without enumerating colourings.

An orbit is specified by independent binary characters (row masks). For
example rows (1, 2, 4) give eight vertices x in F_2^3; group generator j
flips local coordinate i exactly when row[i] has bit j set. Several orbits
share the SAME global generators; unused vertices are fixed individually.

Presets (before the launcher's variable cap):
* klein: every faithful F_2^2 action, up to group/vertex relabelling.
* binary: klein, all faithful F_2^3 actions with <=4 non-singleton orbits,
  and coordinate-projection F_2^4 actions with <=4 non-singleton orbits.
  Rank-four projections are a deliberately bounded subclass, not all actions.

Multisets of quotient character spaces describe actions up to equivariant
vertex relabelling. GL(2,2)/GL(3,2) remove changes of global basis; rank four
uses coordinate permutations only. This prunes equivalent specifications,
not colourings, and makes no graph-isomorphism or universality claim.
"""

from functools import lru_cache
from itertools import combinations, permutations


def span(rows):
    """Bitset of vectors in the binary row span; bit zero is always present."""
    vectors = {0}
    for row in rows:
        vectors |= {v ^ row for v in vectors}
    return sum(1 << v for v in vectors)


def vectors(space):
    return tuple(v for v in range(space.bit_length()) if space >> v & 1)


def basis(space):
    """Deterministic basis, also fixing the preset's local vertex labels."""
    rows, covered = [], 1
    for v in vectors(space):
        if not (covered >> v & 1):
            rows.append(v)
            covered = span(rows)
    return tuple(rows)


def parse_blocks(text):
    try:
        blocks = tuple(tuple(map(int, block.split("."))) for block in text.split("+"))
    except ValueError:
        raise ValueError("xor blocks need dot-separated row masks, e.g. xor:1.2.4+1.2+1") from None
    for rows in blocks:
        if not rows or any(not 1 <= row < 16 for row in rows):
            raise ValueError("xor row masks must be in 1..15 (up to four global bits)")
        if span(rows).bit_count() != 1 << len(rows):
            raise ValueError("xor rows within each block must be linearly independent")
    return blocks


def translation_generators(n, blocks):
    if n < 1 or not blocks:
        raise ValueError("xor action needs positive order and at least one block")
    # Validate programmatic callers too, not just the text parser.
    for rows in blocks:
        if not rows or any(not 1 <= row < 16 for row in rows) or span(rows).bit_count() != 1 << len(rows):
            raise ValueError("invalid independent binary character rows")
    if sum(1 << len(rows) for rows in blocks) > n:
        raise ValueError("xor block sizes exceed the host order")
    rank = max(row.bit_length() for rows in blocks for row in rows)
    generators = [list(range(n)) for _ in range(rank)]
    start = 0
    for rows in blocks:
        size = 1 << len(rows)
        for j, generator in enumerate(generators):
            delta = sum(((row >> j) & 1) << i for i, row in enumerate(rows))
            for x in range(size):
                generator[start + x] = start + (x ^ delta)
        start += size
    return tuple(tuple(g) for g in generators)


@lru_cache(maxsize=None)
def subspaces(rank, coordinate=False):
    result = set()
    choices = tuple(1 << j for j in range(rank)) if coordinate else tuple(range(1, 1 << rank))
    for d in range(1, rank + 1):
        for rows in combinations(choices, d):
            space = span(rows)
            if space.bit_count() == 1 << d:
                result.add(space)
    return tuple(sorted(result))


@lru_cache(maxsize=None)
def changes_of_basis(rank, coordinate=False):
    """Maps on every row-space; only ranks 2/3 use the whole linear group."""
    if rank not in (2, 3, 4) or rank == 4 and not coordinate:
        raise ValueError("full basis enumeration is restricted to ranks 2/3")
    choices = tuple(1 << j for j in range(rank)) if coordinate else tuple(range(1, 1 << rank))
    spaces = subspaces(rank, coordinate)
    maps = []
    for columns in permutations(choices, rank):
        if span(columns).bit_count() != 1 << rank:
            continue
        images = [0] * (1 << rank)
        for v in range(1, 1 << rank):
            low = v & -v
            images[v] = images[v ^ low] ^ columns[low.bit_length() - 1]
        maps.append({s: sum(1 << images[v] for v in vectors(s)) for s in spaces})
    return tuple(maps)


def canonical_spaces(spaces, rank, coordinate=False):
    return min(tuple(sorted(change[s] for s in spaces))
               for change in changes_of_basis(rank, coordinate))


def _multisets(spaces, budget, limit, start=0, prefix=()):
    if prefix:
        yield prefix
    if len(prefix) == limit:
        return
    for i in range(start, len(spaces)):
        size = spaces[i].bit_count()
        if size <= budget:
            yield from _multisets(spaces, budget - size, limit, i, prefix + (spaces[i],))


@lru_cache(maxsize=32)
def action_spaces(n, rank, coordinate=False):
    """One faithful multiset per specified equivalence class, before edge caps."""
    limit = n // 2 if rank == 2 else 4
    full = (1 << (1 << rank)) - 1
    result = []
    for spaces in _multisets(subspaces(rank, coordinate), n, limit):
        if span(row for s in spaces for row in basis(s)) != full:
            continue  # Nonfaithful actions belong to a smaller effective rank.
        if canonical_spaces(spaces, rank, coordinate) == spaces:
            result.append(spaces)
    return tuple(result)


def specification(spaces):
    # Larger vertex orbits first; equal-sized ones ordered by their row space.
    ordered = sorted(spaces, key=lambda s: (-s.bit_count(), s))
    return "xor:" + "+".join(".".join(map(str, basis(s))) for s in ordered)


def specifications(n, preset):
    if preset not in ("klein", "binary"):
        raise ValueError(f"unknown binary preset {preset}")
    yield from (specification(s) for s in action_spaces(n, 2))
    if preset == "binary":
        yield from (specification(s) for s in action_spaces(n, 3))
        yield from (specification(s) for s in action_spaces(n, 4, coordinate=True))


def pair_orbit_count(n, blocks):
    """Independent algebraic count for tests/analysis, without pair traversal.

    One d-dimensional regular orbit has 2^d-1 internal pair orbits (one
    per nonzero XOR difference). Between orbits W,U there are 2^dim(W∩U)
    orbits. Fixed vertices are zero-dimensional orbits of their own.
    """
    spaces = [span(rows) for rows in blocks]
    fixed = n - sum(s.bit_count() for s in spaces)
    if fixed < 0:
        raise ValueError("blocks exceed n")
    spaces += [1] * fixed
    return sum(s.bit_count() - 1 for s in spaces) + sum((a & b).bit_count() for a, b in combinations(spaces, 2))
