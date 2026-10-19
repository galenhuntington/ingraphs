#!/usr/bin/env python3
"""Resumable candidate-directed SAT over orbital families and exceptions.

No seeds, graph enumeration, or fixed colours. This is a SEPARATE ledger from
sat_repair.py: vertex-region results cannot be imported as family exclusions.
Completed outcomes are committed before CSV emission. Retry starts a fresh
solver, not a checkpoint. Family UNSAT is not a universality certificate.
"""

import argparse
from collections import Counter, deque
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
import csv
import fcntl
import hashlib
import io
import json
import math
import os
from pathlib import Path
import random
import shlex
import shutil
import signal
import sqlite3
import subprocess
import sys
import tempfile
from threading import Event, Lock
import time
import uuid

from orbital_families import Family, catalog, normalize
from refine_repair import BINARY

SCHEMA = "orbital-sat-v1"
UNSAT = "family-unsat-unverified"
STATUSES = {UNSAT, "refuted", "time-limit", "sat-limit", "round-limit", "constraint-limit"}
FIELDS = ("n,candidate,refuter,status,variables,partition,walk_seed,last_host,last_copies,"
          "rounds,checks,witnesses,constraints,seconds,sat_seconds,oracle_seconds,conflicts,"
          "decisions,propagations,solver,factor,family_id,family,task_id,campaign_id,attempt_id").split(",")


def log(message):
    print(message, file=sys.stderr, flush=True)


