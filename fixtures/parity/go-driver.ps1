# go-driver.ps1 —— 用 **Go 参考实现**重放与 `cargo xtask seed` 完全相同的操作序列。
#
# 目的：数据级黄金对比——同一批输入分别由 Rust 与 Go 写入两个全新工作空间，
#       再用 `cargo xtask dump` + `cargo xtask parity diff` 比较落库结果。
#       这能直接暴露 SQL 层面的偏差（费用分摊、轮次归档、资金链、JSON 字段形态等）。
#
# 用法（建议用 pwsh 7 运行；Windows PowerShell 5.1 需要文件带 BOM 才能正确读中文）：
#   pwsh -File fixtures/parity/go-driver.ps1 -Workspace D:\tmp\ws-go -Port 29143
#
# 取值必须与 `xtask/src/seed.rs` 逐字一致，否则对比无意义。

param(
    [Parameter(Mandatory = $true)][string]$Workspace,
    [int]$Port = 29143,
    [string]$KernelDir = 'D:\github\Transactions\kernel'
)

$ErrorActionPreference = 'Stop'
$baseUrl = "http://127.0.0.1:$Port/api/v1"

if (-not (Test-Path $Workspace)) { New-Item -ItemType Directory -Path $Workspace -Force | Out-Null }

Write-Host "[go-driver] 启动 Go 内核 ($KernelDir, port $Port)…" -ForegroundColor Cyan
$out = Join-Path $env:TEMP 'go-driver-kernel.out'
$err = Join-Path $env:TEMP 'go-driver-kernel.err'
$kernel = Start-Process -FilePath 'go' -ArgumentList @('run', 'main.go', '-port', "$Port", '-mode', 'release', '-workspace', $Workspace) `
    -WorkingDirectory $KernelDir -RedirectStandardOutput $out -RedirectStandardError $err -PassThru -WindowStyle Hidden

function Stop-Kernel {
    if ($kernel -and -not $kernel.HasExited) { taskkill /PID $kernel.Id /T /F 2>&1 | Out-Null }
}
trap { Stop-Kernel; throw }

$healthy = $false
for ($i = 0; $i -lt 240; $i++) {
    Start-Sleep -Milliseconds 500
    try {
        Invoke-WebRequest "$baseUrl/health" -TimeoutSec 2 -UseBasicParsing | Out-Null
        $healthy = $true; break
    }
    catch { if ($kernel.HasExited) { break } }
}
if (-not $healthy) {
    Stop-Kernel
    Get-Content $out -Tail 20 -ErrorAction SilentlyContinue
    Get-Content $err -Tail 20 -ErrorAction SilentlyContinue
    throw 'Go 内核未在预期时间内就绪'
}
Write-Host "[go-driver] 内核就绪，开始重放操作序列" -ForegroundColor Green

function Invoke-Api {
    param([string]$Method, [string]$Path, $Body = $null)
    $params = @{
        Method  = $Method
        Uri     = "$baseUrl$Path"
        Headers = @{ 'Content-Type' = 'application/json' }
    }
    if ($null -ne $Body) { $params.Body = ($Body | ConvertTo-Json -Depth 10 -Compress) }
    $response = Invoke-RestMethod @params
    if ($response.code -ne 0) { throw "接口失败 $Method $Path -> code=$($response.code) msg=$($response.msg)" }
    return $response.data
}

function New-Tr {
    param($LedgerId, [int64]$Price, [string]$Type, [string]$Category, [string]$Description, [int64]$At, [bool]$Outlier, [string[]]$Tags)
    return Invoke-Api POST '/transactions' @{
        ledgerId        = $LedgerId
        price           = $Price
        transactionType = $Type
        category        = $Category
        description     = $Description
        tags            = $Tags
        transactionAt   = $At
        outlier         = $Outlier
    }
}

# 阶段 2 的取值必须与 `xtask/src/seed.rs` 逐字一致。
$p2Category = '阶段二分类'
$p2Tag = '阶段二标签'
$p2ChartTitle = '阶段二图表'
$p2TemplateName = '阶段二模板'
$p2StockTag = '波段'

# P1-2 图表更新的取值必须与 `xtask/src/seed.rs` 逐字一致。
$p1ChartUpdatedTitle = '阶段二图表（更新：分类+标签+离群点）'
$p1ChartUpdatedGranularity = 'year'
$p1ChartUpdatedChartType = 'bar'
# 大于 3 个预设图表的 0/1/2 → 更新后排在列表末尾。
$p1ChartUpdatedSortOrder = 9

# P1-3 的四个 sort_order 取值必须与 `xtask/src/seed.rs` 逐字一致。
$p1TemplateSortOrder = 7
# 分类与标签是整组重排：一组里每一行都改成新序号（刻意互不相同且不连续）。
$p1CategorySortOrder = [ordered]@{
    '餐饮美食' = 5; '购物消费' = 3; '交通出行' = 8; '生活缴费' = 1; '贷款还款' = 9
    '医疗健康' = 2; '娱乐休闲' = 6; '人情往来' = 4; '教育学习' = 7
}
$p1TagSortOrder = [ordered]@{
    '三餐' = 10; '零食' = 9; '商场' = 8; '外卖' = 7; '饮料' = 6
    '奶茶' = 5; '咖啡' = 4; '水果' = 3; '茶叶' = 2; '买菜' = 1
}

# P1-4 关键事件的日期与两次写入取值必须与 `xtask/src/seed.rs` 逐字一致。
$p1KeyEventDate = '2026-04-01'
$p1KeyEventTitle = 'P1-4 覆盖写标题'
$p1KeyEventContent = "## 覆盖写`n`n- 第二次写入必须更新内容`n- 且不得新增行"
$p1KeyEventColor = 'normal'
$p1KeyEventTitleV2 = 'P1-4 覆盖写标题（二次）'
$p1KeyEventContentV2 = "## 覆盖写（二次）`n`n- 只更新 title/content/color/updated_at"
$p1KeyEventColorV2 = 'outlier'

