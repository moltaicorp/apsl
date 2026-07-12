use std::fmt::Write;

pub trait Canon {
    fn write_canon(&self, out: &mut String);

    fn canon(&self) -> String {
        let mut s = String::new();
        self.write_canon(&mut s);
        s
    }
}

pub fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn write_int(out: &mut String, n: i128) {
    let _ = write!(out, "{}", n);
}

pub fn write_rat(out: &mut String, p: i128, q: u128) {
    debug_assert!(q > 0);
    let (rp, rq) = reduce(p, q);
    let mut s = String::with_capacity(8);
    let _ = write!(&mut s, "{}/{}", rp, rq);
    write_str(out, &s);
}

pub fn write_bool(out: &mut String, b: bool) {
    out.push_str(if b { "true" } else { "false" });
}

pub fn write_null(out: &mut String) {
    out.push_str("null");
}

pub struct ArrayWriter<'a> {
    out: &'a mut String,
    first: bool,
}

impl<'a> ArrayWriter<'a> {
    pub fn new(out: &'a mut String) -> Self {
        out.push('[');
        Self { out, first: true }
    }
    pub fn item<F: FnOnce(&mut String)>(&mut self, f: F) {
        if !self.first {
            self.out.push(',');
        }
        self.first = false;
        f(self.out);
    }
    pub fn finish(self) {
        self.out.push(']');
    }
}

pub struct ObjectWriter<'a> {
    out: &'a mut String,
    entries: Vec<(String, String)>,
}

impl<'a> ObjectWriter<'a> {
    pub fn new(out: &'a mut String) -> Self {
        Self {
            out,
            entries: Vec::new(),
        }
    }
    pub fn field<F: FnOnce(&mut String)>(&mut self, key: &str, f: F) {
        let mut v = String::new();
        f(&mut v);
        self.entries.push((key.to_string(), v));
    }
    pub fn finish(mut self) {
        self.out.push('{');
        self.entries.sort_by(|a, b| a.0.cmp(&b.0));
        let mut first = true;
        for (k, v) in &self.entries {
            if !first {
                self.out.push(',');
            }
            first = false;
            write_str(self.out, k);
            self.out.push(':');
            self.out.push_str(v);
        }
        self.out.push('}');
    }
}

