# gengf: geng restricted to subgraph-free graphs

`gengf` is nauty's `geng` compiled with a `PRUNE` hook (`prune_sub.c`)
that rejects any graph — including partial graphs during generation —
containing a fixed subgraph.  Since geng builds each graph as a chain of
induced subgraphs and containment is monotone, whole subtrees are cut as
soon as the subgraph appears, so `gengf` enumerates exactly the
subgraph-free graphs, usually far faster than generate-then-filter.

The subgraph is passed in the environment variable `GRAPHY_SUB`, in
graphy's decimal encoding (pair (a,b), a<b, is bit b(b-1)/2+a).  Omit
trailing isolated vertices (i.e. just use the number): the fewer
vertices the subgraph has, the earlier the pruning starts.  The matcher
anchors on the newest vertex (sound because the parent already passed
the test), tries high-degree host vertices first, and forward-checks
viable-slot masks.

`GRAPHY_SUB` may also be a comma-separated list; then a graph is pruned
only when it contains *all* of the listed subgraphs, so the output is
the union of the individual sub-free sets (classify afterwards with
`graphy free-scan` per sub — the outputs are tiny).  Anchoring stays
sound via a per-level record of which subs each ancestor already
contained.  In tests on sibling candidates the union run cost only
slightly less than separate runs (~10%), so this is mainly a
convenience.

Example — all 13-vertex 39-edge graphs not containing candidate
69539838912, one slice of 600000:

    GRAPHY_SUB=69539838912 ./gengf -q 13 39:39 123/600000

Output is graph6, which graphy reads natively.  Rebuild with `./build.sh`
(fetches the nauty source tarball on first run).

Validated against `geng | graphy free-scan`: identical result sets on
all 10-vertex tranches 18..24 for sub 2202040 (49,956 graphs) and on
13-vertex 39-edge slices for two different candidates.
