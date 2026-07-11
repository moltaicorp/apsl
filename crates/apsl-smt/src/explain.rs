use apsl_core::ast::Node;

use crate::solver::Model;

pub fn explain(node: &Node, clause_id: usize, model: &Model) -> String {
    let mut bindings = String::new();
    for (i, (k, v)) in model.bindings.iter().enumerate() {
        if i > 0 {
            bindings.push_str(", ");
        }
        bindings.push_str(&format!("{} = {}", k, v));
    }
    if bindings.is_empty() {
        bindings = "<no model bindings reported>".into();
    }
    format!(
        "In clause {} of node {}: the post-condition fails when {}.",
        clause_id,
        node.name.as_str(),
        bindings
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use apsl_core::ast::*;

    #[test]
    fn explain_includes_node_name() {
        let n = Node {
            name: Ident::new("dedupe"),
            sig: TypeSig {
                params: vec![],
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
        };
        let m = Model {
            bindings: vec![("x".into(), "42".into())],
        };
        let s = explain(&n, 0, &m);
        assert!(s.contains("dedupe"));
        assert!(s.contains("x = 42"));
    }
}