# 阶段 3 的账本改名取值必须与 `xtask/src/seed.rs` 逐字一致。
$p3LedgerName = '默认账本（改名）'
$p3LedgerDescription = '阶段三更新描述'

# 阶段 3 的取值必须与 `xtask/src/seed.rs` 逐字一致（独立账本 + 互不相同的成交时间）。
$p3Open = 1778198400
$p3Reduce1 = 1778284800
$p3Reduce2 = 1778371200
$p3Add = 1778457600
$p3Close = 1778544000
$p3R2Open = 1778716800
$p3R2Close = 1778803200
$p3R4Open = 1778976000
$p3R4Close = 1779062400

# 按描述查回一条消费记录（与 Rust 侧 `phase2_find_record` 走同一个公开查询路径）。
function Find-TrByDescription {
    param([string]$LedgerId, [string]$Description)
    $result = Invoke-Api POST '/transactions/query' @{
        ledgerId = $LedgerId
        items    = @(@{ description = $Description })
    }
    if (-not $result.items -or $result.items.Count -lt 1) {
        throw "未查到消费记录: $Description"
    }
    return $result.items[0]
}

# 阶段 3 的「股票派生数据指纹」：断言 `POST /stock/trades/impact` 绝不落库。
# 两侧各自比较「预演调用前 / 后」的自己的库（Go 侧用 `xtask dump` 只读导出），
# 因此这里比的是自洽性，跨侧相等仍由 `parity diff` 负责。
function Get-StockSnapshot {
    param([string]$WorkspaceDir)
    $text = ''
    foreach ($table in @('tbl_billadm_stock_trade', 'tbl_billadm_stock_trade_history',
            'tbl_billadm_stock_trade_round', 'tbl_billadm_stock_position',
            'tbl_billadm_stock_fund_record')) {
        $text += (& cargo -q xtask dump $WorkspaceDir --table $table 2>$null | Out-String)
    }
    if ($LASTEXITCODE -ne 0) { throw "dump 失败: $WorkspaceDir" }
    return $text
}

function Invoke-ApiExpectError {
    <#
      调用一个**预期失败**的接口：断言 HTTP 状态码与错误信封的 code。
      `Invoke-RestMethod` 在 400 上会抛异常，响应体在 `$_.ErrorDetails.Message` 里，
      因此这里绕开 `Invoke-Api`（它把 code != 0 直接 throw）单独解析。
    #>
    param([string]$Method, [string]$Path, $Body, [int]$ExpectStatus, [string]$ExpectMsgLike = '')
    $params = @{
        Method      = $Method
        Uri         = "$baseUrl$Path"
        Headers     = @{ 'Content-Type' = 'application/json' }
        ErrorAction = 'Stop'
    }
    if ($null -ne $Body) { $params.Body = ($Body | ConvertTo-Json -Depth 10 -Compress) }
    try {
        Invoke-RestMethod @params | Out-Null
    }
    catch {
        $status = [int]$_.Exception.Response.StatusCode
        $text = $_.ErrorDetails.Message
        $payload = $null
        if ($text) { $payload = $text | ConvertFrom-Json }
        if ($status -ne $ExpectStatus) {
            throw "预期 HTTP $ExpectStatus，实际 $status（$Method ${Path}: $text）"
        }
        if ($null -eq $payload -or $payload.code -ne -1) {
            throw "预期错误信封 code=-1，实际 $text"
        }
        if ($ExpectMsgLike -and $payload.msg -notlike "*$ExpectMsgLike*") {
            throw "预期错误文案包含「$ExpectMsgLike」，实际「$($payload.msg)」"
        }
        return $payload
    }
    throw "预期 $Method $Path 返回 HTTP $ExpectStatus，实际成功"
}

# P1-4 的「关键事件行数」指纹：`xtask dump --table` 是只读导出，
# 与 Rust 侧 `key_event_row_count()` 读同一张表，用来断言"二次 upsert 是覆盖不是新增"。
function Get-KeyEventCount {
    param([string]$WorkspaceDir, [string]$LedgerId)
    $json = (& cargo -q xtask dump $WorkspaceDir --table 'tbl_billadm_key_event' 2>$null | Out-String)
    if ($LASTEXITCODE -ne 0) { throw "dump 失败: $WorkspaceDir" }
    $rows = @(($json | ConvertFrom-Json).tbl_billadm_key_event)
    return @($rows | Where-Object { $_.ledger_id -eq $LedgerId }).Count
}

