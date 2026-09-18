# build-ui.ps1 -- release build of the WASM UI (ASCII-only on purpose).
#
# Why not `trunk build --release`:
#   In release mode trunk additionally runs wasm-opt to shrink the artifact, but the
#   wasm-opt 123 shipped in trunk's cache cannot validate the bulk-memory instructions
#   emitted by Rust 1.96 (`memory.copy ... requires --enable-bulk-memory-opt`), and a
#   newer wasm-opt has to be downloaded from GitHub (unreachable on this machine).
#
# So we run trunk in DEBUG mode (which skips wasm-opt) while forcing Cargo's optimization
# level to 3 and disabling debug info through environment variables. The result is a
# Rust-optimized, debug-info-free wasm that simply was not post-processed by wasm-opt.
# Measured artifact: ~7.3 MiB (7.6 MB) with all seven pages compiled in
# (an unoptimized debug build would be ~15 MB).
# If a bulk-memory-capable wasm-opt becomes available, switch back to `trunk build --release`.
#
# NOTE: this file must stay ASCII-only: Tauri invokes it through Windows PowerShell 5.1,
# which decodes BOM-less files as ANSI and would mangle non-ASCII characters.

[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$ErrorActionPreference = 'Stop'

$projectRoot = Split-Path -Parent $PSScriptRoot
Set-Location $projectRoot

# NO_COLOR=1 makes trunk fail (invalid value '1' for '--no-color')
Remove-Item Env:NO_COLOR -ErrorAction SilentlyContinue

# Make the debug profile close to release quality, without debug info
$env:CARGO_PROFILE_DEV_OPT_LEVEL = '3'
$env:CARGO_PROFILE_DEV_DEBUG = 'false'
$env:CARGO_PROFILE_DEV_INCREMENTAL = 'false'

Write-Host "Building UI (trunk debug profile + opt-level=3, wasm-opt skipped)" -ForegroundColor Magenta
& trunk build --config crates/tr-ui/Trunk.toml
if ($LASTEXITCODE -ne 0) {
    Write-Error "UI build failed with exit code $LASTEXITCODE"
    exit $LASTEXITCODE
}

$indexHtml = Join-Path $projectRoot 'crates\tr-ui\dist\index.html'
if (-not (Test-Path $indexHtml)) {
    Write-Error "UI artifact missing: $indexHtml"
    exit 1
}
Write-Host "UI build finished: $indexHtml" -ForegroundColor Green
