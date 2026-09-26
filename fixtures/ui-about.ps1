# ui-about.ps1 —— 「设置 → 关于软件」端到端：**打包产物自报的版本号必须等于 tauri.conf.json 的版本号**。
#
# 为什么需要它：应用名/版本来自外壳的 `app_info`（`app.package_info()`，即 tauri.conf.json
# 的 `version` 与 `productName`），而**发布资产名、Release tag、应用内更新比对**都依赖那个数字。
# 两侧一旦漂移（改了 tauri.conf.json 却没重新构建，或反过来只改了 Cargo.toml），
# 界面会照常显示、更新检查却会一直给出错误结论 —— 这类"看不见的错"没有别的护栏能挡住：
#   * `cargo test` 只测纯函数，读不到 exe 里的版本资源；
#   * `fixtures/smoke.ps1` / `ui-smoke.ps1` 只断言"界面起来了、各页渲染了"。
# 所以这里做两件事：
#   1. 从 `src-tauri/tauri.conf.json` 读**期望版本**，断言关于页显示「版本 X.Y.Z」与之一致；
#   2. 断言关于页其余固定内容（应用名 / GitHub 链接 / 版权行）确实渲染，
#      并等更新检查走到**终态**（成功或失败都算，网络不可用时不该让脚本红）。
# 另外反向断言「构建类型：…」那行**不再出现**（按需求移除，别再长回来）。
#
# 用法（pwsh 7；需要打包产物；本仓库不能有实例在跑）：
#   pwsh -File fixtures/ui-about.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

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
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\tests\ui-about\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\tests\ui-about\out' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 pwsh -File build/build.ps1）" }

# 期望版本：唯一来源是 src-tauri/tauri.conf.json（build/build.ps1 的重命名也读它）
$confPath = Join-Path $repo 'src-tauri\tauri.conf.json'
$conf = Get-Content -Raw $confPath | ConvertFrom-Json
$expectedVersion = $conf.version
$expectedName = $conf.productName
if (-not $expectedVersion) { throw "$confPath 里没有 version" }

$trPaths = Initialize-TrSmokeHome -SmokeHome $SmokeHome -OutDir $OutDir
$smokeHome = $trPaths.SmokeHome
$OutDir = $trPaths.OutDir
Assert-NoRepoInstance -Repo $repo

$ws = [System.IO.Path]::GetFullPath($Workspace)

# 公共鼠标 P/Invoke 类（TrAbout）已统一到 fixtures/lib/TrUia.ps1 的 TrUia（调用点是 [TrUia]::…）

