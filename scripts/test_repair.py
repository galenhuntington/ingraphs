"""Tests for repair-result selection and the independent audit counter."""

import csv
import io
from itertools import permutations
from pathlib import Path
import sys
import subprocess
import tempfile
import unittest

from refine_repair import BINARY, canonical_keys, cross_refutations, select_seeds, tasks

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
            # 100 and 101 represent the same class; dedup must precede keep.
            keys = lambda n, hosts: [100 if h == 101 else h for h in hosts]
            self.assertEqual(select_seeds([path], 2, canonicalize=keys), {(14, 10): [100, 102]})
            self.assertEqual(select_seeds([path], 2, max_score=3, canonicalize=keys), {(14, 10): [100]})
            self.assertEqual(select_seeds([path], 2, max_score=2, canonicalize=keys), {})
            self.assertEqual(select_seeds([path], 2, candidates={12}, canonicalize=keys), {})

    def test_individual_restart_tasks_are_round_robin(self):
        groups = {(14, 10): [100], (14, 11): [101]}
        self.assertEqual([(key, restart) for key, _, restart in tasks(groups, 2, 5)],
                         [((14, 10), 5), ((14, 11), 5), ((14, 10), 6), ((14, 11), 6)])

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


@unittest.skipUnless(Path(BINARY).exists(), "build target/release/graphy for CLI integration tests")
class RepairCliTests(unittest.TestCase):
    def test_refiner_schedules_restarts_and_carries_input_seeds(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "input.csv"
            path.write_text("n,candidate,saved_host,best_copies,status\n6,7,0,20,time-limit\n6,7,7,11,time-limit\n")
            result = subprocess.run([sys.executable, str(Path(__file__).with_name("refine_repair.py")),
                                     str(path), "--keep", "2", "--restarts", "4", "--jobs", "2",
                                     "--seconds", "0", "--steps", "10", "--archive", "2"],
                                    text=True, capture_output=True, check=True)
            rows = list(csv.DictReader(io.StringIO(result.stdout)))
            self.assertEqual(sorted(int(r["restart"]) for r in rows if r["record"] == "outcome"), list(range(4)))
            self.assertEqual({int(r["saved_host"]) for r in rows if r["record"] == "seed"}, {0, 7})
            self.assertTrue(all(not r["refuter"] for r in rows))
            # stdin is non-seekable, just like the user's <(grep ...) inputs.
            streamed = subprocess.run([sys.executable, str(Path(__file__).with_name("refine_repair.py")),
                                       "/dev/stdin", "--keep", "2", "--restarts", "1", "--jobs", "1",
                                       "--seconds", "0", "--steps", "0", "--archive", "0"],
                                      input=path.read_text(), text=True, capture_output=True, check=True)
            streamed_rows = list(csv.DictReader(io.StringIO(streamed.stdout)))
            self.assertEqual(sum(r["record"] == "seed" for r in streamed_rows), 2)

    def test_internal_complement_keys_and_cross_checks(self):
        self.assertEqual(len(set(canonical_keys(BINARY, 5, [1, 4, 1022, 1019]))), 1)
        self.assertEqual(cross_refutations(BINARY, 5, [3, 7], [613]), {7: 613})

    def test_refiner_shares_a_seed_before_scheduling_a_refuted_target(self):
        result = subprocess.run([sys.executable, str(Path(__file__).with_name("refine_repair.py")),
                                 "/dev/stdin", "--restarts", "1", "--jobs", "1", "--steps", "0",
                                 "--seconds", "0", "--archive", "0"],
                                input="n,candidate,saved_host,best_copies,status\n5,3,613,10,time-limit\n5,7,0,10,time-limit\n",
                                text=True, capture_output=True, check=True)
        rows = list(csv.DictReader(io.StringIO(result.stdout)))
        hits = [r for r in rows if r["status"] == "refuted"]
        self.assertEqual([(r["candidate"], r["refuter"], r["record"]) for r in hits],
                         [("7", "613", "cross-refutation")])
        self.assertFalse(any(r["candidate"] == "7" and r["record"] == "outcome" for r in rows))

    def test_separate_restart_scheduling_preserves_streams(self):
        common = [BINARY, "ingraph-repair", "6", "/dev/stdin", "--seeds", "0,32767,15",
                  "--random-every", "3", "--steps", "10", "--seconds", "0", "--rng-seed", "42",
                  "--archive", "0", "--moves", "focused"]

        def run(offset, count):
            result = subprocess.run(common + ["--restart-offset", str(offset), "--restarts", str(count)],
                                    input="7\n", capture_output=True, text=True, check=True)
            rows = list(csv.DictReader(io.StringIO(result.stdout)))
            for row in rows:
                del row["seconds"], row["oracle_seconds"]
            return rows

        self.assertEqual(run(0, 6), [run(r, 1)[0] for r in range(6)])

    def test_archived_scores_are_real_and_never_certificates(self):
        result = subprocess.run([BINARY, "ingraph-repair", "6", "/dev/stdin", "--seeds", "0",
                                 "--restarts", "1", "--random-every", "0", "--steps", "40",
                                 "--seconds", "0", "--archive", "8", "--pair-every", "1"],
                                input="7\n", text=True, capture_output=True, check=True)
        rows = list(csv.DictReader(io.StringIO(result.stdout)))
        self.assertGreater(len(rows), 1)
        keys = canonical_keys(BINARY, 6, [int(row["saved_host"]) for row in rows])
        self.assertEqual(len(keys), len(set(keys)))
        for row in rows:
            h = int(row["saved_host"])
            self.assertEqual(int(row["best_copies"]), len(copies(6, 7, h)) + len(copies(6, 7, h ^ 32767)))
            self.assertFalse(row["refuter"])
            if row["record"] == "checkpoint":
                self.assertEqual(row["status"], "checkpoint")
                self.assertEqual(float(row["seconds"]), 0)


if __name__ == "__main__":
    unittest.main()
