# release.ps1 —— 上传构建产物到 GitHub Release（需要 gh CLI 且已登录）
#
# 发布资产命名必须延续 Transactions-x64-v{版本}.exe：
# 应用内的更新检查取"第一个以 .exe 结尾的资产"，改名会让更新流程失效。
# 更新流程用 GitHub release asset 的 digest（sha256）做完整性校验，因此无需签名密钥。
#
# Release 正文取自 `build/release_notes.md`（四节无序列表，骨架见 `build/release_template.md`）——
# 应用内「更新说明」显示的就是它。发布前会先跑一遍 `fixtures/static-guards.ps1`：
# 那份说明不合模板（版本号对不上、空节留标题、某项超 50 字…）就直接不发。

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

# Release 正文 = `build\release_notes.md`（四节无序列表，骨架见 `build\release_template.md`）。
# 从前这里取 CHANGELOG 的 `## [版本]` 那一节，正文是长篇 prose —— 应用内「更新说明」显示的就是
# 它，太长没人读。现在发布说明单独一份短列表，CHANGELOG 继续留详细正文，两者不再互相绑。
# 合规判据只有一处：`fixtures/static-guards.ps1` 第 9 项（节名与顺序 / 无序列表 / 每项字数 /
# 空节不许留标题 / 同一项不许跨节重复 / 版本号对得上 tauri.conf.json）。这里把它整份跑一遍，
# 不抄第二份规则 —— 顺带保证"护栏全绿才发"。
Write-Step "发布前护栏：发布说明必须合模板（判据在 fixtures/static-guards.ps1 第 9 项）"
$notesPath = Join-Path $projectRoot 'build\release_notes.md'
if (-not (Test-Path $notesPath)) {
    Fail "找不到 $notesPath —— 当次发布说明写在这儿（骨架见 build\release_template.md）"
}
$guardLog = Join-Path $env:TEMP "transactions-static-guards-$version.log"
& pwsh -NoProfile -ExecutionPolicy Bypass -File (Join-Path $projectRoot 'fixtures\static-guards.ps1') *> $guardLog
if ($LASTEXITCODE -ne 0) {
    Select-String -Path $guardLog -Pattern '✗' |
        ForEach-Object { Write-Host "      $($_.Line.Trim())" -ForegroundColor DarkGray }
    Fail "静态护栏没过（完整日志 $guardLog）—— 发布说明不合模板时改 build\release_notes.md"
}
$bodyFile = Join-Path $env:TEMP "transactions-release-$version.md"
((Get-Content $notesPath -Raw).TrimEnd() + "`n") | Set-Content -Path $bodyFile -Encoding UTF8
$notesArgs = @('--notes-file', $bodyFile)
Write-Success "发布说明合规（$((Get-Content $bodyFile).Count) 行，取自 build\release_notes.md）"

& gh release view $tag --repo $repo *> $null
if ($LASTEXITCODE -eq 0) {
    & gh release upload $tag @assets --repo $repo --clobber
    # 补上/刷新正文
    & gh release edit $tag --repo $repo --title $tag --notes-file $bodyFile
} else {
    & gh release create $tag @assets --repo $repo --title $tag @notesArgs
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
