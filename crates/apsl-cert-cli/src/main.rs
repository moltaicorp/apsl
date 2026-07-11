use std::path::PathBuf;
use std::process::ExitCode;

use apsl_cert::cert::{emit, parse_cert_json, verify, ClauseProof};
use apsl_cert::key::{load_keypair, load_public, new_keypair, save_keypair};
use apsl_cert::store::{get, put};
use apsl_cert::tcb::TcbManifest;
use apsl_complex::{prove, NodeStatus};
use apsl_parse::parse_str;
use apsl_smt::{default_solver, discharge_node, encode::EmptyTypeOracle, ClauseStatus};
use apsl_types::type_check;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
        return ExitCode::from(2);
    }
    match args[1].as_str() {
        "help" | "--help" | "-h" => {
            usage();
            ExitCode::SUCCESS
        }
        "key" => key_cmd(&args[2..]),
        "emit" => emit_cmd(&args[2..]),
        "verify" => verify_cmd(&args[2..]),
        "show" => show_cmd(&args[2..]),
        other => {
            eprintln!("apsl-cert: unknown subcommand `{}`", other);
            usage();
            ExitCode::from(2)
        }
    }
}

fn usage() {
    eprintln!("apsl-cert — APSL certificate analyzer\n");
    eprintln!("usage:");
    eprintln!("  apsl-cert key new <name>             generate Ed25519 keypair");
    eprintln!(
        "  apsl-cert emit <file> --key <name>   verify file, emit signed cert per node, store"
    );
    eprintln!(
        "  apsl-cert verify <hash> --pub <name>  verify a stored cert (default: loads <name>.pub)"
    );
    eprintln!("  apsl-cert verify <hash> --key <name>  verify (alias for --pub, loads <name>.pub)");
    eprintln!("  apsl-cert show <hash>                pretty-print a stored cert");
    eprintln!();
    eprintln!("store layout: ./.apsl-store/<aa>/<bb>/<rest>.cert");
}

fn key_cmd(rest: &[String]) -> ExitCode {
    if rest.first().map(String::as_str) != Some("new") || rest.len() < 2 {
        eprintln!("apsl-cert key: usage: apsl-cert key new <name>");
        return ExitCode::from(2);
    }
    let name = &rest[1];
    let (sk, vk) = new_keypair();
    if let Err(e) = save_keypair(name, &sk) {
        eprintln!("apsl-cert key new: {}", e);
        return ExitCode::FAILURE;
    }
    println!("wrote {}.priv (chmod 600) and {}.pub", name, name);
    println!("fingerprint: {}", apsl_cert::key::fingerprint(&vk));
    ExitCode::SUCCESS
}

fn parse_named_arg<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let idx = args.iter().position(|a| a == flag)?;
    args.get(idx + 1).map(String::as_str)
}

