use std::io::Write;
use std::path::{Path, PathBuf};

use apsl_core::canon::Canon;
use apsl_core::hash::sha256_hex;

use crate::cert::Certificate;

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    HashMismatch,
    NotFound(String),
    Malformed,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "io: {}", e),
            StoreError::HashMismatch => write!(f, "cert content did not match its hash"),
            StoreError::NotFound(h) => write!(f, "no cert with hash {}", h),
            StoreError::Malformed => write!(f, "cert is malformed"),
        }
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

fn cert_path(base: &Path, hash: &str) -> PathBuf {
    let aa = &hash[..2];
    let bb = &hash[2..4];
    let rest = &hash[4..];
    base.join(aa).join(bb).join(format!("{}.cert", rest))
}

pub fn put(c: &Certificate, base: &Path) -> Result<String, StoreError> {
    let bytes = c.canon();
    let hash = sha256_hex(bytes.as_bytes());
    let final_path = cert_path(base, &hash);
    if let Some(parent) = final_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = final_path.with_extension("cert.tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &final_path)?;
    Ok(hash)
}

pub fn get_bytes(hash: &str, base: &Path) -> Result<String, StoreError> {
    let path = cert_path(base, hash);
    if !path.exists() {
        return Err(StoreError::NotFound(hash.to_string()));
    }
    let bytes = std::fs::read_to_string(&path)?;
    let actual = sha256_hex(bytes.as_bytes());
    if actual != hash {
        return Err(StoreError::HashMismatch);
    }
    Ok(bytes)
}

pub fn get(hash: &str, base: &Path) -> Result<String, StoreError> {
    get_bytes(hash, base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cert::emit;
    use crate::key::new_keypair;
    use crate::tcb::TcbManifest;
    use apsl_core::ast::*;

    fn trivial_node() -> Node {
        Node {
            name: Ident::new("t"),
            sig: TypeSig {
                params: vec![Param {
                    name: Ident::new("in"),
                    ty: Type::Base(Ident::new("Int")),
                }],
                ret: Type::Base(Ident::new("Int")),
            },
            pre: vec![],
            post: vec![],
            cx: CxSpec {
                bigo: CxExpr::Const,
                class: RuntimeClass::Idem,
            },
            sla: None,
            via: None,
            auth: AuthLevel::None,
            scope_constraint: ScopeConstraint::Any,
            audit_req: AuditReq::None,
            state: vec![],
            deploy: None,
            span: Span::NONE,
        }
    }

    fn tempdir() -> std::path::PathBuf {
        let base = std::env::temp_dir();
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let dir = base.join(format!("apsl-cert-store-{}-{}", pid, nanos));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn put_and_get_roundtrip() {
        let dir = tempdir();
        let (sk, _vk) = new_keypair();
        let c = emit(
            &trivial_node(),
            None,
            "ok",
            "O(1)",
            vec![],
            TcbManifest::default(),
            &sk,
        );
        let h = put(&c, &dir).unwrap();
        let bytes = get_bytes(&h, &dir).unwrap();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn missing_cert_errors() {
        let dir = tempdir();
        let r = get_bytes("00".repeat(32).as_str(), &dir);
        assert!(matches!(r, Err(StoreError::NotFound(_))));
    }
}
