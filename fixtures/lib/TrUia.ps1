# =============================================================================
# fixtures/lib/TrUia.ps1 —— fixtures/*.ps1 的**共用 UIA 底座**（dot-source，不是 Import-Module）
#
# 用法：每个脚本在 `param()` 之后、任何 helper 定义/调用之前写一行
#     . (Join-Path $PSScriptRoot 'lib\TrUia.ps1')
#
# 为什么用 dot-source 而不是 Import-Module：脚本自己的 `$failures` / `$UIA` / 辅助函数都在
# **脚本作用域**。dot-source 让本文件里的函数在同一个作用域里定义，于是
#   * `Assert-True` 能沿动态作用域取到调用方的 `$failures`（下面有防"静默假绿"的守卫）；
#   * `$UIA`、`[TrUia]` 对所有脚本里定义的函数可见（模块作用域会看不到调用方的变量）。
#
# 抽取原则：**只并逐字相同的实现**。各脚本里语义不同的变体（更严格的守卫、不同的默认超时、
# 不同的点击时序、不同的提示文案）一律留在脚本本地 —— 脚本里后定义的同名函数会覆盖本文件，
# 这是刻意的，不要"顺手统一成较强的那个"（那会把假红变绿）。
#
# 边界：单独把某个 .ps1 拷出仓库将无法运行（缺 fixtures/lib/）。
# =============================================================================

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

# UIA 类型加速器用到的类型对象（原本每个脚本各写一遍，现在统一在本文件里落到脚本作用域）
$UIA = [System.Windows.Automation.AutomationElement]

# 公共鼠标 P/Invoke（原来 8 份逐字相同的类的公共部分 + 其余脚本里"超集"类的公共部分）。
# 各脚本里**语义不同**的 Click（例如 close-behavior 的 down/up 之间 Sleep(80)、
# dev-shot 的 Sleep(60)/Sleep(40)）不并进来，仍留在各自脚本里。
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class TrUia {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  [DllImport("user32.dll")] public static extern IntPtr SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
  public static void Click(int x, int y) {
    SetCursorPos(x, y);
    mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
    mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
  }
}
'@ -Language CSharp -ErrorAction SilentlyContinue

# ---------------------------------------------------------------- 断言与汇总

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if ($Condition) { Write-Host "  ✓ $Message" -ForegroundColor Green }
    else {
        Write-Host "  ✗ $Message" -ForegroundColor Red
        # 动态作用域取调用方脚本作用域里的 $failures（各脚本自己 New-Object 那个列表）。
        # 取不到就**抛错**：否则断言会"看着失败但不计数"，整轮变成静默假绿。
        $list = Get-Variable -Name failures -ValueOnly -ErrorAction SilentlyContinue
        if ($null -eq $list) { throw 'Assert-True 找不到调用方脚本作用域的 $failures（脚本必须自己建这个列表）' }
        $list.Add($Message)
    }
}

# 末尾汇总：失败 → 逐条打印 + exit 1；全部通过 → 打印脚本自己的成功文案。
# 退出码与文案都由调用方给定，本函数不改变任何一方的语义。
function Show-TrSummary {
    param($Failures, [string]$Tag, [string]$SuccessMessage)
    Write-Host ''
    if ($Failures.Count -gt 0) {
        Write-Host "[$Tag] 失败 $($Failures.Count) 项：" -ForegroundColor Red
        $Failures | ForEach-Object { Write-Host "   - $_" -ForegroundColor Red }
        exit 1
    }
    Write-Host $SuccessMessage -ForegroundColor Green
}

# ---------------------------------------------------------------- 基础 UIA 查询

function Get-Elements { param($Root)
    return $Root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
}

function Find-First { param($Root, [string]$Name)
    $all = $Root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $Name)))
    if ($all.Count -eq 0) { return $null }
    return $all[0]
}

function Find-All { param($Root, [string]$Name)
    return @($Root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $Name))))
}

function Find-Like { param($Root, [string]$Pattern)
    foreach ($element in @(Get-Elements $Root)) {
        $name = $element.Current.Name
        if ($name -and $name.Contains($Pattern)) { return $element }
    }
    return $null
}

function Find-ElementLike { param($Root, [string]$Pattern)
    foreach ($element in @($Root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition))) {
        $name = $element.Current.Name
        if ($name -and $name.Contains($Pattern)) { return $element }
    }
    return $null
}