fn emit_cmd(rest: &[String]) -> ExitCode {
    let file = match rest.first() {
        Some(s) if !s.starts_with("--") => s,
        _ => {
            eprintln!("apsl-cert emit: usage: apsl-cert emit <file> --key <name>");
            return ExitCode::from(2);
        }
    };
    let key_name = match parse_named_arg(rest, "--key") {
        Some(k) => k,
        None => {
            eprintln!("apsl-cert emit: --key <name> required");
            return ExitCode::from(2);
        }
    };
    let store_base = PathBuf::from(parse_named_arg(rest, "--store").unwrap_or(".apsl-store"));
    let impl_hash = parse_named_arg(rest, "--impl-hash");
    let impl_node = parse_named_arg(rest, "--node");

    let src = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("apsl-cert emit: cannot read {}: {}", file, e);
            return ExitCode::from(2);
        }
    };
    let parsed = match parse_str(&src) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("apsl-cert emit: parse error\n  {}", e);
            return ExitCode::FAILURE;
        }
    };
    let prog = match apsl_link::link(&parsed, std::path::Path::new(file), &[]) {
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
                eprintln!("apsl-cert emit: type error\n  {}", e);
            }
            return ExitCode::FAILURE;
        }
    };
    let cx_report = prove(&tp);
    let solver = default_solver();
    let sk_path = PathBuf::from(format!("{}.priv", key_name));
    let sk = match load_keypair(&sk_path) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("apsl-cert emit: load key: {}", e);
            return ExitCode::FAILURE;
        }
    };
    let tcb = pinned_tcb();

    let mut certificates = Vec::new();
    for d in &tp.program.decls {
        if let apsl_core::ast::Decl::Node(n) = d {
            let cx_entry = cx_report.per_node.iter().find(|r| r.node == n.name);
            let (verdict, derived) = match cx_entry {
                Some(r) => {
                    let v = match r.status {
                        NodeStatus::Ok => "ok",
                        NodeStatus::Exceeds { .. } => "exceeds",
                        NodeStatus::Mismatch { .. } => "mismatch",
                    };
                    (v.to_string(), render_cx(&r.derived))
                }
                None => ("ok".into(), "O(1)".into()),
            };
            if verdict == "exceeds" {
                eprintln!(
                    "apsl-cert emit: refusing to certify {} (complexity exceeds O(n log n))",
                    n.name
                );
                return ExitCode::FAILURE;
            }
            let dr = discharge_node(n, &EmptyTypeOracle, &*solver);
            for clause in &dr.per_clause {
                match &clause.status {
                    ClauseStatus::Proved => {}
                    ClauseStatus::Counterexample(_) => {
                        eprintln!(
                            "apsl-cert emit: refusing to certify {} clause {} (counterexample)",
                            n.name, clause.clause_id
                        );
                        return ExitCode::FAILURE;
                    }
                    ClauseStatus::Unknown(message) => {
                        eprintln!(
                            "apsl-cert emit: refusing to certify {} clause {} (unknown: {})",
                            n.name, clause.clause_id, message
                        );
                        return ExitCode::FAILURE;
                    }
                    ClauseStatus::EncodingError(message) => {
                        eprintln!(
                            "apsl-cert emit: refusing to certify {} clause {} (encoding error: {})",
                            n.name, clause.clause_id, message
                        );
                        return ExitCode::FAILURE;
                    }
                }
            }
            let proofs: Vec<ClauseProof> = dr
                .per_clause
                .iter()
                .map(|c| {
                    let (verdict, note) = match &c.status {
                        ClauseStatus::Proved => ("proved", String::new()),
                        ClauseStatus::Counterexample(_) => {
                            ("cex", "counterexample reported".into())
                        }
                        ClauseStatus::Unknown(m) => ("unknown", m.clone()),
                        ClauseStatus::EncodingError(m) => ("error", m.clone()),
                    };
                    ClauseProof {
                        clause_id: c.clause_id,
                        verdict: verdict.into(),
                        note,
                    }
                })
                .collect();
            let node_impl_hash = match impl_node {
                Some(target) if n.name.as_str() == target => impl_hash,
                Some(_) => None,
                None => impl_hash,
            };
            let cert = emit(
                n,
                node_impl_hash,
                &verdict,
                &derived,
                proofs,
                tcb.clone(),
                &sk,
            );
            certificates.push((n.name.as_str().to_string(), cert));
        }
    }
    if certificates.is_empty() {
        eprintln!("apsl-cert emit: no nodes in {}", file);
        return ExitCode::FAILURE;
    }
    for (node_name, cert) in certificates {
        let hash = match put(&cert, &store_base) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("apsl-cert emit: store: {}", e);
                return ExitCode::FAILURE;
            }
        };
        println!("{}  {}", hash, node_name);
    }
    ExitCode::SUCCESS
}

