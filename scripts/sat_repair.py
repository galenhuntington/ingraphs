#!/usr/bin/env python3
"""Run incremental SAT block repair from candidate-specific repair CSV seeds.

Each worker owns an independent graphy process and solver. --ledger adds
resumable, disjoint vertex-region scheduling; legacy mode uses independent
restarts. Results stream immediately; inputs are never rewritten. Local UNSAT
is not a universality certificate. See README for ledger-mode budget semantics.
"""

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import csv
import math
from pathlib import Path
import shlex
import signal
import subprocess
import sys
from threading import Event, Lock

from refine_repair import BINARY, select_seeds, tasks


def command(args, key, seeds, restart):
    n, candidate = key
    cmd = [args.binary, "ingraph-sat", str(n), str(candidate),
           "--restart", str(restart)]
    if seeds:
        cmd.extend(["--seeds", ",".join(map(str, seeds))])
    for option in ("blocks", "block_size", "block_mode", "seconds", "rounds", "batch",
                   "max_constraints", "conflicts", "reset_every", "rng_seed", "vertices"):
        cmd.extend(["--" + option.replace("_", "-"), str(getattr(args, option))])
    for flag in ("relabel", "factor"):
        if getattr(args, flag):
            cmd.append("--" + flag)
    if args.free_edges is not None:
        cmd.extend(["--free-edges", str(args.free_edges)])
    if args.max_vertices is not None:
        cmd.extend(["--max-vertices", str(args.max_vertices)])
    if args.dump_dir:
        cmd.extend(["--dump-dir", str(Path(args.dump_dir) / f"n{n}-f{candidate}-r{restart}")])
    return cmd


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("results", nargs="*", help="candidate-specific seed CSVs; optional with a ledger")
    parser.add_argument("--binary", default=BINARY)
    parser.add_argument("--keep", type=int, default=4, help="best distinct seed classes per candidate")
    parser.add_argument("--max-score", type=int)
    parser.add_argument("--candidate", type=int, action="append")
    parser.add_argument("--candidates", help="current survivor file, decimal first column")
    parser.add_argument("--restarts", type=int, default=1, help="legacy independent runs per candidate; ledger mode requires 1")
    parser.add_argument("--restart-offset", type=int, default=0)
    parser.add_argument("--jobs", type=int, default=3)
    parser.add_argument("--blocks", type=int, default=16, help="legacy: attempts per run; ledger: TOTAL per candidate (0 = all eligible)")
    parser.add_argument("--block-size", type=int, default=32, help="free edges; 0 frees all edges")
    parser.add_argument("--free-edges", type=int, help="prescribed decimal edge mask, overrides block-size/mode")
    parser.add_argument("--block-mode", choices=["directed", "random", "vertex"], help="default vertex with ledger, directed otherwise")
    parser.add_argument("--vertices", type=int, default=4, help="whole vertices to rewire in vertex mode; overrides block-size")
    parser.add_argument("--max-vertices", type=int, help="after vertex-block UNSAT, expand by one core-guided vertex up to this count")
    parser.add_argument("--seconds", type=float, default=5, help="per block, not per restart; 0 disables")
    parser.add_argument("--rounds", type=int, default=10000)
    parser.add_argument("--batch", type=int, default=64)
    parser.add_argument("--max-constraints", type=int, default=200000)
    parser.add_argument("--conflicts", type=int, default=0, help="per SAT call; 0 disables")
    parser.add_argument("--reset-every", type=int, help="default 1 with ledger, 0 otherwise")
    parser.add_argument("--rng-seed", type=int, default=0)
    parser.add_argument("--relabel", action="store_true")
    parser.add_argument("--factor", action="store_true")
    parser.add_argument("--dump-dir")
    parser.add_argument("--ledger", help="SQLite region ledger; one writable launcher, read-only --status allowed")
    parser.add_argument("--import-csv", nargs="+", action="extend", default=[], help="import completed SAT CSVs into ledger (contents deduplicated)")
    parser.add_argument("--import-only", action="store_true", help="import, report ledger totals, then exit")
    parser.add_argument("--status", action="store_true", help="report ledger totals without scheduling work")
    parser.add_argument("--work", choices=["new", "retry", "all"], default="new", help="ledger eligibility; retry = attempted but unresolved, never UNSAT")
    parser.add_argument("--order", type=int, action="append", dest="orders", help="ledger host-order filter; repeatable")
    parser.add_argument("--chunk-size", type=int, default=8, help="ledger: at most this many regions per worker process")
    parser.add_argument("--plan", action="store_true", help="register seed catalogue and print eligible work without running SAT")
    args = parser.parse_args()
    if args.block_mode is None:
        args.block_mode = "vertex" if args.ledger else "directed"
    if args.reset_every is None:
        args.reset_every = 1 if args.ledger else 0
    if min(args.keep, args.restarts, args.jobs, args.batch, args.max_constraints, args.chunk_size) < 1:
        parser.error("keep, restarts, jobs, batch, max-constraints, chunk-size must be positive")
    if args.blocks < 0 or (not args.ledger and args.blocks == 0):
        parser.error("blocks must be positive (or 0 for all eligible ledger regions)")
    if args.ledger:
        if args.block_mode != "vertex" or args.free_edges is not None or args.max_vertices is not None:
            parser.error("ledger supports fixed-size vertex catalogues, without free-edges or max-vertices")
        if args.restarts != 1 or args.restart_offset != 0:
            parser.error("ledger schedules each region once; use --work retry in a later campaign, not restarts")
        if not 1 <= args.vertices <= 32 or (args.orders and min(args.orders) < args.vertices):
            parser.error("vertices must be positive and at most every selected order")
    elif args.import_csv or args.import_only or args.status or args.plan or args.orders or args.work != "new":
        parser.error("bookkeeping options require --ledger")
    elif not args.results:
        parser.error("seed CSVs required without --ledger")
    if min(args.restart_offset, args.block_size, args.rounds, args.conflicts, args.reset_every, args.rng_seed) < 0:
        parser.error("indices, block-size and budgets must be nonnegative")
    if args.conflicts > 2147483647 or args.rng_seed > 18446744073709551615:
        parser.error("conflicts must fit i32 and rng-seed must fit u64")
    if not math.isfinite(args.seconds) or args.seconds < 0:
        parser.error("seconds must be finite and nonnegative")
    candidates = set(args.candidate) if args.candidate else None
    if args.candidates:
        with open(args.candidates) as source:
            survivors = {int(line.split(",", 1)[0]) for line in source if line.strip()}
        candidates = survivors if candidates is None else candidates & survivors
    args.selected_candidates = candidates
    groups = select_seeds(args.results, args.keep, args.max_score, binary=args.binary,
                          candidates=candidates,
                          report=lambda key, raw, classes, chosen: print(
                              f"SAT seeds n={key[0]} F={key[1]}: {chosen}/{classes} classes",
                              file=sys.stderr, flush=True)) if args.results and not (args.import_only or args.status) else {}
    if args.orders:
        groups = {key: seeds for key, seeds in groups.items() if key[0] in args.orders}
    if not groups and (args.results or not args.ledger) and not (args.import_only or args.status):
        parser.error("no eligible candidate-specific seeds")
    if args.free_edges is not None and any(not 0 <= args.free_edges < (1 << (n * (n - 1) // 2)) for n, _ in groups):
        parser.error("free-edges mask must fit every selected host order")
    if args.free_edges is None and args.block_mode == "vertex" and any(not 1 <= args.vertices <= n for n, _ in groups):
        parser.error("vertices must be in 1..n for every selected host order")
    if args.free_edges is None and args.block_mode != "vertex" and any(args.block_size > n * (n - 1) // 2 for n, _ in groups):
        parser.error("block-size exceeds a selected host's edge count; use 0 to free all")
    if args.max_vertices is not None:
        if args.free_edges is not None or args.block_mode != "vertex":
            parser.error("max-vertices requires vertex mode without free-edges")
        if any(not args.vertices <= args.max_vertices <= n for n, _ in groups):
            parser.error("max-vertices must be in vertices..n for every selected host order")
    if args.ledger:
        from sat_ledger import campaign
        campaign(args, groups, command)
        return
    print(f"SAT campaign: {len(groups)} candidates x {args.restarts} independent runs x up to "
          f"{args.blocks} blocks, {args.seconds:g}s per block, {args.jobs} jobs", file=sys.stderr)
    output_lock, processes_lock = Lock(), Lock()
    active = set()
    stopping = Event()
    writer = None
    fields = None

    def interrupt(signum, frame):
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, interrupt)

    def run_one(task):
        nonlocal writer, fields
        if stopping.is_set():
            return
        key, seeds, restart = task
        cmd = command(args, key, seeds, restart)
        with processes_lock:
            if stopping.is_set():
                return
            print(shlex.join(cmd), file=sys.stderr, flush=True)
            process = subprocess.Popen(cmd, stdout=subprocess.PIPE, text=True)
            active.add(process)
        try:
            reader = csv.DictReader(process.stdout)
            for row in reader:
                if None in row or any(value is None for value in row.values()):
                    raise RuntimeError("malformed graphy SAT CSV")
                with output_lock:
                    if writer is None:
                        fields = reader.fieldnames
                        writer = csv.DictWriter(sys.stdout, fieldnames=fields)
                        writer.writeheader()
                    elif reader.fieldnames != fields:
                        raise RuntimeError("inconsistent graphy SAT CSV headers")
                    writer.writerow(row)
                    sys.stdout.flush()
            if process.wait() != 0:
                raise RuntimeError(f"graphy failed for n={key[0]} F={key[1]} restart={restart}")
        finally:
            if process.poll() is None:
                process.terminate()
            process.wait()
            process.stdout.close()
            with processes_lock:
                active.discard(process)

    pool = ThreadPoolExecutor(max_workers=args.jobs)
    futures = []
    try:
        futures = [pool.submit(run_one, task) for task in tasks(groups, args.restarts, args.restart_offset)]
        for future in as_completed(futures):
            future.result()
    except BaseException:
        stopping.set()
        for future in futures:
            future.cancel()
        with processes_lock:
            for process in active:
                if process.poll() is None:
                    process.terminate()
        raise
    finally:
        pool.shutdown(wait=True, cancel_futures=True)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        raise SystemExit(130)
