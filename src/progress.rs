use std::cell::Cell;
use std::time::{Instant,Duration};

pub struct Progress {
    next: Cell<Instant>,
}

impl Progress {
    pub fn new() -> Self {
        Self { next: Instant::now().into() }
    }
    #[inline]
    pub fn tick(&self, str: impl Fn() -> String) {
        let now = Instant::now();
        if now > self.next.get() {
            eprint!(" Progress: {}\r", str());
            self.next.set(now + Duration::from_secs(1));
        }
    }
}

