#!/usr/bin/env bash
# Enforces §5.3: only `tpt-pantheon-spine-wasm-sandbox` may take a DIRECT
# `wasmtime` dependency. Runs in CI (ubuntu runners have python3).
set -euo pipefail

ALLOWED="tpt-pantheon-spine-wasm-sandbox"

python3 - "$ALLOWED" <<'PY'
import json, subprocess, sys

allowed = sys.argv[1]
meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version=1"]))

violators = []
for pkg in meta["packages"]:
    for dep in pkg.get("dependencies", []):
        if dep["name"] == "wasmtime" and dep.get("kind", "normal") in ("normal", None):
            if pkg["name"] != allowed:
                violators.append(f'{pkg["name"]} -> wasmtime (direct)')

if violators:
    print(f"Direct 'wasmtime' dependency found outside {allowed}:")
    print("\n".join(violators))
    sys.exit(1)

print(f"OK: 'wasmtime' is only a direct dependency of {allowed}")
PY
