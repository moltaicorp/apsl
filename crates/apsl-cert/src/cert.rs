use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

use apsl_core::ast::Node;
use apsl_core::canon::{write_str, ArrayWriter, Canon, ObjectWriter};
use apsl_core::hash::sha256_hex;

use crate::tcb::TcbManifest;

#[derive(Debug, Clone)]
pub struct ClauseProof {
    pub clause_id: usize,
    pub verdict: String,
    pub note: String,
}

impl Canon for ClauseProof {
    fn write_canon(&self, out: &mut String) {
        let mut ow = ObjectWriter::new(out);
        ow.field("c", |o| {
            let mut s = String::new();
            s.push_str(&self.clause_id.to_string());
            write_str(o, &s);
        });
        ow.field("v", |o| write_str(o, &self.verdict));
        ow.field("n", |o| write_str(o, &self.note));
        ow.finish();
    }
}

#[derive(Debug, Clone)]
pub struct Certificate {
    pub version: u32,
    pub contract_hash: String,
    pub impl_hash: String,
    pub cx_verdict: String,
    pub cx_derived: String,
    pub pred_proofs: Vec<ClauseProof>,
    pub tcb_manifest: TcbManifest,
    pub signer_fingerprint: String,
    pub sig_hex: String,
}

impl Canon for Certificate {
    fn write_canon(&self, out: &mut String) {
        let mut ow = ObjectWriter::new(out);
        ow.field("contract", |o| write_str(o, &self.contract_hash));
        ow.field("cx_derived", |o| write_str(o, &self.cx_derived));
        ow.field("cx_verdict", |o| write_str(o, &self.cx_verdict));
        ow.field("impl", |o| write_str(o, &self.impl_hash));
        ow.field("preds", |o| {
            let mut aw = ArrayWriter::new(o);
            for p in &self.pred_proofs {
                aw.item(|o2| p.write_canon(o2));
            }
            aw.finish();
        });
        ow.field("sig", |o| write_str(o, &self.sig_hex));
        ow.field("signer", |o| write_str(o, &self.signer_fingerprint));
        ow.field("tcb", |o| self.tcb_manifest.write_canon(o));
        ow.field("version", |o| {
            let s = self.version.to_string();
            o.push_str(&s);
        });
        ow.finish();
    }
}

fn canon_without_sig(c: &Certificate) -> String {
    let mut tmp = c.clone();
    tmp.sig_hex = String::new();
    tmp.canon()
}

pub fn emit(
    node: &Node,
    impl_canon_hash: Option<&str>,
    cx_verdict: &str,
    cx_derived: &str,
    pred_proofs: Vec<ClauseProof>,
    tcb: TcbManifest,
    key: &SigningKey,
) -> Certificate {
    let contract_hash = sha256_hex(node.canon().as_bytes());
    let impl_hash = impl_canon_hash.unwrap_or("").to_string();
    let signer_fingerprint = crate::key::fingerprint(&key.verifying_key());
    let mut c = Certificate {
        version: 1,
        contract_hash,
        impl_hash,
        cx_verdict: cx_verdict.to_string(),
        cx_derived: cx_derived.to_string(),
        pred_proofs,
        tcb_manifest: tcb,
        signer_fingerprint,
        sig_hex: String::new(),
    };
    let body = canon_without_sig(&c);
    let sig: Signature = key.sign(body.as_bytes());
    c.sig_hex = bytes_to_hex(sig.to_bytes().as_slice());
    c
}

