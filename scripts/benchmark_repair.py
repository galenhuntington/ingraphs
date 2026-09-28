#!/usr/bin/env python3
"""Positive controls for ingraph-repair: damage known refuters and try to repair.

Streams the normal repair CSV. Each trial is a separate walk, even after a
success. The undamaged refuter is used ONLY to construct the initial host;
the solver gets no target assignment or list of edges that were changed.
"""

import argparse
import csv
import io
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=str(ROOT / "target/release/graphy"))
    parser.add_argument("--trials", type=int, default=4)
    parser.add_argument("--seconds", type=float, default=2)
    parser.add_argument("--perturb", type=int, nargs="+", default=[0, 1, 2, 4, 8])
    parser.add_argument("--batch", type=int, default=32)
    parser.add_argument("--noise", type=float, default=0.3)
    parser.add_argument("--method", choices=["learned", "counted"], default="counted")
    parser.add_argument("--moves", choices=["focused", "all"], default="all")
    parser.add_argument("--pair-every", type=int, default=0)
    parser.add_argument("--tabu", type=int, default=7)
    parser.add_argument("--legacy", action="store_true", help="omit new flags for an older baseline binary")
    parser.add_argument("--controls", nargs="+", type=Path, help="CSV files with n,candidate,refuter; overrides built-in controls")
    args = parser.parse_args()
    if args.trials < 1 or args.seconds < 0 or any(d < 0 for d in args.perturb):
        parser.error("invalid budgets")
    controls = []
    paths = args.controls or [ROOT / "research" / name for name in
                             ["refutations-2026-09-17.csv", "refutations-2026-09-23.csv"]]
    for path in paths:
        with path.open(newline="") as source:
            controls.extend(row for row in csv.DictReader(source)
                            if row.get("refuter") and (args.controls is not None or row["n"] == "14"))
    if not controls:
        parser.error("no controls with nonempty refuters")
    writer = None
    for control in controls:
        for distance in args.perturb:
            for trial in range(args.trials):
                command = [args.binary, "ingraph-repair", control["n"], "/dev/stdin",
                           "--seeds", control["refuter"], "--restarts", "1",
                           "--random-every", "0", "--perturb", str(distance),
                           "--seconds", str(args.seconds), "--rng-seed", str(trial),
                           "--batch", str(args.batch), "--noise", str(args.noise)]
                command.extend(["--method", args.method, "--tabu", str(args.tabu)])
                if not args.legacy:
                    command.extend(["--moves", args.moves, "--pair-every", str(args.pair_every),
                                    "--archive", "0"])
                result = subprocess.run(command, input=control["candidate"] + "\n",
                                        text=True, stdout=subprocess.PIPE, check=True)
                rows = list(csv.DictReader(io.StringIO(result.stdout)))
                if len(rows) != 1:
                    raise RuntimeError(f"expected one trial, got {len(rows)}")
                row = rows[0] | {"perturb": distance, "trial": trial,
                                 "control_refuter": control["refuter"]}
                if writer is None:
                    writer = csv.DictWriter(sys.stdout, fieldnames=list(row))
                    writer.writeheader()
                writer.writerow(row)
                sys.stdout.flush()


if __name__ == "__main__":
    main()
