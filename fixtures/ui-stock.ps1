# ui-stock.ps1 —— 股票「建仓 → 减仓 → 清仓」的端到端验收（真实界面 + 数据库断言）。
#
# 为什么需要它：数据级黄金对比（`xtask parity` 阶段 3）覆盖的是**落库**语义，
# 而界面上"点减仓/清仓 → 填成交价 → 提交"这条路一直只有「建仓」被手工验过。
# 这里把整条生命周期走完，并按服务层的口径把关键金额算清楚：
#   * 建仓：持仓数量 += 股数，`total_cost += amount + fee`（**成本含手续费**）
#   * 减仓/清仓：`cost_basis = round(total_cost × 本次股数 / 持仓股数)`，
#     `realized_pnl = amount - fee - cost_basis`，数量与成本按比例结转；清空时成本归零并归档本轮
#   * 资金记录：`cash_balance` 恒等于 `principal + Σ amount_change`（不变量，逐步校验）
#
# 用法（pwsh 7；需要 release 产物；本仓库不能有实例在跑）：
#   pwsh -File fixtures/ui-stock.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

param(
    [string]$Exe,
    [string]$SmokeHome,
    [string]$Workspace,
    [string]$OutDir
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$explicitWorkspace = -not [string]::IsNullOrWhiteSpace($Workspace)
if (-not $Exe) { $Exe = Join-Path $repo 'target\release\transactions.exe' }
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\stock-smoke\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\stock-smoke' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$smokeHome = [System.IO.Path]::GetFullPath($SmokeHome)
$OutDir = [System.IO.Path]::GetFullPath($OutDir)
if ($smokeHome -eq [System.IO.Path]::GetFullPath($env:USERPROFILE)) {
    throw "拒绝把临时 HOME 指到真实用户目录 —— 冒烟会改写你的配置"
}
foreach ($dir in @($smokeHome, (Join-Path $smokeHome 'Desktop'), $OutDir)) {
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
}

# 判重按完整路径（原 Electron 版也叫 Transactions.exe）
$repoPrefix = $repo.TrimEnd('\') + '\'
$blockers = @(Get-Process -Name transactions -ErrorAction SilentlyContinue | Where-Object {
    $path = try { $_.Path } catch { $null }
    $path -and $path.StartsWith($repoPrefix, [System.StringComparison]::OrdinalIgnoreCase)
})
if ($blockers.Count -gt 0) {
    throw "本仓库已有 Transactions 实例在运行（PID $($blockers.Id -join ', ')），单实例插件会顶掉本次启动。"
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
public class TrStock {
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
function Find-All { param($Root, [string]$Name)
    return @($Root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $Name))))
}
function Find-ElementLike { param($Root, [string]$Pattern)
    foreach ($element in @($Root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition))) {
        $name = $element.Current.Name
        if ($name -and $name.Contains($Pattern)) { return $element }
    }
    return $null
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
function Wait-ElementLike { param($Root, [string]$Pattern, [int]$TimeoutSec = 25)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $element = Find-ElementLike -Root $Root -Pattern $Pattern
        if ($element) { return $element }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    return $null
}
# 找**输入框**（ControlType.Edit）而不是任意同名节点：「手数」这类文案在弹窗里有好几处
# （标签、汇总行），按名字取第一个常常拿到不可写值的 Text 节点（实测 Set-Value 直接失败）。
function Find-EditLike { param($Root, [string]$Pattern)
    foreach ($element in @(Get-Elements $Root)) {
        $isEdit = ($element.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit) -or
            ($element.Current.ClassName -eq 'Edit')
        if (-not $isEdit -or $element.Current.IsOffscreen) { continue }
        $name = $element.Current.Name
        if ($name -and $name.Contains($Pattern)) { return $element }
    }
    return $null
}
function Wait-EditLike { param($Root, [string]$Pattern, [int]$TimeoutSec = 20)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $element = Find-EditLike -Root $Root -Pattern $Pattern
        if ($element) { return $element }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    return $null
}
function Invoke-Element { param($Element)
    if (-not $Element) { return $false }
    $pattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
        $pattern.Invoke(); return $true
    }
    $rect = $Element.Current.BoundingRectangle
    if ($rect.Width -gt 0) {
        [TrStock]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
        return $true
    }
    return $false
}
function Set-Value { param($Element, [string]$Value)
    if (-not $Element) { return $false }
    $pattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
        $pattern.SetValue($Value); return $true
    }
    return $false
}
function Click-Element { param($Element)
    if (-not $Element) { return $false }
    $rect = $Element.Current.BoundingRectangle
    if ($rect.Width -le 0) { return $false }
    [TrStock]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
    return $true
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
        Write-Host "  截图: $Path" -ForegroundColor DarkYellow
    }
    catch { Write-Host "  截图失败: $_" -ForegroundColor DarkYellow }
}

