use std::process::ExitCode;

use apsl_complex::{prove, NodeStatus};
use apsl_parse::parse_str;
use apsl_smt::{default_solver, discharge_node, encode::EmptyTypeOracle};
use apsl_types::type_check;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
        return ExitCode::from(2);
    }
    let (cmd, file) = match args[1].as_str() {
        "help" | "--help" | "-h" => {
            usage();
            return ExitCode::SUCCESS;
        }
        c @ ("check" | "complex" | "pred" | "explain") => {
            if args.len() < 3 {
                eprintln!("apsl-lint {}: missing <file>", c);
                return ExitCode::from(2);
            }
            (c, args[2].clone())
        }
        other => {
            eprintln!("apsl-lint: unknown subcommand `{}`", other);
            usage();
            return ExitCode::from(2);
        }
    };

    let src = match std::fs::read_to_string(&file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("apsl-lint: cannot read {}: {}", file, e);
            return ExitCode::from(2);
        }
    };
    let parsed = match parse_str(&src) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("apsl-lint: parse error\n  {}", e);
            return ExitCode::FAILURE;
        }
    };
    let prog = match apsl_link::link(&parsed, std::path::Path::new(&file), &[]) {
        Ok(result) => result.program,
        Err(e) => {
            eprintln!("{}", e);
            return ExitCode::FAILURE;
        }
    };
    let tp = match type_check(&prog) {
        Ok(t) => t,
        Err(errs) => {
            for e in errs {
                eprintln!(
                    "apsl-lint: type error at line {} col {}\n  {}",
                    e.span.line, e.span.col, e.msg
                );
            }
            return ExitCode::FAILURE;
        }
    };

    match cmd {
        "complex" => report_complex(&tp),
        "pred" => report_pred(&tp),
        "explain" => {
            if args.len() < 4 {
                eprintln!("apsl-lint explain: usage: apsl-lint explain <file> <node-name>");
                return ExitCode::from(2);
            }
            report_explain(&tp, &args[3])
        }
        "check" => {
            let a = report_complex(&tp);
            let b = report_pred(&tp);
            match (a, b) {
                (ExitCode::SUCCESS, ExitCode::SUCCESS) => ExitCode::SUCCESS,
                _ => ExitCode::FAILURE,
            }
        }
        _ => ExitCode::from(2),
    }
}

fn usage() {
    eprintln!("apsl-lint — APSL linter (O(n log n) prover + predicate discharge)\n");
    eprintln!("usage:");
    eprintln!("  apsl-lint check <file>             complexity + predicates");
    eprintln!("  apsl-lint complex <file>           complexity only");
    eprintln!("  apsl-lint pred <file>              predicate discharge only");
    eprintln!("  apsl-lint explain <file> <node>    derived cost + reasoning");
}

