use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[cfg(test)]
use apsl_core::ast::Ident;
use apsl_core::ast::Type as AstType;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ty {
    Base(String),
    Parameterized(String, Vec<Ty>),
    List(Box<Ty>),
    Record(Vec<(String, Box<Ty>)>),
    Tuple(Vec<Ty>),
    Result(Box<Ty>),
    Fun(Vec<Ty>, Box<Ty>),
    Var(u32),
}

impl Ty {
    pub fn free_vars(&self) -> BTreeSet<u32> {
        let mut out = BTreeSet::new();
        self.collect_vars(&mut out);
        out
    }

    fn collect_vars(&self, out: &mut BTreeSet<u32>) {
        match self {
            Ty::Var(v) => {
                out.insert(*v);
            }
            Ty::Base(_) | Ty::Parameterized(..) => {}
            Ty::List(t) | Ty::Result(t) => t.collect_vars(out),
            Ty::Tuple(ts) => {
                for t in ts {
                    t.collect_vars(out);
                }
            }
            Ty::Record(fs) => {
                for (_, t) in fs {
                    t.collect_vars(out);
                }
            }
            Ty::Fun(args, ret) => {
                for a in args {
                    a.collect_vars(out);
                }
                ret.collect_vars(out);
            }
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Base(n) => f.write_str(n),
            Ty::Parameterized(n, args) => {
                write!(f, "{}<", n)?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", a)?;
                }
                write!(f, ">")
            }
            Ty::List(t) => write!(f, "{}[]", t),
            Ty::Tuple(ts) => {
                write!(f, "(")?;
                for (i, t) in ts.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", t)?;
                }
                write!(f, ")")
            }
            Ty::Record(fs) => {
                write!(f, "{{ ")?;
                for (i, (name, t)) in fs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", name, t)?;
                }
                write!(f, " }}")
            }
            Ty::Result(t) => write!(f, "{} + Err", t),
            Ty::Fun(args, ret) => {
                write!(f, "(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", a)?;
                }
                write!(f, ") -> {}", ret)
            }
            Ty::Var(v) => write!(f, "?t{}", v),
        }
    }
}

pub fn ast_type_to_ty(t: &AstType, aliases: &BTreeMap<String, AstType>) -> Ty {
    match t {
        AstType::Base(id) => {
            if let Some(rhs) = aliases.get(id.as_str()) {
                ast_type_to_ty(rhs, aliases)
            } else {
                Ty::Base(id.as_str().to_string())
            }
        }
        AstType::List(inner) => Ty::List(Box::new(ast_type_to_ty(inner, aliases))),
        AstType::Tuple(ts) => Ty::Tuple(ts.iter().map(|t| ast_type_to_ty(t, aliases)).collect()),
        AstType::Record(fs) => Ty::Record(
            fs.iter()
                .map(|(name, t)| {
                    (
                        name.as_str().to_string(),
                        Box::new(ast_type_to_ty(t, aliases)),
                    )
                })
                .collect(),
        ),
        AstType::Var(v) => Ty::Var(*v),
        AstType::Result(inner) => Ty::Result(Box::new(ast_type_to_ty(inner, aliases))),
        AstType::Parameterized(name, args) => Ty::Parameterized(
            name.as_str().to_string(),
            args.iter().map(|a| ast_type_to_ty(a, aliases)).collect(),
        ),
    }
}

#[derive(Debug, Clone)]
pub struct Scheme {
    pub vars: Vec<u32>,
    pub body: Ty,
}

impl Scheme {
    pub fn mono(t: Ty) -> Self {
        Scheme {
            vars: Vec::new(),
            body: t,
        }
    }
}

impl fmt::Display for Scheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.vars.is_empty() {
            write!(f, "{}", self.body)
        } else {
            write!(f, "forall")?;
            for v in &self.vars {
                write!(f, " ?t{}", v)?;
            }
            write!(f, ". {}", self.body)
        }
    }
}

#[derive(Debug, Clone)]
pub struct TyGen {
    next: u32,
}

