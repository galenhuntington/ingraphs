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
    parser.add_argument("--method", choices=["learned", "counted"], default="learned")
    args = parser.parse_args()
    if args.trials < 1 or args.seconds < 0 or any(d < 0 for d in args.perturb):
        parser.error("invalid budgets")
    controls = []
    for name in ["refutations-2026-09-17.csv", "refutations-2026-09-23.csv"]:
        with (ROOT / "research" / name).open(newline="") as source:
            controls.extend(row for row in csv.DictReader(source) if row["n"] == "14")
    writer = None
    for control in controls:
        for distance in args.perturb:
            for trial in range(args.trials):
                command = [args.binary, "ingraph-repair", "14", "/dev/stdin",
                           "--seeds", control["refuter"], "--restarts", "1",
                           "--random-every", "0", "--perturb", str(distance),
                           "--seconds", str(args.seconds), "--rng-seed", str(trial),
                           "--batch", str(args.batch), "--noise", str(args.noise)]
                command.extend(["--method", args.method])
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
