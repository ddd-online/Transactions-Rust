# ui-drag.ps1 —— 拖拽排序的端到端验收（真实鼠标输入驱动 HTML5 拖放）。
#
# 为什么需要它：拖动排序曾经被当成"自动化不了"的人工项（理由是"HTML5 DnD 合成不了"）。
# 实际上 **OS 级鼠标输入对 Chromium 就是真拖拽**（`mouse_event` 按下 → 分段移动 → 抬起），
# 所以可以自动化；真正做不到的是"派发 JS 合成事件"。
#
# 这条脚本还锁住了一个**曾经的真实缺陷**：Tauri 默认 `dragDropEnabled: true`，
# wry 会在 WebView2 宿主 HWND 上 `RegisterDragDrop` 并 `SetAllowExternalDrop(false)`；
# 而 Chromium 在 Windows 上的**页内拖拽也走 OLE 拖放**，于是 `drop` 永远到不了页面 ——
# 表现就是"分类/标签/模板拖不动"。修复是在建窗口时调 `disable_drag_drop_handler()`
# （见 `src-tauri/src/shell.rs` 的 `create_main_window`），本脚本就是它的回归测试。
#
# 用法（pwsh 7；需要 release 产物；**不能有另一个实例在跑**——单实例插件会拦掉）：
#   pwsh -File fixtures/ui-drag.ps1
#   pwsh -File fixtures/ui-drag.ps1 -Workspace <既有工作空间>   # 会改里面的 sort_order，建议给副本
#
# 隔离：与 smoke.ps1 一致，用临时 USERPROFILE 启动，不碰你真实的 ~/.transactions.json。

param(
    [string]$Exe,
    [string]$SmokeHome,
    [string]$Workspace,
    [string]$OutDir
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $repo 'target\release\transactions.exe' }
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\drag-smoke\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\drag-smoke' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$smokeHome = [System.IO.Path]::GetFullPath($SmokeHome)
# `-OutDir`/工作空间都归一化成绝对路径：配置里写的是 `workspaceDir`，相对路径会依赖应用的当前目录
# （同一类坑在选目录框那边踩过：对话框按自己的当前目录解析相对路径）。
$OutDir = [System.IO.Path]::GetFullPath($OutDir)
if ($smokeHome -eq [System.IO.Path]::GetFullPath($env:USERPROFILE)) {
    throw "拒绝把临时 HOME 指到真实用户目录 —— 冒烟会改写你的配置"
}
if (-not (Test-Path $smokeHome)) { New-Item -ItemType Directory -Force -Path $smokeHome | Out-Null }
# 文件框等原生对话框的起始目录需要"桌面"存在（保持与本目录其它脚本一致的临时 HOME 形状）
if (-not (Test-Path (Join-Path $smokeHome 'Desktop'))) {
    New-Item -ItemType Directory -Force -Path (Join-Path $smokeHome 'Desktop') | Out-Null
}
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Force -Path $OutDir | Out-Null }

# 单实例插件按 identifier 判重：只要**本仓库里任何一个构建**在跑（`target\release\transactions.exe`
# 或 `build\target\*.exe`），这次启动就会被顶掉，表现是"窗口 40 秒都没出现"。
#
# ⚠ 判重**必须按完整路径**，不能按进程名（本机别的目录下可能有同名 exe），
# 按名字判会把人家的进程算进来——它跟我们的 identifier 毫无关系，误判会直接卡住本脚本。
$repoPrefix = $repo.TrimEnd('\') + '\'
$blockers = @(Get-Process -Name transactions -ErrorAction SilentlyContinue | Where-Object {
    $path = try { $_.Path } catch { $null }
    $path -and $path.StartsWith($repoPrefix, [System.StringComparison]::OrdinalIgnoreCase)
})
if ($blockers.Count -gt 0) {
    $paths = $blockers | ForEach-Object { try { $_.Path } catch { '(unknown)' } }
    throw "本仓库已有 Transactions 实例在运行（PID $($blockers.Id -join ', ')）：$($paths -join ' / ')`n" +
        "单实例插件会顶掉本次启动，请先退掉它们（注意：隐藏到托盘也算在运行）。"
}

$ws = [System.IO.Path]::GetFullPath($Workspace)

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
$UIA = [System.Windows.Automation.AutomationElement]

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class TrDragMouse {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  [DllImport("user32.dll")] public static extern IntPtr SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
  public static void Click(int x, int y) {
    SetCursorPos(x, y);
    mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);   // LEFTDOWN
    mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);   // LEFTUP
  }
  public static void Move(int x, int y) { SetCursorPos(x, y); }
  public static void ButtonDown() { mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero); }
  public static void ButtonUp() { mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero); }
}
'@ -Language CSharp -ErrorAction SilentlyContinue

$failures = New-Object System.Collections.Generic.List[string]
function Assert-True {
    param([bool]$Condition, [string]$Message)
    if ($Condition) { Write-Host "  ✓ $Message" -ForegroundColor Green }
    else { Write-Host "  ✗ $Message" -ForegroundColor Red; $failures.Add($Message) }
}

