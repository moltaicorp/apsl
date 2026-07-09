
use sha2::{Digest, Sha256};
use crate::canon::Canon;

pub fn sha256_hex(canon_bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(canon_bytes);
    let digest = h.finalize();
    let mut s = String::with_capacity(64);
    for b in digest.iter() {
        use std::fmt::Write;
        let _ = write!(&mut s, "{:02x}", b);
    }
    s
}

pub trait ContentHash: Canon {
    fn content_hash(&self) -> String {
        let bytes = self.canon();
        sha256_hex(bytes.as_bytes())
    }
}

impl<T: Canon> ContentHash for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;

    #[test]
    fn empty_program_hash_stable() {
        let p = Program::new();
        let h1 = p.content_hash();
        let h2 = p.content_hash();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }
}
