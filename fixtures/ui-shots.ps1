# ui-shots.ps1 —— 逐页**像素级**验证 + 主题切换验证，并留下可眼看的截图。
#
# 为什么需要它（补 UIA 的盲区）：
#   * `ui-smoke.ps1` 用 UI Automation 证明"页面结构在"，但 UIA 树里元素齐全、屏幕上却可能是一片空白
#     （CSS 没生效、wasm 渲染失败、图片全是碎图）。这里直接抓窗口像素来兜底。
#   * 主题切换只能看颜色：这里抓浅色/深色两张图，用平均亮度差来断言 `<html data-theme>` 真的生效。
#   * 顺带产出 14 张 PNG（5 个顶级功能 + 记账 4 个子功能 + 股票 5 个子功能）：
#     人工验收时不用逐个点开页面，先翻图找可疑点即可。
#
# 用法（pwsh 7；需要内嵌界面的产物，见 AGENTS.md 的 custom-protocol 说明）：
#   pwsh -File fixtures/ui-shots.ps1 -Workspace target\smoke\ws-write
#   pwsh -File fixtures/ui-shots.ps1 -Workspace <ws> -OutDir docs\screenshots
#
# 判据（每个页面）：像素标准差 > 8（不是纯色块）、不同颜色数 > 20。

param(
    [string]$Exe,
    [string]$SmokeHome,
    [string]$Workspace,
    [string]$OutDir,
    [int]$BootWaitSec = 15
)

$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot 'lib\TrUia.ps1')

$repo = Split-Path -Parent $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $repo 'build\target\transactions.exe' }
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\smoke\home-shot' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\ui-shots' }
if (-not $Workspace) { $Workspace = Join-Path $repo 'target\smoke\ws-write' }
if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe" }

$smokeHome = [System.IO.Path]::GetFullPath($SmokeHome)
if ($smokeHome -eq [System.IO.Path]::GetFullPath($env:USERPROFILE)) { throw "拒绝把临时 HOME 指到真实用户目录" }
if (-not (Test-Path $smokeHome)) { New-Item -ItemType Directory -Force -Path $smokeHome | Out-Null }
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Force -Path $OutDir | Out-Null }

$ws = [System.IO.Path]::GetFullPath($Workspace)
if (-not (Test-Path (Join-Path $ws 'transactions.db'))) { throw "工作空间里没有 transactions.db: $ws" }

$exeFull = [System.IO.Path]::GetFullPath($Exe)
$sameExe = Get-Process -Name transactions -ErrorAction SilentlyContinue | Where-Object {
    $path = try { $_.Path } catch { $null }
    $path -and ([System.IO.Path]::GetFullPath($path) -eq $exeFull)
}
if ($sameExe) { throw "同一个可执行文件已有实例在运行（PID $($sameExe.Id -join ', ')），请先关掉" }

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class TrShot {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  // 公共 P/Invoke 已统一到 fixtures/lib/TrUia.ps1 的 TrUia（本脚本的调用点用 [TrUia]::…）
}
'@ -Language CSharp

$pages = @('记账', '股票', '事件', '日记', '应用设置')
# 有子功能的顶级页（左侧图标条切换）：每个子功能各抓一张，界面上它们是不同的内容块。
# 「分析」原为顶级页「数据分析」，已并入记账并更名；股票页的五个子功能同理走图标条
# （原顶部页签「账户/持仓/成交记录/交易统计」改成了图标条，并多了一个「设置」）。
$subPages = [ordered]@{
    '记账' = @('记录', '分析', '标签', '模板')
    '股票' = @('账户', '持仓', '记录', '统计', '设置')
}
# 子功能 → 判定"真的切过去了"的标志控件。
# ⚠ 必须选**任何数据状态下都在**的元素：统计子功能没有结算记录时走空态面板，
#   里面的标题不渲染；这里用工具栏上的控件（工具栏恒在）。
$subMarkers = [ordered]@{
    '记账-记录' = '记一笔'; '记账-分析' = '新增图表'; '记账-标签' = '新增分类'; '记账-模板' = '新建模板'
    '股票-账户' = '追加本金'; '股票-持仓' = '建仓'; '股票-记录' = '已实现盈亏'
    '股票-统计' = '刷新'; '股票-设置' = '交易费用设置'
}
$failures = New-Object System.Collections.Generic.List[string]
$rows = New-Object System.Collections.Generic.List[object]

