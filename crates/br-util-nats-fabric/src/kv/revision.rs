#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Revision(u64);

impl Revision {
    pub(crate) fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) fn get(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_and_exposes_the_sequence() {
        assert_eq!(Revision::new(7).get(), 7);
    }
}
