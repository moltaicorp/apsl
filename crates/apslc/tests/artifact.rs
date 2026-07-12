use std::path::PathBuf;
use std::process::Command;

use apsl_core::hash::sha256_hex;
use serde_json::Value;

fn write_spec(source: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("apslc-artifact-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("contract.apsl");
    std::fs::write(&path, source).unwrap();
    path
}

#[test]
fn compile_emits_exact_canonical_artifact_bytes() {
    let path = write_spec(include_str!(
        "../../apsl-artifact/tests/fixtures/compiled-graph-types-v1.apsl"
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_apslc"))
        .args([
            "compile",
            path.to_str().unwrap(),
            "--state",
            "--string-strict",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let canonical = std::str::from_utf8(&output.stdout).expect("artifact is UTF-8");
    let document: Value = serde_json::from_str(canonical).expect("artifact is canonical JSON");
    assert_eq!(document["schema"], "apsl.compiled-graph-types.v1");
    assert_eq!(
        document["checks"],
        serde_json::json!(["state", "string-strict", "types"])
    );
    assert_eq!(sha256_hex(canonical.as_bytes()).len(), 64);

    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn compile_rejects_unknown_strictness_flags() {
    let path = write_spec("identity : Int -> Int\n  cx O(1) idem\n");
    let output = Command::new(env!("CARGO_BIN_EXE_apslc"))
        .args(["compile", path.to_str().unwrap(), "--string-strct"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown flag `--string-strct`"));

    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
