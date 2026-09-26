# close-behavior.ps1 —— 验证"关闭按钮行为"三种分支。
#
# 实现对照 `src-tauri/src/shell.rs::request_close`：
#   closeBehavior = "quit" → 保存窗口尺寸后退出进程
#   closeBehavior = "tray" → 隐藏到托盘（进程继续活着，窗口不可见）
#   closeBehavior = ""     → 弹原生询问框「关闭选项」（是=退出 / 否=缩小到托盘），**不记忆**
#
# 用法（pwsh 7；需要界面内嵌的产物，见 AGENTS.md 的 custom-protocol 说明）：
#   pwsh -File fixtures/close-behavior.ps1 [-Exe <exe>] [-Workspace <ws>]
#
# 隔离：与服务端冒烟一样用临时 USERPROFILE 启动，绝不碰真实配置文件。

param(
    [string]$Exe,
    [string]$SmokeHome,
    [string]$Workspace
)

$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot 'lib\TrUia.ps1')

$repo = Split-Path -Parent $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $repo 'target\release\transactions.exe' }
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\tests\close-behavior\home' }
if (-not $Workspace) { $Workspace = Join-Path $repo 'target\tests\close-behavior\out\ws' }
if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe" }

$smokeHome = [System.IO.Path]::GetFullPath($SmokeHome)
if ($smokeHome -eq [System.IO.Path]::GetFullPath($env:USERPROFILE)) { throw "拒绝把临时 HOME 指到真实用户目录" }
if (-not (Test-Path $smokeHome)) { New-Item -ItemType Directory -Force -Path $smokeHome | Out-Null }
$ws = [System.IO.Path]::GetFullPath($Workspace)
# 工作空间**自己播种**（与 window-bounds / ui-* 同款）：三个场景都只是"写配置 → 启动 →
# 点关闭按钮"，配置里的 `workspaceDir` 指向这里，所以它必须是一份能打开的库。
# 以前这里是"没有 db 就 throw"，于是 `build\clean.ps1` 之后（target\ 被清掉）全量档
# 必然在这一步红 —— 而其余护栏都会自己播种，全量档本该能直接跑起来。
if (-not (Test-Path (Join-Path $ws 'transactions.db'))) {
    $seedDir = Split-Path -Parent $ws
    New-Item -ItemType Directory -Force -Path $seedDir | Out-Null
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $seedDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) {
        Get-Content (Join-Path $seedDir 'seed.log') -Tail 8
        throw "播种失败（exit=$seedExit）"
    }
    Write-Host "[close] 播种工作空间: $ws" -ForegroundColor Cyan
}

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public class TrClick {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  public static void Click(int x, int y) {
    SetCursorPos(x, y);
    System.Threading.Thread.Sleep(80);
    mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);   // LEFTDOWN
    mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);   // LEFTUP
  }
}
public class TrClose {
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr p);
  [DllImport("user32.dll")] static extern int GetWindowText(IntPtr h, StringBuilder t, int c);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  delegate bool EnumProc(IntPtr h, IntPtr p);
  public static List<string> VisibleTitles(uint target) {
    var found = new List<string>();
    EnumWindows((h, l) => {
      uint pid; GetWindowThreadProcessId(h, out pid);
      if (pid == target && IsWindowVisible(h)) {
        var sb = new StringBuilder(256); GetWindowText(h, sb, sb.Capacity);
        var t = sb.ToString();
        if (t.Length > 0 && !t.EndsWith("-siw")) found.Add(t);
      }
      return true;
    }, IntPtr.Zero);
    return found;
  }
}
'@ -Language CSharp

$BTN = [System.Windows.Automation.ControlType]::Button
$failures = New-Object System.Collections.Generic.List[string]

function Write-Config {
    param([string]$CloseBehavior)
    @{ width = 1280; height = 860; workspaceDir = $ws; closeBehavior = $CloseBehavior
       appearance = 'light'; smokeTestMarker = 'close-behavior.ps1' } |
        ConvertTo-Json | Set-Content -Path (Join-Path $smokeHome '.transactions.json') -Encoding UTF8
}

function Get-MainWindow {
    param([int]$ProcessId, [int]$TimeoutSec = 60)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $cond = New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $ProcessId)
        $win = $UIA::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children, $cond)
        if ($win) { return $win }
        Start-Sleep -Milliseconds 600
    }
    return $null
}

# 标题栏的关闭按钮：按名字找「关闭」后取 Y 最小的那个（页面里的弹窗关闭按钮 Y 更大）。
# 注意 Chromium 的 UIA 树是**惰性构建**的：第一次查询常常只返回十几个元素（没有 Button），
# 需要反复查询把它"唤醒"，因此这里轮询而不是固定 sleep。
function Find-CloseButton {
    param($Window)
    $cond = New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, '关闭')
    $cands = @($Window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)) |
        Where-Object { $_.Current.ControlType.ProgrammaticName -eq 'ControlType.Button' }
    return $cands | Sort-Object { $_.Current.BoundingRectangle.Y } | Select-Object -First 1
}

