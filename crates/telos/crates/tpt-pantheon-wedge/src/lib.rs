//! Milestone-1 wedge for tpt-pantheon.
//!
//! This crate is the concrete proof called for by `todo.md` Phase 2: it wires
//! the three vendored Layer-1 primitives together behind one HTTP surface —
//!
//! * **Keystone** (`tpt-keystone-sdk`) — the storage engine; the `accounts`
//!   table and every transfer live here, one transaction-surface per transfer.
//! * **Telos** (`tpt-telos-sdk`) — the `wallet.telos` transfer contract is
//!   *verified* at startup (`telos verify`), so the invariant `balance >= 0`
//!   and the `transfer` post-conditions are proven before the service serves
//!   a single request.
//! * **AppFront** — the DOM client (see `tpt-pantheon-wedge-appfront`) calls
//!   this service's `/accounts` and `/transfer` routes.
//!
//! All auditable writes go through `tpt-pantheon-spine-audit-log`'s
//! [`HashChainSink`], giving a tamper-evident log surfaced at `GET /audit`.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tpt_pantheon_spine_audit_log::{AuditEvent, AuditOutcome, AuditSink, HashChainSink};
use tpt_sdk::keystone::Value as Kv;
use tpt_sdk::prelude::*;

/// The transfer contract, verified with `telos` at startup.
pub const WALLET_TELOS: &str = include_str!("wallet.telos");

/// Run the Telos parse → verify pipeline over [`WALLET_TELOS`].
///
/// This is the "telos verify" step of the Milestone-1 proof: if the contract
/// does not verify, the service refuses to start. A verification failure is
/// reported as data (via `format_outcome_hints`), not as a panic.
pub fn verify_wallet_contract() -> Result<tpt_telos_sdk::VerifiedArtifact> {
    let artifact = tpt_telos_sdk::compile(WALLET_TELOS, &tpt_telos_sdk::StaticAgent::new())
        .map_err(|e| anyhow!("telos compile failed: {e:?}"))?;
    if !artifact.all_verified {
        let hints: Vec<String> = artifact
            .outcomes
            .iter()
            .map(tpt_telos_sdk::format_outcome_hints)
            .collect();
        return Err(anyhow!(
            "wallet.telos did not verify:\n{}",
            hints.join("\n")
        ));
    }
    Ok(artifact)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub balance: i64,
}

/// Create the `accounts` table (idempotent) and seed two demo rows if empty.
pub async fn migrate(client: &mut KeystoneClient) -> Result<()> {
    client
        .query(
            "CREATE TABLE IF NOT EXISTS accounts (\
                id BIGINT PRIMARY KEY, \
                name TEXT NOT NULL, \
                balance BIGINT NOT NULL\
            );",
        )
        .await
        .map_err(|e| anyhow!("create table: {e:?}"))?;

    let count = client
        .query("SELECT count(*) FROM accounts;")
        .await
        .map_err(|e| anyhow!("count: {e:?}"))?;
    let n: i64 = count
        .rows
        .first()
        .and_then(|r| r.get_str(0))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if n == 0 {
        client
            .query(
                "INSERT INTO accounts (id, name, balance) VALUES \
                 (1, 'Alice', 1000), (2, 'Bob', 500);",
            )
            .await
            .map_err(|e| anyhow!("seed: {e:?}"))?;
    }
    Ok(())
}

/// List all accounts ordered by id.
pub async fn list_accounts(client: &mut KeystoneClient) -> Result<Vec<Account>> {
    let res = client
        .query("SELECT id, name, balance FROM accounts ORDER BY id;")
        .await
        .map_err(|e| anyhow!("select: {e:?}"))?;
    let mut out = Vec::with_capacity(res.rows.len());
    for r in &res.rows {
        let id: i64 = r
            .get_str(0)
            .context("missing id column")?
            .parse()
            .context("parse id")?;
        let name = r.get_str(1).context("missing name column")?.to_string();
        let balance: i64 = r
            .get_str(2)
            .context("missing balance column")?
            .parse()
            .context("parse balance")?;
        out.push(Account { id, name, balance });
    }
    Ok(out)
}

/// Shared service state.
pub struct AppState {
    pub client: tokio::sync::Mutex<KeystoneClient>,
    pub audit: std::sync::Arc<std::sync::Mutex<HashChainSink>>,
    pub verified: bool,
}

/// Perform a transfer as one Keystone transaction-surface: both balance updates
/// are issued in a single query call. Rejected transfers (non-positive amount or
/// insufficient funds) are logged as `Deny` and return an error without mutating
/// state.
pub async fn transfer(state: &AppState, from: i64, to: i64, amount: i64) -> Result<()> {
    if amount <= 0 {
        state
            .audit
            .lock()
            .unwrap()
            .append(
                AuditEvent::builder("transfer", "wedge")
                    .target(format!("account:{from}->account:{to}"))
                    .outcome(AuditOutcome::Deny)
                    .meta("amount", amount.to_string())
                    .meta("reason", "non-positive amount")
                    .build(),
            )
            .map_err(|e| anyhow!("audit: {e:?}"))?;
        return Err(anyhow!("amount must be positive"));
    }

    let mut client = state.client.lock().await;

    // Pre-condition check: `from` must cover `amount` (the Telos `requires`).
    let bal = client
        .query(&format!("SELECT balance FROM accounts WHERE id = {from};"))
        .await
        .map_err(|e| anyhow!("read balance: {e:?}"))?;
    let current: i64 = bal
        .rows
        .first()
        .and_then(|r| r.get_str(0))
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow!("unknown account {from}"))?;

    if current < amount {
        drop(client);
        state
            .audit
            .lock()
            .unwrap()
            .append(
                AuditEvent::builder("transfer", "wedge")
                    .target(format!("account:{from}->account:{to}"))
                    .outcome(AuditOutcome::Deny)
                    .meta("amount", amount.to_string())
                    .meta("reason", "insufficient funds")
                    .build(),
            )
            .map_err(|e| anyhow!("audit: {e:?}"))?;
        return Err(anyhow!("insufficient funds in account {from}"));
    }

    // The transfer itself — one transaction-surface over Keystone.
    let sql = format!(
        "UPDATE accounts SET balance = balance - {amount} WHERE id = {from}; \
         UPDATE accounts SET balance = balance + {amount} WHERE id = {to};"
    );
    client
        .query(&sql)
        .await
        .map_err(|e| anyhow!("transfer: {e:?}"))?;
    drop(client);

    state
        .audit
        .lock()
        .unwrap()
        .append(
            AuditEvent::builder("transfer", "wedge")
                .target(format!("account:{from}->account:{to}"))
                .outcome(AuditOutcome::Allow)
                .meta("amount", amount.to_string())
                .build(),
        )
        .map_err(|e| anyhow!("audit: {e:?}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The real Telos verification of the bundled wallet contract. This is the
    // "telos verify" proof of Milestone 1, exercised in CI.
    #[test]
    fn wallet_contract_verifies() {
        let artifact = verify_wallet_contract().expect("wallet.telos must verify");
        assert!(artifact.all_verified, "all transfer functions must verify");
        assert!(
            !artifact.outcomes.is_empty(),
            "the transfer function must produce an outcome"
        );
    }
}
