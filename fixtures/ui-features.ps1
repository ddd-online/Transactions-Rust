# ui-features.ps1 —— 「应用设置 → 功能开关」端到端：**按账本**的开关（侧栏当场少一项，并落到那个账本）。
#
# 为什么需要它：功能开关是**跨页面的**（开关在设置页，效果在侧栏），而且它的两半分别在不同层：
#   * 界面：侧边栏读 `store::AppStores::enabled_features`（`shell.rs` 过滤 NAV_ITEMS），
#     它由"默认那份 + 当前账本那份"算出来（`tr_domain::wire::effective_features`）；
#   * 外壳：`config_set_feature` 把开关写进 `~/.transactions.json` —— 默认那份是 `features`，
#     按账本那份是 `ledgerFeatures[<账本 id>]`。
# 单测只能各测一半（`config.rs` 测序列化与按账本写、`commands.rs` 测作用域，侧栏过滤没有纯函数可测），
# 而"开关点了没反应""侧栏少了但不落盘""切账本串台""重启又全回来了"这四种坏法
# 都只有真跑一遍才看得见。
#
# 断言（7 段，**账本 id 级**）：
#   1. 默认全开：首次启动侧栏五项都在（配置里既没有 features 也没有 ledgerFeatures 键）；
#   2. 面板渲染出作用域说明行（锚点「只对当前账本」）且写着当前账本名；
#   3. 关掉当前账本的「日记」→ 侧栏当场少一项 + `ledgerFeatures.<当前账本 id>.diary=false`，
#      另一个账本**不在表里**，默认那份 `features` 没有被写成 false；
#   4. 切到另一个账本 → 侧栏「日记」回来，设置页那一行是「开」；
#   5. 那个账本也关掉 → 表里两个键都是 false；切回来仍然没有；
#   6. 第一个账本开回来 → 一 true 一 false；切到另一个仍然关着；切回来就有了；
#   7. 两个都关掉后重启 → 侧栏仍然没有「日记」（不管重启落到哪个账本），「记账」还在。
#
# 侧栏与开关按钮的区分：开关是 `<button role="switch">`（UIA 里带 TogglePattern），
# 侧栏条目是带图标文字的普通按钮；两者可访问名都可能叫「日记」，所以判据不能只看名字。
#
# 用法（pwsh 7；需要打包产物；本仓库不能有实例在跑）：
#   pwsh -File fixtures/ui-features.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

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
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\tests\ui-features\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\tests\ui-features\out' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions --features tauri/custom-protocol）" }

$trPaths = Initialize-TrSmokeHome -SmokeHome $SmokeHome -OutDir $OutDir
$smokeHome = $trPaths.SmokeHome
$OutDir = $trPaths.OutDir
Assert-NoRepoInstance -Repo $repo

$ws = [System.IO.Path]::GetFullPath($Workspace)
$configFile = Join-Path $smokeHome '.transactions.json'

$failures = New-Object System.Collections.Generic.List[string]

# ---- 局部工具 ----

function Wait-Element { param($Root, [string]$Name, [int]$TimeoutSec = 20)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $element = Find-First $Root $Name
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
    if (Test-Rect $rect) {
        [TrUia]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
        return $true
    }
    return $false
}

# 页签是 TabItem（不是 Button）：优先 SelectionItemPattern（同 ui-about.ps1 / ui-proxy.ps1）
function Select-Tab { param($Window, [string]$Name, [int]$TimeoutSec = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        foreach ($element in @(Get-Elements $Window)) {
            if ($element.Current.Name -ne $Name) { continue }
            if ($element.Current.IsOffscreen) { continue }
            if (-not (Test-Rect $element.Current.BoundingRectangle)) { continue }
            $pattern = $null
            if ($element.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) {
                $pattern.Select(); Start-Sleep -Milliseconds 800; return $true
            }
            if ($element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
                $pattern.Invoke(); Start-Sleep -Milliseconds 800; return $true
            }
            $rect = $element.Current.BoundingRectangle
            [TrUia]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
            Start-Sleep -Milliseconds 800
            return $true
        }
        Start-Sleep -Milliseconds 300
    } while ((Get-Date) -lt $deadline)
    return $false
}