def file_digest(path):
    with open(path, "rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


class Ledger:
    def __init__(self, path):
        self.path = Path(path).resolve()
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.lock = open(str(self.path) + ".lock", "a")
        try:
            fcntl.flock(self.lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            self.lock.close()
            raise RuntimeError("orbital ledger already in use; use one launcher with --jobs") from None
        self.db = sqlite3.connect(self.path)
        self.db.row_factory = sqlite3.Row
        try:
            tables = {r[0] for r in self.db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
            if tables:
                schema = self.db.execute("SELECT value FROM meta WHERE key='schema'").fetchone() if "meta" in tables else None
                if schema is None or schema[0] != SCHEMA:
                    raise ValueError("not an orbital SAT ledger; do NOT use the vertex-region ledger here")
            self.db.execute("PRAGMA journal_mode=WAL")
            self.db.execute("PRAGMA synchronous=FULL")
            self.db.executescript("""
                CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS families(
                    id TEXT PRIMARY KEY, n INTEGER NOT NULL, name TEXT NOT NULL,
                    cells TEXT NOT NULL, UNIQUE(n,cells));
                CREATE TABLE IF NOT EXISTS tasks(
                    id INTEGER PRIMARY KEY, family_id TEXT NOT NULL, candidate TEXT NOT NULL,
                    state TEXT NOT NULL DEFAULT 'new', attempts INTEGER NOT NULL DEFAULT 0,
                    seconds REAL NOT NULL DEFAULT 0, last_status TEXT,
                    UNIQUE(family_id,candidate));
                CREATE TABLE IF NOT EXISTS campaigns(
                    id TEXT PRIMARY KEY, started REAL, ended REAL, state TEXT, settings TEXT);
                CREATE TABLE IF NOT EXISTS attempts(
                    id TEXT PRIMARY KEY, task_id INTEGER, campaign_id TEXT, started REAL,
                    ended REAL, state TEXT, settings TEXT, outcome TEXT);
                CREATE TABLE IF NOT EXISTS refutations(
                    n INTEGER, candidate TEXT, host TEXT, attempt_id TEXT, PRIMARY KEY(n,candidate));
            """)
            with self.db:
                self.db.execute("INSERT OR IGNORE INTO meta VALUES('schema',?)", (SCHEMA,))
                # The exclusive launcher lock makes stale 'running' records safe
                # to recover; their tasks were never marked completed or claimed.
                self.db.execute("UPDATE attempts SET state='aborted',ended=? WHERE state='running'", (time.time(),))
                self.db.execute("UPDATE campaigns SET state='aborted',ended=? WHERE state='running'", (time.time(),))
        except BaseException:
            self.close()
            raise

    def close(self):
        self.db.close()
        self.lock.close()

    def register(self, families, candidates):
        with self.db:
            for family in families:
                old = self.db.execute("SELECT n,cells FROM families WHERE id=?", (family.id,)).fetchone()
                if old is not None and tuple(old) != (family.n, family.text):
                    raise ValueError("family digest collision")
                self.db.execute("INSERT OR IGNORE INTO families VALUES(?,?,?,?)",
                                (family.id, family.n, family.name, family.text))
                self.db.executemany("INSERT OR IGNORE INTO tasks(family_id,candidate) VALUES(?,?)",
                                    [(family.id, str(f)) for f in candidates])

    def eligible(self, families, candidates, work):
        selected = {family.id for family in families}
        states = {"new"} if work == "new" else {"unresolved"} if work == "retry" else {"new", "unresolved"}
        hits = {(r[0], int(r[1])) for r in self.db.execute("SELECT n,candidate FROM refutations")}
        return [dict(r) for r in self.db.execute("""
            SELECT t.*, f.n, f.name, f.cells FROM tasks t JOIN families f ON f.id=t.family_id
        """) if r["family_id"] in selected and int(r["candidate"]) in candidates
                and r["state"] in states and (r["n"], int(r["candidate"])) not in hits]

    def complete(self, row):
        state = "unsat" if row["status"] == UNSAT else "refuted" if row["status"] == "refuted" else "unresolved"
        with self.db:
            self.db.execute("UPDATE attempts SET state='complete',ended=?,outcome=? WHERE id=? AND state='running'",
                            (time.time(), json.dumps(row, sort_keys=True), row["attempt_id"]))
            self.db.execute("UPDATE tasks SET state=?,attempts=attempts+1,seconds=seconds+?,last_status=? WHERE id=?",
                            (state, float(row["seconds"]), row["status"], row["task_id"]))
            if state == "refuted":
                self.db.execute("INSERT OR IGNORE INTO refutations VALUES(?,?,?,?)",
                                (int(row["n"]), row["candidate"], row["refuter"], row["attempt_id"]))


def read_only(args):
    uri = Path(args.ledger).resolve().as_uri() + "?mode=ro"
    with sqlite3.connect(uri, uri=True) as db:
        if db.execute("SELECT value FROM meta WHERE key='schema'").fetchone() != (SCHEMA,):
            raise ValueError("not an orbital SAT ledger")
        if args.export_csv:
            writer = csv.DictWriter(sys.stdout, fieldnames=FIELDS)
            writer.writeheader()
            for (outcome,) in db.execute("SELECT outcome FROM attempts WHERE state='complete' ORDER BY rowid"):
                writer.writerow(json.loads(outcome))
        else:
            for row in db.execute("""SELECT f.n,t.state,count(*),sum(t.attempts),sum(t.seconds)
                                     FROM tasks t JOIN families f ON f.id=t.family_id
                                     GROUP BY f.n,t.state ORDER BY f.n,t.state"""):
                log(f"n={row[0]} {row[1]}: {row[2]} tasks, {row[3]} completed attempts, {row[4]:.1f} worker seconds")
            for n, f, h in db.execute("SELECT n,candidate,host FROM refutations"):
                log(f"REFUTED n={n} F={f} H={h}")


def validate_row(row, task):
    if None in row or any(v is None for v in row.values()):
        raise ValueError("malformed orbital SAT CSV")
    n, candidate = task["n"], int(task["candidate"])
    cells = tuple(map(int, task["cells"].split("|"))) if task["cells"] else ()
    if (int(row["n"]), int(row["candidate"]), row["partition"], int(row["variables"])) != (n, candidate, task["cells"], len(cells)):
        raise ValueError("worker returned a different candidate or partition")
    if row["status"] not in STATUSES or bool(row["refuter"]) != (row["status"] == "refuted"):
        raise ValueError("invalid orbital outcome")
    for key in ("seconds", "sat_seconds", "oracle_seconds"):
        if not math.isfinite(float(row[key])) or float(row[key]) < 0:
            raise ValueError("invalid timing")
    host = int(row["last_host"])
    if not 0 <= host < 1 << (n * (n - 1) // 2) or any(host & cell not in (0, cell) for cell in cells):
        raise ValueError("worker graph is outside the family")
    if row["refuter"] and (int(row["refuter"]) != host or row["last_copies"] != "0"):
        raise ValueError("inconsistent refutation")


def run_campaign(args, ledger, tasks, families):
    binary = str(Path(shutil.which(args.binary) or args.binary).resolve())
    settings = vars(args) | {"binary": binary, "binary_sha256": file_digest(binary)}
    campaign_id = uuid.uuid4().hex
    with ledger.db:
        ledger.db.execute("INSERT INTO campaigns VALUES(?,?,NULL,'running',?)",
                          (campaign_id, time.time(), json.dumps(settings, sort_keys=True)))
    writer = csv.DictWriter(sys.stdout, fieldnames=FIELDS)
    writer.writeheader()
    sys.stdout.flush()
    active, active_lock, stopping = set(), Lock(), Event()
    found, counts = set(), Counter()

    def worker(task, partition_path, attempt_id):
        # Same inputs -> same stream regardless of --jobs, queue order, or retry
        # history. Change --rng-seed explicitly to change a retry trajectory.
        seed = int.from_bytes(hashlib.sha256(
            f"{args.rng_seed}:{task['n']}:{task['candidate']}:{task['family_id']}".encode()).digest()[:8], "big")
        cmd = [binary, "ingraph-orbital-sat", str(task["n"]), task["candidate"],
               "--partition", str(partition_path), "--rng-seed", str(seed)]
        for option in ("seconds", "rounds", "batch", "max_constraints", "conflicts"):
            cmd.extend(["--" + option.replace("_", "-"), str(getattr(args, option))])
        if args.factor:
            cmd.append("--factor")
        if args.dump_dir:
            path = Path(args.dump_dir) / f"{attempt_id}.cnf"
            cmd.extend(["--dump-cnf", str(path)])
        with active_lock:
            if stopping.is_set():
                return None
            log(f"Task {task['id']} {task['name']}: {shlex.join(cmd)}")
            process = subprocess.Popen(cmd, stdout=subprocess.PIPE, text=True,
                                       env=os.environ | {"RAYON_NUM_THREADS": "1"})
            active.add(process)
        try:
            output, _ = process.communicate()
            if stopping.is_set():
                return None
            if process.returncode:
                raise RuntimeError(f"orbital worker failed for task {task['id']}")
            rows = list(csv.DictReader(io.StringIO(output)))
            if len(rows) != 1:
                raise ValueError("expected exactly one orbital result")
            row, = rows
            validate_row(row, task)
            return row | {"family_id": task["family_id"], "family": task["name"],
                          "task_id": task["id"], "campaign_id": campaign_id, "attempt_id": attempt_id}
        finally:
            if process.poll() is None:
                process.terminate()
            process.wait()
            process.stdout.close()
            with active_lock:
                active.discard(process)

    final_state = "aborted"
    timer = time.monotonic()
    pool = ThreadPoolExecutor(max_workers=args.jobs)
    futures = {}
    temporary = tempfile.TemporaryDirectory(prefix="graphy-orbital-")
    try:
        paths = {}
        for family in families:
            path = Path(temporary.name) / (family.id + ".txt")
            path.write_text("".join(f"{cell}\n" for cell in family.cells))
            paths[family.id] = path
        pending = deque(tasks)
        while pending or futures:
            while pending and len(futures) < args.jobs:
                task = pending.popleft()
                if (task["n"], task["candidate"]) in found:
                    continue
                attempt_id = uuid.uuid4().hex
                with ledger.db:
                    ledger.db.execute("INSERT INTO attempts VALUES(?,?,?,?,NULL,'running',?,NULL)",
                                      (attempt_id, task["id"], campaign_id, time.time(),
                                       json.dumps(settings, sort_keys=True)))
                future = pool.submit(worker, task, paths[task["family_id"]], attempt_id)
                futures[future] = attempt_id
            if not futures:
                break
            done, _ = wait(futures, return_when=FIRST_COMPLETED)
            for future in done:
                futures.pop(future)
                row = future.result()
                if row is None:
                    continue
                ledger.complete(row)  # Durable before printing, including if the CSV pipe closes.
                writer.writerow(row)
                sys.stdout.flush()
                counts[row["status"]] += 1
                if row["status"] == "refuted":
                    found.add((int(row["n"]), row["candidate"]))
                    log(f"REFUTED n={row['n']} F={row['candidate']} H={row['refuter']}")
        final_state = "complete"
    finally:
        stopping.set()
        with active_lock:
            for process in active:
                if process.poll() is None:
                    process.terminate()
        pool.shutdown(wait=True, cancel_futures=True)
        temporary.cleanup()  # Workers have exited before their input files disappear.
        with ledger.db:
            ledger.db.execute("UPDATE attempts SET state='aborted',ended=? WHERE campaign_id=? AND state='running'",
                              (time.time(), campaign_id))
            ledger.db.execute("UPDATE campaigns SET state=?,ended=? WHERE id=?", (final_state, time.time(), campaign_id))
        log(f"Orbital campaign {campaign_id}: {final_state}, {time.monotonic()-timer:.1f}s wall, {dict(counts)}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--order", type=int)
    parser.add_argument("--candidate", type=int, action="append", default=[])
    parser.add_argument("--candidates", help="decimal first-column survivor file")
    parser.add_argument("--preset", choices=["triples", "squares", "cyclic"], action="append", default=[])
    parser.add_argument("--template", action="append", default=[], help="e.g. s3:4, s3:4/fixed, s3:4/0+3, cycle:7+7")
    parser.add_argument("--partition-file", action="append", default=[], help="custom decimal cells, one per line, no header; repeatable")
    parser.add_argument("--min-variables", type=int, default=0)
    parser.add_argument("--max-variables", type=int, default=64)
    parser.add_argument("--ledger", default="output/orbital-sat.sqlite")
    parser.add_argument("--work", choices=["new", "retry", "all"], default="new")
    parser.add_argument("--plan", action="store_true", help="register exact families/tasks and save a plan; no SAT")
    parser.add_argument("--status", action="store_true", help="read-only summary of the WHOLE ledger")
    parser.add_argument("--export-csv", action="store_true", help="read-only export of ALL completed outcomes")
    parser.add_argument("--binary", default=BINARY)
    parser.add_argument("--jobs", type=int, default=3)
    parser.add_argument("--seconds", type=float, default=30)
    parser.add_argument("--rounds", type=int, default=100000)
    parser.add_argument("--batch", type=int, default=64)
    parser.add_argument("--max-constraints", type=int, default=500000)
    parser.add_argument("--conflicts", type=int, default=0)
    parser.add_argument("--rng-seed", type=int, default=0)
    parser.add_argument("--factor", action="store_true")
    parser.add_argument("--dump-dir", help="save per-attempt projected CNFs and witness mappings")
    args = parser.parse_args()
    if sum((args.plan, args.status, args.export_csv)) > 1:
        parser.error("choose only one of --plan, --status, --export-csv")
    if args.status or args.export_csv:
        read_only(args)
        return
    if args.order is None or not 1 <= args.order <= 16:
        parser.error("--order in 1..16 is required")
    if min(args.jobs, args.batch, args.max_constraints) < 1 or min(args.rounds, args.conflicts, args.rng_seed) < 0:
        parser.error("jobs/batch/max-constraints must be positive; other budgets/seed nonnegative")
    if args.conflicts > 2147483647 or args.rng_seed > 18446744073709551615:
        parser.error("conflicts must fit i32; rng-seed must fit u64")
    if not math.isfinite(args.seconds) or args.seconds < 0:
        parser.error("seconds must be finite and nonnegative")
    if not 0 <= args.min_variables <= args.max_variables <= 120:
        parser.error("require 0 <= min-variables <= max-variables <= 120")
    candidates = set(args.candidate)
    if args.candidates:
        with open(args.candidates) as source:
            survivors = {int(line.split(",", 1)[0]) for line in source if line.strip()}
        candidates = candidates & survivors if candidates else survivors
    if not candidates or any(not 0 <= f < 1 << (args.order*(args.order-1)//2) for f in candidates):
        parser.error("supply at least one candidate, and all must fit the host order")
    if not (args.preset or args.template or args.partition_file):
        args.preset = ["triples"]
    families = catalog(args.order, args.preset, args.template, args.min_variables, args.max_variables)
    for path in args.partition_file:
        with open(path) as source:
            cells = normalize(args.order, [int(line.strip()) for line in source if line.strip() and not line.lstrip().startswith("#")])
        if args.min_variables <= len(cells) <= args.max_variables:
            families.append(Family(args.order, "file:" + str(Path(path)), cells))
    families = list({f.cells: f for f in families}.values())
    if not families:
        parser.error("no families within the variable limits")
    def interrupt(signum, frame):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, interrupt)
    ledger = Ledger(args.ledger)
    try:
        ledger.register(families, candidates)
        tasks = ledger.eligible(families, candidates, args.work)
        # Stable diversity across candidates/families; not all of one candidate first.
        tasks.sort(key=lambda task: (task["family_id"], int(task["candidate"])))
        random.Random(args.rng_seed).shuffle(tasks)
        counts = Counter(task["family_id"] for task in tasks)
        for f in families:
            log(f"Family {f.id[:12]} {f.name}: {len(f.cells)} variables, {counts[f.id]} eligible candidates")
        budget = f"{len(tasks)*args.seconds/3600:.2f} worker h, ideally /{args.jobs} wall + overhead" if args.seconds else "unlimited per-task time"
        log(f"Orbital plan: {len(candidates)} candidates, {len(families)} exact partitions, {len(tasks)} {args.work} tasks; {budget}")
        log("No seed colours fixed. Family UNSAT is not universality. Identical partitions deduplicate; isomorphic partitions may overlap.")
        if args.plan:
            with ledger.db:
                ledger.db.execute("INSERT INTO campaigns VALUES(?,?,?,'plan',?)",
                                  (uuid.uuid4().hex, time.time(), time.time(), json.dumps(vars(args) | {"task_ids": [t['id'] for t in tasks]}, sort_keys=True)))
            return
        if not tasks:
            return
        if args.dump_dir:
            Path(args.dump_dir).mkdir(parents=True, exist_ok=True)
        run_campaign(args, ledger, tasks, families)
    finally:
        ledger.close()


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        raise SystemExit(130)
    except (ValueError, RuntimeError, OSError, sqlite3.Error) as error:
        raise SystemExit(str(error))
