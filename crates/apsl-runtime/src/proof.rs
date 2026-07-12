use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proof {
    pub node: String,
    pub input_hash: String,
    pub output_hash: String,
    pub duration_ms: u64,
    pub completed_at: i64,
    pub proof_hash: String,
    pub postconditions_verified: bool,
}

impl Proof {
    pub fn compute_hash(
        node: &str,
        input_hash: &str,
        output_hash: &str,
        completed_at: i64,
    ) -> String {
        use std::io::Write;
        let mut buf = Vec::new();
        let _ = write!(
            buf,
            "{}||{}||{}||{}",
            node, input_hash, output_hash, completed_at
        );
        apsl_core::hash::sha256_hex(&buf)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub graph: String,
    pub spec_hash: String,
    pub proofs: Vec<Proof>,
    pub verified: bool,
    pub total_ms: u64,
}
