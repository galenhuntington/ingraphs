"""Exact edge partitions for orbital SAT; no graph colourings are enumerated.

Syntax: s3:4 = four aligned triples under diagonal S3; c3:4 uses C3;
d4:3 uses D4 on three squares; cycle:4+4+2+2+2 is one cyclic action.
Append /fixed to release all singleton neighbourhoods, or /0+3 to release
explicit vertex labels. Released incident pairs become individual variables.
Family identity is the full sorted edge partition, NOT its descriptive name.
"""

from dataclasses import dataclass
import hashlib
import re

from orbitals import cyclic_actions, edge_orbits, idx


def normalize(n, cells):
    if not 1 <= n <= 16:
        raise ValueError("this launcher supports graphy's default orders 1..16")
    full, covered = (1 << (n * (n - 1) // 2)) - 1, 0
    cells = tuple(sorted(cells))
    for cell in cells:
        if cell <= 0 or cell & ~full or cell & covered:
            raise ValueError("partition cells must be nonzero, disjoint, and fit n")
        covered |= cell
    if covered != full:
        raise ValueError("partition must cover all vertex pairs")
    return cells


def release(n, cells, vertices):
    if any(v < 0 or v >= n for v in vertices):
        raise ValueError("exception vertex outside 0..n-1")
    incident = sum(1 << idx(u, v) for v in range(n) for u in range(v)
                   if u in vertices or v in vertices)
    result = [cell & ~incident for cell in cells if cell & ~incident]
    while incident:
        bit = incident & -incident
        result.append(bit)
        incident ^= bit
    return normalize(n, result)


@dataclass(frozen=True)
class Family:
    n: int
    name: str
    cells: tuple

    @property
    def text(self):
        return "|".join(map(str, self.cells))

    @property
    def id(self):
        return hashlib.sha256(f"orbital-v1:{self.n}:{self.text}".encode()).hexdigest()


def template(n, specification):
    parts = specification.split("/")
    if len(parts) > 2:
        raise ValueError("at most one /exception selector is allowed")
    base = parts[0]
    if base.startswith("cycle:"):
        lengths = list(map(int, base[6:].split("+")))
        generators, = cyclic_actions(n, lengths)
    else:
        match = re.fullmatch(r"([scd])(\d+):(\d+)", base)
        if not match:
            raise ValueError(f"invalid template {base!r}; use s3:4, c3:4, d4:3 or cycle:7+7")
        kind, width, layers = match[1], int(match[2]), int(match[3])
        if width < 2 or layers < 1 or width * layers > n:
            raise ValueError("template needs width >= 2, layers >= 1 and width*layers <= n")
        cycle, swap, reflection = [list(range(n)) for _ in range(3)]
        for start in range(0, width * layers, width):
            for i in range(width):
                cycle[start + i] = start + (i + 1) % width
                reflection[start + i] = start + (-i) % width
            swap[start], swap[start + 1] = start + 1, start
        generators = [cycle] + ([swap] if kind == "s" else [reflection] if kind == "d" else [])
    fixed = {v for v in range(n) if all(g[v] == v for g in generators)}
    cells = edge_orbits(n, generators)
    if len(parts) == 2:
        vertices = fixed if parts[1] == "fixed" else set(map(int, parts[1].split("+")))
        cells = release(n, cells, vertices)
    return Family(n, specification, normalize(n, cells))


def specifications(n, preset):
    if preset in ("triples", "squares"):
        width = 3 if preset == "triples" else 4
        maximum = n // width
        for layers in (maximum, maximum - 1):
            if layers < 1:
                continue
            for kind in ("s", "c"):
                for suffix in ("", "/fixed"):
                    yield f"{kind}{width}:{layers}{suffix}"
        if maximum:
            yield f"s{width}:{maximum}/0"
        if maximum >= 2:
            yield f"s{width}:{maximum}/0+{width}"
    elif preset == "cyclic":
        # Explicit cycle types, including fixed points. No orbit-union enumeration.
        for generators in cyclic_actions(n):
            perm, = generators
            lengths, seen = [], set()
            for v in range(n):
                if v in seen:
                    continue
                length = 0
                while v not in seen:
                    seen.add(v)
                    length += 1
                    v = perm[v]
                lengths.append(length)
            # Put fixed points last, aligning equal-cycle actions with cK:L.
            # Explicit specifications retain their given label order.
            yield "cycle:" + "+".join(map(str, sorted(lengths, reverse=True)))
    else:
        raise ValueError(f"unknown preset {preset}")


def catalog(n, presets, explicit, minimum=0, maximum=64):
    names = [s for p in presets for s in specifications(n, p)] + list(explicit)
    families = {}
    for name in names:
        family = template(n, name)
        if minimum <= len(family.cells) <= maximum:
            families.setdefault(family.cells, family)
    return sorted(families.values(), key=lambda f: (len(f.cells), f.name))
