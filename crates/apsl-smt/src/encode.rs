use std::fmt;

use apsl_core::ast::{BinOp, Expr, Ident, Lit, Node, Quant, Span, Type, UnOp};

pub trait TypeOracle {
    fn type_of(&self, span: &Span) -> Option<Type>;
}

pub struct EmptyTypeOracle;

impl TypeOracle for EmptyTypeOracle {
    fn type_of(&self, _span: &Span) -> Option<Type> {
        None
    }
}

#[derive(Debug, Clone)]
pub struct Smt2Script {
    pub text: String,
}

impl fmt::Display for Smt2Script {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

pub fn encode_vc(node: &Node, _types: &dyn TypeOracle) -> Smt2Script {
    let mut s = String::new();
    s.push_str("(set-logic ALL)\n");
    s.push_str("(declare-sort Value 0)\n");
    s.push_str("(declare-fun valid_email_p (Value) Bool)\n");
    s.push_str("(declare-fun well_formed_json_p (Value) Bool)\n");
    s.push_str("(declare-fun subset_p (Value Value) Bool)\n");
    s.push_str("(declare-fun union_p (Value Value) Value)\n");
    s.push_str("(declare-fun intersect_p (Value Value) Value)\n");
    s.push_str("(declare-fun unique_p (Value) Bool)\n");
    s.push_str("(declare-fun subseteq_p (Value Value) Bool)\n");
    s.push_str("(declare-fun len_v (Value) Int)\n");
    s.push_str("(declare-fun nth_v (Value Int) Value)\n");
    s.push_str("(assert (forall ((xs Value)) (>= (len_v xs) 0)))\n");
    s.push_str("(declare-fun field_0 (Value) Int)\n");
    s.push_str("(declare-fun field_1 (Value) Int)\n");
    s.push_str("(declare-fun field_2 (Value) Int)\n");
    s.push_str("(declare-fun field_3 (Value) Int)\n");
    s.push_str("(declare-fun field_4 (Value) Int)\n");
    s.push_str("(declare-fun tuple (Value Value) Value)\n");
    s.push_str("(declare-fun tuple (Value Value Value) Value)\n");
    s.push_str("(declare-fun tuple (Value Value Value Value) Value)\n");

    s.push_str("(declare-fun f_impl (Value) Value)\n");
    s.push_str("(declare-const |in| Value)\n");
    s.push_str("(declare-const |out| Value)\n");
    for p in &node.sig.params {
        if p.name.as_str() != "in" {
            s.push_str(&format!("(declare-const |{}| Value)\n", p.name.as_str()));
        }
    }
    s.push_str("(assert (= |out| (f_impl |in|)))\n");

    let mut pre = String::from("true");
    for p in &node.pre {
        pre = format!("(and {} {})", pre, encode_expr(p, &mut Ctx::new()));
    }

    let mut post = String::from("true");
    for p in &node.post {
        post = format!("(and {} {})", post, encode_expr(p, &mut Ctx::new()));
    }

    s.push_str(&format!("(assert (not (=> {} {})))\n", pre, post));
    s.push_str("(check-sat)\n");
    Smt2Script { text: s }
}

struct Ctx {
    fresh: u32,
}

impl Ctx {
    fn new() -> Self {
        Self { fresh: 0 }
    }
}

fn sym(id: &Ident) -> String {
    let mut s = String::with_capacity(id.as_str().len() + 2);
    s.push('|');
    s.push_str(id.as_str());
    s.push('|');
    s
}

fn pred_name(id: &Ident) -> String {
    match id.as_str() {
        "valid_email?" => "valid_email_p".into(),
        "well_formed_json?" => "well_formed_json_p".into(),
        "unique?" => "unique_p".into(),
        "subseteq?" => "subseteq_p".into(),
        other => format!("|{}|", other),
    }
}

fn pred_head(arg: &Expr, ctx: &mut Ctx) -> String {
    match arg {
        Expr::Var(id, _) => pred_name(id),
        other => encode_expr(other, ctx),
    }
}

fn encode_lit(l: &Lit) -> String {
    match l {
        Lit::Int(n) => {
            if *n < 0 {
                format!("(- {})", -n)
            } else {
                format!("{}", n)
            }
        }
        Lit::Rat(p, q) => format!("(/ {} {})", p, q),
        Lit::Bool(true) => "true".into(),
        Lit::Bool(false) => "false".into(),
        Lit::Str(s) => format!("\"{}\"", s.replace('"', "\"\"")),
    }
}

fn encode_expr(e: &Expr, ctx: &mut Ctx) -> String {
    match e {
        Expr::Lit(l, _) => encode_lit(l),
        Expr::Var(id, _) => sym(id),
        Expr::Field(e, id, _) => {
            format!("(field_{} {})", id.as_str(), encode_expr(e, ctx))
        }
        Expr::Apply(name, args, _) => {
            let n = name.as_str();
            match n {
                "len" if args.len() == 1 => format!("(len_v {})", encode_expr(&args[0], ctx)),
                "valid_email?" | "well_formed_json?" | "unique?" if args.len() == 1 => {
                    format!("({} {})", pred_name(name), encode_expr(&args[0], ctx))
                }
                "subseteq?" if args.len() == 2 => {
                    format!(
                        "({} {} {})",
                        pred_name(name),
                        encode_expr(&args[0], ctx),
                        encode_expr(&args[1], ctx)
                    )
                }
                "every" if args.len() == 2 => {
                    format!(
                        "(forall ((i Int)) (=> (and (<= 0 i) (< i (len_v {0}))) ({1} (nth_v {0} i))))",
                        encode_expr(&args[0], ctx),
                        pred_head(&args[1], ctx),
                    )
                }
                "some" if args.len() == 2 => {
                    format!(
                        "(exists ((i Int)) (and (<= 0 i) (< i (len_v {0})) ({1} (nth_v {0} i))))",
                        encode_expr(&args[0], ctx),
                        pred_head(&args[1], ctx),
                    )
                }
                _ => {
                    let mut s = String::new();
                    s.push('(');
                    s.push_str(name.as_str());
                    for a in args {
                        s.push(' ');
                        s.push_str(&encode_expr(a, ctx));
                    }
                    s.push(')');
                    s
                }
            }
        }
        Expr::Bin(op, l, r, _) => {
            let opstr = match op {
                BinOp::Eq => "=",
                BinOp::Ne => "distinct",
                BinOp::Lt => "<",
                BinOp::Le => "<=",
                BinOp::Gt => ">",
                BinOp::Ge => ">=",
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Mod => "mod",
                BinOp::And => "and",
                BinOp::Or => "or",
                BinOp::Subset => "subset_p",
                BinOp::Union => "union_p",
                BinOp::Intersect => "intersect_p",
            };
            format!(
                "({} {} {})",
                opstr,
                encode_expr(l, ctx),
                encode_expr(r, ctx)
            )
        }
        Expr::Un(op, e, _) => match op {
            UnOp::Not => format!("(not {})", encode_expr(e, ctx)),
            UnOp::Neg => format!("(- {})", encode_expr(e, ctx)),
        },
        Expr::Quant(q, x, dom, body, _) => {
            let qkw = match q {
                Quant::Forall => "forall",
                Quant::Exists => "exists",
            };
            ctx.fresh += 1;
            let idx = format!("i{}", ctx.fresh);
            let dom_str = encode_expr(dom, ctx);
            let body_str = encode_expr(body, ctx);
            format!(
                "({} (({} Int)) (=> (and (<= 0 {}) (< {} (len_v {}))) (let ((|{}| (nth_v {} {}))) {})))",
                qkw, idx, idx, idx, dom_str, x.as_str(), dom_str, idx, body_str
            )
        }
        Expr::If(c, a, b, _) => format!(
            "(ite {} {} {})",
            encode_expr(c, ctx),
            encode_expr(a, ctx),
            encode_expr(b, ctx)
        ),
        Expr::Let(x, e, body, _) => format!(
            "(let ((|{}| {})) {})",
            x.as_str(),
            encode_expr(e, ctx),
            encode_expr(body, ctx)
        ),
        Expr::Tuple(es, _) => {
            let mut s = String::from("(tuple");
            for e in es {
                s.push(' ');
                s.push_str(&encode_expr(e, ctx));
            }
            s.push(')');
            s
        }
        Expr::Lam(_, _, _) => "true".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn encode_trivial() {
        let n = trivial_node();
        let s = encode_vc(&n, &EmptyTypeOracle);
        assert!(s.text.contains("(check-sat)"));
        assert!(s.text.contains("(assert (not (=> true (and true true))))"));
    }

    #[test]
    fn encode_includes_oracle_decls() {
        let n = trivial_node();
        let s = encode_vc(&n, &EmptyTypeOracle);
        assert!(s.text.contains("valid_email_p"));
        assert!(s.text.contains("well_formed_json_p"));
    }
}
