use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use apsl_core::ast::Program;
use apsl_parse::parse_str;
use apsl_runtime::adapter::AdapterRegistry;
use apsl_runtime::engine::Runtime;
use apsl_runtime::manifest::Manifest;

#[derive(Parser)]
#[command(name = "apsl", about = "APSL runtime — execute graphs against reality")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Run {
        spec: PathBuf,

        #[arg(long)]
        graph: String,

        #[arg(long, default_value = "localhost")]
        target: String,

        #[arg(long, default_value = "local")]
        gate: String,

        #[arg(long, default_value = "nodes")]
        nodes_dir: PathBuf,

        #[arg(long, default_value = "vault")]
        vault_bin: PathBuf,
    },

    Nodes {
        spec: PathBuf,

        #[arg(long)]
        graph: String,
    },

    Verify {
        record: PathBuf,
    },
}

fn load_program(path: &PathBuf) -> Result<Program> {
    let src =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let prog = parse_str(&src)
        .map_err(|e| anyhow::anyhow!("parse error at {}:{}: {}", e.span.line, e.span.col, e.msg))?;

    let linked = apsl_link::link(&prog, path.as_ref(), &[])
        .map_err(|e| anyhow::anyhow!("link error: {e}"))?;

    apsl_types::type_check(&linked.program).map_err(|errs| {
        let msgs: Vec<String> = errs
            .iter()
            .map(|e| format!("  {}:{}: {}", e.span.line, e.span.col, e.msg))
            .collect();
        anyhow::anyhow!("type errors:\n{}", msgs.join("\n"))
    })?;

    Ok(linked.program)
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("apsl=info".parse()?))
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Cmd::Run {
            spec,
            graph,
            target,
            gate,
            nodes_dir,
            vault_bin,
        } => {
            eprintln!("╔══════════════════════════════════════════════════╗");
            eprintln!("║  APSL Runtime v0.1                              ║");
            eprintln!("║  Spec:   {:<39} ║", spec.display());
            eprintln!("║  Graph:  {:<39} ║", graph);
            eprintln!("║  Target: {:<39} ║", target);
            eprintln!("║  Gate:   {:<39} ║", gate);
            eprintln!("╚══════════════════════════════════════════════════╝");

            let program = load_program(&spec)?;
            let registry = AdapterRegistry::new(&nodes_dir, &vault_bin);
            let manifest = Manifest {
                graph,
                target,
                gate,
                nodes_dir,
                vault_bin,
                spec,
                env: std::env::vars().collect(),
            };

            let runtime = Runtime::new(program, registry, manifest);
            let record = runtime.run(serde_json::Value::Null)?;

            println!("{}", serde_json::to_string_pretty(&record)?);

            if record.verified {
                eprintln!(
                    "\n✓ All {} nodes executed; no postcondition obligations were declared.",
                    record.proofs.len()
                );
            } else {
                eprintln!("\n✗ Execution completed but postconditions NOT fully verified.");
                std::process::exit(1);
            }
        }

        Cmd::Nodes { spec, graph } => {
            let program = load_program(&spec)?;
            let g = program
                .decls
                .iter()
                .find_map(|d| match d {
                    apsl_core::ast::Decl::Graph(g) if g.name.as_str() == graph => Some(g),
                    _ => None,
                })
                .with_context(|| format!("graph '{}' not found", graph))?;

            let rt = Runtime::new(
                program.clone(),
                AdapterRegistry::new(&PathBuf::from("."), &PathBuf::from("vault")),
                Manifest {
                    graph: graph.clone(),
                    ..Manifest::default()
                },
            );

            for chain in &g.flow {
                for step in chain {
                    for ident in &step.nodes {
                        let name = ident.as_str();
                        if name == "in" || name == "out" {
                            continue;
                        }
                        let via = program.decls.iter().find_map(|d| match d {
                            apsl_core::ast::Decl::Node(n) if n.name.as_str() == name => {
                                n.via.as_ref().map(|v| {
                                    let attrs: Vec<String> = v
                                        .attrs
                                        .iter()
                                        .map(|(k, val)| format!("{}={}", k, val))
                                        .collect();
                                    format!("@{} {}", v.tag, attrs.join(" "))
                                })
                            }
                            _ => None,
                        });
                        println!("  {} {}", name, via.unwrap_or_default());
                    }
                }
            }
            drop(rt);
        }

        Cmd::Verify { record } => {
            let data = std::fs::read_to_string(&record)
                .with_context(|| format!("cannot read {}", record.display()))?;
            let rec: apsl_runtime::proof::ExecutionRecord =
                serde_json::from_str(&data).context("invalid execution record JSON")?;

            eprintln!("Graph:    {}", rec.graph);
            eprintln!("Spec:     {}", rec.spec_hash);
            eprintln!("Nodes:    {}", rec.proofs.len());
            eprintln!("Total:    {}ms", rec.total_ms);
            eprintln!("Verified: {}", rec.verified);

            for proof in &rec.proofs {
                let expected = apsl_runtime::proof::Proof::compute_hash(
                    &proof.node,
                    &proof.input_hash,
                    &proof.output_hash,
                    proof.completed_at,
                );
                if expected != proof.proof_hash {
                    eprintln!(
                        "✗ {} proof hash mismatch: expected {}, got {}",
                        proof.node, expected, proof.proof_hash
                    );
                    std::process::exit(1);
                }
                eprintln!("✓ {} ({}ms)", proof.node, proof.duration_ms);
            }
            eprintln!("\n✓ All proof hashes valid.");
        }
    }

    Ok(())
}
