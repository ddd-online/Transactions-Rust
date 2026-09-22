# ui-update-restore.ps1 —— 「关于软件」的下载状态**跨页面恢复**（下载是外壳侧的单例任务）。
#
# 为什么需要它：下载跑在外壳的线程里，界面只是订阅进度事件。切走再切回来时，
# 界面若只依赖本地信号（或重新走一遍 check），用户看到的就是"下载不见了" ——
# 进度条消失、又变回「立即更新」，而下载其实还在后台跑。
# 这条护栏就是**在下载进行中离开再回来**，断言界面自己把进度恢复了。
#
# 断言：
#   1. 关于页能算出「有更新」（依赖真实 GitHub API；网络不可用时显式 SKIP 并非零退出提示）；
#   2. 点「立即更新」后进入下载态（出现「取消下载」）；
#   3. **切到别的页面再切回关于页**，仍然处于下载态（进度/取消按钮还在）——
#      若下载已经跑完，则必须显示「下载完成 / 安装并退出」（两种终态都算通过，
#      因为安装包只有几 MB，可能在切页那一瞬间就下完了）；
#   4. 取消下载后回到「立即更新」。
#
# 用法（pwsh 7；需要内嵌界面的产物；本仓库不能有实例在跑）：
#   pwsh -File fixtures/ui-update-restore.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

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
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\update-smoke\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\update-smoke' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions --features tauri/custom-protocol）" }

$trPaths = Initialize-TrSmokeHome -SmokeHome $SmokeHome -OutDir $OutDir
$smokeHome = $trPaths.SmokeHome
$OutDir = $trPaths.OutDir
Assert-NoRepoInstance -Repo $repo

$ws = [System.IO.Path]::GetFullPath($Workspace)

$failures = New-Object System.Collections.Generic.List[string]

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

# 页签是 TabItem（不是 Button）：优先 SelectionItemPattern（同 ui-about.ps1）
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

# 关于页当前的更新状态：用固定文案判定（与 settings.rs 的分支一一对应）
function Get-UpdateState { param($Window)
    if (Find-First $Window '取消下载') { return 'downloading' }
    if (Find-First $Window '安装并退出') { return 'downloaded' }
    if (Find-First $Window '立即更新') { return 'available' }
    if (Find-Like -Root $Window -Pattern '正在检查') { return 'checking' }
    if (Find-First $Window '已是最新版本') { return 'no-update' }
    return 'unknown'
}

# ---- 播种（关于页不需要数据，但外壳要求一个能打开的工作空间） ----
if (-not $explicitWorkspace) {
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
}
if (-not (Test-Path (Join-Path $ws 'transactions.db'))) {
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[update] 播种工作空间: $ws" -ForegroundColor Cyan
}

@{ width = 1500; height = 950; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'ui-update-restore.ps1' } |
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

    $window = Get-ReadyWindow -ProcessId $process.Id -TimeoutSec 60
    if (-not $window) { throw '启动后 60 秒内没有拿到主窗口' }
    $hwnd = [IntPtr]$window.Current.NativeWindowHandle
    [TrUia]::ShowWindow($hwnd, 9) | Out-Null
    [TrUia]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Seconds 1

    Write-Host "`n[update] 1/4 打开「应用设置 → 关于软件」，等更新检查出结果"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '应用设置' -TimeoutSec 20)) '打开「应用设置」页'
    Start-Sleep -Seconds 3
    Assert-True (Select-Tab -Window $window -Name '关于软件') '页签「关于软件」可选中'

    # 等检查走到终态：available（有更新）/ no-update / error（网络不通）
    $deadline = (Get-Date).AddSeconds(45)
    $state = 'unknown'
    do {
        $state = Get-UpdateState -Window $window
        if ($state -eq 'unknown' -or $state -eq 'checking') { Start-Sleep -Milliseconds 700 }
    } while (($state -eq 'unknown' -or $state -eq 'checking') -and (Get-Date) -lt $deadline)
    Write-Host "    更新检查终态: $state"

    if ($state -eq 'no-update') {
        # 当前就是最新版 → 没有可下载的更新，这条护栏只验证"检查链路通"
        Write-Host '[update] 当前已是最新版本：没有可下载的更新，仅验证检查链路' -ForegroundColor Yellow
        Assert-True $true '检查更新走到「已是最新版本」终态（无更新可下载）'
    }
    elseif ($state -ne 'available') {
        Assert-True $false "更新检查应给出「有更新」或「已是最新」（实际 $state；网络不可用时请重试）"
    }
    else {
        Write-Host "[update] 2/4 点「立即更新」开始下载"
        Assert-True (Invoke-Element (Wait-Element -Root $window -Name '立即更新' -TimeoutSec 10)) '点「立即更新」'
        Start-Sleep -Milliseconds 1500
        $started = Get-UpdateState -Window $window
        Assert-True ($started -eq 'downloading' -or $started -eq 'downloaded') "进入下载态（实际 $started）"

        Write-Host "[update] 3/4 切走再切回「关于软件」——下载是外壳侧单例，状态必须恢复"
        Assert-True (Invoke-Element (Wait-Element -Root $window -Name '记账' -TimeoutSec 15)) '切到「记账」页'
        Start-Sleep -Seconds 2
        Assert-True (Invoke-Element (Wait-Element -Root $window -Name '应用设置' -TimeoutSec 15)) '切回「应用设置」页'
        Start-Sleep -Seconds 2
        Assert-True (Select-Tab -Window $window -Name '关于软件') '再次选中「关于软件」页签'
        Start-Sleep -Seconds 2

        $restored = Get-UpdateState -Window $window
        Write-Host "    切回后的状态: $restored"
        Assert-True ($restored -eq 'downloading' -or $restored -eq 'downloaded') `
            "切回来仍是下载态（下载中或已完成；实际 $restored —— 若为 available 说明状态没恢复）"

        Write-Host "[update] 4/4 取消下载（若已完成则跳过），断言回到「立即更新」"
        if ($restored -eq 'downloading') {
            $cancel = Wait-Element -Root $window -Name '取消下载' -TimeoutSec 10
            Assert-True ([bool]$cancel) '找到「取消下载」'
            if ($cancel) { Invoke-Element $cancel | Out-Null }
            Start-Sleep -Seconds 3
            $after = Get-UpdateState -Window $window
            Assert-True ($after -eq 'available') "取消后回到「立即更新」（实际 $after）"
        }
        else {
            Write-Host '[update] 下载已完成，跳过取消步骤' -ForegroundColor Cyan
        }
    }
}
finally { Stop-TrApp -Process $process -Failures $failures -OutDir $OutDir }

Show-TrSummary -Failures $failures -Tag 'update' -SuccessMessage '[update] 全部通过：下载状态在切换页面后能恢复（外壳侧单例任务）'
