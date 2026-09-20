# dev-shot.ps1 —— 对**运行中的 dev 窗口**截图（不重建、不重启、不做发布构建）
#
# 为什么需要它：
#   迭代界面时的"看一眼"不该走 `build/build-ui.ps1` + `cargo build --release`
#   （实测合计约 4 分钟，其中 release 链接 2m40s）。界面跑在 trunk 的 dev 服务上时，
#   改完代码 trunk 会自己重建、`fixtures/dev-hot.ps1` 会让窗口自己刷新 ——
#   这一步只是把当前窗口画到 PNG，耗时 ~1 秒。
#
# 用法（pwsh 7）：
#   pwsh -File fixtures\dev-shot.ps1                       # 抓当前页面 → target\dev-shots\current.png
#   pwsh -File fixtures\dev-shot.ps1 -Page 分类标签         # 先切页再抓 → target\dev-shots\分类标签.png
#   pwsh -File fixtures\dev-shot.ps1 -AllPages             # 5 个顶级功能 + 4 个子功能各抓一张
#   pwsh -File fixtures\dev-shot.ps1 -OutDir target\look    # 换输出目录
#
# 需要 dev 外壳正在运行：
#   pwsh -File fixtures\dev-hot.ps1 -Trunk -Launch -Workspace <你的工作区>
#
# 说明：
#   * 只认**本仓库**的 transactions.exe（别的目录可能也装了同名程序）；
#   * 切页用真实鼠标点侧栏（UIA 的 Invoke 有时对刚渲染的元素无效）；
#   * 抓图优先 `PrintWindow(PW_RENDERFULLCONTENT)`，被别的窗口挡住也能抓到。

param(
    [string]$Page,
    [switch]$AllPages,
    [string]$OutDir,
    [int]$SettleMs = 900
)

$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot 'lib\TrUia.ps1')
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\dev-shots' }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class TrDevShot {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  // 公共 P/Invoke 已统一到 fixtures/lib/TrUia.ps1 的 TrUia（本脚本的调用点用 [TrUia]::…）
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e);
  public static void Click(int x, int y) {
    SetCursorPos(x, y); System.Threading.Thread.Sleep(60);
    mouse_event(2, 0, 0, 0, UIntPtr.Zero); System.Threading.Thread.Sleep(40);
    mouse_event(4, 0, 0, 0, UIntPtr.Zero);
  }
}
'@ -Language CSharp

# 侧栏的 5 个顶级功能 + 记账页的 4 个子功能（子功能走左侧图标条，不进侧栏）
# （原「数据分析」顶级页已并入记账、更名「分析」，所以顶级功能由 6 个减为 5 个）
$ALL_PAGES = @('记账', '记录', '分析', '标签', '模板', '股票', '事件', '日记', '应用设置')

# 子功能 → 该子功能自己的一个标志性控件（判断"真的切过去了"用它，比找同名标题可靠：
# 四个子功能共用标题栏「记账」，而「标签」这类名字在内容里也有同名文字）
$SUB_FUNCTIONS = [ordered]@{
    '记录' = '记一笔'
    '分析' = '新增图表'
    '标签' = '新增分类'
    '模板' = '新建模板'
}

function Get-RepoAppWindow {
    # ⚠ 要**轮询**：窗口刚起来时 UIA 树是惰性构建的（首查常只返回二十来个元素、连侧栏都没有），
    # 一次性查会误判成"没有运行中的实例"（实测踩过：重启 dev 外壳后第一次抓图必失败）。
    param([int]$TimeoutSec = 20)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $procs = @(Get-Process transactions -ErrorAction SilentlyContinue | Where-Object {
                $p = try { $_.Path } catch { $null }
                $p -and $p.StartsWith($repo, [StringComparison]::OrdinalIgnoreCase)
            })
        foreach ($proc in $procs) {
            $cond = New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $proc.Id)
            foreach ($candidate in @($UIA::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $cond))) {
                # 有侧栏「记账」才是主窗口（不是 600×560 的初始化窗口）
                $ok = @($candidate.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                        (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, '记账'))))
                if ($ok.Count -gt 0) { return $candidate }
            }
        }
        Start-Sleep -Milliseconds 700
    } while ((Get-Date) -lt $deadline)
    return $null
}

function Find-NamedElement {
    param($Window, [string]$Name, [int]$TimeoutSec = 8)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        foreach ($el in @($Window.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                    (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $Name))))) {
            if (-not $el.Current.IsOffscreen -and $el.Current.BoundingRectangle.Width -gt 0) { return $el }
        }
        Start-Sleep -Milliseconds 300
    } while ((Get-Date) -lt $deadline)
    return $null
}

