use crate::base;
use crate::base::{BITNUM_ONE,BITNUM_ZERO,BitNum,BitVec,Graph,MAX_SIZE,Triangle,Bits};
use crate::tools::one_bits;
use crate::perm::Perm;
use std::cmp::Ordering::*;

struct Fixed<'a, CB: FnMut(base::BitNum)> {
    pub size: usize,
    pub line: &'a mut Vec<BitNum>,
    pub callback: CB,
    pub filter: (usize, usize),
    // pinned row values for the top vertices, restricting to a subtree
    pub prefix: &'a [BitNum],
}


struct Recursed {
    pub at: usize,
    pub break_bits: BitNum,
    pub so_far: BitNum,
    pub recheck: bool,
}

#[inline]
fn get_breaks(bits: BitNum) -> BitNum {
    !bits & (bits >> 1usize)
}

fn smoosh(row: BitNum, breaks: BitNum) -> BitNum {
    let mut row = row;
    let mut result = BITNUM_ZERO;
    let breaks = BitVec(breaks);
    // eprintln!("{} {}", row, breaks.0);
    loop {
        if row == 0 { return result }
        let start = (BitNum::BITS - 1 - row.leading_zeros()) as usize;
        let mut find = 0;
        for i in 1.. {
            if i > start { break; }
            let j = start - i;
            if breaks.get(j) {
                find = j + 1;
                break;
            }
        }
        let mask = (BITNUM_ONE << find) - BITNUM_ONE;
        let cnt = (row & !mask).count_ones();
        // eprintln!("start={} find={} mask={}", start, find, mask);
        row &= mask;
        result |= ((BITNUM_ONE << cnt) - BITNUM_ONE) << find;
    }
}

fn new_permute(
    cur: BitNum,
    pt: usize,
    swap: usize,
    slice: BitVec,
    target: BitVec,
) -> BitNum {
    let mut perm = vec![!0; pt + 1];
    for bit in [false, true] {
        let mut i = 0;
        let mut j = 0;
        while i <= pt {
            if slice.get(i) != bit { i += 1; continue }
            if target.get(j) != bit { j += 1; continue }
            // let j2 = if i == swap { pt } else { j };
            // let i2 = if j == pt { swap } else { i };
            perm[i] = j;
            i += 1;
            j += 1;
            assert!(j <= pt + 1);
        }
    }
    perm[pt] = perm[swap];
    perm[swap] = pt;
    // eprintln!("perm={:?}", perm);
    let mask = one_bits(Graph::triangle(pt + 1));
    let new_cur = (cur & !mask)
        | Graph::from_bits(pt + 1, cur & mask).renumber(&Perm::new_unchecked(perm)).edges.0.0;
    // eprintln!("cur={:b}, new_cur={:b}", cur, new_cur);
    // double check
    /*
    while perm.len() < 10 {
        perm.push(perm.len());
    }
    let other_cur = Graph::from_bits(10, cur).renumber(&Perm::new(perm.clone())).edges.0.0;
    assert_eq!(new_cur, other_cur, "cur={:b}, new_cur={:b}, other_cur={:b}, perm={:?}",
        cur, new_cur, other_cur, &perm);
    */
    new_cur
}

trait RVal: Sized + Copy {
    type Score;
    const FAIL_FAST: bool;
    fn score(bn: BitNum) -> Self::Score;
    fn pick_best(_v: Self, _sc: Self::Score) -> Self::Score;
    fn val(sc: Self::Score) -> Self;
    fn fail() -> Self { unreachable!("Invalid RVal::fail.") }
    fn fail_fast_on(_v: Self) -> bool { false }
}

impl RVal for bool {
    type Score = ();
    const FAIL_FAST: bool = true;
    fn score(_bn: BitNum) {}
    fn pick_best(_v: Self, _sc: Self::Score) {}
    fn val(_sc: ()) -> bool { true }
    fn fail() -> bool { false }
    fn fail_fast_on(v: Self) -> bool { !v }
}

impl RVal for BitNum {
    type Score = BitNum;
    const FAIL_FAST: bool = false;
    fn score(bn: BitNum) -> BitNum { bn }
    fn pick_best(v: BitNum, sc: BitNum) -> BitNum { v.min(sc) }
    fn val(bn: BitNum) -> BitNum { bn }
}

// Adjacency of v among all vertices, from the triangle bits
fn vert_mask(tri: &Triangle, v: usize) -> u32 {
    let mut m = 0u32;
    for u in 0..MAX_SIZE {
        if u != v && tri.get((u, v)) { m |= 1 << u }
    }
    m
}

