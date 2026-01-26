use rand::Rng;
use std::fmt;
use itertools::Itertools;
use auto_ops::impl_op_ex;


#[derive(Debug,PartialEq,Eq,Clone,Hash,PartialOrd,Ord)]
pub struct Perm { vec: Vec<usize> }

type PermError = ();

impl Perm {
    pub fn new(vec: Vec<usize>) -> Option<Self> {
        Self::is_valid_slice(&vec).then_some(Perm { vec })
    }
    #[inline] pub fn new_unchecked(vec: Vec<usize>) -> Self { Perm { vec } }
    #[inline] pub fn vec(&self) -> &Vec<usize> { &self.vec }
    #[inline] pub fn into_vec(self) -> Vec<usize> { self.vec }
    #[inline] pub fn size(&self) -> usize { self.vec.len() }
    pub fn identity(size: usize) -> Self { Perm::new_unchecked((0..size).collect()) }
    #[inline] pub fn apply(&self, n: usize) -> usize { self.vec[n] }
    pub fn inverse(&self) -> Self {
        let mut v = vec![0; self.size()];
        for (i, n) in self.vec.iter().enumerate() {
            v[*n] = i;
        }
        Perm::new_unchecked(v)
    }
    pub fn random<R>(rng: &mut R, size: usize) -> Self where R: Rng + ?Sized {
        use super::testers::*;
        PermDistr(size).sample(rng)
    }
    fn is_valid_slice(vec: &[usize]) -> bool {
        let mut set = 0;
        for it in vec {
            let bit = 1 << it;
            if *it >= vec.len() || set & bit != 0 { return false }
            set |= bit;
        }
        true
    }
}

impl TryFrom<Vec<usize>> for Perm {
    type Error = PermError;
    fn try_from(vec: Vec<usize>) -> Result<Self, Self::Error> {
        Perm::new(vec).ok_or(())
    }
}

impl fmt::Display for Perm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[")?;
        for (i, &v) in self.vec.iter().enumerate() {
            if i > 0 { write!(f, " ")? }
            write!(f, "{}", v)?;
        }
        write!(f, "]")
    }
}

pub fn all_perms<'a>(size: usize) -> impl Iterator<Item=Perm> + 'a {
    (0..size).permutations(size).map(Perm::new_unchecked)
}

impl_op_ex!(* |a: &Perm, b: &Perm| -> Perm {
    assert_eq!(a.size(), b.size(), "Composed perms must have same size.");
    Perm::new_unchecked((0..a.size()).map(|i| a.apply(b.apply(i))).collect())
});