function Get-AppWindow {
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

function Invoke-ByName {
    param($Window, [string]$Name, [int]$TimeoutSec = 20)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $cond = New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $Name)
        $el = $Window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
        if ($el) {
            $p = $null
            if ($el.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$p)) { $p.Invoke(); return $true }
            if ($el.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$p)) { $p.Select(); return $true }
        }
        Start-Sleep -Milliseconds 400
    }
    return $false
}

# 抓窗口内容（PrintWindow 优先，遮挡也能抓；失败再退到屏幕捕获）并返回统计
function Save-WindowShot {
    param([IntPtr]$Handle, [string]$Path)
    $rect = New-Object TrShot+RECT
    [void][TrShot]::GetWindowRect($Handle, [ref]$rect)
    $w = $rect.Right - $rect.Left
    $h = $rect.Bottom - $rect.Top
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    $ok = [TrShot]::PrintWindow($Handle, $hdc, 2)   # PW_RENDERFULLCONTENT
    $g.ReleaseHdc($hdc)
    if (-not $ok) {
        [void][TrUia]::SetForegroundWindow($Handle)
        Start-Sleep -Milliseconds 400
        $g.CopyFromScreen($rect.Left, $rect.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    }
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose()

    $colors = @{}
    $sum = 0.0; $sum2 = 0.0; $n = 0
    for ($y = 0; $y -lt $h; $y += 8) {
        for ($x = 0; $x -lt $w; $x += 8) {
            $c = $bmp.GetPixel($x, $y)
            $lum = 0.299 * $c.R + 0.587 * $c.G + 0.114 * $c.B
            $sum += $lum; $sum2 += $lum * $lum; $n++
            $colors["$($c.R),$($c.G),$($c.B)"] = 1
        }
    }
    $bmp.Dispose()
    $mean = $sum / $n
    return @{
        Width    = $w
        Height   = $h
        Mean     = [math]::Round($mean, 1)
        StdDev   = [math]::Round([math]::Sqrt(($sum2 / $n) - ($mean * $mean)), 1)
        Colors   = $colors.Count
        Bytes    = (Get-Item $Path).Length
    }
}

# ---------- 启动 ----------
@{ width = 1280; height = 860; workspaceDir = $ws; closeBehavior = 'quit'; appearance = 'light'
   smokeTestMarker = 'ui-shots.ps1' } |
    ConvertTo-Json | Set-Content -Path (Join-Path $smokeHome '.transactions.json') -Encoding UTF8

$saved = @{ USERPROFILE = $env:USERPROFILE; HOME = $env:HOME }
try {
    $env:USERPROFILE = $smokeHome
    $env:HOME = $smokeHome
    $process = Start-Process -FilePath $Exe -PassThru
}
finally {
    $env:USERPROFILE = $saved.USERPROFILE
    $env:HOME = $saved.HOME
}

try {
    $window = Get-AppWindow -ProcessId $process.Id
    if (-not $window) { throw "启动后 60 秒内没有拿到应用窗口" }
    Write-Host "[ui-shots] 等界面挂载（冷启动首个工作空间较慢）…" -ForegroundColor Cyan
    Start-Sleep -Seconds $BootWaitSec
    $handle = [IntPtr]$window.Current.NativeWindowHandle
    if ($handle -eq [IntPtr]::Zero) { throw "拿不到窗口句柄" }

    foreach ($page in $pages) {
        if (-not (Invoke-ByName -Window $window -Name $page)) {
            $failures.Add("找不到侧栏入口: $page")
            continue
        }
        Start-Sleep -Milliseconds 1500
        $file = Join-Path $OutDir "$page.png"
        $stat = Save-WindowShot -Handle $handle -Path $file
        $blank = ($stat.StdDev -lt 8) -or ($stat.Colors -lt 20)
        if ($blank) { $failures.Add("$page 疑似空白：标准差 $($stat.StdDev)、颜色数 $($stat.Colors)") }
        $rows.Add([pscustomobject]@{
                Page = $page; StdDev = $stat.StdDev; Colors = $stat.Colors
                Mean = $stat.Mean; PngKB = [math]::Round($stat.Bytes / 1KB, 1); Blank = $blank
            })
    }

    # 各页的子功能：走左侧图标条（不是侧栏）。先回**父页**，再点子功能，
    # 最后轮询它的标志控件确认真的切过去了（空态页尤其必要）。
    foreach ($parent in $subPages.Keys) {
        foreach ($page in $subPages[$parent]) {
            Invoke-ByName -Window $window -Name $parent | Out-Null
            Start-Sleep -Milliseconds 800
            if (-not (Invoke-SubFunction -Window $window -Name $page)) {
                $failures.Add("找不到 $parent 子功能入口: $page")
                continue
            }
            $marker = $subMarkers["$parent-$page"]
            $deadline = (Get-Date).AddSeconds(8)
            while ((Get-Date) -lt $deadline -and -not (Find-First $window $marker)) {
                Start-Sleep -Milliseconds 300
            }
            Start-Sleep -Milliseconds 1200
            $file = Join-Path $OutDir "$parent-$page.png"
            $stat = Save-WindowShot -Handle $handle -Path $file
            $blank = ($stat.StdDev -lt 8) -or ($stat.Colors -lt 20)
            if ($blank) { $failures.Add("$parent·$page 疑似空白：标准差 $($stat.StdDev)、颜色数 $($stat.Colors)") }
            $rows.Add([pscustomobject]@{
                    Page = "$parent·$page"; StdDev = $stat.StdDev; Colors = $stat.Colors
                    Mean = $stat.Mean; PngKB = [math]::Round($stat.Bytes / 1KB, 1); Blank = $blank
                })
        }
    }

    # ---------- 主题切换 ----------
    Write-Host "[ui-shots] 主题切换验证" -ForegroundColor Cyan
    if (-not (Invoke-ByName -Window $window -Name '应用设置')) { $failures.Add('找不到「应用设置」入口') }
    else {
        Start-Sleep -Milliseconds 1200
        $light = $null; $dark = $null
        if (Invoke-ByName -Window $window -Name '浅色') {
            Start-Sleep -Seconds 2
            $light = Save-WindowShot -Handle $handle -Path (Join-Path $OutDir 'theme-light.png')
        }
        else { $failures.Add('找不到「浅色」按钮') }
        if (Invoke-ByName -Window $window -Name '深色') {
            Start-Sleep -Seconds 2
            $dark = Save-WindowShot -Handle $handle -Path (Join-Path $OutDir 'theme-dark.png')
        }
        else { $failures.Add('找不到「深色」按钮') }

        if ($light -and $dark) {
            $delta = [math]::Round($light.Mean - $dark.Mean, 1)
            Write-Host "  浅色平均亮度 $($light.Mean) / 深色 $($dark.Mean) / 差 $delta" -ForegroundColor Cyan
            if ($delta -lt 60) { $failures.Add("主题切换后亮度差过小（$delta），深色主题可能没生效") }
            if ($dark.StdDev -lt 8) { $failures.Add('深色主题截图疑似空白') }
        }
    }
}
finally {
    if ($process -and -not $process.HasExited) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        $process.WaitForExit(5000) | Out-Null
    }
}

$rows | Format-Table -AutoSize | Out-String | Write-Host
Write-Host "[ui-shots] 截图目录: $OutDir" -ForegroundColor DarkGray

if ($failures.Count -gt 0) {
    Write-Host "[ui-shots] ❌ $($failures.Count) 项不通过：" -ForegroundColor Red
    $failures | ForEach-Object { "  - $_" }
    exit 1
}
Write-Host "[ui-shots] ✅ 5 个顶级功能 + 4 个子功能都有真实像素内容，主题切换生效" -ForegroundColor Green
exit 0