// The old way didn't really work, new approach.
fn new_recurse<T: RVal>(
    cur: BitNum,
    pt: usize,
    break_bits: BitNum,
    cutoff: BitNum,
) -> T {
    if pt == 0 { return T::val(T::score(cur)) }
    let tri = Triangle(BitVec(cur));
    let basis = (cutoff >> Graph::triangle(pt)) & one_bits(pt);
    let next_break = break_bits | (basis & !(basis >> 1usize));
    /*
    if !new_recurse(cur, pt - 1, next_break, cutoff) {
        return false;
    }
    */
    let mut best = T::score(cur);
    // vertices already recursed on, for twin skipping; masks computed lazily
    // so nodes with a single surviving branch pay nothing
    let mut tried_v = [0usize; MAX_SIZE];
    let mut tried_m = [0u32; MAX_SIZE];
    let mut tried_ct = 0;
    let mut masks_done = 0;
    'swaps: for swap in (0..=pt).rev() {
        // eprintln!("break_bits={:b} pt={} swap={}", break_bits, pt, swap);
        if pt != swap && BitVec(break_bits).get(swap) { break }
        let slice = if swap == pt {
            // this optimization barely helps
            BitVec((cur >> Graph::triangle(pt)) & one_bits(pt))
        } else {
            let mut slice = BitVec::new();
            for bit in 0 .. pt {
                if tri.get((swap, if bit == swap { pt } else { bit })) {
                    slice.set(bit)
                }
            }
            slice
        };
        let cand = smoosh(slice.0, break_bits);
        /*
        eprintln!("cur={} ({}) pt={} swap={} slice={:b} cand={:b} basis={:b} break_bits={:b}",
            cur, Graph::from_bits(10, cur), pt, swap, slice.0, cand, basis, break_bits);
        */
        match cand.cmp(&basis) {
            Less => if T::FAIL_FAST { return T::fail() },
            Greater => continue,
            Equal => {},
        }
        // Skip twins: if swap has the same neighborhood as a vertex already
        // recursed on (ignoring one another), the transposition is an
        // automorphism, so this branch reaches the same values.
        if tried_ct > 0 {
            while masks_done < tried_ct {
                tried_m[masks_done] = vert_mask(&tri, tried_v[masks_done]);
                masks_done += 1;
            }
            let mask = vert_mask(&tri, swap);
            for i in 0..tried_ct {
                let both = !((1u32 << swap) | (1 << tried_v[i]));
                if mask & both == tried_m[i] & both { continue 'swaps }
            }
            tried_m[tried_ct] = mask;
            masks_done = tried_ct + 1;
        }
        tried_v[tried_ct] = swap;
        tried_ct += 1;
        let new_cur = new_permute(cur, pt, swap, slice, BitVec(cand));
        if T::FAIL_FAST && new_cur < cutoff { return T::fail() }
        let new_val = new_recurse(
            new_cur,
            pt - 1,
            if T::FAIL_FAST { next_break } else { break_bits | (cand & !(cand >> 1usize)) },
            cutoff);
        if T::fail_fast_on(new_val) {
            return T::fail()
        } else {
            best = T::pick_best(new_val, best);
        }
    }
    T::val(best)
}

pub fn is_best(gr: &Graph) -> bool {
    // eprintln!("is_best({} {} {:b})", gr, gr.edges.0.0, gr.edges.0.0);
    new_recurse(gr.bits(), gr.size - 1, BITNUM_ZERO, gr.bits())
}

pub fn to_best(gr: &Graph) -> Graph {
    // eprintln!("is_best({} {} {:b})", gr, gr.edges.0.0, gr.edges.0.0);
    let mut last = gr.bits();
    // XXX unclear why I need multiple calls
    loop {
        let next: BitNum = new_recurse(last, gr.size - 1, BITNUM_ZERO, last);
        if next == last { return Graph::from_bits(gr.size, last) }
        last = next;
    }
}

fn recurse(
    fixed: &mut Fixed<impl FnMut(base::BitNum)>,
    Recursed { at, break_bits, so_far, recheck }: Recursed,
) {
    let offset = base::Graph::triangle(at);
    let so_far_ones = so_far.count_ones();
    let (row_lo, row_hi) = match fixed.prefix.get(fixed.size - 1 - at) {
        Some(&p) => (p, p + BITNUM_ONE),
        None => (BITNUM_ZERO, BITNUM_ONE << at),
    };
    let rows = std::iter::successors(Some(row_lo), |row| Some(*row + BITNUM_ONE))
        .take_while(|row| *row < row_hi);
    'outer: for row in rows {
        let mut recheck = recheck;
        // eprintln!("at={} break_bits={:b} so_far={:b} row={:b}", at, break_bits, so_far, row);
        let cur_ones = (so_far_ones + row.count_ones()) as usize;
        if cur_ones > fixed.filter.1 || cur_ones + offset < fixed.filter.0 { continue }
        if (get_breaks(row) & !break_bits) != 0 { continue }
        if so_far > 0 {
            let at_mask = !one_bits(at);
            let mut breaks = BITNUM_ZERO;
            for (alt, other) in fixed.line.iter().enumerate() {
                let alt = fixed.size - 1 - alt;
                let mask = one_bits(alt) & at_mask;
                if breaks & mask == 0 {
                    let upper = {
                        let mut upper = BitVec::new();
                        let gr1 = Triangle(BitVec(so_far));
                        for b in at + 1 .. alt {
                            if gr1.get((b, at)) {
                                upper.set(b)
                            }
                        }
                        if gr1.get((at, alt)) { upper.set(at) }
                        upper.0
                    };
                    let rerow = smoosh(upper | row, breaks);
                    /*
                    eprintln!("breaks={:b} mask={:b}, row={:b}, rerow={:b}, alt={} other={:b}",
                        breaks, mask, row, rerow, alt, other);
                    */
                    match rerow.cmp(other) {
                        // eprintln!("rerow < *other");
                        Less => continue 'outer,
                        Equal => recheck = true,
                        _ => { }
                    }
                }
                breaks |= *other & !(*other >> 1usize);
            }
        }
        let new_so_far = so_far | (row << offset);
        if at == 0 {
            if !recheck || is_best(&Graph { size: fixed.size, edges: Triangle(BitVec(new_so_far)) }) {
                (fixed.callback)(new_so_far);
            }
            continue;
        }
        fixed.line.push(row);
        recurse(
            fixed,
            Recursed {
                at: at - 1,
                break_bits: break_bits | (row & !(row >> 1usize)),
                so_far: new_so_far,
                recheck,
            },
        );
        fixed.line.pop();
    }
}

