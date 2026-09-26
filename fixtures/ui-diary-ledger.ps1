# ui-diary-ledger.ps1 —— 日记**按账本隔离**端到端（界面 + 数据库）。
#
# 为什么需要它：日记原来是"工作空间级"（一天一篇，全局唯一），改成按账本隔离之后，
# 唯一键变成 `(ledger_id, date)`，于是有两条必须锁住的回归：
#   1. 账本 A 写的日记，切到账本 B 后**看不见**（编辑器里不是 A 的正文），切回 A 正文原样还在；
#   2. 同一天在 A、B 各存一篇**互不覆盖**（复合唯一索引；写成全局唯一就会撞 UPSERT）。
#
# 用法（pwsh 7；需要 release 产物；本仓库不能有实例在跑）：
#   pwsh -File fixtures/ui-diary-ledger.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

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
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\tests\ui-diary-ledger\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\tests\ui-diary-ledger\out' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$trPaths = Initialize-TrSmokeHome -SmokeHome $SmokeHome -OutDir $OutDir
$smokeHome = $trPaths.SmokeHome
$OutDir = $trPaths.OutDir
Assert-NoRepoInstance -Repo $repo

$ws = [System.IO.Path]::GetFullPath($Workspace)
$failures = New-Object System.Collections.Generic.List[string]

# ---- 播种（默认每次重播；种子里有两个账本，切账本才有意义）----
if (-not $explicitWorkspace) {
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
}
if (-not (Test-Path (Join-Path $ws 'transactions.db'))) {
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[diary-ledger] 播种工作空间: $ws" -ForegroundColor Cyan
}

@{ width = 1500; height = 950; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'ui-diary-ledger.ps1' } |
    ConvertTo-Json | Set-Content -Path (Join-Path $smokeHome '.transactions.json') -Encoding UTF8

$today = (Get-Date).ToString('yyyy-MM-dd')
$stamp = Get-Date -Format 'HHmmss'
$contentA = "# 账本A的日记$stamp`n第一行"
$contentB = "# 账本B的日记$stamp`n第二行"

$ledgers = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_ledger' -OutDir $OutDir)
Assert-True ($ledgers.Count -ge 2) "种子里至少 2 个账本（实际 $($ledgers.Count)）"

function Read-DiaryRows {
    Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_diary_entry' -OutDir $OutDir |
        Where-Object { $_.date -eq $today }
}

