//! Milestone-1 AppFront (DOM) client for the Pantheon wedge.
//!
//! Renders an account list and a transfer form, and wires them to the wedge
//! HTTP service (`tpt-pantheon-wedge`):
//!
//! * `GET  /accounts`  -> rendered as a live, reactive list
//! * `POST /transfer`  -> issued from the form's Submit button
//!
//! This crate only does anything on `wasm32` (like `tpt-appfront-dom`). On
//! other targets it compiles to an empty crate so the AppFront workspace still
//! builds natively.

#![cfg(target_arch = "wasm32")]

use std::rc::Rc;

use tpt_appfront_core::Signal;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};

/// Base URL of the running wedge service.
const BASE: &str = "http://127.0.0.1:3000";

#[derive(Debug, Clone)]
enum Msg {
    FromChanged(String),
    ToChanged(String),
    AmountChanged(String),
    Submit,
    LoadAudit,
}

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();

    let window = web_sys::window().expect("no window");
    let document = window.document().expect("no document");
    let body = document.body().expect("no body");

    let container = document.create_element("div")?;
    body.append_child(&container)?;

    let from = Signal::new(String::new());
    let to = Signal::new(String::new());
    let amount = Signal::new(String::new());
    let status = Signal::new(String::from("ready"));
    let accounts = Signal::new(String::from("loading accounts..."));
    let audit = Signal::new(String::from(""));

    let ui: tpt_appfront_core::UITree<Msg> =
        tpt_appfront_core::UITree::container(|c| {
            c.heading(1, "Pantheon Wallet");
            c.heading(2, "Accounts");
            c.text("(listed below)");
            c.heading(2, "Transfer");
            c.text("From account id:");
            c.input("").on_input(|v| Msg::FromChanged(v));
            c.text("To account id:");
            c.input("").on_input(|v| Msg::ToChanged(v));
            c.text("Amount:");
            c.input("").on_input(|v| Msg::AmountChanged(v));
            c.button("Send transfer").on_click(Msg::Submit);
            c.heading(2, "Audit log");
            c.button("Load audit log").on_click(Msg::LoadAudit);
            c.heading(2, "Status");
            c.text("");
        });

    let dispatch: Rc<dyn Fn(Msg)> = {
        let from = from.clone();
        let to = to.clone();
        let amount = amount.clone();
        let status = status.clone();
        let accounts = accounts.clone();
        let audit = audit.clone();
        Rc::new(move |msg| match msg {
            Msg::FromChanged(v) => from.set(v),
            Msg::ToChanged(v) => to.set(v),
            Msg::AmountChanged(v) => amount.set(v),
            Msg::Submit => {
                let from = from.clone();
                let to = to.clone();
                let amount = amount.clone();
                let status = status.clone();
                let accounts = accounts.clone();
                spawn_local(async move {
                    if let Err(e) = do_transfer(
                        &from.get(),
                        &to.get(),
                        &amount.get(),
                        &status,
                        &accounts,
                    )
                    .await
                    {
                        status.set(format!("error: {e}"));
                    }
                });
            }
            Msg::LoadAudit => {
                let audit = audit.clone();
                let status = status.clone();
                spawn_local(async move {
                    match fetch_text("GET", "/audit", None).await {
                        Ok(body) => audit.set(format!("Audit:\n{body}")),
                        Err(e) => status.set(format!("audit load failed: {e}")),
                    }
                });
            }
        })
    };

    tpt_appfront_dom::mount(&container, &ui, dispatch)?;

    // Reactive text nodes for the live account list and status line.
    let (accounts_node, accounts_handle) =
        tpt_appfront_dom::reactive_text(&document, accounts.clone())?;
    container.append_child(&accounts_node)?;
    std::mem::forget(accounts_handle);

    let (status_node, status_handle) = tpt_appfront_dom::reactive_text(&document, status.clone())?;
    container.append_child(&status_node)?;
    std::mem::forget(status_handle);

    let (audit_node, audit_handle) = tpt_appfront_dom::reactive_text(&document, audit.clone())?;
    container.append_child(&audit_node)?;
    std::mem::forget(audit_handle);

    // Initial load of the account list.
    {
        let accounts = accounts.clone();
        let status = status.clone();
        spawn_local(async move {
            let _ = refresh(&accounts, &status).await;
        });
    }

    Ok(())
}

async fn refresh(
    accounts: &Signal<String>,
    status: &Signal<String>,
) -> anyhow::Result<()> {
    match fetch_text("GET", "/accounts", None).await {
        Ok(body) => accounts.set(format!("Accounts:\n{body}")),
        Err(e) => status.set(format!("failed to load accounts: {e}")),
    }
    Ok(())
}

async fn do_transfer(
    from: &str,
    to: &str,
    amount: &str,
    status: &Signal<String>,
    accounts: &Signal<String>,
) -> anyhow::Result<()> {
    let from_id: i64 = from
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("'from' must be an integer account id"))?;
    let to_id: i64 = to
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("'to' must be an integer account id"))?;
    let amt: i64 = amount
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("'amount' must be an integer"))?;

    let body = serde_json::to_string(
        &serde_json::json!({ "from": from_id, "to": to_id, "amount": amt }),
    )?;

    let resp = fetch_text("POST", "/transfer", Some(body)).await?;
    status.set(format!("transfer result: {resp}"));
    refresh(accounts, status).await?;
    Ok(())
}

/// Minimal `fetch` helper over `web-sys` (no extra HTTP crate needed in wasm).
async fn fetch_text(
    method: &str,
    path: &str,
    body: Option<String>,
) -> anyhow::Result<String> {
    let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("no window"))?;
    let url = format!("{BASE}{path}");

    let opts = web_sys::RequestInit::new();
    opts.set_method(method);
    if let Some(b) = body {
        opts.set_body(&JsValue::from_str(&b));
    }

    let request = web_sys::Request::new_with_str_and_init(&url, &opts)
        .map_err(|e| anyhow::anyhow!("build request: {e:?}"))?;
    request
        .headers()
        .set("Content-Type", "application/json")
        .map_err(|e| anyhow::anyhow!("set header: {e:?}"))?;

    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| anyhow::anyhow!("fetch: {e:?}"))?;
    let resp: web_sys::Response = resp_val
        .dyn_into()
        .map_err(|e| anyhow::anyhow!("response: {e:?}"))?;

    let text_val = JsFuture::from(
        resp.text()
            .map_err(|e| anyhow::anyhow!("read body: {e:?}"))?,
    )
    .await
    .map_err(|e| anyhow::anyhow!("read body await: {e:?}"))?;

    Ok(text_val.as_string().unwrap_or_default())
}
