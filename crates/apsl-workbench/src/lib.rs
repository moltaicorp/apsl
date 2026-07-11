pub mod handlers;
pub mod nix;
pub mod pipeline;
#[cfg(test)]
mod resolve;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use ed25519_dalek::SigningKey;

pub struct AppState {
    pub key: SigningKey,
    pub store_base: std::path::PathBuf,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/healthz", get(handlers::healthz))
        .route("/compile", post(handlers::compile))
        .route("/build", post(handlers::build))
        .route("/verify", post(handlers::verify))
        .with_state(state)
}