# 切到指定账本：账本选择器的可访问名是**当前**账本名，所以先按 `Current` 找到它、点开，
# 再在浮层里选 `Target`。
function Switch-Ledger { param($Window, [string]$Current, [string]$Target)
    $switcher = Wait-Like -Root $Window -Pattern $Current -TimeoutSec 10
    Assert-True ([bool]$switcher) "找到账本选择器（当前显示「$Current」）"
    if (-not $switcher) { return $false }
    Invoke-Element $switcher | Out-Null
    Start-Sleep -Seconds 1
    $option = $null
    $deadline = (Get-Date).AddSeconds(10)
    do {
        $option = Find-Like -Root $Window -Pattern $Target
        if (-not $option) { Start-Sleep -Milliseconds 400 }
    } while (-not $option -and (Get-Date) -lt $deadline)
    Assert-True ([bool]$option) "浮层里能选到「$Target」"
    if (-not $option) { return $false }
    Invoke-Element $option | Out-Null
    Start-Sleep -Seconds 4
    return $true
}

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
    Start-Sleep -Seconds 2

    # 当前账本 = 账本选择器上显示的那个名字（另一个就是"别的账本"）
    $current = $null
    foreach ($ledger in $ledgers) {
        if (Wait-Like -Root $window -Pattern $ledger.name -TimeoutSec 3) { $current = $ledger; break }
    }
    Assert-True ([bool]$current) '找到当前账本（账本选择器上显示的名字）'
    if (-not $current) { throw '找不到当前账本，后续无法继续' }
    $target = @($ledgers | Where-Object { $_.id -ne $current.id })[0]
    Write-Host "[diary-ledger] 当前账本「$($current.name)」→ 另一个「$($target.name)」" -ForegroundColor Cyan

    # ================= 1/4 在当前账本写今天的日记 =================
    Write-Host "`n[diary-ledger] 1/4 当前账本写今天的日记"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '日记')) '打开「日记」'
    Start-Sleep -Seconds 3
    Assert-True ([bool](Get-DiaryTextarea -Window $window)) '日记页直接就是编辑态'
    $rows = @()
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        Set-DiaryContent -Window $window -Text $contentA | Out-Null
        Save-Now -Window $window
        $rows = @(Read-DiaryRows | Where-Object { $_.content -eq $contentA })
        if ($rows.Count -eq 1) { break }
        Write-Host "    第 $attempt 次写入没生效（库里 $($rows.Count) 条），重试" -ForegroundColor DarkYellow
    }
    Assert-True ($rows.Count -eq 1) "当前账本里出现今天的这条日记（实际 $($rows.Count)）"
    Assert-True ($rows[0].ledger_id -eq $current.id) `
        "这篇属于当前账本（期望 $($current.id)，实际 '$($rows[0].ledger_id)'）"

    # ================= 2/4 切到另一个账本：看不见 A 的日记 =================
    Write-Host "`n[diary-ledger] 2/4 切到「$($target.name)」：日记应当看不见"
    Assert-True (Switch-Ledger -Window $window -Current $current.name -Target $target.name) "已切到「$($target.name)」"
    $text = Get-DiaryContent -Window $window
    Assert-True ($null -ne $text) '切账本后编辑器仍在（拿得到正文）'
    Assert-True ($text -ne $contentA) "切账本后编辑器里不是 A 的正文（实际 '$text'）"
    Assert-True (@(Read-DiaryRows | Where-Object { $_.ledger_id -eq $target.id }).Count -eq 0) `
        "目标账本里还没有今天这一条"

    # ================= 3/4 在另一个账本的同一天写不同内容 =================
    Write-Host "`n[diary-ledger] 3/4 目标账本的同一天写不同内容"
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        Set-DiaryContent -Window $window -Text $contentB | Out-Null
        Save-Now -Window $window
        $targetRows = @(Read-DiaryRows | Where-Object { $_.ledger_id -eq $target.id })
        if ($targetRows.Count -eq 1 -and $targetRows[0].content -eq $contentB) { break }
        Write-Host "    第 $attempt 次写入没生效（目标账本 $($targetRows.Count) 条），重试" -ForegroundColor DarkYellow
    }
    $todayRows = @(Read-DiaryRows)
    Assert-True ($todayRows.Count -eq 2) "同一天在库里共 2 行（实际 $($todayRows.Count)）—— 复合唯一键允许不同账本各存一篇"
    $rowA = @($todayRows | Where-Object { $_.ledger_id -eq $current.id })
    $rowB = @($todayRows | Where-Object { $_.ledger_id -eq $target.id })
    Assert-True ($rowA.Count -eq 1 -and $rowB.Count -eq 1) '两行分别属于两个账本'
    if ($rowA.Count -eq 1 -and $rowB.Count -eq 1) {
        Assert-True ($rowA[0].content -eq $contentA) '账本 A 的正文没被覆盖'
        Assert-True ($rowB[0].content -eq $contentB) '账本 B 的正文是新写的'
        Assert-True ($rowA[0].id -ne $rowB[0].id) '两行是不同记录（不是同一行被改写）'
    }

    # ================= 4/4 切回当前账本：A 的正文原样还在 =================
    Write-Host "`n[diary-ledger] 4/4 切回「$($current.name)」：A 的正文应当原样还在"
    Assert-True (Switch-Ledger -Window $window -Current $target.name -Target $current.name) "已切回「$($current.name)」"
    $back = ''
    $deadline = (Get-Date).AddSeconds(15)
    do {
        $back = Get-DiaryContent -Window $window
        if ($back -ne $contentA) { Start-Sleep -Milliseconds 500 }
    } while ($back -ne $contentA -and (Get-Date) -lt $deadline)
    Assert-True ($back -eq $contentA) "切回后编辑器里又是 A 的正文（实际 '$back'）"
}
finally { Stop-TrApp -Process $process -Failures $failures -OutDir $OutDir }

Show-TrSummary -Failures $failures -Tag 'diary-ledger' -SuccessMessage "[diary-ledger] 全部通过：切账本互不可见 + 同一天两账本各存一篇 + 切回内容原样"
