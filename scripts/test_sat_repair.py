"""Independent small-instance and streaming CLI checks for the optional SAT path."""

import csv
import io
from itertools import permutations
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import unittest

from refine_repair import BINARY

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "research"))
from count_copies import copies
from verify_refutations import contains


def has_sat():
    if not Path(BINARY).exists():
        return False
    return "ingraph-sat" in subprocess.run([BINARY, "--help"], capture_output=True, text=True).stdout


@unittest.skipUnless(has_sat(), "build graphy --release --features sat for SAT CLI tests")
class SatCliTests(unittest.TestCase):
    def run_sat(self, n, f, *options):
        proc = subprocess.run([BINARY, "ingraph-sat", str(n), str(f), "--seconds", "0", *options],
                              text=True, capture_output=True, check=True)
        rows = list(csv.DictReader(io.StringIO(proc.stdout)))
        for row in rows:
            self.assertNotIn(None, row)
            self.assertEqual(row["solver"], "cadical-2.2.1")
            if row["best_copies"]:
                h = int(row["saved_host"])
                full = (1 << (n * (n - 1) // 2)) - 1
                self.assertEqual(int(row["best_copies"]), len(copies(n, f, h)) + len(copies(n, f, h ^ full)))
        return rows

    def test_triangle_control_and_zero_rounds(self):
        row, = self.run_sat(5, 7, "--seeds", "0", "--blocks", "1", "--block-size", "0")
        self.assertEqual(row["status"], "refuted")
        host = int(row["refuter"])
        self.assertFalse(contains(5, 7, host)[0])
        self.assertFalse(contains(5, 7, host ^ 1023)[0])
        row, = self.run_sat(6, 7, "--seeds", "0", "--blocks", "1", "--block-size", "0")
        self.assertEqual(row["status"], "unrestricted-unsat-unverified")
        self.assertEqual(row["refuter"], "")
        row, = self.run_sat(5, 7, "--seeds", "0", "--blocks", "1", "--block-size", "0", "--rounds", "0")
        self.assertEqual(row["status"], "round-limit")

    def test_prescribed_block_and_frozen_anchor(self):
        # Red/blue C5 is a refuter; damage one edge, then free exactly that edge.
        row, = self.run_sat(5, 7, "--seeds", "612", "--blocks", "1", "--free-edges", "1")
        self.assertEqual((row["status"], row["refuter"], row["block_mode"]), ("refuted", "613", "Prescribed"))
        row, = self.run_sat(5, 7, "--seeds", "612", "--blocks", "1", "--free-edges", "0")
        self.assertEqual(row["status"], "block-unsat")

    def test_dumped_cnf_matches_block_outcomes_and_only_contains_valid_constraints(self):
        # P3 is universal on four vertices; every run should exhaust its block.
        n, f = 4, 3
        all_copies = set()
        for p in permutations(range(n)):
            mask = 0
            for b in range(n):
                for a in range(b):
                    if f >> (b * (b - 1) // 2 + a) & 1:
                        u, v = sorted((p[a], p[b]))
                        mask |= 1 << (v * (v - 1) // 2 + u)
            all_copies.add(mask)
        with tempfile.TemporaryDirectory() as directory:
            rows = self.run_sat(n, f, "--seeds", "0,21", "--blocks", "4", "--block-size", "3",
                                "--dump-dir", directory, "--batch", "1")
            self.assertEqual(len(rows), 4)
            for row in rows:
                text = (Path(directory) / f"block-{int(row['block']):05}.cnf").read_text()
                clauses = []
                for line in text.splitlines():
                    if line.startswith("p "):
                        _, _, variables, count = line.split()
                        self.assertEqual(int(variables), 6)
                    elif not line.startswith("c "):
                        literals = list(map(int, line.split()))
                        self.assertEqual(literals.pop(), 0)
                        if len(literals) > 1:
                            self.assertTrue(all(lit > 0 for lit in literals) or all(lit < 0 for lit in literals))
                            mask = sum(1 << (abs(lit) - 1) for lit in literals)
                            self.assertIn(mask, all_copies)
                        else:
                            lit, = literals
                            edge = 1 << (abs(lit) - 1)
                            self.assertFalse(int(row["free_edges"]) & edge)
                            self.assertEqual(bool(int(row["start_host"]) & edge), lit > 0)
                        clauses.append(literals)
                self.assertEqual(len(clauses), int(count))
                satisfiable = any(all(any(bool(h & (1 << (abs(lit) - 1))) == (lit > 0)
                                               for lit in clause) for clause in clauses) for h in range(64))
                self.assertFalse(satisfiable)
                self.assertEqual(row["status"], "block-unsat")
                self.assertEqual(row["refuter"], "")

    def test_parallel_frontend_selects_candidate_specific_seeds(self):
        seed_csv = "n,candidate,saved_host,best_copies,status\n6,7,0,20,time-limit\n6,3,0,60,time-limit\n"
        script = str(Path(__file__).with_name("sat_repair.py"))
        command = [sys.executable, script, "/dev/stdin", "--candidate", "7", "--keep", "1",
                   "--restarts", "2", "--jobs", "2", "--blocks", "2", "--block-size", "4",
                   "--seconds", "0", "--rng-seed", "42"]
        proc = subprocess.run(command, input=seed_csv, text=True, capture_output=True, check=True)
        rows = list(csv.DictReader(io.StringIO(proc.stdout)))
        self.assertEqual(len(rows), 4)
        self.assertEqual({row["candidate"] for row in rows}, {"7"})
        self.assertEqual({row["restart"] for row in rows}, {"0", "1"})
        self.assertTrue(all(row["status"] == "block-unsat" for row in rows))
        bad = subprocess.run(command + ["--seconds", "nan"], input=seed_csv, text=True, capture_output=True)
        self.assertNotEqual(bad.returncode, 0)

    @unittest.skipUnless(sys.platform.startswith("linux"), "uses /proc to audit child cleanup")
    def test_sigterm_stops_active_solver_children(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "seed.csv"
            source.write_text("n,candidate,saved_host,best_copies,status\n6,7,0,20,time-limit\n")
            command = [sys.executable, str(Path(__file__).with_name("sat_repair.py")), str(source),
                       "--restarts", "1", "--jobs", "1", "--blocks", "100000",
                       "--block-size", "4", "--seconds", "0"]
            process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
            children = set()
            try:
                self.assertTrue(select.select([process.stdout], [], [], 10)[0], "helper did not emit output")
                self.assertTrue(process.stdout.readline().startswith("n,candidate,"))
                for path in Path(f"/proc/{process.pid}/task").glob("*/children"):
                    children.update(map(int, path.read_text().split()))
                self.assertTrue(children, "expected an active graphy worker")
                process.terminate()
                process.communicate(timeout=10)
                self.assertEqual(process.returncode, 130)
                for pid in children:
                    with self.assertRaises(ProcessLookupError):
                        os.kill(pid, 0)
            finally:
                if process.poll() is None:
                    process.kill()
                    process.communicate(timeout=10)
                for pid in children:
                    try:
                        os.kill(pid, signal.SIGTERM)
                    except ProcessLookupError:
                        pass


if __name__ == "__main__":
    unittest.main()
