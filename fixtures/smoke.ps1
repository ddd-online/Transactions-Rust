# smoke.ps1 —— 端到端冒烟：真正把应用起来，看窗口与日志是否符合预期。
#
# 覆盖两件事（都是自动护栏覆盖不到的部分）：
#   1. **已配置工作空间**启动：只出现主窗口（标题 `Transactions`），并且真的打开了工作空间；
#   2. **首次启动**（`workspaceDir` 为空）：只出现初始化窗口（标题 `欢迎使用 Transactions`），
#      **不会**提前创建主窗口。这一条对应曾经的真实缺陷：`workspace_init` 全仓无调用点，
#      导致选完目录后主窗口永远不出现（详见 AGENTS.md 的踩坑清单）。
#
# 为什么默认用 debug 构建的 exe：`is_dev = cfg!(debug_assertions)`，
# debug 版读 `~/.transactions-dev.json`，release 版读 `~/.transactions.json`（你的真实配置）。
# 用 debug 版做冒烟就不会碰真实配置；脚本仍然会备份并在结束时还原它读的那个文件。
#
# 用法（pwsh 7，先确保应用没在运行）：
#   cargo build --release -p transactions --features tauri/custom-protocol
#       ↑ 必须带 custom-protocol：裸 `cargo build --release` 不会内嵌界面资源，
#         窗口里会是 "127.0.0.1 拒绝连接"（= 去连 devUrl 了）。`cargo tauri build` 自带该特性。
#   pwsh -File fixtures/smoke.ps1                     # 跑两个场景
#   pwsh -File fixtures/smoke.ps1 -Case configured    # 只跑其中一个
#
# **没被自动化的一步**：首次启动里"选目录 → 主窗口出现"需要走 Windows 原生文件夹对话框。
# 有人试过用 SendKeys 盲敲回车，但那会在**当前目录**直接建库（可能建到用户自己的目录里），
# 风险大于收益，因此这一步留给人工点一次。
# 本脚本负责证明它两侧的分支都对：未配置→只有初始化窗口；已配置→只有主窗口。
#
# 注意：脚本会**强杀**自己启动的进程（用 PID 精确定位），不会动你手动开着的实例；
# 如果检测到已有 `transactions` 进程在跑，会直接拒绝执行（单实例插件会干扰判断）。

param(
    [string]$Exe,
    [string]$SmokeHome,
    [string]$WorkspaceRoot,
    [ValidateSet('all', 'configured', 'first-run')][string]$Case = 'all',
    [int]$StartupTimeoutSec = 45,
    # 直接用一个既有工作空间（例如 target\ws-rust）：
    # 这样冒烟就不依赖 `cargo xtask seed` 当前是否可用（xtask 可能正被改动）。
    [string]$Workspace
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $repo 'target\release\transactions.exe' }
if (-not $WorkspaceRoot) { $WorkspaceRoot = Join-Path $repo 'target\smoke' }
if (-not (Test-Path $WorkspaceRoot)) { New-Item -ItemType Directory -Force -Path $WorkspaceRoot | Out-Null }

# 关键设计：给被启动的进程换一个**临时 USERPROFILE**（`config.rs` 的 `home_dir()` 先读它），
# 于是应用读写的就是我这份一次性配置目录，**碰不到你真实的 ~/.transactions.json**。
# 这样 release 构建（界面已内嵌，不需要 trunk dev server）也能安全冒烟。
if (-not $SmokeHome) { $smokeHome = Join-Path $WorkspaceRoot 'home' }
$smokeHome = [System.IO.Path]::GetFullPath($SmokeHome)
$realHome = [System.IO.Path]::GetFullPath($env:USERPROFILE)
if ($smokeHome -eq $realHome) {
    throw "拒绝把临时 HOME 指到真实用户目录（$realHome）——冒烟会改写你的配置"
}
if (-not (Test-Path $smokeHome)) { New-Item -ItemType Directory -Force -Path $smokeHome | Out-Null }

# release 构建 is_dev=false → 读 <HOME>\.transactions.json
$ConfigPath = Join-Path $smokeHome '.transactions.json'
$appLog = Join-Path (Split-Path -Parent $Exe) 'logs\app.log'

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 build/build.ps1 或 cargo build --release -p transactions）" }

