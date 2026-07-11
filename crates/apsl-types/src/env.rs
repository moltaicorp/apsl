use std::collections::BTreeMap;

use crate::types::{Scheme, Ty};

const A: u32 = 1;
const B: u32 = 2;
const K: u32 = 3;

fn a() -> Ty {
    Ty::Var(A)
}
fn b() -> Ty {
    Ty::Var(B)
}
fn k() -> Ty {
    Ty::Var(K)
}

fn int() -> Ty {
    Ty::Base("Int".into())
}
fn bool_t() -> Ty {
    Ty::Base("Bool".into())
}
fn string_t() -> Ty {
    Ty::Base("String".into())
}

fn list(t: Ty) -> Ty {
    Ty::List(Box::new(t))
}
fn result(t: Ty) -> Ty {
    Ty::Result(Box::new(t))
}
fn fun(args: Vec<Ty>, ret: Ty) -> Ty {
    Ty::Fun(args, Box::new(ret))
}

fn forall(vars: &[u32], body: Ty) -> Scheme {
    Scheme {
        vars: vars.to_vec(),
        body,
    }
}

pub fn primitives() -> BTreeMap<String, Scheme> {
    let mut m = BTreeMap::new();

    m.insert("+".into(), Scheme::mono(fun(vec![int(), int()], int())));
    m.insert("-".into(), Scheme::mono(fun(vec![int(), int()], int())));
    m.insert("*".into(), Scheme::mono(fun(vec![int(), int()], int())));
    m.insert(
        "div".into(),
        Scheme::mono(fun(vec![int(), int()], result(int()))),
    );
    m.insert(
        "mod".into(),
        Scheme::mono(fun(vec![int(), int()], result(int()))),
    );

    for op in ["=", "!=", "<", "<=", ">", ">="] {
        m.insert(op.into(), forall(&[A], fun(vec![a(), a()], bool_t())));
    }

    m.insert(
        "and".into(),
        Scheme::mono(fun(vec![bool_t(), bool_t()], bool_t())),
    );
    m.insert(
        "or".into(),
        Scheme::mono(fun(vec![bool_t(), bool_t()], bool_t())),
    );
    m.insert("not".into(), Scheme::mono(fun(vec![bool_t()], bool_t())));

    m.insert("nil".into(), forall(&[A], fun(vec![], list(a()))));
    m.insert(
        "cons".into(),
        forall(&[A], fun(vec![a(), list(a())], list(a()))),
    );
    m.insert("len".into(), forall(&[A], fun(vec![list(a())], int())));
    m.insert(
        "nth".into(),
        forall(&[A], fun(vec![list(a()), int()], result(a()))),
    );
    m.insert(
        "head".into(),
        forall(&[A], fun(vec![list(a())], result(a()))),
    );
    m.insert(
        "tail".into(),
        forall(&[A], fun(vec![list(a())], result(list(a())))),
    );
    m.insert(
        "range".into(),
        Scheme::mono(fun(vec![int(), int()], list(int()))),
    );

    m.insert(
        "map".into(),
        forall(
            &[A, B],
            fun(vec![fun(vec![a()], b()), list(a())], list(b())),
        ),
    );
    m.insert(
        "filter".into(),
        forall(
            &[A],
            fun(vec![fun(vec![a()], bool_t()), list(a())], list(a())),
        ),
    );
    m.insert(
        "fold".into(),
        forall(
            &[A, B],
            fun(vec![fun(vec![b(), a()], b()), b(), list(a())], b()),
        ),
    );
    m.insert("sort".into(), forall(&[A], fun(vec![list(a())], list(a()))));
    m.insert(
        "sort_by".into(),
        forall(
            &[A, B],
            fun(vec![list(a()), fun(vec![a()], b())], list(a())),
        ),
    );
    m.insert(
        "every".into(),
        forall(
            &[A],
            fun(vec![list(a()), fun(vec![a()], bool_t())], bool_t()),
        ),
    );
    m.insert(
        "some".into(),
        forall(
            &[A],
            fun(vec![list(a()), fun(vec![a()], bool_t())], bool_t()),
        ),
    );
    m.insert(
        "count".into(),
        forall(&[A], fun(vec![list(a()), fun(vec![a()], bool_t())], int())),
    );
    m.insert(
        "group_by".into(),
        forall(
            &[A, K],
            fun(
                vec![list(a()), fun(vec![a()], k())],
                list(Ty::Tuple(vec![k(), list(a())])),
            ),
        ),
    );
    m.insert(
        "unique?".into(),
        forall(&[A], fun(vec![list(a())], bool_t())),
    );
    m.insert(
        "subseteq?".into(),
        forall(&[A], fun(vec![list(a()), list(a())], bool_t())),
    );
    m.insert(
        "dedupe".into(),
        forall(&[A], fun(vec![list(a())], list(a()))),
    );
    m.insert(
        "concat".into(),
        forall(&[A], fun(vec![list(a()), list(a())], list(a()))),
    );
    m.insert(
        "reverse".into(),
        forall(&[A], fun(vec![list(a())], list(a()))),
    );
    m.insert(
        "zip".into(),
        forall(
            &[A, B],
            fun(vec![list(a()), list(b())], list(Ty::Tuple(vec![a(), b()]))),
        ),
    );

    m.insert(
        "valid_email?".into(),
        Scheme::mono(fun(vec![string_t()], bool_t())),
    );
    m.insert(
        "well_formed_json?".into(),
        Scheme::mono(fun(vec![string_t()], bool_t())),
    );

    m
}

pub fn is_primitive(name: &str) -> bool {
    primitives().contains_key(name)
}

pub fn lambda_slot(name: &str) -> Option<usize> {
    match name {
        "map" | "filter" => Some(0),
        "fold" => Some(0),
        "every" | "some" | "count" | "group_by" | "sort_by" => Some(1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plus_is_int_int_int() {
        let env = primitives();
        let s = env.get("+").unwrap();
        assert!(s.vars.is_empty());
        match &s.body {
            Ty::Fun(args, ret) => {
                assert_eq!(args.len(), 2);
                assert_eq!(args[0], int());
                assert_eq!(args[1], int());
                assert_eq!(**ret, int());
            }
            other => panic!("expected Fun, got {:?}", other),
        }
    }

    #[test]
    fn valid_email_is_string_bool() {
        let env = primitives();
        let s = env.get("valid_email?").unwrap();
        match &s.body {
            Ty::Fun(args, ret) => {
                assert_eq!(args[0], string_t());
                assert_eq!(**ret, bool_t());
            }
            _ => panic!("not a function"),
        }
    }

    #[test]
    fn every_takes_list_and_pred() {
        let env = primitives();
        let s = env.get("every").unwrap();
        assert_eq!(s.vars.len(), 1);
        match &s.body {
            Ty::Fun(args, ret) => {
                assert_eq!(args.len(), 2);
                assert_eq!(**ret, bool_t());
            }
            _ => panic!("not fun"),
        }
    }
}
