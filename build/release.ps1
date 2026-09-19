# release.ps1 —— 上传构建产物到 GitHub Release（需要 gh CLI 且已登录）
#
# 发布资产命名必须延续 Transactions-x64-v{版本}.exe：
# 应用内的更新检查取"第一个以 .exe 结尾的资产"，改名会让更新流程失效。
# 更新流程用 GitHub release asset 的 digest（sha256）做完整性校验，因此无需签名密钥。

# This script contains non-ASCII (Chinese) comments, so it MUST run on PowerShell 7 (pwsh).
# Windows PowerShell 5.1 decodes BOM-less UTF-8 as ANSI and mis-parses this file. Keep ASCII-only.
if ($PSVersionTable.PSEdition -ne 'Core') {
    $pwsh = (Get-Command pwsh -ErrorAction SilentlyContinue).Source
    if ($pwsh) {
        Write-Host "This script needs PowerShell 7; re-launching under $pwsh" -ForegroundColor Yellow
        & $pwsh -NoProfile -ExecutionPolicy Bypass -File $PSCommandPath @args
        exit $LASTEXITCODE
    }
    Write-Warning "PowerShell 7 (pwsh) not found - running on 5.1 may mis-parse this script."
}

[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$ErrorActionPreference = 'Stop'

$scriptDir = $PSScriptRoot
$projectRoot = Split-Path -Parent $scriptDir
$bundleRoot = Join-Path $projectRoot 'build\target'
$tauriConfPath = Join-Path $projectRoot 'src-tauri\tauri.conf.json'
$repo = 'ddd-online/Transactions-Rust'

function Write-Step($msg) { Write-Host "`n🛠️  $msg" -ForegroundColor Magenta }
function Write-Success($msg) { Write-Host "✅ $msg" -ForegroundColor Green }
function Fail($msg) { Write-Error "❌ $msg"; exit 1 }

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { Fail "未找到 gh CLI" }

$version = (Get-Content -Raw $tauriConfPath | ConvertFrom-Json).version
$tag = "v$version"
$installer = Join-Path $bundleRoot ("Transactions-x64-v{0}.exe" -f $version)
if (-not (Test-Path $installer)) { Fail "安装包不存在: $installer（请先运行 build/build.ps1）" }

$assets = @($installer)

Write-Step "创建/更新 Release $tag"
& gh release view $tag --repo $repo *> $null
if ($LASTEXITCODE -eq 0) {
    & gh release upload $tag @assets --repo $repo --clobber
} else {
    & gh release create $tag @assets --repo $repo --title $tag --generate-notes
}
if ($LASTEXITCODE -ne 0) { Fail "gh 发布失败，退出码: $LASTEXITCODE" }

Write-Success "已发布 $tag（资产 $($assets.Count) 个）"
Write-Host "提示：应用内更新会读取该 release 的 tag_name 与首个 .exe 资产的 digest（sha256）。" -ForegroundColor DarkGray
