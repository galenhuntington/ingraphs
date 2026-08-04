pub mod perm;
pub mod base;
pub mod testers;
pub mod tools;
pub mod enumerate;
pub mod seek;
pub mod progress;

use base::{Graph, BitNum,Bits};
use std::collections::BTreeSet;
use clap::{Parser,Subcommand};
use std::time::{Instant,Duration};

pub fn enumerate(size: usize, range: Option<(usize, usize)>, prefix: &[BitNum]) {
    use std::io::Write;
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    enumerate::enumerate_subtree(size, range, prefix, |bits| { writeln!(out, "{}", bits).unwrap() });
}

pub fn enumerate_middle(size: usize) {
    use std::io::Write;
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    enumerate::enumerate_middle(size, |bits| { writeln!(out, "{}", bits).unwrap() });
}

// Fixed filename for all graphs used for a few operations
pub fn read<B: Bits>(size: usize) -> Vec<B> {
    let x: Vec<B> = tools::read_graphs(size, &format!("output/all{}.txt", size)).collect();
    eprintln!("Read graphs: {:?}", x.len());
    x
}

// Dummy function for experimentation
pub fn run_graphs(_size: usize) {
    // println!("{:?}", seek::seek(&Graph::from_bits(5, 30)));
    // println!("{:?}", seek::seek(&Graph::from_bits(9, 101752)));
    println!("{:?}", seek::seek_full(&Graph::from_bits(10, 2202040)));
    // println!("{:?}", seek::seek(&Graph::from_bits(10, 2167546)));
}

fn stats(path: String) {
    let size = 16; // upper bound; largest we support
    let all = tools::read_graphs::<BitNum>(size, &path);
    let mut counts = vec![0; Graph::triangle(size) + 1];
    for bn in all {
        counts[bn.count_ones() as usize] += 1;
    }
    for (i, c) in counts.iter().enumerate() {
        if *c > 0 {
            println!("{:3} {}", i, c);
        }
    }
}

fn ingraph_scan(size: usize, pool: impl Iterator<Item=Graph>) {
    let all = read::<BitNum>(size);
    // let mut count = 0;
    let progress = progress::Progress::new();
        // |(i, celen, val)| format!("{} {} ({})", i, celen, val));
    let mut counterexamples = BTreeSet::new();
    for (i, gr) in pool.enumerate() {
        let val = gr.bits();
        let ec = val.count_ones();
        progress.tick(|| format!("{} {} ({})", i, counterexamples.len(), val));
        let chkce = tools::noncovers(counterexamples.iter().cloned(), &gr).next();
        let counter =
            chkce.or_else(|| {
                let seek = tools::noncovers(all.iter().cloned(), &gr).next();
                if let Some(c) = seek { counterexamples.insert(c); }
                seek
            });
        // if counter.is_some() { continue }
        println!("{:?},{},{:?},{:?},{}",
            val,
            gr,
            // tools::count_symmetries(&gr),
            ec,
            counter,
            tools::timestamp(),
        );
    }
}

fn ingraph_seek(pool: impl Iterator<Item=Graph>, bailout: usize, seeds: &[BitNum]) {
    let progress = progress::Progress::new();
    let no_time = Duration::from_secs(0);
    let mut counterexamples: BTreeSet<_> = seeds.iter().map(|&ce| (no_time, ce, 0)).collect();
    for (i, gr) in pool.enumerate() {
        let val = gr.bits();
        let ec = val.count_ones();
        progress.tick(|| format!("{} {} ({})", i, counterexamples.len(), val));
        // A lot of hacky stuff here was trying to find "better" counterexamples.
        // let chkce = tools::noncovers(counterexamples.iter().map(|(_, x)| *x), &gr).next();
        let chkce = {
            let sub_sorted = tools::build_sorted_row(&gr);
            let mut ans = None;
            let vec: Vec<_> = counterexamples.iter().cloned().collect();
            for el@(d1, supb, j) in vec {
                let sup = Graph::from_bits(gr.size, supb);
                let t0 = Instant::now();
                let chk = tools::ingraph_check(&sup, &sub_sorted, &gr);
                let d = t0.elapsed();
                counterexamples.remove(&el);
                if chk {
                    if i < j + 10_000 {
                        counterexamples.insert((d.max(d1), supb, j));
                    }
                } else {
                    counterexamples.insert((d, supb, i));
                    ans = Some(supb);
                    break
                }
            }
            ans
        };
        let counter =
            chkce.map_or_else(
                || {
                    let seek = crate::seek::seek(&gr, bailout);
                    if let Some(gr1) = seek.0 {
                        counterexamples.insert((no_time, gr1.bits(), i));
                        // counterexamples.insert((ce_score(&gr1), gr1.bits()));
                    }
                    (seek.0.map(|g| g.bits()), seek.1)
                },
                |x| (Some(x), 0),
            );
        // if counter.is_some() { continue }
        println!("{:?},{},{:?},{:?},{}",
            val,
            gr,
            // tools::count_symmetries(&gr),
            ec,
            counter,
            tools::timestamp(),
        );
    }
}

