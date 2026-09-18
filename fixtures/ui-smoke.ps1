# ui-smoke.ps1 —— 逐页界面冒烟：用 UI Automation 驱动真实窗口，挨个点开 7 个页面并断言内容渲染。
#
# 为什么需要它：`fixtures/smoke.ps1` 只能证明"应用起来了、工作空间打开了"，
# 证明不了"每个页面点开是好的"。人工逐页验收（docs/ACCEPTANCE.md）依然是最终判据，
# 但这个脚本能把"页面能不能打开、有没有明显渲染"这一层自动化，人工只需看视觉细节。
#
# 机制：WebView2 会把 DOM 暴露成 UIA 树（首次查询后 1-2 秒才建好）。
# 侧栏按钮的 UIA Name 就是导航文案，所以可以按名字 Invoke。
#
# 用法（pwsh 7；需要 release 产物，debug 构建不会内嵌界面）：
#   cargo build --release -p transactions
#   pwsh -File fixtures/ui-smoke.ps1 -Workspace target\parity\ws-rust
#   pwsh -File fixtures/ui-smoke.ps1 -Workspace <ws> -Discover     # 导出每页的元素清单，用来维护下面的标记表
#
# 隔离：和 smoke.ps1 一样用临时 USERPROFILE 启动，不会碰你真实的配置文件。

param(
    [string]$Exe,
    [string]$SmokeHome,
    [string]$Workspace,
    [switch]$Discover,
    [string]$OutDir,
    # 写入冒烟：在**该工作空间的副本**里通过界面记一笔，验证"弹窗 → 填表 → 保存 → 列表出现"整条链路。
    [switch]$WriteFlow,
    [string]$WriteDescription = 'UIA 冒烟记录',
    [string]$WriteAmount = '12.34'
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $repo 'target\release\transactions.exe' }
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\smoke\home-ui' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\ui-smoke' }
if (-not $Workspace) { $Workspace = Join-Path $repo 'target\parity\ws-rust' }
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Force -Path $OutDir | Out-Null }
if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$smokeHome = [System.IO.Path]::GetFullPath($SmokeHome)
if ($smokeHome -eq [System.IO.Path]::GetFullPath($env:USERPROFILE)) {
    throw "拒绝把临时 HOME 指到真实用户目录 —— 冒烟会改写你的配置"
}
if (-not (Test-Path $smokeHome)) { New-Item -ItemType Directory -Force -Path $smokeHome | Out-Null }

$ws = [System.IO.Path]::GetFullPath($Workspace)
if (-not (Test-Path (Join-Path $ws 'transactions.db'))) { throw "工作空间里没有 transactions.db: $ws" }

$exeFull = [System.IO.Path]::GetFullPath($Exe)
$sameExe = Get-Process -Name transactions -ErrorAction SilentlyContinue | Where-Object {
    $path = try { $_.Path } catch { $null }
    $path -and ([System.IO.Path]::GetFullPath($path) -eq $exeFull)
}
if ($sameExe) { throw "同一个可执行文件已有实例在运行（PID $($sameExe.Id -join ', ')），请先关掉" }

# 每页的断言标记：点开该页后，UIA 树里应当出现的可见文案/控件名。
# 用 `-Discover` 生成的清单来维护这张表（发现缺失标记时先跑 Discover 看真实文案）。
# 标记刻意选"页面结构/常驻控件"而不是随数据变化的文案，换工作空间也能过。
$markers = [ordered]@{
    '消费记录' = @('记一笔', '排序', '每页条数', '刷新')
    '数据分析' = @('新增图表', '曲线合计', '月度消费趋势')
    '股票交易' = @('我的账户', '我的持仓', '交易历史', '交易统计', '追加本金')
    '关键事件' = @('添加事件', '上一年', '下一年')
    '日记管理' = @('今天', '收起全部', '跳转到日期', '心情')
    '分类标签' = @('添加分类', '添加标签', '分类', '标签')
    '应用设置' = @('工作空间', '外观', '关闭行为', '开发者工具')
}

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$UIA = [System.Windows.Automation.AutomationElement]

function Get-AppWindow {
    param([int]$ProcessId, [int]$TimeoutSec = 40)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $cond = New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $ProcessId)
        $win = $UIA::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children, $cond)
        if ($win) { return $win }
        Start-Sleep -Milliseconds 500
    }
    return $null
}