# 空矩形元素（没真正渲染出来）会报 ±∞：`[int]` 转换会抛异常，必须先显式排除非有限值。
function Test-Rect { param($Rect)
    foreach ($value in @($Rect.X, $Rect.Y, $Rect.Width, $Rect.Height)) {
        if ([double]::IsNaN($value) -or [double]::IsInfinity($value)) { return $false }
    }
    return ($Rect.Width -gt 0 -and $Rect.Height -gt 0)
}

function Wait-Element { param($Root, [string]$Name, [int]$TimeoutSec = 25)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $element = Find-First $Root $Name
        if ($element) { return $element }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    return $null
}

function Wait-Like { param($Root, [string]$Pattern, [int]$TimeoutSec = 25)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $element = Find-Like -Root $Root -Pattern $Pattern
        if ($element) { return $element }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    return $null
}

# 启动时要抓**主窗口**（含侧栏），不是"该进程的第一个窗口"：
# 启动期会先出现 600×560 的初始化窗口，抓到它后面所有按名字的查找都会落空。
# 注意与 `Get-AppWindow`（第一个顶层窗口）是**两个不同的判据**，不要互相替换。
function Get-ReadyWindow { param([int]$ProcessId, [int]$TimeoutSec = 60)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    $last = $null
    while ((Get-Date) -lt $deadline) {
        $candidate = $UIA::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children,
            (New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $ProcessId)))
        if ($candidate) {
            $last = $candidate
            # 侧栏条目出现即说明是主窗口且界面已挂载（'记账' 是默认页）
            if (Find-First $candidate '记账') { return $candidate }
        }
        Start-Sleep -Milliseconds 500
    }
    return $last
}

# ---------------------------------------------------------------- 激活与取值

function Set-Value { param($Element, [string]$Value)
    if (-not $Element) { return $false }
    $pattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
        $pattern.SetValue($Value); return $true
    }
    return $false
}

# 优先 InvokePattern，退而求其次真实鼠标点中心。
# 注意：ui-crud / ui-transactions 的本地版本更严（try/catch + IsFinite 守卫），刻意保留。
function Invoke-Element { param($Element)
    if (-not $Element) { return $false }
    $pattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
        $pattern.Invoke(); return $true
    }
    $rect = $Element.Current.BoundingRectangle
    if ($rect.Width -gt 0) {
        [TrUia]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
        return $true
    }
    return $false
}

function Click-Element { param($Element)
    if (-not $Element) { return $false }
    $rect = $Element.Current.BoundingRectangle
    if ($rect.Width -le 0) { return $false }
    [TrUia]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
    return $true
}

# 记账页左侧的**子功能图标条**：点一个子功能（记录 / 标签 / 模板）。
#
# 为什么不能直接用 `Find-First`/`Invoke-Element` 按名字取第一个同名元素：
# 子功能名（尤其「标签」）和页面内容里的文字会重名（标签栏的标题也叫「标签」），
# 而 UIA 的查询是**取第一个匹配**，撞上文字元素（没有 Invoke 模式）就会卡住不动。
# 这里改成"按名字找 Button、取**最靠左**的那个" —— 图标条在版心的最左边，位置本身即判据。
function Invoke-SubFunction { param($Window, [string]$Name, [int]$TimeoutSec = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $cond = New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $Name)
        $candidates = @($Window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)) |
            Where-Object {
                $_.Current.ControlType.ProgrammaticName -eq 'ControlType.Button' -and
                (Test-Rect $_.Current.BoundingRectangle)
            }
        $button = $candidates | Sort-Object { $_.Current.BoundingRectangle.X } | Select-Object -First 1
        if ($button) {
            $pattern = $null
            if ($button.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
                $pattern.Invoke()
            } else {
                Click-Element $button | Out-Null
            }
            return $true
        }
        Start-Sleep -Milliseconds 400
    }
    return $false
}

# 行内操作按钮：先把指针移到行上（操作区只在 :hover 时 display:flex，不进 UIA 树），
# 再按"中心 Y 最接近该行"挑同名按钮。
function Find-RowButton { param($Window, [string]$RowText, [string]$ButtonName)
    $row = Wait-Like -Root $Window -Pattern $RowText -TimeoutSec 15
    if (-not $row) {
        Write-Host "    找不到行元素「$RowText」" -ForegroundColor DarkYellow
        return $null
    }
    $rowRect = $row.Current.BoundingRectangle
    [TrUia]::SetCursorPos([int]($rowRect.X + $rowRect.Width / 2), [int]($rowRect.Y + $rowRect.Height / 2)) | Out-Null
    Start-Sleep -Milliseconds 400
    $rowCenter = $rowRect.Y + $rowRect.Height / 2
    $best = $null; $bestDistance = [double]::MaxValue
    foreach ($button in (Find-All $Window $ButtonName)) {
        if ($button.Current.IsOffscreen) { continue }
        $rect = $button.Current.BoundingRectangle
        if ($rect.Width -le 0) { continue }
        $distance = [Math]::Abs(($rect.Y + $rect.Height / 2) - $rowCenter)
        if ($distance -lt $bestDistance) { $best = $button; $bestDistance = $distance }
    }
    if ($best -and $bestDistance -le 40) { return $best }
    Write-Host "    行「$RowText」附近没有「$ButtonName」（最近距离 $([int]$bestDistance)）" -ForegroundColor DarkYellow
    return $null
}

