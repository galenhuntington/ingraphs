"""End-to-end checks for prepared matching, library reuse and the n=18 regression."""

import csv
from functools import lru_cache
import io
from itertools import permutations
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

from graphy_binary import require
from refine_repair import BINARY


@lru_cache(maxsize=None)
def labelled_copies(n, graph):
    edges = [(a, b) for b in range(n) for a in range(b)
             if graph >> (b * (b - 1) // 2 + a) & 1]
    result = set()
    for permutation in permutations(range(n)):
        copy = 0
        for a, b in edges:
            u, v = sorted((permutation[a], permutation[b]))
            copy |= 1 << (v * (v - 1) // 2 + u)
        result.add(copy)
    return result


def refutes(n, candidate, host):
    full = (1 << (n * (n - 1) // 2)) - 1
    return not any(copy & colour == copy
                   for copy in labelled_copies(n, candidate)
                   for colour in (host, full ^ host))


@unittest.skipUnless(Path(BINARY).exists(), "build graphy for matcher CLI tests")
class MatcherCliTests(unittest.TestCase):
    def command(self, *args, data=None, threads=1):
        return subprocess.run([BINARY, *map(str, args)], input=data, text=True,
                              capture_output=True, check=True, timeout=30,
                              env=dict(os.environ, RAYON_NUM_THREADS=str(threads)))

    def test_bulk_cull_and_seed_seek_against_independent_permutations(self):
        n = 4
        candidates = list(range(64)) + list(reversed(range(64)))
        hosts = [0, 1, 7, 11, 12, 30, 45, 63]
        expected = [(candidate, next((h for h in hosts if refutes(n, candidate, h)), None))
                    for candidate in candidates]
        data = ''.join(f'{candidate}\n' for candidate in candidates)
        with tempfile.TemporaryDirectory() as directory:
            library = Path(directory) / 'hosts.txt'
            library.write_text(''.join(f'{h}\n' for h in hosts))
            for threads in (1, 4):
                result = self.command('cull', n, '/dev/stdin', library, data=data, threads=threads)
                actual = [(int(candidate), None if hit == 'None' else int(hit[5:-1]))
                          for candidate, hit in csv.reader(io.StringIO(result.stdout))]
                self.assertEqual(actual, expected)
            # The adaptive timing order may choose a different first witness;
            # survivor decisions must agree, and every returned witness is checked.
            result = self.command('ingraph-seek', n, '/dev/stdin', '--seed-file', library,
                                  '--bailout', 0, '--expiration', 0, data=data)
            lines = result.stdout.splitlines()
            self.assertEqual(len(lines), len(expected))
            for line, (candidate, hit) in zip(lines, expected):
                self.assertEqual(int(line.split(',')[0]), candidate)
                match = re.search(r'\((?:Some\((\d+)\)|None), \d+\)', line)
                self.assertIsNotNone(match)
                actual = int(match[1]) if match[1] else None
                self.assertEqual(actual is None, hit is None)
                if actual is not None:
                    self.assertIn(actual, hosts)
                    self.assertTrue(refutes(n, candidate, actual))

    def test_expiration_still_removes_prepared_library_entries(self):
        for expiration in (0, 1):
            result = self.command('ingraph-seek', 4, '/dev/stdin', '--seeds', 45,
                                  '--bailout', 0, '--expiration', expiration, data='0\n0\n7\n')
            last = result.stdout.splitlines()[-1]
            if expiration == 0:
                self.assertIn('(Some(45), 0)', last)
            else:
                self.assertIn('(None, 1)', last)

    def test_mixed_inferred_orders(self):
        self.assertEqual(self.command('is-subgraph', 7, '/', 1).stdout.strip(), 'false')
        self.assertEqual(self.command('is-subgraph', 1, '/', 7).stdout.strip(), 'true')

    def test_eighteen_vertex_regression(self):
        if require(BINARY)['max_order'] < 18:
            self.skipTest('requires u256')
        candidate = 1329268563080305633988669656996007944
        host = 615220134502691169874691659634381667188743
        complement = ((1 << 153) - 1) ^ host
        self.assertEqual(self.command('is-subgraph', candidate, '/', host).stdout.strip(), 'false')
        self.assertEqual(self.command('is-subgraph', candidate, '/', complement).stdout.strip(), 'true')
        # A root packing rejection in one colour is NOT a two-colour refutation.
        with tempfile.TemporaryDirectory() as directory:
            library = Path(directory) / 'host.txt'
            library.write_text(f'{host}\n')
            result = self.command('cull', 18, '/dev/stdin', library, data=f'{candidate}\n')
            self.assertEqual(result.stdout.strip(), f'{candidate},None')


if __name__ == '__main__':
    unittest.main()
