
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::pipeline::{self, CompileError};
use crate::{nix, AppState};

use apsl_core::ast::{Decl, Type};
use apsl_parse::parse_str;
use apsl_verify::{verify_numeric_node, BoxSpec, ProcessImpl};
use std::collections::HashMap;

pub async fn healthz() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "apsl-workbench",
        "specified_by": "workbench.apsl"
    }))
}

fn compile_err_response(e: &CompileError) -> (StatusCode, Json<Value>) {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({
            "ok": false,
            "stage": e.stage(),
            "error": e.message(),
        })),
    )
}

pub async fn compile(
    State(st): State<Arc<AppState>>,
    body: String,
) -> (StatusCode, Json<Value>) {
    match pipeline::compile(&body, &st.store_base, &st.key, &|_| None) {
        Ok(certs) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "graph": "compile_only",
                "certs": certs.iter().map(|c| json!({
                    "node": c.node,
                    "cert_hash": c.cert_hash,
                })).collect::<Vec<_>>(),
            })),
        ),
        Err(e) => compile_err_response(&e),
    }
}

#[derive(Deserialize, Clone)]
pub struct NodeBoxes {
    pub pre: Vec<[f64; 2]>,
    pub post: Vec<[f64; 2]>,
}

#[derive(Deserialize)]
pub struct BuildReq {
    pub apsl: String,
    #[serde(default)]
    pub boxes: HashMap<String, NodeBoxes>,
}

fn parse_build_body(body: &str) -> (String, HashMap<String, NodeBoxes>) {
    match serde_json::from_str::<BuildReq>(body) {
        Ok(r) => (r.apsl, r.boxes),
        Err(_) => (body.to_string(), HashMap::new()),
    }
}

fn is_numeric_base(name: &str) -> bool {
    matches!(name, "Int" | "Float" | "Rat" | "Real" | "Nat")
}

fn numeric_arity(ty: &Type, aliases: &HashMap<String, Type>, depth: usize) -> Option<usize> {
    if depth > 32 {
        return None;
    }
    match ty {
        Type::Base(id) => {
            if is_numeric_base(id.as_str()) {
                Some(1)
            } else if let Some(rhs) = aliases.get(id.as_str()) {
                numeric_arity(rhs, aliases, depth + 1)
            } else {
                None
            }
        }
        Type::List(inner) => numeric_arity(inner, aliases, depth + 1).map(|_| 1),
        Type::Tuple(ts) => {
            let mut total = 0;
            for t in ts {
                total += numeric_arity(t, aliases, depth + 1)?;
            }
            Some(total)
        }
        Type::Var(_) | Type::Result(_) | Type::Parameterized(_, _) => None,
        Type::Record(fields) => {
            let mut total = 0;
            for (_, t) in fields {
                total += numeric_arity(t, aliases, depth + 1)?;
            }
            Some(total)
        }
    }
}

fn unit_box(n: usize) -> BoxSpec {
    vec![(0.0, 1.0); n]
}

fn to_box(spec: &[[f64; 2]]) -> BoxSpec {
    spec.iter().map(|b| (b[0], b[1])).collect()
}

