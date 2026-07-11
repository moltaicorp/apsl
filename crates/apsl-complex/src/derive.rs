use std::collections::BTreeMap;

use apsl_core::ast::{CxExpr, Expr, Ident, Node, Type};

pub fn derive_node_cost(n: &Node) -> CxExpr {
    let mut size_for = BTreeMap::new();
    for p in &n.sig.params {
        if matches!(p.ty, Type::List(_)) {
            size_for.insert(p.name.clone(), p.name.clone());
        }
    }
    if n.sig.params.len() == 1 && matches!(n.sig.params[0].ty, Type::List(_)) {
        size_for.insert(Ident::new("in"), n.sig.params[0].name.clone());
    }
    if matches!(n.sig.ret, Type::List(_)) {
        size_for.insert(Ident::new("out"), Ident::new("n_out"));
    }

    let mut parts = Vec::new();
    for p in n.pre.iter().chain(n.post.iter()) {
        parts.push(cost_of(p, &size_for));
    }
    if parts.is_empty() {
        CxExpr::Const
    } else {
        simplify_sum(parts)
    }
}

fn simplify_sum(mut xs: Vec<CxExpr>) -> CxExpr {
    if xs.len() == 1 {
        return xs.pop().unwrap();
    }
    CxExpr::Sum(xs)
}

fn size_for_expr(e: &Expr, size_for: &BTreeMap<Ident, Ident>) -> Option<Ident> {
    match e {
        Expr::Var(id, _) => size_for.get(id).cloned(),
        Expr::Field(_, _, _) => None,
        _ => None,
    }
}

fn cost_of(e: &Expr, size_for: &BTreeMap<Ident, Ident>) -> CxExpr {
    match e {
        Expr::Lit(_, _) | Expr::Var(_, _) => CxExpr::Const,
        Expr::Field(inner, _, _) => cost_of(inner, size_for),
        Expr::Apply(name, args, _) => apply_cost(name, args, size_for),
        Expr::Bin(_, l, r, _) => CxExpr::Sum(vec![cost_of(l, size_for), cost_of(r, size_for)]),
        Expr::Un(_, e, _) => cost_of(e, size_for),
        Expr::Quant(_, _, dom, body, _) => {
            let body_cost = cost_of(body, size_for);
            match size_for_expr(dom, size_for) {
                Some(sv) => CxExpr::Prod(vec![CxExpr::Size(sv), body_cost]),
                None => body_cost,
            }
        }
        Expr::If(c, a, b, _) => CxExpr::Sum(vec![
            cost_of(c, size_for),
            CxExpr::Max(vec![cost_of(a, size_for), cost_of(b, size_for)]),
        ]),
        Expr::Let(_, e1, body, _) => {
            CxExpr::Sum(vec![cost_of(e1, size_for), cost_of(body, size_for)])
        }
        Expr::Tuple(es, _) => CxExpr::Sum(es.iter().map(|e| cost_of(e, size_for)).collect()),
        Expr::Lam(_, body, _) => cost_of(body, size_for),
    }
}

fn apply_cost(name: &Ident, args: &[Expr], size_for: &BTreeMap<Ident, Ident>) -> CxExpr {
    let n = name.as_str();
    let arg_cost = if args.is_empty() {
        CxExpr::Const
    } else {
        CxExpr::Sum(args.iter().map(|a| cost_of(a, size_for)).collect())
    };
    let body_cost = |i: usize| -> CxExpr {
        if let Some(a) = args.get(i) {
            match a {
                Expr::Lam(_, body, _) => cost_of(body, size_for),
                _ => cost_of(a, size_for),
            }
        } else {
            CxExpr::Const
        }
    };
    let intrinsic = match n {
        "+" | "-" | "*" | "div" | "mod" | "=" | "!=" | "<" | "<=" | ">" | ">=" | "and" | "or"
        | "not" | "cons" | "nil" | "head" | "tail" | "nth" | "len" => CxExpr::Const,
        "valid_email?" | "well_formed_json?" => CxExpr::Const,
        "every" | "some" | "count" => match args.first().and_then(|a| size_for_expr(a, size_for)) {
            Some(sv) => CxExpr::Prod(vec![CxExpr::Size(sv), body_cost(1)]),
            None => body_cost(1),
        },
        "map" | "filter" => match args.get(1).and_then(|a| size_for_expr(a, size_for)) {
            Some(sv) => CxExpr::Prod(vec![CxExpr::Size(sv), body_cost(0)]),
            None => body_cost(0),
        },
        "fold" => match args.get(2).and_then(|a| size_for_expr(a, size_for)) {
            Some(sv) => CxExpr::Prod(vec![CxExpr::Size(sv), body_cost(0)]),
            None => body_cost(0),
        },
        "group_by" | "sort_by" => match args.first().and_then(|a| size_for_expr(a, size_for)) {
            Some(sv) => CxExpr::Prod(vec![CxExpr::NLogN(sv), body_cost(1)]),
            None => body_cost(1),
        },
        "sort" | "unique?" | "dedupe" => {
            match args.first().and_then(|a| size_for_expr(a, size_for)) {
                Some(sv) => CxExpr::NLogN(sv),
                None => CxExpr::Const,
            }
        }
        "subseteq?" => {
            let a0 = args.first().and_then(|a| size_for_expr(a, size_for));
            let a1 = args.get(1).and_then(|a| size_for_expr(a, size_for));
            match (a0, a1) {
                (Some(s0), Some(s1)) => CxExpr::Sum(vec![CxExpr::NLogN(s0), CxExpr::NLogN(s1)]),
                (Some(s), None) | (None, Some(s)) => CxExpr::NLogN(s),
                (None, None) => CxExpr::Const,
            }
        }
        "concat" | "zip" | "reverse" => {
            match args.first().and_then(|a| size_for_expr(a, size_for)) {
                Some(sv) => CxExpr::Size(sv),
                None => CxExpr::Const,
            }
        }
        _ => CxExpr::Const,
    };
    match (arg_cost, intrinsic) {
        (CxExpr::Const, CxExpr::Const) => CxExpr::Const,
        (a, CxExpr::Const) => a,
        (CxExpr::Const, b) => b,
        (a, b) => CxExpr::Sum(vec![a, b]),
    }
}