function Get-NamedElements {
    param($Window)
    $all = $Window.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    $names = New-Object System.Collections.Generic.List[string]
    foreach ($el in $all) {
        $name = $el.Current.Name
        if ($name -and -not $names.Contains($name)) { $names.Add($name) }
    }
    return $names
}

function Invoke-ByName {
    param($Window, [string]$Name, [int]$TimeoutSec = 20)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $cond = New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $Name)
        $el = $Window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
        if ($el) {
            $pattern = $null
            if ($el.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
                $pattern.Invoke()
                return $true
            }
            if ($el.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) {
                $pattern.Select()
                return $true
            }
            # 自绘按钮可能只暴露 LegacyIAccessible；退回鼠标点击其中心点
            $rect = $el.Current.BoundingRectangle
            if ($rect.Width -gt 0 -and $rect.Height -gt 0 -and (Set-CursorAndClick $rect)) { return $true }
        }
        Start-Sleep -Milliseconds 400
    }
    return $false
}

function Set-CursorAndClick {
    param($Rect)
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class TrMouse {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  public static void Click(int x, int y) {
    SetCursorPos(x, y);
    mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);   // LEFTDOWN
    mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);   // LEFTUP
  }
}
'@ -Language CSharp -ErrorAction SilentlyContinue
    $x = [int]($Rect.X + $Rect.Width / 2)
    $y = [int]($Rect.Y + $Rect.Height / 2)
    [TrMouse]::Click($x, $y)
    return $true
}

$appLog = Join-Path (Split-Path -Parent $Exe) 'logs\app.log'
$failures = New-Object System.Collections.Generic.List[string]

function Start-AppSession {
    param([string]$WorkspaceDir)
    @{ width = 1280; height = 860; workspaceDir = $WorkspaceDir; closeBehavior = 'quit'
       appearance = 'light'; smokeTestMarker = 'ui-smoke.ps1' } |
        ConvertTo-Json | Set-Content -Path (Join-Path $smokeHome '.transactions.json') -Encoding UTF8
    if (Test-Path $appLog) { Remove-Item $appLog -Force }

    $saved = @{ USERPROFILE = $env:USERPROFILE; HOME = $env:HOME }
    try {
        $env:USERPROFILE = $smokeHome
        $env:HOME = $smokeHome
        $proc = Start-Process -FilePath $Exe -PassThru
    }
    finally {
        $env:USERPROFILE = $saved.USERPROFILE
        $env:HOME = $saved.HOME
    }

    $win = Get-AppWindow -ProcessId $proc.Id
    if (-not $win) { throw "启动后 40 秒内没有拿到应用窗口" }
    # UIA 树是首次查询后才由 Chromium 导出的，会被查询本身"唤醒"，因此轮询到有内容为止
    $deadline = (Get-Date).AddSeconds(40)
    do {
        Start-Sleep -Seconds 2
        $names = Get-NamedElements -Window $win
    } while ($names.Count -lt 5 -and (Get-Date) -lt $deadline)

    Write-Host "[ui-smoke] 界面已挂载，UIA 可读元素 $($names.Count) 个" -ForegroundColor Cyan
    return @{ Process = $proc; Window = $win }
}

function Stop-AppSession {
    param($Session)
    if ($Session -and $Session.Process -and -not $Session.Process.HasExited) {
        Stop-Process -Id $Session.Process.Id -Force -ErrorAction SilentlyContinue
        $Session.Process.WaitForExit(5000) | Out-Null
    }
}

# 通过 ValuePattern 写入输入框。注意：Chromium 不会把 DOM 值回读给 UIA
# （`Current.Value` 仍是空），所以**不能**用读回来验证，要靠提交后的界面结果验证。
function Set-ElementValue {
    param($Window, [string]$Name, [string]$Value, [int]$TimeoutSec = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $cond = New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $Name)
        $el = $Window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
        if ($el) {
            $pattern = $null
            if ($el.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
                $pattern.SetValue($Value)
                return $true
            }
        }
        Start-Sleep -Milliseconds 400
    }
    return $false
}

