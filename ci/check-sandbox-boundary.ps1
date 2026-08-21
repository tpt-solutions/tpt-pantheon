# Enforces §5.3 on Windows dev machines: only
# `tpt-pantheon-spine-wasm-sandbox` may take a DIRECT `wasmtime` dependency.
$ErrorActionPreference = 'Stop'

$allowed = 'tpt-pantheon-spine-wasm-sandbox'

$meta = cargo metadata --format-version 1 | ConvertFrom-Json

$violators = @()
foreach ($pkg in $meta.packages) {
    foreach ($dep in $pkg.dependencies) {
        $kind = if ($null -eq $dep.kind) { 'normal' } else { $dep.kind }
        if ($dep.name -eq 'wasmtime' -and $kind -eq 'normal' -and $pkg.name -ne $allowed) {
            $violators += "$($pkg.name) -> wasmtime (direct)"
        }
    }
}

if ($violators.Count -gt 0) {
    Write-Error "Direct 'wasmtime' dependency found outside $allowed`:`n$($violators -join "`n")"
    exit 1
}

Write-Host "OK: 'wasmtime' is only a direct dependency of $allowed"
