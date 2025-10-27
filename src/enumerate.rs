use crate::base;
use crate::base::{BitNum,BitVec,Graph,Triangle,Bits};
use crate::tools::one_bits;
use crate::perm::Perm;
use std::cmp::Ordering::*;

struct Fixed<'a, CB: Fn(base::BitNum)> {
    pub size: usize,
    pub line: &'a mut Vec<BitNum>,
    pub callback: CB,
    pub filter: (usize, usize),
}


struct Recursed {
    pub at: usize,
    pub break_bits: BitNum,
    pub so_far: BitNum,
    pub recheck: bool,
}

#[inline]
fn get_breaks(bits: BitNum) -> BitNum {
    !bits & (bits >> 1)
}

fn smoosh(row: BitNum, breaks: BitNum) -> BitNum {
    let mut row = row;
    let mut result = 0;
    let breaks = BitVec(breaks);
    // eprintln!("{} {}", row, breaks.0);
    loop {
        if row == 0 { return result }
        let start = row.ilog2() as usize;
        let mut find = 0;
        for i in 1.. {
            if i > start { break; }
            let j = start - i;
            if breaks.get(j) {
                find = j + 1;
                break;
            }
        }
        let mask = (1 << find) - 1;
        let cnt = (row & !mask).count_ones();
        // eprintln!("start={} find={} mask={}", start, find, mask);
        row &= mask;
        result |= ((1 << cnt) - 1) << find;
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
        | Graph::from_bits(pt + 1, cur & mask).renumber(&Perm::new_unsafe(perm)).edges.0.0;
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
    fn fail() -> Self { panic!("fail") }
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
    let next_break = break_bits | (basis & !(basis >> 1));
    /*
    if !new_recurse(cur, pt - 1, next_break, cutoff) {
        return false;
    }
    */
    let mut best: T::Score = T::score(cur);
    for swap in (0..=pt).rev() {
        // eprintln!("break_bits={:b} pt={} swap={}", break_bits, pt, swap);
        if pt != swap && BitVec(break_bits).get(swap) { break }
        let slice = if swap == pt {
            // this optimization barely helps
            BitVec((cur >> Graph::triangle(pt)) & one_bits(pt))
        } else {
            let mut slice = BitVec(0);
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
        let new_cur = new_permute(cur, pt, swap, slice, BitVec(cand));
        if T::FAIL_FAST && new_cur < cutoff { return T::fail() }
        let new_val = new_recurse(
            new_cur,
            pt - 1,
            if T::FAIL_FAST { next_break } else { break_bits | (cand & !(cand >> 1)) },
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
    new_recurse(gr.bits(), gr.size - 1, 0, gr.bits())
}

pub fn to_best(gr: &Graph) -> Graph {
    // eprintln!("is_best({} {} {:b})", gr, gr.edges.0.0, gr.edges.0.0);
    let mut last = gr.bits();
    // XXX unclear why I need multiple calls
    loop {
        let next: BitNum = new_recurse(last, gr.size - 1, 0, last);
        if next == last { return Graph::from_bits(gr.size, last) }
        last = next;
    }
}

fn recurse(
    fixed: &mut Fixed<impl Fn(base::BitNum)>,
    Recursed { at, break_bits, so_far, recheck }: Recursed,
) {
    let offset = base::Graph::triangle(at);
    let so_far_ones = so_far.count_ones();
    'outer: for row in 0 as BitNum .. 1 << at {
        let mut recheck = recheck;
        // eprintln!("at={} break_bits={:b} so_far={:b} row={:b}", at, break_bits, so_far, row);
        let cur_ones = (so_far_ones + row.count_ones()) as usize;
        if cur_ones > fixed.filter.1 || cur_ones + offset < fixed.filter.0 { continue }
        if (get_breaks(row) & !break_bits) != 0 { continue }
        if so_far > 0 {
            let at_mask = !one_bits(at);
            let mut breaks = 0;
            for (alt, other) in fixed.line.iter().enumerate() {
                let alt = fixed.size - 1 - alt;
                let mask = one_bits(alt) & at_mask;
                if breaks & mask == 0 {
                    let upper = {
                        let mut upper = BitVec(0);
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
                breaks |= *other & !(*other >> 1);
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
                break_bits: break_bits | (row & !(row >> 1)),
                so_far: new_so_far,
                recheck,
            },
        );
        fixed.line.pop();
    }
}

pub fn enumerate_graphs(size: usize, range: Option<(usize, usize)>, callback: impl Fn(base::BitNum)) {
    if size == 0 { return }
    recurse(
        &mut Fixed {
            size,
            line: &mut Vec::with_capacity(size),
            callback,
            filter: range.unwrap_or((0, BitNum::BITS as usize)),
        },
        Recursed {
            at: size - 1,
            break_bits: 0,
            so_far: 0,
            recheck: false,
        },
    );
}

pub fn enumerate_middle(size: usize, callback: fn(base::BitNum)) {
    let half = Graph::triangle(size) / 2;
    enumerate_graphs(size, Some((half, half)), move |bn| {
        let grc = Graph::from_bits(size, bn).complement();
        let mut last = grc.bits();
        loop {
            if last < bn { break }
            let next: BitNum = new_recurse(last, size - 1, 0, last);
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
        assert_eq!(smoosh(0b_110_01_0, 0), 0b_11_1);
        assert_eq!(smoosh(0b_110_01_0, 0b1_0), 0b_1_10_1);
        assert_eq!(smoosh(0b_110_01_0, 0b11_0), 0b_11_00_1);
        assert_eq!(smoosh(0b_101_01_0, 0b1_01_0), 0b_10_10_1);
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
    fn test_best() {
        let rng = &mut rand::thread_rng();
        for _ in 0 .. 1000 {
            let size = rng.gen_range(1..=9);
            let mut rand_bits = || rng.gen_range(0..(1 << Graph::triangle(size)));
            let gr = Graph::from_bits(size, rand_bits());
            let gr_best = tools::naive_find_best(&gr);
            assert_eq!(is_best(&gr), gr == gr_best);
            assert!(is_best(&gr_best));
            assert_eq!(gr_best, to_best(&gr));
        }
    }
}