fn reduce(p: i128, q: u128) -> (i128, u128) {
    let g = gcd(p.unsigned_abs(), q);
    if g == 0 {
        return (p, q);
    }
    (p / g as i128, q / g)
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

use crate::ast::*;

impl Canon for Ident {
    fn write_canon(&self, out: &mut String) {
        write_str(out, &self.0);
    }
}

impl Canon for Lit {
    fn write_canon(&self, out: &mut String) {
        let mut ow = ObjectWriter::new(out);
        match self {
            Lit::Int(n) => {
                ow.field("k", |o| write_str(o, "i"));
                ow.field("v", |o| write_int(o, *n));
            }
            Lit::Rat(p, q) => {
                ow.field("k", |o| write_str(o, "r"));
                ow.field("v", |o| write_rat(o, *p, *q));
            }
            Lit::Bool(b) => {
                ow.field("k", |o| write_str(o, "b"));
                ow.field("v", |o| write_bool(o, *b));
            }
            Lit::Str(s) => {
                ow.field("k", |o| write_str(o, "s"));
                ow.field("v", |o| write_str(o, s));
            }
        }
        ow.finish();
    }
}

impl Canon for Type {
    fn write_canon(&self, out: &mut String) {
        let mut ow = ObjectWriter::new(out);
        match self {
            Type::Base(id) => {
                ow.field("k", |o| write_str(o, "base"));
                ow.field("n", |o| id.write_canon(o));
            }
            Type::List(t) => {
                ow.field("k", |o| write_str(o, "list"));
                ow.field("t", |o| t.write_canon(o));
            }
            Type::Tuple(ts) => {
                ow.field("k", |o| write_str(o, "tuple"));
                ow.field("ts", |o| {
                    let mut aw = ArrayWriter::new(o);
                    for t in ts {
                        aw.item(|o2| t.write_canon(o2));
                    }
                    aw.finish();
                });
            }
            Type::Var(v) => {
                ow.field("k", |o| write_str(o, "var"));
                ow.field("i", |o| write_int(o, *v as i128));
            }
            Type::Record(fields) => {
                ow.field("k", |o| write_str(o, "record"));
                ow.field("fs", |o| {
                    let mut aw = ArrayWriter::new(o);
                    for (name, ty) in fields {
                        aw.item(|o2| {
                            let mut ow2 = ObjectWriter::new(o2);
                            ow2.field("n", |x| name.write_canon(x));
                            ow2.field("t", |x| ty.write_canon(x));
                            ow2.finish();
                        });
                    }
                    aw.finish();
                });
            }
            Type::Result(t) => {
                ow.field("k", |o| write_str(o, "result"));
                ow.field("t", |o| t.write_canon(o));
            }
            Type::Parameterized(name, args) => {
                ow.field("k", |o| write_str(o, "param"));
                ow.field("n", |o| name.write_canon(o));
                ow.field("a", |o| {
                    let mut aw = ArrayWriter::new(o);
                    for arg in args {
                        aw.item(|o2| arg.write_canon(o2));
                    }
                    aw.finish();
                });
            }
        }
        ow.finish();
    }
}

impl Canon for Param {
    fn write_canon(&self, out: &mut String) {
        let mut ow = ObjectWriter::new(out);
        ow.field("n", |o| self.name.write_canon(o));
        ow.field("t", |o| self.ty.write_canon(o));
        ow.finish();
    }
}

impl Canon for TypeSig {
    fn write_canon(&self, out: &mut String) {
        let mut ow = ObjectWriter::new(out);
        ow.field("p", |o| {
            let mut aw = ArrayWriter::new(o);
            for p in &self.params {
                aw.item(|o2| p.write_canon(o2));
            }
            aw.finish();
        });
        ow.field("r", |o| self.ret.write_canon(o));
        ow.finish();
    }
}

impl Canon for AuthLevel {
    fn write_canon(&self, out: &mut String) {
        write_str(
            out,
            match self {
                AuthLevel::None => "none",
                AuthLevel::Bearer => "bearer",
                AuthLevel::Session => "session",
                AuthLevel::Passkey => "passkey",
            },
        );
    }
}

impl Canon for ScopeConstraint {
    fn write_canon(&self, out: &mut String) {
        write_str(
            out,
            match self {
                ScopeConstraint::Any => "any",
                ScopeConstraint::Narrowing => "narrowing",
                ScopeConstraint::Admitted => "admitted",
            },
        );
    }
}

impl Canon for AuditReq {
    fn write_canon(&self, out: &mut String) {
        write_str(
            out,
            match self {
                AuditReq::None => "none",
                AuditReq::Before => "before",
                AuditReq::After => "after",
                AuditReq::Both => "both",
            },
        );
    }
}

impl Canon for BinOp {
    fn write_canon(&self, out: &mut String) {
        let s = match self {
            BinOp::Eq => "=",
            BinOp::Ne => "!=",
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
            BinOp::Subset => "subset",
            BinOp::Union => "union",
            BinOp::Intersect => "intersect",
        };
        write_str(out, s);
    }
}

impl Canon for UnOp {
    fn write_canon(&self, out: &mut String) {
        write_str(
            out,
            match self {
                UnOp::Not => "not",
                UnOp::Neg => "neg",
            },
        );
    }
}

impl Canon for Quant {
    fn write_canon(&self, out: &mut String) {
        write_str(
            out,
            match self {
                Quant::Forall => "forall",
                Quant::Exists => "exists",
            },
        );
    }
}

impl Canon for Expr {
    fn write_canon(&self, out: &mut String) {
        let mut ow = ObjectWriter::new(out);
        match self {
            Expr::Lit(l, _) => {
                ow.field("k", |o| write_str(o, "lit"));
                ow.field("v", |o| l.write_canon(o));
            }
            Expr::Var(id, _) => {
                ow.field("k", |o| write_str(o, "var"));
                ow.field("n", |o| id.write_canon(o));
            }
            Expr::Field(e, id, _) => {
                ow.field("k", |o| write_str(o, "field"));
                ow.field("e", |o| e.write_canon(o));
                ow.field("n", |o| id.write_canon(o));
            }
            Expr::Apply(id, args, _) => {
                ow.field("k", |o| write_str(o, "apply"));
                ow.field("f", |o| id.write_canon(o));
                ow.field("a", |o| {
                    let mut aw = ArrayWriter::new(o);
                    for a in args {
                        aw.item(|o2| a.write_canon(o2));
                    }
                    aw.finish();
                });
            }
            Expr::Bin(op, l, r, _) => {
                ow.field("k", |o| write_str(o, "bin"));
                ow.field("o", |o| op.write_canon(o));
                ow.field("l", |o| l.write_canon(o));
                ow.field("r", |o| r.write_canon(o));
            }
            Expr::Un(op, e, _) => {
                ow.field("k", |o| write_str(o, "un"));
                ow.field("o", |o| op.write_canon(o));
                ow.field("e", |o| e.write_canon(o));
            }
            Expr::Quant(q, id, dom, body, _) => {
                ow.field("k", |o| write_str(o, "quant"));
                ow.field("q", |o| q.write_canon(o));
                ow.field("x", |o| id.write_canon(o));
                ow.field("s", |o| dom.write_canon(o));
                ow.field("b", |o| body.write_canon(o));
            }
            Expr::If(c, a, b, _) => {
                ow.field("k", |o| write_str(o, "if"));
                ow.field("c", |o| c.write_canon(o));
                ow.field("t", |o| a.write_canon(o));
                ow.field("e", |o| b.write_canon(o));
            }
            Expr::Let(x, e, b, _) => {
                ow.field("k", |o| write_str(o, "let"));
                ow.field("x", |o| x.write_canon(o));
                ow.field("e", |o| e.write_canon(o));
                ow.field("b", |o| b.write_canon(o));
            }
            Expr::Tuple(es, _) => {
                ow.field("k", |o| write_str(o, "tuple"));
                ow.field("a", |o| {
                    let mut aw = ArrayWriter::new(o);
                    for e in es {
                        aw.item(|o2| e.write_canon(o2));
                    }
                    aw.finish();
                });
            }
            Expr::Lam(params, body, _) => {
                ow.field("k", |o| write_str(o, "lam"));
                ow.field("p", |o| {
                    let mut aw = ArrayWriter::new(o);
                    for p in params {
                        aw.item(|o2| p.write_canon(o2));
                    }
                    aw.finish();
                });
                ow.field("b", |o| body.write_canon(o));
            }
        }
        ow.finish();
    }
}

impl Canon for CxExpr {
    fn write_canon(&self, out: &mut String) {
        let mut ow = ObjectWriter::new(out);
        match self {
            CxExpr::Const => {
                ow.field("k", |o| write_str(o, "const"));
            }
            CxExpr::Size(n) => {
                ow.field("k", |o| write_str(o, "size"));
                ow.field("n", |o| n.write_canon(o));
            }
            CxExpr::NLogN(n) => {
                ow.field("k", |o| write_str(o, "nlogn"));
                ow.field("n", |o| n.write_canon(o));
            }
            CxExpr::LogN(n) => {
                ow.field("k", |o| write_str(o, "logn"));
                ow.field("n", |o| n.write_canon(o));
            }
            CxExpr::Sum(es) => write_list(&mut ow, "sum", es),
            CxExpr::Prod(es) => write_list(&mut ow, "prod", es),
            CxExpr::Max(es) => write_list(&mut ow, "max", es),
        }
        ow.finish();

        fn write_list(ow: &mut ObjectWriter<'_>, tag: &str, es: &[CxExpr]) {
            ow.field("k", |o| write_str(o, tag));
            ow.field("a", |o| {
                let mut aw = ArrayWriter::new(o);
                for e in es {
                    aw.item(|o2| e.write_canon(o2));
                }
                aw.finish();
            });
        }
    }
}

impl Canon for RuntimeClass {
    fn write_canon(&self, out: &mut String) {
        write_str(
            out,
            match self {
                RuntimeClass::Idem => "idem",
                RuntimeClass::IdemComplex => "idem-complex",
                RuntimeClass::AntiIdem => "anti-idem",
            },
        );
    }
}

impl Canon for CxSpec {
    fn write_canon(&self, out: &mut String) {
        let mut ow = ObjectWriter::new(out);
        ow.field("o", |o| self.bigo.write_canon(o));
        ow.field("c", |o| self.class.write_canon(o));
        ow.finish();
    }
}

impl Canon for Duration {
    fn write_canon(&self, out: &mut String) {
        let s = format!("{}ns", self.ns);
        write_str(out, &s);
    }
}

impl Canon for Sla {
    fn write_canon(&self, out: &mut String) {
        let mut ow = ObjectWriter::new(out);
        ow.field("d", |o| write_rat(o, self.delta.0, self.delta.1));
        ow.field("e", |o| write_rat(o, self.epsilon.0, self.epsilon.1));
        ow.field("t", |o| self.t.write_canon(o));
        ow.finish();
    }
}

impl Canon for Via {
    fn write_canon(&self, out: &mut String) {
        let mut ow = ObjectWriter::new(out);
        ow.field("tag", |o| self.tag.write_canon(o));
        ow.field("attrs", |o| {
            let mut sorted = self.attrs.clone();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            let mut aw = ArrayWriter::new(o);
            for (k, v) in &sorted {
                aw.item(|o2| {
                    let mut ow2 = ObjectWriter::new(o2);
                    ow2.field("k", |x| k.write_canon(x));
                    ow2.field("v", |x| v.write_canon(x));
                    ow2.finish();
                });
            }
            aw.finish();
        });
        ow.finish();
    }
}

impl Canon for TypeAlias {
    fn write_canon(&self, out: &mut String) {
        let mut ow = ObjectWriter::new(out);
        ow.field("k", |o| write_str(o, "type"));
        ow.field("n", |o| self.name.write_canon(o));
        ow.field("r", |o| self.rhs.write_canon(o));
        if !self.supertypes.is_empty() {
            ow.field("supertypes", |o| {
                let mut aw = ArrayWriter::new(o);
                for st in &self.supertypes {
                    aw.item(|o2| st.write_canon(o2));
                }
                aw.finish();
            });
        }
        ow.finish();
    }
}

impl Canon for StateDecl {
    fn write_canon(&self, out: &mut String) {
        let mut ow = ObjectWriter::new(out);
        ow.field("k", |o| self.key.write_canon(o));
        ow.field("t", |o| self.ty.write_canon(o));
        ow.field("d", |o| match &self.default {
            Some(l) => l.write_canon(o),
            None => write_null(o),
        });
        ow.finish();
    }
}

impl Canon for Node {
    fn write_canon(&self, out: &mut String) {
        let mut ow = ObjectWriter::new(out);
        ow.field("audit", |o| self.audit_req.write_canon(o));
        ow.field("auth", |o| self.auth.write_canon(o));
        ow.field("k", |o| write_str(o, "node"));
        ow.field("n", |o| self.name.write_canon(o));
        ow.field("s", |o| self.sig.write_canon(o));
        ow.field("pre", |o| {
            let mut aw = ArrayWriter::new(o);
            for p in &self.pre {
                aw.item(|o2| p.write_canon(o2));
            }
            aw.finish();
        });
        ow.field("post", |o| {
            let mut aw = ArrayWriter::new(o);
            for p in &self.post {
                aw.item(|o2| p.write_canon(o2));
            }
            aw.finish();
        });
        ow.field("cx", |o| self.cx.write_canon(o));
        ow.field("sla", |o| match &self.sla {
            Some(s) => s.write_canon(o),
            None => write_null(o),
        });
        ow.field("scope", |o| self.scope_constraint.write_canon(o));
        ow.field("via", |o| match &self.via {
            Some(v) => v.write_canon(o),
            None => write_null(o),
        });
        ow.field("state", |o| {
            let mut aw = ArrayWriter::new(o);
            for s in &self.state {
                aw.item(|o2| s.write_canon(o2));
            }
            aw.finish();
        });
        ow.finish();
    }
}

impl Canon for FlowStep {
    fn write_canon(&self, out: &mut String) {
        if self.nodes.len() == 1 {
            self.nodes[0].write_canon(out);
        } else {
            let mut aw = ArrayWriter::new(out);
            for n in &self.nodes {
                aw.item(|o| n.write_canon(o));
            }
            aw.finish();
        }
    }
}

impl Canon for Graph {
    fn write_canon(&self, out: &mut String) {
        let mut ow = ObjectWriter::new(out);
        ow.field("k", |o| write_str(o, "graph"));
        ow.field("n", |o| self.name.write_canon(o));
        ow.field("s", |o| self.sig.write_canon(o));
        ow.field("post", |o| {
            let mut aw = ArrayWriter::new(o);
            for p in &self.post {
                aw.item(|o2| p.write_canon(o2));
            }
            aw.finish();
        });
        ow.field("flow", |o| {
            let mut aw = ArrayWriter::new(o);
            for chain in &self.flow {
                aw.item(|o2| {
                    let mut aw2 = ArrayWriter::new(o2);
                    for f in chain {
                        aw2.item(|o3| f.write_canon(o3));
                    }
                    aw2.finish();
                });
            }
            aw.finish();
        });
        if !self.state.is_empty() {
            ow.field("state", |o| {
                let mut aw = ArrayWriter::new(o);
                for s in &self.state {
                    aw.item(|o2| s.write_canon(o2));
                }
                aw.finish();
            });
        }
        ow.finish();
    }
}

impl Canon for Decl {
    fn write_canon(&self, out: &mut String) {
        match self {
            Decl::Type(t) => t.write_canon(out),
            Decl::Node(n) => n.write_canon(out),
            Decl::Graph(g) => g.write_canon(out),
        }
    }
}

impl Canon for Program {
    fn write_canon(&self, out: &mut String) {
        let mut aw = ArrayWriter::new(out);
        for d in &self.decls {
            aw.item(|o| d.write_canon(o));
        }
        aw.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_program_canon() {
        let p = Program::new();
        assert_eq!(p.canon(), "[]");
    }

    #[test]
    fn lit_canon() {
        assert_eq!(Lit::Int(42).canon(), "{\"k\":\"i\",\"v\":42}");
        assert_eq!(Lit::Bool(true).canon(), "{\"k\":\"b\",\"v\":true}");
        assert_eq!(Lit::Rat(4, 8).canon(), "{\"k\":\"r\",\"v\":\"1/2\"}");
    }

    #[test]
    fn keys_sorted() {
        let mut s = String::new();
        let mut ow = ObjectWriter::new(&mut s);
        ow.field("zebra", |o| o.push('1'));
        ow.field("apple", |o| o.push('2'));
        ow.finish();
        assert_eq!(s, "{\"apple\":2,\"zebra\":1}");
    }
}
