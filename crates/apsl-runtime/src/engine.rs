use std::collections::HashMap;
use std::time::Instant;

use anyhow::{Context, Result};
use apsl_core::ast::{Decl, Graph, Node, Program};
use serde_json::Value;

use crate::adapter::{AdapterRegistry, NodeInput, NodeOutput};
use crate::manifest::Manifest;
use crate::proof::{ExecutionRecord, Proof};

pub struct Runtime {
    program: Program,
    registry: AdapterRegistry,
    manifest: Manifest,
}

impl Runtime {
    pub fn new(program: Program, registry: AdapterRegistry, manifest: Manifest) -> Self {
        Self {
            program,
            registry,
            manifest,
        }
    }

    fn find_graph(&self, name: &str) -> Option<&Graph> {
        self.program.decls.iter().find_map(|d| match d {
            Decl::Graph(g) if g.name.as_str() == name => Some(g),
            _ => None,
        })
    }

    fn find_node(&self, name: &str) -> Option<&Node> {
        self.program.decls.iter().find_map(|d| match d {
            Decl::Node(n) if n.name.as_str() == name => Some(n.as_ref()),
            _ => None,
        })
    }

    fn linearize(&self, graph: &Graph) -> Vec<String> {
        let mut nodes = Vec::new();
        for chain in &graph.flow {
            for step in chain {
                for ident in &step.nodes {
                    let name = ident.as_str();
                    if name != "in" && name != "out" && !nodes.contains(&name.to_string()) {
                        nodes.push(name.to_string());
                    }
                }
            }
        }
        nodes
    }

    pub fn run(&self, initial_input: Value) -> Result<ExecutionRecord> {
        let graph = self
            .find_graph(&self.manifest.graph)
            .with_context(|| format!("graph '{}' not found in program", self.manifest.graph))?
            .clone();

        let spec_hash = {
            let canon = apsl_core::Canon::canon(&self.program);
            apsl_core::hash::sha256_hex(canon.as_bytes())
        };

        let node_order = self.linearize(&graph);
        let mut proofs: Vec<Proof> = Vec::new();
        let run_start = Instant::now();
        let mut current_value = initial_input;

        for node_name in &node_order {
            let node = self.find_node(node_name).with_context(|| {
                format!("node '{}' referenced in graph but not declared", node_name)
            })?;

            let upstream_value = current_value.clone();

            let (service, attrs) = match &node.via {
                Some(via) => {
                    let svc = via
                        .attrs
                        .iter()
                        .find(|(k, _)| k.as_str() == "service")
                        .map(|(_, v)| v.as_str().to_string());
                    let attrs: HashMap<String, String> = via
                        .attrs
                        .iter()
                        .map(|(k, v)| (k.as_str().to_string(), v.as_str().to_string()))
                        .collect();
                    (svc, attrs)
                }
                None => (None, HashMap::new()),
            };

            let input = NodeInput {
                node_name: node_name.clone(),
                service: service.clone(),
                attrs,
                values: upstream_value,
                env: self.manifest.env.clone(),
                target: self.manifest.target.clone(),
            };

            let adapter = self.registry.dispatch(service.as_deref());
            let node_start = Instant::now();

            tracing::info!(node = %node_name, service = ?service, "executing");

            let result: NodeOutput = adapter
                .execute(&input)
                .with_context(|| format!("node '{}' execution failed", node_name))?;

            let duration_ms = node_start.elapsed().as_millis() as u64;

            if result.exit_code != 0 {
                anyhow::bail!(
                    "node '{}' failed with exit code {}\nlogs: {}",
                    node_name,
                    result.exit_code,
                    result.logs
                );
            }

            if let Some(sla) = &node.sla {
                let t_ns = sla.t.ns;
                let actual_ns = duration_ms as u128 * 1_000_000;
                if actual_ns > t_ns {
                    tracing::warn!(
                        node = %node_name,
                        sla_ms = t_ns / 1_000_000,
                        actual_ms = duration_ms,
                        "SLA breach"
                    );
                }
            }

            let input_hash =
                apsl_core::hash::sha256_hex(serde_json::to_string(&input.values)?.as_bytes());
            let output_hash =
                apsl_core::hash::sha256_hex(serde_json::to_string(&result.values)?.as_bytes());
            let completed_at = chrono::Utc::now().timestamp();

            let proof = Proof {
                node: node_name.clone(),
                input_hash: input_hash.clone(),
                output_hash: output_hash.clone(),
                duration_ms,
                completed_at,
                proof_hash: Proof::compute_hash(node_name, &input_hash, &output_hash, completed_at),
                postconditions_verified: node.post.is_empty(),
            };

            tracing::info!(
                node = %node_name,
                duration_ms,
                proof_hash = %proof.proof_hash,
                "completed"
            );

            if !result.logs.is_empty() {
                tracing::debug!(node = %node_name, logs = %result.logs);
            }

            current_value = result.values;
            proofs.push(proof);
        }

        let total_ms = run_start.elapsed().as_millis() as u64;
        let verified = proofs.iter().all(|p| p.postconditions_verified);

        Ok(ExecutionRecord {
            graph: self.manifest.graph.clone(),
            spec_hash,
            proofs,
            verified,
            total_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use apsl_core::ast::Decl;
    use serde_json::json;

    use crate::adapter::{Adapter, AdapterRegistry, NodeInput, NodeOutput};
    use crate::manifest::Manifest;

    struct Succeed;

    impl Adapter for Succeed {
        fn execute(&self, input: &NodeInput) -> anyhow::Result<NodeOutput> {
            Ok(NodeOutput {
                values: input.values.clone(),
                exit_code: 0,
                logs: String::new(),
            })
        }
    }

    fn postconditions_verified(source: &str) -> bool {
        let program = apsl_parse::parse_str(source).unwrap();
        let node = program
            .decls
            .iter()
            .find_map(|decl| match decl {
                Decl::Node(node) => Some(node),
                _ => None,
            })
            .unwrap();
        node.post.is_empty()
    }

    #[test]
    fn absent_postcondition_is_not_an_obligation() {
        assert!(postconditions_verified("n : Int -> Int\n  cx O(1) idem\n"));
    }

    #[test]
    fn postcondition_requires_external_evidence() {
        assert!(!postconditions_verified(
            "n : Int -> Int\n  post out = in\n  cx O(1) idem\n"
        ));
    }

    #[test]
    fn successful_execution_with_postcondition_fails_closed() {
        let source = "n : Int -> Int\n  post true\n  cx O(1) idem\n\ngraph g : Int -> Int\n  flow in -> n -> out\n";
        let program = apsl_parse::parse_str(source).unwrap();
        let runtime = super::Runtime::new(
            program,
            AdapterRegistry::with_default(Box::new(Succeed)),
            Manifest {
                graph: "g".into(),
                ..Manifest::default()
            },
        );
        let record = runtime.run(json!(1)).unwrap();
        assert!(!record.verified);
        assert!(!record.proofs[0].postconditions_verified);
    }
}
