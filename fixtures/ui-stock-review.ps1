# ui-stock-review.ps1 —— 「本轮复盘」写作面（弹窗）：两处入口 / 静止不裁剪 / 光标 / 字数上限 / 脏关闭 / 落库。
#
# 为什么需要它：复盘原来是一个内联的 3 行文本框（`rows=3` + `min-height: 72px`），
# 连默认模板都装不下 —— 而 WebView2 是**覆盖式**滚动条、仓库又禁止自绘滚动条
# （见 `base.css` 的滚动条段落），于是**静止时被裁掉的那几行读起来就是"内容丢了"**。
# 现在写作面在 `ModalSize::Large` 的弹窗里（`stock.rs` 的 `review_modal`）：320px 起、
# 随内容长高、静止永不裁剪，且有字数、快捷键与脏关闭二次确认。
#
# 判据分两条腿（**UIA 看不见"被裁掉"**，这是本仓库的既有纪律）：
#   * 几何：写作面高度 ≥ 320 逻辑像素 —— 这是"整个 500 字预算一眼看得完"的那一档，
#     也是"不裁剪"唯一可机器判定的代理；
#   * 交互：弹窗打开**没有点过它**就已是焦点，且光标落在第一行末（直接粘贴、不选不改选区，
#     再看读回的文本长什么样）—— 焦点、光标、受控输入三件事一次验完。
# 另外：字数上限（`maxlength=500` 截断且计数行如实显示）、脏关闭的两态底栏
# （「继续编辑 / 放弃」，「继续编辑」不许丢草稿）、Ctrl+S 落库（断言落在**库**上，不落在"点到了没有"）、
# 两处宿主（持仓 / 轮次）打开的是同一个弹窗。
#
# 用法（pwsh 7；需要 release 产物；本仓库不能有实例在跑）：
#   pwsh -File fixtures/ui-stock-review.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>] [-Appearance light|dark]
# 深色与浅色同形：发布前用 `-Appearance dark` 再跑一遍看截图（判据不因主题而变）。

param(
    [string]$Exe,
    [string]$SmokeHome,
    [string]$Workspace,
    [string]$OutDir,
    # 只影响截图与主题（`appearance` 写进一次性配置）；判据与主题无关
    [string]$Appearance = 'light'
)

$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot 'lib\TrUia.ps1')

$repo = Split-Path -Parent $PSScriptRoot
# ⚠ `$explicitWorkspace` 要在默认值赋值**之前**取：否则"不用显式工作空间就重新播种"恒不成立，
# 断言绝对状态时会叠加（AGENTS.md 的硬纪律）。
$explicitWorkspace = -not [string]::IsNullOrWhiteSpace($Workspace)
if (-not $Exe) { $Exe = Join-Path $repo 'target\release\transactions.exe' }
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\tests\ui-stock-review\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\tests\ui-stock-review\out' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }

if (-not (Test-Path $Exe)) {
    throw "找不到可执行文件: $Exe（先跑 pwsh -File fixtures/test.ps1 -Unit build-ui,build-app）"
}

$trPaths = Initialize-TrSmokeHome -SmokeHome $SmokeHome -OutDir $OutDir
$smokeHome = $trPaths.SmokeHome
$OutDir = $trPaths.OutDir
Assert-NoRepoInstance -Repo $repo
$ws = [System.IO.Path]::GetFullPath($Workspace)

$failures = New-Object System.Collections.Generic.List[string]

# ------------------------------------------------------------------ 本地小工具

