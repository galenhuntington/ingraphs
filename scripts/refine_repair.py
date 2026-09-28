#!/usr/bin/env python3
"""Resume diverse candidate-specific hosts from counted-repair CSV logs.

Deduplicate by exact isomorphism/complement keys BEFORE applying --keep.
Schedule individual restarts, so a focused campaign can use every worker.
Stream/flush outcomes and near-best checkpoints; never edit input files.
"""

import argparse
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
import csv
import math
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
from threading import Lock, Thread


BINARY = str(Path(__file__).resolve().parents[1] / "target/release/graphy")
FIELDS = ("n,candidate,refuter,status,restart,walk_seed,flips,checks,constraints,violated,"
          "seconds,oracle_seconds,start_host,saved_host,distance,best_copies,method,source_seed,"
          "record,pair_evaluations,pair_moves,outside_moves,best_step").split(",")


def read_results(paths):
    for path in paths:
        with open(path, newline="") as source:
            for row in csv.DictReader(source):
                if row["n"] != "n":  # tolerate concatenated CSV files
                    yield row


def canonical_keys(binary, n, hosts):
    """Full packed keys, not hashes; live hosts are never relabelled here."""
    result = subprocess.run([binary, "canon", str(n), "/dev/stdin",
                             "--internal-labels", "--complement"],
                            input="".join(f"{h}\n" for h in hosts), text=True,
                            stdout=subprocess.PIPE, check=True)
    keys = [int(line) for line in result.stdout.splitlines()]
    if len(keys) != len(hosts):
        raise RuntimeError("canonicalizer returned the wrong number of keys")
    return keys


def collect_results(paths):
    """Read even pipes/process substitutions only once; retain minimal metadata."""
    groups = {}
    refuted = set()
    witnesses = {}
    for row in read_results(paths):
        key = int(row["n"]), int(row["candidate"])
        if row["status"] == "refuted":
            refuted.add(key)
            if row.get("refuter"):
                witnesses.setdefault(key[0], set()).add(int(row["refuter"]))
        if not row.get("best_copies"):
            continue
        score = int(row["best_copies"])
        host = int(row["saved_host"])
        hosts = groups.setdefault(key, {})
        hosts[host] = min(score, hosts.get(host, score))
    return groups, refuted, witnesses


def select_seeds(paths, keep, max_score=None, *, binary=BINARY, candidates=None,
                 canonicalize=None, report=None, catalog=None):
    groups, refuted, _ = catalog if catalog is not None else collect_results(paths)
    groups = {key: {h: q for h, q in hosts.items() if max_score is None or q <= max_score}
              for key, hosts in groups.items() if key not in refuted
              and (candidates is None or key[1] in candidates)}
    groups = {key: hosts for key, hosts in groups.items() if hosts}
    by_order = {}
    for (n, _), hosts in groups.items():
        by_order.setdefault(n, set()).update(hosts)
    keys = {}
    for n, hosts in by_order.items():
        hosts = sorted(hosts)
        values = canonicalize(n, hosts) if canonicalize else canonical_keys(binary, n, hosts)
        keys[n] = dict(zip(hosts, values, strict=True))
    selected = {}
    for key, hosts in sorted(groups.items()):
        seen = set()
        seeds = []
        for host, _ in sorted(hosts.items(), key=lambda pair: (pair[1], pair[0])):
            canonical = keys[key[0]][host]
            if canonical not in seen:
                seen.add(canonical)
                if len(seeds) < keep:
                    seeds.append(host)
        selected[key] = seeds
        if report:
            report(key, len(hosts), len(seen), len(seeds))
    return selected


