//! A real LLM-backed [`CodeAgent`], enabled by the `llm` cargo feature.
//!
//! This agent sends a structured prompt — the function's intent (`requires` /
//! `ensures`, parameters, and the counter-example when rewriting) — to a
//! configured LLM provider and parses the returned `mutate state { ... }`
//! block back into the Telos statement AST. The compiler then verifies the
//! result through the normal formal-verification loop, so even an imperfect LLM
//! answer is caught and fed back until the contract is proven.
//!
//! Supported providers (`TELAS_LLM_PROVIDER`): `openai` (default), `ollama`,
//! `openrouter`, `grok`, and `anthropic`. The first four all speak the
//! OpenAI-compatible chat-completions wire format; `anthropic` uses Anthropic's
//! native Messages API instead.
//!
//! Configuration (environment variables):
//!   * `TELAS_LLM_PROVIDER`   – `openai` | `ollama` | `openrouter` | `grok` |
//!     `anthropic` (default: `openai`, or `anthropic` if `TELAS_LLM_URL`
//!     contains `anthropic.com`).
//!   * `TELAS_LLM_URL`        – API endpoint (default: provider-specific).
//!   * `TELAS_LLM_KEY`        – API key.
//!   * `TELAS_LLM_MODEL`      – model id (default: provider-specific).
//!   * `TELAS_LLM_MAX_TOKENS` – max response tokens, only used by the
//!     `anthropic` provider, which requires it (default: `4096`).

use tpt_telos_parser::ast::*;

use crate::{Candidate, CodeAgent, FuncSpec, Model};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Provider {
    OpenAiCompatible,
    Anthropic,
}

pub struct LlmAgent {
    provider: Provider,
    url: String,
    key: String,
    model: String,
    max_tokens: u32,
}

impl LlmAgent {
    pub fn from_env() -> Result<Self, String> {
        let key = std::env::var("TELAS_LLM_KEY")
            .map_err(|_| "TELAS_LLM_KEY not set (required by the LLM agent)".to_string())?;

        let url_override = std::env::var("TELAS_LLM_URL").ok();
        let provider_name = std::env::var("TELAS_LLM_PROVIDER").ok().unwrap_or_else(|| {
            if url_override
                .as_deref()
                .is_some_and(|u| u.contains("anthropic.com"))
            {
                "anthropic".to_string()
            } else {
                "openai".to_string()
            }
        });

        let (provider, default_url, default_model) = match provider_name.as_str() {
            "openai" => (
                Provider::OpenAiCompatible,
                "https://api.openai.com/v1/chat/completions",
                "gpt-4o-mini",
            ),
            "ollama" => (
                Provider::OpenAiCompatible,
                "http://localhost:11434/v1/chat/completions",
                "llama3.1",
            ),
            "openrouter" => (
                Provider::OpenAiCompatible,
                "https://openrouter.ai/api/v1/chat/completions",
                "openai/gpt-4o-mini",
            ),
            "grok" => (
                Provider::OpenAiCompatible,
                "https://api.x.ai/v1/chat/completions",
                "grok-4",
            ),
            "anthropic" => (
                Provider::Anthropic,
                "https://api.anthropic.com/v1/messages",
                "claude-sonnet-4-5",
            ),
            other => return Err(format!(
                "unknown TELAS_LLM_PROVIDER '{other}' (expected one of: openai, ollama, openrouter, grok, anthropic)"
            )),
        };

        let max_tokens = std::env::var("TELAS_LLM_MAX_TOKENS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4096);

        Ok(Self {
            provider,
            url: url_override.unwrap_or_else(|| default_url.to_string()),
            key,
            model: std::env::var("TELAS_LLM_MODEL").unwrap_or_else(|_| default_model.to_string()),
            max_tokens,
        })
    }
}

impl CodeAgent for LlmAgent {
    fn name(&self) -> &str {
        "llm"
    }

    fn generate(&self, spec: &FuncSpec) -> Result<Candidate, String> {
        let prompt = build_prompt(spec, None);
        let body = self.complete(&prompt)?;
        parse_body(&body)
    }

    fn rewrite(&self, spec: &FuncSpec, _prev: &Candidate, ce: &Model) -> Result<Candidate, String> {
        let prompt = build_prompt(spec, Some(ce));
        let body = self.complete(&prompt)?;
        parse_body(&body)
    }
}

impl LlmAgent {
    fn complete(&self, prompt: &str) -> Result<String, String> {
        match self.provider {
            Provider::OpenAiCompatible => self.complete_openai_compatible(prompt),
            Provider::Anthropic => self.complete_anthropic(prompt),
        }
    }

