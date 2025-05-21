pub use rand::Rng;
pub use rand_distr::Distribution;
use crate::perm::Perm;

pub struct PermDistr (pub usize);
impl Distribution<Perm> for PermDistr {
    fn sample<R>(&self, rng: &mut R) -> Perm where R: Rng + ?Sized {
        use rand::prelude::SliceRandom;
        let mut vec: Vec<usize> = (0..self.0).collect();
        vec.as_mut_slice().shuffle(rng);
        Perm::new_unsafe(vec)
    }
}