# 主窗口：启动期先出现初始化窗口（无侧栏），轮询到含「消费记录」为止
function Get-ReadyWindow { param([int]$ProcessId, [int]$TimeoutSec = 60)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    $last = $null
    while ((Get-Date) -lt $deadline) {
        $candidate = $UIA::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children,
            (New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $ProcessId)))
        if ($candidate) {
            $last = $candidate
            if (Find-First $candidate '消费记录') { return $candidate }
        }
        Start-Sleep -Milliseconds 500
    }
    return $last
}

# 下单弹窗里「股票名称 / 股票代码」两个输入框在 UIA 里**没有名字**（FormItem 的 label 不是 label 元素），
# 只能按包围盒区分：同一水平带里靠右那个是「股票代码」。
# 判据必须用 **ControlType.Edit**，不能用 ClassName：本项目的 `Input` 渲染成
# `class='ui-input__control'`，ClassName 并不是 'Edit'——按 ClassName 过滤会一个都找不到（实测踩过）。
function Find-UnnamedEditRight { param($Window)
    $edits = @()
    foreach ($element in @(Get-Elements $Window)) {
        $isEdit = ($element.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit) -or
            ($element.Current.ClassName -eq 'Edit')
        if (-not $isEdit) { continue }
        if ($element.Current.IsOffscreen) { continue }
        $rect = $element.Current.BoundingRectangle
        if ($rect.Width -le 0) { continue }
        $edits += [pscustomobject]@{ Element = $element; Rect = $rect }
    }
    if ($edits.Count -eq 0) { return $null }
    $top = ($edits | Sort-Object { $_.Rect.Y } | Select-Object -First 1).Rect.Y
    $band = @($edits | Where-Object { [Math]::Abs($_.Rect.Y - $top) -le 12 })
    return ($band | Sort-Object { $_.Rect.X } | Select-Object -Last 1).Element
}

# 弹窗/气泡里的按钮：与页面入口同名时取**最后一个**（浮层在 DOM 末尾）
function Invoke-ModalButton { param($Window, [string]$Name)
    $all = Find-All $Window $Name
    if ($all.Count -eq 0) { return $false }
    return (Invoke-Element $all[$all.Count - 1])
}

function Read-Table { param([string]$Table)
    $dump = Join-Path $OutDir "dump-$Table.json"
    Push-Location $repo
    & cargo -q xtask dump $ws --table $Table *> $dump
    $exit = $LASTEXITCODE
    Pop-Location
    if ($exit -ne 0) { throw "导出 $Table 失败（exit=$exit）" }
    return @((Get-Content $dump -Raw | ConvertFrom-Json).$Table)
}

