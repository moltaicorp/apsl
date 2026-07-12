use ed25519_dalek::SigningKey;

use apsl_cert::cert::{emit, ClauseProof};
use apsl_cert::store::put;
use apsl_cert::tcb::TcbManifest;
use apsl_complex::{dominant_weight, prove, NodeStatus, Weight};
use apsl_core::ast::{CxExpr, Decl, Node};
use apsl_parse::parse_str;
use apsl_smt::{default_solver, discharge_node, encode::EmptyTypeOracle, ClauseStatus};
use apsl_types::type_check;

#[derive(Debug, Clone)]
pub struct CertResult {
    pub node: String,
    pub cert_hash: String,
    pub impl_hash: String,
}

#[derive(Debug)]
pub enum CompileError {
    Parse(String),
    Type(String),
    ComplexityExceeds(String),
    Predicate(String),
    Store(String),
    NoNodes,
}

impl CompileError {
    pub fn stage(&self) -> &'static str {
        match self {
            CompileError::Parse(_) => "parse",
            CompileError::Type(_) => "typecheck",
            CompileError::ComplexityExceeds(_) => "complexity",
            CompileError::Predicate(_) => "predicate",
            CompileError::Store(_) => "store",
            CompileError::NoNodes => "parse",
        }
    }
    pub fn message(&self) -> String {
        match self {
            CompileError::Parse(m)
            | CompileError::Type(m)
            | CompileError::ComplexityExceeds(m)
            | CompileError::Predicate(m)
            | CompileError::Store(m) => m.clone(),
            CompileError::NoNodes => "no nodes in source".into(),
        }
    }
}

fn pinned_tcb() -> TcbManifest {
    let mut t = TcbManifest::default();
    t.add("apsl-complex", "self", "0.1.0");
    t.add("apsl-core", "self", "0.1.0");
    t.add("apsl-parse", "self", "0.1.0");
    t.add("apsl-smt", "self", "0.1.0");
    t.add("apsl-types", "self", "0.1.0");
    t
}

pub fn compile(
    src: &str,
    store_base: &std::path::Path,
    key: &SigningKey,
    impl_hashes: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<CertResult>, CompileError> {
    let prog = parse_str(src).map_err(|e| CompileError::Parse(e.to_string()))?;
    let tp = type_check(&prog).map_err(|errs| {
        CompileError::Type(
            errs.iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;
    let cx_report = prove(&tp);
    let solver = default_solver();
    let tcb = pinned_tcb();

    let mut pending = Vec::new();
    for d in &tp.program.decls {
        let Decl::Node(n) = d else { continue };
        let cx_entry = cx_report.per_node.iter().find(|r| r.node == n.name);
        let (verdict, derived) = match cx_entry {
            Some(r) => {
                let v = match r.status {
                    NodeStatus::Ok => "ok",
                    NodeStatus::Exceeds { .. } => "exceeds",
                    NodeStatus::Mismatch { .. } => "mismatch",
                };
                (v.to_string(), render_cx(&r.derived))
            }
            None => ("ok".into(), "O(1)".into()),
        };
        if verdict == "exceeds" {
            return Err(CompileError::ComplexityExceeds(format!(
                "{} complexity exceeds O(n log n)",
                n.name.as_str()
            )));
        }
        let dr = discharge_node(n, &EmptyTypeOracle, &*solver);
        for clause in &dr.per_clause {
            match &clause.status {
                ClauseStatus::Proved => {}
                ClauseStatus::Counterexample(_) => {
                    return Err(CompileError::Predicate(format!(
                        "{} clause {} has a counterexample",
                        n.name, clause.clause_id
                    )));
                }
                ClauseStatus::Unknown(message) => {
                    return Err(CompileError::Predicate(format!(
                        "{} clause {} is unknown: {}",
                        n.name, clause.clause_id, message
                    )));
                }
                ClauseStatus::EncodingError(message) => {
                    return Err(CompileError::Predicate(format!(
                        "{} clause {} encoding failed: {}",
                        n.name, clause.clause_id, message
                    )));
                }
            }
        }
        let proofs: Vec<ClauseProof> = dr
            .per_clause
            .iter()
            .map(|c| {
                let (verdict, note) = match &c.status {
                    ClauseStatus::Proved => ("proved", String::new()),
                    ClauseStatus::Counterexample(_) => ("cex", "counterexample reported".into()),
                    ClauseStatus::Unknown(m) => ("unknown", m.clone()),
                    ClauseStatus::EncodingError(m) => ("error", m.clone()),
                };
                ClauseProof {
                    clause_id: c.clause_id,
                    verdict: verdict.into(),
                    note,
                }
            })
            .collect();
        let bound = impl_hashes(n.name.as_str());
        let cert = emit(
            n,
            bound.as_deref(),
            &verdict,
            &derived,
            proofs,
            tcb.clone(),
            key,
        );
        pending.push((n.name.as_str().to_string(), bound, cert));
    }
    if pending.is_empty() {
        return Err(CompileError::NoNodes);
    }
    let mut out = Vec::with_capacity(pending.len());
    for (node, bound, cert) in pending {
        let cert_hash = put(&cert, store_base).map_err(|e| CompileError::Store(e.to_string()))?;
        out.push(CertResult {
            node,
            cert_hash,
            impl_hash: bound.unwrap_or_default(),
        });
    }
    Ok(out)
}

pub fn node_names(src: &str) -> Result<Vec<String>, CompileError> {
    let prog = parse_str(src).map_err(|e| CompileError::Parse(e.to_string()))?;
    let names: Vec<String> = prog
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Node(n) => Some(n.name.as_str().to_string()),
            _ => None,
        })
        .collect();
    if names.is_empty() {
        return Err(CompileError::NoNodes);
    }
    Ok(names)
}

pub fn render_cx(e: &CxExpr) -> String {
    fn r(e: &CxExpr) -> String {
        use CxExpr::*;
        match e {
            Const => "1".into(),
            Size(n) => n.as_str().to_string(),
            LogN(n) => format!("log {}", n.as_str()),
            NLogN(n) => format!("{} log {}", n.as_str(), n.as_str()),
            Sum(es) => {
                let top = es
                    .iter()
                    .map(dominant_weight)
                    .max()
                    .unwrap_or(Weight::Const);
                let keep: Vec<&CxExpr> = es.iter().filter(|x| dominant_weight(x) == top).collect();
                keep.iter().map(|x| r(x)).collect::<Vec<_>>().join(" + ")
            }
            Prod(es) => {
                let mut strs: Vec<String> =
                    es.iter().filter(|x| !matches!(x, Const)).map(r).collect();
                if strs.is_empty() {
                    strs.push("1".into());
                }
                strs.join(" * ")
            }
            Max(es) => format!("max({})", es.iter().map(r).collect::<Vec<_>>().join(", ")),
        }
    }
    format!("O({})", r(e))
}

#[allow(dead_code)]
fn _node_marker(_: &Node) {}
