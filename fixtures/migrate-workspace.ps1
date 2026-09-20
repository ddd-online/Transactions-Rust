# migrate-workspace.ps1 —— 工作空间**自动迁移**端到端（把旧格式库升级到当前格式）。
#
# 为什么需要它：迁移引擎会在打开工作空间时改写用户数据，这是全仓风险最高的一条路径。
# 本脚本复制一份已播种的工作空间，把它**降级**成"日记还没有账本"的旧格式
# （去掉 ledger_id、旧的全局唯一索引回来、抹掉登记行），再启动真实外壳，断言：
#   1. 应用正常打开（迁移成功，不是拒绝打开）；
#   2. 库结构升级到位：日记有 ledger_id、复合唯一索引在、旧索引没了、登记行写上了；
#   3. **老日记正文一字不差**，并归到最早创建的账本；
#   4. 升级前**留了备份**，且备份文件里是升级前的样子；
#   5. 再启动一次：不重复备份、不重复改（幂等）。
#
# 用法（pwsh 7；需要 release 产物；本仓库不能有实例在跑）：
#   pwsh -File fixtures/migrate-workspace.ps1 [-Exe <exe>] [-OutDir <dir>]

param(
    [string]$Exe,
    [string]$SmokeHome,
    [string]$OutDir,
    [string]$SourceWorkspace
)

$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot 'lib\TrUia.ps1')