pub fn jacobian_verify(
    src: &str,
    manifest: &HashMap<String, String>,
    boxes: &HashMap<String, NodeBoxes>,
) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    let Ok(prog) = parse_str(src) else {
        return out;
    };
    let aliases: HashMap<String, Type> = prog
        .decls
        .iter()
        .filter_map(|d| match d {
            Decl::Type(a) => Some((a.name.as_str().to_string(), a.rhs.clone())),
            _ => None,
        })
        .collect();

    for d in &prog.decls {
        let Decl::Node(n) = d else { continue };
        let name = n.name.as_str().to_string();

        let in_arity = n
            .sig
            .params
            .iter()
            .try_fold(0usize, |acc, p| numeric_arity(&p.ty, &aliases, 0).map(|k| acc + k));
        let out_arity = numeric_arity(&n.sig.ret, &aliases, 0);

        let (Some(in_n), Some(out_n)) = (in_arity, out_arity) else {
            out.insert(name, json!("skipped: non-numeric (needs embedding)"));
            continue;
        };

        let Some(store_path) = manifest.get(&name) else {
            out.insert(name, json!("skipped: no derivation in manifest"));
            continue;
        };

        let bin = format!("{store_path}/bin/{name}");
        let pi = ProcessImpl::new(bin);

        let (pre, post) = match boxes.get(&name) {
            Some(b) => (to_box(&b.pre), to_box(&b.post)),
            None => (unit_box(in_n), unit_box(out_n)),
        };

        let v = verify_numeric_node(&pi, &pre, &post);
        out.insert(
            name,
            json!({
                "satisfies": v.satisfies,
                "witness": v.witness,
                "folds": v.folds,
                "refactor_suggested": v.refactor_suggested,
                "samples": v.samples,
            }),
        );
    }
    out
}

pub async fn build(
    State(st): State<Arc<AppState>>,
    raw: String,
) -> (StatusCode, Json<Value>) {
    let (body, boxes) = parse_build_body(&raw);
    let nodes = match pipeline::node_names(&body) {
        Ok(n) => n,
        Err(e) => return compile_err_response(&e),
    };

    let built = match nix::build(&body, &nodes) {
        Ok(b) => b,
        Err(msg) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"ok": false, "stage": "nix_build", "error": msg})),
            )
        }
    };

    let manifest = built.manifest.clone();
    let bind = move |node: &str| manifest.get(node).map(|p| nix::store_basename(p));
    let certs = match pipeline::compile(&body, &st.store_base, &st.key, &bind) {
        Ok(c) => c,
        Err(e) => return compile_err_response(&e),
    };

    let cert_json: Vec<Value> = certs
        .iter()
        .map(|c| {
            let store = built.manifest.get(&c.node).cloned().unwrap_or_default();
            let in_closure = !store.is_empty() && built.closure.contains(&store);
            json!({
                "node": c.node,
                "cert": c.cert_hash,
                "impl": c.impl_hash,
                "in_closure": in_closure,
            })
        })
        .collect();

    let integrated = cert_json.iter().all(|c| {
        c["in_closure"].as_bool().unwrap_or(false)
            && !c["impl"].as_str().unwrap_or("").is_empty()
    });

    let jacobian = jacobian_verify(&body, &built.manifest, &boxes);

    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "graph": "workbench",
            "app": built.app,
            "nodes": nodes,
            "certs": cert_json,
            "integrated": integrated,
            "jacobian": jacobian,
        })),
    )
}

#[derive(Deserialize)]
pub struct VerifyReq {
    pub apsl: Option<String>,
    pub node: Option<String>,
    pub pre: Vec<[f64; 2]>,
    pub post: Vec<[f64; 2]>,
}

pub async fn verify(Json(req): Json<VerifyReq>) -> (StatusCode, Json<Value>) {
    if req.pre.is_empty() || req.post.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"ok": false, "error": "pre and post boxes required"})),
        );
    }
    let pre: Vec<(f64, f64)> = req.pre.iter().map(|b| (b[0], b[1])).collect();
    let post_box = req.post.clone();

    let post = move |y: &[f64]| -> bool {
        y.iter().enumerate().all(|(i, &v)| {
            post_box
                .get(i)
                .map(|b| v >= b[0] && v <= b[1])
                .unwrap_or(true)
        })
    };
    let f = |x: &[f64]| x.to_vec();

    let v = apsl_verify::verify(&f, &pre, &post, apsl_verify::Params::default());
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "node": req.node,
            "specified_by": req.apsl.is_some().then_some("jacobian-solver.apsl"),
            "verdict": {
                "satisfies": v.satisfies,
                "witness": v.witness,
                "folds": v.folds,
                "refactor_suggested": v.refactor_suggested,
                "samples": v.samples,
            }
        })),
    )
}
