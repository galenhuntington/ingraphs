"""Coverage, native-worker, and restart/bookkeeping tests for finite enumeration."""

from collections import Counter
import csv
import io
import os
from pathlib import Path
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest

from orbital_enum import Ledger, parse_cull, parse_worker
from orbital_enum_families import cubes, geometry, manifest, normalizer_coloring_classes, permute_mask
from orbital_families import template
from refine_repair import BINARY

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "research"))
from verify_refutations import contains

SCRIPT = str(Path(__file__).with_name("orbital_enum.py"))
HAS_ENUM = Path(BINARY).exists() and "orbital-enum" in subprocess.run(
    [BINARY, "--help"], capture_output=True, text=True).stdout


class CoverTests(unittest.TestCase):
    def test_complete_defect_partition_and_counts(self):
        plan = cubes(4)
        expected = [1,4,17,42,98,176,285,370,414,370,285,176,98,42,17,4,1]
        self.assertEqual(Counter(c.defect.bit_count() for c in plan), dict(enumerate(expected)))
        self.assertEqual(len(plan), 2400)
        self.assertEqual(sum(c.generated for c in plan), 39354368)
        self.assertEqual(normalizer_coloring_classes(4), 954508)
        self.assertEqual({len(c.flips) for c in plan}, {14,15})
        for length in (2,4):
            n,q,cells,_,_,actions = geometry(length)
            covered = set()
            for cube in cubes(length):
                orbit = {permute_mask(cube.defect,p) for p in actions}
                self.assertFalse(orbit & covered)
                covered.update(orbit)
                self.assertEqual(cube.defect, min(orbit))
                self.assertLess(cube.base, 1 << (n*(n-1)//2))
                union = 0
                for mask in cube.flips:
                    self.assertFalse(mask & union)
                    union |= mask
            self.assertEqual(covered, set(range(1 << len(cells))))
        self.assertEqual(manifest(4)[1],manifest(4)[1])
        self.assertNotEqual(manifest(2)[1],manifest(4)[1])

    def test_worker_and_cull_reject_partial_or_mismatched_output(self):
        self.assertEqual(parse_worker("orbital-enum-v1,4,2,1,0.1\n0\n",4,2),([0],0.1))
        for output in ("", "orbital-enum-v1,5,2,1,0.1\n0\n",
                       "orbital-enum-v1,4,2,2,0.1\n0\n",
                       "orbital-enum-v1,4,2,2,0.1\n0\n0\n",
                       "orbital-enum-v1,4,2,1,nan\n0\n",
                       "orbital-enum-v1,4,2,1,0.1\n64\n"):
            with self.assertRaises(ValueError):
                parse_worker(output,4,2)
        self.assertEqual(parse_cull("7,None\n63,Some(42)\n",["7","63"],{42}),{"7":None,"63":"42"})
        for output in ("7,None\n", "7,None\n63,Some(43)\n", "7,None\n7,None\n", "7,maybe\n63,None\n"):
            with self.assertRaises(ValueError):
                parse_cull(output,["7","63"],{42})

    @unittest.skipUnless(Path(BINARY).exists(),"needs a binary to fingerprint")
    def test_separate_schema_lock_and_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/"enumeration.sqlite"
            ledger = Ledger(path,2,BINARY)
            with self.assertRaisesRegex(RuntimeError,"already in use"):
                Ledger(path,2,BINARY)
            with ledger.db:
                ledger.db.execute("INSERT INTO campaigns VALUES('stale',0,NULL,'running','{}')")
            ledger.close()
            ledger = Ledger(path,2,BINARY)
            self.assertEqual(ledger.db.execute("SELECT state FROM campaigns").fetchone()[0],"aborted")
            ledger.close()
            with self.assertRaisesRegex(ValueError,"differs"):
                Ledger(path,4,BINARY)
            changed = Path(directory)/"different-worker"
            changed.write_bytes(b"different binary fingerprint")
            with self.assertRaisesRegex(ValueError,"binary_sha256 differs"):
                Ledger(path,2,str(changed))
            other = Path(directory)/"sat.sqlite"
            with sqlite3.connect(other) as db:
                db.execute("CREATE TABLE meta(key TEXT,value TEXT)")
                db.execute("INSERT INTO meta VALUES('schema','orbital-sat-v1')")
            before = other.read_bytes()
            with self.assertRaisesRegex(ValueError,"not an orbital enumeration"):
                Ledger(other,2,BINARY)
            self.assertEqual(other.read_bytes(),before)


@unittest.skipUnless(HAS_ENUM,"build graphy --release with the enumeration worker")
class EnumerationCliTests(unittest.TestCase):
    def launch(self, directory, *options):
        return subprocess.run([sys.executable,SCRIPT,"--ledger",str(Path(directory)/"ledger.sqlite"),
                               "--cycle-length","2",*options],capture_output=True,text=True,check=True,timeout=60)

    def native(self, directory, n, base, flips):
        path=Path(directory)/"cube.txt"
        path.write_text("\n".join(map(str,[base,*flips]))+"\n")
        run=subprocess.run([BINARY,"orbital-enum",str(n),str(path)],capture_output=True,text=True,check=True,timeout=30)
        return parse_worker(run.stdout,n,1<<len(flips))[0]

    def test_small_complete_family_matches_unreduced_enumeration(self):
        with tempfile.TemporaryDirectory() as directory:
            # Independent, ordinary C2^4 orbit-union enumeration: all 2^16
            # assignments, not the square-root/defect reduction under test.
            expected=set(self.native(directory,8,0,template(8,"cycle:2+2+2+2").cells))
            self.launch(directory,"--jobs","3","--generate-only")
            actual=set(map(int,self.launch(directory,"--export-library").stdout.splitlines()))
            self.assertEqual(actual,expected)
            self.assertLessEqual(len(actual),normalizer_coloring_classes(2))
            with sqlite3.connect(Path(directory)/"ledger.sqlite") as db:
                self.assertEqual(db.execute("SELECT sum(complete),count(*) FROM batches").fetchone(),(60,60))

    def test_partial_generation_check_resume_and_added_candidates(self):
        with tempfile.TemporaryDirectory() as directory:
            self.launch(directory,"--candidate","7","--max-batches","1","--plan")
            first=self.launch(directory,"--max-batches","1","--max-check-chunks","0")
            row,=csv.DictReader(io.StringIO(first.stdout))
            self.assertEqual(row["status"],"partial")
            self.assertEqual(row["checked_hosts"],"0")
            full=self.launch(directory,"--jobs","2","--max-check-chunks","0")
            row,=csv.DictReader(io.StringIO(full.stdout))
            self.assertEqual(row["completed_batches"],"60")
            self.assertEqual(row["status"],"partial")
            limited=self.launch(directory,"--max-batches","0","--check-chunk","1","--max-check-chunks","1")
            row,=csv.DictReader(io.StringIO(limited.stdout))
            self.assertEqual(row["checked_hosts"],"1")
            self.assertEqual(row["status"],"partial")
            completed=self.launch(directory,"--max-batches","0")
            row,=csv.DictReader(io.StringIO(completed.stdout))
            self.assertEqual(row["status"],"family-excluded-by-enumeration")
            with sqlite3.connect(Path(directory)/"ledger.sqlite") as db:
                before=db.execute("SELECT defect,completed_at FROM batches ORDER BY defect").fetchall()
            added=self.launch(directory,"--candidate","63")
            rows={int(r["candidate"]):r for r in csv.DictReader(io.StringIO(added.stdout))}
            self.assertEqual(rows[7]["status"],"family-excluded-by-enumeration")
            self.assertEqual(rows[63]["status"],"refuted")
            h=int(rows[63]["refuter"])
            self.assertFalse(contains(8,63,h)[0])
            self.assertFalse(contains(8,63,h^((1<<28)-1))[0])
            self.assertEqual(added.stdout,self.launch(directory,"--export-csv").stdout)
            with sqlite3.connect(Path(directory)/"ledger.sqlite") as db:
                self.assertEqual(before,db.execute("SELECT defect,completed_at FROM batches ORDER BY defect").fetchall())
                self.assertEqual(db.execute("SELECT checked_through FROM candidates WHERE bits='63'").fetchone()[0],
                                 db.execute("SELECT id FROM hosts WHERE bits=?",(str(h),)).fetchone()[0])

    def test_known_sixteen_vertex_positive_control(self):
        with tempfile.TemporaryDirectory() as directory:
            proc=self.launch(directory,"--cycle-length","4","--max-defects","0",
                             "--candidate","108544687478829056")
            row,=csv.DictReader(io.StringIO(proc.stdout))
            self.assertEqual(row["status"],"refuted")
            self.assertEqual(row["total_hosts"],"669")
            h=int(row["refuter"])
            self.assertFalse(contains(16,108544687478829056,h)[0])
            self.assertFalse(contains(16,108544687478829056,h^((1<<120)-1))[0])

    def test_invalid_arguments_and_readonly_missing_file(self):
        with tempfile.TemporaryDirectory() as directory:
            for options in (["--jobs","0"],["--max-batches","-1"],["--check-chunk","0"],
                            ["--min-defects","4","--max-defects","3"],["--candidate",str(1<<28)],
                            ["--status"]):
                with self.assertRaises(subprocess.CalledProcessError):
                    self.launch(directory,*options)
            self.assertFalse((Path(directory)/"ledger.sqlite").exists())

    @unittest.skipUnless(sys.platform.startswith("linux"),"uses /proc for child audit")
    def test_sigterm_reaps_children_and_resume_skips_committed_batches(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/"ledger.sqlite"
            process=subprocess.Popen([sys.executable,SCRIPT,"--ledger",str(path),"--generate-only","--jobs","2"],
                                     stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            children=set()
            try:
                until=time.monotonic()+10
                while not children and process.poll() is None and time.monotonic()<until:
                    for entry in Path(f"/proc/{process.pid}/task").glob("*/children"):
                        children.update(map(int,entry.read_text().split()))
                    time.sleep(0.01)
                self.assertTrue(children)
                process.terminate()
                self.assertEqual(process.wait(timeout=10),130)
                for pid in children:
                    with self.assertRaises(ProcessLookupError): os.kill(pid,0)
                with sqlite3.connect(path) as db:
                    before=db.execute("SELECT defect,completed_at FROM batches WHERE complete=1").fetchall()
                    self.assertEqual(db.execute("SELECT count(*) FROM campaigns WHERE state='running'").fetchone()[0],0)
                self.launch(directory,"--cycle-length","4","--max-batches","1","--generate-only")
                with sqlite3.connect(path) as db:
                    after=db.execute("SELECT defect,completed_at FROM batches WHERE complete=1").fetchall()
                    self.assertEqual(len(after),len(before)+1)
                    self.assertTrue(set(before)<=set(after))
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=10)


if __name__ == "__main__":
    unittest.main()
