
use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub graph: String,
    pub target: String,
    pub gate: String,
    pub nodes_dir: PathBuf,
    pub vault_bin: PathBuf,
    pub env: HashMap<String, String>,
    pub spec: PathBuf,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            graph: String::new(),
            target: String::new(),
            gate: "local".into(),
            nodes_dir: PathBuf::from("nodes"),
            vault_bin: PathBuf::from("vault"),
            env: HashMap::new(),
            spec: PathBuf::new(),
        }
    }
}