# 开关按钮：`<button role="switch">`（`ui-switch`）。**不按名字找** ——
# 侧栏条目、页面标题与开关都可能叫「日记」；判据是"有 TogglePattern"。
# `$RowHint` 是同一张卡片里的说明文字，用来在多个开关里定位目标那一行。
function Find-Switch { param($Window, [string]$RowHint)
    $candidates = @()
    foreach ($element in @(Get-Elements $Window)) {
        if ($element.Current.IsOffscreen) { continue }
        $rect = $element.Current.BoundingRectangle
        if (-not (Test-Rect $rect)) { continue }
        $pattern = $null
        if (-not $element.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$pattern)) { continue }
        $candidates += $element
    }
    if ($candidates.Count -eq 0) { return $null }
    if (-not $RowHint) { return $candidates[0] }

    # 该行说明文字的 Y 中心；取与它纵向距离最近的那个开关
    $hint = Find-First $Window $RowHint
    if (-not $hint) { return $null }
    $hintRect = $hint.Current.BoundingRectangle
    $hintY = $hintRect.Y + $hintRect.Height / 2
    return $candidates | Sort-Object {
        $r = $_.Current.BoundingRectangle
        [math]::Abs(($r.Y + $r.Height / 2) - $hintY)
    } | Select-Object -First 1
}

# 开关当前状态（TogglePattern 的 ToggleState：1 = On，0 = Off）
function Get-SwitchState { param($Switch)
    if (-not $Switch) { return $null }
    $pattern = $null
    if (-not $Switch.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$pattern)) {
        return $null
    }
    return $pattern.Current.ToggleState
}

# 拨动开关：优先 TogglePattern，失败退回真实点击（与其它脚本同一策略）
function Invoke-Switch { param($Switch)
    if (-not $Switch) { return $false }
    $pattern = $null
    if ($Switch.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$pattern)) {
        $pattern.Toggle()
        return $true
    }
    $rect = $Switch.Current.BoundingRectangle
    if (-not (Test-Rect $rect)) { return $false }
    [TrUia]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
    return $true
}

# 侧栏条目：**名字相符的可见按钮**，且排除开关本身（开关有 TogglePattern）。
# 侧栏条目是图标 + 文字的 `<button class="nav-btn">`，可访问名 = `aria-label` = 功能名。
function Find-SidebarButton { param($Window, [string]$Name)
    foreach ($element in @(Get-Elements $Window)) {
        if ($element.Current.Name -ne $Name) { continue }
        if ($element.Current.IsOffscreen) { continue }
        if (-not (Test-Rect $element.Current.BoundingRectangle)) { continue }
        $toggle = $null
        if ($element.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$toggle)) { continue }
        if ($element.Current.ControlType -ne [System.Windows.Automation.ControlType]::Button) { continue }
        return $element
    }
    return $null
}