function Save-WindowShot {
    param($Window, [string]$Path)
    $handle = [IntPtr]$Window.Current.NativeWindowHandle
    $rect = New-Object TrDevShot+RECT
    [void][TrDevShot]::GetWindowRect($handle, [ref]$rect)
    $w = $rect.Right - $rect.Left
    $h = $rect.Bottom - $rect.Top
    if ($w -le 0 -or $h -le 0) { throw '窗口矩形无效' }
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    $ok = [TrDevShot]::PrintWindow($handle, $hdc, 2)
    $g.ReleaseHdc($hdc)
    if (-not $ok) {
        [void][TrUia]::SetForegroundWindow($handle)
        Start-Sleep -Milliseconds 350
        $g.CopyFromScreen($rect.Left, $rect.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    }
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    return @{ Width = $w; Height = $h; Bytes = (Get-Item $Path).Length }
}

$window = Get-RepoAppWindow
if (-not $window) {
    throw '没找到本仓库的运行实例。先起一个：pwsh -File fixtures\dev-hot.ps1 -Trunk -Launch -Workspace <你的工作区>'
}

$targets = if ($AllPages) { $ALL_PAGES } elseif ($Page) { @($Page) } else { @() }
if ($targets.Count -eq 0) {
    $path = Join-Path $OutDir 'current.png'
    $info = Save-WindowShot -Window $window -Path $path
    Write-Host ("[dev-shot] {0}  {1}x{2}  {3:N0} KB" -f $path, $info.Width, $info.Height, ($info.Bytes / 1KB)) -ForegroundColor Green
    return
}

function Test-PageLoaded {
    # 页面已切换的判据：同名的**页面标题**出现在窗口顶部的**内容区**（不是侧栏）。
    #
    # ⚠ 这里不能简单地"按名字找一个元素、再看它靠不靠左"：侧栏导航项与页面标题**同名**
    # （「消费记录」既是侧栏条目也是标题），而侧栏条目本来就靠左，于是切换失败时判定也会通过 ——
    # 实测就抓出过"请求消费记录、截图却是数据分析"。
    # 正确判据：取同名元素里**最靠右**的那个（标题在内容区；侧栏条目在最左），并确认它在窗口顶部。
    param($Window, [string]$PageName)
    $win = $Window.Current.BoundingRectangle
    $best = $null
    foreach ($el in @($Window.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $PageName))))) {
        $r = $el.Current.BoundingRectangle
        if ($el.Current.IsOffscreen -or $r.Width -le 0 -or $r.Height -le 0) { continue }
        if (($r.Y - $win.Y) -gt 120) { continue }                      # 必须在窗口顶部那条带里
        if (-not $best -or $r.X -gt $best.X) { $best = $r }            # 取最靠右的同名元素 = 内容区标题
    }
    if (-not $best) { return $false }
    return (($best.X - $win.X) -gt 260)                                # 侧栏宽 200 + 余量
}

foreach ($name in $targets) {
    $done = $false
    $subMarker = if ($SUB_FUNCTIONS.Contains($name)) { $SUB_FUNCTIONS[$name] } else { $null }
    for ($attempt = 1; $attempt -le 3 -and -not $done; $attempt++) {
        [void][TrUia]::SetForegroundWindow([IntPtr]$window.Current.NativeWindowHandle)
        Start-Sleep -Milliseconds 150
        if ($subMarker) {
            # 子功能：先回到「记账」页，再点左侧图标条上的那一项
            $nav = Find-NamedElement -Window $window -Name '记账'
            if ($nav) {
                $r = $nav.Current.BoundingRectangle
                [TrDevShot]::Click([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2))
                Start-Sleep -Milliseconds 500
            }
            Invoke-SubFunction -Window $window -Name $name | Out-Null
            $deadline = (Get-Date).AddSeconds(8)
            while ((Get-Date) -lt $deadline -and -not $done) {
                Start-Sleep -Milliseconds 250
                if (Find-First $window $subMarker) { $done = $true }
            }
        } else {
            $nav = Find-NamedElement -Window $window -Name $name
            if (-not $nav) { Write-Host "[dev-shot] ✗ 侧栏里找不到「$name」" -ForegroundColor Yellow; break }
            $r = $nav.Current.BoundingRectangle
            [TrDevShot]::Click([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2))
            # 轮询等页面真的切过去（固定 sleep 会抓到上一页：实测「数据分析」抓成了「分类标签」）
            $deadline = (Get-Date).AddSeconds(4)
            while ((Get-Date) -lt $deadline) {
                Start-Sleep -Milliseconds 250
                if (Test-PageLoaded -Window $window -PageName $name) { $done = $true; break }
            }
        }
        if (-not $done) { Write-Host "[dev-shot]   第 $attempt 次点击后页面没切过去，重试" -ForegroundColor DarkYellow }
    }
    if (-not $done) { Write-Host "[dev-shot] ✗ 没切到「$name」，跳过" -ForegroundColor Yellow; continue }
    Start-Sleep -Milliseconds $SettleMs
    $path = Join-Path $OutDir "$name.png"
    $info = Save-WindowShot -Window $window -Path $path
    Write-Host ("[dev-shot] {0}  {1}x{2}  {3:N0} KB" -f $path, $info.Width, $info.Height, ($info.Bytes / 1KB)) -ForegroundColor Green
}
