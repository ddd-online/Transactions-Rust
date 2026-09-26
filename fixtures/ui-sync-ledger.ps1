# ui-sync-ledger.ps1 —— 「同步到其他账本」端到端（界面 + 数据库）。
#
# 为什么需要它：这条路径**此前没有任何覆盖**——IPC 命令面里也没有 `sync`（界面是
# "复制一份 DTO、换上目标账本、清空 id，再调 `tr_create`"），所以既不在单测里，也没有别的端到端覆盖。
# 风险正好落在这类"复制但不完全复制"的地方：新 id 有没有生成、目标账本对不对、
# 价格/类型/分类有没有带过去、**源记录是否原样保留**。
#
# 用法（pwsh 7；需要 release 产物；本仓库不能有实例在跑）：
#   pwsh -File fixtures/ui-sync-ledger.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

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
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\tests\ui-sync-ledger\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\tests\ui-sync-ledger\out' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$trPaths = Initialize-TrSmokeHome -SmokeHome $SmokeHome -OutDir $OutDir
$smokeHome = $trPaths.SmokeHome
$OutDir = $trPaths.OutDir
Assert-NoRepoInstance -Repo $repo

$ws = [System.IO.Path]::GetFullPath($Workspace)

# 公共鼠标 P/Invoke 类（TrSync）已统一到 fixtures/lib/TrUia.ps1 的 TrUia（调用点是 [TrUia]::…）

$failures = New-Object System.Collections.Generic.List[string]

# 同步面板里的账本项（`.tr-sync-item`）。按**类名**找，不按名字：账本名在别处也会出现
# （账本切换器里就有当前账本的名字），而"面板关掉了没有"这件事只有面板项本身说了算。
# 逐个 try/catch：UIA 枚举期间元素可能已被重渲染摘掉，读 `Current` 会抛 ElementNotAvailable。
function Get-SyncPanelItems { param($Window)
    $items = New-Object System.Collections.Generic.List[object]
    foreach ($element in @(Get-Elements $Window)) {
        try { $class = $element.Current.ClassName } catch { continue }
        if ($class -like '*tr-sync-item*') { $items.Add($element) }
    }
    # ⚠ 不要写 `return , $items.ToArray()`：那会把空数组也包成一个元素，
    # 于是调用方的 `@(...).Count` **永远 ≥ 1**，"面板关掉了"这条断言会恒假红。
    return $items.ToArray()
}

# 轮询等"面板项全没了"（关掉是异步渲染，不能只看一次）。
function Wait-SyncPanelClosed { param($Window, [int]$TimeoutSec = 8)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        if (@(Get-SyncPanelItems -Window $Window).Count -eq 0) { return $true }
        Start-Sleep -Milliseconds 300
    } while ((Get-Date) -lt $deadline)
    return (@(Get-SyncPanelItems -Window $Window).Count -eq 0)
}

# ---- 播种（默认每次重播；种子里有两个账本，同步才有目标）----
if (-not $explicitWorkspace) {
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
}
if (-not (Test-Path (Join-Path $ws 'transactions.db'))) {
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[sync] 播种工作空间: $ws" -ForegroundColor Cyan
}

@{ width = 1500; height = 950; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'ui-sync-ledger.ps1' } |
    ConvertTo-Json | Set-Content -Path (Join-Path $smokeHome '.transactions.json') -Encoding UTF8