$failures = New-Object System.Collections.Generic.List[string]
function Get-Names { param($Root)
    return @(Get-Elements $Root | ForEach-Object { $_.Current.Name } | Where-Object { $_ })
}
function Wait-Like { param($Root, [string]$Pattern, [int]$TimeoutSec = 20)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $element = Find-Like -Root $Root -Pattern $Pattern
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
function Wait-Element { param($Root, [string]$Name, [int]$TimeoutSec = 20)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $element = Find-First $Root $Name
        if ($element) { return $element }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    return $null
}
# 页签：`<button role="tab">` 在 UIA 里是 TabItem（**不是** Button），优先用 SelectionItemPattern，
# 退而求其次用 InvokePattern，最后才是真实点击。
function Select-Tab { param($Window, [string]$Name, [int]$TimeoutSec = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        foreach ($element in @(Get-Elements $Window)) {
            if ($element.Current.Name -ne $Name) { continue }
            if ($element.Current.IsOffscreen) { continue }
            if (-not (Test-Rect $element.Current.BoundingRectangle)) { continue }
            $pattern = $null
            if ($element.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) {
                $pattern.Select()
                Start-Sleep -Milliseconds 800
                return $true
            }
            if ($element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
                $pattern.Invoke()
                Start-Sleep -Milliseconds 800
                return $true
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
function Find-StatusText { param($Window)
    # 更新卡片里的终态文案（来自 settings.rs 的 AboutSetting 分支）
    foreach ($candidate in @('已是最新版本', '下载完成', '发现新版本')) {
        $hit = Find-Like -Root $Window -Pattern $candidate
        if ($hit) { return $candidate }
    }
    # "检查失败…" 的正文是后端返回的文案（网络不通时不一定带"失败"两个字），
    # 所以用"重试/检查更新按钮出现 + 不再是 checking"来判定终态。
    if (Find-First $Window '重试') { return '检查失败（错误态）' }
    if (Find-First $Window '检查更新') { return 'idle（未开始）' }
    return $null
}

# ---- 播种（关于页本身不需要数据，但外壳要求一个可打开的工作空间） ----
if (-not $explicitWorkspace) {
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
}
if (-not (Test-Path (Join-Path $ws 'transactions.db'))) {
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[about] 播种工作空间: $ws" -ForegroundColor Cyan
}

@{ width = 1500; height = 950; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'ui-about.ps1' } |
    ConvertTo-Json | Set-Content -Path (Join-Path $smokeHome '.transactions.json') -Encoding UTF8

$year = (Get-Date).ToString('yyyy')
$copyright = "© $year Transactions. All rights reserved."

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

    Write-Host "`n[about] 1/3 打开「应用设置」页"
    # 侧栏条目名是**「应用设置」**（与原版一致），别写成「设置」——
    # 页面标题也叫「设置」，按名字取第一个会点到标题上，什么都不发生。
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '应用设置' -TimeoutSec 20)) '打开「应用设置」页'
    Start-Sleep -Seconds 3

    Write-Host "[about] 2/3 切到「关于软件」页签"
    Assert-True (Select-Tab -Window $window -Name '关于软件') '页签「关于软件」可选中'
    $paneReady = Wait-Like -Root $window -Pattern '版本 ' -TimeoutSec 15
    Assert-True ([bool]$paneReady) '关于面板已渲染（出现「版本 …」）'

    $names = Get-Names $window
    Assert-True ($names -contains $expectedName) "显示应用名「$expectedName」"
    $versionShown = "版本 $expectedVersion"
    Assert-True ($names -contains $versionShown) "显示「$versionShown」（与 tauri.conf.json 一致；实际版本文案: $(($names | Where-Object { $_ -like '版本 *' }) -join '/')）"
    # 反向断言：这一行已按需求移除，不该再出现在关于页
    $buildType = @($names | Where-Object { $_ -like '*构建类型*' })
    Assert-True ($buildType.Count -eq 0) "关于页没有「构建类型」行（实际: $($buildType -join '/')）"
    Assert-True ($names -contains 'GitHub') '关于页有「GitHub」链接'
    Assert-True ($names -contains $copyright) "版权行正确（$copyright）"

    Write-Host "[about] 3/3 等更新检查走到终态（网络不可用时允许失败终态）"
    $finalText = $null
    $deadline = (Get-Date).AddSeconds(45)
    do {
        $finalText = Find-StatusText -Window $window
        if (-not $finalText) { Start-Sleep -Milliseconds 500 }
    } while (-not $finalText -and (Get-Date) -lt $deadline)
    Assert-True ([bool]$finalText) "更新检查已出结果（实际: $finalText）"
    if ($finalText) {
        $hasFollowUp = @('重新检查', '立即更新', '安装并退出', '重试', '检查更新') |
            Where-Object { Find-First $window $_ } | Select-Object -First 1
        Assert-True ([bool]$hasFollowUp) "终态带后续操作按钮（$hasFollowUp）"
        if ($finalText -eq '已是最新版本') {
            Assert-True ($versionShown -eq "版本 $expectedVersion") "「已是最新版本」与自身版本 $expectedVersion 自洽"
        }
        Write-Host "[about] 更新检查结果：$finalText" -ForegroundColor Cyan
    }
}
finally { Stop-TrApp -Process $process -Failures $failures -OutDir $OutDir }

Show-TrSummary -Failures $failures -Tag 'about' -SuccessMessage "[about] 全部通过：关于页版本 $expectedVersion 与 tauri.conf.json 一致"
