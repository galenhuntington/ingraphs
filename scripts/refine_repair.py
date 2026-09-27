#!/usr/bin/env python3
"""Resume the best candidate-specific hosts from counted-repair CSV logs.

Select the lowest exact copy counts per (n, candidate), and launch graphy
walks from those hosts. Stream/flush completed restart rows immediately.
Input files are read-only; stdout is the normal ingraph-repair CSV.
"""

import argparse
from concurrent.futures import ThreadPoolExecutor
import csv
from pathlib import Path
import shlex
import subprocess
import sys
from threading import Lock, Thread


def select_seeds(paths, keep, max_score=None):
    groups = {}
    refuted = set()
    for path in paths:
        with open(path, newline="") as source:
            for row in csv.DictReader(source):
                key = int(row["n"]), int(row["candidate"])
                if row["status"] == "refuted":
                    refuted.add(key)
                if not row.get("best_copies"):
                    continue
                score = int(row["best_copies"])
                if max_score is not None and score > max_score:
                    continue
                host = int(row["saved_host"])
                hosts = groups.setdefault(key, {})
                hosts[host] = min(score, hosts.get(host, score))
    return {key: [h for h, _ in sorted(hosts.items(), key=lambda pair: (pair[1], pair[0]))[:keep]]
            for key, hosts in sorted(groups.items()) if key not in refuted}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("results", nargs="+")
    parser.add_argument("--binary", default=str(Path(__file__).resolve().parents[1] / "target/release/graphy"))
    parser.add_argument("--keep", type=int, default=3, help="distinct labelled seeds per candidate")
    parser.add_argument("--max-score", type=int)
    parser.add_argument("--method", choices=["counted", "learned", "neighbourhood"], default="counted")
    parser.add_argument("--depth", type=int, default=4)
    parser.add_argument("--tabu", type=int, default=7)
    parser.add_argument("--noise", type=float, default=0.3)
    parser.add_argument("--seconds", type=float, default=10)
    parser.add_argument("--steps", type=int, default=1_000_000)
    parser.add_argument("--restarts", type=int, default=8)
    parser.add_argument("--perturb", type=int, default=0)
    parser.add_argument("--rng-seed", type=int, default=0)
    parser.add_argument("--jobs", type=int, default=4)
    args = parser.parse_args()
    if args.keep < 1 or args.jobs < 1 or args.restarts < 1:
        parser.error("keep, jobs, and restarts must be positive")
    groups = select_seeds(args.results, args.keep, args.max_score)
    if not groups:
        parser.error("no unrefuted candidates with complete exact scores in these files")
    lock = Lock()
    header = None

    def run(item):
        nonlocal header
        (n, candidate), seeds = item
        command = [args.binary, "ingraph-repair", str(n), "/dev/stdin", "--method", args.method,
                   "--seeds", ",".join(map(str, seeds)), "--random-every", "0", "--jobs", "1"]
        for option in ["depth", "tabu", "noise", "seconds", "steps", "restarts", "perturb", "rng_seed"]:
            command.extend(["--" + option.replace("_", "-"), str(getattr(args, option))])
        print(f"refine candidate={candidate}: {shlex.join(command)}", file=sys.stderr, flush=True)
        with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                              stderr=subprocess.PIPE, text=True) as process:
            def forward_errors():
                for line in process.stderr:
                    with lock:
                        sys.stderr.write(line)
                        sys.stderr.flush()

            errors = Thread(target=forward_errors, daemon=True)
            errors.start()
            process.stdin.write(str(candidate) + "\n")
            process.stdin.close()
            for line in process.stdout:
                with lock:
                    if line.startswith("n,candidate,"):
                        if header is not None:
                            if line != header:
                                raise RuntimeError("inconsistent output headers")
                            continue
                        header = line
                    sys.stdout.write(line)
                    sys.stdout.flush()
            code = process.wait()
            errors.join()
            if code != 0:
                raise RuntimeError(f"graphy failed for candidate {candidate}")

    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        list(pool.map(run, groups.items()))


if __name__ == "__main__":
    main()
