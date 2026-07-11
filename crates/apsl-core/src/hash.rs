use crate::canon::Canon;
use sha2::{Digest, Sha256};

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

    fn node() -> Node {
        Node {
            name: Ident::new("guarded"),
            sig: TypeSig {
                params: vec![Param {
                    name: Ident::new("in"),
                    ty: Type::Base(Ident::new("Int")),
                }],
                ret: Type::Base(Ident::new("Int")),
            },
            pre: Vec::new(),
            post: Vec::new(),
            cx: CxSpec {
                bigo: CxExpr::Const,
                class: RuntimeClass::Idem,
            },
            sla: None,
            via: None,
            auth: AuthLevel::None,
            scope_constraint: ScopeConstraint::Any,
            audit_req: AuditReq::None,
            state: Vec::new(),
            deploy: None,
            span: Span::NONE,
        }
    }

    #[test]
    fn empty_program_hash_stable() {
        let p = Program::new();
        let h1 = p.content_hash();
        let h2 = p.content_hash();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn authority_clauses_change_node_identity() {
        let base = node();

        let mut auth = base.clone();
        auth.auth = AuthLevel::Passkey;
        assert_ne!(base.content_hash(), auth.content_hash());

        let mut scope = base.clone();
        scope.scope_constraint = ScopeConstraint::Narrowing;
        assert_ne!(base.content_hash(), scope.content_hash());

        let mut audit = base.clone();
        audit.audit_req = AuditReq::Both;
        assert_ne!(base.content_hash(), audit.content_hash());
    }
}
