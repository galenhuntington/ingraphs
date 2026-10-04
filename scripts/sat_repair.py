#!/usr/bin/env python3
"""Run incremental SAT block repair from candidate-specific repair CSV seeds.

Each worker owns an independent graphy process and solver. Blocks within that
process retain clauses and learning. Results stream immediately; inputs are
never rewritten. Local UNSAT is not a universality certificate.
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
           "--seeds", ",".join(map(str, seeds)), "--restart", str(restart)]
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
    parser.add_argument("results", nargs="+")
    parser.add_argument("--binary", default=BINARY)
    parser.add_argument("--keep", type=int, default=4, help="best distinct seed classes per candidate")
    parser.add_argument("--max-score", type=int)
    parser.add_argument("--candidate", type=int, action="append")
    parser.add_argument("--candidates", help="current survivor file, decimal first column")
    parser.add_argument("--restarts", type=int, default=2, help="independent solver processes per candidate")
    parser.add_argument("--restart-offset", type=int, default=0)
    parser.add_argument("--jobs", type=int, default=3)
    parser.add_argument("--blocks", type=int, default=16, help="maximum total attempts per run, including expansions")
    parser.add_argument("--block-size", type=int, default=32, help="free edges; 0 frees all edges")
    parser.add_argument("--free-edges", type=int, help="prescribed decimal edge mask, overrides block-size/mode")
    parser.add_argument("--block-mode", choices=["directed", "random", "vertex"], default="directed")
    parser.add_argument("--vertices", type=int, default=4, help="whole vertices to rewire in vertex mode; overrides block-size")
    parser.add_argument("--max-vertices", type=int, help="after vertex-block UNSAT, expand by one core-guided vertex up to this count")
    parser.add_argument("--seconds", type=float, default=5, help="per block, not per restart; 0 disables")
    parser.add_argument("--rounds", type=int, default=10000)
    parser.add_argument("--batch", type=int, default=64)
    parser.add_argument("--max-constraints", type=int, default=200000)
    parser.add_argument("--conflicts", type=int, default=0, help="per SAT call; 0 disables")
    parser.add_argument("--reset-every", type=int, default=0)
    parser.add_argument("--rng-seed", type=int, default=0)
    parser.add_argument("--relabel", action="store_true")
    parser.add_argument("--factor", action="store_true")
    parser.add_argument("--dump-dir")
    args = parser.parse_args()
    if min(args.keep, args.restarts, args.jobs, args.blocks, args.batch, args.max_constraints) < 1:
        parser.error("keep, restarts, jobs, blocks, batch, max-constraints must be positive")
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
    groups = select_seeds(args.results, args.keep, args.max_score, binary=args.binary,
                          candidates=candidates,
                          report=lambda key, raw, classes, chosen: print(
                              f"SAT seeds n={key[0]} F={key[1]}: {chosen}/{classes} classes",
                              file=sys.stderr, flush=True))
    if not groups:
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