fn ingraph_check(sub: &Graph, list: impl Iterator<Item=Graph> + Send) -> Option<Graph> {
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    let sub_sorted = tools::build_sorted_row(sub);
    let done = AtomicU64::new(0);
    list.par_bridge().find_map_any(|sup| {
        let d = done.fetch_add(1, Ordering::Relaxed);
        if d % 1_000_000 == 0 {
            let dm = d / 1_000_000;
            eprint!(" Progress: {}M ({})\r", dm, tools::timestamp());
            if dm % 50 == 0 { eprintln!(); }
        }
        (!tools::ingraph_check(&sup, &sub_sorted, sub)).then_some(sup)
    })
}

// Test each candidate ingraph against a shared library of counterexamples;
// a library graph H refutes candidate G if G embeds in neither H nor its
// complement.  Prints candidate,refuter (or None for survivors).
fn cull(size: usize, candidates: Vec<BitNum>, library: &[BitNum]) {
    use rayon::prelude::*;
    let library: Vec<Graph> = library.iter()
        .map(|&h| Graph::from_bits(size, h)).collect();
    let progress = std::sync::atomic::AtomicU64::new(0);
    let survived = std::sync::atomic::AtomicU64::new(0);
    let results: Vec<_> = candidates.par_iter().map(|&g| {
        let gr = Graph::from_bits(size, g);
        let sorted = tools::build_sorted_row(&gr);
        let hit = library.iter()
            .find(|h| !tools::ingraph_check(h, &sorted, &gr))
            .map(|h| h.bits());
        let d = progress.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if hit.is_none() { survived.fetch_add(1, std::sync::atomic::Ordering::Relaxed); }
        if d % 1000 == 0 { eprint!(" Progress: {}k\r", d / 1000); }
        (g, hit)
    }).collect();
    for (g, hit) in results {
        println!("{},{:?}", g, hit);
    }
    eprintln!("\nCandidates: {}, survivors: {}",
        candidates.len(), survived.load(std::sync::atomic::Ordering::Relaxed));
}

fn free_scan(sub: &Graph, list: impl Iterator<Item=Graph> + Send) {
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    let sub_sorted = tools::build_sorted_row(sub);
    let done = AtomicU64::new(0);
    let found = AtomicU64::new(0);
    list.par_bridge().for_each(|sup| {
        let d = done.fetch_add(1, Ordering::Relaxed);
        if d % 1_000_000 == 0 {
            let dm = d / 1_000_000;
            eprint!(" Progress: {}M ({})\r", dm, tools::timestamp());
            if dm % 50 == 0 { eprintln!(); }
        }
        if !tools::isso_inner::<bool>(sub, &sub_sorted, &sup) {
            found.fetch_add(1, Ordering::Relaxed);
            println!("{},{},{}", sup.bits(), sup, tools::timestamp());
        }
    });
    eprintln!("\nChecked {}, free: {}",
        done.load(Ordering::Relaxed), found.load(Ordering::Relaxed));
}

