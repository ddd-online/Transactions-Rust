# build.ps1 —— 一键构建：Rust 界面(WASM) + Tauri 桌面应用(NSIS 安装包)
#
# 产物统一落到 build/target/，安装包重命名为 Transactions-x64-v{版本}.exe，
# 该命名是发布资产的固定约定（更新检查依赖 .exe 结尾的资产）。
#
# 版本号唯一来源：src-tauri/tauri.conf.json

# This script contains non-ASCII (Chinese) comments, so it MUST run on PowerShell 7 (pwsh).
# Windows PowerShell 5.1 decodes BOM-less UTF-8 as ANSI, which mis-parses this file: the last
# step silently broke (the portable exe was never archived) WHILE STILL EXITING 0. Re-launch
# ourselves under pwsh instead of trusting the caller's shell. Keep this block ASCII-only.
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
    #
    # 真实事故（0.2.0 的发布资产其实是 0.1.0 的安装包）：
    # cargo tauri build 不会清 NSIS 产物目录，里面于是同时留着 Transactions_0.1.0_x64-setup.exe
    # 与 Transactions_0.2.0_x64-setup.exe；原来的 `Get-ChildItem *-setup.exe | Select-Object -First 1`
    # 取的是**字典序第一个**（0.1.0 排在 0.2.0 前面），于是旧包被改名成 Transactions-x64-v0.2.0.exe
    # 上传，两个 release 的资产字节数/digest 一模一样，用户装上看到的还是 0.1.0 的界面。
    # 现在：先清掉陈旧安装包 → 只认本次版本的文件名 → 再断言它是这一轮构建出来的。
    $nsisDir = Join-Path $projectRoot 'target\release\bundle\nsis'
    if (Test-Path $nsisDir) {
        Get-ChildItem $nsisDir -Filter '*-setup.exe' | Remove-Item -Force
    }

    Write-Step "构建桌面应用（cargo tauri build）"
    $buildStart = Get-Date
    & cargo tauri build
    if ($LASTEXITCODE -ne 0) { Fail "cargo tauri build 失败，退出码: $LASTEXITCODE" }

    if (-not (Test-Path $nsisDir)) { Fail "未找到 NSIS 产物目录: $nsisDir" }
    $expectedName = "Transactions_{0}_x64-setup.exe" -f $version
    $installer = Get-ChildItem $nsisDir -Filter '*-setup.exe' |
        Where-Object { $_.Name -eq $expectedName } | Select-Object -First 1
    if (-not $installer) {
        $found = ((Get-ChildItem $nsisDir -Filter '*-setup.exe' | ForEach-Object { $_.Name }) -join ', ')
        Fail "NSIS 目录下没有本次版本的安装包 $expectedName（实际: $found）"
    }
    if ($installer.LastWriteTime -lt $buildStart) {
        Fail "安装包不是本次构建的产物（$($installer.LastWriteTime) < $buildStart）：$($installer.FullName)"
    }
    # 安装包自己的版本资源（NSIS 由 tauri.conf.json 的 version 写入）：名字对、时间新还不够，
    # 装出来到底是不是这一版，只有它自报的版本号说了算。
    $installerVersion = (Get-Item $installer.FullName).VersionInfo.ProductVersion
    if ($installerVersion -ne $version) {
        Fail "安装包自报版本是 $installerVersion，期望 $version：$($installer.FullName)"
    }

    New-Item -ItemType Directory -Force -Path $bundleRoot | Out-Null
    $target = Join-Path $bundleRoot ("Transactions-x64-v{0}.exe" -f $version)
    Copy-Item $installer.FullName $target -Force
    if (-not (Test-Path $target)) { Fail "安装包未落盘: $target" }
    Write-Success "安装包: $target（$((Get-Item $target).Length) 字节）"

    # 3. 便携版（未打包的 exe）也一并留档，便于快速验证；缺了也算失败（别让退出码骗过去）
    $appExe = Join-Path $projectRoot 'target\release\transactions.exe'
    if (-not (Test-Path $appExe)) { Fail "未找到未打包可执行文件: $appExe" }
    $portable = Join-Path $bundleRoot 'transactions.exe'
    Copy-Item $appExe $portable -Force
    if (-not (Test-Path $portable)) { Fail "便携版未落盘: $portable" }
    Write-Info "已留档未打包可执行文件: build\target\transactions.exe"

    Write-Host "`n🎉 构建完成" -ForegroundColor Green
}
finally {
    Set-Location $initialLocation
}
