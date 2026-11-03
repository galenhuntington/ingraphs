"""Width contracts and wide-graph end-to-end regressions, including ledgers."""

import csv
import io
from itertools import combinations
import json
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import graphy_binary
from graphy_binary import check_orders, require, validate
from orbital_families import normalize, template
from refine_repair import BINARY, select_seeds

SCRIPTS = Path(__file__).resolve().parent


def capabilities(width=128, sat=True):
    n = {64: 11, 128: 16, 256: 23}[width]
    return dict(schema="graphy-capabilities-v1", version="test", bit_width=width,
                max_order=n, max_edges=n*(n-1)//2, copy_count_bits=128,
                sat=sat, nauty=True, native=False, orbital_enum_max_variables=20)


class ProtocolTests(unittest.TestCase):
    def test_schema_types_limits_and_features(self):
        for width in (64, 128, 256):
            data = validate(capabilities(width))
            check_orders(data, [1, data["max_order"]])
            for n in [0, data["max_order"] + 1]:
                with self.assertRaisesRegex(ValueError, "supported range"):
                    check_orders(data, [n])
        for bad in [None, {}, capabilities() | {"schema": "future"},
                    capabilities() | {"sat": "false"}, capabilities() | {"max_order": True},
                    capabilities() | {"max_order": 23}, capabilities() | {"copy_count_bits": 64}]:
            with self.assertRaises(ValueError):
                validate(bad)
        with patch.object(graphy_binary, "_probe", return_value=capabilities(sat=False)):
            with self.assertRaisesRegex(ValueError, "SAT support is absent"):
                require(sys.executable, sat=True)

    def test_old_binary_fails_closed(self):
        graphy_binary._probe.cache_clear()
        with patch.object(graphy_binary.subprocess, "run", side_effect=subprocess.CalledProcessError(2, "capabilities")):
            with self.assertRaisesRegex(ValueError, "rebuild"):
                require(sys.executable)

    def test_probe_cache_is_invalidated_by_file_identity(self):
        graphy_binary._probe.cache_clear()
        response = subprocess.CompletedProcess([], 0, json.dumps(capabilities()), "")
        with patch.object(graphy_binary, "_identity", return_value=(1, 2, 3, 4, 5)) as identity:
            with patch.object(graphy_binary.subprocess, "run", return_value=response) as run:
                require(sys.executable)
                require(sys.executable)
                self.assertEqual(run.call_count, 1)
                identity.return_value = (1, 2, 3, 4, 6)
                require(sys.executable)
                self.assertEqual(run.call_count, 2)
        graphy_binary._probe.cache_clear()

    def test_python_partitions_are_not_limited_to_default_build(self):
        for n in (17, 23):
            cells = template(n, f"cycle:{n}").cells
            self.assertEqual(sum(normalize(n, cells)), (1 << (n*(n-1)//2)) - 1)

    def test_too_narrow_binary_cannot_mutate_existing_wide_ledger(self):
        from sat_ledger import Ledger
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/"wide.sqlite"
            with sqlite3.connect(path) as db:
                db.execute("CREATE TABLE regions(n INTEGER)")
                db.execute("INSERT INTO regions VALUES(17)")
            before = path.read_bytes()
            with patch("sat_ledger.require", return_value=capabilities(128)):
                with self.assertRaisesRegex(ValueError, "supported range"):
                    Ledger(path, sys.executable)
            self.assertEqual(before, path.read_bytes())
            self.assertFalse(Path(str(path) + ".lock").exists())

    def test_order_filter_precedes_native_canonicalization(self):
        catalog = ({(14, 7): {0: 1}, (17, 7): {0: 1}}, set(), {})
        visited = []
        def canon(n, hosts):
            visited.append(n)
            return hosts
        groups = select_seeds([], 1, orders=[14], catalog=catalog, canonicalize=canon)
        self.assertEqual(visited, [14])
        self.assertEqual(set(groups), {(14, 7)})


@unittest.skipUnless(Path(BINARY).exists(), "build graphy for integration tests")
class BinaryTests(unittest.TestCase):
    def launch(self, script, *args, **kwargs):
        return subprocess.run([sys.executable, str(SCRIPTS/script), *map(str, args)],
                              capture_output=True, text=True, timeout=30, **kwargs)

    def test_rejects_unsupported_order_before_creating_ledger(self):
        cap = require(BINARY)
        with tempfile.TemporaryDirectory() as directory:
            db = Path(directory)/"not-created.sqlite"
            for script in ("orbital_sat.py", "sat_repair.py"):
                result = self.launch(script, "--binary", BINARY, "--order", cap["max_order"] + 1,
                                     "--ledger", db, "--plan")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("supported range", result.stderr)
                self.assertFalse(db.exists())
                self.assertFalse(Path(str(db) + ".lock").exists())

    def test_wide_sat_and_vertex_ledger(self):
        cap = require(BINARY)
        if not cap["sat"] or cap["max_order"] < 17:
            self.skipTest("needs u256,sat")
        n, f = 17, 63
        full = (1 << 136) - 1
        squares = {i*i % n for i in range(1, n)}
        host = sum(1 << (v*(v-1)//2 + u) for v in range(n) for u in range(v)
                   if (v-u) % n in squares)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            partition = root/"paley.txt"
            partition.write_text(f"{host}\n{full ^ host}\n")
            db = root/"orbital.sqlite"
            result = self.launch("orbital_sat.py", "--binary", BINARY, "--order", n, "--candidate", f,
                                 "--partition-file", partition, "--ledger", db,
                                 "--batch", 10000, "--seconds", 0, "--rng-seed", 1, check=True)
            row, = csv.DictReader(io.StringIO(result.stdout))
            self.assertEqual(row["status"], "refuted")
            h = int(row["refuter"])
            # Independent K4 verification, not the Rust counting or SAT code.
            for vertices in combinations(range(n), 4):
                mask = sum(1 << (v*(v-1)//2 + u) for u, v in combinations(vertices, 2))
                self.assertNotIn(mask & h, (0, mask))
            self.launch("orbital_sat.py", "--ledger", db, "--status", "--binary", "/nonexistent", check=True)

            seeds = root/"seeds.csv"
            seeds.write_text(f"n,candidate,saved_host,best_copies,status\n17,63,{host},0,checkpoint\n")
            vertex_db = root/"vertex.sqlite"
            result = self.launch("sat_repair.py", seeds, "--binary", BINARY, "--ledger", vertex_db,
                                 "--vertices", 1, "--blocks", 1, "--seconds", 0, check=True)
            rows = list(csv.DictReader(io.StringIO(result.stdout)))
            self.assertTrue(any(r["status"] == "refuted" for r in rows))
            self.launch("sat_repair.py", "--ledger", vertex_db, "--status", "--binary", "/nonexistent", check=True)

    def test_orbital_plan_accepts_more_than_128_variables(self):
        cap = require(BINARY)
        if not cap["sat"] or cap["max_order"] < 17:
            self.skipTest("needs u256,sat")
        with tempfile.TemporaryDirectory() as directory:
            db = Path(directory)/"wide.sqlite"
            self.launch("orbital_sat.py", "--binary", BINARY, "--order", 17, "--candidate", 63,
                        "--template", "cycle:17/" + "+".join(map(str, range(17))),
                        "--min-variables", 136, "--max-variables", 136,
                        "--ledger", db, "--plan", check=True)
            with sqlite3.connect(db) as connection:
                settings, = connection.execute("SELECT settings FROM campaigns").fetchone()
                self.assertEqual(json.loads(settings)["capabilities"]["bit_width"], 256)

    def test_wide_binary_portfolio_and_native_partition(self):
        cap = require(BINARY)
        if not cap["sat"] or cap["max_order"] < 17:
            self.skipTest("needs u256,sat")
        with tempfile.TemporaryDirectory() as directory:
            db = Path(directory)/"binary.sqlite"
            self.launch("orbital_sat.py", "--binary", BINARY, "--order", 17, "--candidate", 1,
                        "--preset", "binary", "--max-variables", 30, "--ledger", db, "--plan", check=True)
            with sqlite3.connect(db) as connection:
                self.assertEqual(connection.execute("SELECT count(*) FROM families").fetchone()[0], 38)
            name = "xor:1.2+4.8+1.4+2.8"
            family = template(17, name)
            self.assertEqual(sum(family.cells).bit_length(), 136)
            result = self.launch("orbital_sat.py", "--binary", BINARY, "--order", 17, "--candidate", 1,
                                 "--template", name, "--ledger", db, "--seconds", 0, check=True)
            row, = csv.DictReader(io.StringIO(result.stdout))
            self.assertEqual(row["status"], "family-unsat-unverified")
            self.assertEqual(row["family_id"], family.id)

    def test_wide_fixed_edge_cnf_units(self):
        cap = require(BINARY)
        if not cap["sat"] or cap["max_order"] < 17:
            self.skipTest("needs u256,sat")
        h = (1 << 135) | (1 << 128) | 1
        with tempfile.TemporaryDirectory() as directory:
            dump = Path(directory)/"cnfs"
            subprocess.run([BINARY, "ingraph-sat", "17", "3", "--seeds", str(h),
                            "--free-edges", "0", "--blocks", "1", "--batch", "1",
                            "--rounds", "0", "--seconds", "0", "--dump-dir", str(dump)],
                           capture_output=True, text=True, check=True, timeout=30)
            units = set()
            for line in (dump/"block-00000.cnf").read_text().splitlines():
                if not line.startswith(("c", "p")):
                    literals = list(map(int, line.split()))
                    if len(literals) == 2:
                        self.assertEqual(literals[-1], 0)
                        units.add(literals[0])
            self.assertEqual(units, {e + 1 if h >> e & 1 else -(e + 1) for e in range(136)})


if __name__ == "__main__":
    unittest.main()
