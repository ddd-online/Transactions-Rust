# ui-smoke.ps1 —— 逐页界面冒烟：用 UI Automation 驱动真实窗口，挨个点开 5 个顶级功能 +
# 记账页的 4 个子功能（记录 / 分析 / 标签 / 模板）与股票页的 5 个子功能
# （账户 / 持仓 / 记录 / 统计 / 设置，都走左侧图标条）并断言内容渲染。
# 注：原「数据分析」顶级页已并入记账并更名「分析」，所以顶级功能从 6 个减为 5 个。
#
# 为什么需要它：`fixtures/smoke.ps1` 只能证明"应用起来了、工作空间打开了"，
# 证明不了"每个页面点开是好的"。自动化脚本覆盖不了的部分仍需人工逐页验收，
# 但这个脚本能把"页面能不能打开、有没有明显渲染"这一层自动化，人工只需看视觉细节。
#
# 机制：WebView2 会把 DOM 暴露成 UIA 树（首次查询后 1-2 秒才建好）。
# 侧栏按钮的 UIA Name 就是导航文案，所以可以按名字 Invoke。
#
# 用法（pwsh 7；需要 release 产物，debug 构建不会内嵌界面）：
#   cargo build --release -p transactions
#   pwsh -File fixtures/ui-smoke.ps1 -Workspace target\tests\ui-smoke\out\ws
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

. (Join-Path $PSScriptRoot 'lib\TrUia.ps1')

$repo = Split-Path -Parent $PSScriptRoot
# ⚠ 必须在这里先记下"调用方是否显式给了 -Workspace"（下面马上会给它赋默认值，之后再判就恒为真）
$explicitWorkspace = -not [string]::IsNullOrWhiteSpace($Workspace)
if (-not $Exe) { $Exe = Join-Path $repo 'target\release\transactions.exe' }
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\tests\ui-smoke\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\tests\ui-smoke\out' }
if (-not $Workspace) { $Workspace = Join-Path $repo 'target\tests\ui-smoke\out\ws' }
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Force -Path $OutDir | Out-Null }
if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$smokeHome = [System.IO.Path]::GetFullPath($SmokeHome)
if ($smokeHome -eq [System.IO.Path]::GetFullPath($env:USERPROFILE)) {
    throw "拒绝把临时 HOME 指到真实用户目录 —— 冒烟会改写你的配置"
}
if (-not (Test-Path $smokeHome)) { New-Item -ItemType Directory -Force -Path $smokeHome | Out-Null }

$ws = [System.IO.Path]::GetFullPath($Workspace)

# ---- 播种：没显式给 -Workspace 就重新播种一份干净基线；显式给了只补齐缺失的库，不动调用方的数据 ----
if (-not $explicitWorkspace -and (Test-Path $ws)) { Remove-Item $ws -Recurse -Force }
if (-not (Test-Path (Join-Path $ws 'transactions.db'))) {
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[ui-smoke] 播种工作空间: $ws" -ForegroundColor Cyan
}

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
    '股票' = @('账户', '持仓', '记录', '统计', '设置')
    '事件' = @('新增事件', '上一年', '下一年')
    '日记' = @('今天', '全部展开', '全部收起', '心情')
    '应用设置' = @('工作空间', '外观', '关闭行为', '开发者工具')
}

# 记账页与股票页的**子功能**（左侧图标条切换，不是侧栏条目）：子功能名 → 标记。
# 「记账」这一项看的是默认子功能「记录」。原「数据分析」顶级页已并入这里并更名「分析」；
# 股票页的子功能原为顶部页签（账户/持仓/成交记录/交易统计），现改为图标条并多了「设置」。
$subMarkers = [ordered]@{
    '记录' = @('记一笔', '排序', '筛选', '每页条数')
    # 「曲线合计」是**随数据变化**的：图表区间没有记录时整列隐藏（画布走空态）。
    # 换一份"数据不在当前区间"的工作空间就会假红 —— 这里改用同面板里常驻的「曲线配置」。
    '分析' = @('新增图表', '曲线配置', '月度消费趋势')
    '标签' = @('新增分类', '新增标签', '分类', '标签')
    '模板' = @('新建模板', '模板名称', '交易类型')
}

# 子功能 → 它所属的顶级页（点子功能前要先回这一页）。
# 图标条按钮按"同名 Button 里最靠左的那个"定位（见 lib/TrUia.ps1 的 Invoke-SubFunction）。
$subParents = [ordered]@{
    '记录' = '记账'; '分析' = '记账'; '标签' = '记账'; '模板' = '记账'
    '账户' = '股票'; '持仓' = '股票'; '统计' = '股票'; '设置' = '股票'
}

