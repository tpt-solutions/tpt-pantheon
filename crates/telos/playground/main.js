import init, { verify, parse } from "./pkg/out_telos_wasm.js";

const DEFAULT_SOURCE = `module Bank {
    invariant Wallet { balance >= 0 }

    func deposit(w: Wallet, amount: Int)
        requires amount > 0
        ensures w.balance == old(w.balance) + amount
    { mutate state { w.balance += amount } }

    func withdraw(w: Wallet, amount: Int)
        requires amount > 0
        requires amount <= w.balance
        ensures w.balance == old(w.balance) - amount
    { mutate state { w.balance -= amount } }
}
`;

const sourceEl = document.getElementById("source");
const outputEl = document.getElementById("output");
const statusEl = document.getElementById("status");

sourceEl.value = DEFAULT_SOURCE;

function escapeHtml(s) {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function renderVerify(json) {
  if (!json.ok) {
    outputEl.innerHTML = `<span class="err">parse/extract error:\n${escapeHtml(
      json.error
    )}</span>`;
    return;
  }
  const lines = [];
  lines.push(
    json.passed
      ? `<span class="check-pass">RESULT: verification passed</span>`
      : `<span class="check-fail">RESULT: verification failed</span>`
  );
  lines.push("");
  for (const f of json.functions) {
    lines.push(`<span class="fn-head">function ${escapeHtml(f.name)}</span>`);
    for (const c of f.checks) {
      const tag = c.passed
        ? "check-pass"
        : c.is_approximation
        ? "check-approx"
        : "check-fail";
      const mark = c.passed ? "PASS" : "FAIL";
      let line = `  [${mark}] ${escapeHtml(c.description)}`;
      if (c.is_approximation) line += " [interval-bounded]";
      lines.push(`  <span class="${tag}">${line}</span>`);
      if (!c.passed && c.counterexample) {
        const binds = Object.entries(c.counterexample)
          .map(([k, v]) => `${k}=${v}`)
          .join(", ");
        lines.push(`    <span class="counterexample">counterexample: {${binds}}</span>`);
      }
    }
    lines.push("");
  }
  outputEl.innerHTML = lines.join("\n");
}

function renderParse(json) {
  if (!json.ok) {
    outputEl.innerHTML = `<span class="err">parse error:\n${escapeHtml(
      json.error
    )}</span>`;
    return;
  }
  const lines = json.modules.map((m) => {
    return `module ${escapeHtml(m.name)}: functions [${m.functions
      .map((f) => escapeHtml(f))
      .join(", ")}], invariants [${m.invariants
      .map((i) => escapeHtml(i))
      .join(", ")}]`;
  });
  outputEl.innerHTML = `<span class="check-pass">Parsed OK</span>\n\n${escapeHtml(
    lines.join("\n")
  )}`;
}

async function runVerify() {
  statusEl.textContent = "verifying…";
  try {
    const res = JSON.parse(verify(sourceEl.value));
    renderVerify(res);
    statusEl.textContent = res.passed ? "passed" : "failed";
  } catch (e) {
    outputEl.innerHTML = `<span class="err">internal error: ${escapeHtml(
      String(e)
    )}</span>`;
    statusEl.textContent = "error";
  }
}

async function runParse() {
  statusEl.textContent = "parsing…";
  try {
    const res = JSON.parse(parse(sourceEl.value));
    renderParse(res);
    statusEl.textContent = res.ok ? "parsed" : "parse error";
  } catch (e) {
    outputEl.innerHTML = `<span class="err">internal error: ${escapeHtml(
      String(e)
    )}</span>`;
    statusEl.textContent = "error";
  }
}

document.getElementById("verify-btn").addEventListener("click", () => {
  runVerify();
});
document.getElementById("parse-btn").addEventListener("click", () => {
  runParse();
});

await init();
statusEl.textContent = "ready";