impl TyGen {
    pub fn new() -> Self {
        TyGen { next: 1_000_000 }
    }
    pub fn fresh(&mut self) -> Ty {
        let v = self.next;
        self.next += 1;
        Ty::Var(v)
    }
}

impl Default for TyGen {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Subst {
    map: BTreeMap<u32, Ty>,
}

impl Subst {
    pub fn new() -> Self {
        Subst::default()
    }

    pub fn apply(&self, t: &Ty) -> Ty {
        match t {
            Ty::Var(v) => match self.map.get(v) {
                Some(sub) => self.apply(sub),
                None => Ty::Var(*v),
            },
            Ty::Base(_) => t.clone(),
            Ty::Parameterized(name, args) => {
                Ty::Parameterized(name.clone(), args.iter().map(|a| self.apply(a)).collect())
            }
            Ty::List(inner) => Ty::List(Box::new(self.apply(inner))),
            Ty::Result(inner) => Ty::Result(Box::new(self.apply(inner))),
            Ty::Tuple(ts) => Ty::Tuple(ts.iter().map(|x| self.apply(x)).collect()),
            Ty::Record(fs) => Ty::Record(
                fs.iter()
                    .map(|(n, t)| (n.clone(), Box::new(self.apply(t))))
                    .collect(),
            ),
            Ty::Fun(args, ret) => Ty::Fun(
                args.iter().map(|x| self.apply(x)).collect(),
                Box::new(self.apply(ret)),
            ),
        }
    }

    pub fn compose(&self, other: &Subst) -> Subst {
        let mut out = Subst::new();
        for (k, v) in &other.map {
            out.map.insert(*k, self.apply(v));
        }
        for (k, v) in &self.map {
            out.map.entry(*k).or_insert_with(|| v.clone());
        }
        out
    }

    pub fn insert(&mut self, v: u32, t: Ty) {
        self.map.insert(v, t);
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

pub fn unify(a: &Ty, b: &Ty) -> Result<Subst, UnifyError> {
    match (a, b) {
        (Ty::Var(x), Ty::Var(y)) if x == y => Ok(Subst::new()),
        (Ty::Var(x), other) | (other, Ty::Var(x)) => bind(*x, other),
        (Ty::Base(x), Ty::Base(y)) if x == y => Ok(Subst::new()),
        (Ty::Parameterized(na, aa), Ty::Parameterized(nb, ab))
            if na == nb && aa.len() == ab.len() =>
        {
            let mut s = Subst::new();
            for (x, y) in aa.iter().zip(ab.iter()) {
                let x2 = s.apply(x);
                let y2 = s.apply(y);
                let s2 = unify(&x2, &y2)?;
                s = s2.compose(&s);
            }
            Ok(s)
        }
        (Ty::List(x), Ty::List(y)) => unify(x, y),
        (Ty::Result(x), Ty::Result(y)) => unify(x, y),
        (Ty::Record(xfs), Ty::Record(yfs)) if xfs.len() == yfs.len() => {
            let mut s = Subst::new();
            for ((xn, xt), (yn, yt)) in xfs.iter().zip(yfs.iter()) {
                if xn != yn {
                    return Err(UnifyError::Mismatch(a.clone(), b.clone()));
                }
                let x2 = s.apply(xt);
                let y2 = s.apply(yt);
                let s2 = unify(&x2, &y2)?;
                s = s2.compose(&s);
            }
            Ok(s)
        }
        (Ty::Tuple(xs), Ty::Tuple(ys)) if xs.len() == ys.len() => {
            let mut s = Subst::new();
            for (x, y) in xs.iter().zip(ys.iter()) {
                let x2 = s.apply(x);
                let y2 = s.apply(y);
                let s2 = unify(&x2, &y2)?;
                s = s2.compose(&s);
            }
            Ok(s)
        }
        (Ty::Fun(xa, xr), Ty::Fun(ya, yr)) if xa.len() == ya.len() => {
            let mut s = Subst::new();
            for (x, y) in xa.iter().zip(ya.iter()) {
                let x2 = s.apply(x);
                let y2 = s.apply(y);
                let s2 = unify(&x2, &y2)?;
                s = s2.compose(&s);
            }
            let xr2 = s.apply(xr);
            let yr2 = s.apply(yr);
            let sr = unify(&xr2, &yr2)?;
            Ok(sr.compose(&s))
        }
        _ => Err(UnifyError::Mismatch(a.clone(), b.clone())),
    }
}

fn bind(v: u32, t: &Ty) -> Result<Subst, UnifyError> {
    if let Ty::Var(w) = t {
        if *w == v {
            return Ok(Subst::new());
        }
    }
    if t.free_vars().contains(&v) {
        return Err(UnifyError::OccursCheck(v, t.clone()));
    }
    let mut s = Subst::new();
    s.insert(v, t.clone());
    Ok(s)
}

pub fn instantiate(scheme: &Scheme, gen: &mut TyGen) -> Ty {
    if scheme.vars.is_empty() {
        return scheme.body.clone();
    }
    let mut s = Subst::new();
    for v in &scheme.vars {
        let fresh = gen.fresh();
        s.insert(*v, fresh);
    }
    s.apply(&scheme.body)
}

#[derive(Debug, Clone)]
pub enum UnifyError {
    Mismatch(Ty, Ty),
    OccursCheck(u32, Ty),
}

impl fmt::Display for UnifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnifyError::Mismatch(a, b) => write!(f, "cannot unify {} with {}", a, b),
            UnifyError::OccursCheck(v, t) => {
                write!(f, "occurs check failed: ?t{} appears in {}", v, t)
            }
        }
    }
}