# 判重必须按**完整路径**，不能按进程名（本机别的目录下可能有同名 exe），
# 按名字判重会误伤用户正在用的那个应用。
$exeFull = [System.IO.Path]::GetFullPath($Exe)
$sameExe = Get-Process -Name transactions -ErrorAction SilentlyContinue | Where-Object {
    $path = try { $_.Path } catch { $null }
    $path -and ([System.IO.Path]::GetFullPath($path) -eq $exeFull)
}
if ($sameExe) {
    throw "同一个可执行文件已有实例在运行（PID $($sameExe.Id -join ', ')）——单实例插件会让新进程只唤醒旧窗口，请先关掉它"
}

# ---------- Win32：按 PID 枚举可见窗口标题 ----------
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public class TrSmokeWindows {
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc lpEnumFunc, IntPtr lParam);
  [DllImport("user32.dll")] static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hWnd);
  delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  public static List<string> ForPid(uint target) {
    var found = new List<string>();
    EnumWindows((h, l) => {
      uint pid; GetWindowThreadProcessId(h, out pid);
      if (pid == target && IsWindowVisible(h)) {
        var sb = new StringBuilder(512); GetWindowText(h, sb, sb.Capacity);
        var title = sb.ToString();
        if (title.Length > 0) found.Add(title);
      }
      return true;
    }, IntPtr.Zero);
    return found;
  }
}
'@ -Language CSharp

$failures = New-Object System.Collections.Generic.List[string]

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if ($Condition) { Write-Host "  ✓ $Message" -ForegroundColor Green }
    else { Write-Host "  ✗ $Message" -ForegroundColor Red; $failures.Add($Message) }
}

function Write-AppConfig {
    param([string]$WorkspaceDir)
    $json = [ordered]@{
        width           = 1280
        height          = 860
        workspaceDir    = $WorkspaceDir
        closeBehavior   = 'quit'
        appearance      = 'light'
        smokeTestMarker = 'smoke.ps1'      # 未知键：顺带验证配置读写会保留它
    } | ConvertTo-Json -Depth 5
    Set-Content -Path $ConfigPath -Value $json -Encoding UTF8
}

function Wait-AppWindow {
    param([System.Diagnostics.Process]$Process, [int]$TimeoutSec)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        if ($Process.HasExited) { return @() }
        $titles = Get-AppWindowTitles $Process
        if ($titles.Count -gt 0) { return $titles }
        Start-Sleep -Milliseconds 400
    }
    return @()
}

# Tauri 的单实例插件会建一个可见的消息窗口，标题形如 `com.github.Transactions-siw`。
# 它不是界面窗口，统计"应该有几个窗口"时必须排除。
function Get-AppWindowTitles {
    param([System.Diagnostics.Process]$Process)
    return @([TrSmokeWindows]::ForPid([uint32]$Process.Id) | Where-Object { $_ -notmatch '-siw$' })
}

# 等待应用日志出现某个标记：界面（WASM）启动要花一两秒，
# 固定 sleep 会在慢机器上误报"没有打开工作空间"。
function Wait-AppLog {
    param([string]$Pattern, [int]$TimeoutSec = 30)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $appLog) {
            $text = Get-Content $appLog -Raw
            if ($text -match $Pattern) { return $true }
        }
        Start-Sleep -Milliseconds 300
    }
    return $false
}

function Stop-App {
    param([System.Diagnostics.Process]$Process)
    if ($Process -and -not $Process.HasExited) {
        Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
        $Process.WaitForExit(5000) | Out-Null
    }
}

# 用临时 HOME 启动：子进程继承改过的 USERPROFILE/HOME，读写的是一次性配置。
function Start-App {
    $saved = @{ USERPROFILE = $env:USERPROFILE; HOME = $env:HOME }
    try {
        $env:USERPROFILE = $smokeHome
        $env:HOME = $smokeHome
        return Start-Process -FilePath $Exe -PassThru
    }
    finally {
        $env:USERPROFILE = $saved.USERPROFILE
        $env:HOME = $saved.HOME
    }
}

