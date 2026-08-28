/* PRUNE hook for geng: reject any (partial) graph that already contains
   ALL of a list of fixed subgraphs, so geng enumerates exactly the union
   of the subgraph-free sets.  Sound because each partial graph is an
   induced subgraph of all its descendants and containment is monotone.
   With a single subgraph this is plain subgraph-free enumeration.

   GRAPHY_SUB is a comma-separated list of decimal bit numbers in graphy's
   encoding: pair (a,b) with a<b is bit b*(b-1)/2+a.  Trailing isolated
   vertices should be omitted (use the number as-is; the vertex count is
   inferred from the highest bit).  With several subgraphs, classify the
   output afterwards (e.g. graphy free-scan per subgraph): a graph is kept
   if it misses at least one of them.

   Per sub, the matcher anchors on the newest vertex, which is sound only
   if the parent lacked that sub; geng calls PRUNE parent-before-children
   in DFS order, so a per-level bitmask of already-contained subs carries
   that knowledge down.  Matching uses connected sub order, high-degree
   host slots first, and forward-checking of viable-slot masks. */

#include "gtools.h"
#include <stdlib.h>
#include <stdio.h>
#include <string.h>

#define MAXSUBS 63

static int inited = 0;
static int nsubs = 0;

typedef struct {
    int n;
    int deg[16];
    unsigned adj[16];
    int order[16];     /* matching order: components dense-first, connected */
    /* latest earlier-in-order interchangeable vertex, else -1; images are
       forced ascending along these chains, killing twin permutations */
    int twin_pred[16];
    /* all vertices interchangeable with v (swap is an automorphism) */
    unsigned twin_mask[16];
} subgraph;

typedef unsigned long long mask_t;

typedef unsigned __int128 bits_t;

static subgraph subs[MAXSUBS];

/* subs already contained at each level of the current DFS chain */
static mask_t level_mask[64];

static int sup_n;
static int sup_deg[16];
static unsigned sup_adj[16];
static int sup_order[16];  /* sup vertices by degree descending */
/* interchangeable earlier (in sup_order) host slot, else -1; while it is
   unused, this slot is redundant to try */
static int sup_twin_pred[16];

static const subgraph *cs; /* sub being matched */
static int cur_skip;       /* sub vertex pre-assigned to the anchor */
static unsigned used;      /* sup slots taken */
static unsigned assigned;  /* sub vertices placed */
static unsigned cand[16];  /* viable sup slots per sub vertex */
static int img[16];        /* sub vertex -> host slot once assigned */
static int undo_u[512];
static unsigned undo_m[512];
static int undo_sp;

static bits_t strtobits(const char *s)
{
    bits_t bits = 0;
    while (*s) {
        if (*s < '0' || *s > '9') { fprintf(stderr, ">E GRAPHY_SUB bad char\n"); exit(1); }
        bits = bits*10 + (*s - '0');
        s++;
    }
    return bits;
}

