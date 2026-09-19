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
$assetName = Split-Path -Leaf $installer
$localDigest = 'sha256:' + (Get-FileHash $installer -Algorithm SHA256).Hash.ToLower()

# 起飞前自检：**同一个字节流不能出现在两个 tag 下**。
# 真实事故：v0.2.0 的资产其实是 0.1.0 的安装包，两个 release 的 digest 完全相同
# （根因见 build.ps1 里那段注释）。本地哈希都一样，说明这次多半又拿旧包发了新版。
Write-Step "自检：本地安装包 digest = $localDigest"
$otherTags = @()
$listed = & gh release list --repo $repo --limit 100 --json tagName | ConvertFrom-Json
foreach ($item in @($listed)) {
    if ($item.tagName -and $item.tagName -ne $tag) { $otherTags += $item.tagName }
}
foreach ($other in $otherTags) {
    $otherAssets = (& gh release view $other --repo $repo --json assets | ConvertFrom-Json).assets
    foreach ($asset in @($otherAssets)) {
        if ($asset.digest -and $asset.digest.ToLower() -eq $localDigest) {
            Fail "本地安装包与 $other 的资产 $($asset.name) 字节完全相同（$localDigest）—— 这是旧包，别发。"
        }
    }
}
Write-Success "自检通过：与其它 $($otherTags.Count) 个 release 的资产都不相同"

Write-Step "创建/更新 Release $tag"
& gh release view $tag --repo $repo *> $null
if ($LASTEXITCODE -eq 0) {
    & gh release upload $tag @assets --repo $repo --clobber
} else {
    & gh release create $tag @assets --repo $repo --title $tag --generate-notes
}
if ($LASTEXITCODE -ne 0) { Fail "gh 发布失败，退出码: $LASTEXITCODE" }

# 发布后回读：远端资产必须就是刚才那个字节流（用户下载到的正是它）
$published = (& gh release view $tag --repo $repo --json assets | ConvertFrom-Json).assets
$hit = @($published) | Where-Object { $_.name -eq $assetName } | Select-Object -First 1
if (-not $hit) { Fail "发布后读不到资产 $assetName（tag $tag）" }
if ($hit.digest.ToLower() -ne $localDigest) {
    Fail "远端 digest $($hit.digest) 与本地 $localDigest 不一致"
}

Write-Success "已发布 $tag（资产 $($assets.Count) 个，digest 已核对：$localDigest）"
Write-Host "提示：应用内更新会读取该 release 的 tag_name 与首个 .exe 资产的 digest（sha256）。" -ForegroundColor DarkGray
