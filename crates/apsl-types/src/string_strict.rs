use std::collections::{BTreeMap, BTreeSet};

use apsl_core::ast::{Decl, Expr, Lit, Node, Program, StateDecl, Type};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateKind {
    Abstract,
    Fixed,
}

pub fn state_kind(state: &StateDecl) -> StateKind {
    if state.default.is_some() {
        StateKind::Fixed
    } else {
        StateKind::Abstract
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodePlacement {
    Fungible,
    Positional,
}

pub fn node_placement(node: &Node) -> NodePlacement {
    if node
        .state
        .iter()
        .any(|state| state_kind(state) == StateKind::Abstract)
    {
        NodePlacement::Positional
    } else {
        NodePlacement::Fungible
    }
}

fn check_type(ty: &Type, context: &str, errors: &mut Vec<String>) {
    match ty {
        Type::Base(name) if name.as_str() == "String" => errors.push(format!(
            "string-strict: {context}: raw `String` must be introduced through a named semantic type"
        )),
        Type::Base(_) | Type::Var(_) => {}
        Type::Parameterized(_, arguments) | Type::Tuple(arguments) => {
            for argument in arguments {
                check_type(argument, context, errors);
            }
        }
        Type::Record(fields) => {
            for (name, field_type) in fields {
                check_type(
                    field_type,
                    &format!("{context} field `{}`", name.as_str()),
                    errors,
                );
            }
        }
        Type::List(inner) => check_type(inner, context, errors),
        Type::Result(inner) => check_type(inner, context, errors),
    }
}

fn check_expr(expr: &Expr, context: &str, errors: &mut Vec<String>) {
    match expr {
        Expr::Lit(Lit::Str(_), _) => errors.push(format!(
            "string-strict: {context}: free string literals are not typed state"
        )),
        Expr::Lit(_, _) | Expr::Var(_, _) => {}
        Expr::Field(inner, _, _) | Expr::Un(_, inner, _) => {
            check_expr(inner, context, errors);
        }
        Expr::Apply(_, arguments, _) | Expr::Tuple(arguments, _) => {
            for argument in arguments {
                check_expr(argument, context, errors);
            }
        }
        Expr::Bin(_, left, right, _) => {
            check_expr(left, context, errors);
            check_expr(right, context, errors);
        }
        Expr::Quant(_, _, domain, body, _) => {
            check_expr(domain, context, errors);
            check_expr(body, context, errors);
        }
        Expr::If(condition, when_true, when_false, _) => {
            check_expr(condition, context, errors);
            check_expr(when_true, context, errors);
            check_expr(when_false, context, errors);
        }
        Expr::Let(_, value, body, _) => {
            check_expr(value, context, errors);
            check_expr(body, context, errors);
        }
        Expr::Lam(_, body, _) => check_expr(body, context, errors),
    }
}

fn resolved_base<'a>(
    ty: &'a Type,
    aliases: &BTreeMap<&str, &'a Type>,
    seen: &mut BTreeSet<&'a str>,
) -> Option<&'a str> {
    let Type::Base(name) = ty else {
        return None;
    };
    let name = name.as_str();
    if !seen.insert(name) {
        return None;
    }
    match aliases.get(name) {
        Some(rhs) => resolved_base(rhs, aliases, seen),
        None => Some(name),
    }
}

fn default_matches(default: &Lit, ty: &Type, aliases: &BTreeMap<&str, &Type>) -> bool {
    let Some(base) = resolved_base(ty, aliases, &mut BTreeSet::new()) else {
        return false;
    };
    matches!(
        (default, base),
        (Lit::Int(_), "Int")
            | (Lit::Rat(_, _), "Rat")
            | (Lit::Bool(_), "Bool")
            | (Lit::Str(_), "String")
    )
}

fn type_name(ty: &Type) -> String {
    match ty {
        Type::Base(name) => name.as_str().to_string(),
        _ => format!("{ty:?}"),
    }
}

pub fn check_state_defaults(program: &Program) -> Vec<String> {
    check_state_defaults_with_prefix(program, "state")
}

fn check_state_defaults_with_prefix(program: &Program, prefix: &str) -> Vec<String> {
    let aliases: BTreeMap<&str, &Type> = program
        .decls
        .iter()
        .filter_map(|declaration| match declaration {
            Decl::Type(alias) => Some((alias.name.as_str(), &alias.rhs)),
            _ => None,
        })
        .collect();
    let mut errors = Vec::new();
    for declaration in &program.decls {
        let (owner_kind, owner_name, states) = match declaration {
            Decl::Node(node) => ("node", node.name.as_str(), node.state.as_slice()),
            Decl::Graph(graph) => ("graph", graph.name.as_str(), graph.state.as_slice()),
            Decl::Type(_) => continue,
        };
        for state in states {
            if state
                .default
                .as_ref()
                .is_some_and(|default| !default_matches(default, &state.ty, &aliases))
            {
                errors.push(format!(
                    "{prefix}: {owner_kind} `{owner_name}` state `{}`: fixed default does not inhabit `{}`",
                    state.key.as_str(),
                    type_name(&state.ty)
                ));
            }
        }
    }
    errors
}

pub fn check_string_strict(program: &Program) -> Vec<String> {
    let mut errors = check_state_defaults_with_prefix(program, "string-strict");
    for declaration in &program.decls {
        match declaration {
            Decl::Type(alias) => {
                if !matches!(&alias.rhs, Type::Base(name) if name.as_str() == "String") {
                    check_type(
                        &alias.rhs,
                        &format!("type `{}`", alias.name.as_str()),
                        &mut errors,
                    );
                }
            }
            Decl::Node(node) => {
                for parameter in &node.sig.params {
                    check_type(
                        &parameter.ty,
                        &format!(
                            "node `{}` input `{}`",
                            node.name.as_str(),
                            parameter.name.as_str()
                        ),
                        &mut errors,
                    );
                }
                check_type(
                    &node.sig.ret,
                    &format!("node `{}` output", node.name.as_str()),
                    &mut errors,
                );
                for state in &node.state {
                    let context = format!(
                        "node `{}` state `{}`",
                        node.name.as_str(),
                        state.key.as_str()
                    );
                    check_type(&state.ty, &context, &mut errors);
                }
                for (index, clause) in node.pre.iter().enumerate() {
                    check_expr(
                        clause,
                        &format!("node `{}` pre clause {index}", node.name.as_str()),
                        &mut errors,
                    );
                }
                for (index, clause) in node.post.iter().enumerate() {
                    check_expr(
                        clause,
                        &format!("node `{}` post clause {index}", node.name.as_str()),
                        &mut errors,
                    );
                }
            }
            Decl::Graph(graph) => {
                for parameter in &graph.sig.params {
                    check_type(
                        &parameter.ty,
                        &format!(
                            "graph `{}` input `{}`",
                            graph.name.as_str(),
                            parameter.name.as_str()
                        ),
                        &mut errors,
                    );
                }
                check_type(
                    &graph.sig.ret,
                    &format!("graph `{}` output", graph.name.as_str()),
                    &mut errors,
                );
                for state in &graph.state {
                    let context = format!(
                        "graph `{}` state `{}`",
                        graph.name.as_str(),
                        state.key.as_str()
                    );
                    check_type(&state.ty, &context, &mut errors);
                }
                for (index, clause) in graph.post.iter().enumerate() {
                    check_expr(
                        clause,
                        &format!("graph `{}` post clause {index}", graph.name.as_str()),
                        &mut errors,
                    );
                }
            }
        }
    }
    errors
}
