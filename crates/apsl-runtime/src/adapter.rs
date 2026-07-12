use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct NodeInput {
    pub node_name: String,
    pub service: Option<String>,
    pub attrs: HashMap<String, String>,
    pub values: Value,
    pub env: HashMap<String, String>,
    pub target: String,
}

#[derive(Debug, Clone)]
pub struct NodeOutput {
    pub values: Value,
    pub exit_code: i32,
    pub logs: String,
}

pub trait Adapter: Send + Sync {
    fn execute(&self, input: &NodeInput) -> Result<NodeOutput>;
}

pub struct ShellAdapter {
    pub nodes_dir: PathBuf,
}

impl Adapter for ShellAdapter {
    fn execute(&self, input: &NodeInput) -> Result<NodeOutput> {
        let script = self.nodes_dir.join(&input.node_name);
        if !script.exists() {
            anyhow::bail!(
                "shell adapter: no implementation at {} for node '{}'",
                script.display(),
                input.node_name
            );
        }

        let input_json =
            serde_json::to_string(&input.values).context("shell adapter: serialize input")?;

        let mut cmd = Command::new(&script);
        cmd.env("APSL_NODE", &input.node_name);
        cmd.env("APSL_TARGET", &input.target);
        cmd.env("APSL_INPUT", &input_json);
        for (k, v) in &input.env {
            cmd.env(k, v);
        }
        for (k, v) in &input.attrs {
            cmd.env(format!("APSL_ATTR_{}", k.to_uppercase()), v);
        }

        let output = cmd
            .output()
            .with_context(|| format!("shell adapter: failed to run {}", script.display()))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let values: Value = if stdout.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(stdout.trim())
                .unwrap_or_else(|_| Value::String(stdout.trim().to_string()))
        };

        Ok(NodeOutput {
            values,
            exit_code: output.status.code().unwrap_or(-1),
            logs: stderr.to_string(),
        })
    }
}

pub struct VaultAdapter {
    pub vault_bin: PathBuf,
}

impl Adapter for VaultAdapter {
    fn execute(&self, input: &NodeInput) -> Result<NodeOutput> {
        let op = input.attrs.get("op").map(|s| s.as_str()).unwrap_or("read");

        let path = input
            .values
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let mut cmd = Command::new(&self.vault_bin);
        cmd.arg(op).arg(path);
        cmd.env("APSL_NODE", &input.node_name);

        if op == "write" {
            if let Some(val) = input.values.get("value") {
                cmd.arg("--value").arg(serde_json::to_string(val)?);
            }
        }

        let output = cmd
            .output()
            .with_context(|| format!("vault adapter: failed to run {} {}", op, path))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let values: Value = if stdout.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(stdout.trim())
                .unwrap_or_else(|_| Value::String(stdout.trim().to_string()))
        };

        Ok(NodeOutput {
            values,
            exit_code: output.status.code().unwrap_or(-1),
            logs: stderr.to_string(),
        })
    }
}

pub struct AdapterRegistry {
    adapters: HashMap<String, Box<dyn Adapter>>,
    default: Box<dyn Adapter>,
}

impl AdapterRegistry {
    pub fn with_default(default: Box<dyn Adapter>) -> Self {
        Self {
            adapters: HashMap::new(),
            default,
        }
    }

    pub fn new(nodes_dir: &Path, vault_bin: &Path) -> Self {
        let mut adapters: HashMap<String, Box<dyn Adapter>> = HashMap::new();

        adapters.insert(
            "vault_kv_v2".into(),
            Box::new(VaultAdapter {
                vault_bin: vault_bin.to_path_buf(),
            }),
        );

        Self {
            adapters,
            default: Box::new(ShellAdapter {
                nodes_dir: nodes_dir.to_path_buf(),
            }),
        }
    }

    pub fn dispatch(&self, service: Option<&str>) -> &dyn Adapter {
        match service {
            Some(svc) => self
                .adapters
                .get(svc)
                .map(|a| a.as_ref())
                .unwrap_or(&*self.default),
            None => &*self.default,
        }
    }
}
