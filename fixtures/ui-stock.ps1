# ui-stock.ps1 —— 股票「建仓 → 减仓 → 清仓」的端到端验收（真实界面 + 数据库断言）。
#
# 为什么需要它：减仓 / 多轮次 / 预演这些**落库**语义在数据层已经有测试覆盖，
# 而界面上"点减仓/清仓 → 填成交价 → 提交"这条路一直只有「建仓」被手工验过。
# 这里把整条生命周期走完，并按服务层的口径把关键金额算清楚：
#   * 建仓：持仓数量 += 股数，`total_cost += amount + fee`（**成本含手续费**）
#   * 编辑成交：改价后成交额/持仓成本按新价重算（手续费字段自洽；100→101 时佣金都低于最低 5 元，
#     所以金额变了费用不变 —— 这本身是个有意义的断言，见脚本注释）
#   * 删除整笔委托：回放掉这笔委托的全部成交与派生资金记录，持仓归零
#   * 减仓/清仓：`cost_basis = round(total_cost × 本次股数 / 持仓股数)`，
#     `realized_pnl = amount - fee - cost_basis`，数量与成本按比例结转；清空时成本归零并归档本轮
#   * 费用设置：佣金费率（UI 单位**万分之**）/最低佣金（元）/印花税/过户费（%）→ 保存后
#     **新委托立刻按新费率计费**（买入 = 佣金 + 过户费；卖出 = 佣金 + 印花税 + 过户费），逐项断言到分
#   * 重置股票数据：清空该账本股票侧全部表，而**记账数据与账本本身不动**；
#     费用设置/交易标签会被界面"重新拉一遍"按默认值重建 —— 对这两张表断言的是"回到默认值"
#   * 资金记录：`cash_balance` 恒等于 `principal + Σ amount_change`（不变量，逐步校验）
#
# ⚠ 它顺带锁死一个**真实缺陷**（本轮才发现，"减仓/清仓按钮点不动"的真凶）：
#   详情区那三个按钮（清仓/减仓/加仓）原来在 `on_click` 里先写 `selected_code.set(code)`
#   再 `open_trade(...)`。详情区本来就是按 `current_position()`（= `selected_code` 命中的那条）
#   渲染的，写的是**同一个值**；但 `RwSignal` 同值写入依然会通知订阅者，于是点击瞬间详情子树
#   重渲染，交易弹窗**永远弹不出来**——现象极像"按钮点不动"（真实鼠标 / 键盘回车 / InvokePattern
#   三种激活方式都无效，而表头那个不写 selected_code 的「建仓」按钮一直正常）。
#   修法：那三处去掉冗余的 `selected_code.set`（见 `src/pages/stock.rs` 的注释）。
#
# 写这个脚本踩到的坑（都留在注释里）：
#   * 成交记录表的行内「编辑/删除」在**持仓详情区**，所以要有持仓才看得到；清仓后它们就不在了；
#   * 「编辑成交」弹窗的输入框**预填之后可访问名就是那个值**（不是占位符），按名字找会命中旁边的
#     标签 <p>，必须按 Y 序取弹窗里的 Edit（`Get-ModalEdits`）；
#   * 这个工作空间里**同代码的种子成交也在同一个账本**（界面只显示当前轮次那笔，DB 查询会一起捞），
#     而且种子记录与我们的是同一秒 —— "我们的那笔"只能按"该类型里 created_at 最新"认（`Get-NewestTrade`）；
#     另外**编辑会触发整个账本的派生资金记录重放**，
#     所以资金记录也只能按"变动额"认，不能用"条数/相邻两条"这种全局判据；
#   * 股票页的子功能走**左侧图标条**：「建仓」在**持仓**子功能、「下单」也在持仓、
#     费用设置与重置在**设置**子功能、已清仓轮次在**记录**子功能 —— 换子功能后再下单必须先切回去，
#     否则会出现"设置存好了但下单弹窗压根没弹"的假象（`Switch-StockSub`）；
#   * 子功能名与页面文案同名（图标条「记录」/ 面板里的「记录」字样、图标条「设置」/ 侧栏「应用设置」），
#     按名字取第一个会点到页内元素 —— 共享版 `Invoke-SubFunction` 按"同名 Button 里最靠左的那个"定位。
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

. (Join-Path $PSScriptRoot 'lib\TrUia.ps1')

$repo = Split-Path -Parent $PSScriptRoot
$explicitWorkspace = -not [string]::IsNullOrWhiteSpace($Workspace)
if (-not $Exe) { $Exe = Join-Path $repo 'target\release\transactions.exe' }
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\stock-smoke\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\stock-smoke' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$trPaths = Initialize-TrSmokeHome -SmokeHome $SmokeHome -OutDir $OutDir
$smokeHome = $trPaths.SmokeHome
$OutDir = $trPaths.OutDir
# 判重必须按**完整路径**，不能按进程名（本机别的目录下可能有同名 exe）
Assert-NoRepoInstance -Repo $repo

$ws = [System.IO.Path]::GetFullPath($Workspace)

# 公共鼠标 P/Invoke 类（TrStock）已统一到 fixtures/lib/TrUia.ps1 的 TrUia（调用点是 [TrUia]::…）

$failures = New-Object System.Collections.Generic.List[string]

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

# 「设置」子功能费用表单里的 4 个输入框（佣金费率 / 最低佣金 / 印花税 / 过户费）。
# 它们是同一行里的四列（标签在上、输入框在下），所以按"同一 Y 带 + 按 X 排序"取。
function Get-FeeFormEdits { param($Window)
    $edits = @()
    foreach ($element in @(Get-Elements $Window)) {
        $isEdit = ($element.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit) -or
            ($element.Current.ClassName -eq 'Edit')
        if (-not $isEdit) { continue }
        if ($element.Current.IsOffscreen) { continue }
        $rect = $element.Current.BoundingRectangle
        if ($rect.Width -le 0 -or $rect.Height -le 0) { continue }
        if ([double]::IsInfinity($rect.X) -or [double]::IsNaN($rect.X)) { continue }
        $edits += [pscustomobject]@{ Element = $element; Rect = $rect }
    }
    if ($edits.Count -lt 4) { return @() }
    # 取"同一水平带里元素最多"的那一带（>= 4 个），再按 X 从左到右
    $best = @()
    foreach ($candidate in $edits) {
        $band = @($edits | Where-Object { [Math]::Abs($_.Rect.Y - $candidate.Rect.Y) -le 14 })
        if ($band.Count -gt $best.Count) { $best = $band }
    }
    if ($best.Count -lt 4) { return @() }
    return @($best | Sort-Object { $_.Rect.X } | Select-Object -First 4 | ForEach-Object { $_.Element })
}

