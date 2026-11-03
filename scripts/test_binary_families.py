"""Independent action/count checks for the elementary-abelian portfolio."""

from itertools import combinations, product
import unittest

import binary_families as binary
from orbital_families import catalog, normalize, template
from orbitals import idx


def permutation_group(n, generators):
    """Explicit closure, independent of the edge-orbit traversal."""
    group = {tuple(range(n))}
    pending = list(group)
    while pending:
        p = pending.pop()
        for g in generators:
            q = tuple(g[p[v]] for v in range(n))
            if q not in group:
                group.add(q)
                pending.append(q)
    return group


class BinaryFamilyTests(unittest.TestCase):
    def test_aliases_validation_and_exceptions(self):
        self.assertEqual(template(14, "v4:3").id, template(14, "xor:1.2+1.2+1.2").id)
        self.assertEqual(len(template(14, "v4:3").cells), 28)
        self.assertEqual(template(14, "v4:3/fixed").id, template(14, "v4:3/12+13").id)
        self.assertEqual(template(12, "v4:3/fixed").id, template(12, "v4:3").id)
        parent = template(14, "v4:3")
        child = template(14, "v4:3/0+4")
        self.assertGreater(len(child.cells), len(parent.cells))
        self.assertTrue(all(any(c & p == c for p in parent.cells) for c in child.cells))
        for text in ("xor:", "xor:0", "xor:16", "xor:-1", "xor:1.1", "xor:1.2.3",
                     "xor:1++2", "xor:a", "xor:1.2.4.8", "v4:0", "v4:4", "v4:x"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                template(14, text)
        for n, blocks in ((0, [(1,)]), (3, [(1, 2)]), (4, []), (4, [(0,)]),
                          (4, [(16,)]), (4, [(1, 1)])):
            with self.subTest(n=n, blocks=blocks), self.assertRaises(ValueError):
                binary.translation_generators(n, blocks)

    def test_commuting_involutions_and_full_group_pair_partition(self):
        cases = [(14, "1.2+1.2+1+2+3", 4), (14, "1.2.4+1.2+4", 8),
                 (17, "1.2+4.8+1.4+2.8", 16), (23, "1.2.4.8+1.2+3", 16),
                 (6, "1.4+5", 4)]  # A redundant global bit is allowed explicitly.
        for n, text, size in cases:
            blocks = binary.parse_blocks(text)
            generators = binary.translation_generators(n, blocks)
            identity = tuple(range(n))
            for g in generators:
                self.assertEqual(tuple(sorted(g)), identity)
                self.assertEqual(tuple(g[g[v]] for v in range(n)), identity)
                for h in generators:
                    self.assertEqual(tuple(g[h[v]] for v in range(n)), tuple(h[g[v]] for v in range(n)))
            group = permutation_group(n, generators)
            self.assertEqual(len(group), size)
            cells, fixed_pairs = set(), 0
            for u, v in combinations(range(n), 2):
                orbit = {idx(p[u], p[v]) for p in group}
                cells.add(sum(1 << i for i in orbit))
                fixed_pairs += sum({p[u], p[v]} == {u, v} for p in group)
            family = template(n, "xor:" + text)
            self.assertEqual(set(family.cells), cells)
            self.assertEqual(len(cells) * size, fixed_pairs)  # Burnside, unordered pairs.
            self.assertEqual(len(cells), binary.pair_orbit_count(n, blocks))

    def test_klein_catalog_is_complete_up_to_action_isomorphism(self):
        # Every V4 action is a multiset of regular 4-point orbits, three sorts
        # of 2-point orbit, and fixed points. Its automorphisms freely permute
        # those three sorts. This independent profile enumeration is exhaustive.
        for n in range(4, 24):
            expected = set()
            for regular in range(n // 4 + 1):
                for counts in product(range(n // 2 + 1), repeat=3):
                    if tuple(sorted(counts, reverse=True)) != counts:
                        continue
                    if 4 * regular + 2 * sum(counts) > n:
                        continue
                    if not regular and sum(c > 0 for c in counts) < 2:
                        continue  # Otherwise the group action is not faithful.
                    expected.add((regular, counts))
            actual = []
            for spaces in binary.action_spaces(n, 2):
                actual.append((spaces.count(15), tuple(sorted((spaces.count(s) for s in (3, 5, 9)), reverse=True))))
            self.assertEqual(len(actual), len(set(actual)))
            self.assertEqual(set(actual), expected)
        self.assertEqual(len(catalog(14, ["klein"], [], maximum=30)), 6)
        self.assertEqual(len(catalog(14, ["klein"], [], maximum=35)), 18)

    def test_linear_basis_dedup_and_preset_counts(self):
        self.assertEqual(len(binary.subspaces(2)), 4)
        self.assertEqual(len(binary.subspaces(3)), 15)
        self.assertEqual(len(binary.subspaces(4, coordinate=True)), 15)
        for rank, coordinate, count in ((2, False, 6), (3, False, 168), (4, True, 24)):
            changes = binary.changes_of_basis(rank, coordinate)
            spaces = set(binary.subspaces(rank, coordinate))
            self.assertEqual(len(changes), count)
            for change in changes:
                self.assertEqual(set(change), spaces)
                self.assertEqual(set(change.values()), spaces)
                self.assertTrue(all(s.bit_count() == t.bit_count() for s, t in change.items()))
            for action in binary.action_spaces(14, rank, coordinate):
                self.assertEqual(binary.canonical_spaces(action, rank, coordinate), action)
                # Every allowed change of global basis has the same signature.
                changed = tuple(changes[-1][s] for s in action)
                self.assertEqual(binary.canonical_spaces(changed, rank, coordinate), action)
        for n, count in ((14, 130), (17, 189), (23, 374)):
            families = catalog(n, ["binary"], [], maximum=n * (n - 1) // 2)
            self.assertEqual(len(families), count)
            self.assertEqual(len({f.id for f in families}), count)
            for f in families:
                self.assertEqual(normalize(n, f.cells), f.cells)
                self.assertEqual(len(f.cells), binary.pair_orbit_count(n, binary.parse_blocks(f.name[4:])))
            klein = catalog(n, ["klein"], [], maximum=n * (n - 1) // 2)
            self.assertLessEqual({f.id for f in klein}, {f.id for f in families})
        self.assertEqual(catalog(14, ["binary"], [], minimum=28, maximum=30),
                         [f for f in catalog(14, ["binary"], []) if 28 <= len(f.cells) <= 30])

    def test_known_augmented_cube_host_has_rank_four_action(self):
        # Independently construct the n=17 self-complementary refuter model:
        # A(i,a0,a1), B(j,b0,b1), apex. Cross edges satisfy a_j=b_i.
        vertices = [(side, block, x) for side in (0, 1) for block in (0, 1) for x in range(4)] + [(2, 0, 0)]
        host = 0
        for u, v in combinations(range(17), 2):
            side, i, a = vertices[u]
            other, j, b = vertices[v]
            if other == 2:
                edge = side == 1
            elif side == other:
                edge = (i != j) if side == 0 else (i == j)
            else:
                edge = (a >> j & 1) == (b >> i & 1)
            if edge:
                host |= 1 << idx(u, v)
        name = "xor:1.2+4.8+1.4+2.8"
        family = template(17, name)
        self.assertEqual(host.bit_count(), 68)
        self.assertEqual(len(family.cells), 26)
        self.assertTrue(all(host & c in (0, c) for c in family.cells))
        # Explicit label map back to the previously independently verified
        # canonical host (research/refuter17_structure.py).
        labels = (3, 1, 2, 0, 16, 14, 15, 13, 4, 5, 10, 11, 6, 7, 8, 9, 12)
        relabelled = sum(1 << idx(labels[u], labels[v]) for u, v in combinations(range(17), 2)
                         if host >> idx(u, v) & 1)
        self.assertEqual(relabelled, 338986856409490997648489224340590907136)
        spaces = tuple(binary.span(rows) for rows in binary.parse_blocks(name[4:]))
        canonical = binary.canonical_spaces(spaces, 4, coordinate=True)
        self.assertIn(canonical, binary.action_spaces(17, 4, coordinate=True))
        self.assertIn(binary.specification(canonical), {f.name for f in catalog(17, ["binary"], [])})


if __name__ == "__main__":
    unittest.main()