fn report_complex(tp: &apsl_types::TypedProgram) -> ExitCode {
    let report = prove(tp);
    let mut bad = false;
    for n in &report.per_node {
        match &n.status {
            NodeStatus::Ok => {
                println!(
                    "  ok       {:<24} derived {}",
                    n.node.as_str(),
                    summary(&n.derived)
                );
            }
            NodeStatus::Exceeds { hint, .. } => {
                println!(
                    "  REJECT   {:<24} derived {}",
                    n.node.as_str(),
                    summary(&n.derived)
                );
                for line in hint.lines() {
                    println!("           | {}", line);
                }
                bad = true;
            }
            NodeStatus::Mismatch { hint } => {
                println!(
                    "  TIGHT    {:<24} derived {} (declared was looser)",
                    n.node.as_str(),
                    summary(&n.derived)
                );
                for line in hint.lines() {
                    println!("           | {}", line);
                }
            }
        }
    }
    if bad {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn report_pred(tp: &apsl_types::TypedProgram) -> ExitCode {
    let solver = default_solver();
    let mut any_failed = false;
    for d in &tp.program.decls {
        if let apsl_core::ast::Decl::Node(n) = d {
            let r = discharge_node(n, &EmptyTypeOracle, &*solver);
            for c in &r.per_clause {
                use apsl_smt::ClauseStatus as S;
                let label = match &c.status {
                    S::Proved => "proved".to_string(),
                    S::Counterexample(model) => {
                        any_failed = true;
                        let explanation = apsl_smt::explain(n, c.clause_id, model);
                        format!("counterexample\n           | {}", explanation)
                    }
                    S::Unknown(msg) => {
                        any_failed = true;
                        format!("unknown ({})", msg)
                    }
                    S::EncodingError(msg) => {
                        any_failed = true;
                        format!("error ({})", msg)
                    }
                };
                println!("  {:<14} {} clause {}", label, n.name.as_str(), c.clause_id);
            }
        }
    }
    if any_failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn report_explain(tp: &apsl_types::TypedProgram, name: &str) -> ExitCode {
    let report = prove(tp);
    for n in &report.per_node {
        if n.node.as_str() == name {
            println!("node: {}", n.node.as_str());
            println!("derived complexity: {}", summary(&n.derived));
            println!("declared complexity: {}", summary(&n.declared));
            match &n.status {
                NodeStatus::Ok => println!("status: ok"),
                NodeStatus::Exceeds { hint, .. } => {
                    println!("status: REJECTED (exceeds O(n log n))");
                    println!("{}", hint);
                }
                NodeStatus::Mismatch { hint } => {
                    println!("status: TIGHT");
                    println!("{}", hint);
                }
            }
            return ExitCode::SUCCESS;
        }
    }
    eprintln!("apsl-lint explain: no node named `{}` in this file", name);
    ExitCode::FAILURE
}

fn summary(e: &apsl_core::ast::CxExpr) -> String {
    let pruned = prune(e);
    format!("O({})", render(&pruned))
}

fn prune(e: &apsl_core::ast::CxExpr) -> apsl_core::ast::CxExpr {
    use apsl_complex::{dominant_weight, Weight};
    use apsl_core::ast::CxExpr::*;
    match e {
        Sum(xs) => {
            let mut flat: Vec<_> = xs.iter().map(prune).collect();
            let mut next = Vec::new();
            for x in flat.drain(..) {
                if let Sum(inner) = x {
                    next.extend(inner);
                } else {
                    next.push(x);
                }
            }
            let any_nonconst = next.iter().any(|x| dominant_weight(x) > Weight::Const);
            if any_nonconst {
                next.retain(|x| dominant_weight(x) > Weight::Const);
            }
            let top = next
                .iter()
                .map(dominant_weight)
                .max()
                .unwrap_or(Weight::Const);
            next.retain(|x| dominant_weight(x) == top);
            next.sort_by_key(|x| std::cmp::Reverse(dominant_weight(x)));
            next.dedup();
            if next.is_empty() {
                Const
            } else if next.len() == 1 {
                next.remove(0)
            } else {
                Sum(next)
            }
        }
        Prod(xs) => {
            let mut flat: Vec<_> = xs.iter().map(prune).collect();
            flat.retain(|x| !matches!(x, Const));
            let mut next = Vec::new();
            for x in flat.drain(..) {
                if let Prod(inner) = x {
                    next.extend(inner);
                } else {
                    next.push(x);
                }
            }
            if next.is_empty() {
                Const
            } else if next.len() == 1 {
                next.remove(0)
            } else {
                Prod(next)
            }
        }
        Max(xs) => {
            let mut flat: Vec<_> = xs.iter().map(prune).collect();
            flat.sort_by_key(|x| std::cmp::Reverse(dominant_weight(x)));
            flat.dedup();
            if flat.len() == 1 {
                flat.remove(0)
            } else {
                Max(flat)
            }
        }
        other => other.clone(),
    }
}

fn render(e: &apsl_core::ast::CxExpr) -> String {
    use apsl_core::ast::CxExpr::*;
    match e {
        Const => "1".into(),
        Size(n) => n.as_str().to_string(),
        LogN(n) => format!("log {}", n.as_str()),
        NLogN(n) => format!("{} log {}", n.as_str(), n.as_str()),
        Sum(es) => es.iter().map(render).collect::<Vec<_>>().join(" + "),
        Prod(es) => es.iter().map(render).collect::<Vec<_>>().join(" * "),
        Max(es) => format!(
            "max({})",
            es.iter().map(render).collect::<Vec<_>>().join(", ")
        ),
    }
}

pub fn security_lint(prog: &apsl_core::ast::Program) -> Vec<String> {
    use apsl_core::ast::*;
    let mut warnings = Vec::new();

    for decl in &prog.decls {
        if let Decl::Node(node) = decl {
            let returns_token = match &node.sig.ret {
                Type::Tuple(ts) => ts
                    .iter()
                    .any(|t| matches!(t, Type::Base(id) if id.as_str() == "MintedToken")),
                Type::Base(id) => id.as_str() == "MintedToken",
                _ => false,
            };

            if returns_token && node.auth == AuthLevel::None {
                warnings.push(format!(
                    "SECURITY: node '{}' produces MintedToken but has auth=none. Tokens must require at least bearer auth.",
                    node.name
                ));
            }

            if returns_token
                && node.scope_constraint == ScopeConstraint::Any
                && node.post.is_empty()
            {
                warnings.push(format!(
                    "SECURITY: node '{}' produces MintedToken with scope=any and no postconditions. Add scope=narrowing or a narrows? postcondition.",
                    node.name
                ));
            }

            if returns_token && node.auth != AuthLevel::Passkey && node.auth != AuthLevel::None {
                warnings.push(format!(
                    "SECURITY WARNING: node '{}' mints tokens with auth={:?}. Consider auth=passkey for HITL gate.",
                    node.name, node.auth
                ));
            }
        }
    }

    warnings
}
