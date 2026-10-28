#!/usr/bin/env python3
"""Resumable exhaustive enumeration of four equal even cycles.

Default: cycle:4+4+4+4 on 16 vertices. Generate once, canonically deduplicate
in a separate SQLite library, then check all selected candidates. Completed
batches and candidate cursors survive interruption. See research/2026-10-27.md.
"""

import argparse
from collections import deque
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
import csv
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import signal
import sqlite3
import subprocess
import sys
import tempfile
from threading import Event, Lock
import time
import uuid

from orbital_enum_families import cubes, geometry, manifest
from refine_repair import BINARY

SCHEMA = "orbital-enum-v1"
FIELDS = "n,candidate,refuter,status,checked_hosts,total_hosts,completed_batches,total_batches".split(",")


def log(message):
    print(message, file=sys.stderr, flush=True)


def digest(path):
    with open(path, "rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def check_schema(db):
    tables = {row[0] for row in db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    if tables:
        row = db.execute("SELECT value FROM meta WHERE key='schema'").fetchone() if "meta" in tables else None
        if row is None or row[0] != SCHEMA:
            raise ValueError("not an orbital enumeration ledger; do NOT use either SAT ledger")
    return bool(tables)


class Ledger:
    def __init__(self, path, cycle_length, binary):
        self.path = Path(path).resolve()
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.lock = open(str(self.path) + ".lock", "a")
        try:
            fcntl.flock(self.lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            self.lock.close()
            raise RuntimeError("enumeration ledger already in use; use one launcher with --jobs") from None
        self.db = sqlite3.connect(self.path)
        self.db.row_factory = sqlite3.Row
        try:
            existing = check_schema(self.db)
            plan, plan_id = manifest(cycle_length)
            settings = {"schema": SCHEMA, "plan_id": plan_id, "n": str(plan["n"]),
                        "cycle_length": str(cycle_length), "binary_sha256": digest(binary)}
            if existing:
                old = dict(self.db.execute("SELECT key,value FROM meta"))
                for key, value in settings.items():
                    if old.get(key) != value:
                        raise ValueError(f"ledger {key} differs; keep the original binary/plan or use a fresh ledger")
            # Validate identity BEFORE writing to an existing database.
            self.db.execute("PRAGMA journal_mode=WAL")
            self.db.execute("PRAGMA synchronous=FULL")
            self.db.executescript("""
                CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS batches(
                    defect INTEGER PRIMARY KEY, weight INTEGER NOT NULL, cube TEXT NOT NULL,
                    generated INTEGER NOT NULL, complete INTEGER NOT NULL DEFAULT 0,
                    unique_hosts INTEGER, seconds REAL, completed_at REAL);
                CREATE TABLE IF NOT EXISTS hosts(id INTEGER PRIMARY KEY, bits TEXT NOT NULL UNIQUE);
                CREATE TABLE IF NOT EXISTS candidates(
                    bits TEXT PRIMARY KEY, checked_through INTEGER NOT NULL DEFAULT 0,
                    refuter TEXT);
                CREATE TABLE IF NOT EXISTS campaigns(
                    id TEXT PRIMARY KEY, started REAL, ended REAL, state TEXT, settings TEXT);
            """)
            with self.db:
                if not existing:
                    self.db.executemany("INSERT INTO meta VALUES(?,?)", settings.items())
                    self.db.execute("INSERT INTO meta VALUES('binary_path',?)", (binary,))
                    self.db.executemany("INSERT INTO batches(defect,weight,cube,generated) VALUES(?,?,?,?)",
                                        [(c.defect, c.defect.bit_count(), c.text, c.generated) for c in cubes(cycle_length)])
                self.db.execute("UPDATE campaigns SET state='aborted',ended=? WHERE state='running'", (time.time(),))
            self.n = plan["n"]
        except BaseException:
            self.close()
            raise

    def close(self):
        self.db.close()
        self.lock.close()

    def register(self, candidates):
        with self.db:
            self.db.executemany("INSERT OR IGNORE INTO candidates(bits) VALUES(?)", [(str(f),) for f in sorted(candidates)])

    def pending(self, minimum, maximum, limit):
        rows = self.db.execute("SELECT * FROM batches WHERE complete=0 AND weight BETWEEN ? AND ? ORDER BY weight,defect",
                               (minimum, maximum)).fetchall()
        return rows if limit is None else rows[:limit]

    def complete_batch(self, task, hosts, seconds):
        # The graph inserts and completion flag are ONE transaction. A killed
        # child / failed transaction cannot create a falsely completed batch.
        with self.db:
            self.db.executemany("INSERT OR IGNORE INTO hosts(bits) VALUES(?)", [(str(h),) for h in hosts])
            self.db.execute("UPDATE batches SET complete=1,unique_hosts=?,seconds=?,completed_at=? WHERE defect=?",
                            (len(hosts), seconds, time.time(), task["defect"]))


def totals(db):
    completed, batches, generated, total = db.execute(
        "SELECT sum(complete),count(*),sum(CASE WHEN complete THEN generated ELSE 0 END),sum(generated) FROM batches").fetchone()
    hosts, last = db.execute("SELECT count(*),coalesce(max(id),0) FROM hosts").fetchone()
    return completed, batches, generated, total, hosts, last


def export_csv(db, stream=sys.stdout):
    n = int(db.execute("SELECT value FROM meta WHERE key='n'").fetchone()[0])
    complete, batches, _, _, hosts, last = totals(db)
    writer = csv.DictWriter(stream, fieldnames=FIELDS)
    writer.writeheader()
    for candidate, cursor, hit in db.execute("SELECT bits,checked_through,refuter FROM candidates ORDER BY length(bits),bits"):
        checked = db.execute("SELECT count(*) FROM hosts WHERE id<=?", (cursor,)).fetchone()[0]
        status = "refuted" if hit is not None else (
            "family-excluded-by-enumeration" if complete == batches and cursor == last else "partial")
        writer.writerow(dict(zip(FIELDS, [n, candidate, hit or "", status, checked, hosts, complete, batches])))
    stream.flush()


def status(db):
    complete, batches, generated, total, hosts, last = totals(db)
    log(f"Batches: {complete}/{batches}; assignments: {generated:,}/{total:,}; unique hosts: {hosts:,}")
    for weight, done, count in db.execute("SELECT weight,sum(complete),count(*) FROM batches GROUP BY weight ORDER BY weight"):
        log(f"  defects={weight}: {done}/{count} batches")
    candidates, refuted, caught_up = db.execute("""SELECT count(*),coalesce(sum(refuter IS NOT NULL),0),
        coalesce(sum(refuter IS NULL AND checked_through=?),0) FROM candidates""", (last,)).fetchone()
    log(f"Candidates: {candidates}; refuted: {refuted}; checked through current library: {caught_up}")
    if complete != batches:
        log("PARTIAL family coverage: no negative result is a whole-family exclusion yet.")


def read_only(args):
    with sqlite3.connect(Path(args.ledger).resolve().as_uri() + "?mode=ro", uri=True) as db:
        if not check_schema(db):
            raise ValueError("empty enumeration ledger")
        if args.export_csv:
            export_csv(db)
        elif args.export_library:
            for host, in db.execute("SELECT bits FROM hosts ORDER BY id"):
                print(host)
        else:
            status(db)


def parse_worker(output, n, generated):
    lines = output.splitlines()
    if not lines:
        raise ValueError("empty enumeration worker output")
    fields = lines[0].split(",")
    if len(fields) != 5 or fields[:3] != [SCHEMA, str(n), str(generated)]:
        raise ValueError("enumeration worker returned a different protocol/order/cube size")
    unique, seconds = int(fields[3]), float(fields[4])
    if not 0 < unique <= generated or not math.isfinite(seconds) or seconds < 0:
        raise ValueError("invalid enumeration worker counts/timing")
    hosts = list(map(int, lines[1:]))
    if len(hosts) != unique or len(set(hosts)) != unique or any(not 0 <= h < 1 << (n*(n-1)//2) for h in hosts):
        raise ValueError("truncated, duplicate, or invalid enumeration worker hosts")
    return hosts, seconds


def parse_cull(output, candidates, hosts):
    results = {}
    for line in output.splitlines():
        candidate, found = line.split(",")
        if candidate not in candidates or candidate in results:
            raise ValueError("unexpected/duplicate candidate from cull")
        if found == "None":
            results[candidate] = None
        elif found.startswith("Some(") and found.endswith(")"):
            host = int(found[5:-1])
            if host not in hosts:
                raise ValueError("cull returned a host outside the checked chunk")
            results[candidate] = str(host)
        else:
            raise ValueError("malformed cull result")
    if set(results) != set(candidates):
        raise ValueError("incomplete cull result")
    return results


class Children:
    def __init__(self):
        self.active, self.lock, self.stopping = set(), Lock(), Event()

    def run(self, command, threads=1):
        with self.lock:
            if self.stopping.is_set():
                raise RuntimeError("campaign stopping")
            process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                       env=os.environ | {"RAYON_NUM_THREADS": str(threads)})
            self.active.add(process)
        try:
            output, error = process.communicate()
            if process.returncode:
                raise RuntimeError(f"worker exited {process.returncode}: {error[-4000:]}")
            return output
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
            process.wait()
            process.stdout.close()
            process.stderr.close()
            with self.lock:
                self.active.discard(process)

    def stop(self):
        self.stopping.set()
        with self.lock:
            active = list(self.active)
            for process in active:
                if process.poll() is None:
                    process.terminate()
        for process in active:
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


def generate(args, ledger, tasks, temporary, children):
    def worker(task):
        path = Path(temporary) / f"cube-{task['defect']}.txt"
        path.write_text(task["cube"])
        output = children.run([args.binary, "orbital-enum", str(ledger.n), str(path)])
        return parse_worker(output, ledger.n, task["generated"])

    pool = ThreadPoolExecutor(max_workers=args.jobs)
    pending, futures = deque(tasks), {}
    done, generated = 0, 0
    started = time.monotonic()
    try:
        while pending or futures:
            while pending and len(futures) < args.jobs:
                task = pending.popleft()
                futures[pool.submit(worker, task)] = task
            completed, _ = wait(futures, return_when=FIRST_COMPLETED)
            for future in completed:
                task = futures.pop(future)
                hosts, seconds = future.result()
                ledger.complete_batch(task, hosts, seconds)
                generated += task["generated"]
                done += 1
                if done == 1 or done % 25 == 0 or done == len(tasks):
                    elapsed = time.monotonic() - started
                    log(f"Generation: {done}/{len(tasks)} selected batches committed, "
                        f"{generated:,} assignments, {elapsed:.1f}s wall")
    finally:
        if futures:
            children.stop()
        pool.shutdown(wait=True, cancel_futures=True)


def check_candidates(args, ledger, temporary, children):
    chunks, started = 0, time.monotonic()
    while args.max_check_chunks is None or chunks < args.max_check_chunks:
        # Group candidates at the earliest cursor. New candidates may start at
        # zero while previous candidates have already checked the old library.
        cursor = ledger.db.execute("SELECT min(checked_through) FROM candidates WHERE refuter IS NULL").fetchone()[0]
        if cursor is None:
            return
        rows = ledger.db.execute("SELECT id,bits FROM hosts WHERE id>? ORDER BY id LIMIT ?", (cursor, args.check_chunk)).fetchall()
        if not rows:
            return
        candidates = [r[0] for r in ledger.db.execute(
            "SELECT bits FROM candidates WHERE refuter IS NULL AND checked_through=? ORDER BY length(bits),bits", (cursor,))]
        host_path, candidate_path = Path(temporary)/"hosts.txt", Path(temporary)/"candidates.txt"
        host_path.write_text("".join(f"{row['bits']}\n" for row in rows))
        candidate_path.write_text("".join(f"{f}\n" for f in candidates))
        output = children.run([args.binary, "cull", str(ledger.n), str(candidate_path), str(host_path)], args.jobs)
        results = parse_cull(output, candidates, {int(row["bits"]) for row in rows})
        last = rows[-1]["id"]
        host_ids = {row["bits"]: row["id"] for row in rows}
        with ledger.db:
            for candidate, hit in results.items():
                # cull stops at the first witness for this candidate; don't
                # count the unvisited suffix of that chunk as checked work.
                checked_through = last if hit is None else host_ids[hit]
                ledger.db.execute("UPDATE candidates SET checked_through=?,refuter=? WHERE bits=?",
                                  (checked_through, hit, candidate))
        for candidate, hit in results.items():
            if hit is not None:
                log(f"REFUTED n={ledger.n} F={candidate} H={hit} (committed)")
        chunks += 1
        if chunks == 1 or chunks % 10 == 0:
            log(f"Checking: {chunks} chunks committed, latest host id {last}, {time.monotonic()-started:.1f}s wall")


def run(args, ledger, tasks):
    campaign = uuid.uuid4().hex
    with ledger.db:
        ledger.db.execute("INSERT INTO campaigns VALUES(?,?,NULL,'running',?)",
                          (campaign, time.time(), json.dumps(vars(args), sort_keys=True)))
    children, state = Children(), "aborted"
    try:
        with tempfile.TemporaryDirectory(prefix="graphy-enumeration-") as temporary:
            generate(args, ledger, tasks, temporary, children)
            if not args.generate_only:
                check_candidates(args, ledger, temporary, children)
        state = "complete"
    finally:
        children.stop()
        with ledger.db:
            ledger.db.execute("UPDATE campaigns SET state=?,ended=? WHERE id=?", (state, time.time(), campaign))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ledger", required=True, help="a NEW enumeration ledger, not a SAT ledger")
    parser.add_argument("--cycle-length", type=int, choices=(2, 4), default=4, help="four cycles of this length; 2 is a small exhaustive control")
    parser.add_argument("--candidate", action="append", type=int, default=[])
    parser.add_argument("--candidates", help="decimal first-column file; UNION with --candidate; saved candidates remain registered")
    parser.add_argument("--binary", default=BINARY)
    parser.add_argument("--jobs", type=int, default=3)
    parser.add_argument("--min-defects", type=int, default=0)
    parser.add_argument("--max-defects", type=int, default=16)
    parser.add_argument("--max-batches", type=int, help="new generation batches this invocation; default all, 0 checks only")
    parser.add_argument("--generate-only", action="store_true")
    parser.add_argument("--check-chunk", type=int, default=4096)
    parser.add_argument("--max-check-chunks", type=int, help="limit committed check chunks this invocation; default all")
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--plan", action="store_true", help="register library/candidates and print remaining work; do not spawn workers")
    modes.add_argument("--status", action="store_true", help="read-only whole-ledger summary")
    modes.add_argument("--export-csv", action="store_true", help="read-only candidate outcomes, including partial coverage")
    modes.add_argument("--export-library", action="store_true", help="read-only decimal internal labels, one host per line")
    args = parser.parse_args()
    if args.status or args.export_csv or args.export_library:
        read_only(args)
        return
    if args.jobs < 1 or args.check_chunk < 1 or any(v is not None and v < 0 for v in (args.max_batches, args.max_check_chunks)):
        parser.error("jobs/check-chunk must be positive; batch limits must be nonnegative")
    if not 0 <= args.min_defects <= args.max_defects <= 16:
        parser.error("require 0 <= min-defects <= max-defects <= 16")
    args.binary = str(Path(shutil.which(args.binary) or args.binary).resolve())
    n = geometry(args.cycle_length)[0]
    candidates = set(args.candidate)
    if args.candidates:
        with open(args.candidates) as source:
            candidates.update(int(line.split(",", 1)[0]) for line in source if line.strip())
    if any(not 0 <= f < 1 << (n*(n-1)//2) for f in candidates):
        parser.error(f"all candidates must fit n={n}")

    def interrupt(signum, frame):
        raise KeyboardInterrupt

    previous = {sig: signal.signal(sig, interrupt) for sig in (signal.SIGINT, signal.SIGTERM)}
    ledger = None
    try:
        ledger = Ledger(args.ledger, args.cycle_length, args.binary)
        ledger.register(candidates)
        tasks = ledger.pending(args.min_defects, args.max_defects, args.max_batches)
        if args.plan:
            status(ledger.db)
            log(f"Selected remaining work: {len(tasks)} batches, {sum(r['generated'] for r in tasks):,} assignments")
            with ledger.db:
                ledger.db.execute("INSERT INTO campaigns VALUES(?,?,?,'plan',?)",
                                  (uuid.uuid4().hex, time.time(), time.time(), json.dumps(vars(args), sort_keys=True)))
            return
        run(args, ledger, tasks)
        status(ledger.db)
        export_csv(ledger.db)
    except KeyboardInterrupt:
        log("Interrupted: completed transactions retained; the same command resumes unfinished work.")
        if ledger is not None:
            export_csv(ledger.db)
        raise SystemExit(130) from None
    finally:
        if ledger is not None:
            ledger.close()
        for sig, handler in previous.items():
            signal.signal(sig, handler)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, RuntimeError, OSError, sqlite3.Error) as error:
        raise SystemExit(f"orbital_enum: {error}") from None
