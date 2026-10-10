"""Durable vertex-region bookkeeping for sat_repair.py (stdlib only).

Region identity is (n, decimal candidate, fixed order, exact canonical core
up to complementation). UNSAT is a solver result, NOT a checked certificate.
Only one launcher may own a ledger. Unfinished work has no permanent claim.
"""

from collections import defaultdict, deque
from concurrent.futures import ThreadPoolExecutor, as_completed
from contextlib import contextmanager
import csv
import fcntl
from functools import lru_cache
import hashlib
import json
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

from refine_repair import canonical_keys, cross_refutations

UNSAT = {"block-unsat", "unrestricted-unsat-unverified"}
STATUSES = UNSAT | {"refuted", "time-limit", "sat-limit", "round-limit", "constraint-limit"}
SCHEMA = "1"


def log(message):
    print(message, file=sys.stderr, flush=True)


def digest(path):
    with open(path, "rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


@lru_cache(maxsize=65536)
def fixed_graph(n, anchor, selected):
    if not 1 <= n <= 32 or not 0 < selected < 1 << n:
        raise ValueError("invalid host order or free-vertex mask")
    if not 0 <= anchor < 1 << (n * (n - 1) // 2):
        raise ValueError("anchor exceeds host order")
    kept = [v for v in range(n) if not selected >> v & 1]
    fixed = sum(((anchor >> (v * (v - 1) // 2 + u)) & 1) << (j * (j - 1) // 2 + i)
                for j, v in enumerate(kept) for i, u in enumerate(kept[:j]))
    free = sum(1 << (v * (v - 1) // 2 + u) for v in range(n) for u in range(v)
               if selected & ((1 << u) | (1 << v)))
    return len(kept), fixed, free


@lru_cache(maxsize=65536)
def infer_vertices(n, free):
    """Recognize exact star unions in older prescribed-mask replay CSVs."""
    if not 1 <= n <= 32:
        raise ValueError("invalid host order")
    full = (1 << (n * (n - 1) // 2)) - 1
    if not 0 <= free <= full:
        raise ValueError("free-edge mask exceeds host order")
    kept = 0
    for v in range(n):
        for u in range(v):
            if not free >> (v * (v - 1) // 2 + u) & 1:
                kept |= (1 << u) | (1 << v)
    selected = ((1 << n) - 1) ^ kept
    if selected and fixed_graph(n, 0, selected)[2] == free:
        return selected
    return None


class Ledger:
    def __init__(self, path, binary):
        self.binary = str(Path(shutil.which(binary) or binary).resolve())
        self.path = Path(path).resolve()
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.lock = open(str(self.path) + ".lock", "a")
        try:
            fcntl.flock(self.lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            self.lock.close()
            raise RuntimeError(f"ledger already in use: {path}; use one launcher with --jobs") from None
        self.db = sqlite3.connect(self.path, check_same_thread=False)
        self.db.row_factory = sqlite3.Row
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute("PRAGMA synchronous=FULL")
        self.db.executescript("""
            CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS regions (
                id INTEGER PRIMARY KEY, n INTEGER NOT NULL, candidate TEXT NOT NULL,
                fixed_order INTEGER NOT NULL, core TEXT NOT NULL,
                anchor TEXT NOT NULL, vertices INTEGER NOT NULL,
                state TEXT NOT NULL DEFAULT 'new', attempts INTEGER NOT NULL DEFAULT 0,
                seconds REAL NOT NULL DEFAULT 0, last_status TEXT, source TEXT, resolved_source TEXT,
                UNIQUE(n,candidate,fixed_order,core));
            CREATE TABLE IF NOT EXISTS refutations (
                n INTEGER, candidate TEXT, host TEXT, source TEXT, PRIMARY KEY(n,candidate));
            CREATE TABLE IF NOT EXISTS imports (
                digest TEXT PRIMARY KEY, path TEXT, rows INTEGER, skipped INTEGER, imported_at REAL);
            CREATE TABLE IF NOT EXISTS campaigns (
                id TEXT PRIMARY KEY, started REAL, ended REAL, status TEXT, settings TEXT);
            CREATE TABLE IF NOT EXISTS campaign_outcomes (
                token TEXT PRIMARY KEY, region_id INTEGER, digest TEXT);
            CREATE INDEX IF NOT EXISTS region_core ON regions(fixed_order,core);
        """)
        schema = self.db.execute("SELECT value FROM meta WHERE key='schema'").fetchone()
        if schema is not None and schema[0] != SCHEMA:
            self.close()
            raise RuntimeError("unsupported SAT ledger schema")
        # Retain terminal-result provenance even if a later imported restart
        # timed out in a previously completed region.
        if 'resolved_source' not in {r[1] for r in self.db.execute('PRAGMA table_info(regions)')}:
            with self.db:
                self.db.execute('ALTER TABLE regions ADD COLUMN resolved_source TEXT')
                self.db.execute("UPDATE regions SET resolved_source=source WHERE last_status IN ('block-unsat','unrestricted-unsat-unverified','refuted')")
        self.binary_digest = digest(self.binary)
        previous = self.db.execute("SELECT value FROM meta WHERE key='binary'").fetchone()
        if previous is not None and previous[0] != self.binary_digest:
            # Canonical graphs are representatives, not permanent identifiers.
            # Re-key them under the new executable; never trust old label bytes.
            log("Graphy executable changed: re-canonicalizing stored region cores")
            by_order = defaultdict(list)
            for row in self.db.execute("SELECT DISTINCT fixed_order,core FROM regions"):
                by_order[row[0]].append(int(row[1]))
            changed = []
            for order, cores in by_order.items():
                for offset in range(0, len(cores), 4096):
                    batch = cores[offset:offset + 4096]
                    keys = self.canonicalize(order, batch)
                    changed.extend((order, str(c), str(k)) for c, k in zip(batch, keys) if c != k)
            with self.db:
                self.db.executemany("UPDATE regions SET core='old:' || core WHERE fixed_order=? AND core=?",
                                    [(order, old) for order, old, _ in changed])
                self.db.executemany("UPDATE regions SET core=? WHERE fixed_order=? AND core=?",
                                    [(new, order, 'old:' + old) for order, old, new in changed])
        with self.db:
            self.db.executemany("INSERT OR REPLACE INTO meta VALUES (?,?)",
                                [("schema", SCHEMA), ("binary", self.binary_digest)])
            abandoned = self.db.execute("UPDATE campaigns SET ended=?,status='interrupted' WHERE status='running'",
                                        (time.time(),)).rowcount
            if abandoned:
                log(f"Recovered {abandoned} interrupted campaign records; unfinished regions remain eligible")
        self.cache = {}

    def close(self):
        self.db.close()
        self.lock.close()

    def canonicalize(self, order, graphs):
        return [0] * len(graphs) if order < 2 else canonical_keys(self.binary, order, graphs)

    def normalize(self, rows, *, outcomes):
        """Derive identities from actual anchors, not a CSV's cached fixed_core."""
        values = []
        missing = defaultdict(set)
        for row in rows:
            n, candidate = int(row["n"]), int(row["candidate"])
            anchor, selected = int(row["start_host"]), int(row["free_vertices"])
            order, core, free = fixed_graph(n, anchor, selected)
            if not 0 <= candidate < 1 << (n * (n - 1) // 2):
                raise ValueError("candidate exceeds host order")
            if outcomes:
                if row["status"] not in STATUSES or int(row["free_edges"]) != free:
                    raise ValueError("invalid status or non-star free-edge mask in vertex outcome")
                elapsed = float(row["seconds"])
                if not 0 <= elapsed < float("inf"):
                    raise ValueError("invalid elapsed time")
            if (order, core) not in self.cache:
                missing[order].add(core)
            values.append((n, str(candidate), order, core, str(anchor), selected))
        for order, cores in missing.items():
            cores = sorted(cores)
            self.cache.update(((order, c), k) for c, k in zip(cores, self.canonicalize(order, cores)))
        result = [(n, f, order, str(self.cache[order, c]), h, v) for n, f, order, c, h, v in values]
        if len(self.cache) > 250000:
            self.cache.clear()
        return result

    def add_region(self, identity):
        self.db.execute("""INSERT OR IGNORE INTO regions
            (n,candidate,fixed_order,core,anchor,vertices) VALUES (?,?,?,?,?,?)""", identity)
        return self.db.execute("SELECT id FROM regions WHERE n=? AND candidate=? AND fixed_order=? AND core=?",
                               identity[:4]).fetchone()[0]

    def record(self, region_id, row, source):
        token = None
        if row.get("campaign_id"):
            token = f"{row['campaign_id']}:{int(row['chunk_id'])}:{int(row['block'])}"
            payload = {k: row[k] for k in ("n", "candidate", "status", "refuter", "start_host", "free_edges", "free_vertices")}
            fingerprint = hashlib.sha256(json.dumps(payload, sort_keys=True).encode()).hexdigest()
            old = self.db.execute("SELECT region_id,digest FROM campaign_outcomes WHERE token=?", (token,)).fetchone()
            if old is not None:
                if tuple(old) != (region_id, fingerprint):
                    raise ValueError("conflicting outcomes for the same campaign/chunk/block")
                return  # Reimporting this launcher's CSV is harmless too.
        status = row["status"]
        state = "unsat" if status in UNSAT else "unresolved"
        region = self.db.execute("SELECT * FROM regions WHERE id=?", (region_id,)).fetchone()
        if status == "refuted":
            if region["state"] == "unsat":
                raise RuntimeError("refutation contradicts recorded region UNSAT; investigate before continuing")
            n, f, h = int(row["n"]), int(row["candidate"]), int(row["refuter"])
            if not 0 <= h < 1 << (n * (n - 1) // 2):
                raise ValueError("refuter exceeds host order")
            if (h ^ int(row["start_host"])) & ~int(row["free_edges"]):
                raise ValueError("refuter does not belong to its reported region")
            known = self.db.execute("SELECT host FROM refutations WHERE n=? AND candidate=?", (n, str(f))).fetchone()
            if known is None or int(known[0]) != h:
                if cross_refutations(self.binary, n, [f], [h]) != {f: h}:
                    raise ValueError("reported refuter failed the separate existence check")
            self.db.execute("INSERT OR REPLACE INTO refutations VALUES (?,?,?,?)", (n, str(f), str(h), source))
            state = "refuted"
        if status in UNSAT and region["state"] == "refuted":
            raise RuntimeError("UNSAT contradicts recorded refutation; investigate before continuing")
        self.db.execute("""UPDATE regions SET
            state=CASE WHEN state IN ('unsat','refuted') THEN state ELSE ? END,
            attempts=attempts+1, seconds=seconds+?,last_status=?,source=?,
            resolved_source=COALESCE(resolved_source,?) WHERE id=?""",
                        (state, float(row["seconds"]), status, source,
                         source if state in ('unsat', 'refuted') else None, region_id))
        if token is not None:
            self.db.execute("INSERT INTO campaign_outcomes VALUES (?,?,?)", (token, region_id, fingerprint))

    def import_csv(self, path):
        fingerprint = digest(path)
        if self.db.execute("SELECT 1 FROM imports WHERE digest=?", (fingerprint,)).fetchone():
            log(f"Already imported (identical contents): {path}")
            return
        log(f"Importing vertex outcomes: {path}")
        count, skipped, batch = 0, 0, []

        def flush():
            identities = self.normalize([r for _, r in batch], outcomes=True)
            for (line, row), identity in zip(batch, identities):
                self.record(self.add_region(identity), row, f"{path}:{line}")
            batch.clear()

        # A damaged/truncated file rolls back in full. Previously imported files
        # remain committed. Original CSVs are never modified.
        with self.db, open(path, newline="") as source:
            reader = csv.DictReader(source)
            required = {"n", "candidate", "status", "block_mode"}
            if not required <= set(reader.fieldnames or []):
                raise ValueError(f"{path}: not a SAT outcome CSV")
            for row in reader:
                if None in row or any(v is None for v in row.values()):
                    raise ValueError(f"{path}:{reader.line_num}: malformed CSV (file import rolled back)")
                if row["n"] == "n":
                    continue
                if row["block_mode"] != "Vertex":
                    selected = infer_vertices(int(row["n"]), int(row["free_edges"]))
                    if selected is None:
                        skipped += 1
                        continue
                    row = row | {"free_vertices": str(selected)}
                batch.append((reader.line_num, row))
                count += 1
                if len(batch) == 2048:
                    flush()
            flush()
            if digest(path) != fingerprint:
                raise ValueError(f"{path}: changed during import; wait until its writer stops")
            self.db.execute("INSERT INTO imports VALUES (?,?,?,?,?)",
                            (fingerprint, str(Path(path).resolve()), count, skipped, time.time()))
        log(f"Imported {count} vertex-region outcomes; skipped {skipped} non-star masks")

    def summary(self):
        print_summary(self.db)


def print_summary(db):
    for row in db.execute("""SELECT n,n-fixed_order AS r,state,count(*) AS total,
            sum(attempts) AS attempts FROM regions
            WHERE NOT EXISTS (SELECT 1 FROM refutations f WHERE f.n=regions.n AND f.candidate=regions.candidate)
            GROUP BY n,r,state ORDER BY n,r,state"""):
        log(f"Ledger n={row['n']} vertices={row['r']}: {row['total']} {row['state']} regions, {row['attempts']} recorded attempts")
    log(f"Ledger verified refuted candidates: {db.execute('SELECT count(*) FROM refutations').fetchone()[0]}")
    for row in db.execute("SELECT id,status FROM campaigns ORDER BY started DESC LIMIT 3"):
        log(f"Campaign {row['id']}: {row['status']}")


@contextmanager
def open_ledger(path, binary):
    ledger = Ledger(path, binary)
    try:
        yield ledger
    finally:
        ledger.close()


def campaign(args, groups, command):
    if args.status and not args.import_csv:
        # Reporting is a read-only snapshot and can run while a writer owns the
        # ledger. Do not rebuild keys, create a DB, or recover campaigns here.
        with sqlite3.connect(Path(args.ledger).resolve().as_uri() + '?mode=ro', uri=True) as db:
            db.row_factory = sqlite3.Row
            db.execute('BEGIN')
            print_summary(db)
        db.close()
        return
    with open_ledger(args.ledger, args.binary) as ledger:
        for path in args.import_csv:
            ledger.import_csv(path)
        if args.import_only or args.status:
            ledger.summary()
            return
        solved = {(r[0], int(r[1])) for r in ledger.db.execute("SELECT n,candidate FROM refutations")}
        for key, seeds in groups.items():
            if key in solved:
                continue
            cmd = command(args, key, seeds, 0) + ["--list-regions"]
            cmd[cmd.index("--blocks") + 1] = "1"
            # Catalogue construction is read-only and does not allocate a solver.
            result = subprocess.run(cmd, text=True, stdout=subprocess.PIPE, check=True)
            rows = list(csv.DictReader(result.stdout.splitlines()))
            identities = []
            for row in rows:
                n, candidate = int(row['n']), int(row['candidate'])
                anchor, selected, core = int(row['start_host']), int(row['free_vertices']), int(row['fixed_core'])
                order = n - selected.bit_count()
                if (n, candidate) != key or not 0 < selected < 1 << n or not 0 <= order <= n:
                    raise ValueError('invalid native catalogue region')
                if not 0 <= anchor < 1 << (n*(n-1)//2) or not 0 <= core < 1 << (order*(order-1)//2):
                    raise ValueError('invalid native catalogue graph')
                identities.append((n, str(candidate), order, str(core), str(anchor), selected))
            with ledger.db:
                # Fresh native keys already use this executable; unlike old
                # CSVs they need no second canonicalization. IDs are queried
                # later, so bulk insertion needs no per-row SELECT either.
                ledger.db.executemany('''INSERT OR IGNORE INTO regions
                    (n,candidate,fixed_order,core,anchor,vertices) VALUES (?,?,?,?,?,?)''', identities)
        ledger.summary()
        queues = defaultdict(list)
        states = {"new"} if args.work == "new" else {"unresolved"} if args.work == "retry" else {"new", "unresolved"}
        for row in ledger.db.execute("SELECT * FROM regions ORDER BY n,candidate,id"):
            key = row["n"], int(row["candidate"])
            if key in solved or row["state"] not in states or row["n"] - row["fixed_order"] != args.vertices:
                continue
            if args.orders and row["n"] not in args.orders:
                continue
            if args.selected_candidates is not None and key[1] not in args.selected_candidates:
                continue
            # Seed files restrict candidate selection, not previously registered
            # regions for those candidates. Without seed files use the ledger.
            if groups and key not in groups:
                continue
            queues[key].append(row["id"])
        rng = random.Random(args.rng_seed)
        for key in queues:
            rng.shuffle(queues[key])
            if args.blocks:
                queues[key] = queues[key][:args.blocks]
        planned = sum(map(len, queues.values()))
        bound = planned * args.seconds
        log(f"Ledger plan: {planned} distinct {args.work} regions, {len(queues)} candidates, "
            f"{args.jobs} workers, chunks <= {args.chunk_size}, {args.seconds:g}s/block; "
            + (f"soft ceiling {bound/3600:.2f} summed worker-hours (not a wall-time prediction)" if args.seconds else "no time ceiling"))
        if args.plan or not planned:
            return
        run = uuid.uuid4().hex
        settings = vars(args) | {"selected_candidates": sorted(args.selected_candidates) if args.selected_candidates is not None else None,
                                 "binary_sha256": ledger.binary_digest, "planned_regions": planned}
        with ledger.db:
            ledger.db.execute("INSERT INTO campaigns VALUES (?,?,?,?,?)",
                              (run, time.time(), None, "running", json.dumps(settings, sort_keys=True)))
        log(f"Campaign {run}; results committed after each completed block")
        queues = deque((key, deque(regions)) for key, regions in queues.items())
        lock = Lock()
        stopping = Event()
        active = {}
        cancelled = set()
        fields, writer = None, None
        chunk_number, completed = 0, 0

        def worker(directory):
            nonlocal chunk_number, completed, fields, writer
            while True:
                with lock:
                    if stopping.is_set():
                        return
                    while queues and queues[0][0] in solved:
                        queues.popleft()
                    if not queues:
                        return
                    key, pending = queues.popleft()
                    chunk = [dict(ledger.db.execute("SELECT * FROM regions WHERE id=?", (pending.popleft(),)).fetchone())
                             for _ in range(min(args.chunk_size, len(pending)))]
                    if pending:
                        queues.append((key, pending))
                    index = chunk_number
                    chunk_number += 1
                    path = Path(directory) / f"regions-{index}.txt"
                    path.write_text("".join(f"{r['anchor']},{r['vertices']}\n" for r in chunk))
                    cmd = command(args, key, [], index)
                    # The native cap is local to this chunk; launcher --blocks
                    # has already capped TOTAL scheduled work per candidate.
                    cmd[cmd.index("--blocks") + 1] = str(len(chunk))
                    if args.dump_dir:
                        cmd[cmd.index("--dump-dir") + 1] = str(Path(args.dump_dir) / run / f"chunk-{index}")
                    cmd += ["--region-file", str(path)]
                    log(f"chunk={index} regions={','.join(str(r['id']) for r in chunk)} {shlex.join(cmd)}")
                    process = subprocess.Popen(cmd, stdout=subprocess.PIPE, text=True)
                    active[process] = key
                emitted = 0
                try:
                    reader = csv.DictReader(process.stdout)
                    for row in reader:
                        if None in row or any(v is None for v in row.values()):
                            raise RuntimeError("malformed graphy SAT CSV")
                        b = int(row["block"])
                        if b != emitted or b >= len(chunk) or (int(row['n']), int(row['candidate'])) != key:
                            raise RuntimeError("worker output does not match scheduled chunk")
                        with lock:
                            expected = chunk[b]
                            selected = int(row["free_vertices"])
                            order, _, free = fixed_graph(key[0], int(row["start_host"]), selected)
                            # These keys come from this running binary, unlike
                            # imported CSVs which must be re-canonicalized.
                            if (order, row["fixed_core"]) != (expected['fixed_order'], expected['core']) or int(row['free_edges']) != free or row['status'] not in STATUSES:
                                raise RuntimeError("worker returned the wrong region")
                            metadata = {"campaign_id": run, "region_id": expected['id'], "chunk_id": index}
                            with ledger.db:
                                ledger.record(expected["id"], row | metadata, f"campaign:{run}:chunk:{index}:block:{b}")
                            if fields is None:
                                fields = reader.fieldnames
                                writer = csv.DictWriter(sys.stdout, fieldnames=fields + ["campaign_id", "region_id", "chunk_id"])
                                writer.writeheader()
                            elif fields != reader.fieldnames:
                                raise RuntimeError("inconsistent graphy SAT CSV headers")
                            writer.writerow(row | metadata)
                            sys.stdout.flush()
                            completed += 1
                            emitted += 1
                            if row["status"] == "refuted":
                                solved.add(key)
                                for other, other_key in active.items():
                                    if other is not process and other_key == key and other.poll() is None:
                                        cancelled.add(other)
                                        other.terminate()
                    code = process.wait()
                    if code != 0 and process not in cancelled and not stopping.is_set():
                        raise RuntimeError(f"graphy exited {code} for {key}, chunk {index}")
                    if emitted < len(chunk) and key not in solved and not stopping.is_set():
                        log(f"Chunk {index} ended early: {len(chunk)-emitted} regions remain eligible for a later campaign")
                except BaseException:
                    stopping.set()
                    with lock:
                        for other in active:
                            if other.poll() is None:
                                other.terminate()
                    raise
                finally:
                    if process.poll() is None:
                        process.terminate()
                    process.wait()
                    process.stdout.close()
                    with lock:
                        active.pop(process, None)
                        cancelled.discard(process)
                    path.unlink()

        def interrupt(signum, frame):
            raise KeyboardInterrupt

        previous_handler = signal.signal(signal.SIGTERM, interrupt)
        outcome = "interrupted"
        pool = ThreadPoolExecutor(max_workers=args.jobs)
        try:
            with tempfile.TemporaryDirectory(prefix="graphy-sat-regions-") as directory:
                futures = [pool.submit(worker, directory) for _ in range(args.jobs)]
                try:
                    for future in as_completed(futures):
                        future.result()
                    outcome = "finished"
                finally:
                    stopping.set()
                    with lock:
                        for process in active:
                            if process.poll() is None:
                                process.terminate()
                    pool.shutdown(wait=True, cancel_futures=True)
        finally:
            signal.signal(signal.SIGTERM, previous_handler)
            with ledger.db:
                ledger.db.execute("UPDATE campaigns SET ended=?,status=? WHERE id=?", (time.time(), outcome, run))
            log(f"Campaign {run} {outcome}: {completed}/{planned} planned regions returned outcomes")