$stamp = Get-Date -Format 'HHmmss'
$description = "UIA同步源$stamp"

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
    Write-Host "[sync] UIA 可读元素 $((Get-Elements $window).Count) 个" -ForegroundColor Cyan

    $ledgers = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_ledger' -OutDir $OutDir)
    Assert-True ($ledgers.Count -ge 2) "种子里至少有 2 个账本（实际 $($ledgers.Count)）"

    # ================= 1/3 在当前账本记一笔 =================
    Write-Host "`n[sync] 1/3 在当前账本记一笔 66.66（$description）"
    Assert-True (Add-Record -Window $window -Description $description -Amount '66.66') '记一笔'

    $records = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_transaction_record' -OutDir $OutDir)
    $sourceRows = @($records | Where-Object { $_.description -eq $description })
    Assert-True ($sourceRows.Count -eq 1) "库里出现源记录（$description）"
    if ($sourceRows.Count -lt 1) { throw '没有源记录，无法继续' }
    $source = $sourceRows[0]
    $currentLedgerId = $source.ledger_id
    $currentLedger = @($ledgers | Where-Object { $_.id -eq $currentLedgerId })
    Assert-True ($currentLedger.Count -eq 1) "当前账本 = $($currentLedger[0].name)"
    $targetCandidates = @($ledgers | Where-Object { $_.id -ne $currentLedgerId })
    Assert-True ($targetCandidates.Count -ge 1) '存在"其他账本"可作为同步目标'
    $target = $targetCandidates[0]
    Write-Host "    目标账本：$($target.name)"

    # ================= 2/3 界面点「同步到其他账本」→ 选目标账本 =================
    Write-Host "`n[sync] 2/3 点该行的「同步到其他账本」并选中目标账本"
    $syncButton = Find-RowButton -Window $window -RowText $description -ButtonName '同步到其他账本'
    Assert-True ([bool]$syncButton) '找到该行的「同步到其他账本」按钮'
    if (-not $syncButton) { throw '找不到同步按钮' }
    Click-Element $syncButton | Out-Null
    Start-Sleep -Seconds 2

    # 面板里列出的是"其他账本"的名字
    $targetButton = Wait-Element -Root $window -Name $target.name -TimeoutSec 10
    Assert-True ([bool]$targetButton) "同步面板里出现目标账本「$($target.name)」"

    # ---- 回归 A：点面板以外的地方 → 面板自动收起 ----
    # 缺陷背景（用户报的）：开关挂在外层 `<span>` 上，面板内/外任何一次点击冒泡上来都会
    # 把面板**再打开一次**，于是只有"再点一次同步图标"才关得掉。这里先用"点别的地方"验。
    Assert-True (@(Get-SyncPanelItems -Window $window).Count -ge 1) '同步面板已打开（面板项可见）'
    $outsideCell = Wait-Like -Root $window -Pattern $description -TimeoutSec 10
    Assert-True ([bool]$outsideCell) '找到源记录所在行（用它当「面板以外的地方」）'
    if ($outsideCell) { Click-Element $outsideCell | Out-Null }
    Assert-True (Wait-SyncPanelClosed -Window $window) '点面板以外的地方 → 同步面板自动收起'

    # ---- 回归 B：点账本名 → 触发同步，且面板**当场**收起 ----
    $syncButton = Find-RowButton -Window $window -RowText $description -ButtonName '同步到其他账本'
    Assert-True ([bool]$syncButton) '重新打开同步面板（再点一次图标）'
    if ($syncButton) { Click-Element $syncButton | Out-Null }
    $targetButton = Wait-Element -Root $window -Name $target.name -TimeoutSec 10
    Assert-True ([bool]$targetButton) "面板里能再看到目标账本「$($target.name)」"
    if ($targetButton) { Invoke-Element $targetButton | Out-Null }
    Start-Sleep -Seconds 4
    Assert-True (Wait-SyncPanelClosed -Window $window) '点账本名后同步面板当场收起（不用再点一次图标）'

    # ================= 3/3 断言：目标账本多了一份副本、源记录原样保留 =================
    Write-Host "`n[sync] 3/3 校验落库结果"
    $after = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_transaction_record' -OutDir $OutDir)
    $synced = @($after | Where-Object { $_.description -eq $description -and $_.ledger_id -eq $target.id })
    Assert-True ($synced.Count -eq 1) "目标账本里出现副本（$($target.name)）"
    if ($synced.Count -eq 1) {
        $copy = $synced[0]
        Assert-True ($copy.transaction_id -ne $source.transaction_id) '副本是新的 transaction_id（不是同一行）'
        Assert-True ($copy.price -eq $source.price) "金额一致（$($copy.price) 分）"
        Assert-True ($copy.transaction_type -eq $source.transaction_type) "类型一致（$($copy.transaction_type)）"
        Assert-True ($copy.category -eq $source.category) "分类一致（$($copy.category)）"
        Assert-True ($copy.transaction_at -eq $source.transaction_at) "记录时间一致（$($copy.transaction_at)）"
    }
    $sourceStillThere = @($after | Where-Object { $_.transaction_id -eq $source.transaction_id })
    Assert-True ($sourceStillThere.Count -eq 1) '源记录仍然存在（同步是复制不是移动）'
    Assert-True (@($after | Where-Object { $_.description -eq $description }).Count -eq 2) "同描述共 2 行（源 + 副本）"

    # 切到目标账本，界面里应该能看到这笔
    Write-Host "`n[sync] 额外：切到目标账本后界面上能看到副本"
    $ledgerSwitcher = Wait-Like -Root $window -Pattern $currentLedger[0].name -TimeoutSec 10
    Assert-True ([bool]$ledgerSwitcher) "找到账本选择器（当前显示 $($currentLedger[0].name)）"
    if ($ledgerSwitcher) {
        Invoke-Element $ledgerSwitcher | Out-Null
        Start-Sleep -Seconds 1
        $option = Wait-Element -Root $window -Name $target.name -TimeoutSec 10
        Assert-True ([bool]$option) "下拉里能选到「$($target.name)」"
        if ($option) { Invoke-Element $option | Out-Null }
        Start-Sleep -Seconds 4
        $visible = $false
        $deadline = (Get-Date).AddSeconds(15)
        do {
            $texts = @(Get-Elements $window | ForEach-Object { $_.Current.Name } | Where-Object { $_ })
            $visible = @($texts | Where-Object { $_ -like "*$description*" }).Count -gt 0
            if (-not $visible) { Start-Sleep -Milliseconds 500 }
        } while (-not $visible -and (Get-Date) -lt $deadline)
        Assert-True $visible "切到目标账本后列表里能看到 $description"
    }
}
finally { Stop-TrApp -Process $process -Failures $failures -OutDir $OutDir }

Show-TrSummary -Failures $failures -Tag 'sync' -SuccessMessage "[sync] 全部通过：同步到其他账本 = 新 id 的副本 + 源记录保留 + 字段一致 + 界面可见"
