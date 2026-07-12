use apsl_artifact::{check, compile, Check};
use apsl_parse::parse_str;
use serde_json::Value;

const SOURCE: &str = include_str!("fixtures/compiled-graph-types-v1.apsl");
const EXPECTED_CANON: &str = include_str!("fixtures/compiled-graph-types-v1.canon.json");
const EXPECTED_SHA256: &str = include_str!("fixtures/compiled-graph-types-v1.sha256");

#[test]
fn emits_the_cross_repository_golden_artifact() {
    let program = parse_str(SOURCE).expect("fixture parses");
    let checked = check(&program, &[Check::State, Check::StringStrict])
        .expect("fixture passes selected composable checks");
    let artifact = compile(&checked).expect("checked fixture compiles");
    let document: Value = serde_json::from_str(artifact.canonical_utf8()).expect("canonical JSON");

    assert_eq!(artifact.canonical_utf8(), EXPECTED_CANON.trim_end());
    assert_eq!(artifact.sha256_hex(), EXPECTED_SHA256.trim());
    assert_eq!(document["schema"], "apsl.compiled-graph-types.v1");
    assert_eq!(
        document["checks"],
        serde_json::json!(["state", "string-strict", "types"])
    );
    assert_eq!(document["contracts"][0]["placement"], "fungible");
    assert_eq!(document["contracts"][1]["placement"], "fungible");
    assert_eq!(document["contracts"][2]["placement"], "positional");
    assert_eq!(
        document["graphs"][0]["flow"][0][0][0],
        serde_json::json!({ "port": "in" })
    );
    assert_eq!(
        document["graphs"][0]["flow"][0][4][0],
        serde_json::json!({ "port": "out" })
    );
    assert_eq!(
        document["graphs"][0]["state_addresses"],
        serde_json::json!([
            {
                "contract": 1,
                "kind": "fixed",
                "owner": [0, 1],
                "state": 0
            },
            {
                "contract": 2,
                "kind": "abstract",
                "owner": [0, 2],
                "state": 0
            }
        ])
    );
    assert_eq!(artifact.sha256_hex().len(), 64);
}
