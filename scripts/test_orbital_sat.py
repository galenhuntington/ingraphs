"""Independent projection/CNF checks plus durable orbital campaign tests."""

import csv
import io
from itertools import permutations
import os
from pathlib import Path
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest

from orbital_families import Family, catalog, normalize, release, template
from orbital_sat import Ledger, SCHEMA
from refine_repair import BINARY

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "research"))
from verify_refutations import contains

SCRIPT = str(Path(__file__).with_name("orbital_sat.py"))
HAS_ORBITAL = Path(BINARY).exists() and "ingraph-orbital-sat" in subprocess.run(
    [BINARY, "--help"], capture_output=True, text=True).stdout


class FamilyTests(unittest.TestCase):
    def test_counts_and_exception_relaxation(self):
        for n, name, variables in [(14,"s3:4",25), (14,"s3:3",34), (15,"s3:4",31),
                                   (16,"s3:4",38), (16,"s3:5",30), (15,"c3:4",37),
                                   (14,"s3:4/fixed",41), (16,"s3:5/fixed",40)]:
            family = template(n, name)
            self.assertEqual(len(family.cells), variables)
            normalize(n, family.cells)
        original = template(14, "s3:4")
        expanded = template(14, "s3:4/0+3")
        self.assertTrue(all(any(cell & parent == cell for parent in original.cells) for cell in expanded.cells))
        self.assertEqual(len(release(4, [63], set(range(4)))), 6)

    def test_identity_and_catalog(self):
        a = template(15, "c3:5")
        b = template(15, "cycle:3+3+3+3+3")
        self.assertEqual(a.id, b.id)
        self.assertEqual(template(15,"s3:5").id, template(15,"s3:5/fixed").id)
        self.assertNotEqual(template(15,"s3:5").id, template(15,"s3:5/0").id)
        # The cyclic preset uses large cycles first, so the overlapping c3:4
        # aligned action is exactly deduplicated even with fixed points.
        cyclic = catalog(14,["cyclic"],[],0,64)
        self.assertIn(template(14,"c3:4").cells,{f.cells for f in cyclic})
        for n in (14,15,16):
            families = catalog(n,["triples","squares"],[],0,64)
            self.assertEqual(len(families),len({f.cells for f in families}))
            self.assertTrue(all(len(f.cells) <= 64 for f in families))
        for cells in ([1,3,4], [1,2], [1,2,8], [0,7]):
            with self.assertRaises(ValueError):
                normalize(3,cells)
        for name in ("s3:5", "s1:3", "s3:0", "cycle:3+3", "s3:4/14", "s3:4/fixed/0"):
            with self.assertRaises(ValueError):
                template(14,name)

    def test_separate_schema_lock_and_stale_attempt_recovery(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/"ledger.sqlite"
            ledger = Ledger(path)
            ledger.register([template(5,"cycle:5")], {7})
            with self.assertRaisesRegex(RuntimeError,"already in use"):
                Ledger(path)
            with ledger.db:
                ledger.db.execute("INSERT INTO attempts VALUES('stale',1,'old',0,NULL,'running','{}',NULL)")
            ledger.close()
            ledger = Ledger(path)
            self.assertEqual(ledger.db.execute("SELECT state FROM attempts").fetchone()[0],"aborted")
            self.assertEqual(ledger.db.execute("SELECT state FROM tasks").fetchone()[0],"new")
            ledger.close()
            other = Path(directory)/"vertex.sqlite"
            with sqlite3.connect(other) as db:
                db.execute("CREATE TABLE meta(key TEXT,value TEXT)")
                db.execute("INSERT INTO meta VALUES('schema','1')")
            before = other.read_bytes()
            with self.assertRaisesRegex(ValueError,"not an orbital"):
                Ledger(other)
            self.assertEqual(other.read_bytes(),before)


@unittest.skipUnless(HAS_ORBITAL,"build graphy --release --features sat")
class OrbitalCliTests(unittest.TestCase):
    def native(self, directory, n, f, cells, *options):
        path = Path(directory)/"partition.txt"
        path.write_text("".join(f"{cell}\n" for cell in cells))
        proc = subprocess.run([BINARY,"ingraph-orbital-sat",str(n),str(f),"--partition",str(path),
                               "--seconds","0",*options],capture_output=True,text=True,check=True)
        row, = csv.DictReader(io.StringIO(proc.stdout))
        self.assertNotIn(None,row)
        return row

    def launch(self, directory, *options):
        proc = subprocess.run([sys.executable,SCRIPT,"--ledger",str(Path(directory)/"ledger.sqlite"),*options],
                              capture_output=True,text=True,check=True)
        return list(csv.DictReader(io.StringIO(proc.stdout))),proc.stderr

    def test_positive_triangle_and_refinement_recovers_excluded_family(self):
        with tempfile.TemporaryDirectory() as directory:
            # With every pair tied, no host avoids K3; with a C5 action, one does.
            row = self.native(directory,5,7,[1023])
            self.assertEqual(row["status"],"family-unsat-unverified")
            row = self.native(directory,5,7,template(5,"cycle:5").cells)
            self.assertEqual(row["status"],"refuted")
            h = int(row["refuter"])
            self.assertFalse(contains(5,7,h)[0])
            self.assertFalse(contains(5,7,h^1023)[0])
            row = self.native(directory,6,7,template(6,"cycle:6").cells,"--rounds","0")
            self.assertEqual(row["status"],"round-limit")

    def test_dump_is_sound_projected_formula_with_full_witnesses(self):
        n,f,cells = 4,3,(1,6,56)
        copies = set()
        for p in permutations(range(n)):
            copies.add(sum(1 << (max(p[u],p[v])*(max(p[u],p[v])-1)//2+min(p[u],p[v]))
                           for v in range(n) for u in range(v) if f >> (v*(v-1)//2+u)&1))
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/"clauses.cnf"
            row = self.native(directory,n,f,cells,"--dump-cnf",str(path),"--batch","1")
            self.assertEqual(row["status"],"family-unsat-unverified")
            clauses, witness = [],None
            for line in path.read_text().splitlines():
                if line.startswith("p "):
                    _,_,variables,count = line.split()
                    self.assertEqual(int(variables),len(cells))
                elif line.startswith("c witness "):
                    witness = int(line.split()[-1])
                    self.assertIn(witness,copies)
                elif not line.startswith("c "):
                    clause = list(map(int,line.split()))
                    self.assertEqual(clause.pop(),0)
                    self.assertEqual({abs(lit)-1 for lit in clause},{i for i,c in enumerate(cells) if c&witness})
                    clauses.append(clause)
            self.assertEqual(len(clauses),int(count))
            self.assertFalse(any(all(any(bool(a & (1 << (abs(lit)-1))) == (lit>0) for lit in c)
                                        for c in clauses) for a in range(1<<len(cells))))
            with self.assertRaises(subprocess.CalledProcessError):
                self.native(directory,n,f,cells,"--dump-cnf",str(path))

    def test_native_rejects_invalid_partitions(self):
        with tempfile.TemporaryDirectory() as directory:
            for cells in ([1,3,4],[1,2],[1,2,8]):
                with self.assertRaises(subprocess.CalledProcessError):
                    self.native(directory,3,3,cells)

    def test_plan_new_retry_resume_terminal_and_export(self):
        with tempfile.TemporaryDirectory() as directory:
            options = ["--order","6","--candidate","7","--template","cycle:6","--template","s3:2",
                       "--jobs","2","--seconds","0","--rng-seed","77"]
            rows,_ = self.launch(directory,*options,"--plan")
            self.assertEqual(rows,[])
            rows,_ = self.launch(directory,*options,"--rounds","0")
            self.assertEqual(len(rows),2)
            self.assertTrue(all(r["status"]=="round-limit" for r in rows))
            self.assertEqual(self.launch(directory,*options)[0],[])
            retried,_ = self.launch(directory,*options,"--work","retry")
            self.assertEqual(len(retried),2)
            self.assertTrue(all(r["status"]=="family-unsat-unverified" for r in retried))
            self.assertEqual({r["task_id"]:r["walk_seed"] for r in rows},
                             {r["task_id"]:r["walk_seed"] for r in retried})
            self.assertEqual(self.launch(directory,*options,"--work","all")[0],[])
            exported,_ = self.launch(directory,"--export-csv")
            self.assertEqual(len(exported),4)
            self.assertEqual({r["attempt_id"] for r in exported},{r["attempt_id"] for r in rows+retried})
            with sqlite3.connect(Path(directory)/"ledger.sqlite") as db:
                self.assertEqual(db.execute("SELECT sum(attempts) FROM tasks").fetchone()[0],4)
                self.assertEqual(db.execute("SELECT count(*) FROM campaigns WHERE state='plan'").fetchone()[0],1)
            self.assertIn("unsat: 2 tasks",self.launch(directory,"--status")[1])

    def test_found_candidate_skips_other_templates_and_invalid_budgets(self):
        with tempfile.TemporaryDirectory() as directory:
            options = ["--order","5","--candidate","7","--template","cycle:5","--template","s2:1",
                       "--jobs","1","--seconds","0"]
            rows,_ = self.launch(directory,*options)
            self.assertEqual(sum(r["status"]=="refuted" for r in rows),1)
            self.assertEqual(self.launch(directory,*options,"--work","all")[0],[])
            for extra in (["--seconds","nan"],["--jobs","0"],["--min-variables","10","--max-variables","5"]):
                with self.assertRaises(subprocess.CalledProcessError):
                    self.launch(directory,*options,*extra)

    @unittest.skipUnless(sys.platform.startswith("linux"),"uses /proc for child audit")
    def test_sigterm_reaps_worker_and_leaves_task_eligible(self):
        with tempfile.TemporaryDirectory() as directory:
            options = ["--order","14","--candidate","208095050752","--template","s3:4/0+3",
                       "--seconds","600","--jobs","1"]
            proc = subprocess.Popen([sys.executable,SCRIPT,"--ledger",str(Path(directory)/"ledger.sqlite"),*options],
                                    stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            children = set()
            try:
                until = time.monotonic()+10
                while not children and time.monotonic()<until and proc.poll() is None:
                    for path in Path(f"/proc/{proc.pid}/task").glob("*/children"):
                        children.update(map(int,path.read_text().split()))
                    time.sleep(0.01)
                self.assertTrue(children,"expected active graphy child")
                proc.terminate()
                self.assertEqual(proc.wait(timeout=10),130)
                for pid in children:
                    with self.assertRaises(ProcessLookupError):
                        os.kill(pid,0)
                rows,_ = self.launch(directory,*options,"--rounds","0")
                self.assertEqual(len(rows),1)
                with sqlite3.connect(Path(directory)/"ledger.sqlite") as db:
                    self.assertEqual(db.execute("SELECT count(*) FROM attempts WHERE state='running'").fetchone()[0],0)
            finally:
                if proc.poll() is None:
                    proc.kill()
                    proc.wait(timeout=10)
                for pid in children:
                    try: os.kill(pid,signal.SIGTERM)
                    except ProcessLookupError: pass


if __name__ == "__main__":
    unittest.main()
