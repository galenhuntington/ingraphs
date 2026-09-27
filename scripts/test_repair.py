"""Tests for repair-result selection and the independent audit counter."""

import csv
from itertools import permutations
from pathlib import Path
import sys
import tempfile
import unittest

from refine_repair import select_seeds

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "research"))
from count_copies import copies


class RepairTests(unittest.TestCase):
    def test_selects_best_distinct_hosts_and_skips_refuted_candidates(self):
        fields = ["n", "candidate", "saved_host", "best_copies", "status"]
        rows = [(14, 10, 100, 7, "time-limit"),
                (14, 10, 101, 4, "time-limit"),
                (14, 10, 100, 3, "time-limit"),
                (14, 10, 102, 9, "time-limit"),
                (14, 11, 100, 5, "time-limit"),
                (14, 11, 103, 0, "refuted"),
                (14, 12, 104, "", "time-limit")]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "results.csv"
            with path.open("w", newline="") as output:
                writer = csv.writer(output)
                writer.writerow(fields)
                writer.writerows(rows)
            self.assertEqual(select_seeds([path], 2), {(14, 10): [100, 101]})
            self.assertEqual(select_seeds([path], 2, max_score=3), {(14, 10): [100]})
            self.assertEqual(select_seeds([path], 2, max_score=2), {})

    def test_independent_counter_matches_permutation_edge_sets(self):
        def relabel(pattern, permutation):
            result = 0
            for b in range(5):
                for a in range(b):
                    if pattern >> (b * (b - 1) // 2 + a) & 1:
                        u, v = sorted((permutation[a], permutation[b]))
                        result |= 1 << (v * (v - 1) // 2 + u)
            return result

        for pattern in [0, 3, 7, 15, 31, 45, 613, 1023]:
            all_copies = {relabel(pattern, p) for p in permutations(range(5))}
            for host in [0, 7, 235, 613, 771, 1023]:
                expected = {copy for copy in all_copies if copy & host == copy}
                self.assertEqual(copies(5, pattern, host), expected)


if __name__ == "__main__":
    unittest.main()
