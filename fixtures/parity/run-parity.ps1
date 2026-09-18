# run-parity.ps1 —— 一条命令跑完数据级黄金对比（验收护栏）。
#
# 步骤：
#   1. 用 Go 参考实现重放种子操作序列 -> 全新工作空间 A（go-driver.ps1）
#   2. 用 `cargo xtask seed` 在全新工作空间 B 播种同一批输入
#   3. 两侧各 `cargo xtask dump` 出规范化 JSON，再 `cargo xtask parity diff` 比较
#   4. 退出码即结论：0 = 两侧落库逐字段一致，1 = 有差异（打印 JSON 路径）
#
# 用法（pwsh 7；中文注释需要 pwsh 或带 BOM 的 5.1）：
#   pwsh -File fixtures/parity/run-parity.ps1
#   pwsh -File fixtures/parity/run-parity.ps1 -OutDir D:\tmp\parity -Port 29143
#
# 前置：本机装有 Go（用于跑参考内核）、cargo；参考仓库在 $KernelDir 只读存在。
# 产物默认落在 <repo>/target/parity/：parity-go.json / parity-rust.json / 归一化文件 / 日志。

param(
    [string]$OutDir,
    [int]$Port = 29143,
    [string]$KernelDir = 'D:\github\Transactions\kernel'
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\parity' }
# **必须绝对化**：Go 内核以 `-WorkingDirectory <KernelDir>` 启动，会按**它自己的 cwd** 解析
# 传进去的工作空间路径。相对路径会让库落到参照仓库里，而 `xtask dump`（相对本仓库）找不到它 ——
# 表现是"HTTP 全部成功，第一个落库断言才报『目录中没有 transactions.db』"。
# 与 `fixtures/ui-diary-io.ps1` 里"填进原生选目录框的路径必须绝对"是同一类坑。
$OutDir = [System.IO.Path]::GetFullPath($OutDir)
$KernelDir = [System.IO.Path]::GetFullPath($KernelDir)
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }

$goWorkspace = [System.IO.Path]::GetFullPath((Join-Path $OutDir 'ws-go'))
$rustWorkspace = [System.IO.Path]::GetFullPath((Join-Path $OutDir 'ws-rust'))
foreach ($dir in @($goWorkspace, $rustWorkspace)) {
    if (Test-Path $dir) { Remove-Item $dir -Recurse -Force }
}

# cargo 的进度输出走 stderr；JSON 必须只留 stdout，所以两条流分开重定向。
$env:HTTPS_PROXY = if ($env:HTTPS_PROXY) { $env:HTTPS_PROXY } else { 'http://127.0.0.1:7890' }
$env:HTTP_PROXY = $env:HTTPS_PROXY
$env:CARGO_NET_RETRY = '10'
$env:CARGO_HTTP_TIMEOUT = '120'

function Invoke-Xtask {
    param([string[]]$Arguments, [string]$StdOut, [string]$StdErr)
    if ($StdOut) {
        & cargo -q xtask @Arguments 2>$StdErr >$StdOut
    }
    else {
        & cargo -q xtask @Arguments *> (Join-Path $OutDir 'xtask.log')
    }
    if ($LASTEXITCODE -ne 0) { throw "cargo xtask $($Arguments -join ' ') 失败（exit=$LASTEXITCODE）" }
}

Write-Host '[parity] 1/3 用 Go 参考实现重放操作序列…' -ForegroundColor Cyan
& pwsh -NoProfile -File (Join-Path $PSScriptRoot 'go-driver.ps1') -Workspace $goWorkspace -Port $Port -KernelDir $KernelDir `
    *> (Join-Path $OutDir 'go-driver.log')
if ($LASTEXITCODE -ne 0) {
    Get-Content (Join-Path $OutDir 'go-driver.log') -Tail 30
    throw "Go 侧重放失败（exit=$LASTEXITCODE）"
}

Write-Host '[parity] 2/3 在 Rust 侧重放同一批输入…' -ForegroundColor Cyan
Invoke-Xtask -Arguments @('seed', $rustWorkspace) -StdErr (Join-Path $OutDir 'seed.err')

Write-Host '[parity] 3/3 导出两侧数据并比较…' -ForegroundColor Cyan
$goJson = Join-Path $OutDir 'parity-go.json'
$rustJson = Join-Path $OutDir 'parity-rust.json'
$goNorm = Join-Path $OutDir 'parity-go-norm.json'
$rustNorm = Join-Path $OutDir 'parity-rust-norm.json'
Invoke-Xtask -Arguments @('dump', $goWorkspace) -StdOut $goJson -StdErr (Join-Path $OutDir 'dump-go.err')
Invoke-Xtask -Arguments @('dump', $rustWorkspace) -StdOut $rustJson -StdErr (Join-Path $OutDir 'dump-rust.err')
Invoke-Xtask -Arguments @('parity', 'normalize', $goJson, '--out', $goNorm)
Invoke-Xtask -Arguments @('parity', 'normalize', $rustJson, '--out', $rustNorm)

& cargo -q xtask parity diff $goNorm $rustNorm *> (Join-Path $OutDir 'parity-diff.log')
$diff = $LASTEXITCODE
Get-Content (Join-Path $OutDir 'parity-diff.log')

# 确定性提示（**不判失败**）：股票资金记录若在**同一秒**落多条，Go 侧（GORM autoCreateTime 只有秒级精度）
# 的相对顺序可能退化成随机 UUID 字典序，而 Rust 侧 `create_fund_record` 把 created_at 拉成严格递增
# （这是 AGENTS.md 里记录在案的**有意偏离**）。两侧的 `recalculateCashChain` 都"按录入顺序逐条取前值"算余额，
# 顺序一变余额就跟着变 —— 这类并列**无法完全消除**：重放会在一秒内重写整批派生资金记录。
# 所以这里只做提示：出现同秒时说明"这一轮如果红了，先怀疑它"。
$goDump = Get-Content $goJson -Raw | ConvertFrom-Json
$fundRows = @($goDump.tbl_billadm_stock_fund_record)
$tieNote = $false
if ($fundRows.Count -gt 1) {
    $ties = @($fundRows | Group-Object created_at | Where-Object { $_.Count -gt 1 })
    if ($ties.Count -gt 0) {
        $tieNote = $true
        Write-Host "[parity] 提示：Go 侧资金记录有 $($ties.Count) 组同秒 created_at（$((($ties | ForEach-Object { "$($_.Name)×$($_.Count)" }) -join ', '))）" -ForegroundColor Yellow
        Write-Host '  这些行的余额取决于重放时的取前值顺序，是"Go 秒级时间戳 vs Rust 单调递增"这一已知偏离的产物；' -ForegroundColor Yellow
        Write-Host '  若本轮 diff 红且差异**只**出现在 tbl_billadm_stock_fund_record[*].cash_balance，请先重跑一次再判断。' -ForegroundColor Yellow
    }
}

if ($diff -eq 0) {
    $suffix = if ($tieNote) { '（注：存在同秒并列，见上方提示）' } else { '' }
    Write-Host "[parity] ✅ 数据级黄金对比通过，产物: $OutDir $suffix" -ForegroundColor Green
}
else {
    Write-Host "[parity] ❌ 两侧落库存在差异（exit=$diff），详见 $OutDir" -ForegroundColor Red
}
exit $diff