$repo = Split-Path -Parent $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $repo 'target\release\transactions.exe' }
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\migrate\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\migrate' }
if (-not $SourceWorkspace) { $SourceWorkspace = Join-Path $OutDir 'seed' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$trPaths = Initialize-TrSmokeHome -SmokeHome $SmokeHome -OutDir $OutDir
$smokeHome = $trPaths.SmokeHome
$OutDir = $trPaths.OutDir
Assert-NoRepoInstance -Repo $repo

$ws = [System.IO.Path]::GetFullPath((Join-Path $OutDir 'ws'))
$failures = New-Object System.Collections.Generic.List[string]

# ---- 1. 播种一份干净的工作空间（当前格式）----
if (Test-Path $SourceWorkspace) { Remove-Item $SourceWorkspace -Recurse -Force }
Push-Location $repo
& cargo -q xtask seed $SourceWorkspace *> (Join-Path $OutDir 'seed.log')
$seedExit = $LASTEXITCODE
Pop-Location
if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
Write-Host "[migrate] 已播种当前格式的工作空间: $SourceWorkspace" -ForegroundColor Cyan

# 复制成"待升级"的工作空间（连 data/ 一起，模拟真实工作空间）
if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
Copy-Item $SourceWorkspace $ws -Recurse
$db = Join-Path $ws 'transactions.db'

# ---- 2. 把它降级成旧格式：老索引回来、ledger_id 去掉、登记行删掉 ----
# 用系统 sqlite3（本机在 PATH；AGENTS 里记的 Miniconda 那份）。降级必须在**应用启动前**做完。
$downgrade = @'
PRAGMA foreign_keys = OFF;
BEGIN IMMEDIATE;
DROP INDEX IF EXISTS idx_tbl_billadm_diary_entry_ledger_date;
CREATE UNIQUE INDEX idx_tbl_billadm_diary_entry_date ON tbl_billadm_diary_entry(date);
ALTER TABLE tbl_billadm_diary_entry DROP COLUMN ledger_id;
DELETE FROM tbl_billadm_schema_migration WHERE id = '20260920_diary_ledger_scope';
COMMIT;
'@
$sqlFile = Join-Path $OutDir 'downgrade.sql'
Set-Content -Path $sqlFile -Value $downgrade -Encoding UTF8
& sqlite3 $db ".read $($sqlFile.Replace('\', '/'))"
if ($LASTEXITCODE -ne 0) { throw "降级失败（sqlite3 exit=$LASTEXITCODE）" }

# 降级后的样子（升级前）：列没有、旧索引在
$before = & sqlite3 $db "SELECT name FROM pragma_table_info('tbl_billadm_diary_entry') WHERE name='ledger_id';"
Assert-True ([string]::IsNullOrWhiteSpace($before)) '降级后日记表没有 ledger_id 列'
$legacyIndex = & sqlite3 $db "SELECT name FROM sqlite_master WHERE type='index' AND name='idx_tbl_billadm_diary_entry_date';"
Assert-True ($legacyIndex -eq 'idx_tbl_billadm_diary_entry_date') '降级后旧的全工作空间唯一索引在'

# 让"最早创建的账本"在库里与界面上都**无歧义**（种子两个账本的 created_at 可能同秒）：
# 目标账本 created_at = 1，其余 = 999。界面按 created_at 选当前账本，因此界面里就是它。
& sqlite3 $db "UPDATE tbl_billadm_ledger SET created_at = 999; UPDATE tbl_billadm_ledger SET created_at = 1 WHERE id = (SELECT id FROM tbl_billadm_ledger ORDER BY created_at, rowid LIMIT 1);"
if ($LASTEXITCODE -ne 0) { throw "设置账本创建时间失败（sqlite3 exit=$LASTEXITCODE）" }

# 记下升级前的老日记与"最早创建的账本"
$oldDiary = @((& sqlite3 -json $db "SELECT date, content FROM tbl_billadm_diary_entry ORDER BY date;") -join "`n" | ConvertFrom-Json)
$oldestLedger = & sqlite3 $db "SELECT id FROM tbl_billadm_ledger ORDER BY created_at, rowid LIMIT 1;"
Assert-True (-not [string]::IsNullOrWhiteSpace($oldestLedger)) '工作空间里至少有一个账本'
Assert-True ($oldDiary.Count -ge 1) "升级前有老日记可供核对（实际 $($oldDiary.Count) 篇）"
Write-Host "[migrate] 升级前：日记 $($oldDiary.Count) 篇，最早账本 $oldestLedger" -ForegroundColor Cyan

# 冒充"上一次升级留下的备份"：升级后它必须被清掉（同一工作空间只保留最近一份）
$staleBackup = Join-Path $ws 'transactions.db.pre-migration-1.bak'
Set-Content -Path $staleBackup -Value 'stale-backup' -Encoding ascii
# 顺带放一个不符合命名规则的文件：清理时绝不能碰它
$bystander = Join-Path $ws '我的手工备份.bak'
Set-Content -Path $bystander -Value 'keep-me' -Encoding ascii
Assert-True (Test-Path $staleBackup) '已放入一份旧的迁移备份（用于验证"只留最近一份"）'

@{ width = 1500; height = 950; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'migrate-workspace.ps1' } |
    ConvertTo-Json | Set-Content -Path (Join-Path $smokeHome '.transactions.json') -Encoding UTF8

# ---- 3. 启动外壳：应当自动升级 ----
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

    # 能拿到主窗口（含侧栏）就说明工作空间被成功打开 = 迁移成功
    $window = Get-ReadyWindow -ProcessId $process.Id -TimeoutSec 60
    Assert-True ([bool]$window) '旧格式工作空间能被打开（主窗口出现，说明迁移成功而不是被拒绝）'
    if (-not $window) { throw '没有拿到主窗口：迁移可能失败了（看工作空间目录下的 transactions.log）' }
    Start-Sleep -Seconds 2

    # ---- 4. 结构升级到位 ----
    $columns = @(& sqlite3 $db "SELECT name FROM pragma_table_info('tbl_billadm_diary_entry');")
    Assert-True ($columns -contains 'ledger_id') '升级后日记表有 ledger_id 列'
    $addedIndex = & sqlite3 $db "SELECT name FROM sqlite_master WHERE type='index' AND name='idx_tbl_billadm_diary_entry_ledger_date';"
    Assert-True ($addedIndex -eq 'idx_tbl_billadm_diary_entry_ledger_date') '升级后有 (ledger_id,date) 复合唯一索引'
    $legacyAfter = & sqlite3 $db "SELECT name FROM sqlite_master WHERE type='index' AND name='idx_tbl_billadm_diary_entry_date';"
    Assert-True ([string]::IsNullOrWhiteSpace($legacyAfter)) '升级后旧的全工作空间唯一索引已删除'
    $registered = & sqlite3 $db "SELECT id FROM tbl_billadm_schema_migration WHERE id='20260920_diary_ledger_scope';"
    Assert-True ($registered -eq '20260920_diary_ledger_scope') '升级写了迁移登记行'

    # ---- 5. 老日记一字不差，且归到最早账本 ----
    $newDiary = @((& sqlite3 -json $db "SELECT date, content, ledger_id FROM tbl_billadm_diary_entry ORDER BY date;") -join "`n" | ConvertFrom-Json)
    Assert-True ($newDiary.Count -eq $oldDiary.Count) "日记条数不变（升级前 $($oldDiary.Count)，升级后 $($newDiary.Count)）"
    foreach ($row in $oldDiary) {
        $match = @($newDiary | Where-Object { $_.date -eq $row.date })
        Assert-True ($match.Count -eq 1) "升级后仍有 $($row.date) 这一篇"
        if ($match.Count -eq 1) {
            Assert-True ($match[0].content -eq $row.content) "$($row.date) 正文一字不差"
            Assert-True ($match[0].ledger_id -eq $oldestLedger) "$($row.date) 归到最早创建的账本"
        }
    }

    # ---- 6. 升级前留了备份（且**只留这一份**），备份里是升级前的样子 ----
    $backups = @(Get-ChildItem $ws -Filter 'transactions.db.pre-migration-*.bak')
    Assert-True ($backups.Count -eq 1) "只保留最近一份备份（实际 $($backups.Count) 份）"
    Assert-True (-not (Test-Path $staleBackup)) '上一次升级留下的旧备份已被清掉'
    Assert-True (Test-Path $bystander) '不符合命名规则的文件（手工备份）没被动'
    if ($backups.Count -ge 1) {
        $backupColumns = @(& sqlite3 $backups[0].FullName "SELECT name FROM pragma_table_info('tbl_billadm_diary_entry');")
        Assert-True (-not ($backupColumns -contains 'ledger_id')) '备份里是升级前的结构（没有 ledger_id）'
    }

    # ---- 7. 界面里能读到老日记：日期树里出现那一年的节点、且计到 1 篇 ----
    # （当前账本 = 最早创建的账本，老日记全归它；页面默认选中"今天"，所以看树而不是编辑器）
    Write-Host "`n[migrate] 界面核对：日记页的日期树里有老日记" -ForegroundColor Cyan
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '日记')) '打开「日记」'
    Start-Sleep -Seconds 4
    Assert-True ([bool](Get-DiaryTextarea -Window $window)) '日记页渲染出编辑器'
    $newest = @($oldDiary | Sort-Object date | Select-Object -Last 1)
    Assert-True ($newest.Count -eq 1) '取到一篇老日记用于界面核对'
    $year = ([string]$newest[0].date).Substring(0, 4)
    $month = [int]([string]$newest[0].date).Substring(5, 2)
    $yearNode = Wait-Like -Root $window -Pattern "$($year)年" -TimeoutSec 20
    Assert-True ([bool]$yearNode) "日期树里出现 $year 年节点（升级后的老日记在当前账本里可见）"
    if ($yearNode) {
        Assert-True ($yearNode.Current.Name -like '*1篇*') `
            "该年份下计到 1 篇（实际 '$($yearNode.Current.Name)'）"
        Invoke-Element $yearNode | Out-Null
        Start-Sleep -Seconds 1
        $monthNode = Wait-Like -Root $window -Pattern "$($month)月" -TimeoutSec 15
        Assert-True ([bool]$monthNode) "展开后出现 $($month) 月节点"
    }
}
finally { Stop-TrApp -Process $process -Failures $failures -OutDir $OutDir }

# ---- 8. 幂等：再启动一次，不重复备份、不重复改 ----
$backupCountBefore = @(Get-ChildItem $ws -Filter 'transactions.db.pre-migration-*.bak').Count
$process2 = $null
try {
    $saved = @{ USERPROFILE = $env:USERPROFILE; HOME = $env:HOME }
    try {
        $env:USERPROFILE = $smokeHome
        $env:HOME = $smokeHome
        $process2 = Start-Process -FilePath $Exe -PassThru
    }
    finally {
        $env:USERPROFILE = $saved.USERPROFILE
        $env:HOME = $saved.HOME
    }
    $window2 = Get-ReadyWindow -ProcessId $process2.Id -TimeoutSec 60
    Assert-True ([bool]$window2) '第二次启动同样能打开（已是当前格式）'
    Start-Sleep -Seconds 2
}
finally { Stop-TrApp -Process $process2 -Failures $failures -OutDir $OutDir }

$backupCountAfter = @(Get-ChildItem $ws -Filter 'transactions.db.pre-migration-*.bak').Count
Assert-True ($backupCountAfter -eq $backupCountBefore) `
    "已是当前格式时不再备份（第二次启动前后都是 $backupCountBefore 份）"

Show-TrSummary -Failures $failures -Tag 'migrate' -SuccessMessage "[migrate] 全部通过：旧格式自动升级 + 数据一字不差 + 升级前备份 + 幂等"
