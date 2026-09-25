#include <stdint.h>
#include <string.h>
#include "nauty.h"

_Static_assert(WORDSIZE == 32 && MAXN == 32 && MAXM == 1, "wrong nauty build");
_Static_assert(HAVE_TLS, "all nauty objects must use thread-local storage");

/* Both arrays have 32 entries; bit v of rows[u] denotes edge uv.
 * The caller supplies valid simple undirected adjacency rows. No pointer is
 * retained, and no nauty-owned type or allocation crosses this boundary. */
int graphy_canon(uint32_t n, const uint32_t *rows, uint32_t *out)
{
    if (n > 32) return -1;
    memset(out, 0, 32 * sizeof(uint32_t));
    if (n == 0) return 0;
    graph g[32], canon[32];
    int lab[32], ptn[32], orbits[32];
    DEFAULTOPTIONS_GRAPH(options);
    statsblk stats;
    static TLS_ATTR int checked = 0;
    if (!checked) {
        nauty_check(WORDSIZE, 1, 32, NAUTYVERSIONID);
        checked = 1;
    }
    options.getcanon = TRUE;
    EMPTYGRAPH(g, 1, n);
    for (uint32_t u = 0; u < n; ++u)
        for (uint32_t v = u + 1; v < n; ++v)
            if ((rows[u] >> v) & 1) ADDONEEDGE(g, u, v, 1);
    densenauty(g, lab, ptn, orbits, &options, &stats, 1, n, canon);
    if (stats.errstatus) return stats.errstatus;
    for (uint32_t u = 0; u < n; ++u)
        for (uint32_t v = 0; v < n; ++v)
            if (ISELEMENT(GRAPHROW(canon, u, 1), v)) out[u] |= UINT32_C(1) << v;
    return 0;
}
