#!/usr/bin/env python3
"""Score candidate-specific free-close hosts and export repair-compatible seeds.

Read headerless runsN/F-*/found-*.csv files without modifying them. Sample each
edge-count layer, deduplicate by exact isomorphism/complement keys, then rank
by complete two-colour copy counts. Neither edge-maximality nor the old boolean
column is a substitute for recounting. Timed-out scores are never used.
"""

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import csv
import hashlib
import io
import math
from pathlib import Path
import subprocess
import sys

from refine_repair import BINARY, FIELDS, canonical_keys


EXTRA = ["source_file", "host_edges", "red_copies", "blue_copies"]


def collect_hosts(root, n, candidates):
    """Return F -> {H: (edge count, first source)}; preserve the F-free orientation."""
    groups = {}
    mask = (1 << (n * (n - 1) // 2)) - 1
    for directory in sorted(Path(root).iterdir()):
        prefix, separator, _ = directory.name.partition("-")
        if not directory.is_dir() or not separator or not prefix.isdecimal():
            continue
        f = int(prefix)
        if f not in candidates:
            continue
        if f & ~mask:
            raise ValueError(f"candidate does not fit n={n}: {f}")
        for path in sorted(directory.glob("found-*.csv")):
            with path.open() as source:
                for line_number, line in enumerate(source, 1):
                    if not line.strip():
                        continue
                    try:
                        fields = line.split(",", 3)
                        h, edges = map(int, fields[:2])
                        if h < 0 or h & ~mask or h.bit_count() != edges or fields[2] not in ("true", "false"):
                            raise ValueError("invalid host, edge count, or boolean")
                    except (ValueError, IndexError) as error:
                        raise ValueError(f"{path}:{line_number}: invalid free-close row") from error
                    groups.setdefault(f, {}).setdefault(h, (edges, str(path)))
    return groups


def sample_layers(hosts, per_layer, seed, candidate):
    """Order-independent deterministic sampling; 0 means score every raw host."""
    layers = {}
    for h, (edges, _) in hosts.items():
        layers.setdefault(edges, []).append(h)

    def rank(h):
        return hashlib.blake2b(f"{seed}:{candidate}:{h}".encode(), digest_size=16).digest(), h

    selected = []
    for _, layer in sorted(layers.items()):
        selected.extend(sorted(layer, key=rank)[:per_layer or None])
    return selected


def score_hosts(binary, n, candidate, hosts, seconds):
    result = subprocess.run([binary, "ingraph-count", str(n), str(candidate), "/dev/stdin",
                             "--seconds", str(seconds), "--jobs", "1"],
                            input="".join(f"{h}\n" for h in hosts), text=True,
                            stdout=subprocess.PIPE, check=True)
    rows = list(csv.DictReader(io.StringIO(result.stdout)))
    if len(rows) != len(hosts) or {int(row["host"]) for row in rows} != set(hosts):
        raise RuntimeError("counter returned the wrong hosts")
    return rows


def seed_row(n, candidate, score, source):
    h = int(score["host"])
    total = int(score["total_copies"])
    row = dict.fromkeys(FIELDS + EXTRA, "")
    row.update(n=n, candidate=candidate, saved_host=h, source_seed=h,
               best_copies=total, status="refuted" if total == 0 else "checkpoint",
               refuter=h if total == 0 else "", record="free-close", method="Counted",
               source_file=source, host_edges=score["edges"], red_copies=score["red_copies"],
               blue_copies=score["blue_copies"], seconds=score["seconds"],
               oracle_seconds=score["seconds"])
    for field in ["flips", "checks", "constraints", "violated", "pair_evaluations", "pair_moves",
                  "outside_moves", "best_step", "penalty_updates", "penalty_constraints"]:
        row[field] = 0
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("n", type=int)
    parser.add_argument("--root", type=Path, help="default: output/runsN")
    parser.add_argument("--candidates", type=Path, help="default: output/batches/allN")
    parser.add_argument("--candidate", type=int, action="append", help="further restrict; repeatable")
    parser.add_argument("--binary", default=BINARY)
    parser.add_argument("--per-layer", type=int, default=32, help="raw samples per candidate/edge count; 0 takes all")
    parser.add_argument("--keep", type=int, default=16, help="best classes per candidate; 0 keeps all completed scores")
    parser.add_argument("--seconds", type=float, default=0.25, help="counting budget per host; 0 disables")
    parser.add_argument("--jobs", type=int, default=4, help="candidates counted concurrently")
    parser.add_argument("--rng-seed", type=int, default=0)
    args = parser.parse_args()
    if args.n < 1 or args.jobs < 1 or min(args.per_layer, args.keep) < 0:
        parser.error("n/jobs must be positive; per-layer/keep must be nonnegative")
    if not math.isfinite(args.seconds) or args.seconds < 0:
        parser.error("seconds must be finite and nonnegative")
    candidates_path = args.candidates or Path(f"output/batches/all{args.n}")
    with candidates_path.open() as source:
        candidates = {int(line.split(",")[0]) for line in source if line.strip()}
    if args.candidate:
        candidates &= set(args.candidate)
    groups = collect_hosts(args.root or Path(f"output/runs{args.n}"), args.n, candidates)
    missing = sorted(candidates - groups.keys())
    if missing:
        print(f"no free-close hosts for {len(missing)} candidates: {missing}", file=sys.stderr)

    def prepare(item):
        f, hosts = item
        sample = sample_layers(hosts, args.per_layer, args.rng_seed, f)
        keys = canonical_keys(args.binary, args.n, sample)
        unique = {}
        for h, key in zip(sample, keys, strict=True):
            unique.setdefault(key, h)
        scores = score_hosts(args.binary, args.n, f, list(unique.values()), args.seconds)
        complete = [r for r in scores if r["status"] != "time-limit"]
        valid = [r for r in complete if int(r["red_copies"]) == 0]
        if len(valid) != len(complete):
            print(f"WARNING F={f}: {len(complete) - len(valid)} input hosts were NOT F-free; dropping them",
                  file=sys.stderr, flush=True)
        valid.sort(key=lambda r: (int(r["total_copies"]), int(r["host"])))
        kept = valid[:args.keep or None]
        best = valid[0]["total_copies"] if valid else "unknown"
        print(f"free-close F={f}: {len(hosts)} raw hosts, {len(sample)} sampled, {len(unique)} classes, "
              f"{len(scores) - len(complete)} timed out, keeping {len(kept)}, best copies={best}",
              file=sys.stderr, flush=True)
        return [seed_row(args.n, f, r, hosts[int(r["host"])][1]) for r in kept]

    writer = csv.DictWriter(sys.stdout, fieldnames=FIELDS + EXTRA, lineterminator="\n")
    writer.writeheader()
    sys.stdout.flush()
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        pending = [pool.submit(prepare, item) for item in sorted(groups.items())]
        for future in as_completed(pending):
            writer.writerows(future.result())
            sys.stdout.flush()


if __name__ == "__main__":
    main()