// Upward closure of sub-free seed graphs: repeatedly add edges keeping them
// sub-free, and for each report whether the complement covers sub.  A "false"
// in the third column is a counterexample to sub being a universal ingraph.
fn free_close(sub: &Graph, seeds: impl Iterator<Item=Graph>) {
    let sub_sorted = tools::build_sorted_row(sub);
    let mut seen = BTreeSet::new();
    let mut level = Vec::new();
    for g in seeds {
        let g = enumerate::to_best(&g);
        if seen.insert(g.bits()) { level.push(g) }
    }
    let mut count = 0;
    let mut bad = 0;
    while !level.is_empty() {
        let mut next = Vec::new();
        for g in &level {
            count += 1;
            let covered = tools::is_subgraph_of(sub, &g.complement());
            if !covered { bad += 1 }
            println!("{},{},{},{}", g.bits(), g.edge_count(), covered, g);
            for ext in tools::bump(g, true) {
                if !seen.contains(&ext) {
                    let eg = Graph::from_bits(g.size, ext);
                    if !tools::isso_inner::<bool>(sub, &sub_sorted, &eg) {
                        seen.insert(ext);
                        next.push(eg);
                    }
                }
            }
        }
        level = next;
    }
    eprintln!("Closure size: {}, counterexamples: {}", count, bad);
}

// Counts: modulo complements and symmetries, modulo symmetries, "labelled"
fn miss_counts(gr: &Graph) -> (usize, usize, usize) {
    let all = read::<BitNum>(gr.size);
    let half = match Graph::triangle(gr.size) {
        x if x % 2 == 0 => Some(x / 2),
        _ => None,
    };
    let fac = tools::factorial(gr.size);
    let mut counts = (0, 0, 0);
    for gr1 in tools::noncovers(all.iter().cloned(), gr) {
        let gr1 = Graph::from_bits(gr.size, gr1);
        let syms = fac / tools::count_symmetries(&gr1);
        // eprintln!("{} {}", gr1, syms);
        if matches!(half, Some(x) if x == gr1.edge_count()) {
            counts.0 +=
                if tools::is_subgraph_of(&gr1, &gr1.complement()) { 2 } else { 1 };
            counts.1 += 1;
            counts.2 += syms;
        } else {
            counts.0 += 2;
            counts.1 += 1;
            counts.2 += 2 * syms;
        }
    }
    counts.0 /= 2;
    counts
}

fn successors(size: usize, pool: impl Iterator<Item=BitNum>, max: BitNum) {
    let pool: BTreeSet<_> = pool.collect();
    let extends: BTreeSet<_> = pool.iter().flat_map(|g| tools::bump(
            &Graph::from_bits(size, *g), true)).collect();
    for g in extends {
        let rets = tools::bump(&Graph::from_bits(size, g), false);
        let mut highs = Vec::new();
        let mut bads = Vec::new();
        for g1 in rets.iter() {
            if *g1 > max {
                highs.push(*g1);
                continue;
            }
            if !pool.contains(g1) {
                bads.push(g1);
            }
        }
        if !bads.is_empty() { continue }
        print!("{},{},{:?},", g.bits(), Graph::from_bits(size, g), bads);
        show_iter(highs.iter().cloned());
    }
}