try {
    Invoke-Api POST '/workspace' @{ workspaceDir = $Workspace } | Out-Null

    # ---- 账本 ----
    $main = Invoke-Api POST '/ledgers' @{ name = '默认账本'; description = '种子数据' }
    $other = Invoke-Api POST '/ledgers' @{ name = '备用账本'; description = '' }
    Write-Host "[go-driver] 账本 $main / $other"

    # ---- 分类与标签（19 / 57）----
    Invoke-Api POST '/categories/initialize' @{ ledgerId = $main } | Out-Null

    # ---- 消费记录（与 seed.rs 的取值逐一对应）----
    $d0105 = 1767571200   # 2026-01-05 UTC
    $d0210 = 1770681600   # 2026-02-10 UTC
    $d0315 = 1773590400   # 2026-03-15 UTC

    # 第 3 条（耳机）会被关联到关键事件，因此单独接住它的返回值
    New-Tr $main 12345  'expense'  '餐饮美食' '午餐'     $d0105 $false @('三餐')      | Out-Null
    New-Tr $main 8800   'expense'  '交通出行' '地铁'     $d0105 $false @('地铁')      | Out-Null
    $third = New-Tr $main 259900 'expense'  '购物消费' '耳机'     $d0210 $true  @('数码')
    New-Tr $main 1500000 'income'  '工资奖金' '一月工资' $d0210 $false @('工资')      | Out-Null
    New-Tr $main 1500000 'income'  '工资奖金' '二月工资' $d0315 $false @('工资')      | Out-Null
    New-Tr $main 300000 'transfer' '五险一金' '公积金'   $d0315 $false @('住房')      | Out-Null
    New-Tr $other 6600  'expense'  '餐饮美食' '备用账本的一条' $d0105 $false @()      | Out-Null

    # ---- 关键事件 + 关联 ----
    Invoke-Api POST '/key-events' @{
        ledger_id = $main
        date      = '2026-02-10'
        title     = '买了新耳机'
        content   = "## 记录`n`n- 价格 2599 元`n- 有点冲动消费"
        color     = 'outlier'
    } | Out-Null
    Invoke-Api POST '/transactions/link' @{ transaction_id = $third; date = '2026-02-10' } | Out-Null

    # ---- 日记 ----
    Invoke-Api PUT '/diary/2026-02-10' @{ content = "# 2026-02-10`n`n今天买了耳机，复盘一下。"; mood = '开心' } | Out-Null
    Invoke-Api PUT '/diary/2026-03-15' @{ content = '发工资了，先存一半。'; mood = '平静' } | Out-Null

    # ---- 消费模板 ----
    # 接住模板 ID：P1-3a 的 `PATCH /templates/:id/sort` 需要它（seed.rs 用同一处返回值）。
    $templateId = Invoke-Api POST '/templates' @{
        ledger_id        = $main
        template_name    = '早餐'
        transaction_type = 'expense'
        category         = '餐饮美食'
        tags             = @('三餐')
        flags            = ''
        description      = '早餐'
        sort_order       = 0
    }

    # ---- 图表（GET 会顺带播种 3 个预设）----
    Invoke-Api GET "/charts?ledgerId=$main" | Out-Null

    # ---- 股票（与 seed.rs 同一批输入）----
    Invoke-Api POST '/stock/account/principal' @{ ledger_id = $main; amount = 15000000 } | Out-Null
    Invoke-Api POST '/stock/trades' @{
        ledger_id  = $main
        stock_code = '600519'
        stock_name = '贵州茅台'
        trade_type = 'open'
        trade_time = 1767657600
        remark     = '种子建仓'
        fills      = @(@{ price = 1700.00; lots = 1 }, @{ price = 1701.50; lots = 1 })
    } | Out-Null
    Invoke-Api POST '/stock/trades' @{
        ledger_id  = $main
        stock_code = '600519'
        stock_name = '贵州茅台'
        trade_type = 'add'
        trade_time = 1768867200
        remark     = '种子加仓'
        fills      = @(@{ price = 1695.00; lots = 1 })
    } | Out-Null
    Invoke-Api POST '/stock/trades' @{
        ledger_id  = $main
        stock_code = '600519'
        stock_name = '贵州茅台'
        trade_type = 'close'
        trade_time = 1773100800
        remark     = '种子清仓'
        tag        = '打板'
        fills      = @(@{ price = 1750.00; lots = 3 })
    } | Out-Null
    Invoke-Api POST '/stock/account/withdraw' @{ ledger_id = $main; amount = 1000000; date = '2026-03-20' } | Out-Null

    $overview = Invoke-Api GET "/stock/account/overview?ledger_id=$main"
    Write-Host "[go-driver] 股票总览: 本金=$($overview.principal) 可用现金=$($overview.availableCash) 已实现盈亏=$($overview.realizedPnl)" -ForegroundColor Green

    # ==================================================================================
    # 阶段 2：更新 / 删除写入路径（必须与 `xtask/src/seed.rs` 的阶段 2 同序同值）
    # ==================================================================================
    Write-Host '[go-driver] ---- 阶段 2：更新 / 删除 ----' -ForegroundColor Cyan

    # ---- 1. 消费记录：批量新建 → 关联 → 取消关联 → 删除 ----
    Invoke-Api POST '/transactions/batch' @(
        @{
            ledgerId        = $main
            price           = 66600
            transactionType = 'expense'
            category        = '餐饮美食'
            description     = '批量A'
            tags            = @('三餐')
            transactionAt   = $d0315
            outlier         = $false
        },
        @{
            ledgerId        = $main
            price           = 8800
            transactionType = 'expense'
            category        = '交通出行'
            description     = '批量B'
            tags            = @('地铁')
            transactionAt   = $d0315
            outlier         = $false
        }
    ) | Out-Null
    $batchA = Find-TrByDescription -LedgerId $main -Description '批量A'
    Invoke-Api POST '/transactions/link' @{ transaction_id = $batchA.transactionId; date = '2026-04-01' } | Out-Null
    Invoke-Api POST '/transactions/unlink' @{ transaction_id = $batchA.transactionId } | Out-Null
    Invoke-Api DELETE "/transactions/$($batchA.transactionId)" | Out-Null
    Write-Host "[go-driver] 批量新建 2 条，批量A=$($batchA.transactionId) 关联→取消关联→删除"

    # ----------------------------------------------------------------------------------
    # 确定性护栏：把「非交易资金记录（支取）」与「重放重建的买卖资金记录」的时间戳错开。
    #
    # 现象（实测）：不加这段等待时，`run-parity.ps1` 会在约 1/5 的运行里失败，且失败点
    # 恒定是 `tbl_billadm_stock_fund_record[*].cash_balance`（5 处），其它列全部一致。
    # 对照两次运行的真实落库数据可见：Go 侧 `withdraw` 与它之后几条重建记录**落在同一秒**
    # （`created_at` 相同，相对顺序因此由随机 UUID 的字典序决定），Rust 侧则被
    # `create_fund_record` 的「已有最大 created_at + 1」拉成严格递增——两侧的
    # `recalculateCashChain` 是按录入顺序逐条取前值重算余额的，顺序一变余额就跟着变。
    #
    # 两侧的**算法与字段值逐字相同**，差别只来自这一秒边界撞没撞上（参考实现重放时
    # 原样沿用成交自身的 `created_at`，GORM `autoCreateTime` 只有秒级精度），
    # 因此这是与实现无关的假红，不是行为差异。
    #
    # 这里显式等待 1.2 秒，让支取与重建记录必然落在不同秒。加这段之后连续 5 次运行全部通过。
    # 注：未能把该现象收敛成一条最小服务层断言（两种录入顺序下的余额推演反复对不上实测值），
    # 所以这里只保留护栏本身，不谎称已有单测覆盖。
    # ----------------------------------------------------------------------------------
    Start-Sleep -Milliseconds 1200

    # ---- 2. 日记：删除 2026-03-15 那篇 ----
    Invoke-Api DELETE '/diary/2026-03-15' | Out-Null

    # ---- 3. 分类与标签：各新建一个再各自删除 ----
    # name 走路由参数，中文需要百分号编码（Rust 侧直接传字符串，无编码问题）
    $p2CategoryEsc = [uri]::EscapeDataString($p2Category)
    $p2TagEsc = [uri]::EscapeDataString($p2Tag)
    Invoke-Api POST '/categories' @{ ledgerId = $main; name = $p2Category; transactionType = 'expense'; sortOrder = 0 } | Out-Null
    Invoke-Api DELETE "/categories/$p2CategoryEsc`?type=expense&ledgerId=$main" | Out-Null
    Invoke-Api POST '/tags' @{ ledgerId = $main; name = $p2Tag; categoryTransactionType = "$p2Category`:expense"; sortOrder = 0 } | Out-Null
    Invoke-Api DELETE "/tags/$p2TagEsc`?categoryTransactionType=$([uri]::EscapeDataString("$p2Category`:expense"))&ledgerId=$main" | Out-Null

    # ---- 4. 图表：新建一个自定义图表再删除 ----
    $p2Chart = Invoke-Api POST '/charts' @{
        ledgerId    = $main
        title       = $p2ChartTitle
        granularity = 'month'
        lines       = @(@{
                label           = '阶段二支出'
                transactionType = 'expense'
                includeOutlier  = $false
                conditions      = @(@{ transactionType = 'expense' })
            })
        chartType   = 'line'
    }
    Invoke-Api DELETE "/charts/$($p2Chart.chartId)" | Out-Null

    # ---- P1-2. 图表：新建自定义图表后用 `PATCH /charts` 整体替换 lines ----
    # 必须放在上一条 DELETE 之后（与 seed.rs 同序），`get_max_sort` 才会取到同一个基数。
    # 覆盖点：tbl_billadm_chart.chart_lines 的 JSON 文本逐字一致（对象字段顺序敏感）。
    $p1Chart = Invoke-Api POST '/charts' @{
        ledgerId    = $main
        title       = $p2ChartTitle
        granularity = 'month'
        lines       = @(@{
                label           = 'P1-2 初始曲线'
                transactionType = 'expense'
                includeOutlier  = $false
                conditions      = @()
            })
        chartType   = 'line'
    }
    $p1ChartUpdated = Invoke-Api PATCH '/charts' @{
        chartId     = $p1Chart.chartId
        title       = $p1ChartUpdatedTitle
        granularity = $p1ChartUpdatedGranularity
        lines       = @(
            @{
                label           = 'P1-2 餐饮支出'
                transactionType = 'expense'
                includeOutlier  = $true
                conditions      = @(
                    @{
                        transactionType = 'expense'
                        category        = '餐饮美食'
                        tags            = @('三餐', '外卖')
                        tagPolicy       = 'all'
                        tagNot          = $false
                        description     = ''
                    },
                    @{
                        transactionType = 'expense'
                        category        = ''
                        tags            = @('数码')
                        tagPolicy       = 'any'
                        tagNot          = $true
                        description     = '耳机'
                    }
                )
            },
            @{
                label           = 'P1-2 无分类收入'
                transactionType = 'income'
                includeOutlier  = $false
                conditions      = @()
            }
        )
        chartType   = $p1ChartUpdatedChartType
        sortOrder   = $p1ChartUpdatedSortOrder
    }
    if ($p1ChartUpdated.isPreset) { throw 'P1-2：更新自定义图表后 isPreset 必须仍为 false' }
    if ($p1ChartUpdated.sortOrder -ne $p1ChartUpdatedSortOrder) {
        throw "P1-2：更新后 sortOrder 应为 $p1ChartUpdatedSortOrder，实际 $($p1ChartUpdated.sortOrder)"
    }
    Write-Host "[go-driver] 图表 P1-2：新建 $($p1Chart.chartId) → PATCH 覆盖 lines/粒度/排序号 OK"

    # ---- 5. 模板：新建一个模板再删除 ----
    $p2TemplateId = Invoke-Api POST '/templates' @{
        ledger_id        = $main
        template_name    = $p2TemplateName
        transaction_type = 'expense'
        category         = '餐饮美食'
        tags             = @('三餐')
        flags            = ''
        description      = $p2TemplateName
        sort_order       = 0
    }
    Invoke-Api DELETE "/templates/$p2TemplateId" | Out-Null

    # ---- 6a. 股票：编辑「建仓委托」第二笔成交的成交价（1701.50 → 1702.00）----
    # 建仓委托已归档到第一轮，取其第二笔成交明细的 id（`update_trade_fill` 收的是成交明细 id）
    $tradesAfterOpen = Invoke-Api GET "/stock/trades?ledger_id=$main&stock_code=600519"
    $openingRoundId = ($tradesAfterOpen | Where-Object { $_.tradeType -eq 'open' -and $_.orderSeq -eq 1 } |
        Sort-Object tradeTime | Select-Object -First 1).roundId
    $secondFill = $tradesAfterOpen | Where-Object { $_.roundId -eq $openingRoundId -and $_.orderSeq -eq 2 } |
        Sort-Object tradeTime | Select-Object -First 1
    Invoke-Api PUT "/stock/trades/$($secondFill.id)" @{
        ledger_id  = $main
        price      = 1702.00
        lots       = 1
        trade_time = 1767657600
    } | Out-Null
    Write-Host "[go-driver] 编辑建仓第 2 笔成交 $($secondFill.id)（1701.50 → 1702.00）"

    # ---- 6b. 股票：编辑已归档轮次的标签与复盘（第一轮来自清仓）----
    # 历史集合由列表接口懒补齐，详情接口自身不补齐，因此必须先查一次列表
    Invoke-Api GET "/stock/history?ledger_id=$main" | Out-Null
    $detail = Invoke-Api GET "/stock/history/detail?ledger_id=$main&stock_code=600519"
    $roundId = $detail.rounds[0].id
    Invoke-Api PUT "/stock/history/rounds/$roundId/tag" @{ ledger_id = $main; tag = '尾盘' } | Out-Null
    Invoke-Api PUT "/stock/history/rounds/$roundId/review" @{ ledger_id = $main; review = '阶段二：轮次复盘已更新' } | Out-Null
    Write-Host "[go-driver] 轮次 $roundId 标签→尾盘、复盘已更新"

    # ---- 6c. 股票：重新建仓 1 手 @16.80，编辑持仓复盘后删除该委托（持仓回到 0）----
    $reopenTrades = Invoke-Api POST '/stock/trades' @{
        ledger_id  = $main
        stock_code = '600519'
        stock_name = '贵州茅台'
        trade_type = 'open'
        trade_time = 1775779200
        remark     = '二次建仓'
        fills      = @(@{ price = 16.80; lots = 1 })
    }
    $reopenOrderId = $reopenTrades[0].orderId
    Invoke-Api PUT '/stock/positions/600519/review' @{ ledger_id = $main; review = '阶段二：持仓复盘已更新' } | Out-Null
    Invoke-Api DELETE "/stock/trade-orders/$reopenOrderId`?ledger_id=$main" | Out-Null
    Write-Host "[go-driver] 二次建仓 $reopenOrderId → 编辑持仓复盘 → 删除委托"

    # ---- 6d. 股票：追加本金 50 万（指定发生日期，避免依赖当天日期）----
    Invoke-Api POST '/stock/account/principal/add' @{ ledger_id = $main; amount = 50000000; date = '2026-04-20' } | Out-Null

    # ---- 6e. 股票：更新费用设置 ----
    Invoke-Api PUT '/stock/account/fee-settings' @{
        ledger_id         = $main
        commission_rate   = 0.0001
        min_commission    = 500
        stamp_duty_rate   = 0.0005
        transfer_fee_rate = 0.00001
    } | Out-Null

    # ---- 6f. 股票：更新交易标签设置（默认列表 + 波段）----
    $tagSetting = Invoke-Api GET "/stock/tag-settings?ledger_id=$main"
    $newTags = @($tagSetting.tags) + @($p2StockTag)
    Invoke-Api PUT '/stock/tag-settings' @{ ledger_id = $main; tags = $newTags } | Out-Null

    # ==================================================================================
    # P1-3：四个 `sort_order` 写入路径（模板 / 分类 / 标签）
    # 必须与 `xtask/src/seed.rs` 同序同值：分类与标签是**整组重排**，落库后逐行比对。
    # ==================================================================================

    # ---- P1-3a. 模板：PATCH /templates/:id/sort（路径参数是模板 ID）----
    Invoke-Api PATCH "/templates/$templateId/sort" @{
        ledgerId  = $main
        sortOrder = $p1TemplateSortOrder
    } | Out-Null
    Write-Host "[go-driver] 模板：P1-3 排序号 $templateId → $p1TemplateSortOrder"

    # ---- P1-3b. 分类：PATCH /categories/:name/sort（9 个 expense 分类整组重排）----
    # name 走路由参数，中文需要百分号编码（与服务端 `c.Param("name")` 解码后一致）。
    foreach ($name in $p1CategorySortOrder.Keys) {
        $sortOrder = $p1CategorySortOrder[$name]
        Invoke-Api PATCH "/categories/$([uri]::EscapeDataString($name))/sort" @{
            ledgerId        = $main
            name            = $name
            transactionType = 'expense'
            sortOrder       = $sortOrder
        } | Out-Null
    }
    Write-Host "[go-driver] 分类：P1-3 expense 整组重排 $($p1CategorySortOrder.Count) 行"

    # ---- P1-3c. 标签：PATCH /tags/:name/sort（「餐饮美食:expense」下 10 个标签整组重排）----
    $p1TagCategory = '餐饮美食:expense'
    foreach ($name in $p1TagSortOrder.Keys) {
        $sortOrder = $p1TagSortOrder[$name]
        Invoke-Api PATCH "/tags/$([uri]::EscapeDataString($name))/sort" @{
            ledgerId                = $main
            name                    = $name
            categoryTransactionType = $p1TagCategory
            sortOrder               = $sortOrder
        } | Out-Null
    }
    Write-Host "[go-driver] 标签：P1-3 $p1TagCategory 整组重排 $($p1TagSortOrder.Count) 行"

    # ==================================================================================
    # P1-4：关键事件覆盖写（同一 date 二次 upsert）+ 删除
    # 必须与 `xtask/src/seed.rs` 同序同值：
    #   (ledger_id, date) 冲突时只更新 title/content/color/updated_at，保留原 id 与 created_at，
    #   行数不变（是覆盖不是新增）；再删除后该行必须消失。
    # ==================================================================================
    # `2026-04-01` 这一行已存在（阶段 2 的 link_to_key_event 懒创建了空事件），先读回它的 id。
    $p1EventBefore = Invoke-Api GET "/key-events/$p1KeyEventDate`?ledger_id=$main"
    $p1RowsBefore = Get-KeyEventCount -WorkspaceDir $Workspace -LedgerId $main

    Invoke-Api POST '/key-events' @{
        ledger_id = $main
        date      = $p1KeyEventDate
        title     = $p1KeyEventTitle
        content   = $p1KeyEventContent
        color     = $p1KeyEventColor
    } | Out-Null
    Invoke-Api POST '/key-events' @{
        ledger_id = $main
        date      = $p1KeyEventDate
        title     = $p1KeyEventTitleV2
        content   = $p1KeyEventContentV2
        color     = $p1KeyEventColorV2
    } | Out-Null

    $p1EventAfter = Invoke-Api GET "/key-events/$p1KeyEventDate`?ledger_id=$main"
    $p1RowsAfter = Get-KeyEventCount -WorkspaceDir $Workspace -LedgerId $main
    if ($p1RowsAfter -ne $p1RowsBefore) {
        throw "P1-4：同一天二次 upsert 必须是覆盖而不是新增（行数 $p1RowsBefore → $p1RowsAfter）"
    }
    if ($p1EventAfter.id -ne $p1EventBefore.id -or $p1EventAfter.createdAt -ne $p1EventBefore.createdAt) {
        throw 'P1-4：覆盖写必须保留原 id 与 createdAt'
    }
    if ($p1EventAfter.title -ne $p1KeyEventTitleV2 -or $p1EventAfter.content -ne $p1KeyEventContentV2 -or
        $p1EventAfter.color -ne $p1KeyEventColorV2) {
        throw 'P1-4：覆盖写必须更新 title / content / color'
    }
    Write-Host "[go-driver] 关键事件：P1-4 $p1KeyEventDate 二次 upsert 覆盖 OK（行数 $p1RowsAfter 不变，id/createdAt 保留）"

    Invoke-Api DELETE "/key-events/$p1KeyEventDate`?ledger_id=$main" | Out-Null
    $p1RowsDeleted = Get-KeyEventCount -WorkspaceDir $Workspace -LedgerId $main
    if ($p1RowsDeleted -ne ($p1RowsBefore - 1)) {
        throw "P1-4：删除后行数应为 $($p1RowsBefore - 1)，实际 $p1RowsDeleted"
    }
    try {
        Invoke-Api GET "/key-events/$p1KeyEventDate`?ledger_id=$main" | Out-Null
        throw "P1-4：删除 $p1KeyEventDate 后该行必须消失"
    }
    catch {
        if ($_.Exception.Message -like 'P1-4：删除*') { throw }
        # 其余异常即"查不到"——正是期望结果
    }
    Write-Host "[go-driver] 关键事件：P1-4 删除 $p1KeyEventDate OK 行数 $p1RowsAfter → $p1RowsDeleted"

    # ---- P1-5（补充）. 预演：**非法目标**必须 400 且零痕迹 ----
    # 删掉建仓委托会让成交流只剩卖出，预演与真删都会以同一个 400 拒绝（正确行为）。
    # 这条断言"失败的预演同样不落库"：`xtask dump` 的派生数据指纹必须逐字不变。
    # 语义已被 `crates/tr-service/src/stock.rs` 的
    # `preview_delete_order_keeps_state_intact_for_later_replay` 单测锁住，这里只是补一层护栏。
    $p1OpenOrderId = (Invoke-Api GET "/stock/trades?ledger_id=$main&stock_code=600519" |
        Where-Object { $_.tradeType -eq 'open' } | Sort-Object tradeTime | Select-Object -First 1).orderId
    $p1Before = Get-StockSnapshot -WorkspaceDir $Workspace
    $null = Invoke-ApiExpectError -Method 'POST' -Path '/stock/trades/impact' -Body @{
        ledger_id = $main; action = 'delete_order'; order_id = $p1OpenOrderId
    } -ExpectStatus 400 -ExpectMsgLike '卖出数量超过持仓'
    if ((Get-StockSnapshot -WorkspaceDir $Workspace) -ne $p1Before) {
        throw 'P1-5：非法目标的预演以 400 拒绝，但库内容发生了变化（必须零痕迹）'
    }
    Write-Host "[go-driver] 预演：P1-5 非法目标（删建仓委托 $p1OpenOrderId）→ 400「卖出数量超过持仓」且零痕迹 OK"

    # ---- 8. 账本：更新名称与描述（PATCH 只改这两列，createdAt 必须不变）----
    Invoke-Api PATCH "/ledgers/$main" @{
        name        = $p3LedgerName
        description = $p3LedgerDescription
    } | Out-Null

    # ---- 7. 账本：删除「备用账本」，验证级联清理 ----
    Invoke-Api DELETE "/ledgers/$other" | Out-Null

    $finalOverview = Invoke-Api GET "/stock/account/overview?ledger_id=$main"
    Write-Host "[go-driver] 阶段 2 后总览: 本金=$($finalOverview.principal) 可用现金=$($finalOverview.availableCash) 已实现盈亏=$($finalOverview.realizedPnl)" -ForegroundColor Green

    # ==================================================================================
    # 阶段 3：减仓（reduce）+ 多轮次归档（必须与 `xtask/src/seed.rs` 的阶段 3 同序同值）
    #
    # 在**独立账本**上运行，避免与阶段 1/2 的持仓行 / 历史集合 / 资金记录互相影响。
    # 手数守恒：第二轮 3+1=4 手买、1+1+2=4 手卖；第三轮与第四轮各 1 手买 / 1 手卖。
    # 所有 trade_time 互不相同（同秒卖单会被重放并成一笔）。
    # ==================================================================================
    Write-Host '[go-driver] ---- 阶段 3：reduce / 多轮次（独立账本）----' -ForegroundColor Cyan
    $p3 = Invoke-Api POST '/ledgers' @{ name = '阶段三账本'; description = '' }
    Invoke-Api POST '/stock/account/principal' @{ ledger_id = $p3; amount = 15000000 } | Out-Null

    # ---- 3-1. 600519：建仓 3 手 → 两次减仓（各 1 手）→ 加仓 1 手 → 清仓 2 手 ----
    Invoke-Api POST '/stock/trades' @{
        ledger_id = $p3; stock_code = '600519'; stock_name = '贵州茅台'; trade_type = 'open'
        trade_time = $p3Open; remark = '第二轮建仓'; fills = @(@{ price = 1980.00; lots = 3 })
    } | Out-Null
    Invoke-Api POST '/stock/trades' @{
        ledger_id = $p3; stock_code = '600519'; stock_name = '贵州茅台'; trade_type = 'reduce'
        trade_time = $p3Reduce1; remark = '第二轮减仓甲'; fills = @(@{ price = 1990.00; lots = 1 })
    } | Out-Null
    Invoke-Api POST '/stock/trades' @{
        ledger_id = $p3; stock_code = '600519'; stock_name = '贵州茅台'; trade_type = 'reduce'
        trade_time = $p3Reduce2; remark = '第二轮减仓乙'; fills = @(@{ price = 1975.00; lots = 1 })
    } | Out-Null
    Invoke-Api POST '/stock/trades' @{
        ledger_id = $p3; stock_code = '600519'; stock_name = '贵州茅台'; trade_type = 'add'
        trade_time = $p3Add; remark = '第二轮加仓'; fills = @(@{ price = 2010.00; lots = 1 })
    } | Out-Null
    Invoke-Api POST '/stock/trades' @{
        ledger_id = $p3; stock_code = '600519'; stock_name = '贵州茅台'; trade_type = 'close'
        trade_time = $p3Close; remark = '第二轮清仓'; tag = '追涨'
        fills     = @(@{ price = 2000.00; lots = 2 })
    } | Out-Null

    # 自检 ①：两笔减仓必须是独立的两笔、各 1 手
    $p3Trades = Invoke-Api GET "/stock/trades?ledger_id=$p3&stock_code=600519"
    $p3Reduces = @($p3Trades | Where-Object { $_.tradeType -eq 'reduce' })
    if ($p3Reduces.Count -ne 2 -or ($p3Reduces | Where-Object { $_.lots -ne 1 })) {
        throw "阶段3自检失败：减仓应为两笔各 1 手，实际 $($p3Reduces | ForEach-Object { $_.lots })"
    }

    Invoke-Api GET "/stock/history?ledger_id=$p3" | Out-Null
    $p3Detail = Invoke-Api GET "/stock/history/detail?ledger_id=$p3&stock_code=600519"
    $p3Round1 = ($p3Detail.rounds | Where-Object { $_.roundNo -eq 1 } | Select-Object -First 1).id
    Invoke-Api PUT "/stock/history/rounds/$p3Round1/review" @{ ledger_id = $p3; review = '阶段三：第二轮复盘（编辑后必须保留）' } | Out-Null

    # ---- 3-2. 600519：再建仓 1 手 → 清仓 1 手（round_no = 2）----
    $p3R2OpenTrades = Invoke-Api POST '/stock/trades' @{
        ledger_id = $p3; stock_code = '600519'; stock_name = '贵州茅台'; trade_type = 'open'
        trade_time = $p3R2Open; remark = '第二轮建仓'; fills = @(@{ price = 1900.00; lots = 1 })
    }
    $p3R2OpenId = $p3R2OpenTrades[0].id
    Invoke-Api POST '/stock/trades' @{
        ledger_id = $p3; stock_code = '600519'; stock_name = '贵州茅台'; trade_type = 'close'
        trade_time = $p3R2Close; remark = '第二轮清仓'; fills = @(@{ price = 1950.00; lots = 1 })
    } | Out-Null
    Invoke-Api GET "/stock/history?ledger_id=$p3" | Out-Null
    $p3Detail = Invoke-Api GET "/stock/history/detail?ledger_id=$p3&stock_code=600519"
    if ($p3Detail.rounds.Count -ne 2) { throw "600519 此时应有 2 轮，实际 $($p3Detail.rounds.Count)" }
    $p3Round2 = ($p3Detail.rounds | Where-Object { $_.roundNo -eq 2 } | Select-Object -First 1).id
    Invoke-Api PUT "/stock/history/rounds/$p3Round2/tag" @{ ledger_id = $p3; tag = '蓄力' } | Out-Null
    Invoke-Api PUT "/stock/history/rounds/$p3Round2/review" @{ ledger_id = $p3; review = '阶段三：第三轮复盘' } | Out-Null

    # ---- 3-3a. 预演（不落库）：删除**合法目标**的委托（删减仓甲，删完仍持有持仓）----
    # 非法目标（删建仓委托 → 只剩卖出）会被预演自己以 400 拒绝，那是正确行为，
    # 因此这里用合法目标覆盖 `delete_order` 分支。
    $p3ReduceOrder = ($p3Trades | Where-Object { $_.tradeType -eq 'reduce' } |
        Sort-Object tradeTime | Select-Object -First 1).orderId
    $p3Before = Get-StockSnapshot -WorkspaceDir $Workspace
    $p3PreviewDelete = Invoke-Api POST '/stock/trades/impact' @{
        ledger_id = $p3; action = 'delete_order'; order_id = $p3ReduceOrder
    }
    if ((Get-StockSnapshot -WorkspaceDir $Workspace) -ne $p3Before) {
        throw '预演 delete 不应落库，但库内容发生了变化'
    }
    Write-Host "[go-driver] 预演 delete（删除减仓甲委托）不落库 OK 预演后持仓=$($p3PreviewDelete.positionAfter)"

    # ---- 3-3b. 预演（不落库）：第 2 轮建仓 1 手→3 手会让该轮不再成立 ----
    $p3Before = Get-StockSnapshot -WorkspaceDir $Workspace
    $p3PreviewEdit = Invoke-Api POST '/stock/trades/impact' @{
        ledger_id = $p3; action = 'update_trade'; trade_id = $p3R2OpenId
        price = 1900.00; lots = 3; trade_time = $p3R2Open
    }
    if ((Get-StockSnapshot -WorkspaceDir $Workspace) -ne $p3Before) {
        throw '预演 edit 不应落库，但库内容发生了变化'
    }
    $p3RemovedEdit = @($p3PreviewEdit.removedRounds | ForEach-Object { $_.roundNo })
    if ($p3RemovedEdit.Count -ne 1 -or $p3RemovedEdit[0] -ne 2) {
        throw "预演 edit 应只失效第 2 轮，实际 $($p3RemovedEdit -join ',')"
    }

    # ---- 3-4. 真正编辑第 2 轮建仓的**价格**（1900→1920）并触发重放 ----
    Invoke-Api PUT "/stock/trades/$p3R2OpenId" @{
        ledger_id = $p3; price = 1920.00; lots = 1; trade_time = $p3R2Open
    } | Out-Null
    Invoke-Api GET "/stock/history?ledger_id=$p3" | Out-Null
    $p3Detail = Invoke-Api GET "/stock/history/detail?ledger_id=$p3&stock_code=600519"
    $p3Round1After = $p3Detail.rounds | Where-Object { $_.roundNo -eq 1 } | Select-Object -First 1
    $p3Round2After = $p3Detail.rounds | Where-Object { $_.roundNo -eq 2 } | Select-Object -First 1
    if ($p3Detail.rounds.Count -ne 2 -or -not $p3Round1After -or -not $p3Round2After) {
        throw "编辑价格后应仍为 2 轮，实际 $($p3Detail.rounds.Count)"
    }
    if ($p3Round1After.id -ne $p3Round1 -or $p3Round1After.tag -ne '追涨' -or
        [string]::IsNullOrWhiteSpace($p3Round1After.review)) {
        throw '编辑后第 1 轮的 ID/tag/review 必须保留'
    }
    if ($p3Round2After.id -ne $p3Round2 -or $p3Round2After.tag -ne '蓄力') {
        throw '编辑后第 2 轮的 ID/tag 必须保留'
    }
    Write-Host "[go-driver] 600519：两轮各 1 手、编辑价格后轮次元数据保留 OK"

    # ---- 3-5. 第四轮 000001：建仓 1 手 → 清仓 1 手（独立股票）----
    $p3R4 = Invoke-Api POST '/stock/trades' @{
        ledger_id = $p3; stock_code = '000001'; stock_name = '平安银行'; trade_type = 'open'
        trade_time = $p3R4Open; remark = '第四轮建仓'; fills = @(@{ price = 1120.00; lots = 1 })
    }
    $p3R4OpenId = $p3R4[0].id
    Invoke-Api POST '/stock/trades' @{
        ledger_id = $p3; stock_code = '000001'; stock_name = '平安银行'; trade_type = 'close'
        trade_time = $p3R4Close; remark = '第四轮清仓'; tag = '尾盘'
        fills     = @(@{ price = 1180.00; lots = 1 })
    } | Out-Null
    Invoke-Api GET "/stock/history?ledger_id=$p3" | Out-Null
    $p3Detail4 = Invoke-Api GET "/stock/history/detail?ledger_id=$p3&stock_code=000001"
    if ($p3Detail4.rounds.Count -ne 1) { throw "000001 应只有 1 轮，实际 $($p3Detail4.rounds.Count)" }

    # ---- 3-7. 编辑第四轮的成交价（独立股票上的重放，验证其 tag 保留）----
    Invoke-Api PUT "/stock/trades/$p3R4OpenId" @{
        ledger_id = $p3; price = 1130.00; lots = 1; trade_time = $p3R4Open
    } | Out-Null
    Invoke-Api GET "/stock/history?ledger_id=$p3" | Out-Null
    $p3Detail4 = Invoke-Api GET "/stock/history/detail?ledger_id=$p3&stock_code=000001"
    if (($p3Detail4.rounds | Select-Object -First 1).tag -ne '尾盘') {
        throw '000001 的轮次 tag 应保留「尾盘」'
    }
    Write-Host '[go-driver] 000001：清仓归档 + 编辑价格后 tag 保留 OK'

    # ---- 3-8. 自检 ②：每只股票的卖出总量 ≤ 买入总量 ----
    foreach ($code in @('600519', '000001')) {
        $rows = @(Invoke-Api GET "/stock/trades?ledger_id=$p3&stock_code=$code")
        $bought = ($rows | Where-Object { $_.tradeType -in @('open', 'add') } |
            Measure-Object -Property shares -Sum).Sum
        $sold = ($rows | Where-Object { $_.tradeType -in @('reduce', 'close') } |
            Measure-Object -Property shares -Sum).Sum
        if ($sold -gt $bought) {
            throw "阶段3自检失败：$code 卖出 $sold 股 > 买入 $bought 股（手数不守恒）"
        }
    }
    Write-Host '[go-driver] 阶段 3 自检通过（减仓两笔各 1 手 / 每只股票卖出 <= 买入）'
}
finally {
    Stop-Kernel
    Write-Host '[go-driver] 内核已停止' -ForegroundColor Cyan
}

Write-Host "[go-driver] 完成，工作空间: $Workspace" -ForegroundColor Green