static void init_one(subgraph *s, bits_t bits)
{
    s->n = 2;
    while (bits >> (s->n*(s->n-1)/2)) s->n++;
    if (s->n > 16) { fprintf(stderr, ">E GRAPHY_SUB too large\n"); exit(1); }

    int k = 0;
    for (int b = 1; b < s->n; b++)
        for (int a = 0; a < b; a++, k++)
            if (bits >> k & 1) {
                s->adj[a] |= 1u << b;
                s->adj[b] |= 1u << a;
            }
    for (int v = 0; v < s->n; v++)
        s->deg[v] = __builtin_popcount(s->adj[v]);

    /* connected components, densest (edges/vertices) first so that an
       unembeddable dense component fails before sparse ones are explored;
       greedy connected order within each: most placed neighbors, then
       highest degree.  Isolated vertices land at the end. */
    unsigned comp[16];
    unsigned ce2[16], cv[16];
    int ncomp = 0;
    unsigned seen = 0;
    for (int v = 0; v < s->n; v++) {
        if (seen & (1u << v)) continue;
        unsigned m = 1u << v, grown;
        for (;;) {
            grown = m;
            for (int u = 0; u < s->n; u++)
                if (m & (1u << u)) grown |= s->adj[u];
            if (grown == m) break;
            m = grown;
        }
        seen |= m;
        unsigned e2 = 0;
        for (int u = 0; u < s->n; u++)
            if (m & (1u << u)) e2 += s->deg[u];
        comp[ncomp] = m; ce2[ncomp] = e2; cv[ncomp] = __builtin_popcount(m);
        ncomp++;
    }
    /* insertion sort, densest first, ties smaller first */
    for (int i = 1; i < ncomp; i++) {
        unsigned m = comp[i], e2 = ce2[i], v = cv[i];
        int j = i;
        while (j > 0 && (ce2[j-1]*v < e2*cv[j-1]
                || (ce2[j-1]*v == e2*cv[j-1] && cv[j-1] > v))) {
            comp[j] = comp[j-1]; ce2[j] = ce2[j-1]; cv[j] = cv[j-1]; j--;
        }
        comp[j] = m; ce2[j] = e2; cv[j] = v;
    }
    int oi = 0;
    for (int c = 0; c < ncomp; c++) {
        unsigned placed = 0;
        for (unsigned i = 0; i < cv[c]; i++) {
            int best = -1, bp = -1, bd = -1;
            for (int v = 0; v < s->n; v++) {
                if (!(comp[c] & (1u << v)) || (placed & (1u << v))) continue;
                int p = __builtin_popcount(s->adj[v] & placed);
                if (p > bp || (p == bp && s->deg[v] > bd)) {
                    best = v; bp = p; bd = s->deg[v];
                }
            }
            s->order[oi++] = best;
            placed |= 1u << best;
        }
    }
    for (int v = 0; v < s->n; v++) {
        unsigned tm = 0;
        for (int u = 0; u < s->n; u++) {
            if (u == v) continue;
            unsigned both = ~((1u << v) | (1u << u));
            if ((s->adj[v] & both) == (s->adj[u] & both)) tm |= 1u << u;
        }
        s->twin_mask[v] = tm;
    }
    for (int i = 0; i < s->n; i++) s->twin_pred[s->order[i]] = -1;
    for (int i = 1; i < s->n; i++) {
        int el = s->order[i];
        for (int k = i-1; k >= 0; k--)
            if (s->twin_mask[el] & (1u << s->order[k])) {
                s->twin_pred[el] = s->order[k];
                break;
            }
    }
}

static void init(void)
{
    const char *s = getenv("GRAPHY_SUB");
    if (!s) { fprintf(stderr, ">E GRAPHY_SUB not set\n"); exit(1); }
    char *buf = strdup(s), *save = NULL;
    for (char *tok = strtok_r(buf, ",", &save); tok;
            tok = strtok_r(NULL, ",", &save)) {
        if (nsubs == MAXSUBS) { fprintf(stderr, ">E too many subs\n"); exit(1); }
        bits_t bits = strtobits(tok);
        if (!bits) { fprintf(stderr, ">E GRAPHY_SUB empty graph\n"); exit(1); }
        memset(&subs[nsubs], 0, sizeof(subgraph));
        init_one(&subs[nsubs], bits);
        fprintf(stderr, ">A pruning graphs containing %s (%d vertices)%s\n",
            tok, subs[nsubs].n, nsubs ? " (conjunctive)" : "");
        nsubs++;
    }
    free(buf);
    inited = 1;
}