function Invoke-NavigationChecks {
    param($Session)
    $window = $Session.Window
    foreach ($page in $markers.Keys) {
        Write-Host "`n[ui-smoke] 打开页面：$page" -ForegroundColor Cyan
        $clicked = Invoke-ByName -Window $window -Name $page
        if (-not $clicked) {
            $failures.Add("找不到侧栏入口: $page")
            Write-Host "  ✗ 找不到侧栏入口" -ForegroundColor Red
            continue
        }
        Start-Sleep -Milliseconds 1200
        $pageNames = Get-NamedElements -Window $window

        if ($Discover) {
            $file = Join-Path $OutDir "$page.txt"
            $pageNames | Set-Content -Path $file -Encoding UTF8
            Write-Host "  已导出 $($pageNames.Count) 个元素名 → $file" -ForegroundColor DarkGray
        }

        if ($Session.Process.HasExited) { $failures.Add("打开 $page 后进程退出"); break }

        if ($pageNames.Count -lt 5) {
            $failures.Add("$page 页面渲染后 UIA 元素过少（$($pageNames.Count)）")
            Write-Host "  ✗ 元素过少（$($pageNames.Count)）" -ForegroundColor Red
        }
        else {
            Write-Host "  ✓ 页面渲染（$($pageNames.Count) 个元素）" -ForegroundColor Green
        }

        foreach ($marker in $markers[$page]) {
            if ($pageNames -contains $marker) { Write-Host "  ✓ 标记 '$marker'" -ForegroundColor Green }
            else { $failures.Add("$page 缺少标记 '$marker'"); Write-Host "  ✗ 缺少标记 '$marker'" -ForegroundColor Red }
        }
    }
}

# 通过界面记一笔：弹窗 → 填描述/金额 → 确认 → 列表里应出现该记录与金额。
# 这是唯一一条"界面 → IPC → 数据库 → 界面回读"的自动化闭环。
function Invoke-WriteFlow {
    param($Session)
    $window = $Session.Window
    Write-Host "`n[ui-smoke] 写入冒烟：界面记一笔" -ForegroundColor Cyan

    if (-not (Invoke-ByName -Window $window -Name '记一笔' -TimeoutSec 15)) {
        $failures.Add("找不到「记一笔」按钮")
        return
    }
    Start-Sleep -Seconds 2

    # 先拿到两个输入框的引用：写入后 Name（占位符）可能变化，事后再找就不稳了
    $descCond = New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, '描述消费内容')
    $amountCond = New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, '0.00')
    $desc = $window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $descCond)
    $amount = $window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $amountCond)

    $okDesc = $false
    $okAmount = $false
    if ($desc) {
        $vp = $null
        if ($desc.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$vp)) {
            $vp.SetValue($WriteDescription); $okDesc = $true
        }
    }
    if ($amount) {
        $vp = $null
        if ($amount.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$vp)) {
            $vp.SetValue($WriteAmount); $okAmount = $true
        }
    }
    Assert-True $okDesc "填入了描述（$WriteDescription）"
    Assert-True $okAmount "填入了金额（$WriteAmount）"

    if (-not (Invoke-ByName -Window $window -Name '确认' -TimeoutSec 10)) {
        $failures.Add("找不到「确认」按钮")
        return
    }
    Start-Sleep -Seconds 3

    $names = Get-NamedElements -Window $window
    Assert-True ($names -contains $WriteDescription) "列表里出现了新记录（$WriteDescription）"
    Assert-True ([bool]($names | Where-Object { $_ -match [regex]::Escape($WriteAmount) })) "列表里出现了金额 $WriteAmount"
    Assert-True (-not $Session.Process.HasExited) "保存后进程仍然存活"
}

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if ($Condition) { Write-Host "  ✓ $Message" -ForegroundColor Green }
    else { Write-Host "  ✗ $Message" -ForegroundColor Red; $failures.Add($Message) }
}

$session = $null
try {
    $session = Start-AppSession -WorkspaceDir $ws
    Invoke-NavigationChecks -Session $session
}
finally { Stop-AppSession $session }

if ($WriteFlow) {
    # 在**副本**里写入，绝不污染作为对比基线的工作空间
    $writeWs = Join-Path $OutDir 'ws-write'
    if (Test-Path $writeWs) { Remove-Item $writeWs -Recurse -Force }
    Copy-Item $ws $writeWs -Recurse

    $session = $null
    try {
        $session = Start-AppSession -WorkspaceDir $writeWs
        Invoke-WriteFlow -Session $session
    }
    finally { Stop-AppSession $session }
}

if ($failures.Count -gt 0) {
    Write-Host "`n[ui-smoke] ❌ $($failures.Count) 项不通过：" -ForegroundColor Red
    $failures | ForEach-Object { "  - $_" }
    exit 1
}
Write-Host "`n[ui-smoke] ✅ 7 个页面都能打开并渲染" -ForegroundColor Green
exit 0