function Wait-SidebarButton { param($Window, [string]$Name, [int]$TimeoutSec = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $element = Find-SidebarButton -Window $Window -Name $Name
        if ($element) { return $element }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    return $null
}

function Wait-SidebarGone { param($Window, [string]$Name, [int]$TimeoutSec = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        if (-not (Find-SidebarButton -Window $Window -Name $Name)) { return $true }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    return $false
}

# ---- 配置判据（**磁盘**：只看界面等于什么都没验）----
#
# 两份开关：`features`（默认）与 `ledgerFeatures`（按账本 id）。缺键一律当"没有这一份"。
# 账本 id 是 uuid（带连字符），所以读属性走 `PSObject.Properties[...]`，不用 `$obj.$id`。

function Get-ConfigJson {
    if (-not (Test-Path $configFile)) { return $null }
    return (Get-Content -Raw $configFile | ConvertFrom-Json)
}

function Get-LedgerFeaturesNode {
    $json = Get-ConfigJson
    if (-not $json) { return $null }
    $property = $json.PSObject.Properties['ledgerFeatures']
    if (-not $property) { return $null }
    return $property.Value
}

# 表里现在挂着哪几个账本（键名，排序后用 `|` 连成一个**字符串**）。
#
# ⚠ 刻意返回字符串而不是数组：PowerShell 输出空数组时会被展开成"什么都没有"，
# 调用方再 `@(...)` 包一层就得到"含一个空数组的数组"（`.Count` = 1）——
# 于是"表还是空的"这条断言会莫名其妙地红（实测踩过）。连成一个字符串只有一种比较方式。
function Get-LedgerFeatureKeys {
    $node = Get-LedgerFeaturesNode
    if (-not $node) { return '' }
    return (@($node.PSObject.Properties.Name) | Sort-Object) -join '|'
}

# 某个账本那一份开关；没这个键返回 `$null`（= 它用默认）。
function Get-LedgerFeatureEntry { param([string]$LedgerId)
    $node = Get-LedgerFeaturesNode
    if (-not $node) { return $null }
    $property = $node.PSObject.Properties[$LedgerId]
    if (-not $property) { return $null }
    return $property.Value
}

function Get-LedgerFeature { param([string]$LedgerId, [string]$Feature)
    $entry = Get-LedgerFeatureEntry -LedgerId $LedgerId
    if (-not $entry) { return $null }
    $property = $entry.PSObject.Properties[$Feature]
    if (-not $property) { return $null }
    return $property.Value
}

# 默认那份（`features` 键）里的某一项。
function Get-DefaultFeature { param([string]$Feature)
    $json = Get-ConfigJson
    if (-not $json -or -not $json.features) { return $null }
    $property = $json.features.PSObject.Properties[$Feature]
    if (-not $property) { return $null }
    return $property.Value
}

function Wait-LedgerFeature { param([string]$LedgerId, [string]$Feature, [bool]$Expected, [int]$TimeoutSec = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $value = Get-LedgerFeature -LedgerId $LedgerId -Feature $Feature
        if ($null -ne $value -and [bool]$value -eq $Expected) { return $true }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    return $false
}

# 表里的键**恰好**是这几个账本（顺序无关）。
function Wait-LedgerFeatureKeys { param([string[]]$Expected, [int]$TimeoutSec = 15)
    $want = (@($Expected) | Sort-Object) -join '|'
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        if ((Get-LedgerFeatureKeys) -eq $want) { return $true }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    return $false
}

# 用一次性配置目录启动（`USERPROFILE` 指向 smokeHome，碰不到真实的 ~/.transactions.json）→ 主窗口
function Start-TrWindow { param([string]$Exe, [string]$SmokeHome)
    $saved = @{ USERPROFILE = $env:USERPROFILE; HOME = $env:HOME }
    try {
        $env:USERPROFILE = $SmokeHome
        $env:HOME = $SmokeHome
        $started = Start-Process -FilePath $Exe -PassThru
    }
    finally {
        $env:USERPROFILE = $saved.USERPROFILE
        $env:HOME = $saved.HOME
    }
    $ready = Get-ReadyWindow -ProcessId $started.Id -TimeoutSec 60
    if (-not $ready) {
        Stop-TrApp -Process $started -Failures $failures -OutDir $OutDir
        throw '启动后 60 秒内没有拿到主窗口'
    }
    $handle = [IntPtr]$ready.Current.NativeWindowHandle
    [TrUia]::ShowWindow($handle, 9) | Out-Null
    [TrUia]::SetForegroundWindow($handle) | Out-Null
    Start-Sleep -Seconds 1
    return [pscustomobject]@{ Process = $started; Window = $ready }
}

# ---- 播种（种子里有两个账本，切账本才有意义）----
if (-not $explicitWorkspace) {
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
}
if (-not (Test-Path (Join-Path $ws 'transactions.db'))) {
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[features] 播种工作空间: $ws" -ForegroundColor Cyan
}

# 故意**两个键都不写**：这正是老配置的形态（默认全开 + 没有任何账本单独配过），
# 用来验证"升级后行为不变"。
@{ width = 1500; height = 950; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'ui-features.ps1' } |
    ConvertTo-Json | Set-Content -Path $configFile -Encoding UTF8

# 按账本断言要拿真实账本 id：走 `cargo xtask dump`（不依赖 sqlite3 CLI）。
$ledgers = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_ledger' -OutDir $OutDir)
Assert-True ($ledgers.Count -ge 2) "种子里至少 2 个账本（实际 $($ledgers.Count)）"
if ($ledgers.Count -lt 2) { throw '账本不足 2 个，后续的"按账本"断言无法进行' }

# ---- 启动 ----
$process = $null
try {
    $run = Start-TrWindow -Exe $Exe -SmokeHome $smokeHome
    $process = $run.Process
    $window = $run.Window

    # 当前账本 = 账本选择器上显示的那个名字（另一个就是"别的账本"）。
    # 必须在打开设置页**之前**认：功能开关的说明行里也写着当前账本名，那之后按名字找会先命中它。
    $current = $null
    foreach ($ledger in $ledgers) {
        if (Wait-Like -Root $window -Pattern $ledger.name -TimeoutSec 3) { $current = $ledger; break }
    }
    Assert-True ([bool]$current) '找到当前账本（账本选择器上显示的名字）'
    if (-not $current) { throw '找不到当前账本，后续无法继续' }
    $target = @($ledgers | Where-Object { $_.id -ne $current.id })[0]
    Write-Host "[features] 当前账本「$($current.name)」($($current.id))；另一个「$($target.name)」($($target.id))" -ForegroundColor Cyan

    # ================= 1/7 默认全开 =================
    Write-Host "`n[features] 1/7 默认全开：侧栏五项都在，配置里没有任何账本单独配过"
    foreach ($name in @('记账', '股票', '待办', '事件', '日记')) {
        Assert-True ([bool](Wait-SidebarButton -Window $window -Name $name)) "侧栏默认显示「$name」"
    }
    # 老配置（两个键都没有）读进来就是全开；外壳在启动期保存窗口几何时会顺手把整份配置写回去，
    # 所以这里**不能**断言"文件里仍然没有这两个键"（那是保存时机问题，不是行为问题），
    # 要断言的是它没把任何功能关掉、也没替任何账本建条目。
    foreach ($feature in @('accounting', 'stock', 'keyEvent', 'diary', 'todo')) {
        $value = Get-DefaultFeature -Feature $feature
        Assert-True ($null -eq $value -or [bool]$value) "features.$feature 默认是开的（实际: $value）"
    }
    Assert-True ((Get-LedgerFeatureKeys) -eq '') '没有任何账本单独配过（ledgerFeatures 为空）'

    # ================= 2/7 面板与作用域说明 =================
    Write-Host "[features] 2/7 打开「应用设置 → 功能开关」：出现作用域说明行"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '应用设置' -TimeoutSec 20)) '打开「应用设置」页'
    Start-Sleep -Seconds 3
    Assert-True (Select-Tab -Window $window -Name '功能开关') '页签「功能开关」可选中'

    # 行说明是与 `settings.rs` 的 `FeatureRow` 文案**逐字对齐**的定位锚点：文案一改这条就得跟着改，
    # 否则 `Find-First`（按可访问名精确匹配）永远找不到 → 后面整段连坐变红。
    # 它上一次失效是 `5a855b8`（日记从 Markdown 改成纯文本）—— 当时漏改了这里。
    $diaryHint = '按账本隔离的纯文本日记，可导入导出 txt'
    $paneReady = Wait-Like -Root $window -Pattern $diaryHint -TimeoutSec 20
    Assert-True ([bool]$paneReady) '功能开关面板已渲染（出现「日记」那行的说明）'
    # 作用域说明行：锚点「只对当前账本」定在 `settings.rs` 的 `scope_hint` 里（那边也有注释互指）
    $scopeHint = Wait-Like -Root $window -Pattern '只对当前账本' -TimeoutSec 10
    Assert-True ([bool]$scopeHint) '出现作用域说明行（锚点：只对当前账本）'
    if ($scopeHint) {
        Assert-True ($scopeHint.Current.Name.Contains($current.name)) `
            "说明行里写着当前账本名「$($current.name)」（实际: $($scopeHint.Current.Name)）"
    }

    # ================= 3/7 关掉当前账本的「日记」 =================
    Write-Host "[features] 3/7 关掉当前账本「$($current.name)」的「日记」：侧栏少一项，且只落到这个账本"
    $switch = Find-Switch -Window $window -RowHint $diaryHint
    Assert-True ([bool]$switch) '找到「日记」那一行的开关（role=switch）'
    if ($switch) {
        Assert-True ((Get-SwitchState -Switch $switch) -eq [System.Windows.Automation.ToggleState]::On) '开关初始为「开」'
        Assert-True (Invoke-Switch -Switch $switch) '拨动开关'
        Assert-True (Wait-SidebarGone -Window $window -Name '日记') '侧栏不再有「日记」'
        Assert-True (Wait-LedgerFeature -LedgerId $current.id -Feature 'diary' -Expected $false) `
            "配置里 ledgerFeatures.<当前账本>.diary = false（$($current.id)）"
        Assert-True (Wait-LedgerFeatureKeys -Expected @($current.id)) `
            "表里**只有**当前账本这一个键（实际: $((Get-LedgerFeatureKeys) -replace '\|', ', ')）"
        Assert-True ($null -eq (Get-LedgerFeature -LedgerId $target.id -Feature 'diary')) `
            "另一个账本「$($target.name)」不在表里（没被写）"
        # 默认那份不能被账本的改动碰上：写的是"某个账本"，不是整份覆盖
        $defaultDiary = Get-DefaultFeature -Feature 'diary'
        Assert-True ($null -eq $defaultDiary -or [bool]$defaultDiary) `
            "默认那份 features.diary 没有被写成 false（实际: $defaultDiary）"
        # 其它功能不受影响（写的是"某一个开关"，不是整份覆盖）
        Assert-True ((Get-LedgerFeature -LedgerId $current.id -Feature 'accounting') -eq $true) '这个账本的「记账」仍然是开的'
        Assert-True ([bool](Find-SidebarButton -Window $window -Name '记账')) '侧栏仍有「记账」'
        Assert-True ((Get-SwitchState -Switch (Find-Switch -Window $window -RowHint $diaryHint)) -eq [System.Windows.Automation.ToggleState]::Off) '开关显示为「关」'
    }

    # ================= 4/7 切到另一个账本：它不受影响 =================
    Write-Host "[features] 4/7 切到「$($target.name)」：日记还在，开关是「开」"
    Assert-True (Switch-TrLedger -Window $window -Current $current.name -Target $target.name) "已切到「$($target.name)」"
    Assert-True ([bool](Wait-SidebarButton -Window $window -Name '日记')) '侧栏「日记」回来了（另一个账本没被上一步影响）'
    $switchB = $null
    $stateOn = $false
    $deadline = (Get-Date).AddSeconds(15)
    do {
        $switchB = Find-Switch -Window $window -RowHint $diaryHint
        if ((Get-SwitchState -Switch $switchB) -eq [System.Windows.Automation.ToggleState]::On) { $stateOn = $true; break }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    Assert-True $stateOn '切换账本后，设置页那一行开关跟着变回「开」（读的是这个账本那一份）'

    # ================= 5/7 这个账本也关掉 =================
    Write-Host "[features] 5/7 「$($target.name)」也关掉：两个账本各记各的"
    Assert-True (Invoke-Switch -Switch $switchB) '拨动开关'
    Assert-True (Wait-SidebarGone -Window $window -Name '日记') '侧栏不再有「日记」'
    Assert-True (Wait-LedgerFeature -LedgerId $target.id -Feature 'diary' -Expected $false) `
        "配置里 ledgerFeatures.<另一个账本>.diary = false（$($target.id)）"
    Assert-True (Wait-LedgerFeatureKeys -Expected @($current.id, $target.id)) `
        "表里现在有两个账本的键（实际: $((Get-LedgerFeatureKeys) -replace '\|', ', ')）"
    Assert-True ((Get-LedgerFeature -LedgerId $current.id -Feature 'diary') -eq $false) '第一个账本那一份没被改写（仍是 false）'
    $defaultDiary2 = Get-DefaultFeature -Feature 'diary'
    Assert-True ($null -eq $defaultDiary2 -or [bool]$defaultDiary2) '默认那份始终没被写成 false'
    Assert-True (Switch-TrLedger -Window $window -Current $target.name -Target $current.name) "切回「$($current.name)」"
    Assert-True (Wait-SidebarGone -Window $window -Name '日记') '切回第一个账本：侧栏仍然没有「日记」'

    # ================= 6/7 第一个账本开回来 =================
    Write-Host "[features] 6/7 在「$($current.name)」开回来：一个 true 一个 false"
    $switchA = $null
    $stateOff = $false
    $deadline = (Get-Date).AddSeconds(15)
    do {
        $switchA = Find-Switch -Window $window -RowHint $diaryHint
        if ((Get-SwitchState -Switch $switchA) -eq [System.Windows.Automation.ToggleState]::Off) { $stateOff = $true; break }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    Assert-True $stateOff '切回来后那一行显示为「关」（读的是这个账本那一份）'
    Assert-True (Invoke-Switch -Switch $switchA) '拨回开关'
    Assert-True ([bool](Wait-SidebarButton -Window $window -Name '日记')) '侧栏恢复「日记」'
    Assert-True (Wait-LedgerFeature -LedgerId $current.id -Feature 'diary' -Expected $true) `
        "ledgerFeatures.<第一个账本>.diary = true（$($current.id)）"
    Assert-True ((Get-LedgerFeature -LedgerId $target.id -Feature 'diary') -eq $false) `
        "另一个账本那一份仍是 false（没被这次拨动碰到）"
    Assert-True (Switch-TrLedger -Window $window -Current $current.name -Target $target.name) "切到「$($target.name)」"
    Assert-True (Wait-SidebarGone -Window $window -Name '日记') '它仍然是关的（那个 false 是它的）'

    # ================= 7/7 两个都关掉后重启 =================
    Write-Host "[features] 7/7 两个账本都关掉后重启：侧栏仍然没有「日记」"
    # 此刻在「$($target.name)」：先把它关掉（上一步刚切过来，它已经是关的），再切回去关掉另一个。
    # 两个都关掉之后，"重启落到哪个账本"就与断言无关（当前账本取 createdAt 最小的那个）。
    $switchT = Find-Switch -Window $window -RowHint $diaryHint
    if ((Get-SwitchState -Switch $switchT) -eq [System.Windows.Automation.ToggleState]::On) {
        Assert-True (Invoke-Switch -Switch $switchT) "关掉「$($target.name)」的日记"
    }
    Assert-True (Wait-LedgerFeature -LedgerId $target.id -Feature 'diary' -Expected $false) '它是 false'
    Assert-True (Switch-TrLedger -Window $window -Current $target.name -Target $current.name) "切回「$($current.name)」准备重启"
    $switchC = Find-Switch -Window $window -RowHint $diaryHint
    if ((Get-SwitchState -Switch $switchC) -eq [System.Windows.Automation.ToggleState]::On) {
        Assert-True (Invoke-Switch -Switch $switchC) '关掉当前账本的日记'
    }
    Assert-True (Wait-LedgerFeature -LedgerId $current.id -Feature 'diary' -Expected $false) '两个账本都关掉了'

    Stop-TrApp -Process $process -Failures $failures -OutDir $OutDir
    $process = $null
    # 单实例插件：等上一个进程真的退干净（`Stop-TrApp` 内已 WaitForExit，这里再留一点余量）
    Start-Sleep -Seconds 2

    $run2 = Start-TrWindow -Exe $Exe -SmokeHome $smokeHome
    $process = $run2.Process
    $window2 = $run2.Window
    if ($window2) {
        Assert-True (Wait-SidebarGone -Window $window2 -Name '日记') '重启后侧栏仍然没有「日记」（配置真的被读了）'
        Assert-True ([bool](Find-SidebarButton -Window $window2 -Name '记账')) '重启后「记账」还在'
    }
}
finally { Stop-TrApp -Process $process -Failures $failures -OutDir $OutDir }

Show-TrSummary -Failures $failures -Tag 'features' -SuccessMessage '[features] 全部通过：功能开关按账本独立（写进 ledgerFeatures.<账本 id>、切账本互不影响、重启仍在）'