#[derive(Debug,Subcommand)]
enum C {
    /// Enumerate all graphs
    #[command(arg_required_else_help = true)]
    Enumerate {
        /// Number of vertices
        size: usize,
    },
    /// Enumerate all graphs in a range of edge counts
    EnumerateFilter {
        /// Number of vertices
        size: usize,
        /// Minimum number of edges
        min: usize,
        /// Maximum number of edges
        max: usize,
        /// Pin the rows of the top vertices, enumerating one subtree
        #[arg(long, value_delimiter = ',')]
        prefix: Vec<BitNum>,
    },
    /// Enumerate all graphs with half the possible edges modulo complements
    EnumerateMiddle {
        /// Number of vertices
        size: usize,
    },
    /// List all graphs with one fewer edge
    Retract {
        /// Graph to retract
        bits: BitNum,
    },
    /// List all graphs with one more edge
    Extend {
        /// Number of vertices
        size: usize,
        /// Graph to extend
        bits: BitNum,
    },
    /// Stats on number of graphs per edge count in a file
    // Can check against https://oeis.org/A008406/b008406.txt
    Stats {
        /// Graphs file
        path: String,
    },
    /// Number of counterexamples for a non-universal ingraph
    Misses {
        /// Number of vertices
        size: usize,
        /// Key graphs
        bits: Vec<BitNum>,
    },
    /// Scan a file of graphs for ingraphs
    // Results include counterexamples; grep None for ingraphs
    IngraphScan {
        /// Number of vertices
        size: usize,
        /// Graphs file
        path: String,
    },
    /// Try to construct counterexamples for each ingraph in a file
    IngraphSeek {
        /// Number of vertices
        size: usize,
        /// Graphs file
        path: String,
        /// Bail out after this many checks
        #[arg(long)]
        bailout: Option<usize>,
        /// Seed the counterexample pool
        #[arg(long, value_delimiter = ',')]
        seeds: Vec<BitNum>,
    },
    /// Check if a single graph is an ingraph
    IngraphCheck {
        /// Number of vertices
        size: usize,
        /// Graph
        bits: BitNum,
        /// Graphs file
        path: String,
    },
    /// Filter a file of graphs to those within an edge count range
    Filter {
        /// Minimum number of edges
        min: usize,
        /// Maximum number of edges
        max: usize,
        /// Graphs file
        path: String,
    },
    /// Dump basic info on a graph
    Info {
        /// Number of vertices
        size: usize,
        /// Graph
        bits: BitNum,
    },
    /// One-edge extensions whose retracts are all in a file
    Successors {
        /// Number of vertices
        size: usize,
        /// Graphs file
        path: String,
        /// Maximum scanned
        #[arg(long)]
        max: Option<BitNum>,
    },
    /// Show whether a list of graphs are subgraphs of another list
    IsSubgraph {
        /// Force showing as table
        #[arg(long)]
        table: bool,
        #[arg(long)]
        color: bool,
        #[arg(help = "/ [GRAPHS] ...")]
        graphs: Vec<String>,
    },
    /// Complement of a graph
    Complement {
        /// Number of vertices
        size: usize,
        /// Graph
        bits: BitNum,
    },
    /// Scan a file of graphs for those NOT containing a given subgraph
    FreeScan {
        /// Number of vertices
        size: usize,
        /// Subgraph to seek
        bits: BitNum,
        /// Graphs file
        path: String,
    },
    /// Refute candidate ingraphs en masse against a counterexample library
    Cull {
        /// Number of vertices
        size: usize,
        /// Candidates file
        path: String,
        /// Counterexamples file
        library: String,
    },
    /// Grow subgraph-free seed graphs upward, reporting complement coverage
    FreeClose {
        /// Number of vertices
        size: usize,
        /// Subgraph to stay free of
        bits: BitNum,
        /// Seed graphs file
        path: String,
    },
    /// Canonicalize each graph in a file (decimal or graph6), print numbers
    Canon {
        /// Number of vertices
        size: usize,
        /// Graphs file
        path: String,
    },
    /// Placeholder for custom operations
    Run {
        /// Number of vertices
        size: usize,
    },
}

#[derive(Debug,Parser)]
struct Cli {
    #[command(subcommand)]
    command: C,
}

fn show_iter(it: impl Iterator<Item=base::BitNum>) {
    for (i, x) in it.enumerate() {
        print!("{}{}", if i == 0 { "" } else { " " }, x);
    }
    println!();
}

fn parse_subgraph_args(strs: Vec<String>) -> Option<(Vec<BitNum>, Vec<BitNum>)> {
    let mut subs = Vec::new();
    let mut sups = Vec::new();
    let mut target = &mut subs;
    for s in strs {
        if s == "/" {
            target = &mut sups;
            continue;
        }
        target.push(s.parse().ok()?);
    }
    if subs.is_empty() || sups.is_empty() {
        None
    } else {
        Some((subs, sups))
    }
}

