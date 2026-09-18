# window-bounds.ps1 —— 窗口大小/位置的"记住上次"回归测试。
#
# 为什么需要它：用户反馈"每次打开软件都没有保留上次的窗口大小和位置"。
# 根因是**单位混用**：Windows 上 Tauri 的 `inner_size()` / `outer_position()` 返回**物理像素**，
# 而 `WebviewWindowBuilder::inner_size()` / `position()` 收的是**逻辑像素**；
# 原 Electron 版 (`getBounds()` / `new BrowserWindow({width,height,x,y})`) 两边都是逻辑像素。
# 于是 150% 缩放的机器上：存了 1500、下次当 1500 逻辑用 → 变成 2250，每启动一次就更大、更偏。
# 修复见 `src-tauri/src/shell.rs` 的 `save_window_bounds`（物理 → 逻辑）与 `logical_bounds` 单测。
#
# 断言三件事：
#   1. 配置里的**逻辑**尺寸能正确还原成窗口的物理尺寸（应用方向）；
#   2. 关闭后写回的配置仍是**逻辑**尺寸（保存方向，不能写物理）；
#   3. 再启动一次，窗口尺寸与第一次一致（往返稳定 —— 用户真正感知的"记住上次"）。
#
# 用法（pwsh 7；需要 release 产物；本仓库不能有实例在跑）：
#   pwsh -File fixtures/window-bounds.ps1

param(
    [string]$Exe,
    [string]$SmokeHome,
    [string]$OutDir
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $repo 'target\release\transactions.exe' }
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\bounds-smoke\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\bounds-smoke' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$smokeHome = [System.IO.Path]::GetFullPath($SmokeHome)
if ($smokeHome -eq [System.IO.Path]::GetFullPath($env:USERPROFILE)) {
    throw "拒绝把临时 HOME 指到真实用户目录 —— 冒烟会改写你的配置"
}
foreach ($dir in @($smokeHome, (Join-Path $smokeHome 'Desktop'), $OutDir)) {
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
}

# 判重按完整路径（原 Electron 版也叫 Transactions.exe，不能按进程名）
$repoPrefix = $repo.TrimEnd('\') + '\'
$blockers = @(Get-Process -Name transactions -ErrorAction SilentlyContinue | Where-Object {
    $path = try { $_.Path } catch { $null }
    $path -and $path.StartsWith($repoPrefix, [System.StringComparison]::OrdinalIgnoreCase)
})
if ($blockers.Count -gt 0) {
    throw "本仓库已有 Transactions 实例在运行（PID $($blockers.Id -join ', ')），单实例插件会顶掉本次启动。"
}

$ws = Join-Path $OutDir 'ws'
if (-not (Test-Path (Join-Path $ws 'transactions.db'))) {
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[bounds] 播种工作空间: $ws" -ForegroundColor Cyan
}

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$UIA = [System.Windows.Automation.AutomationElement]

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class TrBounds {
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  public const uint WM_CLOSE = 0x0010;
  public static void CloseWindow(IntPtr hWnd) { PostMessage(hWnd, WM_CLOSE, IntPtr.Zero, IntPtr.Zero); }
}
'@ -Language CSharp -ErrorAction SilentlyContinue

$failures = New-Object System.Collections.Generic.List[string]
function Assert-True {
    param([bool]$Condition, [string]$Message)
    if ($Condition) { Write-Host "  ✓ $Message" -ForegroundColor Green }
    else { Write-Host "  ✗ $Message" -ForegroundColor Red; $failures.Add($Message) }
}

$configPath = Join-Path $smokeHome '.transactions.json'
function Write-Config {
    param([int]$Width, [int]$Height, [int]$X, [int]$Y)
    [ordered]@{
        width         = $Width
        height        = $Height
        x             = $X
        y             = $Y
        workspaceDir  = $ws
        closeBehavior = 'quit'
        appearance    = 'light'
    } | ConvertTo-Json | Set-Content -Path $configPath -Encoding UTF8
}
function Read-Config { return Get-Content $configPath -Raw | ConvertFrom-Json }

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

function Get-AppWindow {
    param([System.Diagnostics.Process]$Process, [int]$TimeoutSec = 40)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $window = $UIA::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children,
            (New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $Process.Id)))
        if ($window) { return $window }
        Start-Sleep -Milliseconds 500
    }
    return $null
}

# 关掉应用并等它真的退出（WM_CLOSE = 点关闭按钮；配置里 closeBehavior=quit → 保存后退出）
function Stop-App {
    param($Window, [System.Diagnostics.Process]$Process)
    if ($Window) { [TrBounds]::CloseWindow([IntPtr]$Window.Current.NativeWindowHandle) | Out-Null }
    if (-not $Process.WaitForExit(15000)) {
        Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
        $Process.WaitForExit(5000) | Out-Null
        return $false
    }
    return $true
}

