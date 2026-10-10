"""Region-ledger persistence, import validation, sharding and resume checks."""

import csv
import io
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest

from refine_repair import BINARY
from sat_ledger import Ledger, fixed_graph
from test_sat_repair import has_sat

SCRIPT = str(Path(__file__).with_name("sat_repair.py"))


@unittest.skipUnless(has_sat(), "build graphy --release --features sat")
class LedgerTests(unittest.TestCase):
    def native(self, n=5, candidate=7, seed=0, vertices=1, rounds=10000):
        result = subprocess.run([BINARY, "ingraph-sat", str(n), str(candidate), "--seeds", str(seed),
                                 "--block-mode", "vertex", "--vertices", str(vertices), "--blocks", "100",
                                 "--seconds", "0", "--rounds", str(rounds)], capture_output=True, text=True, check=True)
        return list(csv.DictReader(io.StringIO(result.stdout)))

    def csv_file(self, path, rows):
        with open(path, "w", newline="") as out:
            writer = csv.DictWriter(out, fieldnames=list(rows[0]))
            writer.writeheader()
            writer.writerows(rows)

    def launch(self, directory, *options):
        result = subprocess.run([sys.executable, SCRIPT, "--ledger", str(Path(directory)/"regions.sqlite"),
                                 *options], capture_output=True, text=True, check=True)
        return list(csv.DictReader(io.StringIO(result.stdout))), result.stderr

    def test_import_unsat_dominance_complements_idempotence_and_lock(self):
        row, = self.native()
        self.assertEqual(row['status'], 'block-unsat')
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'old.csv'
            # Deliberately stale fixed_core is ignored: recompute from the
            # actual anchor, vertices and fixed assignments.
            timeout = row | {'start_host': '1023', 'status': 'time-limit', 'fixed_core': '999999999'}
            prescribed = row | {'block_mode': 'Prescribed', 'free_vertices': '', 'fixed_core': ''}
            self.csv_file(path, [row, timeout, prescribed])
            ledger = Ledger(str(Path(directory)/'regions.sqlite'), BINARY)
            try:
                ledger.import_csv(path)
                ledger.import_csv(path)
                region, = ledger.db.execute('SELECT * FROM regions').fetchall()
                self.assertEqual((region['state'], region['attempts']), ('unsat', 3))
                with self.assertRaisesRegex(RuntimeError, 'already in use'):
                    Ledger(str(Path(directory)/'regions.sqlite'), BINARY)
                self.assertIn('1 unsat regions', self.launch(directory, '--status')[1])
                # Binary changes re-key, preserving terminal status.
                with ledger.db:
                    ledger.db.execute("UPDATE regions SET core='63'")  # complementary old representative
                    ledger.db.execute("UPDATE meta SET value='different build' WHERE key='binary'")
            finally:
                ledger.close()
            ledger = Ledger(str(Path(directory)/'regions.sqlite'), BINARY)
            self.assertEqual(ledger.db.execute('SELECT state FROM regions').fetchone()[0], 'unsat')
            self.assertEqual(ledger.db.execute('SELECT core FROM regions').fetchone()[0], '0')
            ledger.close()

    def test_order_candidate_and_fixed_order_are_all_in_identity(self):
        rows = (self.native(n=5, candidate=7, vertices=1, rounds=0)
                + self.native(n=5, candidate=3, vertices=1, rounds=0)
                + self.native(n=5, candidate=7, vertices=2, rounds=0)
                + self.native(n=6, candidate=7, vertices=2, rounds=0))
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'orders.csv'
            self.csv_file(path, rows)
            ledger = Ledger(str(Path(directory)/'regions.sqlite'), BINARY)
            try:
                ledger.import_csv(path)
                self.assertEqual(ledger.db.execute('SELECT count(*) FROM regions').fetchone()[0], 4)
                self.assertEqual({r[0] for r in ledger.db.execute('SELECT state FROM regions')}, {'unresolved'})
            finally:
                ledger.close()

    def test_import_rolls_back_bad_masks_bad_witnesses_and_truncation(self):
        row, = self.native()
        hit, = self.native(seed=613, vertices=2)
        with tempfile.TemporaryDirectory() as directory:
            ledger = Ledger(str(Path(directory)/'regions.sqlite'), BINARY)
            try:
                for bad in [row | {'free_edges': '0'}, hit | {'refuter': '0'}]:
                    path = Path(directory)/'bad.csv'
                    self.csv_file(path, [row, bad])
                    with self.assertRaises((ValueError, RuntimeError)):
                        ledger.import_csv(path)
                    self.assertEqual(ledger.db.execute('SELECT count(*) FROM regions').fetchone()[0], 0)
                path = Path(directory)/'truncated.csv'
                self.csv_file(path, [row])
                with open(path, 'a') as out:
                    out.write('5,7,broken\n')
                with self.assertRaisesRegex(ValueError, 'malformed CSV'):
                    ledger.import_csv(path)
                self.assertEqual(ledger.db.execute('SELECT count(*) FROM imports').fetchone()[0], 0)
                self.csv_file(path, [hit])
                ledger.import_csv(path)
                self.assertEqual(ledger.db.execute('SELECT host FROM refutations').fetchone()[0], '613')
            finally:
                ledger.close()

    def test_plan_parallel_new_retry_caps_and_order_filters(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory)/'seeds.csv'
            source.write_text('n,candidate,saved_host,best_copies,status\n6,7,0,20,time-limit\n'
                              '6,7,21,30,time-limit\n6,7,1000,40,time-limit\n')
            rows, _ = self.launch(directory, str(source), '--keep', '3', '--vertices', '2', '--blocks', '0', '--plan')
            self.assertEqual(rows, [])
            with sqlite3.connect(Path(directory)/'regions.sqlite') as db:
                total, = db.execute('SELECT count(*) FROM regions').fetchone()
            self.assertGreater(total, 3)
            options = ['--vertices', '2', '--rounds', '0', '--jobs', '3', '--chunk-size', '1', '--rng-seed', '99']
            rows, _ = self.launch(directory, *options, '--blocks', '3')
            self.assertEqual(len(rows), 3)  # not 3 times worker count
            first_ids = {r['region_id'] for r in rows}
            self.assertEqual(len(first_ids), 3)
            self.assertEqual(len({r['campaign_id'] for r in rows}), 1)
            saved = Path(directory)/'own.csv'
            self.csv_file(saved, rows)
            self.launch(directory, '--import-only', '--import-csv', str(saved))
            with sqlite3.connect(Path(directory)/'regions.sqlite') as db:
                self.assertEqual(db.execute('SELECT sum(attempts) FROM regions').fetchone()[0], 3)
            later, _ = self.launch(directory, *options, '--blocks', '0')
            self.assertEqual(len(later), total - 3)
            self.assertFalse(first_ids & {r['region_id'] for r in later})
            self.assertEqual(self.launch(directory, *options, '--blocks', '0')[0], [])
            retry, _ = self.launch(directory, *options, '--blocks', '2', '--work', 'retry')
            self.assertEqual(len(retry), 2)
            self.assertTrue(all(r['status'] == 'round-limit' for r in retry))
            self.assertEqual(self.launch(directory, *options, '--work', 'retry', '--order', '5')[0], [])
            bad = subprocess.run([sys.executable, SCRIPT, '--ledger', str(Path(directory)/'regions.sqlite'),
                                  '--restarts', '2'], capture_output=True)
            self.assertNotEqual(bad.returncode, 0)

    def test_exact_region_file_relabels_and_rejects_duplicate_classes(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'regions.txt'
            path.write_text('0,1\n613,3\n')
            cmd = [BINARY, 'ingraph-sat', '5', '7', '--block-mode', 'vertex', '--vertices', '1',
                   '--region-file', str(path), '--relabel', '--reset-every', '1', '--seconds', '0']
            result = subprocess.run(cmd, capture_output=True, text=True, check=True)
            rows = list(csv.DictReader(io.StringIO(result.stdout)))
            self.assertEqual([r['status'] for r in rows], ['block-unsat', 'refuted'])
            for row in rows:
                _, _, free = fixed_graph(5, int(row['start_host']), int(row['free_vertices']))
                self.assertEqual(free, int(row['free_edges']))
            path.write_text('0,1\n1023,2\n')
            self.assertNotEqual(subprocess.run(cmd, capture_output=True).returncode, 0)

    def test_refutation_stops_future_work_for_candidate(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory)/'seeds.csv'
            source.write_text('n,candidate,saved_host,best_copies,status\n6,63,0,15,time-limit\n'
                              '6,63,21,8,time-limit\n6,63,1000,1,time-limit\n')
            rows, _ = self.launch(directory, str(source), '--vertices', '2', '--blocks', '0',
                                  '--jobs', '3', '--chunk-size', '1', '--keep', '3')
            self.assertTrue(any(r['status'] == 'refuted' for r in rows))
            rows, _ = self.launch(directory, '--vertices', '2', '--blocks', '0', '--work', 'all')
            self.assertEqual(rows, [])

    @unittest.skipUnless(sys.platform.startswith('linux'), 'uses /proc')
    def test_interrupt_reaps_children_and_keeps_unfinished_work_eligible(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory)/'seeds.csv'
            source.write_text('n,candidate,saved_host,best_copies,status\n14,105760090929024,0,100,time-limit\n')
            db_path = Path(directory)/'regions.sqlite'
            cmd = [sys.executable, SCRIPT, str(source), '--ledger', str(db_path), '--vertices', '14',
                   '--blocks', '0', '--jobs', '1', '--seconds', '300']
            process = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
            children = set()
            try:
                deadline = time.monotonic() + 10
                while time.monotonic() < deadline:
                    if db_path.exists():
                        with sqlite3.connect(db_path) as db:
                            try:
                                running = db.execute("SELECT count(*) FROM campaigns WHERE status='running'").fetchone()[0]
                            except sqlite3.OperationalError:
                                running = 0
                        if running:
                            for path in Path(f'/proc/{process.pid}/task').glob('*/children'):
                                children.update(map(int, path.read_text().split()))
                            if children:
                                break
                    time.sleep(.02)
                self.assertTrue(children)
                process.terminate()
                process.communicate(timeout=10)
                self.assertEqual(process.returncode, 130)
                for pid in children:
                    with self.assertRaises(ProcessLookupError):
                        os.kill(pid, 0)
                with sqlite3.connect(db_path) as db:
                    self.assertEqual(db.execute('SELECT status FROM campaigns').fetchone()[0], 'interrupted')
                    self.assertEqual(db.execute('SELECT state FROM regions').fetchone()[0], 'new')
            finally:
                if process.poll() is None:
                    process.kill()
                    process.communicate(timeout=10)


if __name__ == '__main__':
    unittest.main()
