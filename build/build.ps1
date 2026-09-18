# build.ps1 —— 一键构建：Rust 界面(WASM) + Tauri 桌面应用(NSIS 安装包)
#
# 产物统一落到 build/target/，安装包重命名为 Transactions-x64-v{版本}.exe，
# 以延续原版本的发布资产命名（更新检查依赖 .exe 结尾的资产）。
#
# 版本号唯一来源：src-tauri/tauri.conf.json

[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$scriptDir = $PSScriptRoot
$projectRoot = Split-Path -Parent $scriptDir
$bundleRoot = Join-Path $projectRoot 'build\target'
$tauriConfPath = Join-Path $projectRoot 'src-tauri\tauri.conf.json'

function Write-Step($msg) { Write-Host "`n🛠️  $msg" -ForegroundColor Magenta }
function Write-Info($msg) { Write-Host "📦 $msg" -ForegroundColor Cyan }
function Write-Success($msg) { Write-Host "✅ $msg" -ForegroundColor Green }
function Fail($msg) { Write-Error "❌ $msg"; exit 1 }

$initialLocation = Get-Location

try {
    if (-not (Test-Path $tauriConfPath)) { Fail "未找到 $tauriConfPath" }
    $version = (Get-Content -Raw $tauriConfPath | ConvertFrom-Json).version
    Write-Info "版本号: $version（来自 src-tauri/tauri.conf.json）"

    # cargo/tauri 的下载同样受系统代理影响；显式透传一遍便于受限网络环境
    if ($env:HTTPS_PROXY) { Write-Info "使用 HTTPS_PROXY=$env:HTTPS_PROXY" }

    # 1. 界面（WASM）
    Write-Step "构建界面（trunk，跳过 wasm-opt，见 build-ui.ps1 顶部说明）"
    Set-Location $projectRoot
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $scriptDir 'build-ui.ps1')
    if ($LASTEXITCODE -ne 0) { Fail "界面构建失败，退出码: $LASTEXITCODE" }
    $indexHtml = Join-Path $projectRoot 'crates\tr-ui\dist\index.html'
    if (-not (Test-Path $indexHtml)) { Fail "界面产物缺失: $indexHtml" }
    Write-Success "界面构建成功"

    # 2. 桌面应用（Tauri + NSIS）
    Write-Step "构建桌面应用（cargo tauri build）"
    & cargo tauri build
    if ($LASTEXITCODE -ne 0) { Fail "cargo tauri build 失败，退出码: $LASTEXITCODE" }

    $nsisDir = Join-Path $projectRoot 'target\release\bundle\nsis'
    if (-not (Test-Path $nsisDir)) { Fail "未找到 NSIS 产物目录: $nsisDir" }
    $installer = Get-ChildItem $nsisDir -Filter '*-setup.exe' | Select-Object -First 1
    if (-not $installer) { Fail "NSIS 目录下没有 *-setup.exe" }

    New-Item -ItemType Directory -Force -Path $bundleRoot | Out-Null
    $target = Join-Path $bundleRoot ("Transactions-x64-v{0}.exe" -f $version)
    Copy-Item $installer.FullName $target -Force
    Write-Success "安装包: $target"

    # 3. 便携版（未打包的 exe）也一并留档，便于快速验证
    $appExe = Join-Path $projectRoot 'target\release\transactions.exe'
    if (Test-Path $appExe) {
        Copy-Item $appExe (Join-Path $bundleRoot 'transactions.exe') -Force
        Write-Info "已留档未打包可执行文件: build\target\transactions.exe"
    }

    Write-Host "`n🎉 构建完成" -ForegroundColor Green
}
finally {
    Set-Location $initialLocation
}
