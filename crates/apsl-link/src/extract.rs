use std::collections::BTreeSet;

use apsl_core::ast::*;

const BASE_TYPES: &[&str] = &["Int", "Rat", "Bool", "String", "Float", "Real", "World"];

pub fn locally_defined(p: &Program) -> BTreeSet<String> {
    let mut defined = BTreeSet::new();
    for d in &p.decls {
        match d {
            Decl::Type(ta) => {
                defined.insert(ta.name.as_str().to_string());
            }
            Decl::Node(n) => {
                defined.insert(n.name.as_str().to_string());
            }
            Decl::Graph(g) => {
                defined.insert(g.name.as_str().to_string());
            }
        }
    }
    defined
}

pub fn unresolved_symbols(p: &Program, local: &BTreeSet<String>) -> BTreeSet<String> {
    let mut referenced = BTreeSet::new();
    let bound = BTreeSet::new();
    for d in &p.decls {
        match d {
            Decl::Type(ta) => {
                collect_type_refs(&ta.rhs, &mut referenced);
                for supertype in &ta.supertypes {
                    referenced.insert(supertype.as_str().to_string());
                }
            }
            Decl::Node(n) => {
                collect_sig_refs(&n.sig, &mut referenced);
                for state in &n.state {
                    collect_type_refs(&state.ty, &mut referenced);
                }
                let mut node_bound = bound.clone();
                for p in &n.sig.params {
                    node_bound.insert(p.name.as_str().to_string());
                }
                for e in n.pre.iter().chain(n.post.iter()) {
                    collect_expr_refs(e, &mut referenced, &node_bound);
                }
            }
            Decl::Graph(g) => {
                collect_sig_refs(&g.sig, &mut referenced);
                for state in &g.state {
                    collect_type_refs(&state.ty, &mut referenced);
                }
                let mut graph_bound = bound.clone();
                for p in &g.sig.params {
                    graph_bound.insert(p.name.as_str().to_string());
                }
                for e in &g.post {
                    collect_expr_refs(e, &mut referenced, &graph_bound);
                }
                for chain in &g.flow {
                    for step in chain {
                        for node_name in &step.nodes {
                            let n = node_name.as_str();
                            if n != "in" && n != "out" {
                                referenced.insert(n.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    referenced
        .into_iter()
        .filter(|s| {
            !local.contains(s) && !BASE_TYPES.contains(&s.as_str()) && !apsl_types::is_primitive(s)
        })
        .collect()
}

fn collect_type_refs(ty: &Type, out: &mut BTreeSet<String>) {
    match ty {
        Type::Base(id) => {
            let name = id.as_str();
            if !BASE_TYPES.contains(&name) {
                out.insert(name.to_string());
            }
        }
        Type::List(inner) => collect_type_refs(inner, out),
        Type::Tuple(elems) => {
            for e in elems {
                collect_type_refs(e, out);
            }
        }
        Type::Var(_) => {}
        Type::Result(inner) => collect_type_refs(inner, out),
        Type::Record(fields) => {
            for (_, ty) in fields {
                collect_type_refs(ty, out);
            }
        }
        Type::Parameterized(name, args) => {
            let n = name.as_str();
            if !BASE_TYPES.contains(&n) {
                out.insert(n.to_string());
            }
            for arg in args {
                collect_type_refs(arg, out);
            }
        }
    }
}

fn collect_sig_refs(sig: &TypeSig, out: &mut BTreeSet<String>) {
    for p in &sig.params {
        collect_type_refs(&p.ty, out);
    }
    collect_type_refs(&sig.ret, out);
}

fn collect_expr_refs(e: &Expr, out: &mut BTreeSet<String>, bound: &BTreeSet<String>) {
    match e {
        Expr::Lit(_, _) => {}
        Expr::Var(id, _) => {
            let name = id.as_str();
            if name != "in"
                && name != "out"
                && name != "true"
                && name != "false"
                && !bound.contains(name)
            {
                out.insert(name.to_string());
            }
        }
        Expr::Field(inner, _, _) => collect_expr_refs(inner, out, bound),
        Expr::Apply(name, args, _) => {
            let n = name.as_str();
            if !bound.contains(n) {
                out.insert(n.to_string());
            }
            for a in args {
                collect_expr_refs(a, out, bound);
            }
        }
        Expr::Bin(_, l, r, _) => {
            collect_expr_refs(l, out, bound);
            collect_expr_refs(r, out, bound);
        }
        Expr::Un(_, inner, _) => collect_expr_refs(inner, out, bound),
        Expr::Quant(_, var, domain, body, _) => {
            collect_expr_refs(domain, out, bound);
            let mut inner_bound = bound.clone();
            inner_bound.insert(var.as_str().to_string());
            collect_expr_refs(body, out, &inner_bound);
        }
        Expr::If(c, a, b, _) => {
            collect_expr_refs(c, out, bound);
            collect_expr_refs(a, out, bound);
            collect_expr_refs(b, out, bound);
        }
        Expr::Let(name, e1, body, _) => {
            collect_expr_refs(e1, out, bound);
            let mut inner_bound = bound.clone();
            inner_bound.insert(name.as_str().to_string());
            collect_expr_refs(body, out, &inner_bound);
        }
        Expr::Tuple(es, _) => {
            for e in es {
                collect_expr_refs(e, out, bound);
            }
        }
        Expr::Lam(params, body, _) => {
            let mut inner_bound = bound.clone();
            for p in params {
                inner_bound.insert(p.as_str().to_string());
            }
            collect_expr_refs(body, out, &inner_bound);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_program_no_unresolved() {
        let p = Program::new();
        let local = locally_defined(&p);
        let unresolved = unresolved_symbols(&p, &local);
        assert!(unresolved.is_empty());
    }

    #[test]
    fn type_alias_to_base_resolved() {
        let mut p = Program::new();
        p.decls.push(Decl::Type(TypeAlias {
            name: Ident::new("Email"),
            rhs: Type::Base(Ident::new("String")),
            supertypes: vec![],
            span: Span::NONE,
        }));
        let local = locally_defined(&p);
        assert!(local.contains("Email"));
        let unresolved = unresolved_symbols(&p, &local);
        assert!(unresolved.is_empty());
    }

    #[test]
    fn type_alias_to_unknown_unresolved() {
        let mut p = Program::new();
        p.decls.push(Decl::Type(TypeAlias {
            name: Ident::new("Email"),
            rhs: Type::Base(Ident::new("CustomType")),
            supertypes: vec![],
            span: Span::NONE,
        }));
        let local = locally_defined(&p);
        let unresolved = unresolved_symbols(&p, &local);
        assert!(unresolved.contains("CustomType"));
    }
}
