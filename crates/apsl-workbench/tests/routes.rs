
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use ed25519_dalek::SigningKey;
use rand_core::OsRng;
use serde_json::Value;
use tower::ServiceExt;

#[path = "../src/pipeline.rs"]
mod pipeline;
#[path = "../src/nix.rs"]
mod nix;
#[path = "../src/handlers.rs"]
mod handlers;

struct AppState {
    key: SigningKey,
    store_base: std::path::PathBuf,
}


const DEDUPE: &str = r#"
type Email = String
type MessageId = String

normalize : String[] -> Email[]
  cx    O(n) idem

dedupe : Email[] -> Email[]
  cx    O(n log n) idem

graph email_pipeline : String[] -> MessageId[]
  flow  in -> normalize -> dedupe -> out
"#;

fn state() -> Arc<crate::AppState> {
    Arc::new(crate::AppState {
        key: SigningKey::generate(&mut OsRng),
        store_base: std::env::temp_dir().join("wb-test-certstore"),
    })
}

fn app(st: Arc<crate::AppState>) -> axum::Router {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/healthz", get(handlers::healthz))
        .route("/compile", post(handlers::compile))
        .route("/verify", post(handlers::verify))
        .with_state(st)
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn healthz_reports_self() {
    let resp = app(state())
        .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["service"], "apsl-workbench");
    assert_eq!(v["specified_by"], "workbench.apsl");
}

#[tokio::test]
async fn compile_emits_per_node_certs() {
    let resp = app(state())
        .oneshot(
            Request::post("/compile")
                .body(Body::from(DEDUPE))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["ok"], true);
    assert_eq!(v["graph"], "compile_only");
    let certs = v["certs"].as_array().unwrap();
    assert_eq!(certs.len(), 2, "two nodes -> two certs: {v}");
    for c in certs {
        assert!(c["cert_hash"].as_str().unwrap().len() >= 32, "{c}");
        assert!(c["node"].is_string());
    }
}

#[tokio::test]
async fn compile_rejects_garbage_with_422() {
    let resp = app(state())
        .oneshot(
            Request::post("/compile")
                .body(Body::from("this is not apsl ((("))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let v = body_json(resp).await;
    assert_eq!(v["ok"], false);
    assert!(v["stage"].is_string());
}

#[tokio::test]
async fn verify_identity_satisfies_when_post_covers_pre() {
    let req = serde_json::json!({
        "pre": [[0.0, 1.0]],
        "post": [[0.0, 1.0]]
    });
    let resp = app(state())
        .oneshot(
            Request::post("/verify")
                .header("content-type", "application/json")
                .body(Body::from(req.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["verdict"]["satisfies"], true, "{v}");
}


fn fake_node_store(tag: &str, name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!(
        "wb-jac-{}-{}-{}",
        std::process::id(),
        tag,
        name
    ));
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let f = bin.join(name);
    std::fs::write(&f, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    dir
}

#[tokio::test]
async fn jacobian_skips_non_numeric_nodes() {
    let manifest = std::collections::HashMap::new();
    let boxes = std::collections::HashMap::new();
    let jac = handlers::jacobian_verify(DEDUPE, &manifest, &boxes);
    assert_eq!(
        jac["normalize"], "skipped: non-numeric (needs embedding)",
        "{jac:?}"
    );
    assert_eq!(
        jac["dedupe"], "skipped: non-numeric (needs embedding)",
        "{jac:?}"
    );
}

#[tokio::test]
async fn jacobian_verifies_numeric_node_against_store_binary() {
    const SRC: &str = r#"
double : Int -> Int
  cx    O(n)
"#;
    let dir = fake_node_store("violate", "double", "read x; awk -v x=$x 'BEGIN{print 2*x}'");
    let mut manifest = std::collections::HashMap::new();
    manifest.insert("double".to_string(), dir.to_string_lossy().to_string());
    let boxes = std::collections::HashMap::new();

    let jac = handlers::jacobian_verify(SRC, &manifest, &boxes);
    let v = &jac["double"];
    assert_eq!(v["satisfies"], false, "{jac:?}");
    assert!(v["witness"].is_array(), "{jac:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn jacobian_uses_per_node_boxes_from_request() {
    const SRC: &str = r#"
double : Int -> Int
  cx    O(n)
"#;
    let dir = fake_node_store("boxes", "double", "read x; awk -v x=$x 'BEGIN{print 2*x}'");
    let mut manifest = std::collections::HashMap::new();
    manifest.insert("double".to_string(), dir.to_string_lossy().to_string());
    let mut boxes = std::collections::HashMap::new();
    boxes.insert(
        "double".to_string(),
        handlers::NodeBoxes {
            pre: vec![[0.0, 1.0]],
            post: vec![[0.0, 2.0]],
        },
    );

    let jac = handlers::jacobian_verify(SRC, &manifest, &boxes);
    assert_eq!(jac["double"]["satisfies"], true, "{jac:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn verify_identity_violates_when_post_too_tight() {
    let req = serde_json::json!({
        "node": "f",
        "apsl": "f : (x: Real) -> (Real)",
        "pre": [[0.0, 1.0]],
        "post": [[0.0, 0.5]]
    });
    let resp = app(state())
        .oneshot(
            Request::post("/verify")
                .header("content-type", "application/json")
                .body(Body::from(req.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let v = body_json(resp).await;
    assert_eq!(v["verdict"]["satisfies"], false, "{v}");
    assert!(v["verdict"]["witness"].is_array(), "{v}");
}
