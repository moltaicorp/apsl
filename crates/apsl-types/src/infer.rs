use std::collections::{BTreeMap, HashMap};
use std::fmt;

#[cfg(test)]
use apsl_core::ast::Ident;
use apsl_core::ast::{
    BinOp, Decl, Expr, Graph, Lit, Node, Program, Quant, Span, Type as AstType, TypeAlias, UnOp,
};

use crate::env::{lambda_slot, primitives};
use crate::types::{ast_type_to_ty, instantiate, unify, Env, Scheme, Subst, Ty, TyGen, UnifyError};

#[derive(Debug, Clone)]
pub struct TypedProgram {
    pub program: Program,
    pub types: TypeMap,
}

pub type TypeMap = HashMap<Span, Ty>;

#[derive(Debug, Clone)]
pub enum TypeErrorKind {
    UnknownName(String),
    Arity {
        name: String,
        expected: usize,
        found: usize,
    },
    Unify(Box<UnifyError>),
    NotATuple(Ty),
    BadIndex {
        ty: Ty,
        index: String,
    },
    AliasCycle(String),
    FlowMismatch {
        left: String,
        right: String,
        want: Box<Ty>,
        got: Box<Ty>,
    },
    NotPredicate(Ty),
}

#[derive(Debug, Clone)]
pub struct TypeError {
    pub msg: String,
    pub span: Span,
    pub kind: TypeErrorKind,
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

pub fn type_check(p: &Program) -> Result<TypedProgram, Vec<TypeError>> {
    let aliases = collect_aliases(p);
    let prim = primitives();

    let mut errors = Vec::new();
    let mut types: TypeMap = HashMap::new();

    let mut user_env: Env = BTreeMap::new();
    for d in &p.decls {
        if let Decl::Node(n) = d {
            let scheme = scheme_for_signature(n, &aliases);
            user_env.insert(n.name.as_str().to_string(), scheme);
        }
    }

    for d in &p.decls {
        match d {
            Decl::Type(_) => {}
            Decl::Node(n) => {
                if let Err(e) = check_node(n, &aliases, &prim, &user_env, &mut types) {
                    errors.extend(e);
                }
            }
            Decl::Graph(g) => {
                if let Err(e) = check_graph(g, &aliases, &prim, &user_env, &mut types) {
                    errors.extend(e);
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(TypedProgram {
            program: p.clone(),
            types,
        })
    } else {
        Err(errors)
    }
}

fn collect_aliases(p: &Program) -> BTreeMap<String, AstType> {
    let mut m = BTreeMap::new();
    for d in &p.decls {
        if let Decl::Type(TypeAlias { name, rhs, .. }) = d {
            m.insert(name.as_str().to_string(), rhs.clone());
        }
    }
    m
}

fn scheme_for_signature(n: &Node, aliases: &BTreeMap<String, AstType>) -> Scheme {
    let args: Vec<Ty> = n
        .sig
        .params
        .iter()
        .map(|p| ast_type_to_ty(&p.ty, aliases))
        .collect();
    let ret = ast_type_to_ty(&n.sig.ret, aliases);
    Scheme::mono(Ty::Fun(args, Box::new(ret)))
}

fn check_node(
    n: &Node,
    aliases: &BTreeMap<String, AstType>,
    prim: &Env,
    user: &Env,
    types: &mut TypeMap,
) -> Result<(), Vec<TypeError>> {
    let mut gen = TyGen::new();
    let mut env: Env = BTreeMap::new();
    env.extend(prim.clone());
    env.extend(user.clone());

    let in_ty = if n.sig.params.len() == 1 {
        let t = ast_type_to_ty(&n.sig.params[0].ty, aliases);
        env.insert(
            n.sig.params[0].name.as_str().to_string(),
            Scheme::mono(t.clone()),
        );
        env.insert("in".into(), Scheme::mono(t.clone()));
        t
    } else {
        let mut field_types = Vec::new();
        for p in &n.sig.params {
            let t = ast_type_to_ty(&p.ty, aliases);
            env.insert(p.name.as_str().to_string(), Scheme::mono(t.clone()));
            field_types.push(t);
        }
        let t = Ty::Tuple(field_types);
        env.insert("in".into(), Scheme::mono(t.clone()));
        t
    };
    let out_ty = ast_type_to_ty(&n.sig.ret, aliases);
    env.insert("out".into(), Scheme::mono(out_ty.clone()));

    let _ = in_ty;
    let mut errs = Vec::new();
    let mut subst = Subst::new();

    for p in &n.pre {
        let ty = match infer(p, &env, &mut subst, &mut gen, types) {
            Ok(t) => t,
            Err(e) => {
                errs.push(e);
                continue;
            }
        };
        match unify(&ty, &Ty::Base("Bool".into())) {
            Ok(s) => subst = s.compose(&subst),
            Err(_) => errs.push(TypeError {
                msg: format!("pre-condition must be Bool, got {}", ty),
                span: p.span(),
                kind: TypeErrorKind::NotPredicate(ty),
            }),
        }
    }
    for p in &n.post {
        let ty = match infer(p, &env, &mut subst, &mut gen, types) {
            Ok(t) => t,
            Err(e) => {
                errs.push(e);
                continue;
            }
        };
        match unify(&ty, &Ty::Base("Bool".into())) {
            Ok(s) => subst = s.compose(&subst),
            Err(_) => errs.push(TypeError {
                msg: format!("post-condition must be Bool, got {}", ty),
                span: p.span(),
                kind: TypeErrorKind::NotPredicate(ty),
            }),
        }
    }

    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

fn check_graph(
    g: &Graph,
    aliases: &BTreeMap<String, AstType>,
    prim: &Env,
    user: &Env,
    types: &mut TypeMap,
) -> Result<(), Vec<TypeError>> {
    let mut gen = TyGen::new();
    let mut env: Env = BTreeMap::new();
    env.extend(prim.clone());
    env.extend(user.clone());

    let in_ty = if g.sig.params.len() == 1 {
        ast_type_to_ty(&g.sig.params[0].ty, aliases)
    } else {
        Ty::Tuple(
            g.sig
                .params
                .iter()
                .map(|p| ast_type_to_ty(&p.ty, aliases))
                .collect(),
        )
    };
    let out_ty = ast_type_to_ty(&g.sig.ret, aliases);
    env.insert("in".into(), Scheme::mono(in_ty.clone()));
    env.insert("out".into(), Scheme::mono(out_ty.clone()));

    let mut errs = Vec::new();
    let mut subst = Subst::new();
    for p in &g.post {
        let ty = match infer(p, &env, &mut subst, &mut gen, types) {
            Ok(t) => t,
            Err(e) => {
                errs.push(e);
                continue;
            }
        };
        match unify(&ty, &Ty::Base("Bool".into())) {
            Ok(s) => subst = s.compose(&subst),
            Err(_) => errs.push(TypeError {
                msg: format!("graph post-condition must be Bool, got {}", ty),
                span: p.span(),
                kind: TypeErrorKind::NotPredicate(ty),
            }),
        }
    }

    let mut node_outputs: std::collections::HashMap<String, Ty> = std::collections::HashMap::new();
    for chain in &g.flow {
        for step in chain {
            if step.nodes.len() == 1 {
                let name = step.nodes[0].as_str();
                if name == "in" || name == "out" {
                    continue;
                }
                if let Some(scheme) = env.get(name).cloned() {
                    if let Ty::Fun(_, ret) = instantiate(&scheme, &mut gen) {
                        node_outputs.insert(name.to_string(), *ret);
                    }
                }
            }
        }
    }

    for chain in &g.flow {
        if chain.is_empty() {
            continue;
        }
        let mut cur: Option<Ty> = None;
        let mut prev_name = String::new();
        for step in chain {
            let step_label = step
                .nodes
                .iter()
                .map(|n| n.as_str())
                .collect::<Vec<_>>()
                .join(",");
            let (expected_input, position_output): (Option<Ty>, Option<Ty>) = if step.nodes.len()
                == 1
            {
                let name = step.nodes[0].as_str();
                if name == "in" {
                    (None, Some(in_ty.clone()))
                } else if name == "out" {
                    (Some(out_ty.clone()), Some(out_ty.clone()))
                } else {
                    let scheme = match env.get(name).cloned() {
                        Some(s) => s,
                        None => {
                            errs.push(TypeError {
                                msg: format!("unknown node in flow: {}", name),
                                span: step.span.clone(),
                                kind: TypeErrorKind::UnknownName(name.to_string()),
                            });
                            cur = None;
                            continue;
                        }
                    };
                    match instantiate(&scheme, &mut gen) {
                        Ty::Fun(args, ret) => {
                            let input = if args.len() == 1 {
                                args.into_iter().next().unwrap()
                            } else {
                                Ty::Tuple(args)
                            };
                            (Some(input), Some(*ret))
                        }
                        other => {
                            errs.push(TypeError {
                                msg: format!("flow step `{}` is not a function: {}", name, other),
                                span: step.span.clone(),
                                kind: TypeErrorKind::Arity {
                                    name: name.into(),
                                    expected: 1,
                                    found: 0,
                                },
                            });
                            cur = None;
                            continue;
                        }
                    }
                }
            } else {
                let mut outs = Vec::with_capacity(step.nodes.len());
                let mut ok = true;
                for n in &step.nodes {
                    let nm = n.as_str();
                    if nm == "in" {
                        outs.push(in_ty.clone());
                        continue;
                    }
                    if nm == "out" {
                        outs.push(out_ty.clone());
                        continue;
                    }
                    if let Some(ty) = node_outputs.get(nm).cloned() {
                        outs.push(ty);
                        continue;
                    }
                    match env.get(nm).cloned() {
                        Some(scheme) => match instantiate(&scheme, &mut gen) {
                            Ty::Fun(_, ret) => outs.push(*ret),
                            other => {
                                errs.push(TypeError {
                                    msg: format!(
                                        "flow tuple source `{}` is not a function: {}",
                                        nm, other
                                    ),
                                    span: step.span.clone(),
                                    kind: TypeErrorKind::Arity {
                                        name: nm.into(),
                                        expected: 0,
                                        found: 0,
                                    },
                                });
                                ok = false;
                                break;
                            }
                        },
                        None => {
                            errs.push(TypeError {
                                msg: format!("unknown node in flow tuple: {}", nm),
                                span: step.span.clone(),
                                kind: TypeErrorKind::UnknownName(nm.into()),
                            });
                            ok = false;
                            break;
                        }
                    }
                }
                if !ok {
                    cur = None;
                    continue;
                }
                let flattened = flatten_world_tuple(&outs);
                (None, Some(flattened))
            };
            if let (Some(c), Some(expected)) = (&cur, &expected_input) {
                if unify(&subst.apply(c), &subst.apply(expected)).is_err() {
                    errs.push(TypeError {
                        msg: format!(
                            "flow step `{}` expects {} from `{}` but receives {}",
                            step_label, expected, prev_name, c,
                        ),
                        span: step.span.clone(),
                        kind: TypeErrorKind::FlowMismatch {
                            left: prev_name.clone(),
                            right: step_label.clone(),
                            want: Box::new(expected.clone()),
                            got: Box::new(c.clone()),
                        },
                    });
                } else {
                    let s = unify(&subst.apply(c), &subst.apply(expected)).unwrap();
                    subst = s.compose(&subst);
                }
            }
            cur = position_output.map(|t| subst.apply(&t));
            prev_name = step_label;
        }
    }

    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

fn infer(
    e: &Expr,
    env: &Env,
    subst: &mut Subst,
    gen: &mut TyGen,
    types: &mut TypeMap,
) -> Result<Ty, TypeError> {
    let span = e.span();
    let ty = match e {
        Expr::Lit(l, _) => lit_ty(l),
        Expr::Var(id, _) => match env.get(id.as_str()) {
            Some(s) => instantiate(s, gen),
            None => {
                return Err(TypeError {
                    msg: format!("unknown name `{}`", id),
                    span: span.clone(),
                    kind: TypeErrorKind::UnknownName(id.as_str().to_string()),
                })
            }
        },
        Expr::Field(inner, field, _) => {
            let t = infer(inner, env, subst, gen, types)?;
            let t = subst.apply(&t);
            match &t {
                Ty::Tuple(elts) => {
                    let f = field.as_str();
                    if let Ok(i) = f.parse::<usize>() {
                        if i < elts.len() {
                            elts[i].clone()
                        } else {
                            return Err(TypeError {
                                msg: format!("tuple index {} out of range for {}", i, t),
                                span: span.clone(),
                                kind: TypeErrorKind::BadIndex {
                                    ty: t.clone(),
                                    index: f.to_string(),
                                },
                            });
                        }
                    } else {
                        return Err(TypeError {
                            msg: format!("named field access `{}` not supported on tuple", f),
                            span: span.clone(),
                            kind: TypeErrorKind::BadIndex {
                                ty: t.clone(),
                                index: f.to_string(),
                            },
                        });
                    }
                }
                _ => {
                    return Err(TypeError {
                        msg: format!("field access on non-tuple value of type {}", t),
                        span: span.clone(),
                        kind: TypeErrorKind::NotATuple(t),
                    })
                }
            }
        }
        Expr::Apply(name, args, _) => {
            let scheme = match env.get(name.as_str()) {
                Some(s) => s.clone(),
                None => {
                    return Err(TypeError {
                        msg: format!("unknown function `{}`", name),
                        span: span.clone(),
                        kind: TypeErrorKind::UnknownName(name.as_str().to_string()),
                    })
                }
            };
            let inst = instantiate(&scheme, gen);
            let (f_args, f_ret) = match inst {
                Ty::Fun(a, r) => (a, *r),
                other => {
                    return Err(TypeError {
                        msg: format!("`{}` is not a function: {}", name, other),
                        span: span.clone(),
                        kind: TypeErrorKind::Arity {
                            name: name.as_str().to_string(),
                            expected: args.len(),
                            found: 0,
                        },
                    })
                }
            };
            if f_args.len() != args.len() {
                return Err(TypeError {
                    msg: format!(
                        "`{}` expects {} args, got {}",
                        name,
                        f_args.len(),
                        args.len()
                    ),
                    span: span.clone(),
                    kind: TypeErrorKind::Arity {
                        name: name.as_str().to_string(),
                        expected: f_args.len(),
                        found: args.len(),
                    },
                });
            }
            let lam_slot = lambda_slot(name.as_str());
            for (i, a) in args.iter().enumerate() {
                let expected = subst.apply(&f_args[i]);
                if Some(i) == lam_slot {
                    if let Expr::Lam(params, body, _) = a {
                        if let Ty::Fun(lam_args, lam_ret) = &expected {
                            if params.len() != lam_args.len() {
                                return Err(TypeError {
                                    msg: format!(
                                        "lambda arity {} does not match expected {}",
                                        params.len(),
                                        lam_args.len()
                                    ),
                                    span: a.span(),
                                    kind: TypeErrorKind::Arity {
                                        name: format!("lambda for {}", name),
                                        expected: lam_args.len(),
                                        found: params.len(),
                                    },
                                });
                            }
                            let mut inner = env.clone();
                            for (pname, pty) in params.iter().zip(lam_args.iter()) {
                                inner.insert(pname.as_str().to_string(), Scheme::mono(pty.clone()));
                            }
                            let body_ty = infer(body, &inner, subst, gen, types)?;
                            let s = unify_or_err(
                                &subst.apply(&body_ty),
                                &subst.apply(lam_ret),
                                a.span(),
                            )?;
                            *subst = s.compose(subst);
                            continue;
                        }
                    }
                    if let Expr::Var(fname, fspan) = a {
                        if let Some(sc) = env.get(fname.as_str()) {
                            let t = instantiate(&sc.clone(), gen);
                            types.insert(fspan.clone(), t.clone());
                            let s = unify_or_err(&t, &expected, fspan.clone())?;
                            *subst = s.compose(subst);
                            continue;
                        }
                    }
                }
                let at = infer(a, env, subst, gen, types)?;
                let s = unify_or_err(&subst.apply(&at), &expected, a.span())?;
                *subst = s.compose(subst);
            }
            subst.apply(&f_ret)
        }
        Expr::Bin(op, l, r, _) => {
            let lt = infer(l, env, subst, gen, types)?;
            let rt = infer(r, env, subst, gen, types)?;
            match op {
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    let s = unify_or_err(&subst.apply(&lt), &subst.apply(&rt), span.clone())?;
                    *subst = s.compose(subst);
                    Ty::Base("Bool".into())
                }
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                    let s = unify_or_err(&subst.apply(&lt), &Ty::Base("Int".into()), l.span())?;
                    *subst = s.compose(subst);
                    let s = unify_or_err(&subst.apply(&rt), &Ty::Base("Int".into()), r.span())?;
                    *subst = s.compose(subst);
                    Ty::Base("Int".into())
                }
                BinOp::And | BinOp::Or => {
                    let s = unify_or_err(&subst.apply(&lt), &Ty::Base("Bool".into()), l.span())?;
                    *subst = s.compose(subst);
                    let s = unify_or_err(&subst.apply(&rt), &Ty::Base("Bool".into()), r.span())?;
                    *subst = s.compose(subst);
                    Ty::Base("Bool".into())
                }
                BinOp::Subset | BinOp::Union | BinOp::Intersect => {
                    let s = unify_or_err(&subst.apply(&lt), &subst.apply(&rt), span.clone())?;
                    *subst = s.compose(subst);
                    match op {
                        BinOp::Subset => Ty::Base("Bool".into()),
                        _ => subst.apply(&lt),
                    }
                }
            }
        }
        Expr::Un(op, e, _) => {
            let t = infer(e, env, subst, gen, types)?;
            match op {
                UnOp::Not => {
                    let s = unify_or_err(&subst.apply(&t), &Ty::Base("Bool".into()), e.span())?;
                    *subst = s.compose(subst);
                    Ty::Base("Bool".into())
                }
                UnOp::Neg => {
                    let s = unify_or_err(&subst.apply(&t), &Ty::Base("Int".into()), e.span())?;
                    *subst = s.compose(subst);
                    Ty::Base("Int".into())
                }
            }
        }
        Expr::Quant(Quant::Forall, x, dom, body, _)
        | Expr::Quant(Quant::Exists, x, dom, body, _) => {
            let dt = infer(dom, env, subst, gen, types)?;
            let dt = subst.apply(&dt);
            let elem = match dt {
                Ty::List(inner) => *inner,
                Ty::Var(v) => {
                    let elem = gen.fresh();
                    let want = Ty::List(Box::new(elem.clone()));
                    let s = unify_or_err(&Ty::Var(v), &want, dom.span())?;
                    *subst = s.compose(subst);
                    elem
                }
                other => {
                    return Err(TypeError {
                        msg: format!("quantifier domain must be a list, got {}", other),
                        span: dom.span(),
                        kind: TypeErrorKind::NotPredicate(other),
                    })
                }
            };
            let mut inner = env.clone();
            inner.insert(x.as_str().to_string(), Scheme::mono(elem));
            let bt = infer(body, &inner, subst, gen, types)?;
            let s = unify_or_err(&subst.apply(&bt), &Ty::Base("Bool".into()), body.span())?;
            *subst = s.compose(subst);
            Ty::Base("Bool".into())
        }
        Expr::If(c, a, b, _) => {
            let ct = infer(c, env, subst, gen, types)?;
            let s = unify_or_err(&subst.apply(&ct), &Ty::Base("Bool".into()), c.span())?;
            *subst = s.compose(subst);
            let at = infer(a, env, subst, gen, types)?;
            let bt = infer(b, env, subst, gen, types)?;
            let s = unify_or_err(&subst.apply(&at), &subst.apply(&bt), e.span())?;
            *subst = s.compose(subst);
            subst.apply(&at)
        }
        Expr::Let(x, e1, body, _) => {
            let t1 = infer(e1, env, subst, gen, types)?;
            let mut inner = env.clone();
            inner.insert(x.as_str().to_string(), Scheme::mono(subst.apply(&t1)));
            infer(body, &inner, subst, gen, types)?
        }
        Expr::Tuple(es, _) => {
            let mut ts = Vec::with_capacity(es.len());
            for e in es {
                ts.push(infer(e, env, subst, gen, types)?);
            }
            Ty::Tuple(ts)
        }
        Expr::Lam(params, body, _) => {
            let mut arg_tys = Vec::with_capacity(params.len());
            let mut inner = env.clone();
            for p in params {
                let v = gen.fresh();
                inner.insert(p.as_str().to_string(), Scheme::mono(v.clone()));
                arg_tys.push(v);
            }
            let bt = infer(body, &inner, subst, gen, types)?;
            Ty::Fun(arg_tys, Box::new(bt))
        }
    };
    let ty = subst.apply(&ty);
    types.insert(span, ty.clone());
    Ok(ty)
}

fn lit_ty(l: &Lit) -> Ty {
    match l {
        Lit::Int(_) => Ty::Base("Int".into()),
        Lit::Rat(_, _) => Ty::Base("Rat".into()),
        Lit::Bool(_) => Ty::Base("Bool".into()),
        Lit::Str(_) => Ty::Base("String".into()),
    }
}

fn unify_or_err(a: &Ty, b: &Ty, span: Span) -> Result<Subst, TypeError> {
    unify(a, b).map_err(|e| TypeError {
        msg: format!("type mismatch: {}", e),
        span,
        kind: TypeErrorKind::Unify(Box::new(e)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use apsl_core::ast::*;

    fn mk_node(
        name: &str,
        in_ty: AstType,
        ret: AstType,
        pre: Vec<Expr>,
        post: Vec<Expr>,
    ) -> Box<Node> {
        Box::new(Node {
            name: Ident::new(name),
            sig: TypeSig {
                params: vec![Param {
                    name: Ident::new("in"),
                    ty: in_ty,
                }],
                ret,
            },
            pre,
            post,
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
        })
    }

    #[test]
    fn empty_program_ok() {
        let p = Program::new();
        let tp = type_check(&p).unwrap();
        assert!(tp.types.is_empty());
    }

    #[test]
    fn type_alias_then_node() {
        let mut p = Program::new();
        p.decls.push(Decl::Type(TypeAlias {
            name: Ident::new("Email"),
            rhs: AstType::Base(Ident::new("String")),
            supertypes: vec![],
            span: Span::NONE,
        }));
        let post = vec![Expr::Apply(
            Ident::new("unique?"),
            vec![Expr::Var(Ident::new("out"), Span::NONE)],
            Span::NONE,
        )];
        p.decls.push(Decl::Node(mk_node(
            "dedupe",
            AstType::List(Box::new(AstType::Base(Ident::new("Email")))),
            AstType::List(Box::new(AstType::Base(Ident::new("Email")))),
            vec![],
            post,
        )));
        type_check(&p).unwrap();
    }

    #[test]
    fn rejects_non_bool_post() {
        let post = vec![Expr::Bin(
            BinOp::Add,
            Box::new(Expr::Var(Ident::new("out"), Span::NONE)),
            Box::new(Expr::Lit(Lit::Int(1), Span::NONE)),
            Span::NONE,
        )];
        let mut p = Program::new();
        p.decls.push(Decl::Node(mk_node(
            "bad",
            AstType::List(Box::new(AstType::Base(Ident::new("String")))),
            AstType::List(Box::new(AstType::Base(Ident::new("String")))),
            vec![],
            post,
        )));
        assert!(type_check(&p).is_err());
    }

    #[test]
    fn every_in_with_predicate_typechecks() {
        let post = vec![Expr::Apply(
            Ident::new("every"),
            vec![
                Expr::Var(Ident::new("in"), Span::NONE),
                Expr::Var(Ident::new("valid_email?"), Span::NONE),
            ],
            Span::NONE,
        )];
        let mut p = Program::new();
        p.decls.push(Decl::Node(mk_node(
            "n",
            AstType::List(Box::new(AstType::Base(Ident::new("String")))),
            AstType::Base(Ident::new("Bool")),
            vec![],
            post,
        )));
        type_check(&p).unwrap();
    }

    #[test]
    fn forall_quantifier_typechecks() {
        let body = Expr::Apply(
            Ident::new("valid_email?"),
            vec![Expr::Var(Ident::new("x"), Span::NONE)],
            Span::NONE,
        );
        let post = vec![Expr::Quant(
            Quant::Forall,
            Ident::new("x"),
            Box::new(Expr::Var(Ident::new("in"), Span::NONE)),
            Box::new(body),
            Span::NONE,
        )];
        let mut p = Program::new();
        p.decls.push(Decl::Node(mk_node(
            "n",
            AstType::List(Box::new(AstType::Base(Ident::new("String")))),
            AstType::Base(Ident::new("Bool")),
            vec![],
            post,
        )));
        type_check(&p).unwrap();
    }

    #[test]
    fn unknown_name_errors() {
        let post = vec![Expr::Var(Ident::new("nope"), Span::NONE)];
        let mut p = Program::new();
        p.decls.push(Decl::Node(mk_node(
            "n",
            AstType::Base(Ident::new("Int")),
            AstType::Base(Ident::new("Bool")),
            vec![],
            post,
        )));
        let errs = type_check(&p).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e.kind, TypeErrorKind::UnknownName(_))));
    }
}

fn flatten_world_tuple(outs: &[Ty]) -> Ty {
    let mut flat: Vec<Ty> = Vec::new();
    for t in outs {
        match t {
            Ty::Tuple(inner) if inner.first().is_some_and(is_world_type) => {
                for (i, e) in inner.iter().enumerate() {
                    if i == 0 && is_world_type(e) {
                        if !flat.iter().any(is_world_type) {
                            flat.push(e.clone());
                        }
                    } else {
                        flat.push(e.clone());
                    }
                }
            }
            _ => flat.push(t.clone()),
        }
    }
    if flat.len() == 1 {
        flat.remove(0)
    } else {
        Ty::Tuple(flat)
    }
}

fn is_world_type(t: &Ty) -> bool {
    matches!(t, Ty::Base(n) if n == "World") || matches!(t, Ty::Parameterized(n, _) if n == "World")
}

#[cfg(test)]
mod fan_in_tests {
    use super::*;

    fn base(name: &str) -> Ty {
        Ty::Base(name.to_string())
    }

    #[test]
    fn ordinary_tuple_outputs_remain_nested() {
        let left = Ty::Tuple(vec![base("A"), base("B")]);
        let right = Ty::Tuple(vec![base("C"), base("D")]);
        assert_eq!(
            flatten_world_tuple(&[left.clone(), right.clone()]),
            Ty::Tuple(vec![left, right])
        );
    }

    #[test]
    fn world_outputs_share_one_world_and_flatten_payloads() {
        let left = Ty::Tuple(vec![base("World"), base("A")]);
        let right = Ty::Tuple(vec![base("World"), base("B")]);
        assert_eq!(
            flatten_world_tuple(&[left, right]),
            Ty::Tuple(vec![base("World"), base("A"), base("B")])
        );
    }
}
