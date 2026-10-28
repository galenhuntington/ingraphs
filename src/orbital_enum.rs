//! Bounded affine-cube worker for scripts/orbital_enum.py. No SAT dependency.
//! The input is a base graph followed by disjoint nonzero XOR masks. Enumerate
//! EVERY assignment, returning exact isomorphism/complement representatives.

use crate::base::{BitNum, Bits, Graph, MAX_SIZE};
use crate::canon;
use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

pub const PROTOCOL: &str = "orbital-enum-v1";

#[derive(Debug, clap::Args)]
pub struct Args {
    size: usize,
    /// Decimal base graph, then one disjoint nonzero XOR mask per line
    cube: PathBuf,
}

fn validate(size: usize, base: BitNum, masks: &[BitNum]) -> Result<(), String> {
    if !(1..=MAX_SIZE).contains(&size) {
        return Err(format!("order must be in 1..={MAX_SIZE}"));
    }
    if masks.len() > 20 {
        return Err("a worker cube may have at most 20 variables; split it first".into());
    }
    let full = ((1 as BitNum) << Graph::triangle(size)) - 1;
    if base & !full != 0 {
        return Err("base graph has out-of-range edges".into());
    }
    let mut covered = 0;
    for &mask in masks {
        if mask == 0 || mask & !full != 0 || mask & covered != 0 {
            return Err("XOR masks must be nonzero, disjoint, and fit the order".into());
        }
        covered |= mask;
    }
    Ok(())
}

/// Edge count first, then the native key. Only balanced hosts need two nauty
/// calls. These are internal labels, not `canon --complement`'s numeric minimum.
fn representative(size: usize, bits: BitNum) -> BitNum {
    let graph = Graph::from_bits(size, bits);
    let edges = graph.edge_count() * 2;
    let total = Graph::triangle(size);
    if edges < total {
        canon::key(&graph).graph(size).bits()
    } else if edges > total {
        canon::key(&graph.complement()).graph(size).bits()
    } else {
        canon::key(&graph)
            .min(canon::key(&graph.complement()))
            .graph(size)
            .bits()
    }
}

fn enumerate(size: usize, base: BitNum, masks: &[BitNum]) -> Result<Vec<BitNum>, String> {
    validate(size, base, masks)?;
    let mut hosts = HashSet::new();
    let mut host = base;
    hosts.insert(representative(size, host));
    // Binary reflected Gray code visits each assignment exactly once.
    for step in 1usize..(1 << masks.len()) {
        host ^= masks[step.trailing_zeros() as usize];
        hosts.insert(representative(size, host));
    }
    let mut hosts: Vec<_> = hosts.into_iter().collect();
    hosts.sort_unstable();
    Ok(hosts)
}

pub fn run(args: Args) -> Result<(), String> {
    let text = std::fs::read_to_string(&args.cube).map_err(|e| e.to_string())?;
    let values = text
        .lines()
        .filter(|s| !s.trim().is_empty() && !s.trim_start().starts_with('#'))
        .map(|s| s.trim().parse::<BitNum>().map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let (&base, masks) = values.split_first().ok_or("cube needs a base graph")?;
    let start = Instant::now();
    let hosts = enumerate(args.size, base, masks)?;
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    // Explicit completion metadata lets the launcher reject partial/truncated
    // output. No batch is committed until the child exits successfully.
    writeln!(
        out,
        "{PROTOCOL},{},{},{},{:.6}",
        args.size,
        1usize << masks.len(),
        hosts.len(),
        start.elapsed().as_secs_f64()
    )
    .map_err(|e| e.to_string())?;
    for host in hosts {
        writeln!(out, "{host}").map_err(|e| e.to_string())?;
    }
    out.flush().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_four_vertex_graphs_and_complement_normalization() {
        let masks: Vec<_> = (0..6).map(|e| 1 << e).collect();
        let hosts = enumerate(4, 0, &masks).unwrap();
        assert_eq!(hosts.len(), 6);
        for h in 0..64 {
            assert_eq!(representative(4, h), representative(4, h ^ 63));
            assert!(hosts.contains(&representative(4, h)));
        }
    }

    #[test]
    fn affine_base_and_empty_cube() {
        let hosts = enumerate(4, 21, &[3, 12, 48]).unwrap();
        let expected: HashSet<_> = (0..8)
            .map(|a| {
                representative(
                    4,
                    21 ^ (if a & 1 != 0 { 3 } else { 0 })
                        ^ (if a & 2 != 0 { 12 } else { 0 })
                        ^ (if a & 4 != 0 { 48 } else { 0 }),
                )
            })
            .collect();
        assert_eq!(hosts.into_iter().collect::<HashSet<_>>(), expected);
        assert_eq!(enumerate(4, 21, &[]).unwrap(), vec![representative(4, 21)]);
    }

    #[test]
    fn rejects_bad_cubes() {
        for masks in [vec![0], vec![3, 2], vec![64]] {
            assert!(enumerate(4, 0, &masks).is_err());
        }
        assert!(enumerate(4, 64, &[]).is_err());
        assert!(enumerate(0, 0, &[]).is_err());
        assert!(enumerate(MAX_SIZE + 1, 0, &[]).is_err());
        assert!(validate(4, 0, &[1; 21]).is_err());
    }
}
