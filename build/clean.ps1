# clean.ps1 —— 清理构建产物（不会触碰源码与 fixtures）

[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$scriptDir = $PSScriptRoot
$projectRoot = Split-Path -Parent $scriptDir

function Write-Step($msg) { Write-Host "🛠️  $msg" -ForegroundColor Magenta }
function Write-Success($msg) { Write-Host "✅ $msg" -ForegroundColor Green }

Write-Step "清理 Rust 构建产物"
foreach ($path in @(
        (Join-Path $projectRoot 'target'),
        (Join-Path $projectRoot 'build\target')
    )) {
    if (Test-Path $path) {
        Remove-Item $path -Recurse -Force -ErrorAction SilentlyContinue
        Write-Success "已删除 $path"
    }
}

Write-Step "清理界面构建产物（crates/tr-ui/dist，保留 .gitkeep）"
$distDir = Join-Path $projectRoot 'crates\tr-ui\dist'
if (Test-Path $distDir) {
    Get-ChildItem $distDir -Force | Where-Object { $_.Name -ne '.gitkeep' } | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
    $trunkDir = Join-Path $projectRoot 'crates\tr-ui\.trunk'
    if (Test-Path $trunkDir) { Remove-Item $trunkDir -Recurse -Force -ErrorAction SilentlyContinue }
    Write-Success "已清理 $distDir"
}

Write-Host "`n🎉 清理完成" -ForegroundColor Green
