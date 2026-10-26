/// Fixed-size bitset over module indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitSet {
    words: Vec<u64>,
    len: usize,
}

impl BitSet {
    pub fn new(len: usize) -> Self {
        Self {
            words: vec![0; len.div_ceil(64)],
            len,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn contains(&self, i: usize) -> bool {
        i < self.len && self.words[i / 64] & (1 << (i % 64)) != 0
    }

    /// Returns `true` if the bit was newly set.
    pub fn insert(&mut self, i: usize) -> bool {
        assert!(i < self.len, "bit {i} out of range {}", self.len);
        let w = &mut self.words[i / 64];
        let mask = 1 << (i % 64);
        let new = *w & mask == 0;
        *w |= mask;
        new
    }

    pub fn count(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.len).filter(|&i| self.contains(i))
    }
}

#[cfg(test)]
mod tests {
    use super::BitSet;

    #[test]
    fn basics() {
        let mut b = BitSet::new(130);
        assert!(b.insert(0));
        assert!(b.insert(129));
        assert!(!b.insert(129));
        assert!(b.contains(0) && b.contains(129) && !b.contains(64) && !b.contains(500));
        assert_eq!(b.count(), 2);
        assert_eq!(b.iter().collect::<Vec<_>>(), vec![0, 129]);
    }
}