function Find-VisibleButton { param($Window, [string]$Name, [switch]$Last, [switch]$Like, [string]$ClassPart)
    $windowRect = $Window.Current.BoundingRectangle
    $hits = @()
    foreach ($element in @(Get-Elements $Window)) {
        if ($element.Current.ControlType -ne [System.Windows.Automation.ControlType]::Button) { continue }
        if ($element.Current.IsOffscreen) { continue }
        $elementName = $element.Current.Name
        if (-not $elementName) { continue }
        $nameMatches = if ($Like) { $elementName.Contains($Name) } else { $elementName -eq $Name }
        if (-not $nameMatches) { continue }
        # 标题栏三键里有一个 `aria-label="关闭"`：按名字点它会把应用关掉（ui-stock.ps1 踩过），
        # 所以界面按钮一律用类名 `ui-btn` 再筛一道。
        if ($ClassPart) {
            $className = [string]$element.Current.ClassName
            if (-not $className -or -not $className.Contains($ClassPart)) { continue }
        }
        $rect = $element.Current.BoundingRectangle
        if ($rect.Width -le 0 -or $rect.Height -le 0) { continue }
        if ($rect.Y -lt $windowRect.Y -or ($rect.Y + $rect.Height) -gt ($windowRect.Y + $windowRect.Height)) { continue }
        if ($rect.X -lt $windowRect.X -or ($rect.X + $rect.Width) -gt ($windowRect.X + $windowRect.Width)) { continue }
        $hits += $element
    }
    if ($hits.Count -eq 0) { return $null }
    if ($Last) { return $hits[$hits.Count - 1] }
    return $hits[0]
}

function Wait-VisibleButton { param($Window, [string]$Name, [int]$TimeoutSec = 15, [switch]$Last, [switch]$Like, [string]$ClassPart)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $element = Find-VisibleButton -Window $Window -Name $Name -Last:$Last -Like:$Like -ClassPart $ClassPart
        if ($element) { return $element }
        Start-Sleep -Milliseconds 300
    } while ((Get-Date) -lt $deadline)
    return $null
}

# 子功能（持仓 / 记录 / …）走左侧图标条：共享版 `Invoke-SubFunction` 按"同名 Button 里最靠左的那个"定位。
function Switch-StockSub { param($Window, [string]$Name, [int]$TimeoutSec = 20)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        if (Invoke-SubFunction -Window $Window -Name $Name -TimeoutSec 3) { return $true }
        Start-Sleep -Milliseconds 400
    }
    return $false
}

# 写作面 = 当前可见的**最大那个** Edit。页面本身没有别的输入框（持仓/记录两页全是按钮与文本），
# 弹窗一开它就是唯一大块；按面积取最大是为了不受将来别处加输入框的影响。
function Get-ReviewEditor { param($Window)
    $best = $null; $bestArea = 0.0
    foreach ($element in @(Get-Elements $Window)) {
        $isEdit = ($element.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit) -or
            ($element.Current.ClassName -eq 'Edit')
        if (-not $isEdit -or $element.Current.IsOffscreen) { continue }
        $rect = $element.Current.BoundingRectangle
        if (-not (Test-Rect $rect)) { continue }
        $area = [double]$rect.Width * [double]$rect.Height
        if ($area -gt $bestArea) { $best = $element; $bestArea = $area }
    }
    return $best
}

function Get-EditValue { param($Element)
    if (-not $Element) { return '' }
    $pattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
        return [string]$pattern.Current.Value
    }
    return ''
}

function Get-EditHeight { param($Element)
    if (-not $Element) { return 0.0 }
    return [double]$Element.Current.BoundingRectangle.Height
}

# 弹窗标题 = `本轮复盘 · <股票名> <代码>`：**内联那块只写「本轮复盘」四个字**，
# 所以"标题里带得出身份"本身就是"弹窗开着"的判据（也是两处宿主共用同一个弹窗的判据）。
function Wait-ReviewModal { param($Window, [int]$TimeoutSec = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $title = Find-Like -Root $Window -Pattern '本轮复盘 · '
        if ($title) { return $title }
        Start-Sleep -Milliseconds 300
    } while ((Get-Date) -lt $deadline)
    return $null
}

# ⚠ 形参**不能**取名 `$Button`：PowerShell 的形参带类型约束（`[string]`），
# 后面把元素赋给 `$button`（同名、大小写不敏感）会被**强转成字符串**，
# 于是 `Invoke-Element` 收到一个 String、在 `TryGetCurrentPattern` 上炸掉。
function Close-ReviewModal { param($Window, [string]$ButtonName)
    $target = Wait-VisibleButton -Window $Window -Name $ButtonName -TimeoutSec 10 -ClassPart 'ui-btn'
    if (-not $target) { return $false }
    Invoke-Element $target | Out-Null
    Start-Sleep -Milliseconds 800
    return $true
}