static int match(int i)
{
    if (i == cs->n) return 1;
    int el = cs->order[i];
    if (assigned & (1u << el)) return match(i + 1);
    unsigned free_cand = cand[el] & ~used;
    if (!free_cand) return 0;
    /* twin chain: image forced above the interchangeable predecessor's;
       the anchored vertex is excluded, relink past it when it intervenes */
    int p = cs->twin_pred[el];
    if (p == cur_skip) {
        p = p < 0 ? -1 : cs->twin_pred[p];
        if (p >= 0 && !(cs->twin_mask[el] & (1u << p))) p = -1;
    }
    if (p >= 0 && (assigned & (1u << p)))
        free_cand &= ~((2u << img[p]) - 1);
    if (!free_cand) return 0;
    for (int jx = 0; jx < sup_n; jx++) {
        int j = sup_order[jx];
        unsigned jb = 1u << j;
        if (!(free_cand & jb)) continue;
        /* while an interchangeable earlier host slot is unused, j is
           redundant to try */
        int hp = sup_twin_pred[j];
        if (hp >= 0 && !(used & (1u << hp))) continue;
        img[el] = j;
        used |= jb;
        assigned |= 1u << el;
        int sp0 = undo_sp;
        int ok = 1;
        unsigned nbrs = cs->adj[el] & ~assigned;
        while (nbrs) {
            int u = __builtin_ctz(nbrs); nbrs &= nbrs-1;
            unsigned nc = cand[u] & sup_adj[j];
            if (nc != cand[u]) {
                undo_u[undo_sp] = u; undo_m[undo_sp++] = cand[u];
                cand[u] = nc;
            }
            if (!(nc & ~used)) { ok = 0; break; }
        }
        if (ok && match(i + 1)) return 1;
        while (undo_sp > sp0) { --undo_sp; cand[undo_u[undo_sp]] = undo_m[undo_sp]; }
        assigned &= ~(1u << el);
        used &= ~jb;
    }
    return 0;
}

/* does the host contain sub s via an embedding using the newest vertex? */
static int contains_anchored(const subgraph *s)
{
    cs = s;
    unsigned cand0[16];
    for (int v = 0; v < s->n; v++) {
        unsigned m = 0;
        for (int j = 0; j < sup_n; j++)
            if (sup_deg[j] >= s->deg[v]) m |= 1u << j;
        cand0[v] = m;
    }
    int anchor = sup_n - 1;
    unsigned ab = 1u << anchor;
    unsigned tried = 0;
    for (int skip_el = 0; skip_el < s->n; skip_el++) {
        if (!(cand0[skip_el] & ab)) continue;
        /* a twin of an already-tried anchor image reaches the same graphs */
        if (s->twin_mask[skip_el] & tried) continue;
        tried |= 1u << skip_el;
        cur_skip = skip_el;
        memcpy(cand, cand0, s->n * sizeof(unsigned));
        used = ab;
        assigned = 1u << skip_el;
        undo_sp = 0;
        int ok = 1;
        unsigned nbrs = s->adj[skip_el];
        while (nbrs) {
            int u = __builtin_ctz(nbrs); nbrs &= nbrs-1;
            cand[u] &= sup_adj[anchor];
            if (!(cand[u] & ~used)) { ok = 0; break; }
        }
        if (ok && match(0)) return 1;
    }
    return 0;
}

int prune_sub(graph *g, int n, int maxn)
{
    if (!inited) init();
    mask_t pmask = n > 1 ? level_mask[n-1] : 0;
    mask_t all = (1ull << nsubs) - 1;
    if (pmask == all) return 1;   /* ancestor already contained everything */
    sup_n = n;
    for (int i = 0; i < n; i++) {
        setword row = g[i];
        unsigned m = 0;
        for (int j = 0; j < n; j++)
            if (row & bit[j]) m |= 1u << j;
        sup_adj[i] = m;
        sup_deg[i] = __builtin_popcount(m);
    }
    for (int i = 0; i < n; i++) {
        int j = i;
        while (j > 0 && sup_deg[sup_order[j-1]] < sup_deg[i]) {
            sup_order[j] = sup_order[j-1]; j--;
        }
        sup_order[j] = i;
    }
    /* host twin chains along sup_order; twins share a degree, so the
       degree-sorted scan can stop at the first degree change */
    for (int jx = 0; jx < n; jx++) {
        int j = sup_order[jx];
        sup_twin_pred[j] = -1;
        for (int kx = jx-1; kx >= 0; kx--) {
            int u = sup_order[kx];
            if (sup_deg[u] != sup_deg[j]) break;
            unsigned both = ~((1u << j) | (1u << u));
            if ((sup_adj[j] & both) == (sup_adj[u] & both)) {
                sup_twin_pred[j] = u;
                break;
            }
        }
    }
    mask_t mask = pmask;
    for (int s = 0; s < nsubs; s++) {
        mask_t sbit = 1ull << s;
        if (mask & sbit) continue;
        if (n < subs[s].n) continue;
        if (contains_anchored(&subs[s])) mask |= sbit;
    }
    level_mask[n] = mask;
    return mask == all;
}
