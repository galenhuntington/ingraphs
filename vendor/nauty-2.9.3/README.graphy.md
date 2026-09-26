# Pinned nauty source subset

Unmodified files from Brendan McKay and Adolfo Piperno's nauty 2.9.3:

- Source: https://users.cecs.anu.edu.au/~bdm/nauty/nauty2_9_3.tar.gz
- Archive SHA-256: `9fc4edae04f88a0f5883985be3b39cf7f898fd6cc96e96b9ee25452743cc1b5b`
- License: see `COPYRIGHT` and `LICENSE-2.0.txt`. The upstream configure
  helpers retain their own license notices.

Only the dense canonicalizer's five C translation units, required headers,
header templates, and upstream configuration helpers are included. No
configured headers or object files are checked in. `native/build.rs` runs
upstream configure in Cargo's OUT_DIR and compiles the adapter and all C
objects with consistent WORDSIZE=32, MAXN=32 and TLS options.

No code in this directory should be edited to customize graphy; the adapter
lives in `native/nauty_shim.c`. Updating nauty requires renewing the source
checksum, checking the include closure, and rerunning canonicalization,
successor and seek tests. Canonical labels are not a stable file-format
promise across versions or option choices; graphy normalizes cache inputs.
