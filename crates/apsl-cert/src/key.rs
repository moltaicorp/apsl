use std::io::{Read, Write};
use std::path::Path;

use ed25519_dalek::{SigningKey, VerifyingKey, SECRET_KEY_LENGTH};
use rand_core::OsRng;
use sha2::{Digest, Sha256};

pub fn new_keypair() -> (SigningKey, VerifyingKey) {
    let sk = SigningKey::generate(&mut OsRng);
    let vk = sk.verifying_key();
    (sk, vk)
}

pub fn save_keypair(name: &str, sk: &SigningKey) -> std::io::Result<()> {
    let priv_path = format!("{}.priv", name);
    let pub_path = format!("{}.pub", name);
    let priv_hex = hex(sk.to_bytes().as_slice());
    let pub_hex = hex(sk.verifying_key().to_bytes().as_slice());
    std::fs::write(&priv_path, &priv_hex)?;
    std::fs::write(&pub_path, &pub_hex)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&priv_path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&priv_path, perms)?;
    }
    Ok(())
}

pub fn load_keypair(priv_path: &Path) -> std::io::Result<SigningKey> {
    let mut s = String::new();
    std::fs::File::open(priv_path)?.read_to_string(&mut s)?;
    let bytes = unhex(s.trim()).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "private-key file is not hex",
        )
    })?;
    if bytes.len() != SECRET_KEY_LENGTH {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "private-key wrong length",
        ));
    }
    let mut arr = [0u8; SECRET_KEY_LENGTH];
    arr.copy_from_slice(&bytes);
    Ok(SigningKey::from_bytes(&arr))
}

pub fn load_public(pub_path: &Path) -> std::io::Result<VerifyingKey> {
    let mut s = String::new();
    std::fs::File::open(pub_path)?.read_to_string(&mut s)?;
    let bytes = unhex(s.trim()).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "public-key file is not hex",
        )
    })?;
    if bytes.len() != 32 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "public-key wrong length",
        ));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    VerifyingKey::from_bytes(&arr).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid verifying key: {}", e),
        )
    })
}

pub fn fingerprint(vk: &VerifyingKey) -> String {
    let mut h = Sha256::new();
    h.update(vk.to_bytes());
    let d = h.finalize();
    let mut s = String::with_capacity(16);
    for b in d.iter().take(8) {
        use std::fmt::Write;
        let _ = write!(&mut s, "{:02x}", b);
    }
    s
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        use std::fmt::Write;
        let _ = write!(&mut s, "{:02x}", x);
    }
    s
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    for i in (0..s.len()).step_by(2) {
        let hi = digit(bytes[i])?;
        let lo = digit(bytes[i + 1])?;
        out.push((hi << 4) | lo);
    }
    Some(out)
}

fn digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub use std::fs::File;

#[allow(unused)]
fn _unused_imports(_w: &mut dyn Write) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keypair_roundtrip() {
        let dir = tempdir();
        let path = dir.join("k");
        let (sk, vk) = new_keypair();
        save_keypair(path.to_str().unwrap(), &sk).unwrap();
        let loaded = load_keypair(&dir.join("k.priv")).unwrap();
        assert_eq!(loaded.verifying_key().to_bytes(), vk.to_bytes());
    }

    #[test]
    fn public_key_verification() {
        let dir = tempdir();
        let path = dir.join("k");
        let (sk, vk) = new_keypair();
        save_keypair(path.to_str().unwrap(), &sk).unwrap();
        let loaded_pub = load_public(&dir.join("k.pub")).unwrap();
        assert_eq!(loaded_pub.to_bytes(), vk.to_bytes());
    }

    fn tempdir() -> std::path::PathBuf {
        let base = std::env::temp_dir();
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let dir = base.join(format!("apsl-cert-test-{}-{}", pid, nanos));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