# ---------------------------------------------------------------- 日期选择器

function Find-DateTrigger { param($Window)
    foreach ($element in @(Get-Elements $Window)) {
        if ($element.Current.ControlType -ne [System.Windows.Automation.ControlType]::Button) { continue }
        if ($element.Current.IsOffscreen) { continue }
        if ($element.Current.ClassName -notlike '*ui-date-picker__trigger*') { continue }
        if (-not (Test-Rect $element.Current.BoundingRectangle)) { continue }
        return $element
    }
    return $null
}
function Find-DateCell { param($Window, [int]$Day)
    foreach ($element in @(Get-Elements $Window)) {
        if ($element.Current.ControlType -ne [System.Windows.Automation.ControlType]::Button) { continue }
        if ($element.Current.IsOffscreen) { continue }
        if ($element.Current.Name -ne "$Day") { continue }
        if ($element.Current.ClassName -notlike '*ui-date-picker__cell*') { continue }
        if ($element.Current.ClassName -like '*is-outside*') { continue }
        if (-not (Test-Rect $element.Current.BoundingRectangle)) { continue }
        return $element
    }
    return $null
}

# ---------------------------------------------------------------- 截图 / 数据库读取

function Save-Screenshot { param([string]$Path)
    try {
        $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
        $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        $graphics.CopyFromScreen($bounds.X, $bounds.Y, 0, 0, $bounds.Size)
        $graphics.Dispose()
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
        $bitmap.Dispose()
        Write-Host "  截图: $Path" -ForegroundColor DarkYellow
    }
    catch { Write-Host "  截图失败: $_" -ForegroundColor DarkYellow }
}

# 只读导出某张表。
# ⚠ 三者**必须显式传参**：原来靠调用方作用域里的 `$repo` / `$ws` / `$OutDir`，
# 正是"断言走文件系统、被测进程走另一个 cwd"那类坑的来源。缺参直接抛错（不要静默取默认值）。
function Read-Table {
    param([string]$Repo, [string]$Workspace, [string]$Table, [string]$OutDir)
    if (-not $Repo -or -not $Workspace -or -not $Table -or -not $OutDir) {
        throw 'Read-Table 需要显式传参：-Repo/-Workspace/-Table/-OutDir（不要依赖调用方作用域）'
    }
    $dump = Join-Path $OutDir "dump-$Table.json"
    Push-Location $Repo
    & cargo -q xtask dump $Workspace --table $Table *> $dump
    $exit = $LASTEXITCODE
    Pop-Location
    if ($exit -ne 0) { throw "导出 $Table 失败（exit=$exit）" }
    return @((Get-Content $dump -Raw | ConvertFrom-Json).$Table)
}

# ---------------------------------------------------------------- 记账小工具

# 「记一笔」→ 填描述/金额 → 点最后一个「确认」。
# 注意 ui-transactions 的本地版本走 `Invoke-ButtonByName -Name '保存'`（弹窗出口不同），刻意保留。
function Add-Record { param($Window, [string]$Description, [string]$Amount)
    if (-not (Invoke-Element (Wait-Element -Root $Window -Name '记一笔'))) { return $false }
    Start-Sleep -Seconds 2
    $okDesc = Set-Value (Wait-Element -Root $Window -Name '描述消费内容') $Description
    $okAmount = Set-Value (Wait-Element -Root $Window -Name '0.00') $Amount
    Start-Sleep -Milliseconds 800
    $confirm = Find-All $Window '确认'
    if ($confirm.Count -gt 0) { Invoke-Element $confirm[$confirm.Count - 1] | Out-Null }
    Start-Sleep -Seconds 3
    return ($okDesc -and $okAmount)
}

# ---------------------------------------------------------------- 前导/收尾样板