pub fn verify(
    c: &Certificate,
    verifying_key: &VerifyingKey,
    expected_tcb: &TcbManifest,
) -> Result<(), VerifyError> {
    let fp = crate::key::fingerprint(verifying_key);
    if fp != c.signer_fingerprint {
        return Err(VerifyError::SignerMismatch);
    }
    if c.tcb_manifest != *expected_tcb {
        return Err(VerifyError::TcbMismatch);
    }
    let body = canon_without_sig(c);
    let sig_bytes = hex_to_bytes(&c.sig_hex).ok_or(VerifyError::MalformedSignature)?;
    if sig_bytes.len() != 64 {
        return Err(VerifyError::MalformedSignature);
    }
    let mut arr = [0u8; 64];
    arr.copy_from_slice(&sig_bytes);
    let sig = Signature::from_bytes(&arr);
    verifying_key
        .verify(body.as_bytes(), &sig)
        .map_err(|_| VerifyError::SignatureInvalid)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    SignerMismatch,
    TcbMismatch,
    MalformedSignature,
    SignatureInvalid,
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

pub fn parse_cert_json(json: &str) -> Result<Certificate, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid cert JSON: {}", e))?;
    let obj = v.as_object().ok_or("cert is not a JSON object")?;

    let s = |key: &str| -> Result<String, String> {
        obj.get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("missing or non-string field: {}", key))
    };

    let version = obj
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or("missing or non-integer version")? as u32;

    let preds = obj
        .get("preds")
        .and_then(|v| v.as_array())
        .ok_or("missing or non-array preds")?;
    let mut pred_proofs = Vec::new();
    for p in preds {
        let po = p.as_object().ok_or("pred entry is not an object")?;
        pred_proofs.push(ClauseProof {
            clause_id: po
                .get("c")
                .and_then(|v| v.as_str())
                .unwrap_or("0")
                .parse::<usize>()
                .unwrap_or(0),
            verdict: po
                .get("v")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            note: po
                .get("n")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        });
    }

    let tcb_arr = obj
        .get("tcb")
        .and_then(|v| v.as_array())
        .ok_or("missing or non-array tcb")?;
    let mut tcb = TcbManifest::default();
    for entry in tcb_arr {
        let eo = entry.as_object().ok_or("tcb entry is not an object")?;
        let n = eo.get("n").and_then(|v| v.as_str()).unwrap_or("");
        let h = eo.get("h").and_then(|v| v.as_str()).unwrap_or("");
        let ver = eo.get("v").and_then(|v| v.as_str()).unwrap_or("");
        tcb.add(n, h, ver);
    }

    tcb.components.sort();

    Ok(Certificate {
        version,
        contract_hash: s("contract")?,
        impl_hash: s("impl").unwrap_or_default(),
        cx_verdict: s("cx_verdict")?,
        cx_derived: s("cx_derived")?,
        pred_proofs,
        tcb_manifest: tcb,
        signer_fingerprint: s("signer")?,
        sig_hex: s("sig")?,
    })
}

fn bytes_to_hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        use std::fmt::Write;
        let _ = write!(&mut s, "{:02x}", x);
    }
    s
}

fn hex_to_bytes(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    for i in (0..s.len()).step_by(2) {
        let hi = hex_digit(bytes[i])?;
        let lo = hex_digit(bytes[i + 1])?;
        out.push((hi << 4) | lo);
    }
    Some(out)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            span: Span::NONE,
        }
    }

    #[test]
    fn round_trip_sign_and_verify() {
        let (sk, vk) = new_keypair();
        let tcb = TcbManifest::default();
        let c = emit(
            &trivial_node(),
            None,
            "ok",
            "O(1)",
            vec![],
            tcb.clone(),
            &sk,
        );
        verify(&c, &vk, &tcb).unwrap();
    }

    #[test]
    fn tampered_cert_rejected() {
        let (sk, vk) = new_keypair();
        let tcb = TcbManifest::default();
        let mut c = emit(
            &trivial_node(),
            None,
            "ok",
            "O(1)",
            vec![],
            tcb.clone(),
            &sk,
        );
        c.cx_derived = "O(n^2)".into();
        let r = verify(&c, &vk, &tcb);
        assert!(matches!(r, Err(VerifyError::SignatureInvalid)));
    }

    #[test]
    fn signer_mismatch_rejected() {
        let (sk, _vk) = new_keypair();
        let (_sk2, vk2) = new_keypair();
        let tcb = TcbManifest::default();
        let c = emit(
            &trivial_node(),
            None,
            "ok",
            "O(1)",
            vec![],
            tcb.clone(),
            &sk,
        );
        let r = verify(&c, &vk2, &tcb);
        assert!(matches!(r, Err(VerifyError::SignerMismatch)));
    }

    #[test]
    fn tcb_mismatch_rejected() {
        let (sk, vk) = new_keypair();
        let tcb = TcbManifest::default();
        let mut tcb2 = TcbManifest::default();
        tcb2.components
            .push(("z3".into(), "deadbeef".into(), "4.13".into()));
        let c = emit(
            &trivial_node(),
            None,
            "ok",
            "O(1)",
            vec![],
            tcb.clone(),
            &sk,
        );
        let r = verify(&c, &vk, &tcb2);
        assert!(matches!(r, Err(VerifyError::TcbMismatch)));
    }
}