# 键盘动作一律三步：**提到前台（并确认）→ UIA 聚焦写作面（并确认焦点在 Edit 上）→ SendKeys**。
#
# 为什么不能省任何一步（两条都是实测踩出来的）：
#   * `SendKeys` 发给**前台**窗口，而守卫跑在真实桌面上，用户自己的浏览器随时可能占了前台
#     —— 一次运行里 Ctrl+A/Ctrl+V 整段贴进了浏览器，库里自然没东西（表现为"保存没落库"的假红）；
#     而 `SetForegroundWindow` 会被系统拒绝（调用进程不是前台进程时），所以用 `Bring-AppToFront`
#     循环确认，失败就报出来而不是静默继续。
#   * 更不能用**真实鼠标点**写作面来"顺带聚焦"：窗口上方可能压着别的窗口，那一下点的是别人，
#     于是前台与焦点一起跑到别人那里（实测：粘贴与 Ctrl+S 双双落到浏览器）。
function Assert-ReviewFocus { param([string]$Stage)
    $focused = [System.Windows.Automation.AutomationElement]::FocusedElement
    $isEdit = $focused -and (
        ($focused.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit) -or
        ($focused.Current.ClassName -eq 'Edit'))
    Assert-True ([bool]$isEdit) "$Stage 焦点在写作面里（实际 '$($focused.Current.Name)'）"
    return [bool]$isEdit
}

# 替换整段（Ctrl+A + Ctrl+V），并**读回校验**。
#
# 读回这一条不能省：受控组件（`<textarea prop:value=signal>`）里如果信号没跟上这一次输入，
# 下一次重渲染会把 DOM 值刷回旧文本，表现为"保存成功、库里还是原文"的假绿
# （实测：轮次侧一次运行里真的发生了 —— 弹窗关了、库里的 review 一个字没变）。
function Set-ReviewText { param($Window, $Element, [string]$Text, [string]$Expect, [int]$Retry = 3)
    # `-Expect` 与写入内容分开：写入 600 字是**故意越限**的用例，读回来只会是 500 字
    # （`maxlength` 截断），拿写入内容当期望会永远不等 —— 那是判据写错，不是应用错。
    if (-not $Expect) { $Expect = $Text }
    for ($attempt = 1; $attempt -le $Retry; $attempt++) {
        if (-not (Bring-AppToFront -Window $Window)) { return $false }
        if ($Element) {
            try { $Element.SetFocus() } catch { Write-Host "    聚焦写作面失败：$_" -ForegroundColor DarkYellow }
            Start-Sleep -Milliseconds 400
        }
        if (-not (Assert-ReviewFocus -Stage '粘贴前')) { return $false }
        Set-Clipboard -Value $Text
        Start-Sleep -Milliseconds 300
        [System.Windows.Forms.SendKeys]::SendWait('^a')
        Start-Sleep -Milliseconds 250
        [System.Windows.Forms.SendKeys]::SendWait('^v')
        Start-Sleep -Milliseconds 700
        # 读回用**当下**找到的元素：重渲染会换掉旧的 Edit 句柄
        $readBack = Get-EditValue -Element (Get-ReviewEditor -Window $Window)
        if ($readBack -eq $Expect) { return $true }
        $head = if ($readBack.Length -gt 24) { $readBack.Substring(0, 24) } else { $readBack }
        Write-Host "    写入读回不一致（第 $attempt 次）：'$head…'" -ForegroundColor DarkYellow
        Start-Sleep -Milliseconds 500
    }
    return $false
}

# 直接粘贴到**当前光标处**（不 Ctrl+A、不点输入框）：焦点与光标两件事都靠它验。
function Paste-AtCaret { param($Window, [string]$Text)
    if (-not (Bring-AppToFront -Window $Window)) { return $false }
    if (-not (Assert-ReviewFocus -Stage '粘贴前')) { return $false }
    Set-Clipboard -Value $Text
    Start-Sleep -Milliseconds 200
    [System.Windows.Forms.SendKeys]::SendWait('^v')
    Start-Sleep -Milliseconds 600
    return $true
}

# Ctrl+S 保存：同样要先确认前台是应用，再发键（lib 的 `Save-Now` 只 SetForegroundWindow，
# 被系统拒绝时会**静默地**把 ^s 发给别的窗口）。
function Save-ReviewNow { param($Window)
    Assert-True (Bring-AppToFront -Window $Window) '保存前把应用窗口提回前台'
    Assert-ReviewFocus -Stage '保存前' | Out-Null
    [System.Windows.Forms.SendKeys]::SendWait('^s')
    Start-Sleep -Seconds 2
}

