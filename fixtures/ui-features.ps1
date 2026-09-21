# ui-features.ps1 —— 「应用设置 → 功能开关」端到端：**关掉一个功能，侧边栏就少一项，并落盘**。
#
# 为什么需要它：功能开关是**跨页面的**（开关在设置页，效果在侧栏），而且它的两半分别在不同层：
#   * 界面：侧边栏读 `store::AppStores::enabled_features`（`shell.rs` 过滤 NAV_ITEMS）；
#   * 外壳：`config_set_feature` 把 `features: { accounting, stock, keyEvent, diary }` 写进
#     `~/.transactions.json`。
# 单测只能各测一半（`config.rs` 测序列化与默认值，侧栏过滤根本没有纯函数可测），
# 而"开关点了没反应""侧栏少了但不落盘""重启又全回来了"这三种坏法都只有真跑一遍才看得见。
#
# 断言三条：
#   1. 默认全开：首次启动侧栏四项都在（老配置里没有 features 键也必须全开）；
#   2. 关掉「日记」→ 侧栏当场少一项 + 配置里 `features.diary=false`；
#   3. 开回来 → 侧栏恢复 + 配置里 `features.diary=true`（顺带证明写的是布尔值本身，
#      不是"只写了 false 就再也开不回来"）。
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
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\features-smoke\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\features-smoke' }
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

# 配置里的功能开关（**磁盘判据**：只看界面等于什么都没验）
function Get-ConfigFeature { param([string]$Feature)
    if (-not (Test-Path $configFile)) { return $null }
    $json = Get-Content -Raw $configFile | ConvertFrom-Json
    if (-not $json.features) { return $null }
    return $json.features.$Feature
}

function Wait-ConfigFeature { param([string]$Feature, [bool]$Expected, [int]$TimeoutSec = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $value = Get-ConfigFeature -Feature $Feature
        if ($null -ne $value -and [bool]$value -eq $Expected) { return $true }
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

# ---- 播种 ----
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

# 故意**不写 features 键**：这正是老配置的形态，用来验证"默认全开"
@{ width = 1500; height = 950; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'ui-features.ps1' } |
    ConvertTo-Json | Set-Content -Path $configFile -Encoding UTF8

# ---- 启动 ----
$process = $null
try {
    $run = Start-TrWindow -Exe $Exe -SmokeHome $smokeHome
    $process = $run.Process
    $window = $run.Window

    Write-Host "`n[features] 1/4 默认全开：侧栏四项都在（配置里没有 features 键）"
    foreach ($name in @('记账', '股票', '事件', '日记')) {
        Assert-True ([bool](Wait-SidebarButton -Window $window -Name $name)) "侧栏默认显示「$name」"
    }
    # 老配置（没有 features 键）读进来就是全开；外壳在启动期保存窗口几何时会顺手把整份配置写回去，
    # 所以这里**不能**断言"文件里仍然没有 features 键"（那是保存时机问题，不是行为问题），
    # 要断言的是它没把任何功能关掉。
    foreach ($feature in @('accounting', 'stock', 'keyEvent', 'diary')) {
        $value = Get-ConfigFeature -Feature $feature
        Assert-True ($null -eq $value -or [bool]$value) "features.$feature 默认是开的（实际: $value）"
    }

    Write-Host "[features] 2/4 打开「应用设置 → 功能开关」"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '应用设置' -TimeoutSec 20)) '打开「应用设置」页'
    Start-Sleep -Seconds 3
    Assert-True (Select-Tab -Window $window -Name '功能开关') '页签「功能开关」可选中'

    $diaryHint = '按账本隔离的 Markdown 日记'
    $paneReady = $null
    $deadline = (Get-Date).AddSeconds(20)
    do {
        $paneReady = Find-First $window $diaryHint
        if (-not $paneReady) { Start-Sleep -Milliseconds 400 }
    } while (-not $paneReady -and (Get-Date) -lt $deadline)
    Assert-True ([bool]$paneReady) '功能开关面板已渲染（出现「日记」那行的说明）'

    Write-Host "[features] 3/4 关掉「日记」→ 侧栏少一项 + 落盘 false"
    $switch = Find-Switch -Window $window -RowHint $diaryHint
    Assert-True ([bool]$switch) '找到「日记」那一行的开关（role=switch）'
    if ($switch) {
        Assert-True ((Get-SwitchState -Switch $switch) -eq [System.Windows.Automation.ToggleState]::On) '开关初始为「开」'
        Assert-True (Invoke-Switch -Switch $switch) '拨动开关'
        Assert-True (Wait-SidebarGone -Window $window -Name '日记') '侧栏不再有「日记」'
        Assert-True (Wait-ConfigFeature -Feature 'diary' -Expected $false) '配置里 features.diary = false'
        # 其它功能不受影响（写的是"某一个开关"，不是整份覆盖）
        Assert-True ((Get-ConfigFeature -Feature 'accounting') -eq $true) '记账仍然是开的'
        Assert-True ([bool](Find-SidebarButton -Window $window -Name '记账')) '侧栏仍有「记账」'
        # 关掉的功能**不该**被"当前页兜底"选中：此刻停在设置页，侧栏也不该冒出「日记」
        Assert-True ((Get-SwitchState -Switch (Find-Switch -Window $window -RowHint $diaryHint)) -eq [System.Windows.Automation.ToggleState]::Off) '开关显示为「关」'
    }

    Write-Host "[features] 4/4 开回来 → 侧栏恢复 + 落盘 true"
    $switchBack = Find-Switch -Window $window -RowHint $diaryHint
    Assert-True ([bool]$switchBack) '再次找到「日记」开关'
    if ($switchBack) {
        Assert-True (Invoke-Switch -Switch $switchBack) '拨回开关'
        Assert-True ([bool](Wait-SidebarButton -Window $window -Name '日记')) '侧栏恢复「日记」'
        Assert-True (Wait-ConfigFeature -Feature 'diary' -Expected $true) '配置里 features.diary = true'
    }

    # 顺带看一眼重开是否记住（同一份配置、同一个工作空间）：关掉 → 重启 → 侧栏仍然没有
    Write-Host "[features] 附加：关掉「日记」后重启，侧栏仍然没有它（配置真的被读了）"
    $switchAgain = Find-Switch -Window $window -RowHint $diaryHint
    if ($switchAgain) {
        Assert-True (Invoke-Switch -Switch $switchAgain) '关掉「日记」准备重启'
        Assert-True (Wait-ConfigFeature -Feature 'diary' -Expected $false) '重启前 features.diary = false'
    }
    Stop-TrApp -Process $process -Failures $failures -OutDir $OutDir
    $process = $null
    # 单实例插件：等上一个进程真的退干净（`Stop-TrApp` 内已 WaitForExit，这里再留一点余量）
    Start-Sleep -Seconds 2

    $run2 = Start-TrWindow -Exe $Exe -SmokeHome $smokeHome
    $process = $run2.Process
    $window2 = $run2.Window
    if ($window2) {
        Assert-True (-not (Find-SidebarButton -Window $window2 -Name '日记')) '重启后侧栏仍然没有「日记」'
        Assert-True ([bool](Find-SidebarButton -Window $window2 -Name '记账')) '重启后「记账」还在'
    }
}
finally { Stop-TrApp -Process $process -Failures $failures -OutDir $OutDir }

Show-TrSummary -Failures $failures -Tag 'features' -SuccessMessage '[features] 全部通过：功能开关能隐藏/恢复侧栏条目，并落盘到用户配置'
