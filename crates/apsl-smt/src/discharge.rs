use apsl_core::ast::Node;

use crate::encode::{encode_vc, TypeOracle};
use crate::solver::{Model, Solver, SolverResult};

#[derive(Debug, Clone)]
pub struct ClauseResult {
    pub clause_id: usize,
    pub status: ClauseStatus,
}

#[derive(Debug, Clone)]
pub enum ClauseStatus {
    Proved,
    Counterexample(Model),
    Unknown(String),
    EncodingError(String),
}

#[derive(Debug, Clone, Default)]
pub struct DischargeReport {
    pub per_clause: Vec<ClauseResult>,
}

pub fn discharge_node(node: &Node, types: &dyn TypeOracle, solver: &dyn Solver) -> DischargeReport {
    let script = encode_vc(node, types);
    let mut report = DischargeReport::default();
    let status = match solver.check(&script) {
        SolverResult::Unsat => ClauseStatus::Proved,
        SolverResult::Sat(m) => ClauseStatus::Counterexample(m),
        SolverResult::Unknown(msg) => ClauseStatus::Unknown(msg),
        SolverResult::Error(msg) => ClauseStatus::EncodingError(msg),
    };
    report.per_clause.push(ClauseResult {
        clause_id: 0,
        status,
    });
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::EmptyTypeOracle;
    use crate::solver::NullSolver;
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
            post: vec![Expr::Lit(Lit::Bool(true), Span::NONE)],
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

    #[test]
    fn null_solver_yields_unknown() {
        let n = trivial_node();
        let r = discharge_node(&n, &EmptyTypeOracle, &NullSolver);
        assert_eq!(r.per_clause.len(), 1);
        match &r.per_clause[0].status {
            ClauseStatus::Unknown(_) => {}
            other => panic!("expected Unknown, got {:?}", other),
        }
    }
}