def cross_refutations(binary, n, candidates, witnesses):
    """Use the separate existence matcher, not the repair objective, to share hits."""
    if not candidates or not witnesses:
        return {}
    with tempfile.TemporaryDirectory(prefix="graphy-repair-cull-") as directory:
        library = Path(directory) / "witnesses.txt"
        library.write_text("".join(f"{h}\n" for h in sorted(set(witnesses))))
        result = subprocess.run([binary, "cull", str(n), "/dev/stdin", str(library)],
                                input="".join(f"{f}\n" for f in candidates), text=True,
                                capture_output=True, check=True,
                                env=os.environ | {"RAYON_NUM_THREADS": "1"})
    hits = {}
    checked = set()
    for line in result.stdout.splitlines():
        candidate, value = line.split(",", 1)
        candidate = int(candidate)
        checked.add(candidate)
        if value.startswith("Some(") and value.endswith(")"):
            hits[candidate] = int(value[5:-1])
        elif value != "None":
            raise RuntimeError(f"unexpected cull result: {line}")
    if checked != set(candidates):
        raise RuntimeError("cull did not check exactly the requested candidates")
    return hits


def tasks(groups, restarts, offset=0):
    # Round-robin over candidates, not entire candidate campaigns in one worker.
    for restart in range(offset, offset + restarts):
        for key, seeds in groups.items():
            yield key, seeds, restart


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("results", nargs="+")
    parser.add_argument("--binary", default=BINARY)
    parser.add_argument("--keep", type=int, default=8, help="isomorphism/complement classes per candidate")
    parser.add_argument("--max-score", type=int)
    parser.add_argument("--candidates", help="restrict to this current survivor file (decimal first column)")
    parser.add_argument("--candidate", type=int, action="append", help="restrict to this candidate; repeatable")
    parser.add_argument("--method", choices=["counted", "learned", "neighbourhood"], default="counted")
    parser.add_argument("--depth", type=int, default=4)
    parser.add_argument("--tabu", type=int, default=3)
    parser.add_argument("--noise", type=float, default=0.2)
    parser.add_argument("--moves", choices=["focused", "all"], default="all")
    parser.add_argument("--pair-every", type=int, default=0)
    parser.add_argument("--archive", type=int, default=8)
    parser.add_argument("--archive-slack", type=int, default=32)
    parser.add_argument("--seconds", type=float, default=10)
    parser.add_argument("--steps", type=int, default=1_000_000)
    parser.add_argument("--restarts", type=int, default=8)
    parser.add_argument("--restart-offset", type=int, default=0)
    parser.add_argument("--perturb", type=int, default=0)
    parser.add_argument("--rng-seed", type=int, default=0)
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--no-share", action="store_true", help="disable cross-candidate witness checks")
    parser.add_argument("--prepare-only", action="store_true", help="write selected seeds/cross-hits without launching walks")
    args = parser.parse_args()
    if args.keep < 1 or args.jobs < 1 or args.restarts < 1:
        parser.error("keep, jobs, and restarts must be positive")
    if min(args.pair_every, args.archive, args.archive_slack, args.restart_offset) < 0:
        parser.error("pair-every, archive, archive-slack, restart-offset must be nonnegative")
    if not math.isfinite(args.seconds) or args.seconds < 0 or not 0 <= args.noise <= 1:
        parser.error("seconds must be finite and nonnegative; noise must be in [0,1]")
    if min(args.steps, args.perturb, args.tabu, args.depth) < 0:
        parser.error("steps, perturb, tabu, and depth must be nonnegative")
    candidates = None
    if args.candidates:
        with open(args.candidates) as source:
            candidates = {int(line.split(",")[0]) for line in source if line.strip()}
    if args.candidate:
        candidates = set(args.candidate) if candidates is None else candidates & set(args.candidate)

    def report(key, labelled, classes, kept):
        print(f"seeds n={key[0]} F={key[1]}: {labelled} labels, {classes} classes, keeping {kept}",
              file=sys.stderr, flush=True)

    catalog = collect_results(args.results)
    groups = select_seeds([], args.keep, args.max_score, binary=args.binary,
                          candidates=candidates, report=report, catalog=catalog)
    if not groups:
        parser.error("no selected unrefuted candidates with complete exact scores in these files")
    lock = Lock()
    solved = set()
    writer = csv.DictWriter(sys.stdout, fieldnames=FIELDS, lineterminator="\n")
    writer.writeheader()
    sys.stdout.flush()

    def share(n, witnesses):
        with lock:
            remaining = [f for nn, f in groups if nn == n and (n, f) not in solved]
        hits = cross_refutations(args.binary, n, remaining, witnesses)
        with lock:
            for f, h in hits.items():
                if (n, f) in solved:
                    continue
                solved.add((n, f))
                row = {field: "" for field in FIELDS}
                row.update(n=n, candidate=f, refuter=h, saved_host=h, best_copies=0,
                           status="refuted", record="cross-refutation", method="CrossCheck")
                for field in ["flips", "checks", "constraints", "violated", "seconds", "oracle_seconds",
                              "pair_evaluations", "pair_moves", "outside_moves"]:
                    row[field] = 0
                writer.writerow(row)
                print(f"shared refuter n={n}: {h} refutes {f}", file=sys.stderr, flush=True)
            sys.stdout.flush()

    if not args.no_share:
        pool = {n: set(hosts) for n, hosts in catalog[2].items()}
        # A near-miss for its own target can already refute another candidate.
        for (n, _), hosts in groups.items():
            pool.setdefault(n, set()).update(hosts)
        for n, hosts in pool.items():
            share(n, hosts)

    # Carry the selected seeds forward too. Otherwise a perturbed, short walk
    # can lose an input minimum, making a chain of CSV-only resumptions regress.
    for (n, f), hosts in groups.items():
        if (n, f) in solved:
            continue
        for h in hosts:
            row = {field: "" for field in FIELDS}
            row.update(n=n, candidate=f, saved_host=h, best_copies=catalog[0][n, f][h],
                       status="checkpoint", record="seed", source_seed=h, method="Counted")
            for field in ["flips", "checks", "constraints", "violated", "seconds", "oracle_seconds",
                          "pair_evaluations", "pair_moves", "outside_moves"]:
                row[field] = 0
            writer.writerow(row)
    sys.stdout.flush()
    del catalog
    if args.prepare_only:
        return

    def run(item):
        (n, candidate), seeds, restart = item
        with lock:
            if (n, candidate) in solved:
                return None
        command = [args.binary, "ingraph-repair", str(n), "/dev/stdin", "--method", args.method,
                   "--seeds", ",".join(map(str, seeds)), "--random-every", "0", "--jobs", "1",
                   "--restarts", "1", "--restart-offset", str(restart)]
        for option in ["depth", "tabu", "noise", "seconds", "steps", "perturb", "rng_seed",
                       "moves", "pair_every", "archive", "archive_slack"]:
            command.extend(["--" + option.replace("_", "-"), str(getattr(args, option))])
        with lock:
            print(f"refine n={n} F={candidate} restart={restart}: {shlex.join(command)}",
                  file=sys.stderr, flush=True)
        found = None
        with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                              stderr=subprocess.PIPE, text=True) as process:
            def forward_errors():
                for line in process.stderr:
                    with lock:
                        sys.stderr.write(line)
                        sys.stderr.flush()

            errors = Thread(target=forward_errors, daemon=True)
            errors.start()
            try:
                process.stdin.write(str(candidate) + "\n")
                process.stdin.close()
                reader = csv.DictReader(process.stdout)
                if reader.fieldnames != FIELDS:
                    raise RuntimeError("unexpected repair CSV schema; rebuild graphy")
                for row in reader:
                    with lock:
                        writer.writerow(row)
                        sys.stdout.flush()
                        if row["status"] in ("refuted", "trivial-universal"):
                            solved.add((n, candidate))
                        if row["status"] == "refuted":
                            found = n, int(row["refuter"])
                if process.wait() != 0:
                    raise RuntimeError(f"graphy failed for candidate {candidate}")
            finally:
                if process.poll() is None:
                    process.terminate()
                    process.wait()
                errors.join()
        return found

    pending = iter(tasks(groups, args.restarts, args.restart_offset))
    # Only one task per worker is in flight. Hits skip future attempts; already
    # running walks finish and preserve their checkpoints (at most jobs-1 extra).
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        running = set()
        while True:
            while len(running) < args.jobs:
                item = next(pending, None)
                if item is None:
                    break
                if item[0] not in solved:
                    running.add(pool.submit(run, item))
            if not running:
                break
            finished, running = wait(running, return_when=FIRST_COMPLETED)
            for future in finished:
                hit = future.result()
                if hit and not args.no_share:
                    share(hit[0], [hit[1]])


if __name__ == "__main__":
    main()