pub fn parse_signature_string(_s: &str) -> Option<Scheme> {
    None
}

pub type Env = BTreeMap<String, Scheme>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unify_same_base() {
        let s = unify(&Ty::Base("Int".into()), &Ty::Base("Int".into())).unwrap();
        assert!(s.is_empty());
    }

    #[test]
    fn unify_var_binds() {
        let s = unify(&Ty::Var(1), &Ty::Base("Bool".into())).unwrap();
        assert_eq!(s.apply(&Ty::Var(1)), Ty::Base("Bool".into()));
    }

    #[test]
    fn occurs_check_catches_loop() {
        let res = unify(&Ty::Var(1), &Ty::List(Box::new(Ty::Var(1))));
        assert!(matches!(res, Err(UnifyError::OccursCheck(_, _))));
    }

    #[test]
    fn unify_list_recurses() {
        let a = Ty::List(Box::new(Ty::Var(1)));
        let b = Ty::List(Box::new(Ty::Base("String".into())));
        let s = unify(&a, &b).unwrap();
        assert_eq!(s.apply(&Ty::Var(1)), Ty::Base("String".into()));
    }

    #[test]
    fn ast_alias_resolves() {
        let mut aliases = BTreeMap::new();
        aliases.insert("Email".to_string(), AstType::Base(Ident::new("String")));
        let t = ast_type_to_ty(
            &AstType::List(Box::new(AstType::Base(Ident::new("Email")))),
            &aliases,
        );
        assert_eq!(t, Ty::List(Box::new(Ty::Base("String".into()))));
    }

    #[test]
    fn parameterized_same_args_unify() {
        let a = Ty::Parameterized("World".into(), vec![Ty::Base("Filename".into())]);
        let b = Ty::Parameterized("World".into(), vec![Ty::Base("Filename".into())]);
        let s = unify(&a, &b).unwrap();
        assert!(s.is_empty());
    }

    #[test]
    fn parameterized_diff_args_do_not_unify() {
        let a = Ty::Parameterized("World".into(), vec![Ty::Base("Filename".into())]);
        let b = Ty::Parameterized("World".into(), vec![Ty::Base("Token".into())]);
        assert!(unify(&a, &b).is_err());
    }

    #[test]
    fn parameterized_diff_name_do_not_unify() {
        let a = Ty::Parameterized("World".into(), vec![Ty::Base("X".into())]);
        let b = Ty::Parameterized("State".into(), vec![Ty::Base("X".into())]);
        assert!(unify(&a, &b).is_err());
    }
}