function Get-Elements { param($Root)
    return $Root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
}
function Find-First { param($Root, [string]$Name)
    $all = $Root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $Name)))
    if ($all.Count -eq 0) { return $null }
    return $all[0]
}
function Invoke-Element { param($Element)
    if (-not $Element) { return $false }
    $pattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
        $pattern.Invoke(); return $true
    }
    $rect = $Element.Current.BoundingRectangle
    if ($rect.Width -gt 0) {
        [TrDragMouse]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
        return $true
    }
    return $false
}

# 列表项要**轮询等待**：UIA 树是惰性构建的，切页之后立刻查会"找不到分类"
# （实测踩过：报「界面上找不到源项」但界面其实正常）。
function Wait-Element { param($Window, [string]$Name, [int]$TimeoutSec = 25)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $element = Find-First $Window $Name
        if ($element) { return $element }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    return $null
}
function Save-Screenshot { param([string]$Path)
    try {
        $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
        $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        $graphics.CopyFromScreen($bounds.X, $bounds.Y, 0, 0, $bounds.Size)
        $graphics.Dispose()
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
        $bitmap.Dispose()
        Write-Host "  失败截图: $Path" -ForegroundColor DarkYellow
    }
    catch { Write-Host "  截图失败: $_" -ForegroundColor DarkYellow }
}

function Read-Categories {
    $dump = Join-Path $OutDir 'categories.json'
    Push-Location $repo
    & cargo -q xtask dump $ws --table tbl_billadm_category *> $dump
    $exit = $LASTEXITCODE
    Pop-Location
    if ($exit -ne 0) { throw "导出分类失败（exit=$exit）" }
    return @((Get-Content $dump -Raw | ConvertFrom-Json).'tbl_billadm_category' |
        Where-Object { $_.transaction_type -eq 'expense' } | Sort-Object sort_order)
}

# 把某个列表项拖到另一个列表项的位置（真实鼠标：按下 → 分段移动 → 抬起）
#
# 起手点用**行文字的中心**而不是行左边缘：左边缘 20px 处是 `.ct-drag-handle`，
# 实测从那里起手偶尔不会进入拖拽（手柄上有 `touch-action: none`），从文字中心起手稳定。
# 移动分段要够密（20 段 × 60ms）：Chromium 需要看到 pointer 连续移动才启动 HTML5 拖拽。
function Invoke-DragTo {
    param($Window, [string]$FromName, [string]$ToName)
    $from = Wait-Element -Window $Window -Name $FromName
    $to = Wait-Element -Window $Window -Name $ToName
    if (-not $from) { throw "界面上找不到源项「$FromName」" }
    if (-not $to) { throw "界面上找不到目标项「$ToName」" }
    $fromRect = $from.Current.BoundingRectangle
    $toRect = $to.Current.BoundingRectangle
    $fromX = [int]($fromRect.X + $fromRect.Width / 2)
    $fromY = [int]($fromRect.Y + $fromRect.Height / 2)
    $toX = [int]($toRect.X + $toRect.Width / 2)
    $toY = [int]($toRect.Y + $toRect.Height / 2)
    [TrDragMouse]::Move($fromX, $fromY)
    Start-Sleep -Milliseconds 250
    [TrDragMouse]::ButtonDown()
    Start-Sleep -Milliseconds 350
    for ($step = 1; $step -le 20; $step++) {
        [TrDragMouse]::Move(
            [int]($fromX + ($toX - $fromX) * $step / 20),
            [int]($fromY + ($toY - $fromY) * $step / 20))
        Start-Sleep -Milliseconds 60
    }
    Start-Sleep -Milliseconds 400
    [TrDragMouse]::ButtonUp()
    Start-Sleep -Seconds 3
}

# ---- 播种 ----
if (-not $Workspace -or -not (Test-Path (Join-Path $ws 'transactions.db'))) {
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[ui-drag] 播种工作空间: $ws" -ForegroundColor Cyan
}

@{ width = 1280; height = 860; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'ui-drag.ps1' } |
    ConvertTo-Json | Set-Content -Path (Join-Path $smokeHome '.transactions.json') -Encoding UTF8

