//! A small, fast and deterministic hasher for integer keys (the "Fx" hash used inside rustc).
//!
//! The standard `HashMap` uses SipHash with a random seed: safe against hostile keys, but slow
//! for the millions of small integer keys of mesh editing, and its iteration order changes from
//! run to run. Mesh tables only ever hold our own vertex and edge numbers, so speed and a stable
//! order matter more here.
use std::hash::{BuildHasherDefault, Hasher};

/// `HashMap` with the Fx hasher. Create it with `HashMap::default()`.
pub type HashMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<FxHasher>>;
/// `HashSet` with the Fx hasher. Create it with `HashSet::default()`.
pub type HashSet<K> = std::collections::HashSet<K, BuildHasherDefault<FxHasher>>;

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

#[derive(Default, Clone, Copy)]
pub struct FxHasher {
    hash: u64,
}
impl FxHasher {
    #[inline]
    fn add(&mut self, w: u64) {
        self.hash = (self.hash.rotate_left(5) ^ w).wrapping_mul(SEED);
    }
}
impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut c = bytes.chunks_exact(8);
        for w in &mut c {
            self.add(u64::from_le_bytes(w.try_into().unwrap()));
        }
        let r = c.remainder();
        if !r.is_empty() {
            let mut b = [0u8; 8];
            b[..r.len()].copy_from_slice(r);
            self.add(u64::from_le_bytes(b));
        }
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64)
    }
    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add(i as u64)
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64)
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i)
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64)
    }
    #[inline]
    fn finish(&self) -> u64 {
        // spread the low bits (hashbrown picks buckets from them) over the whole word
        self.hash ^ (self.hash >> 29)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_work_and_iterate_in_the_same_order_every_time() {
        let make = || {
            let mut m: HashMap<(u32, u32), u32> = HashMap::default();
            for i in 0..5000u32 {
                m.insert((i % 97, i / 97), i);
            }
            m
        };
        let (a, b) = (make(), make());
        assert_eq!(a.len(), 5000);
        assert_eq!(a.get(&(3, 7)), Some(&(7 * 97 + 3)));
        let ka: Vec<_> = a.keys().copied().collect();
        let kb: Vec<_> = b.keys().copied().collect();
        assert_eq!(ka, kb, "iteration order is the same for the same insertions");
        let s: HashSet<u64> = (0..1000u64).map(|x| x * 1024).collect();
        assert_eq!(s.len(), 1000);
    }
}