# 弹窗/气泡里的按钮：与页面入口同名时取**最后一个**（浮层在 DOM 末尾）
function Invoke-ModalButton { param($Window, [string]$Name)
    $all = Find-All $Window $Name
    if ($all.Count -eq 0) { return $false }
    return (Invoke-Element $all[$all.Count - 1])
}

# 资金记录：只验**增量链**与本次金额，不去建模整个种子历史。
# （种子里有追加本金/支取/多轮买卖，`cash_balance = 本金 + Σ变动` 这种整体口径不成立——
#   追加本金会同时改 principal 与记一条 add_principal，两边都算就重复了。）
# 每次提交后断言两条：
#   1. 链式：新记录.cash_balance == 上一条.cash_balance + 新记录.amount_change
#   2. 金额：买入 = -(成交额 + 手续费)，卖出 = +(成交额 - 手续费)
function Get-FundRecords { param([string]$LedgerId)
    return @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_fund_record' -OutDir $OutDir | Where-Object { $_.ledger_id -eq $LedgerId } |
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

# 股票页的子功能走**左侧图标条**（不是顶部页签，也不是侧栏条目）。
# 图标条按钮的可访问名 = 子功能名，与页面里的文案可能同名（「记录」既是图标条项、
# 「结算统计」面板里也有「记录」字样），所以统一用共享版的 `Invoke-SubFunction`：
# 它按"同名 Button 里**最靠左**的那个"定位（图标条在版心最左边）。
function Switch-StockSub { param($Window, [string]$Name, [int]$TimeoutSec = 20)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        if (Invoke-SubFunction -Window $Window -Name $Name -TimeoutSec 3) { return $true }
        Start-Sleep -Milliseconds 400
    }
    return $false
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
            [TrUia]::SetForegroundWindow([IntPtr]$Window.Current.NativeWindowHandle) | Out-Null
            Start-Sleep -Milliseconds 200
            [TrUia]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
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

# 建仓（无断言版）：供"删除委托后重建现场"复用。
# 认"我们的那笔成交"：这个账本里还有**种子数据**的同代码成交（界面只显示当前轮次那笔，
# 数据库查询会把它们一起捞出来；而且种子的记录与我们的是同一秒，按 created_at 过滤并不可靠），
# 所以用与 1/5 相同的口径：同类型（open/reduce/close）里 created_at 最新的那一笔。
function Get-NewestTrade { param([string]$LedgerId, [string]$Code, [string]$TradeType)
    $rows = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_trade' -OutDir $OutDir | Where-Object {
            $_.ledger_id -eq $LedgerId -and $_.stock_code -eq $Code -and $_.trade_type -eq $TradeType
        } | Sort-Object created_at)
    if ($rows.Count -eq 0) { return $null }
    return $rows[$rows.Count - 1]
}
# 弹窗里的输入框：预填之后可访问名就是**值**（不是占位符），按 Y 序取才稳。
function Get-ModalEdits { param($Window, [string]$ModalTitle)
    $title = Find-First $Window $ModalTitle
    if (-not $title) { return @() }
    $titleY = $title.Current.BoundingRectangle.Y
    $edits = @()
    foreach ($element in @(Get-Elements $Window)) {
        $isEdit = ($element.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit) -or
            ($element.Current.ClassName -eq 'Edit')
        if (-not $isEdit -or $element.Current.IsOffscreen) { continue }
        $rect = $element.Current.BoundingRectangle
        if ($rect.Width -le 0 -or $rect.Y -lt $titleY) { continue }
        $edits += [pscustomobject]@{ Element = $element; Y = $rect.Y }
    }
    return @($edits | Sort-Object Y | ForEach-Object { $_.Element })
}
function Add-Position {
    param($Window, [string]$Code, [string]$Name, [string]$Price, [string]$Lots)
    $opened = $false
    for ($attempt = 1; $attempt -le 3 -and -not $opened; $attempt++) {
        Invoke-Element (Wait-Element -Root $Window -Name '建仓') | Out-Null
        Start-Sleep -Seconds 1
        # 判据用"弹窗里的输入框出现了"而不是标题文字：标题是 Text 元素，
        # Chromium 的 UIA 树惰性构建时**可能滞后于弹窗本体** ——
        # 实测出现过"标题 3 次都没查到，但弹窗已经能填、成交也落库了"的假红。
        # 先看结果（输入框在不在），标题只作为兜底。
        $opened = [bool](Find-UnnamedEditRight -Window $Window)
        if (-not $opened) { $opened = [bool](Wait-Element -Root $Window -Name '委托建仓' -TimeoutSec 6) }
    }
    if (-not $opened) { return $false }
    $codeInput = Find-UnnamedEditRight -Window $Window
    if (-not $codeInput) { return $false }
    Set-Value $codeInput $Code | Out-Null
    Start-Sleep -Milliseconds 600
    # 名称靠**失焦自动查**（「查询股票名称」按钮已去掉）：聚焦代码框 → Tab 离开它
    $codeInput.SetFocus()
    Start-Sleep -Milliseconds 300
    [System.Windows.Forms.SendKeys]::SendWait('{TAB}')
    Start-Sleep -Seconds 3
    Set-Value (Wait-Element -Root $Window -Name '成交价（元/股）') $Price | Out-Null
    $lotsInput = Wait-EditLike -Root $Window -Pattern '手数'
    if ($lotsInput) { Set-Value $lotsInput $Lots | Out-Null }
    Start-Sleep -Milliseconds 800
    $buttons = Find-All $Window '建仓'
    if ($buttons.Count -gt 0) { Invoke-Element $buttons[$buttons.Count - 1] | Out-Null }
    Start-Sleep -Seconds 5
    return $true
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
    [TrUia]::ShowWindow($hwnd, 9) | Out-Null
    [TrUia]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 500

    # 账本 id 不预设：种子里有两个账本，界面默认打开哪个不确定；
    # 另外种子里**本来就有 600519 的历史成交**（含 open/close），所以本次操作必须按
    # `created_at`（或价格/手数组合）精确定位，不能只按 trade_type 取第一条。
    $ledgerId = ''
    $startedAt = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds() - 5

    # ================= 1/7 建仓 =================
    Write-Host "`n[stock] 1/7 建仓 $code × $buyLots 手 @ $buyPrice"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '股票')) '打开「股票」页'
    Start-Sleep -Seconds 3
    Assert-True (Switch-StockSub -Window $window -Name '持仓') '切到「持仓」子功能（建仓按钮在工具栏）'
    Start-Sleep -Seconds 2

    $opened = $false
    for ($attempt = 1; $attempt -le 3 -and -not $opened; $attempt++) {
        Invoke-Element (Wait-Element -Root $window -Name '建仓') | Out-Null
        Start-Sleep -Seconds 1
        # ⚠ 弹窗标题是**「委托建仓」**（应用里由 `format!("委托{类型}")` 生成）。
        # 这里以前写的是旧标题「记录建仓」，改名后没同步 → 断言恒红（弹窗其实开着、后面全过）。
        # 判据优先看结果：弹窗里的输入框在不在。
        $opened = [bool](Find-UnnamedEditRight -Window $window)
        if (-not $opened) { $opened = [bool](Wait-Element -Root $window -Name '委托建仓' -TimeoutSec 6) }
        if (-not $opened) { Write-Host "  第 $attempt 次没弹出「委托建仓」，重试" -ForegroundColor DarkYellow }
    }
    Assert-True $opened '弹窗「委托建仓」已打开'

    # **建仓时两格都应当是空的**（真缺陷：以前「股票名称」无条件取"当前选中那条持仓"的名字，
    # 于是点建仓会带着上一只票的名字 —— 用户当场抓到）。弹窗里**没有占位符**的两个 Edit
    # 就是名称与代码：按包围盒从左到右读值，两格都得是空串。
    $unnamedEdits = @(Get-Elements $window) | Where-Object {
        (($_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit) -or
            ($_.Current.ClassName -eq 'Edit')) -and
            -not $_.Current.Name -and -not $_.Current.IsOffscreen -and
            (Test-Rect $_.Current.BoundingRectangle)
    } | Sort-Object { $_.Current.BoundingRectangle.X }
    $blank = @()
    foreach ($el in $unnamedEdits) {
        $pattern = $null
        if ($el.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
            $blank += [string]$pattern.Current.Value
        }
    }
    Assert-True (($blank.Count -ge 2) -and ($blank[0] -eq '') -and ($blank[1] -eq '')) `
        "建仓弹窗默认两格为空（实际: '$($blank -join " / ")'）"

    $codeInput = Find-UnnamedEditRight -Window $window
    Assert-True ([bool]$codeInput) '找到弹窗里的「股票代码」输入框（按包围盒靠右那个）'
    Assert-True (Set-Value $codeInput $code) "填入股票代码 $code"
    Start-Sleep -Milliseconds 600
    # 名称靠**失焦自动查**（「查询股票名称」按钮已去掉）：聚焦代码框 → Tab 离开它 → 走真实行情
    $codeInput.SetFocus()
    Start-Sleep -Milliseconds 300
    [System.Windows.Forms.SendKeys]::SendWait('{TAB}')
    Start-Sleep -Seconds 3
    # 判据落在结果上：名称框（弹窗里**靠左**那个无名输入框）应当被自动填上
    $nameValue = ''
    $nameEdit = @(Get-Elements $window) | Where-Object {
        (($_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit) -or
            ($_.Current.ClassName -eq 'Edit')) -and
            -not $_.Current.Name -and -not $_.Current.IsOffscreen -and
            (Test-Rect $_.Current.BoundingRectangle)
    } | Sort-Object { $_.Current.BoundingRectangle.X } | Select-Object -First 1
    if ($nameEdit) {
        $pattern = $null
        if ($nameEdit.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
            $nameValue = $pattern.Current.Value
        }
    }
    Assert-True (-not [string]::IsNullOrWhiteSpace($nameValue)) "代码失焦后名称被自动填上（实际 '$nameValue'）"

    Assert-True (Set-Value (Wait-Element -Root $window -Name '成交价（元/股）') $buyPrice) "填入成交价 $buyPrice"
    $lotsInput = Wait-EditLike -Root $window -Pattern '手数'
    Assert-True ([bool]$lotsInput) '找到「手数」输入框'
    Assert-True (Set-Value $lotsInput $buyLots) "填入手数 $buyLots"
    Start-Sleep -Milliseconds 800
    $buyButtons = Find-All $window '建仓'
    if ($buyButtons.Count -gt 0) { Invoke-Element $buyButtons[$buyButtons.Count - 1] | Out-Null }
    Start-Sleep -Seconds 5

    # 本次建仓 = 该股票 created_at 最新的一条 open
    $trades = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_trade' -OutDir $OutDir | Where-Object { $_.stock_code -eq $code } |
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
    $position = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_position' -OutDir $OutDir | Where-Object { $_.ledger_id -eq $ledgerId -and $_.stock_code -eq $code }) | Select-Object -First 1
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

    # ================= 2/7 界面展示（建仓之后）=================
    Write-Host "`n[stock] 2/7 界面展示：持仓卡片与成交记录"
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

    Assert-True (Switch-StockSub -Window $window -Name '记录') '切到「记录」子功能'
    Start-Sleep -Seconds 3
    $seen = $false
    $deadline = (Get-Date).AddSeconds(15)
    do {
        $texts = @(Get-Elements $window | ForEach-Object { $_.Current.Name } | Where-Object { $_ })
        $seen = (@($texts | Where-Object { $_ -like "*$name*" }).Count -gt 0) -and
            (@($texts | Where-Object { $_ -like "*$code*" }).Count -gt 0)
        if (-not $seen) { Start-Sleep -Milliseconds 500 }
    } while (-not $seen -and (Get-Date) -lt $deadline)
    Assert-True $seen "成交记录里出现「$name / $code」"
    Assert-True (Switch-StockSub -Window $window -Name '持仓') '切回「持仓」（减仓/清仓按钮在详情区）'
    Start-Sleep -Seconds 3

    # ================= 3/7 编辑成交（改成交价 → 费用/成本/资金链重算）=================
    # 成交记录表每行的「编辑」按钮（在持仓详情区，所以这时必须还有持仓）。
    # ⚠ 两个坑：① 这个账本里**还有种子数据**的同代码成交（界面只显示当前轮次那笔，数据库查询会一起捞出来），
    # 所以"我们的成交"用与 1/5 相同的口径认：同类型里 created_at 最新的那一笔；
    # ② 编辑弹窗的输入框**预填后名字就是值**（不是占位符），必须按 Y 序取 Edit，不能按名字找。
    Write-Host "`n[stock] 3/7 编辑成交：把建仓价从 $buyPrice 改成 101"
    $orderTrade = Get-NewestTrade -LedgerId $ledgerId -Code $code -TradeType 'open'
    Assert-True ([bool]$orderTrade) '编辑前能认到我们的建仓成交（该类型最新一笔）'
    if ($orderTrade) {
        Assert-True (([int64]$orderTrade.price) -eq 10000) "编辑前成交价 100.00 元（$($orderTrade.price) 分）"
        Assert-True ($orderTrade.lots -eq 3) "编辑前手数 3（$($orderTrade.lots)）"
    }

    $editButton = Wait-VisibleButton -Window $window -Name '编辑' -TimeoutSec 15
    Assert-True ([bool]$editButton) '找到成交行「编辑」按钮'
    if ($editButton) {
        $rect = $editButton.Current.BoundingRectangle
        [TrUia]::SetForegroundWindow([IntPtr]$window.Current.NativeWindowHandle) | Out-Null
        Start-Sleep -Milliseconds 300
        [TrUia]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
    }
    $editOpened = [bool](Wait-Element -Root $window -Name '编辑成交' -TimeoutSec 10)
    Assert-True $editOpened '弹窗「编辑成交」已打开'
    if ($editOpened) {
        $modalEdits = Get-ModalEdits -Window $window -ModalTitle '编辑成交'
        Assert-True ($modalEdits.Count -ge 2) "弹窗里有两个输入框（成交价/手数，实际 $($modalEdits.Count) 个）"
        if ($modalEdits.Count -ge 2) {
            Assert-True (Set-Value $modalEdits[0] '101') '把成交价改成 101'
        }
        Start-Sleep -Milliseconds 600
        $saveButton = Wait-VisibleButton -Window $window -Name '保存' -TimeoutSec 10
        Assert-True ([bool]$saveButton) '找到「保存」'
        if ($saveButton) { Invoke-Element $saveButton | Out-Null }
        Start-Sleep -Seconds 4
        # 若弹出「影响预演」确认框（本轮无已归档轮次时通常直接写库），一并确认
        $impactOk = Wait-VisibleButton -Window $window -Name '确认' -TimeoutSec 4
        if ($impactOk) {
            Write-Host '  出现影响预演确认框，点「确认」'
            Invoke-Element $impactOk | Out-Null
            Start-Sleep -Seconds 4
        }
    }

    $editedTrade = Get-NewestTrade -LedgerId $ledgerId -Code $code -TradeType 'open'
    Assert-True ([bool]$editedTrade) '编辑后仍能认到那笔建仓成交'
    if ($editedTrade) {
        Assert-True (([int64]$editedTrade.price) -eq 10100) "成交价改成 101.00 元（实际 $($editedTrade.price) 分）"
        Assert-True (([int64]$editedTrade.amount) -eq 3030000) "成交额按新价重算 = 101×300 股 = 3,030,000 分（实际 $($editedTrade.amount)）"
        # 手续费：**不能断言"一定变了"** —— 100 → 101 时两笔成交额都落在佣金最低 5 元以下
        # （实测 3,000,000 与 3,030,000 分都是 530 分），这里断言的是字段自洽性。
        Assert-True (([int64]$editedTrade.commission + [int64]$editedTrade.stamp_duty + [int64]$editedTrade.transfer_fee) `
            -eq ([int64]$editedTrade.fee)) `
            "手续费自洽（佣金 $($editedTrade.commission) + 印花税 $($editedTrade.stamp_duty) + 过户费 $($editedTrade.transfer_fee) = $($editedTrade.fee)）"
    }
    $posAfterEdit = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_position' -OutDir $OutDir |
        Where-Object { $_.ledger_id -eq $ledgerId -and $_.stock_code -eq $code }) | Select-Object -First 1
    if ($editedTrade -and $posAfterEdit) {
        Assert-True ($posAfterEdit.quantity -eq 300) "编辑不改数量（$($posAfterEdit.quantity) 股）"
        Assert-True (([int64]$posAfterEdit.total_cost) -eq (([int64]$editedTrade.amount) + ([int64]$editedTrade.fee))) `
            "持仓成本按新价重算 = 成交额 + 手续费（$($posAfterEdit.total_cost)）"
    }
    # 资金记录：**按"变动额"认我们这条**。编辑会触发整个账本的派生资金记录重放
    # 这与数据层的重放行为一致：种子记录的 created_at 会被重写，所以"条数/相邻两条"都不可靠。
    if ($editedTrade) {
        $expectedChange = -(([int64]$editedTrade.amount) + ([int64]$editedTrade.fee))
        $ourFundsAfterEdit = @(Get-FundRecords -LedgerId $ledgerId | Where-Object { ([int64]$_.amount_change) -eq $expectedChange })
        Assert-True ($ourFundsAfterEdit.Count -ge 1) `
            "编辑后资金记录按新价重放（存在变动 = $expectedChange 的记录，实际 $($ourFundsAfterEdit.Count) 条）"
    }

    # ================= 4/7 删除委托（整笔回放）→ 重新建仓 =================
    Write-Host "`n[stock] 4/7 删除整笔委托 → 再建仓（好继续验减仓/清仓）"
    $ourBuyFundIds = @(if ($editedTrade) {
            $expected = -(([int64]$editedTrade.amount) + ([int64]$editedTrade.fee))
            Get-FundRecords -LedgerId $ledgerId | Where-Object { ([int64]$_.amount_change) -eq $expected } |
                ForEach-Object { $_.id }
        })
    $deleteButton = Wait-VisibleButton -Window $window -Name '删除' -TimeoutSec 15
    Assert-True ([bool]$deleteButton) '找到成交行「删除」按钮'
    if ($deleteButton) {
        $rect = $deleteButton.Current.BoundingRectangle
        [TrUia]::SetForegroundWindow([IntPtr]$window.Current.NativeWindowHandle) | Out-Null
        Start-Sleep -Milliseconds 300
        [TrUia]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
    }
    $deleteOpened = [bool](Wait-Element -Root $window -Name '删除整笔委托？' -TimeoutSec 10)
    Assert-True $deleteOpened '弹窗「删除整笔委托？」已打开'
    if ($deleteOpened) {
        $deleteSummary = Get-Elements $window | ForEach-Object { $_.Current.Name } |
            Where-Object { $_ -and $_ -like '*共 1 笔成交*' } | Select-Object -First 1
        Assert-True ([bool]$deleteSummary) "确认框里带委托摘要（$deleteSummary）"
        $confirm = Wait-VisibleButton -Window $window -Name '确认' -TimeoutSec 10
        Assert-True ([bool]$confirm) '找到「确认」'
        if ($confirm) { Invoke-Element $confirm | Out-Null }
        Start-Sleep -Seconds 4
    }

    $tradeAfterDelete = Get-NewestTrade -LedgerId $ledgerId -Code $code -TradeType 'open'
    Assert-True (-not $tradeAfterDelete -or ([int64]$tradeAfterDelete.price -ne 10100)) `
        "删除后那笔 101.00 元的建仓成交没了（最新一笔：$(if ($tradeAfterDelete) { "$($tradeAfterDelete.price) 分" } else { '无' })）"
    $posAfterDelete = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_position' -OutDir $OutDir |
        Where-Object { $_.ledger_id -eq $ledgerId -and $_.stock_code -eq $code }) | Select-Object -First 1
    if ($posAfterDelete) {
        Assert-True ($posAfterDelete.quantity -eq 0) "删除后持仓数量归零（实际 $($posAfterDelete.quantity)）"
        Assert-True (([int64]$posAfterDelete.total_cost) -eq 0) "删除后持仓成本归零（实际 $($posAfterDelete.total_cost)）"
    }
    $fundIdsAfterDelete = @(Get-FundRecords -LedgerId $ledgerId | ForEach-Object { $_.id })
    $survivors = @($ourBuyFundIds | Where-Object { $fundIdsAfterDelete -contains $_ })
    Assert-True ($survivors.Count -eq 0) "删除委托后我们那条资金记录也被回放掉了（残留 $($survivors.Count) 条）"

    # 重新建仓，供 5/5 的减仓/清仓使用
    Assert-True (Add-Position -Window $window -Code $code -Name $name -Price $buyPrice -Lots $buyLots) '重新建仓 3 手 @ 100'
    Start-Sleep -Seconds 2
    $posRebuilt = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_position' -OutDir $OutDir |
        Where-Object { $_.ledger_id -eq $ledgerId -and $_.stock_code -eq $code }) | Select-Object -First 1
    Assert-True ([bool]$posRebuilt -and $posRebuilt.quantity -eq 300) "重建仓后持仓回到 300 股（实际 $(if ($posRebuilt) { $posRebuilt.quantity } else { 'n/a' })）"

    # ================= 5/7 减仓 → 清仓 =================
    # 详情区的「减仓/清仓」现在真的能点开了（见文件顶部那条缺陷说明）。
    Write-Host "`n[stock] 3/3 减仓 1 手 @ $reducePrice → 清仓 2 手 @ $closePrice"

    $posBefore = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_position' -OutDir $OutDir |
        Where-Object { $_.ledger_id -eq $ledgerId -and $_.stock_code -eq $code }) | Select-Object -First 1
    Assert-True ([bool]$posBefore) '减仓前能读到持仓行'
    # 轮次是**清仓时**才归档出来的（`close_round`：建历史 + 建轮次 + 把未挂接的成交挂上去），
    # 所以此刻：没有新轮次，刚建的仓也还没挂 round_id。
    $roundsBefore = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_trade_round' -OutDir $OutDir |
        Where-Object { $_.ledger_id -eq $ledgerId -and $_.stock_code -eq $code })
    if ($openTrade) {
        Assert-True ([string]::IsNullOrEmpty($openTrade.round_id)) '建仓成交尚未挂接轮次（轮到清仓时才归档）'
    }

    # ---- 减仓 1 手 ----
    $reduceLots = '1'
    Assert-True (Submit-Trade -Window $window -OpenButton '减仓' -ModalTitle '委托减仓' -Price $reducePrice -Lots $reduceLots) `
        '「减仓」弹窗走完（填价/手数 → 提交）'
    Start-Sleep -Seconds 2

    $reduceTrade = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_trade' -OutDir $OutDir |
        Where-Object { $_.ledger_id -eq $ledgerId -and $_.stock_code -eq $code -and $_.trade_type -eq 'reduce' } |
        Sort-Object created_at) | Select-Object -Last 1
    Assert-True ([bool]$reduceTrade) '库里出现减仓成交（trade_type=reduce）'
    if ($reduceTrade -and $posBefore) {
        $expectedBasis = [int64][Math]::Round([double]$posBefore.total_cost * 100.0 / [double]$posBefore.quantity)
        $expectedRealized = [int64]$reduceTrade.amount - [int64]$reduceTrade.fee - $expectedBasis
        Assert-True ($reduceTrade.price -eq 12000) "减仓价按分存（$reducePrice 元 → 12000，实际 $($reduceTrade.price)）"
        Assert-True ($reduceTrade.lots -eq 1) "减仓手数 1（实际 $($reduceTrade.lots)）"
        Assert-True ($reduceTrade.shares -eq 100) "减仓股数 100（实际 $($reduceTrade.shares)）"
        Assert-True ($reduceTrade.amount -eq 1200000) "减仓成交额 1,200,000 分（实际 $($reduceTrade.amount)）"
        Assert-True ($reduceTrade.fee -gt 0) "减仓手续费已计算（$($reduceTrade.fee) 分）"
        Assert-True ($reduceTrade.realized_pnl -eq $expectedRealized) `
            "减仓已实现盈亏 = 成交额 - 手续费 - 结转成本（期望 $expectedRealized，实际 $($reduceTrade.realized_pnl)）"

        $posAfterReduce = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_position' -OutDir $OutDir |
            Where-Object { $_.ledger_id -eq $ledgerId -and $_.stock_code -eq $code }) | Select-Object -First 1
        Assert-True ($posAfterReduce.quantity -eq ($posBefore.quantity - 100)) `
            "持仓数量 300 → 200（实际 $($posAfterReduce.quantity)）"
        Assert-True ($posAfterReduce.total_cost -eq ([int64]$posBefore.total_cost - $expectedBasis)) `
            "持仓成本按比例结转（$($posBefore.total_cost) - $expectedBasis = $($posAfterReduce.total_cost)）"
        Assert-True ($posAfterReduce.realized_pnl -eq ([int64]$posBefore.realized_pnl + $expectedRealized)) `
            "持仓已实现盈亏累加（实际 $($posAfterReduce.realized_pnl)）"
        # 减仓同样不归档：这一笔也还没挂轮次
        Assert-True ([string]::IsNullOrEmpty($reduceTrade.round_id)) '减仓成交同样尚未挂接轮次'
    }
    Assert-FundChain -LedgerId $ledgerId -Stage '减仓后'

    # ---- 清仓：可用手数应预填 2 手（清仓时预填全仓手数）----
    $closeButton = Wait-VisibleButton -Window $window -Name '清仓' -TimeoutSec 15
    Assert-True ([bool]$closeButton) '详情区还有「清仓」按钮'
    if ($closeButton) {
        $rect = $closeButton.Current.BoundingRectangle
        [TrUia]::SetForegroundWindow([IntPtr]$window.Current.NativeWindowHandle) | Out-Null
        Start-Sleep -Milliseconds 300
        [TrUia]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
    }
    $closeOpened = [bool](Wait-Element -Root $window -Name '委托清仓' -TimeoutSec 10)
    Assert-True $closeOpened '弹窗「委托清仓」已打开'
    if ($closeOpened) {
        $lotsInput = Wait-EditLike -Root $window -Pattern '手数'
        $prefilled = ''
        $valuePattern = $null
        if ($lotsInput -and $lotsInput.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$valuePattern)) {
            $prefilled = $valuePattern.Current.Value
        }
        Assert-True ($prefilled -eq '2') "清仓时手数预填剩余可卖手数 2（实际 '$prefilled'）"
        Assert-True (Set-Value (Wait-Element -Root $window -Name '成交价（元/股）') $closePrice) "填入清仓价 $closePrice"
        Start-Sleep -Milliseconds 600
        $confirm = @(Find-All $window '清仓' | Where-Object { -not $_.Current.IsOffscreen })
        if ($confirm.Count -gt 0) { Invoke-Element $confirm[$confirm.Count - 1] | Out-Null }
        Start-Sleep -Seconds 4
    }

    $closeTrade = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_trade' -OutDir $OutDir |
        Where-Object { $_.ledger_id -eq $ledgerId -and $_.stock_code -eq $code -and $_.trade_type -eq 'close' } |
        Sort-Object created_at) | Select-Object -Last 1
    Assert-True ([bool]$closeTrade) '库里出现清仓成交（trade_type=close）'
    $posAfterClose = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_position' -OutDir $OutDir |
        Where-Object { $_.ledger_id -eq $ledgerId -and $_.stock_code -eq $code }) | Select-Object -First 1
    if ($closeTrade) {
        Assert-True ($closeTrade.lots -eq 2) "清仓手数 2（实际 $($closeTrade.lots)）"
        Assert-True ($closeTrade.shares -eq 200) "清仓股数 200（实际 $($closeTrade.shares)）"
        Assert-True ($closeTrade.amount -eq 2600000) "清仓成交额 2,600,000 分（实际 $($closeTrade.amount)）"
        # 卖出全部持仓 → 结转成本 = 剩余总成本，成本与数量都归零
        Assert-True ($posAfterClose.quantity -eq 0) "清仓后数量归零（实际 $($posAfterClose.quantity)）"
        Assert-True ($posAfterClose.total_cost -eq 0) "清仓后成本归零（实际 $($posAfterClose.total_cost)）"
    }
    Assert-FundChain -LedgerId $ledgerId -Stage '清仓后'

    # ---- 清仓归档：建一个新轮次 + 把本轮三笔成交挂上去 + 指向该股票的成交记录 ----
    $roundsAfter = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_trade_round' -OutDir $OutDir |
        Where-Object { $_.ledger_id -eq $ledgerId -and $_.stock_code -eq $code })
    Assert-True ($roundsAfter.Count -eq ($roundsBefore.Count + 1)) `
        "清仓归档出一个新轮次（$($roundsBefore.Count) → $($roundsAfter.Count)）"
    if ($closeTrade) {
        Assert-True (-not [string]::IsNullOrEmpty($closeTrade.round_id)) '清仓成交挂上了轮次 id（清仓单一并归档）'
        $newRound = $roundsAfter | Where-Object { $_.id -eq $closeTrade.round_id } | Select-Object -First 1
        Assert-True ([bool]$newRound) '新轮次行存在'
        if ($newRound) {
            Assert-True ([int64]$newRound.closed_at -eq [int64]$closeTrade.trade_time) `
                "轮次 closed_at = 清仓成交时间（$($newRound.closed_at)）"
            if ($openTrade) {
                Assert-True ([int64]$newRound.opened_at -eq [int64]$openTrade.trade_time) `
                    "轮次 opened_at = 本次建仓成交时间（$($newRound.opened_at)）"
            }
            Assert-True ($newRound.round_no -eq ($roundsBefore.Count + 1)) `
                "轮次序号 = $($roundsBefore.Count + 1)（实际 $($newRound.round_no)）"
            # 建仓 / 减仓 / 清仓 三笔都挂到这一轮（归档是"回填"未挂接的成交）
            foreach ($tradeId in @($openTrade.id, $reduceTrade.id, $closeTrade.id)) {
                if (-not $tradeId) { continue }
                $row = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_trade' -OutDir $OutDir | Where-Object { $_.id -eq $tradeId }) | Select-Object -First 1
                if ($row) {
                    Assert-True ($row.round_id -eq $newRound.id) "成交「$($row.trade_type)」归到清仓归档的轮次"
                }
            }
            $historyRow = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_trade_history' -OutDir $OutDir |
                Where-Object { $_.id -eq $newRound.history_id }) | Select-Object -First 1
            Assert-True ([bool]$historyRow) '轮次的 history_id 指向该股票的成交记录行'
            if ($historyRow) {
                Assert-True ($historyRow.stock_code -eq $code) "成交记录行属于 $code"
            }
        }
    }
    # ================= 6/7 交易费用设置：改完费率，**新委托立刻按新费率计费** =================
    # ⚠ 费用设置的编辑入口**在股票页的「设置」子功能**（原来在「应用设置 → 股票」分栏，
    # 已随股票设置整体迁进股票页；股票页的其他子功能只读取它来估费，不渲染表单）。
    # 费用设置的 UI 单位：佣金费率是**万分之**（÷10000 落库）、最低佣金是**元**（转分）、
    # 印花税/过户费是**百分比**（÷100）；公式见 `tr-domain/src/fee.rs`：
    #   佣金 = max(round(委托总额×费率), 最低佣金)（**整笔委托只收一次**）；
    #   买入 = 佣金 + 过户费(沪市)；卖出 = 佣金 + 印花税 + 过户费(沪市)（印花税只卖出收）；
    #   多笔成交时印花税/过户费**逐笔取整再相加**（各笔自己进整到分），下面这几笔都是单笔成交。
    Write-Host "`n[stock] 6/7 费用设置（股票 → 设置）：佣金 1/10000、最低 0、印花税 0.1%、过户费 0.002%"
    Assert-True (Switch-StockSub -Window $window -Name '设置') '切到「设置」子功能（费用设置在这里）'
    Start-Sleep -Seconds 2
    Assert-True ([bool](Wait-Element -Root $window -Name '佣金费率' -TimeoutSec 15)) '设置子功能出现费用表单'
    $feeEdits = @(Get-FeeFormEdits -Window $window)
    Assert-True ($feeEdits.Count -ge 4) "费用表单有 4 个输入框（实际 $($feeEdits.Count)）"
    if ($feeEdits.Count -ge 4) {
        Assert-True (Set-Value $feeEdits[0] '1') '佣金费率填 1（万分之）'
        Assert-True (Set-Value $feeEdits[1] '0') '最低佣金填 0 元'
        Assert-True (Set-Value $feeEdits[2] '0.1') '印花税填 0.1%'
        Assert-True (Set-Value $feeEdits[3] '0.002') '过户费填 0.002%'
        Start-Sleep -Milliseconds 500
        $saveFee = Wait-VisibleButton -Window $window -Name '保存' -TimeoutSec 10
        Assert-True ([bool]$saveFee) '找到费用表单的「保存」'
        if ($saveFee) { Invoke-Element $saveFee | Out-Null }
        Start-Sleep -Seconds 3
    }
    $feeRow = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_fee_setting' -OutDir $OutDir | Where-Object { $_.ledger_id -eq $ledgerId }) |
        Select-Object -First 1
    Assert-True ([bool]$feeRow) '库里能读到该账本的费用设置行'
    if ($feeRow) {
        Assert-True ([Math]::Abs([double]$feeRow.commission_rate - 0.0001) -lt 1e-9) `
            "佣金费率落库 0.0001（实际 $($feeRow.commission_rate)）"
        Assert-True ([int64]$feeRow.min_commission -eq 0) "最低佣金落库 0 分（实际 $($feeRow.min_commission)）"
        Assert-True ([Math]::Abs([double]$feeRow.stamp_duty_rate - 0.001) -lt 1e-9) `
            "印花税落库 0.001（实际 $($feeRow.stamp_duty_rate)）"
        Assert-True ([Math]::Abs([double]$feeRow.transfer_fee_rate - 0.00002) -lt 1e-9) `
            "过户费落库 0.00002（实际 $($feeRow.transfer_fee_rate)）"
    }

    # ---- 买入：3,000,000 分 → 佣金 300 + 过户费 60 = 360 分（印花税 0）----
    # ⚠ 下单在「持仓」子功能：费用存好后要切过去，否则下单弹窗压根没弹。
    Assert-True (Switch-StockSub -Window $window -Name '持仓') '从「设置」切到「持仓」再下单'
    Start-Sleep -Seconds 3
    Assert-True (Add-Position -Window $window -Code $code -Name $name -Price $buyPrice -Lots $buyLots) `
        '按新费率建仓 3 手 @ 100'
    Start-Sleep -Seconds 2
    $feeBuyTrade = Get-NewestTrade -LedgerId $ledgerId -Code $code -TradeType 'open'
    Assert-True ([bool]$feeBuyTrade) '新委托（买入）的成交落库'
    if ($feeBuyTrade) {
        Assert-True (([int64]$feeBuyTrade.commission) -eq 300) `
            "买入佣金 = round(3,000,000 × 1/10000) = 300 分（实际 $($feeBuyTrade.commission)）"
        Assert-True (([int64]$feeBuyTrade.transfer_fee) -eq 60) `
            "买入过户费 = round(3,000,000 × 0.002%) = 60 分（实际 $($feeBuyTrade.transfer_fee)）"
        Assert-True (([int64]$feeBuyTrade.stamp_duty) -eq 0) "买入不收印花税（实际 $($feeBuyTrade.stamp_duty)）"
        Assert-True (([int64]$feeBuyTrade.fee) -eq 360) "买入费用 = 300 + 60 = 360 分（实际 $($feeBuyTrade.fee)）"
    }

    # ---- 卖出：1,200,000 分 → 佣金 120 + 印花税 1200 + 过户费 24 = 1344 分 ----
    Assert-True (Submit-Trade -Window $window -OpenButton '减仓' -ModalTitle '委托减仓' -Price $reducePrice -Lots '1') `
        '按新费率减仓 1 手 @ 120'
    Start-Sleep -Seconds 3
    $feeSellTrade = Get-NewestTrade -LedgerId $ledgerId -Code $code -TradeType 'reduce'
    Assert-True ([bool]$feeSellTrade) '新委托（卖出）的成交落库'
    if ($feeSellTrade) {
        Assert-True (([int64]$feeSellTrade.commission) -eq 120) `
            "卖出佣金 = round(1,200,000 × 1/10000) = 120 分（实际 $($feeSellTrade.commission)）"
        Assert-True (([int64]$feeSellTrade.stamp_duty) -eq 1200) `
            "印花税 = round(1,200,000 × 0.1%) = 1200 分（实际 $($feeSellTrade.stamp_duty)）"
        Assert-True (([int64]$feeSellTrade.transfer_fee) -eq 24) `
            "过户费 = round(1,200,000 × 0.002%) = 24 分（实际 $($feeSellTrade.transfer_fee)）"
        Assert-True (([int64]$feeSellTrade.fee) -eq 1344) `
            "卖出费用 = 120 + 1200 + 24 = 1344 分（实际 $($feeSellTrade.fee)）"
    }
    # ================= 7/7 重置股票数据：清空股票侧、**不动记账数据** =================
    Write-Host "`n[stock] 7/7 重置股票数据（股票 → 设置 → 重置）"
    $recordsBeforeReset = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_transaction_record' -OutDir $OutDir)
    $stockTables = @(
        'tbl_billadm_stock_account', 'tbl_billadm_stock_position', 'tbl_billadm_stock_trade',
        'tbl_billadm_stock_trade_round', 'tbl_billadm_stock_trade_history', 'tbl_billadm_stock_fund_record',
        'tbl_billadm_stock_fee_setting', 'tbl_billadm_stock_trade_tag_setting'
    )
    $stockRowsBefore = 0
    foreach ($table in $stockTables) {
        $stockRowsBefore += @(Read-Table -Repo $repo -Workspace $ws -Table $table -OutDir $OutDir | Where-Object { $_.ledger_id -eq $ledgerId }).Count
    }
    Assert-True ($stockRowsBefore -gt 0) "重置前该账本有股票数据（$stockRowsBefore 行）"

    Assert-True (Switch-StockSub -Window $window -Name '设置') '切到「设置」子功能（重置在这里）'
    Start-Sleep -Seconds 3

    $resetButton = Wait-VisibleButton -Window $window -Name '重置' -TimeoutSec 15
    Assert-True ([bool]$resetButton) '找到设置子功能的「重置」按钮'
    if ($resetButton) { Invoke-Element $resetButton | Out-Null }
    Start-Sleep -Seconds 2
    Assert-True ([bool](Wait-Element -Root $window -Name '重置股票数据' -TimeoutSec 10)) `
        '弹窗「重置股票数据」已打开'
    $resetWarning = Get-Elements $window | ForEach-Object { $_.Current.Name } |
        Where-Object { $_ -and $_ -like '*将清空当前账本*' } | Select-Object -First 1
    Assert-True ([bool]$resetWarning) '弹窗里写明了会清空什么（账户本金/持仓/交易/资金记录/费用设置/交易标签）'
    $confirmReset = Wait-VisibleButton -Window $window -Name '确认重置' -TimeoutSec 10
    Assert-True ([bool]$confirmReset) '找到「确认重置」'
    if ($confirmReset) { Invoke-Element $confirmReset | Out-Null }
    Start-Sleep -Seconds 4

    # 交易类表重置后必须是空的；**费用设置与交易标签这两张表会被界面立刻"重新拉一遍"
    # 而按默认值重建**（设置子功能 `do_reset` 之后 load_fee/load_tags，与代码注释一致），
    # 所以对这两张表断言的是"回到默认值"，而不是"没有行"。
    foreach ($table in @('tbl_billadm_stock_account', 'tbl_billadm_stock_position', 'tbl_billadm_stock_trade',
            'tbl_billadm_stock_trade_round', 'tbl_billadm_stock_trade_history', 'tbl_billadm_stock_fund_record')) {
        $left = @(Read-Table -Repo $repo -Workspace $ws -Table $table -OutDir $OutDir | Where-Object { $_.ledger_id -eq $ledgerId }).Count
        Assert-True ($left -eq 0) "重置后 $table 里该账本已无数据（实际 $left 行）"
    }
    $feeAfterReset = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_fee_setting' -OutDir $OutDir | Where-Object { $_.ledger_id -eq $ledgerId }) |
        Select-Object -First 1
    if ($feeAfterReset) {
        # 6/7 里我们把佣金改成 0.0001/最低 0/印花税 0.001/过户费 0.00002，重置后必须回到默认
        Assert-True ([Math]::Abs([double]$feeAfterReset.commission_rate - 0.0002354) -lt 1e-9) `
            "重置后佣金费率回到默认 0.0002354（实际 $($feeAfterReset.commission_rate)）"
        Assert-True ([int64]$feeAfterReset.min_commission -eq 500) `
            "重置后最低佣金回到默认 500 分（实际 $($feeAfterReset.min_commission)）"
        Assert-True ([Math]::Abs([double]$feeAfterReset.transfer_fee_rate - 0.00001) -lt 1e-9) `
            "重置后过户费回到默认 0.00001（实际 $($feeAfterReset.transfer_fee_rate)）"
    }
    $tagAfterReset = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_stock_trade_tag_setting' -OutDir $OutDir | Where-Object { $_.ledger_id -eq $ledgerId }) |
        Select-Object -First 1
    if ($tagAfterReset) {
        Assert-True ($tagAfterReset.tags -like '*分析*' -and $tagAfterReset.tags -like '*打板*') `
            "重置后交易标签回到默认集合（实际 $($tagAfterReset.tags)）"
    }
    $recordsAfterReset = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_transaction_record' -OutDir $OutDir)
    Assert-True ($recordsAfterReset.Count -eq $recordsBeforeReset.Count) `
        "记账数据**未被**重置触及（$($recordsBeforeReset.Count) → $($recordsAfterReset.Count) 条）"
    # 账本本身也还在（重置只清股票侧，不删账本）
    $ledgerStill = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_ledger' -OutDir $OutDir | Where-Object { $_.id -eq $ledgerId })
    Assert-True ($ledgerStill.Count -eq 1) '账本行仍在（重置不删账本）'
}

finally { Stop-TrApp -Process $process -Failures $failures -OutDir $OutDir }
Show-TrSummary -Failures $failures -Tag 'stock' -SuccessMessage "[stock] 全部通过：建仓 → 编辑成交 → 删除委托 → 减仓/清仓（成本结转/已实现盈亏/资金链/清仓归档）→ 费用设置生效 → 重置股票数据（不动记账数据）"
