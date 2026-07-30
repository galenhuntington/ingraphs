/* PRUNE hook for geng: reject any (partial) graph that already contains a
   fixed subgraph SUB, so geng enumerates exactly the SUB-free graphs.
   Sound because each partial graph is an induced subgraph of all its
   descendants, and subgraph containment is monotone.

   SUB is given in the environment variable GRAPHY_SUB as a decimal bit
   number in graphy's encoding: pair (a,b) with a<b is bit b*(b-1)/2+a.
   Trailing isolated vertices should be omitted (use the number as-is;
   the vertex count is inferred from the highest bit). */

#include "gtools.h"
#include <stdlib.h>
#include <stdio.h>
#include <string.h>

static int inited = 0;
static int sub_n = 0;
static int sub_deg[16];
static unsigned sub_adj[16];
static int order[16];      /* matching order: connected, degree descending */

static int sup_n;
static int sup_deg[16];
static unsigned sup_adj[16];
static unsigned req[16];
static unsigned used;

static void init(void)
{
    const char *s = getenv("GRAPHY_SUB");
    if (!s) { fprintf(stderr, ">E GRAPHY_SUB not set\n"); exit(1); }
    unsigned long long bits = strtoull(s, NULL, 10);
    if (!bits) { fprintf(stderr, ">E GRAPHY_SUB empty graph\n"); exit(1); }

    /* vertex count: smallest t with all bits below t*(t-1)/2 */
    sub_n = 2;
    while (bits >> (sub_n*(sub_n-1)/2)) sub_n++;
    if (sub_n > 16) { fprintf(stderr, ">E GRAPHY_SUB too large\n"); exit(1); }

    int k = 0;
    for (int b = 1; b < sub_n; b++)
        for (int a = 0; a < b; a++, k++)
            if (bits >> k & 1) {
                sub_adj[a] |= 1u << b;
                sub_adj[b] |= 1u << a;
            }
    for (int v = 0; v < sub_n; v++)
        sub_deg[v] = __builtin_popcount(sub_adj[v]);

    /* greedy connected order: most placed neighbors, then highest degree */
    unsigned placed = 0;
    for (int i = 0; i < sub_n; i++) {
        int best = -1, bp = -1, bd = -1;
        for (int v = 0; v < sub_n; v++) {
            if (placed & (1u << v)) continue;
            int p = __builtin_popcount(sub_adj[v] & placed);
            if (p > bp || (p == bp && sub_deg[v] > bd)) {
                best = v; bp = p; bd = sub_deg[v];
            }
        }
        order[i] = best;
        placed |= 1u << best;
    }
    inited = 1;
    fprintf(stderr, ">A pruning graphs containing GRAPHY_SUB=%llu (%d vertices)\n",
        bits, sub_n);
}

static int skip_el;   /* sub vertex pre-assigned to the anchor */

static int match(int i)
{
    if (i == sub_n) return 1;
    int el = order[i];
    if (el == skip_el) return match(i + 1);
    int el_deg = sub_deg[el];
    unsigned req_el = req[el];
    for (int j = 0; j < sup_n; j++) {
        unsigned jb = 1u << j;
        if (used & jb) continue;
        if (sup_deg[j] < el_deg) continue;
        if (req_el & ~sup_adj[j]) continue;
        used |= jb;
        unsigned nbrs = sub_adj[el];
        while (nbrs) { int u = __builtin_ctz(nbrs); nbrs &= nbrs-1; req[u] |= jb; }
        if (match(i+1)) return 1;
        nbrs = sub_adj[el];
        while (nbrs) { int u = __builtin_ctz(nbrs); nbrs &= nbrs-1; req[u] &= ~jb; }
        used &= ~jb;
    }
    return 0;
}

int prune_sub(graph *g, int n, int maxn)
{
    if (!inited) init();
    if (n < sub_n) return 0;
    sup_n = n;
    for (int i = 0; i < n; i++) {
        setword row = g[i];
        unsigned m = 0;
        for (int j = 0; j < n; j++)
            if (row & bit[j]) m |= 1u << j;
        sup_adj[i] = m;
        sup_deg[i] = __builtin_popcount(m);
    }
    /* The parent (without the last vertex) already passed, so any embedding
       must use the newest vertex: try each sub vertex as its image. */
    int anchor = n - 1;
    unsigned ab = 1u << anchor;
    for (skip_el = 0; skip_el < sub_n; skip_el++) {
        if (sub_deg[skip_el] > sup_deg[anchor]) continue;
        used = ab;
        memset(req, 0, sub_n * sizeof(unsigned));
        unsigned nbrs = sub_adj[skip_el];
        while (nbrs) { int u = __builtin_ctz(nbrs); nbrs &= nbrs-1; req[u] |= ab; }
        if (match(0)) return 1;
    }
    return 0;
}