# 键盘动作一律先过这里：`SendKeys` 发给**前台**窗口，而守卫跑在真实桌面上，
# 用户自己的浏览器/即时通讯随时可能把前台抢走（实测：一次运行里 Ctrl+A/Ctrl+V 全贴进了浏览器，
# 库里自然没有东西 —— 表现为"保存没落库"的假红）。
# `SetForegroundWindow` 本身会被系统拒绝（调用进程不是前台进程时），所以：ShowWindow 复原
# + BringWindowToTop + SetForegroundWindow，然后**确认**前台真的是它。
function Bring-AppToFront { param($Window, [int]$Attempts = 6)
    $hwnd = [IntPtr]$Window.Current.NativeWindowHandle
    for ($i = 1; $i -le $Attempts; $i++) {
        [TrUia]::ShowWindow($hwnd, 9) | Out-Null          # SW_RESTORE
        [TrReviewDpi]::BringWindowToTop($hwnd) | Out-Null
        [TrUia]::SetForegroundWindow($hwnd) | Out-Null
        Start-Sleep -Milliseconds 300
        if ([TrReviewDpi]::GetForegroundWindow() -eq $hwnd) { return $true }
    }
    Write-Host "    前台窗口不是应用（$([TrReviewDpi]::GetForegroundWindow()) ≠ $hwnd）" -ForegroundColor DarkYellow
    return $false
}

# 建仓（供"持仓侧"那几步造出一个**进行中**的轮次）：种子里的轮次全部已归档，
# `list_positions` 只列"还有持仓或本轮还没归档"的，所以不建仓就没有持仓侧可写。
function Add-OpenPosition { param($Window, [string]$Code, [string]$Price, [string]$Lots)
    if (-not (Invoke-Element (Wait-Element -Root $Window -Name '建仓' -TimeoutSec 10))) {
        Write-Host '    找不到「建仓」按钮' -ForegroundColor DarkYellow
        return $false
    }
    Start-Sleep -Seconds 2
    $opened = [bool](Wait-Element -Root $Window -Name '委托建仓' -TimeoutSec 8)
    Assert-True $opened '建仓弹窗已打开'
    if (-not $opened) { return $false }

    # 弹窗里**没有占位符**的两个 Edit 是「股票名称 / 股票代码」，靠右那个是代码（ui-stock.ps1 同口径）
    $unnamed = @(Get-Elements $Window) | Where-Object {
        (($_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit) -or
            ($_.Current.ClassName -eq 'Edit')) -and
            -not $_.Current.Name -and -not $_.Current.IsOffscreen -and
            (Test-Rect $_.Current.BoundingRectangle)
    } | Sort-Object { $_.Current.BoundingRectangle.X }
    if ($unnamed.Count -lt 2) {
        Write-Host "    建仓弹窗里无名输入框只有 $($unnamed.Count) 个" -ForegroundColor DarkYellow
        return $false
    }
    $codeInput = $unnamed[$unnamed.Count - 1]
    Assert-True (Set-Value $codeInput $Code) "填入股票代码 $Code"
    Start-Sleep -Milliseconds 500
    # 名称靠失焦自动查（真实行情）：查不到也不影响本轮复盘的判据，所以不断言它
    $codeInput.SetFocus()
    Start-Sleep -Milliseconds 200
    [System.Windows.Forms.SendKeys]::SendWait('{TAB}')
    Start-Sleep -Seconds 2
    Assert-True (Set-Value (Wait-Element -Root $Window -Name '成交价（元/股）' -TimeoutSec 8) $Price) "填入成交价 $Price"
    $lotsInput = Find-EditLike -Window $Window -Pattern '手数'
    Assert-True (Set-Value $lotsInput $Lots) "填入手数 $Lots"
    Start-Sleep -Milliseconds 500
    $confirm = Wait-VisibleButton -Window $Window -Name '建仓' -TimeoutSec 10 -Last -ClassPart 'ui-btn'
    if ($confirm) { Invoke-Element $confirm | Out-Null }
    Start-Sleep -Seconds 4
    return (-not [bool](Find-First $Window '委托建仓'))
}