if ($Case -in @('all', 'configured')) {
    Write-Host "`n[smoke] 场景 1/2：已配置工作空间启动" -ForegroundColor Cyan
    if ($Workspace) {
        $ws = [System.IO.Path]::GetFullPath($Workspace)
        if (-not (Test-Path (Join-Path $ws 'transactions.db'))) {
            throw "指定的工作空间里没有 transactions.db: $ws"
        }
        Write-Host "  使用既有工作空间: $ws" -ForegroundColor DarkGray
    }
    else {
        $ws = Join-Path $WorkspaceRoot 'configured'
        if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
        & cargo -q xtask seed $ws *> (Join-Path $WorkspaceRoot 'seed.log')
        if ($LASTEXITCODE -ne 0) {
            Get-Content (Join-Path $WorkspaceRoot 'seed.log') -Tail 8
            throw "播种失败（exit=$LASTEXITCODE）；也可以用 -Workspace <既有工作空间> 跳过播种"
        }
    }

    Write-AppConfig -WorkspaceDir $ws
    if (Test-Path $appLog) { Remove-Item $appLog -Force }

    $process = Start-App
    try {
        Wait-AppWindow -Process $process -TimeoutSec $StartupTimeoutSec | Out-Null
        Wait-AppLog -Pattern '工作空间已打开' -TimeoutSec 30 | Out-Null
        $titles = Get-AppWindowTitles $process

        Assert-True ($titles -contains 'Transactions') "出现主窗口（标题 Transactions）"
        Assert-True (-not ($titles -contains '欢迎使用 Transactions')) "没有同时出现初始化窗口"
        Assert-True ($titles.Count -eq 1) "只有 1 个界面窗口（实际: $($titles.Count) → $($titles -join ' | ')）"

        $log = if (Test-Path $appLog) { Get-Content $appLog -Raw } else { '' }
        Assert-True ($log -match '启动 Transactions') "应用日志记录了启动"
        Assert-True ($log -match '工作空间已打开') "应用日志记录了打开工作空间（界面真的起来了）"
        Assert-True ($log -match 'IPC config_get') "界面读到了配置（config_get）"
        Assert-True (Test-Path (Join-Path $ws 'transactions.log')) "在工作空间里写了事务日志"
    }
    finally { Stop-App $process }
}

if ($Case -in @('all', 'first-run')) {
    Write-Host "`n[smoke] 场景 2/2：首次启动（workspaceDir 为空）" -ForegroundColor Cyan
    Write-AppConfig -WorkspaceDir ''
    if (Test-Path $appLog) { Remove-Item $appLog -Force }

    $process = Start-App
    try {
        Wait-AppWindow -Process $process -TimeoutSec $StartupTimeoutSec | Out-Null
        Wait-AppLog -Pattern 'IPC config_get' -TimeoutSec 30 | Out-Null
        $titles = Get-AppWindowTitles $process

        Assert-True ($titles -contains '欢迎使用 Transactions') "出现初始化窗口（标题 欢迎使用 Transactions）"
        Assert-True (-not ($titles -contains 'Transactions')) "没有提前创建主窗口"
        Assert-True ($titles.Count -eq 1) "只有 1 个界面窗口（实际: $($titles.Count) → $($titles -join ' | ')）"

        $log = if (Test-Path $appLog) { Get-Content $appLog -Raw } else { '' }
        Assert-True ($log -match '启动 Transactions') "应用日志记录了启动"
        Assert-True ($log -match 'IPC config_get') "界面读到了配置（config_get）"
        Assert-True (-not ($log -match '工作空间已打开')) "没有打开任何工作空间（符合预期）"

        # 配置里的未知键必须被原样保留（配置文件是用户数据）
        $after = Get-Content $ConfigPath -Raw | ConvertFrom-Json
        Assert-True ($after.smokeTestMarker -eq 'smoke.ps1') "配置文件里的未知键被保留"
    }
    finally { Stop-App $process }
}

if ($failures.Count -gt 0) {
    Write-Host "`n[smoke] ❌ $($failures.Count) 项不通过：" -ForegroundColor Red
    $failures | ForEach-Object { "  - $_" }
    exit 1
}
Write-Host "`n[smoke] ✅ 端到端冒烟通过" -ForegroundColor Green
exit 0