# 股票子功能各自的页面标记（股票页里「记录」这个名字与记账的「记录」重名，
# 靠 $subParents 决定先回哪一页，所以这张表与 $subMarkers 分开维护）。
# ⚠ 标记要选**常驻**的：统计子功能没有结算记录时走空态（面板标题「结算统计」不渲染），
#   所以用工具栏里的筛选组（任何数据状态下都在；「刷新」按钮已移除）。
$stockSubMarkers = [ordered]@{
    '账户' = @('总资产', '追加本金')
    '持仓' = @('建仓')
    '记录' = @('已实现盈亏')
    '统计' = @('全部标签', '最近 N 笔')
    '设置' = @('交易费用设置', '交易标签')
}

# 注：`FeaturePage` 的 `toolbar` 是**可选插槽**，没人用的页面整段不渲染（不留空发丝线）。
# 股票页里只有「账户」和「统计」给了工具栏（标记表里的「追加本金」/「全部标签」就是它的控件）；
# 持仓的「建仓」在左侧持仓列表底栏、记录的汇总与轮次自成一栏、设置的「保存」在费用卡片里，
# 这三个子功能都没有工具栏。

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
    # 鼠标点击统一走 fixtures/lib/TrUia.ps1 的 TrUia（原来这里每次调用都重新 Add-Type 一个 TrMouse）
    $x = [int]($Rect.X + $Rect.Width / 2)
    $y = [int]($Rect.Y + $Rect.Height / 2)
    [TrUia]::Click($x, $y)
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