pub fn main() {
    let args = Cli::parse();
    match args.command {
        C::Enumerate { size } => {
            enumerate(size, None, &[]);
        }
        C::EnumerateFilter { size, min, max, prefix } => {
            enumerate(size, Some((min, max)), &prefix);
        }
        C::EnumerateMiddle { size } => {
            let tri = Graph::triangle(size);
            let half = tri / 2;
            if tri.is_multiple_of(2) {
                enumerate_middle(size);
            } else {
                enumerate(size, Some((half, half)), &[]);
            }
        }
        C::Run { size } => {
            run_graphs(size);
        }
        C::Retract { bits } => {
            let gr = tools::infer_graph(bits);
            let seen = tools::bump(&gr, false);
            show_iter(seen.into_iter());
        }
        C::Extend { size, bits } => {
            let gr = Graph::from_bits(size, bits);
            let seen = tools::bump(&gr, true);
            show_iter(seen.into_iter());
        }
        C::Stats { path } => {
            stats(path);
        }
        C::Misses { size, bits } => {
            for bits in bits {
                let gr = Graph::from_bits(size, bits);
                let counts = miss_counts(&gr);
                println!("{},{},{},{},{}", gr.bits(), counts.0, counts.1, counts.2, gr);
            }
        }
        C::IngraphScan { size, path } => {
            let pool = tools::read_graphs(size, &path);
            ingraph_scan(size, pool);
        }
        C::IngraphSeek { size, path, bailout, seeds } => {
            eprintln!("Threads: {}", rayon::current_num_threads());
            let pool = tools::read_graphs(size, &path);
            ingraph_seek(pool, bailout.unwrap_or(usize::MAX), &seeds);
        }
        C::IngraphCheck { size, bits, path } => {
            eprintln!("Threads: {}", rayon::current_num_threads());
            let gr = Graph::from_bits(size, bits);
            let ans = ingraph_check(&gr, tools::read_graphs(size, &path));
            println!("{:?} {} {:?} {:?}",
                bits,
                gr,
                ans.map(|x| x.bits()),
                ans.map(|x| format!("{}", x))
            );
        }
        C::Filter { min, max, path } => {
            for g in tools::read_graphs::<BitNum>(max, &path) {
                let ct = g.count_ones() as usize;
                if ct >= min && ct <= max {
                     println!("{},{}", g, g.count_ones())
                }
            }
        }
        C::Info { size, bits } => {
            let gr = Graph::from_bits(size, bits);
            println!("{} {} ({}) {} syms:{} degree_row:{:?}",
                bits, gr, bits.count_ones(), bits.show_bits(), tools::count_symmetries(&gr),
                tools::build_sorted_row(&gr));
        }
        C::Successors { size, path, max } => {
            let pool = tools::read_graphs(size, &path);
            successors(size, pool, max.unwrap_or(BitNum::MAX));
        }
        C::IsSubgraph { table, color, graphs } => {
            let (subs, sups) = parse_subgraph_args(graphs).unwrap_or_else(||
                clap::Error::new(clap::error::ErrorKind::InvalidValue).exit()
            );
            let table = table || (subs.len() > 1 && sups.len() > 1);
            let width = if table {
                subs.iter().chain(sups.iter())
                    .map(|x| format!("{}", x).len()).max().unwrap_or(0).max(5)
                } else {
                    0
                };
            if table {
                print!(" {:width$}", "", width=width);
                for sup in sups.iter() {
                    print!(" {:width$}", sup, width=width);
                }
                println!();
            }
            for sub in subs {
                if table { print!(" {:width$}", sub, width=width) }
                for sup in sups.iter() {
                    let size = tools::infer_size(*sup);
                    let sub = Graph::from_bits(size, sub);
                    let sup = Graph::from_bits(size, *sup);
                    let ans = tools::is_subgraph_of(&sub, &sup);
                    print!(" {}{:>width$}{}",
                        if color { if ans { "\x1b[01;32m" } else { "\x1b[01;31m" } } else { "" },
                        tools::is_subgraph_of(&sub, &sup),
                        if color { "\x1b[00m" } else { "" },
                        );
                }
                if table { println!() }
            }
            if !table { println!() }
        }
        C::Complement { size, bits } => {
            let gr = enumerate::to_best(&Graph::from_bits(size, bits).complement());
            println!("{}", gr.bits());
        }
        C::FreeScan { size, bits, path } => {
            eprintln!("Threads: {}", rayon::current_num_threads());
            let gr = Graph::from_bits(size, bits);
            free_scan(&gr, tools::read_graphs(size, &path));
        }
        C::Cull { size, path, library } => {
            eprintln!("Threads: {}", rayon::current_num_threads());
            let cands: Vec<BitNum> = tools::read_graphs(size, &path).collect();
            let lib: Vec<BitNum> = tools::read_graphs(size, &library).collect();
            eprintln!("Candidates: {}, library: {}", cands.len(), lib.len());
            cull(size, cands, &lib);
        }
        C::FreeClose { size, bits, path } => {
            let gr = Graph::from_bits(size, bits);
            free_close(&gr, tools::read_graphs(size, &path));
        }
        C::Canon { size, path } => {
            use std::io::Write;
            let mut out = std::io::BufWriter::new(std::io::stdout().lock());
            for g in tools::read_graphs::<Graph>(size, &path) {
                writeln!(out, "{}", enumerate::to_best(&g).bits()).unwrap();
            }
        }
    }
}

