"""Run with: python3 -m unittest discover -s scripts -p 'test_*.py'."""

import itertools
import unittest

import orbitals
from refuter_variants import edge_index, graph6, variants


def group_closure(n, generators):
    group = {tuple(range(n))}
    todo = list(group)
    while todo:
        current = todo.pop()
        for generator in generators:
            image = tuple(generator[current[v]] for v in range(n))
            if image not in group:
                group.add(image)
                todo.append(image)
    return group


class OrbitalTests(unittest.TestCase):
    def test_generator_orbits_against_full_groups(self):
        for n in range(2, 7):
            rotation = tuple(range(1, n)) + (0,)
            transposition = (1, 0) + tuple(range(2, n))
            reflection = tuple((-v) % n for v in range(n))
            for generators in ([rotation], [rotation, reflection],
                               [rotation, transposition], []):
                group = group_closure(n, generators)
                expected = set()
                for a, b in itertools.combinations(range(n), 2):
                    expected.add(sum(1 << e for e in {
                        edge_index(p[a], p[b]) for p in group
                    }))
                self.assertEqual(set(orbitals.edge_orbits(n, generators)), expected)

    def test_large_group_not_discarded(self):
        # S_14 has 87 billion elements but just one unordered-pair orbit.
        n = 14
        generators = [(1, 0) + tuple(range(2, n)), tuple(range(1, n)) + (0,)]
        self.assertEqual(orbitals.edge_orbits(n, generators), ((1 << 91) - 1,))

    def test_range_pruning_against_all_subsets(self):
        masks = (0b1, 0b110, 0b111000, 0b1000000)
        for lo in range(8):
            for hi in range(lo, 8):
                expected = {
                    sum(m for i, m in enumerate(masks) if pick >> i & 1)
                    for pick in range(1 << len(masks))
                }
                expected = {g for g in expected if lo <= g.bit_count() <= hi}
                actual = list(orbitals.orbit_unions(masks, lo, hi))
                self.assertEqual(set(actual), expected)
                self.assertEqual(len(actual), len(expected))

    def test_coupled_cycles_allow_matchings(self):
        rotation = (1, 2, 0, 4, 5, 3)
        orbits = orbitals.edge_orbits(6, [rotation])
        matching = sum(1 << edge_index(i, i + 3) for i in range(3))
        self.assertIn(matching, orbits)
        self.assertEqual(len(orbits), 5)
        self.assertIn([rotation], list(orbitals.cyclic_actions(6)))
        self.assertEqual(list(orbitals.cyclic_actions(6, [3, 3])), [[rotation]])
        with self.assertRaises(ValueError):
            list(orbitals.cyclic_actions(6, [3, 2]))

    def test_small_cyclic_family_covers_every_graph(self):
        # Identity is among the cycle types, if its orbit count is permitted.
        self.assertEqual(set(orbitals.generate(4, 0, 6, "cyclic")), set(range(64)))


class VariantTests(unittest.TestCase):
    def test_graph6_roundtrip(self):
        for n in (1, 2, 3, 5, 14, 17, 62):
            length = n * (n - 1) // 2
            full = (1 << length) - 1
            for bits in (0, full, full // 3):
                encoded = graph6(n, bits)
                decoded = 0
                for position in range(length):
                    value = ord(encoded[1 + position // 6]) - 63
                    decoded |= (value >> (5 - position % 6) & 1) << position
                self.assertEqual(decoded, bits)
                self.assertEqual(ord(encoded[0]) - 63, n)
                self.assertEqual(len(encoded), 1 + (length + 5) // 6)

    def test_flips_are_exact_hamming_ball(self):
        for seed in (0, 17, 63):
            self.assertEqual(set(variants(4, seed, "flips", 2)),
                             {g for g in range(64) if (g ^ seed).bit_count() <= 2})

    def test_switches_against_explicit_cuts(self):
        n, seed = 5, 101
        expected = set()
        for subset in range(1 << n):
            cut = sum(1 << edge_index(a, b)
                      for a, b in itertools.combinations(range(n), 2)
                      if (subset >> a & 1) != (subset >> b & 1))
            expected.add(seed ^ cut)
        self.assertEqual(set(variants(n, seed, "switch")), expected)
        self.assertEqual(len(expected), 1 << (n - 1))

    def test_rewire_changes_only_one_star(self):
        n, seed = 4, 17
        stars = [sum(1 << edge_index(v, u) for u in range(n) if u != v)
                 for v in range(n)]
        expected = {g for g in range(64) if any((g ^ seed) & ~s == 0 for s in stars)}
        self.assertEqual(set(variants(n, seed, "rewire")), expected)
        self.assertLessEqual(set(variants(n, seed, "clone")), expected)


if __name__ == "__main__":
    unittest.main()