pub fn enumerate_graphs(size: usize, range: Option<(usize, usize)>, callback: impl FnMut(base::BitNum)) {
    enumerate_subtree(size, range, &[], callback)
}

pub fn enumerate_subtree(
    size: usize,
    range: Option<(usize, usize)>,
    prefix: &[BitNum],
    callback: impl FnMut(base::BitNum),
) {
    if size == 0 { return }
    assert!(size <= MAX_SIZE, "graph size {size} exceeds configured maximum {MAX_SIZE}");
    recurse(
        &mut Fixed {
            size,
            line: &mut Vec::with_capacity(size),
            callback,
            filter: range.unwrap_or((0, BitNum::BITS as usize)),
            prefix,
        },
        Recursed {
            at: size - 1,
            break_bits: BITNUM_ZERO,
            so_far: BITNUM_ZERO,
            recheck: false,
        },
    );
}

pub fn enumerate_middle(size: usize, mut callback: impl FnMut(base::BitNum)) {
    let half = Graph::triangle(size) / 2;
    enumerate_graphs(size, Some((half, half)), move |bn| {
        let grc = Graph::from_bits(size, bn).complement();
        let mut last = grc.bits();
        loop {
            if last < bn { break }
            let next: BitNum = new_recurse(last, size - 1, BITNUM_ZERO, last);
            if next == last { callback(bn); break }
            last = next;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools;
    use rand::Rng;

    #[test]
    fn test_smoosh() {
        assert_eq!(smoosh(BitNum::from(0b_110_01_0u8), BITNUM_ZERO), BitNum::from(0b_11_1u8));
        assert_eq!(smoosh(BitNum::from(0b_110_01_0u8), BitNum::from(0b1_0u8)), BitNum::from(0b_1_10_1u8));
        assert_eq!(smoosh(BitNum::from(0b_110_01_0u8), BitNum::from(0b11_0u8)), BitNum::from(0b_11_00_1u8));
        assert_eq!(smoosh(BitNum::from(0b_101_01_0u8), BitNum::from(0b1_01_0u8)), BitNum::from(0b_10_10_1u8));
    }

    /*
    #[test]
    fn test_enumerate_graphs() {
        let mut count = 0;
        enumerate_graphs(3, None, |_| count += 1);
        assert_eq!(count, 16);
    }
    */

    #[test]
    fn test_best_symmetric() {
        // twin-heavy families where the search used to blow up
        let rng = &mut rand::thread_rng();
        for size in 2..=9 {
            let mut grs = Vec::new();
            // stars with any center
            for c in 0..size {
                grs.push(Graph::from_fn(size, |a, b| a == c || b == c));
            }
            // complete bipartite splits
            for k in 1..size {
                grs.push(Graph::from_fn(size, |a, b| (a < k) != (b < k)));
            }
            // two cliques
            for k in 1..size {
                grs.push(Graph::from_fn(size, |a, b| (a < k) == (b < k)));
            }
            for gr in grs {
                // random relabeling
                let gr = gr.renumber(&crate::perm::Perm::random(rng, size));
                assert_eq!(to_best(&gr), tools::naive_find_best(&gr), "{}", gr);
                assert_eq!(is_best(&gr), gr == tools::naive_find_best(&gr));
            }
        }
    }

    #[test]
    fn test_best() {
        let rng = &mut rand::thread_rng();
        for _ in 0 .. 300 {
            let size = rng.gen_range(1..=9);
            let gr = crate::base::random_graph(rng, size);
            let gr_best = tools::naive_find_best(&gr);
            assert_eq!(is_best(&gr), gr == gr_best);
            assert!(is_best(&gr_best));
            assert_eq!(gr_best, to_best(&gr));
        }
    }
}