function Find-EditLike { param($Window, [string]$Pattern)
    $deadline = (Get-Date).AddSeconds(10)
    do {
        foreach ($element in @(Get-Elements $Window)) {
            $isEdit = ($element.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit) -or
                ($element.Current.ClassName -eq 'Edit')
            if (-not $isEdit -or $element.Current.IsOffscreen) { continue }
            if (-not (Test-Rect $element.Current.BoundingRectangle)) { continue }
            $name = [string]$element.Current.Name
            if ($name -and $name.Contains($Pattern)) { return $element }
        }
        Start-Sleep -Milliseconds 300
    } while ((Get-Date) -lt $deadline)
    return $null
}

# 窗口缩放：写作面高度是**逻辑** 320px，比对时要乘上它（150% 屏上是 480 物理像素）。
# 同一处再借两个前台相关的 user32 入口（键盘动作的可达性判据，见 `Bring-AppToFront`）。
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class TrReviewDpi {
  [DllImport("user32.dll")] public static extern int GetDpiForWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr hWnd);
}
'@
function Get-WindowScale { param($Window)
    $hwnd = if ($Window) { [IntPtr]$Window.Current.NativeWindowHandle } else { [IntPtr]::Zero }
    $dpi = if ($hwnd -ne [IntPtr]::Zero) { [TrReviewDpi]::GetDpiForWindow($hwnd) } else { 0 }
    if ($dpi -le 0) { $dpi = [TrReviewDpi]::GetDpiForSystem() }
    if ($dpi -le 0) { $dpi = 96 }
    return $dpi / 96.0
}

# ------------------------------------------------------------------ 播种与配置

if (-not $explicitWorkspace) {
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
}
if (-not (Test-Path (Join-Path $ws 'transactions.db'))) {
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[review] 播种工作空间: $ws" -ForegroundColor Cyan
}

@{
    width           = 1500
    height          = 1000
    workspaceDir    = $ws
    closeBehavior   = 'quit'
    appearance      = $Appearance
    smokeTestMarker = 'ui-stock-review.ps1'
} | ConvertTo-Json | Set-Content -Path (Join-Path $smokeHome '.transactions.json') -Encoding UTF8