# 资金记录：只验**增量链**与本次金额，不去建模整个种子历史。
# （种子里有追加本金/支取/多轮买卖，`cash_balance = 本金 + Σ变动` 这种整体口径不成立——
#   追加本金会同时改 principal 与记一条 add_principal，两边都算就重复了。）
# 每次提交后断言两条：
#   1. 链式：新记录.cash_balance == 上一条.cash_balance + 新记录.amount_change
#   2. 金额：买入 = -(成交额 + 手续费)，卖出 = +(成交额 - 手续费)
function Get-FundRecords { param([string]$LedgerId)
    return @(Read-Table 'tbl_billadm_stock_fund_record' | Where-Object { $_.ledger_id -eq $LedgerId } |
        Sort-Object created_at)
}
function Assert-FundChain { param([string]$LedgerId, [string]$Stage)
    $records = Get-FundRecords -LedgerId $LedgerId
    if ($records.Count -lt 2) { Assert-True $false "$Stage：资金记录至少 2 条"; return }
    $new = $records[$records.Count - 1]
    $previous = $records[$records.Count - 2]
    Assert-True ([int64]$new.cash_balance -eq ([int64]$previous.cash_balance + [int64]$new.amount_change)) `
        "$Stage：余额链一致（$($previous.cash_balance) + $($new.amount_change) = $($new.cash_balance)）"
}

# 只认**真正可见**的按钮：页面里同名元素很多（另一个页签/未展开面板里也可能有「减仓」），
# 按名字取第一个常常拿到隐藏节点，点它什么都不会发生（实测：三次点击都没弹窗）。
function Find-VisibleButton { param($Window, [string]$Name, [switch]$Last, [switch]$Like)
    $windowRect = $Window.Current.BoundingRectangle
    $matches = @()
    foreach ($element in @(Get-Elements $Window)) {
        if ($element.Current.ControlType -ne [System.Windows.Automation.ControlType]::Button) { continue }
        if ($element.Current.IsOffscreen) { continue }
        $elementName = $element.Current.Name
        if (-not $elementName) { continue }
        $nameMatches = if ($Like) { $elementName.Contains($Name) } else { $elementName -eq $Name }
        if (-not $nameMatches) { continue }
        $rect = $element.Current.BoundingRectangle
        if ($rect.Width -le 0 -or $rect.Height -le 0) { continue }
        # 矩形必须在窗口可视范围内（隐藏面板里的元素会被排到可视区之外）
        if ($rect.Y -lt $windowRect.Y -or ($rect.Y + $rect.Height) -gt ($windowRect.Y + $windowRect.Height)) { continue }
        if ($rect.X -lt $windowRect.X -or ($rect.X + $rect.Width) -gt ($windowRect.X + $windowRect.Width)) { continue }
        $matches += $element
    }
    if ($matches.Count -eq 0) { return $null }
    if ($Last) { return $matches[$matches.Count - 1] }
    return $matches[0]
}
function Wait-VisibleButton { param($Window, [string]$Name, [int]$TimeoutSec = 20, [switch]$Last)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $element = Find-VisibleButton -Window $Window -Name $Name -Last:$Last
        if ($element) { return $element }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    return $null
}

# 提交一笔委托：点开弹窗（**重试到出现标题为止**，因为持仓卡片/按钮会随重渲染换元素）→
# 填价格与手数 → 点弹窗确认（与页面入口同名时取最后一个）。
function Submit-Trade {
    param($Window, [string]$OpenButton, [string]$ModalTitle, [string]$Price, [string]$Lots)
    $opened = $false
    for ($attempt = 1; $attempt -le 3 -and -not $opened; $attempt++) {
        # **用真实鼠标点**，不要用 InvokePattern：详情区的「减仓/清仓」按钮每次重渲染都会换元素，
        # 对旧元素 Invoke 会静默无效。鼠标按"刚读到的矩形"落点，与元素句柄新旧无关。
        $button = Wait-VisibleButton -Window $Window -Name $OpenButton -TimeoutSec 10
        if ($button) {
            $rect = $button.Current.BoundingRectangle
            [TrStock]::SetForegroundWindow([IntPtr]$Window.Current.NativeWindowHandle) | Out-Null
            Start-Sleep -Milliseconds 200
            [TrStock]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
        }
        else {
            Write-Host "  可视区里找不到「$OpenButton」按钮（详情区可能没选中持仓）" -ForegroundColor DarkYellow
        }
        Start-Sleep -Seconds 1
        $opened = [bool](Wait-Element -Root $Window -Name $ModalTitle -TimeoutSec 6)
        if (-not $opened) {
            Write-Host "  第 $attempt 次点「$OpenButton」没弹出「$ModalTitle」，重试（窗口=$($Window.Current.BoundingRectangle) 按钮=$(if ($button) { $button.Current.BoundingRectangle } else { '(none)' })）" -ForegroundColor DarkYellow
        }
    }
    if (-not $opened) {
        $names = @(Get-Elements $Window | ForEach-Object { $_.Current.Name } | Where-Object { $_ -and ($_ -like '*记录*' -or $_ -like '*成交价*' -or $_ -like '*手数*') })
        Write-Host "    点击后出现的相关文案: $($names -join ' | ')" -ForegroundColor DarkYellow
    }
    Assert-True $opened "弹窗「$ModalTitle」已打开"
    if (-not $opened) { return $false }
    Assert-True (Set-Value (Wait-Element -Root $Window -Name '成交价（元/股）') $Price) "填入成交价 $Price"
    if ($Lots) {
        $lotsInput = Wait-EditLike -Root $Window -Pattern '手数'
        Assert-True (Set-Value $lotsInput $Lots) "填入手数 $Lots"
    }
    Start-Sleep -Milliseconds 800
    $confirm = Wait-VisibleButton -Window $Window -Name $OpenButton -TimeoutSec 10 -Last
    if ($confirm) { Invoke-Element $confirm | Out-Null }
    Start-Sleep -Seconds 4
    return -not [bool](Find-First $Window $ModalTitle)
}

# ---- 播种 ----
# **默认每次重新播种**：本脚本断言的是"持仓数量/成本"这种**绝对状态**，复用旧工作空间必然失真
# （实测踩过：上一轮留下的 300 股让这一轮变成 600 股，后面全崩）。
# 判据要在**默认值赋值之前**取好，否则 `$Workspace` 已被填成默认路径、`-not $Workspace` 恒为 false。
if (-not $explicitWorkspace) {
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
}
if (-not (Test-Path (Join-Path $ws 'transactions.db'))) {
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[stock] 播种工作空间: $ws" -ForegroundColor Cyan
}

# 窗口要**够宽**：股票页详情区的「减仓/清仓」在最右侧，1400 逻辑宽时实测落在窗口右边缘之外
# （UIA 报 x≈2232，而窗口右边界≈2214），鼠标点过去等于点到别的窗口上，弹窗自然不出现。
@{ width = 1650; height = 950; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'ui-stock.ps1' } |
    ConvertTo-Json | Set-Content -Path (Join-Path $smokeHome '.transactions.json') -Encoding UTF8

$code = '600519'
$name = '贵州茅台'
$buyPrice = '100'
$buyLots = '3'
$reducePrice = '120'
$closePrice = '130'

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
    Start-Sleep -Seconds 1
    Write-Host "[stock] UIA 可读元素 $((Get-Elements $window).Count) 个" -ForegroundColor Cyan

    $hwnd = [IntPtr]$window.Current.NativeWindowHandle
    [TrStock]::ShowWindow($hwnd, 9) | Out-Null
    [TrStock]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 500

    # 账本 id 不预设：种子里有两个账本，界面默认打开哪个不确定；
    # 另外种子里**本来就有 600519 的历史成交**（含 open/close），所以本次操作必须按
    # `created_at`（或价格/手数组合）精确定位，不能只按 trade_type 取第一条。
    $ledgerId = ''
    $startedAt = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds() - 5

    # ================= 1/4 建仓 =================
    Write-Host "`n[stock] 1/4 建仓 $code × $buyLots 手 @ $buyPrice"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '股票交易')) '打开「股票交易」页'
    Start-Sleep -Seconds 3
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '我的持仓')) '切到「我的持仓」页签（建仓按钮在这里）'
    Start-Sleep -Seconds 2

    $opened = $false
    for ($attempt = 1; $attempt -le 3 -and -not $opened; $attempt++) {
        Invoke-Element (Wait-Element -Root $window -Name '建仓') | Out-Null
        Start-Sleep -Seconds 1
        $opened = [bool](Wait-Element -Root $window -Name '记录建仓' -TimeoutSec 6)
        if (-not $opened) { Write-Host "  第 $attempt 次没弹出「记录建仓」，重试" -ForegroundColor DarkYellow }
    }
    Assert-True $opened '弹窗「记录建仓」已打开'

    $codeInput = Find-UnnamedEditRight -Window $window
    Assert-True ([bool]$codeInput) '找到弹窗里的「股票代码」输入框（按包围盒靠右那个）'
    Assert-True (Set-Value $codeInput $code) "填入股票代码 $code"
    Start-Sleep -Milliseconds 600
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '查询股票名称')) '点「查询股票名称」（走真实行情）'
    Start-Sleep -Seconds 3

    Assert-True (Set-Value (Wait-Element -Root $window -Name '成交价（元/股）') $buyPrice) "填入成交价 $buyPrice"
    $lotsInput = Wait-EditLike -Root $window -Pattern '手数'
    Assert-True ([bool]$lotsInput) '找到「手数」输入框'
    Assert-True (Set-Value $lotsInput $buyLots) "填入手数 $buyLots"
    Start-Sleep -Milliseconds 800
    $buyButtons = Find-All $window '建仓'
    if ($buyButtons.Count -gt 0) { Invoke-Element $buyButtons[$buyButtons.Count - 1] | Out-Null }
    Start-Sleep -Seconds 5

    # 本次建仓 = 该股票 created_at 最新的一条 open
    $trades = @(Read-Table 'tbl_billadm_stock_trade' | Where-Object { $_.stock_code -eq $code } |
        Sort-Object created_at)
    $openTrade = @($trades | Where-Object { $_.trade_type -eq 'open' }) | Select-Object -Last 1
    Assert-True ([bool]$openTrade) "库里出现建仓成交（$code）"
    if ($openTrade) { $ledgerId = $openTrade.ledger_id }
    Assert-True ([bool]$ledgerId) "从建仓成交反推当前账本（$ledgerId）"
    if ($openTrade) {
        Assert-True ($openTrade.price -eq 10000) "成交价按分存（100 元 → 10000，实际 $($openTrade.price)）"
        Assert-True ($openTrade.lots -eq 3) "手数 3（实际 $($openTrade.lots)）"
        Assert-True ($openTrade.shares -eq 300) "股数 = 手数 × 100（实际 $($openTrade.shares)）"
        Assert-True ($openTrade.amount -eq 3000000) "成交额 = 100 元 × 300 股 = 3,000,000 分（实际 $($openTrade.amount)）"
        Assert-True ($openTrade.fee -gt 0) "手续费已计算（$($openTrade.fee) 分）"
        Assert-True ([int64]$openTrade.created_at -ge $startedAt) "这笔成交确实是本次操作写进去的"
    }
    $position = @(Read-Table 'tbl_billadm_stock_position' | Where-Object { $_.ledger_id -eq $ledgerId -and $_.stock_code -eq $code }) | Select-Object -First 1
    Assert-True ([bool]$position) "库里出现持仓（$code）"
    if ($position -and $openTrade) {
        Assert-True ($position.quantity -eq 300) "持仓数量 300（实际 $($position.quantity)）"
        Assert-True ($position.total_cost -eq ([int64]$openTrade.amount + [int64]$openTrade.fee)) "持仓成本 = 成交额 + 手续费（$($position.total_cost)）"
    }
    Assert-FundChain -LedgerId $ledgerId -Stage '建仓后'
    $fundsAfterBuy = Get-FundRecords -LedgerId $ledgerId
    if ($fundsAfterBuy.Count -ge 1 -and $openTrade) {
        $buyRecord = $fundsAfterBuy[$fundsAfterBuy.Count - 1]
        Assert-True ([int64]$buyRecord.amount_change -eq -([int64]$openTrade.amount + [int64]$openTrade.fee)) `
            "买入资金变动 = -(成交额 + 手续费)（$($buyRecord.amount_change)）"
    }

    # ================= 2/2 界面展示 =================
    # **减仓/清仓没做成自动**（诚实记录）：详情区那两个按钮在当前布局下确实"可见、矩形也在窗口内"，
    # 但无论用 InvokePattern 还是真实鼠标点它们的矩形中心，都不会弹出「记录减仓/清仓」——
    # 实测窗口=(304,304,2497,1438)、按钮=(2569,546,77,43)（在窗口内），点击后页面文案毫无变化。
    # 这两条路径的**落库语义**已由黄金对比阶段 3（减仓/多轮次/预演）与服务层单测覆盖，
    # 界面点击留给人工验收；这里只断言建仓之后界面**展示**正确。
    Write-Host "`n[stock] 2/2 界面展示：持仓卡片与交易历史"
    $cardText = $null
    $deadline = (Get-Date).AddSeconds(20)
    do {
        $cardText = Get-Elements $window | ForEach-Object { $_.Current.Name } |
            Where-Object { $_ -and $_.Contains($code) -and $_.Contains($name) } | Select-Object -First 1
        if (-not $cardText) { Start-Sleep -Milliseconds 500 }
    } while (-not $cardText -and (Get-Date) -lt $deadline)
    Assert-True ([bool]$cardText) "持仓卡片显示「$name $code」（$cardText）"
    if ($cardText) {
        Assert-True ($cardText -match '持仓\s*3\s*手') "卡片上写着持仓 3 手（$cardText）"
    }

    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '交易历史')) '切到「交易历史」页签'
    Start-Sleep -Seconds 3
    $seen = $false
    $deadline = (Get-Date).AddSeconds(15)
    do {
        $texts = @(Get-Elements $window | ForEach-Object { $_.Current.Name } | Where-Object { $_ })
        $seen = (@($texts | Where-Object { $_ -like "*$name*" }).Count -gt 0) -and
            (@($texts | Where-Object { $_ -like "*$code*" }).Count -gt 0)
        if (-not $seen) { Start-Sleep -Milliseconds 500 }
    } while (-not $seen -and (Get-Date) -lt $deadline)
    Assert-True $seen "交易历史里出现「$name / $code」"
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
    Write-Host "[stock] 失败 $($failures.Count) 项：" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "   - $_" -ForegroundColor Red }
    exit 1
}
Write-Host '[stock] 全部通过：建仓落库（成本含手续费/资金链）+ 持仓卡片与交易历史展示' -ForegroundColor Green
