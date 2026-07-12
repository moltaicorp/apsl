use std::path::PathBuf;
use std::process::Command;

fn write_spec(name: &str, source: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "apslc-string-strict-{}-{name}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("contract.apsl");
    std::fs::write(&path, source).unwrap();
    path
}

#[test]
fn rejects_raw_string_signature() {
    let path = write_spec(
        "raw-signature",
        "identity : String -> String\n  cx O(1) idem\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_apslc"))
        .args(["check", path.to_str().unwrap(), "--string-strict"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(
            "string-strict: node `identity` input `in`: raw `String` must be introduced through a named semantic type"
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn accepts_named_string_alias_in_owned_state() {
    let path = write_spec(
        "unclassified-state",
        "type Endpoint = String\n\nserver : Int -> Int\n  state origin : Endpoint\n  cx O(1) idem\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_apslc"))
        .args(["check", path.to_str().unwrap(), "--string-strict"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn preserves_same_named_state_at_each_node_position() {
    let path = write_spec(
        "positional-state",
        "type Endpoint = String\n\nleft : Int -> Int\n  state origin : Endpoint\n  cx O(1) idem\n\nright : Int -> Int\n  state origin : Endpoint\n  cx O(1) idem\n\ngraph pipeline : Int -> Int\n  flow in -> left -> right -> out\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_apslc"))
        .args([
            "check",
            path.to_str().unwrap(),
            "--string-strict",
            "--state",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "state ownership must remain positional: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn rejects_raw_string_state_type() {
    let path = write_spec(
        "raw-state",
        "server : Int -> Int\n  state origin : String\n  cx O(1) idem\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_apslc"))
        .args(["check", path.to_str().unwrap(), "--string-strict"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(
            "string-strict: node `server` state `origin`: raw `String` must be introduced through a named semantic type"
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn rejects_raw_string_nested_in_semantic_type() {
    let path = write_spec(
        "raw-field",
        "type User = { name: String }\n\nidentity : User -> User\n  cx O(1) idem\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_apslc"))
        .args(["check", path.to_str().unwrap(), "--string-strict"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(
            "string-strict: type `User` field `name`: raw `String` must be introduced through a named semantic type"
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn rejects_free_string_literal_in_predicate() {
    let path = write_spec(
        "literal-predicate",
        "type Label = String\n\nidentity : Label -> Label\n  post out = \"fixed\"\n  cx O(1) idem\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_apslc"))
        .args(["check", path.to_str().unwrap(), "--string-strict"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(
            "string-strict: node `identity` post clause 0: free string literals are not typed state"
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn rejects_fixed_state_default_with_wrong_type() {
    let path = write_spec(
        "wrong-default",
        "type Endpoint = String\n\nserver : Int -> Int\n  state origin : Endpoint = 42\n  cx O(1) idem\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_apslc"))
        .args(["check", path.to_str().unwrap(), "--string-strict"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(
            "string-strict: node `server` state `origin`: fixed default does not inhabit `Endpoint`"
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn rejects_unresolved_state_type() {
    let path = write_spec(
        "unresolved-state-type",
        "server : Int -> Int\n  state origin : MissingEndpoint\n  cx O(1) idem\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_apslc"))
        .args(["check", path.to_str().unwrap(), "--string-strict"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("symbol `MissingEndpoint` not found"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn rejects_duplicate_state_key_at_same_node_position() {
    let path = write_spec(
        "duplicate-state-path",
        "type Endpoint = String\n\nserver : Int -> Int\n  state origin : Endpoint\n  state origin : Endpoint\n  cx O(1) idem\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_apslc"))
        .args([
            "check",
            path.to_str().unwrap(),
            "--string-strict",
            "--state",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(
            "state: node `server`: duplicate key `origin` would produce the same canonical state path"
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn accepts_named_string_signature_and_type_correct_fixed_state() {
    let path = write_spec(
        "named-and-fixed",
        "type Message = String\ntype Method = String\n\nrelay : Message -> Message\n  state method : Method = \"GET\"\n  cx O(1) idem\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_apslc"))
        .args(["check", path.to_str().unwrap(), "--string-strict"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
