#![forbid(unsafe_code)]

mod algebra;
mod derive;
mod hints;

use std::collections::BTreeSet;

use apsl_core::ast::{CxExpr, Decl, Ident, Node};
use apsl_types::TypedProgram;

pub use algebra::{dominant_term, dominant_weight, exceeds_n_log_n, normalize, Term, Weight};
pub use derive::derive_node_cost;
pub use hints::{hint_for_status, HintKind};

#[derive(Debug, Clone)]
pub struct NodeReport {
    pub node: Ident,
    pub derived: CxExpr,
    pub declared: CxExpr,
    pub status: NodeStatus,
}

#[derive(Debug, Clone)]
pub enum NodeStatus {
    Ok,
    Exceeds { hint: String, kind: HintKind },
    Mismatch { hint: String },
}

#[derive(Debug, Clone, Default)]
pub struct ComplexityReport {
    pub per_node: Vec<NodeReport>,
}

impl ComplexityReport {
    pub fn all_ok(&self) -> bool {
        self.per_node
            .iter()
            .all(|n| matches!(n.status, NodeStatus::Ok))
    }
}

pub fn prove(tp: &TypedProgram) -> ComplexityReport {
    let mut report = ComplexityReport::default();
    for d in &tp.program.decls {
        if let Decl::Node(n) = d {
            report.per_node.push(prove_node(n));
        }
    }
    report
}

fn prove_node(n: &Node) -> NodeReport {
    let derived = derive_node_cost(n);
    let declared = n.cx.bigo.clone();
    let size_vars = collect_size_vars(n);
    let status = if exceeds_n_log_n(&derived, &size_vars) {
        let kind = hints::classify(&derived);
        let hint = hints::hint_for_kind(kind);
        NodeStatus::Exceeds { hint, kind }
    } else if exceeds(&derived, &declared, &size_vars) {
        NodeStatus::Mismatch {
            hint: format!(
                "you declared O({:?}) but the body's derived cost is O({:?})",
                summary(&declared),
                summary(&derived),
            ),
        }
    } else {
        NodeStatus::Ok
    };
    NodeReport {
        node: n.name.clone(),
        derived,
        declared,
        status,
    }
}

fn collect_size_vars(n: &Node) -> BTreeSet<Ident> {
    let mut out = BTreeSet::new();
    for p in &n.sig.params {
        if matches!(p.ty, apsl_core::ast::Type::List(_)) {
            out.insert(p.name.clone());
        }
    }
    out
}

fn summary(e: &CxExpr) -> String {
    use apsl_core::ast::CxExpr::*;
    let simplified = simplify(e);
    match simplified {
        Const => "1".into(),
        Size(n) => n.as_str().to_string(),
        LogN(n) => format!("log {}", n.as_str()),
        NLogN(n) => format!("{} log {}", n.as_str(), n.as_str()),
        Sum(es) => es.iter().map(summary).collect::<Vec<_>>().join(" + "),
        Prod(es) => es.iter().map(summary).collect::<Vec<_>>().join(" * "),
        Max(es) => format!(
            "max({})",
            es.iter().map(summary).collect::<Vec<_>>().join(", ")
        ),
    }
}

fn simplify(e: &CxExpr) -> CxExpr {
    use apsl_core::ast::CxExpr::*;
    match e {
        Sum(xs) => {
            let mut flat = Vec::new();
            for x in xs {
                match simplify(x) {
                    Sum(inner) => flat.extend(inner),
                    Const => {}
                    other => flat.push(other),
                }
            }
            if flat.is_empty() {
                Const
            } else if flat.len() == 1 {
                flat.remove(0)
            } else {
                flat.sort_by_key(|value| std::cmp::Reverse(dominant_weight(value)));
                flat.dedup();
                Sum(flat)
            }
        }
        Prod(xs) => {
            let mut flat = Vec::new();
            for x in xs {
                match simplify(x) {
                    Prod(inner) => flat.extend(inner),
                    Const => {}
                    other => flat.push(other),
                }
            }
            if flat.is_empty() {
                Const
            } else if flat.len() == 1 {
                flat.remove(0)
            } else {
                Prod(flat)
            }
        }
        Max(xs) => {
            let mut flat: Vec<_> = xs.iter().map(simplify).collect();
            flat.sort_by_key(|value| std::cmp::Reverse(dominant_weight(value)));
            flat.dedup();
            if flat.len() == 1 {
                flat.remove(0)
            } else {
                Max(flat)
            }
        }
        other => other.clone(),
    }
}

fn exceeds(derived: &CxExpr, declared: &CxExpr, _vars: &BTreeSet<Ident>) -> bool {
    dominant_weight(derived) > dominant_weight(declared)
}