$dpi = [TrBounds]::GetDpiForSystem()
$scale = $dpi / 96.0
Write-Host "[bounds] 系统 DPI = $dpi（缩放 $scale）" -ForegroundColor Cyan

# ---- 场景 1：配置里的逻辑尺寸要能还原成正确的物理窗口 ----
$logicalWidth = 1000
$logicalHeight = 700
Write-Host "`n[bounds] 1/3 用逻辑尺寸 ${logicalWidth}×${logicalHeight} 启动"
Write-Config -Width $logicalWidth -Height $logicalHeight -X 120 -Y 90
$process = Start-App
try {
    $window = Get-AppWindow -Process $process
    if (-not $window) { throw '启动后 40 秒内没有拿到应用窗口' }
    Start-Sleep -Seconds 2
    $rect = $window.Current.BoundingRectangle
    $expectedWidth = $logicalWidth * $scale
    $expectedHeight = $logicalHeight * $scale
    Write-Host ("  实际窗口: {0}×{1} @ ({2},{3})；期望约 {4}×{5}" -f `
        [int]$rect.Width, [int]$rect.Height, [int]$rect.X, [int]$rect.Y, [int]$expectedWidth, [int]$expectedHeight)
    Assert-True ([Math]::Abs($rect.Width - $expectedWidth) -le 60) "窗口宽度 ≈ 逻辑宽 × 缩放（$([int]$rect.Width) vs $([int]$expectedWidth)）"
    Assert-True ([Math]::Abs($rect.Height - $expectedHeight) -le 60) "窗口高度 ≈ 逻辑高 × 缩放（$([int]$rect.Height) vs $([int]$expectedHeight)）"
    # 这条专门抓"把逻辑值当物理值用"（那样会小 1/scale）
    Assert-True ($rect.Width -gt ($logicalWidth * 1.15)) '窗口没有被当成物理像素而缩小（DPI 方向没搞反）'

    Write-Host "`n[bounds] 2/3 关闭应用后检查写回的配置"
    $exitedNormally = Stop-App -Window $window -Process $process
    Assert-True $exitedNormally '关闭后进程正常退出（走的是保存 + 退出路径）'
    $saved = Read-Config
    Write-Host ("  配置写回: {0}×{1} @ ({2},{3})" -f $saved.width, $saved.height, $saved.x, $saved.y)
    Assert-True ([Math]::Abs([int]$saved.width - $logicalWidth) -le 8) "写回的宽度仍是逻辑像素（$($saved.width) ≈ $logicalWidth，不是 $([int]$expectedWidth)）"
    Assert-True ([Math]::Abs([int]$saved.height - $logicalHeight) -le 8) "写回的高度仍是逻辑像素（$($saved.height) ≈ $logicalHeight）"

    Write-Host "`n[bounds] 3/3 再启动一次，尺寸应与第一次一致"
    $process = Start-App
    $window = Get-AppWindow -Process $process
    if (-not $window) { throw '第二次启动也没拿到窗口' }
    Start-Sleep -Seconds 2
    $rect2 = $window.Current.BoundingRectangle
    Write-Host ("  第二次窗口: {0}×{1} @ ({2},{3})" -f [int]$rect2.Width, [int]$rect2.Height, [int]$rect2.X, [int]$rect2.Y)
    Assert-True ([Math]::Abs($rect2.Width - $rect.Width) -le 40) "两次启动窗口宽度一致（$([int]$rect.Width) → $([int]$rect2.Width)）"
    Assert-True ([Math]::Abs($rect2.Height - $rect.Height) -le 40) "两次启动窗口高度一致（$([int]$rect.Height) → $([int]$rect2.Height)）"
    Assert-True ([Math]::Abs($rect2.X - $rect.X) -le 40 -and [Math]::Abs($rect2.Y - $rect.Y) -le 40) "两次启动窗口位置一致（$([int]$rect.X),$([int]$rect.Y) → $([int]$rect2.X),$([int]$rect2.Y)）"
}
finally {
    if ($process -and -not $process.HasExited) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        $process.WaitForExit(5000) | Out-Null
    }
}

Write-Host ''
if ($failures.Count -gt 0) {
    Write-Host "[bounds] 失败 $($failures.Count) 项：" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "   - $_" -ForegroundColor Red }
    exit 1
}
Write-Host '[bounds] 全部通过：逻辑尺寸生效 → 关闭写回逻辑值 → 重启尺寸/位置不变' -ForegroundColor Green