# 临时 HOME / 输出目录：`GetFullPath` 归一化（相对路径会被"别的 cwd"的进程按它自己的目录解析）
# + 拒绝指到真实用户目录 + 建目录（含 `Desktop`：原生文件框的起始目录）。
# 返回归一化后的两个路径，调用方必须用它回填自己的 `$smokeHome` / `$OutDir`。
function Initialize-TrSmokeHome {
    param([string]$SmokeHome, [string]$OutDir)
    if (-not $SmokeHome -or -not $OutDir) {
        throw 'Initialize-TrSmokeHome 需要显式传参：-SmokeHome/-OutDir'
    }
    # ⚠ 不能用 `$home`：它是 PowerShell 的**只读自动变量**（`$HOME`），赋值会直接报
    # "无法覆盖变量 HOME"。这里用 `$smokeHomeAbs` / `$outDirAbs`。
    $smokeHomeAbs = [System.IO.Path]::GetFullPath($SmokeHome)
    if ($smokeHomeAbs -eq [System.IO.Path]::GetFullPath($env:USERPROFILE)) {
        throw '拒绝把临时 HOME 指到真实用户目录 —— 冒烟会改写你的配置'
    }
    $outDirAbs = [System.IO.Path]::GetFullPath($OutDir)
    foreach ($dir in @($smokeHomeAbs, (Join-Path $smokeHomeAbs 'Desktop'), $outDirAbs)) {
        if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
    }
    return @{ SmokeHome = $smokeHomeAbs; OutDir = $outDirAbs }
}

# 单实例插件按 identifier 判重：本仓库里**任何**构建在跑（含隐藏到托盘的）都会顶掉本次启动。
# 判重必须按**完整路径**，不能按进程名；而"别的目录下装着同一款应用"同样要拦
# （插件认的是应用标识、不是 exe 路径 —— 用户自己那份开着时，表现是"启动后 60 秒拿不到主窗口"）。
function Assert-NoRepoInstance {
    param([string]$Repo)
    if (-not $Repo) { throw 'Assert-NoRepoInstance 需要显式传参：-Repo' }
    $repoPrefix = $Repo.TrimEnd('\') + '\'
    $blockers = @(Get-Process -Name transactions -ErrorAction SilentlyContinue | Where-Object {
            $path = try { $_.Path } catch { $null }
            $path -and $path.StartsWith($repoPrefix, [System.StringComparison]::OrdinalIgnoreCase)
        })
    if ($blockers.Count -gt 0) {
        $paths = $blockers | ForEach-Object { try { $_.Path } catch { '(unknown)' } }
        throw "本仓库已有 Transactions 实例在运行（PID $($blockers.Id -join ', ')）：$($paths -join ' / ')`n" +
        "单实例插件会顶掉本次启动，请先退掉它们（注意：隐藏到托盘也算在运行）。"
    }
    $foreign = @(Get-Process -Name transactions -ErrorAction SilentlyContinue | Where-Object {
            $path = try { $_.Path } catch { $null }
            $path -and -not $path.StartsWith($repoPrefix, [System.StringComparison]::OrdinalIgnoreCase)
        })
    if ($foreign.Count -gt 0) {
        throw "本机另有 Transactions 实例在运行（PID $($foreign.Id -join ', ')：$($foreign.Path -join '; ')）—— 单实例插件会顶掉本次启动，请先退出它。"
    }
}

# 用临时 HOME 启动：子进程继承改过的 USERPROFILE/HOME，读写的是一次性配置。
# ⚠ 必须显式传参（原来读调用方的 `$smokeHome` / `$Exe`）。
function Start-App {
    param([string]$SmokeHome, [string]$Exe)
    if (-not $SmokeHome -or -not $Exe) { throw 'Start-App 需要显式传参：-SmokeHome/-Exe' }
    $saved = @{ USERPROFILE = $env:USERPROFILE; HOME = $env:HOME }
    try {
        $env:USERPROFILE = $SmokeHome
        $env:HOME = $SmokeHome
        return Start-Process -FilePath $Exe -PassThru
    }
    finally {
        $env:USERPROFILE = $saved.USERPROFILE
        $env:HOME = $saved.HOME
    }
}

# finally 块：失败先落 `failure.png`，再杀掉自己启动的进程（按 PID，不动别人的实例）。
function Stop-TrApp {
    param($Process, $Failures, [string]$OutDir)
    if ($Failures.Count -gt 0) { Save-Screenshot (Join-Path $OutDir 'failure.png') }
    if ($Process -and -not $Process.HasExited) {
        Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
        $Process.WaitForExit(5000) | Out-Null
    }
}