function Wait-CloseButton {
    param($Window, [int]$TimeoutSec = 40)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $btn = Find-CloseButton $Window
        if ($btn) { return $btn }
        Start-Sleep -Milliseconds 800
    }
    return $null
}

function Invoke-Element {
    param($Element)
    if (-not $Element) { return $false }
    $p = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$p)) { $p.Invoke(); return $true }
    # 原生对话框的按钮常只暴露 LegacyIAccessible，退回鼠标点击其中心
    $rect = $Element.Current.BoundingRectangle
    if ($rect.Width -gt 0 -and $rect.Height -gt 0) {
        [TrClick]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
        return $true
    }
    return $false
}

# 原生询问框（#32770）里按钮的 UIA 名带快捷键后缀，如 `是(Y)` / `否(N)`，且控件类型是 Pane
function Find-DialogButton {
    param($Dialog, [string[]]$Names)
    foreach ($name in $Names) {
        $cond = New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $name)
        $el = $Dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
        if ($el) { return $el }
    }
    return $null
}

function Wait-Exit {
    param($Process, [int]$TimeoutSec = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        if ($Process.HasExited) { return $true }
        Start-Sleep -Milliseconds 400
    }
    return $false
}

# ---------- 场景 1：closeBehavior = quit ----------
Write-Host "`n[close] 场景 1/3：closeBehavior=quit → 点关闭应退出进程" -ForegroundColor Cyan
Write-Config -CloseBehavior 'quit'
$p = Start-App -SmokeHome $smokeHome -Exe $Exe
try {
    $win = Get-MainWindow -ProcessId $p.Id
    if (-not $win) { throw '场景 1 拿不到窗口' }
    $closeBtn = Wait-CloseButton -Window $win
    Assert-True ($null -ne $closeBtn) '找到标题栏关闭按钮'
    Assert-True (Invoke-Element $closeBtn) '点击了标题栏关闭按钮'
    Assert-True (Wait-Exit $p 15) '进程已退出'
}
finally { if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue } }

# ---------- 场景 2：closeBehavior = tray ----------
Write-Host "`n[close] 场景 2/3：closeBehavior=tray → 点关闭应隐藏到托盘、进程仍在" -ForegroundColor Cyan
Write-Config -CloseBehavior 'tray'
$p = Start-App -SmokeHome $smokeHome -Exe $Exe
try {
    $win = Get-MainWindow -ProcessId $p.Id
    if (-not $win) { throw '场景 2 拿不到窗口' }
    $closeBtn = Wait-CloseButton -Window $win
    Assert-True ($null -ne $closeBtn) '找到标题栏关闭按钮'
    [void](Invoke-Element $closeBtn)
    Start-Sleep -Seconds 4
    Assert-True (-not $p.HasExited) '进程仍然存活（缩到托盘）'
    $titles = @([TrClose]::VisibleTitles([uint32]$p.Id))
    Assert-True ($titles.Count -eq 0) "主窗口已隐藏（可见窗口数 $($titles.Count)：$($titles -join ' | ')）"
}
finally { if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue } }

# ---------- 场景 3：closeBehavior = "" → 询问框 ----------
Write-Host "`n[close] 场景 3/3：closeBehavior 为空 → 弹「关闭选项」询问框，选「是」应退出" -ForegroundColor Cyan
Write-Config -CloseBehavior ''
$p = Start-App -SmokeHome $smokeHome -Exe $Exe
try {
    $win = Get-MainWindow -ProcessId $p.Id
    if (-not $win) { throw '场景 3 拿不到窗口' }
    $closeBtn = Wait-CloseButton -Window $win
    Assert-True ($null -ne $closeBtn) '找到标题栏关闭按钮'
    [void](Invoke-Element $closeBtn)
    Start-Sleep -Seconds 3

    # 询问框是本进程的另一个顶层窗口，标题「关闭选项」
    $dialog = $null
    $deadline = (Get-Date).AddSeconds(15)
    while ((Get-Date) -lt $deadline -and -not $dialog) {
        $cond = New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $p.Id)
        foreach ($w in @($UIA::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $cond))) {
            if ($w.Current.Name -eq '关闭选项') { $dialog = $w; break }
        }
        if (-not $dialog) { Start-Sleep -Milliseconds 500 }
    }
    Assert-True ($null -ne $dialog) '出现了「关闭选项」询问框'

    if ($dialog) {
        $yes = Find-DialogButton -Dialog $dialog -Names @('是(Y)', '是', '&Yes', 'Yes', '确定', 'OK')
        Assert-True ($null -ne $yes) '找到「是」按钮'
        [void](Invoke-Element $yes)
        Assert-True (Wait-Exit $p 15) '选「是」后进程退出'
    }
}
finally { if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue } }

if ($failures.Count -gt 0) {
    Write-Host "`n[close] ❌ $($failures.Count) 项不通过：" -ForegroundColor Red
    $failures | ForEach-Object { "  - $_" }
    exit 1
}
Write-Host "`n[close] ✅ 三种关闭行为都符合预期" -ForegroundColor Green
exit 0
