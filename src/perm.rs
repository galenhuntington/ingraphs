use rand::Rng;
use itertools::Itertools;
use std::collections::BTreeSet;
use auto_ops::impl_op_ex;


#[derive(Debug,PartialEq,Eq,Clone,Hash,PartialOrd,Ord)]
pub struct Perm { pub vec: Vec<usize> }

impl Perm {
    pub fn new(vec: Vec<usize>) -> Self {
        let p = Perm { vec };
        assert!(p.is_valid(), "Invalid permutation: {:?}", p);
        p
    }
    pub fn new_unsafe(vec: Vec<usize>) -> Self { Perm { vec } }
    pub fn size(&self) -> usize { self.vec.len() }
    pub fn identity(size: usize) -> Self { Perm::new_unsafe((0..size).collect()) }
    // pub fn from_fn<F>(size: usize, f: impl Fn(usize) -> usize) -> Self {
    pub fn from_fn(size: usize, f: impl Fn(usize) -> usize) -> Self {
        (0..size).map(f).collect()
    }
    pub fn apply(&self, n: usize) -> usize { self.vec[n] }
    pub fn inverse(&self) -> Self {
        let mut v = vec![0; self.vec.len()];
        for (i, n) in self.vec.iter().enumerate() {
            v[*n] = i;
        }
        Perm::new_unsafe(v)
    }
    pub fn random<R>(rng: &mut R, size: usize) -> Self where R: Rng + ?Sized {
        use super::testers::*;
        PermDistr(size).sample(rng)
    }
    pub fn is_valid(self: &Self) -> bool {
        let mut set = BTreeSet::new();
        for it in &self.vec {
            if *it >= self.size() || set.contains(it) { return false }
            set.insert(it);
        }
        true
    }
}

impl<const N: usize> From<[usize; N]> for Perm {
    fn from(arr: [usize; N]) -> Perm { Perm::new(Vec::from(arr)) }
}

impl From<Vec<usize>> for Perm {
    fn from(vec: Vec<usize>) -> Perm { Perm::new(vec) }
}

impl FromIterator<usize> for Perm {
    fn from_iter<I: IntoIterator<Item=usize>>(iter: I) -> Self {
        Perm::new(Vec::from_iter(iter))
    }
}

pub fn all_perms<'a>(size: usize) -> impl Iterator<Item=Perm> + 'a {
    (0..size).permutations(size).map(|v| Perm::new_unsafe(v))
}

impl_op_ex!(* |a: &Perm, b: &Perm| -> Perm {
    assert_eq!(a.size(), b.size(), "Composed perms must have same size.");
    Perm::new_unsafe((0..a.size()).map(|i| a.vec[b.vec[i]]).collect())
});