    fn complete_openai_compatible(&self, prompt: &str) -> Result<String, String> {
        let req = ureq::post(&self.url)
            .set("Authorization", &format!("Bearer {}", self.key))
            .set("Content-Type", "application/json")
            .send_json(ureq::json!({
                "model": self.model,
                "messages": [{ "role": "user", "content": prompt }],
                "temperature": 0.0,
            }))
            .map_err(|e| format!("LLM request failed: {e}"))?;

        let resp: serde_json::Value = req
            .into_json()
            .map_err(|e| format!("LLM response was not JSON: {e}"))?;
        let content = resp
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "LLM response missing choices[0].message.content".to_string())?
            .to_string();
        Ok(strip_code_fence(&content))
    }

    fn complete_anthropic(&self, prompt: &str) -> Result<String, String> {
        let req = ureq::post(&self.url)
            .set("x-api-key", &self.key)
            .set("anthropic-version", "2023-06-01")
            .set("Content-Type", "application/json")
            .send_json(ureq::json!({
                "model": self.model,
                "max_tokens": self.max_tokens,
                "messages": [{ "role": "user", "content": prompt }],
                "temperature": 0.0,
            }))
            .map_err(|e| format!("LLM request failed: {e}"))?;

        let resp: serde_json::Value = req
            .into_json()
            .map_err(|e| format!("LLM response was not JSON: {e}"))?;
        let content = resp
            .pointer("/content/0/text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "LLM response missing content[0].text".to_string())?
            .to_string();
        Ok(strip_code_fence(&content))
    }
}

fn strip_code_fence(s: &str) -> String {
    let trimmed = s.trim();
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        let after = after.trim_start_matches("telos").trim_start();
        if let Some(end) = after.rfind("```") {
            return after[..end].trim().to_string();
        }
        return after.trim().to_string();
    }
    trimmed.to_string()
}

/// Wrap an LLM-returned statement block in a synthetic module and extract the
/// parsed body. This reuses the existing Telos parser, keeping the LLM output in
/// the same verified AST domain as the static agent.
fn parse_body(body: &str) -> Result<Candidate, String> {
    let wrapped = format!(
        "module _T {{\n    func _f() {{\n        {}\n    }}\n}}\n",
        body
    );
    let modules = tpt_telos_parser::parse(&wrapped)?;
    for m in &modules {
        for item in &m.items {
            if let Item::Func(f) = item {
                return Ok(Candidate {
                    stmts: f.body.clone(),
                });
            }
        }
    }
    Err("LLM did not return a parseable function body".to_string())
}

fn build_prompt(spec: &FuncSpec, ce: Option<&Model>) -> String {
    let f = &spec.func;
    let params = f
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, p.ty.name()))
        .collect::<Vec<_>>()
        .join(", ");
    let requires = f
        .requires
        .iter()
        .map(|e| format!("requires {}", pretty(e)))
        .collect::<Vec<_>>()
        .join("\n");
    let ensures = f
        .ensures
        .iter()
        .map(|e| format!("ensures {}", pretty(e)))
        .collect::<Vec<_>>()
        .join("\n");
    let body_hint = if f.elided {
        "(no body provided — you must synthesize it)".to_string()
    } else {
        let inner = f
            .body
            .iter()
            .map(pretty_stmt)
            .collect::<Vec<_>>()
            .join("\n");
        format!("current body:\n{}", inner)
    };

    let mut p = String::new();
    p.push_str("You are the code-generation agent for the tpt-telos compiler.\n");
    p.push_str("Given a function intent, produce ONLY a `mutate state { ... }` block ");
    p.push_str("whose assignments satisfy the `ensures` clauses. Use only field ");
    p.push_str("assignments of the form `base.field += expr` / `-=` / `=`.\n\n");
    p.push_str(&format!(
        "func {}({}) {{\n{}\n{}\n}}\n",
        f.name, params, requires, ensures
    ));
    p.push_str(&format!("\n{}\n", body_hint));

    if let Some(ce) = ce {
        p.push_str("\nThe formal verifier rejected the previous attempt. ");
        p.push_str("Concrete counter-example (variable -> value):\n");
        let mut entries: Vec<_> = ce.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (k, v) in entries {
            p.push_str(&format!("  {} = {}\n", k, v));
        }
        p.push_str("Repair the assignments so the ensures hold for this case.\n");
    }
    p
}