$process = $null
try {
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

    $deadline = (Get-Date).AddSeconds(40)
    $window = $null
    while ((Get-Date) -lt $deadline -and -not $window) {
        $window = $UIA::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children,
            (New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $process.Id)))
        if (-not $window) { Start-Sleep -Milliseconds 500 }
    }
    if (-not $window) { throw '启动后 40 秒内没有拿到应用窗口' }

    $deadline = (Get-Date).AddSeconds(40)
    do {
        Start-Sleep -Seconds 1
        $elements = Get-Elements $window
    } while ($elements.Count -lt 8 -and (Get-Date) -lt $deadline)
    Write-Host "[ui-drag] UIA 可读元素 $($elements.Count) 个" -ForegroundColor Cyan

    $hwnd = [IntPtr]$window.Current.NativeWindowHandle
    [TrDragMouse]::ShowWindow($hwnd, 9) | Out-Null   # SW_RESTORE
    [TrDragMouse]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 500

    Write-Host "`n[ui-drag] 1/2 打开「分类标签」并读取初始顺序"
    Assert-True (Invoke-Element (Find-First $window '分类标签')) '打开「分类标签」页'
    Start-Sleep -Seconds 3

    $before = Read-Categories
    Assert-True ($before.Count -ge 3) "消费分类至少 3 个（实际 $($before.Count)）"
    if ($before.Count -lt 3) { throw '种子里消费分类不足 3 个，无法验证换位' }
    Write-Host ("  初始顺序: " + (($before | ForEach-Object { "$($_.sort_order)=$($_.name)" }) -join ' → '))

    # 界面顺序（UIA 文本）必须与库里的 sort_order 顺序一致
    $namesInUi = @(Get-Elements $window | ForEach-Object { $_.Current.Name } | Where-Object { $_ })
    $uiIndexes = @($before | ForEach-Object {
        $found = $false
        for ($i = 0; $i -lt $namesInUi.Count; $i++) {
            if ($namesInUi[$i] -eq $_.name) { $found = $i; break }
        }
        if ($found) { $found } else { -1 }
    })
    $uiOrderMatchesDb = ($uiIndexes -notcontains -1) -and
        (@(0..($uiIndexes.Count - 2)) | ForEach-Object { $uiIndexes[$_] -lt $uiIndexes[$_ + 1] } | Where-Object { -not $_ }).Count -eq 0
    Assert-True $uiOrderMatchesDb '界面顺序与库里 sort_order 一致（起点对齐）'

    Write-Host "`n[ui-drag] 2/2 把第 1 项拖到第 3 项"
    $source = $before[0].name
    $target = $before[2].name
    [TrDragMouse]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 300
    Invoke-DragTo -Window $window -FromName $source -ToName $target

    $after = Read-Categories
    Write-Host ("  新顺序: " + (($after | ForEach-Object { "$($_.sort_order)=$($_.name)" }) -join ' → '))
    $expected = @($before[1].name, $before[2].name, $before[0].name) + @($before | Select-Object -Skip 3 | ForEach-Object { $_.name })
    $actual = @($after | ForEach-Object { $_.name })
    Assert-True (($actual -join ',') -eq ($expected -join ',')) "拖拽后顺序为 [1,2,3…] → [2,3,1…]（期望 $($expected -join ',')）"

    # 落库的 sort_order 会被重排成 0..N-1（`sortOrder !== i` 语义），
    # 注意种子数据本身是 1..N（`max+1`），所以这里断言的是**重排之后**的稠密 0 基。
    $sortOrders = @($after | ForEach-Object { $_.sort_order })
    $dense = ($sortOrders -join ',') -eq ((0..($sortOrders.Count - 1)) -join ',')
    Assert-True $dense "sort_order 落成 0..N-1（实际 $($sortOrders -join ',')）"

    # 界面也要跟着变（本地写回新顺序）
    Start-Sleep -Seconds 1
    $namesAfter = @(Get-Elements $window | ForEach-Object { $_.Current.Name } | Where-Object { $_ })
    $firstIndexAfter = [array]::IndexOf($namesAfter, $after[0].name)
    $thirdIndexAfter = [array]::IndexOf($namesAfter, $before[0].name)
    Assert-True ($firstIndexAfter -ge 0 -and $thirdIndexAfter -gt $firstIndexAfter) '界面上被拖动的项确实排到了后面'

    # 切走再回来：界面顺序必须仍然等于库里的顺序（这条能抓住"列表没按 sort_order 排"、
    # 或者"只在本地改了没落库"这类问题——用户看到的就是"拖了没用/一刷新就弹回去"）。
    Invoke-Element (Find-First $window '消费记录') | Out-Null
    Start-Sleep -Seconds 2
    Invoke-Element (Find-First $window '分类标签') | Out-Null
    Start-Sleep -Seconds 3
    $namesReopened = @(Get-Elements $window | ForEach-Object { $_.Current.Name } | Where-Object { $_ })
    $reopenedIndexes = @($after | ForEach-Object {
        [array]::IndexOf($namesReopened, $_.name)
    })
    $reopenedMatches = ($reopenedIndexes -notcontains -1 -and
        (@(0..($reopenedIndexes.Count - 2)) | ForEach-Object { $reopenedIndexes[$_] -lt $reopenedIndexes[$_ + 1] } |
            Where-Object { -not $_ }).Count -eq 0)
    Assert-True $reopenedMatches '重新进入页面后顺序仍与库里一致（真的落库了，不是只改了本地）'
}
finally {
    if ($failures.Count -gt 0) { Save-Screenshot (Join-Path $OutDir 'failure.png') }
    if ($process -and -not $process.HasExited) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        $process.WaitForExit(5000) | Out-Null
    }
}

Write-Host ''
if ($failures.Count -gt 0) {
    Write-Host "[ui-drag] 失败 $($failures.Count) 项：" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "   - $_" -ForegroundColor Red }
    exit 1
}
Write-Host '[ui-drag] 全部通过：真实鼠标拖拽 → 顺序变化 → sort_order 落库 → 界面同步' -ForegroundColor Green
