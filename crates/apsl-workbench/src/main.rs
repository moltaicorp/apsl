use std::net::SocketAddr;
use std::sync::Arc;

use apsl_workbench::{router, AppState};
use ed25519_dalek::SigningKey;

fn default_state() -> Arc<AppState> {
    use rand_core::OsRng;
    let key = SigningKey::generate(&mut OsRng);
    Arc::new(AppState {
        key,
        store_base: std::path::PathBuf::from("/tmp/wb-certstore"),
    })
}

#[tokio::main]
async fn main() {
    let state = default_state();
    let app = router(state);
    let addr: SocketAddr = std::env::var("WB_ADDR")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| "0.0.0.0:8800".parse().unwrap());
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    eprintln!("apsl-workbench listening on {addr} (specified_by apsl.apsl)");
    axum::serve(listener, app).await.expect("serve");
}
