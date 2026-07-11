use std::collections::BTreeMap;

use apsl_core::ast::{CxExpr, Ident};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Weight {
    Const,
    LogN,
    Size,
    NLogN,
    Polynomial,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Term {
    pub factors: BTreeMap<Ident, Weight>,
}

impl Term {
    pub fn one() -> Self {
        Term {
            factors: BTreeMap::new(),
        }
    }

    pub fn mul(a: &Term, b: &Term) -> Term {
        let mut out = a.clone();
        for (k, w) in &b.factors {
            let cur = out.factors.entry(k.clone()).or_insert(Weight::Const);
            *cur = combine_mul(*cur, *w);
        }
        out
    }

    pub fn factor(&self, v: &Ident) -> Weight {
        self.factors.get(v).copied().unwrap_or(Weight::Const)
    }
}

impl PartialOrd for Term {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Term {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let mut vars: std::collections::BTreeSet<&Ident> = self.factors.keys().collect();
        for k in other.factors.keys() {
            vars.insert(k);
        }
        for v in vars {
            let a = self.factor(v);
            let b = other.factor(v);
            match a.cmp(&b) {
                std::cmp::Ordering::Equal => continue,
                o => return o,
            }
        }
        std::cmp::Ordering::Equal
    }
}

fn combine_mul(a: Weight, b: Weight) -> Weight {
    use Weight::*;
    match (a, b) {
        (Const, x) | (x, Const) => x,
        (LogN, LogN) => LogN,
        (LogN, Size) | (Size, LogN) => NLogN,
        (LogN, NLogN) | (NLogN, LogN) => Polynomial,
        (Size, Size) => Polynomial,
        (Size, NLogN) | (NLogN, Size) => Polynomial,
        (NLogN, NLogN) => Polynomial,
        (Polynomial, _) | (_, Polynomial) => Polynomial,
    }
}

pub fn normalize(e: &CxExpr) -> Vec<Term> {
    match e {
        CxExpr::Const => vec![Term::one()],
        CxExpr::Size(v) => {
            let mut t = Term::one();
            t.factors.insert(v.clone(), Weight::Size);
            vec![t]
        }
        CxExpr::LogN(v) => {
            let mut t = Term::one();
            t.factors.insert(v.clone(), Weight::LogN);
            vec![t]
        }
        CxExpr::NLogN(v) => {
            let mut t = Term::one();
            t.factors.insert(v.clone(), Weight::NLogN);
            vec![t]
        }
        CxExpr::Sum(xs) => {
            let mut out = Vec::new();
            for x in xs {
                out.extend(normalize(x));
            }
            out
        }
        CxExpr::Prod(xs) => {
            let parts: Vec<Vec<Term>> = xs.iter().map(normalize).collect();
            let mut acc = vec![Term::one()];
            for part in parts {
                let mut next = Vec::new();
                for a in &acc {
                    for b in &part {
                        next.push(Term::mul(a, b));
                    }
                }
                acc = next;
            }
            acc
        }
        CxExpr::Max(xs) => {
            let mut out = Vec::new();
            for x in xs {
                out.extend(normalize(x));
            }
            out
        }
    }
}

pub fn dominant_term(poly: &[Term], _vars: &std::collections::BTreeSet<Ident>) -> Term {
    poly.iter().max().cloned().unwrap_or_else(Term::one)
}

pub fn dominant_weight(e: &CxExpr) -> Weight {
    let mut max = Weight::Const;
    for t in &normalize(e) {
        for w in t.factors.values() {
            if *w > max {
                max = *w;
            }
        }
    }
    max
}

pub fn exceeds_n_log_n(e: &CxExpr, vars: &std::collections::BTreeSet<Ident>) -> bool {
    let poly = normalize(e);
    for t in &poly {
        for v in vars {
            if t.factor(v) > Weight::NLogN {
                return true;
            }
        }
        for w in t.factors.values() {
            if *w > Weight::NLogN {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> Ident {
        Ident::new(s)
    }
    fn vs(names: &[&str]) -> std::collections::BTreeSet<Ident> {
        names.iter().map(|s| id(s)).collect()
    }

    #[test]
    fn const_admissible() {
        assert!(!exceeds_n_log_n(&CxExpr::Const, &vs(&["n"])));
    }

    #[test]
    fn n_admissible() {
        assert!(!exceeds_n_log_n(&CxExpr::Size(id("n")), &vs(&["n"])));
    }

    #[test]
    fn nlogn_admissible() {
        assert!(!exceeds_n_log_n(&CxExpr::NLogN(id("n")), &vs(&["n"])));
    }

    #[test]
    fn n_squared_rejected() {
        let e = CxExpr::Prod(vec![CxExpr::Size(id("n")), CxExpr::Size(id("n"))]);
        assert!(exceeds_n_log_n(&e, &vs(&["n"])));
    }

    #[test]
    fn cross_product_n_m_admissible() {
        let e = CxExpr::Prod(vec![CxExpr::Size(id("n")), CxExpr::Size(id("m"))]);
        assert!(!exceeds_n_log_n(&e, &vs(&["n", "m"])));
    }
}