fn pretty(e: &Expr) -> String {
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Var(v) => v.clone(),
        Expr::Field { base, field } => format!("{}.{}", base, field),
        Expr::Old(inner) => format!("old({})", pretty(inner)),
        Expr::Unary { op, expr } => match op {
            UnOp::Neg => format!("-{}", pretty(expr)),
        },
        Expr::Bin { op, lhs, rhs } => {
            let s = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Eq => "==",
                BinOp::Ne => "!=",
                BinOp::Lt => "<",
                BinOp::Le => "<=",
                BinOp::Gt => ">",
                BinOp::Ge => ">=",
                BinOp::And => "&&",
                BinOp::Or => "||",
            };
            format!("{} {} {}", pretty(lhs), s, pretty(rhs))
        }
        Expr::Call(c) => {
            let args: Vec<_> = c.args.iter().map(pretty).collect();
            format!("{}({})", c.func, args.join(", "))
        }
        Expr::MethodCall(m) => {
            let args: Vec<_> = m.args.iter().map(pretty).collect();
            format!("{}.{}({})", pretty(&m.receiver), m.method, args.join(", "))
        }
        Expr::Index(i) => format!("{}[{}]", pretty(&i.receiver), pretty(&i.index)),
        Expr::If(i) => format!(
            "if {} {{ {} }} else {{ {} }}",
            pretty(&i.condition),
            pretty(&i.then_expr),
            pretty(&i.else_expr)
        ),
        Expr::Match(m) => {
            let arms: Vec<_> = m
                .arms
                .iter()
                .map(|a| format!("... => {}", pretty(&a.expr)))
                .collect();
            format!("match {} {{ {} }}", pretty(&m.scrutinee), arms.join(", "))
        }
        Expr::Try(e) => format!("{}?", pretty(e)),
        Expr::Forall(f) => format!(
            "forall {}: {} {{ {} }}",
            f.var,
            f.var_ty.name(),
            pretty(&f.body)
        ),
        Expr::Aggregate(a) => {
            let args: Vec<_> = a.args.iter().map(pretty).collect();
            format!("{}({})", a.op.op_name(), args.join(", "))
        }
        Expr::Range { lo, hi } => format!("{}..{}", pretty(lo), pretty(hi)),
    }
}

fn pretty_stmt(s: &Stmt) -> String {
    match s {
        Stmt::MutateState(assigns) => {
            let inner = assigns
                .iter()
                .map(pretty_assign)
                .collect::<Vec<_>>()
                .join("\n");
            format!("mutate state {{\n{}\n}}", inner)
        }
        Stmt::Assign(a) => pretty_assign(a),
        Stmt::Let(l) => format!("let {} = {};", l.name, pretty(&l.value)),
        Stmt::If(i) => {
            let then_inner = i
                .then_body
                .iter()
                .map(pretty_stmt)
                .collect::<Vec<_>>()
                .join("\n");
            match &i.else_body {
                Some(else_body) => {
                    let else_inner = else_body
                        .iter()
                        .map(pretty_stmt)
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!(
                        "if {} {{\n{}\n}} else {{\n{}\n}}",
                        pretty(&i.condition),
                        then_inner,
                        else_inner
                    )
                }
                None => format!("if {} {{\n{}\n}}", pretty(&i.condition), then_inner),
            }
        }
        Stmt::Match(m) => {
            let arms = m
                .arms
                .iter()
                .map(|a| {
                    let body = a
                        .body
                        .iter()
                        .map(pretty_stmt)
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!("... => {{\n{}\n}}", body)
                })
                .collect::<Vec<_>>()
                .join(",\n");
            format!("match {} {{\n{}\n}}", pretty(&m.scrutinee), arms)
        }
        Stmt::Return(e) => match e {
            Some(e) => format!("return {};", pretty(e)),
            None => "return;".to_string(),
        },
    }
}

fn pretty_assign(a: &Assign) -> String {
    let op = match a.op {
        AssignOp::Set => "=",
        AssignOp::Add => "+=",
        AssignOp::Sub => "-=",
    };
    format!("{} {} {};", pretty(&a.target), op, pretty(&a.value))
}
