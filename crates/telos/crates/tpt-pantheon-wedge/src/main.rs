//! Axum front-end for the Milestone-1 wedge.
//!
//! Boots by verifying `wallet.telos` (the Telos proof), connecting to Keystone
//! and migrating the `accounts` table, then serving:
//!
//! * `GET  /accounts` — list accounts (Keystone read)
//! * `POST /transfer` — `{ "from": i64, "to": i64, "amount": i64 }`, one
//!   Keystone transaction per transfer, audited
//! * `GET  /audit`    — the tamper-evident audit log

use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tpt_pantheon_wedge::{list_accounts, transfer, verify_wallet_contract, Account, AppState};
use tpt_sdk::prelude::*;

#[derive(Deserialize)]
struct TransferReq {
    from: i64,
    to: i64,
    amount: i64,
}

async fn get_accounts(State(state): State<Arc<AppState>>) -> Json<Vec<Account>> {
    let mut client = state.client.lock().await;
    match list_accounts(&mut client).await {
        Ok(accounts) => Json(accounts),
        Err(e) => {
            tracing::warn!("list_accounts failed: {e:#}");
            Json(vec![])
        }
    }
}

async fn post_transfer(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TransferReq>,
) -> (StatusCode, Json<serde_json::Value>) {
    match transfer(&state, req.from, req.to, req.amount).await {
        Ok(()) => (StatusCode::OK, Json(json!({"status": "ok"}))),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

async fn get_audit(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<tpt_pantheon_spine_audit_log::AuditEvent>> {
    let sink = state.audit.lock().unwrap();
    Json(sink.events())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let keystone_addr =
        std::env::var("KEYSTONE_ADDR").unwrap_or_else(|_| "127.0.0.1:55432".to_string());

    // Milestone-1 proof step 1: verify the Telos contract before serving.
    let artifact = verify_wallet_contract().context("wallet.telos verification failed")?;
    tracing::info!(
        "wallet.telos verified: {} function(s) proven",
        artifact.outcomes.len()
    );

    // Milestone-1 proof step 2: connect to Keystone and migrate.
    let mut client = KeystoneClient::connect(&keystone_addr)
        .await
        .map_err(|e| anyhow::anyhow!("connect to keystone at {keystone_addr}: {e:?}"))?;
    tpt_pantheon_wedge::migrate(&mut client)
        .await
        .context("keystone migration failed")?;

    let state = Arc::new(AppState {
        client: tokio::sync::Mutex::new(client),
        audit: Arc::new(std::sync::Mutex::new(
            tpt_pantheon_spine_audit_log::HashChainSink::new(),
        )),
        verified: true,
    });

    let app = Router::new()
        .route("/accounts", get(get_accounts))
        .route("/transfer", post(post_transfer))
        .route("/audit", get(get_audit))
        .with_state(state);

    let bind = std::env::var("WEDGE_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .with_context(|| format!("bind {bind}"))?;
    tracing::info!("wedge listening on {bind} (keystone at {keystone_addr})");
    axum::serve(listener, app)
        .await
        .context("axum serve")?;
    Ok(())
}