function Invoke-NavigationChecks {
    param($Session)
    $window = $Session.Window

    # 待检查的页面清单：先记账页（默认子功能「记录」），再两个页的子功能（走左侧图标条），
    # 最后其余顶级功能（走侧栏）。
    $checks = @([pscustomobject]@{ Name = '记账'; Run = '记录'; Parent = '记账'; Markers = $subMarkers['记录']; Sub = $false })
    foreach ($key in $subMarkers.Keys) {
        if ($key -eq '记录') { continue }   # 已经由「记账」这一条覆盖（默认子功能就是它）
        $checks += [pscustomobject]@{ Name = $key; Run = $key; Parent = '记账'; Markers = $subMarkers[$key]; Sub = $true }
    }
    foreach ($key in $markers.Keys) {
        $checks += [pscustomobject]@{ Name = $key; Run = $key; Parent = ''; Markers = $markers[$key]; Sub = $false }
    }
    # 股票页的子功能：名字与记账的重名（两边都有「记录」），用例名加前缀区分，
    # 断言与点击仍用 `Run` 里的原名。
    foreach ($key in $stockSubMarkers.Keys) {
        $checks += [pscustomobject]@{ Name = "股票·$key"; Run = $key; Parent = '股票'; Markers = $stockSubMarkers[$key]; Sub = $true }
    }

    foreach ($check in $checks) {
        $page = $check.Name
        Write-Host "`n[ui-smoke] 打开页面：$page" -ForegroundColor Cyan
        if ($check.Sub) {
            # 子功能：先回它的顶级页，再点左侧图标条上的那一项
            Invoke-ByName -Window $window -Name $check.Parent -TimeoutSec 10 | Out-Null
            Start-Sleep -Milliseconds 600
            $clicked = Invoke-SubFunction -Window $window -Name $check.Run
        } else {
            $clicked = Invoke-ByName -Window $window -Name $page
        }
        if (-not $clicked) {
            $failures.Add("找不到入口: $page")
            Write-Host "  ✗ 找不到入口" -ForegroundColor Red
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

        foreach ($marker in $check.Markers) {
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

    # 记一笔弹窗的确认键是「保存」（见 ui-transactions.ps1 的同款断言）
    if (-not (Invoke-ByName -Window $window -Name '保存' -TimeoutSec 10)) {
        $failures.Add("找不到「保存」按钮")
        return
    }
    Start-Sleep -Seconds 3

    $names = Get-NamedElements -Window $window
    Assert-True ($names -contains $WriteDescription) "列表里出现了新记录（$WriteDescription）"
    Assert-True ([bool]($names | Where-Object { $_ -match [regex]::Escape($WriteAmount) })) "列表里出现了金额 $WriteAmount"
    Assert-True (-not $Session.Process.HasExited) "保存后进程仍然存活"
}

# 账本菜单的回归：**必须用真实鼠标点**菜单项。
#
# 为什么单独立这一条：菜单弹层是 `.ledger-menu`（曾写成 z-index:900），而它自带的全屏点击捕获层
# 用的是 `.ui-select__backdrop`（z-index:1040）—— 捕获层盖在菜单上，于是"菜单弹出来了，
# 但「创建账本」点了没反应"（点击被捕获层吃掉，还顺手把菜单关掉）。
# 用 UIA 的 InvokePattern 点**测不出来**这个缺陷（它绕过命中测试），只有真实鼠标点击才暴露。
# 按 class 找**可见**的按钮（账本按钮/菜单项的可访问名是动态的：当前账本名 / 菜单文案，
# 只有 class 是稳定的；`FindAll` 的嵌套括号容易写坏，这里收敛成一个 helper）。
function Find-VisibleButtonByClass {
    param($Window, [string]$ClassPart, [int]$TimeoutSec = 12)
    $buttonType = New-Object System.Windows.Automation.PropertyCondition(
        $UIA::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $all = $Window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $buttonType)
        foreach ($el in @($all)) {
            if ($el.Current.ClassName -like "*$ClassPart*" -and -not $el.Current.IsOffscreen) { return $el }
        }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    return $null
}

function Invoke-LedgerMenuChecks {
    param($Session)
    $window = $Session.Window
    Write-Host "`n[ui-smoke] 账本菜单：真实鼠标点「创建账本」" -ForegroundColor Cyan

    $ledgerBtn = Find-VisibleButtonByClass -Window $window -ClassPart 'ledger-btn'
    Assert-True ([bool]$ledgerBtn) '找到账本切换按钮（class=ledger-btn）'
    if (-not $ledgerBtn) { return }

    $invoke = $null
    if ($ledgerBtn.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$invoke)) {
        $invoke.Invoke()
    } else {
        Set-CursorAndClick $ledgerBtn.Current.BoundingRectangle | Out-Null
    }
    Start-Sleep -Seconds 2

    $create = Find-VisibleButtonByClass -Window $window -ClassPart 'ledger-menu-create'
    Assert-True ([bool]$create) '账本下拉里出现菜单项「创建账本」'
    if (-not $create) { return }

    # ⚠ 必须是**真实鼠标点击**：UIA 的 InvokePattern 绕过命中测试，测不出"被捕获层盖住"
    Set-CursorAndClick $create.Current.BoundingRectangle | Out-Null
    Start-Sleep -Seconds 2

    $title = $null
    $nameInput = $null
    $deadline = (Get-Date).AddSeconds(10)
    do {
        $editType = New-Object System.Windows.Automation.PropertyCondition(
            $UIA::ControlTypeProperty, [System.Windows.Automation.ControlType]::Edit)
        $nameInput = $null
        foreach ($el in @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $editType))) {
            if ($el.Current.Name -eq '请输入账本名称') { $nameInput = $el; break }
        }
        if ($nameInput) { $title = $true }
        if (-not $title) { Start-Sleep -Milliseconds 500 }
    } while (-not $title -and (Get-Date) -lt $deadline)
    Assert-True ([bool]$title) '真实鼠标点击打开了「创建账本」弹窗（此处曾因浮层 z-index 失效）'

    # 弹窗居中：输入框中心应与窗口中心重合（弹窗固定在视口中央）
    if ($nameInput) {
        $winRect = $window.Current.BoundingRectangle
        $inputRect = $nameInput.Current.BoundingRectangle
        $offset = [Math]::Abs(($inputRect.X + $inputRect.Width / 2) - ($winRect.X + $winRect.Width / 2))
        Assert-True ($offset -le 30) ("弹窗水平居中（输入框中心偏差 {0:N0}px）" -f $offset)
    }

    # 收尾：点「取消」关掉弹窗，别把状态留给后面的用例
    $cancelType = New-Object System.Windows.Automation.PropertyCondition(
        $UIA::NameProperty, '取消')
    foreach ($el in @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cancelType))) {
        if (-not $el.Current.IsOffscreen) { Set-CursorAndClick $el.Current.BoundingRectangle | Out-Null; break }
    }
    Start-Sleep -Seconds 1
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
        Invoke-LedgerMenuChecks -Session $session
        Invoke-WriteFlow -Session $session
    }
    finally { Stop-AppSession $session }
}

if ($failures.Count -gt 0) {
    Write-Host "`n[ui-smoke] ❌ $($failures.Count) 项不通过：" -ForegroundColor Red
    $failures | ForEach-Object { "  - $_" }
    exit 1
}
Write-Host "`n[ui-smoke] ✅ 5 个顶级功能 + 9 个子功能（记账 4 / 股票 5）都能打开并渲染" -ForegroundColor Green
exit 0