$stockCode = '600519'
$positionText = '持仓侧：回踩年线企稳，等放量确认'
$roundText = '轮次侧：减仓点位偏早，下次等回踩确认'
$caretMarker = '光标落点'

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

    $window = Get-ReadyWindow -ProcessId $process.Id -TimeoutSec 60
    if (-not $window) { throw '启动后 60 秒内没有拿到主窗口' }
    $hwnd = [IntPtr]$window.Current.NativeWindowHandle
    [TrUia]::ShowWindow($hwnd, 9) | Out-Null
    [TrUia]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Seconds 1

    $scale = Get-WindowScale -Window $window
    Write-Host "[review] 窗口 DPI 缩放 $scale；主题 $Appearance" -ForegroundColor Cyan

    # ================= 1/5 持仓侧：建仓造出进行中的一轮 =================
    Write-Host "`n[review] 1/5 持仓侧：建仓 $stockCode × 1 手（种子的轮次都已归档，持仓侧需要一轮进行中）"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '股票' -TimeoutSec 15)) '打开「股票」页'
    Start-Sleep -Seconds 3
    Assert-True (Switch-StockSub -Window $window -Name '持仓') '切到「持仓」子功能'
    Start-Sleep -Seconds 2
    Assert-True (Add-OpenPosition -Window $window -Code $stockCode -Price '100' -Lots '1') '建仓成功（持仓侧出现进行中的一轮）'
    $stockName = [string](@(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_position' -OutDir $OutDir |
        Where-Object { $_.stock_code -eq $stockCode -and [int64]$_.quantity -gt 0 } |
        Select-Object -First 1).stock_name)

    # ================= 2/5 写作面：不裁剪 / 打开即聚焦 / 光标落在第一行末 =================
    Write-Host "`n[review] 2/5 写作面：几何（≥320 逻辑像素）+ 焦点 + 光标"
    $openButton = $null
    foreach ($candidate in @('写复盘', '编辑')) {
        $openButton = Wait-VisibleButton -Window $window -Name $candidate -TimeoutSec 5 -ClassPart 'ui-btn'
        if ($openButton) { break }
    }
    Assert-True ([bool]$openButton) '持仓侧找到「写复盘 / 编辑」入口'
    if ($openButton) { Invoke-Element $openButton | Out-Null }
    Start-Sleep -Seconds 1

    $title = Wait-ReviewModal -Window $window
    Assert-True ([bool]$title) '持仓侧打开了「本轮复盘 · …」写作面弹窗'
    if ($title) {
        Write-Host "    标题：$($title.Current.Name)" -ForegroundColor DarkGray
        Assert-True ($title.Current.Name.Contains($stockCode)) `
            "标题带得出身份（$stockName / $stockCode，实际 '$($title.Current.Name)'）"
    }

    # `FocusedElement` 读的是**全系统**焦点：别的窗口抢了前台会让下面那条假红，先提回前台
    Assert-True (Bring-AppToFront -Window $window) '读焦点前把应用窗口提回前台'

    $editor = Get-ReviewEditor -Window $window
    Assert-True ([bool]$editor) '弹窗里有写作面（文本域）'
    if ($editor) {
        $height = Get-EditHeight -Element $editor
        Assert-True ($height -ge (320 * $scale - 4)) `
            "写作面不裁剪：高度 $([int]$height)px ≥ 320×$scale（静止时整个 500 字预算都要看得见）"

        # 焦点：**没有点过它**（点「写复盘」只是开弹窗），此刻聚焦元素就该是文本域
        $focused = [System.Windows.Automation.AutomationElement]::FocusedElement
        $focusedIsEdit = $focused -and (
            ($focused.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit) -or
            ($focused.Current.ClassName -eq 'Edit'))
        Assert-True ([bool]$focusedIsEdit) '弹窗打开即把焦点放进写作面（没有点过输入框）'

        $prefill = Get-EditValue -Element $editor
        Assert-True ($prefill -eq "买入理由：`n卖出理由：`n交易心得：`n") `
            "空复盘预填三行模板（实际 '$prefill'）"

        # 光标：直接粘贴，不选不改选区 → 只可能落在「买入理由：」之后
        Paste-AtCaret -Window $window -Text $caretMarker
        $value = Get-EditValue -Element $editor
        Assert-True ($value.StartsWith("买入理由：$caretMarker")) `
            "光标落在第一行末（读回 '$value'）"
        $counter = Find-Like -Root $window -Pattern ' / 500 字'
        $expectedCount = $value.Length
        Assert-True ([bool]$counter) '有字数行「N / 500 字」'
        if ($counter) {
            Assert-True ($counter.Current.Name -eq "$expectedCount / 500 字") `
                "字数行如实（期望 $expectedCount / 500 字，实际 '$($counter.Current.Name)'）"
        }
        Save-Screenshot -Path (Join-Path $OutDir "review-$Appearance-editor.png")
    }

    # ================= 3/5 字数上限 =================
    Write-Host "`n[review] 3/5 字数上限：粘 600 字 → 截断到 500"
    $over = '测' * 600
    # 整段替换（Ctrl+A + Ctrl+V，带读回校验）；前台/焦点由 `Set-ReviewText` 自己确认
    if ($editor) {
        Assert-True (Set-ReviewText -Window $window -Element $editor -Text $over -Expect ('测' * 500)) `
            '写入 600 字后被截到 500（读回一致）'
    }
    Start-Sleep -Milliseconds 500
    $capped = Get-EditValue -Element $editor
    Assert-True ($capped.Length -eq 500) "粘贴 600 字后正文是 500 字（实际 $($capped.Length)）"
    $capCounter = Find-Like -Root $window -Pattern ' / 500 字'
    if ($capCounter) {
        Assert-True ($capCounter.Current.Name -eq '500 / 500 字') "字数行显示 500 / 500 字（实际 '$($capCounter.Current.Name)'）"
    }

    # ================= 4/5 脏关闭：二次确认，且「继续编辑」不丢草稿 =================
    Write-Host "`n[review] 4/5 脏关闭：有改动 → 二次确认 → 继续编辑 → 放弃"
    Assert-True (Close-ReviewModal -Window $window -ButtonName '取消') '点到了「取消」'
    $confirming = Wait-VisibleButton -Window $window -Name '继续编辑' -TimeoutSec 8 -ClassPart 'ui-btn'
    Assert-True ([bool]$confirming) '有改动时「取消」不直接关，底栏换成「继续编辑 / 放弃」'
    $discard = Find-VisibleButton -Window $window -Name '放弃' -ClassPart 'ui-btn'
    Assert-True ([bool]$discard) '确认态有「放弃」'
    if ($confirming) { Invoke-Element $confirming | Out-Null }
    Start-Sleep -Milliseconds 800
    $backToEdit = Wait-VisibleButton -Window $window -Name '取消' -TimeoutSec 8 -ClassPart 'ui-btn'
    Assert-True ([bool]$backToEdit) '「继续编辑」回到编辑态（「取消」回来了）'
    $kept = Get-EditValue -Element (Get-ReviewEditor -Window $window)
    Assert-True ($kept.Length -eq 500) "「继续编辑」不丢草稿（实际 $($kept.Length) 字）"

    if ($backToEdit) { Invoke-Element $backToEdit | Out-Null }
    Start-Sleep -Milliseconds 800
    $discard = Wait-VisibleButton -Window $window -Name '放弃' -TimeoutSec 8 -ClassPart 'ui-btn'
    Assert-True ([bool]$discard) '再点「取消」又回到确认态'
    if ($discard) { Invoke-Element $discard | Out-Null }
    Start-Sleep -Seconds 1
    Assert-True (-not [bool](Find-Like -Root $window -Pattern '本轮复盘 · ')) '「放弃」关掉了弹窗'
    Assert-True (-not [bool](Get-ReviewEditor -Window $window)) '关掉后页面上没有写作面（写作只在弹窗里）'
    $posReview = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_position' -OutDir $OutDir |
        Where-Object { $_.stock_code -eq $stockCode -and [int64]$_.quantity -gt 0 })
    Assert-True (($posReview.Count -ge 1) -and [string]::IsNullOrEmpty([string]$posReview[0].review)) `
        '「放弃」没有写库（持仓复盘的 review 仍是空）'

    # ================= 5/5 落库：持仓侧走 Ctrl+S，轮次侧走弹窗的「保存」 =================
    Write-Host "`n[review] 5/5 落库：持仓侧 Ctrl+S → 轮次侧「保存」按钮"
    $openButton = Wait-VisibleButton -Window $window -Name '写复盘' -TimeoutSec 10 -ClassPart 'ui-btn'
    if ($openButton) { Invoke-Element $openButton | Out-Null }
    Start-Sleep -Seconds 1
    Assert-True ([bool](Wait-ReviewModal -Window $window)) '重新打开持仓侧写作面'
    $editor = Get-ReviewEditor -Window $window
    if ($editor) {
        Assert-True (Set-ReviewText -Window $window -Element $editor -Text $positionText) '持仓侧写入目标文本（读回一致）'
    }
    Start-Sleep -Milliseconds 500
    # 持仓侧走 **Ctrl+S**（写作面的快捷保存）
    Save-ReviewNow -Window $window
    Start-Sleep -Seconds 1
    $savedPosition = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_position' -OutDir $OutDir |
        Where-Object { [string]$_.review -eq $positionText })
    Assert-True ($savedPosition.Count -eq 1) "持仓侧 Ctrl+S 落库（库里 review = '$positionText' 的行数 $($savedPosition.Count)）"
    Assert-True (-not [bool](Find-Like -Root $window -Pattern '本轮复盘 · ')) '保存成功后弹窗自己关上'
    Assert-True ([bool](Find-Like -Root $window -Pattern $positionText)) '保存后这块复盘在下文里摊开显示'
    Save-Screenshot -Path (Join-Path $OutDir "review-$Appearance-saved.png")

    # ---- 轮次侧：记录子功能里某一轮 ----
    Assert-True (Switch-StockSub -Window $window -Name '记录') '切到「记录」子功能'
    Start-Sleep -Seconds 3
    $card = $null
    $deadline = (Get-Date).AddSeconds(20)
    do {
        foreach ($element in @(Get-Elements $window)) {
            if ($element.Current.ControlType -ne [System.Windows.Automation.ControlType]::Button) { continue }
            if ($element.Current.IsOffscreen) { continue }
            $name = [string]$element.Current.Name
            if ($name -and $name.Contains($stockCode) -and $name.Contains('轮')) { $card = $element; break }
        }
        if (-not $card) { Start-Sleep -Milliseconds 400 }
    } while (-not $card -and (Get-Date) -lt $deadline)
    Assert-True ([bool]$card) "找到 $stockCode 的历史卡片"
    if ($card) { Invoke-Element $card | Out-Null }
    Start-Sleep -Seconds 3

    $roundButton = $null
    $deadline = (Get-Date).AddSeconds(15)
    do {
        $candidates = @()
        foreach ($candidate in @('编辑', '写复盘')) {
            $candidates += @(Find-All $window $candidate | Where-Object {
                    -not $_.Current.IsOffscreen -and (Test-Rect $_.Current.BoundingRectangle) -and
                    ([string]$_.Current.ClassName).Contains('ui-btn')
                })
        }
        if ($candidates.Count -gt 0) {
            $roundButton = $candidates | Sort-Object { $_.Current.BoundingRectangle.Y } | Select-Object -First 1
        }
        if (-not $roundButton) { Start-Sleep -Milliseconds 400 }
    } while (-not $roundButton -and (Get-Date) -lt $deadline)
    Assert-True ([bool]$roundButton) '轮次区找到「编辑 / 写复盘」入口（最靠上那一轮）'
    if ($roundButton) { Invoke-Element $roundButton | Out-Null }
    Start-Sleep -Seconds 1

    $roundTitle = Wait-ReviewModal -Window $window
    Assert-True ([bool]$roundTitle) '轮次侧打开的是**同一个**写作面弹窗'
    if ($roundTitle) { Write-Host "    标题：$($roundTitle.Current.Name)" -ForegroundColor DarkGray }
    Assert-True ([bool](Find-Like -Root $window -Pattern '轮 · ')) '轮次侧正文首行带着「第 N 轮 · 起止时间」'
    $roundEditor = Get-ReviewEditor -Window $window
    if ($roundEditor) {
        $existing = Get-EditValue -Element $roundEditor
        $known = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_trade_round' -OutDir $OutDir |
            Where-Object { [string]$_.stock_code -eq $stockCode } | ForEach-Object { [string]$_.review })
        Assert-True ($known -contains $existing) "已有复盘带原文进写作面（读回 '$existing'）"
    }
    if ($roundEditor) {
        Assert-True (Set-ReviewText -Window $window -Element $roundEditor -Text $roundText) '轮次侧写入目标文本（读回一致）'
    }
    Start-Sleep -Milliseconds 500
    # 轮次侧走**弹窗的「保存」按钮**：两条保存入口各覆盖一次。按钮用**真实鼠标点**
    # （ui-stock.ps1 的教训：详情区按钮每次重渲染都换元素，对旧句柄 Invoke 会静默无效），
    # 点之前先确认前台是应用，否则点到的是压在它上面的窗口。
    Assert-True (Bring-AppToFront -Window $window) '点保存前把应用窗口提回前台'
    $saveButton = Wait-VisibleButton -Window $window -Name '保存' -TimeoutSec 10 -ClassPart 'ui-btn'
    Assert-True ([bool]$saveButton) '轮次侧找到弹窗的「保存」按钮'
    if ($saveButton) {
        $rect = $saveButton.Current.BoundingRectangle
        [TrUia]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2)) | Out-Null
    }
    Start-Sleep -Seconds 3
    $savedRound = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_trade_round' -OutDir $OutDir |
        Where-Object { [string]$_.review -eq $roundText })
    Assert-True ($savedRound.Count -eq 1) "轮次侧「保存」落库（库里 review = '$roundText' 的轮次 $($savedRound.Count) 个）"

    Write-Host "[review] 截图目录：$OutDir" -ForegroundColor Cyan
}
finally { Stop-TrApp -Process $process -Failures $failures -OutDir $OutDir }

Show-TrSummary -Failures $failures -Tag 'review' -SuccessMessage "[review] 全部通过：写作面（两处入口 / 不裁剪 / 光标 / 上限 / 脏关闭 / 落库）"