fn verify_cmd(rest: &[String]) -> ExitCode {
    let hash = match rest.first() {
        Some(s) if !s.starts_with("--") => s.clone(),
        _ => {
            eprintln!("apsl-cert verify: usage: apsl-cert verify <hash> --key <name>");
            eprintln!("                   apsl-cert verify <hash> --pub <name>");
            return ExitCode::from(2);
        }
    };

    let store_base = PathBuf::from(parse_named_arg(rest, "--store").unwrap_or(".apsl-store"));

    let cert_json = match get(&hash, &store_base) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("apsl-cert verify: {}", e);
            return ExitCode::FAILURE;
        }
    };
    let cert = match parse_cert_json(&cert_json) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("apsl-cert verify: {}", e);
            return ExitCode::FAILURE;
        }
    };

    let vk = {
        let key_name = parse_named_arg(rest, "--pub")
            .or_else(|| parse_named_arg(rest, "--key"))
            .ok_or_else(|| {
                eprintln!("apsl-cert verify: --pub <name> required");
                ExitCode::from(2)
            });
        let key_name = match key_name {
            Ok(k) => k,
            Err(code) => return code,
        };
        let pub_path = PathBuf::from(format!("{}.pub", key_name));
        match load_public(&pub_path) {
            Ok(k) => k,
            Err(e) => {
                eprintln!("apsl-cert verify: load public key: {}", e);
                return ExitCode::FAILURE;
            }
        }
    };

    let expected_tcb = pinned_tcb();
    match verify(&cert, &vk, &expected_tcb) {
        Ok(()) => {
            println!("ok: Ed25519 signature verified, TCB matches");
        }
        Err(e) => {
            eprintln!("apsl-cert verify: FAILED — {:?}", e);
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

fn show_cmd(rest: &[String]) -> ExitCode {
    let hash = match rest.first() {
        Some(s) => s,
        None => {
            eprintln!("apsl-cert show: usage: apsl-cert show <hash>");
            return ExitCode::from(2);
        }
    };
    let store_base = PathBuf::from(parse_named_arg(rest, "--store").unwrap_or(".apsl-store"));
    let bytes = match get(hash, &store_base) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("apsl-cert show: {}", e);
            return ExitCode::FAILURE;
        }
    };
    println!("{}", bytes);
    ExitCode::SUCCESS
}

fn render_cx(e: &apsl_core::ast::CxExpr) -> String {
    use apsl_complex::{dominant_weight, Weight};
    use apsl_core::ast::CxExpr::*;
    fn r(e: &apsl_core::ast::CxExpr) -> String {
        match e {
            Const => "1".into(),
            Size(n) => n.as_str().to_string(),
            LogN(n) => format!("log {}", n.as_str()),
            NLogN(n) => format!("{} log {}", n.as_str(), n.as_str()),
            Sum(es) => {
                let top = es
                    .iter()
                    .map(dominant_weight)
                    .max()
                    .unwrap_or(Weight::Const);
                let mut keep: Vec<&apsl_core::ast::CxExpr> =
                    es.iter().filter(|x| dominant_weight(x) == top).collect();
                keep.dedup_by(|a, b| std::ptr::eq(*a, *b));
                let strs: Vec<String> = keep.iter().map(|x| r(x)).collect();
                strs.join(" + ")
            }
            Prod(es) => {
                let mut strs: Vec<String> =
                    es.iter().filter(|x| !matches!(x, Const)).map(r).collect();
                if strs.is_empty() {
                    strs.push("1".into());
                }
                strs.join(" * ")
            }
            Max(es) => {
                let strs: Vec<String> = es.iter().map(r).collect();
                format!("max({})", strs.join(", "))
            }
        }
    }
    format!("O({})", r(e))
}

fn pinned_tcb() -> TcbManifest {
    let mut t = TcbManifest::default();
    t.add("apsl-complex", "self", "0.1.0");
    t.add("apsl-core", "self", "0.1.0");
    t.add("apsl-parse", "self", "0.1.0");
    t.add("apsl-smt", "self", "0.1.0");
    t.add("apsl-types", "self", "0.1.0");
    t
}
