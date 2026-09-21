# ui-link-event.ps1 —— 「关联事件 / 解除关联」端到端（界面 + 数据库）。
#
# 覆盖点：
#   * 记一笔 → 行内「关联事件」→ 弹窗里用 **DatePicker** 选日期（本项目第一次自动化这个组件：
#     触发器是按钮、日期格子是 `<button class="ui-date-picker__cell">`，可访问名就是"日"数字）→ 确认关联；
#   * 断言 `tbl_billadm_transaction_record.key_event_date` 写成所选日期；
#     该日期若还没有事件，后端会**懒创建**一条空事件 → 一并断言；
#   * 再点「修改关联」→「解除关联」→ 断言 `key_event_date` 清空；
#   * 关联交易卡上的「删除交易」气泡**向上弹**（锁住
#     `.key-event-linked__card .ui-popconfirm__panel` 这条容易被顺手删掉的覆盖规则）。
#
# 为什么需要它：关联/解除此前只覆盖了**落库**（`link_to_key_event` / `unlink`），
# 界面这条路径（含日期选择器、`修改关联` 这个按钮名切换）此前没人走过。
#
# 它同时锁住一个**真实缺陷**：`.ui-modal__content` 原来带 `overflow: hidden`（只为圆角），
# 于是弹窗里 DatePicker 的下拉面板被裁掉 —— 本弹窗只有一个表单项，日历被裁到只剩标题和星期行，
# 日期格子**看不见也点不动**（下拉面板是绝对定位子元素，弹窗的 `overflow` 会把它裁掉）。已改 `overflow: visible`。
# 两个写脚本时踩到的坑也留在这里当范例：
#   * 触发器**不能按占位符找**：`link_date` 默认今天，有值时它的可访问名就是那个日期；
#   * 必须选一个**不是今天**的日子，否则"选择器有没有生效"根本看不出来（第一版就这样，全绿但没测到）；
#   * UIA 里没渲染出来的元素会报 ±∞ 的"空矩形"、却仍 `IsOffscreen=False`，一定要显式排除非有限值。
#
# 用法（pwsh 7；需要 release 产物；本仓库不能有实例在跑）：
#   pwsh -File fixtures/ui-link-event.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]
# 排查"下拉面板到底画没画出来"时设 `$env:TR_LINK_SHOT=1`：会在点开面板后落一张全屏 PNG
# （`<OutDir>\panel-open.png`）——UIA 看不出裁剪，只有截图能。

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
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\link-smoke\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\link-smoke' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$trPaths = Initialize-TrSmokeHome -SmokeHome $SmokeHome -OutDir $OutDir
$smokeHome = $trPaths.SmokeHome
$OutDir = $trPaths.OutDir
Assert-NoRepoInstance -Repo $repo

$ws = [System.IO.Path]::GetFullPath($Workspace)

# 公共鼠标 P/Invoke 类（TrLink）已统一到 fixtures/lib/TrUia.ps1 的 TrUia（调用点是 [TrUia]::…）

$failures = New-Object System.Collections.Generic.List[string]

# 打印矩形：**不能直接 [int] 转换**，空矩形里是 ±∞，会抛
# `无法将值 "∞" 转换为类型 "System.Int32"`（诊断代码自己崩掉是最气人的）。
function Format-Rect { param($Rect)
    if (-not (Test-Rect $Rect)) { return '(空矩形)' }
    return ("({0},{1},{2},{3})" -f [int]$Rect.X, [int]$Rect.Y, [int]$Rect.Width, [int]$Rect.Height)
}

function Select-Date { param($Window, [int]$Day)
    # 触发器**不能按占位符找**：`link_date` 默认就是今天，有值时可访问名会变成那个日期；
    # 按 ClassName `ui-date-picker__trigger` 找才稳。
    $deadline = (Get-Date).AddSeconds(10)
    $trigger = $null
    do {
        $trigger = Find-DateTrigger -Window $Window
        if (-not $trigger) { Start-Sleep -Milliseconds 400 }
    } while (-not $trigger -and (Get-Date) -lt $deadline)
    if (-not $trigger) {
        Write-Host '    找不到日期选择器触发器（class=ui-date-picker__trigger）；以下是诊断：' -ForegroundColor DarkYellow
        foreach ($element in @(Get-Elements $Window)) {
            $cls = $element.Current.ClassName
            if ($cls -notlike '*date-picker*' -and $cls -notlike '*modal*' -and $cls -notlike '*calendar*') { continue }
            $rect = $element.Current.BoundingRectangle
            Write-Host ("      {0} name='{1}' class='{2}' offscreen={3} rect={4}" -f `
                $element.Current.ControlType.ProgrammaticName, $element.Current.Name, $cls,
                $element.Current.IsOffscreen, (Format-Rect $element.Current.BoundingRectangle))
        }
        return ''
    }
    $before = $trigger.Current.Name
    Invoke-Element $trigger | Out-Null
    # 排查"面板画没画出来"要看截图（UIA 看不出裁剪）：设 TR_LINK_SHOT=1 会落一张全屏 PNG。
    if ($env:TR_LINK_SHOT) { Start-Sleep -Milliseconds 600; Save-Screenshot (Join-Path $OutDir 'panel-open.png') }
    # 面板是 `Show when=open` 渲染的：格子要等它挂上 UIA 树并**真正排版**出来，
    # 所以这里轮询等"有有限矩形的第 Day 天"，而不是固定 sleep 后取第一个碰到的同名元素。
    $cell = $null
    $openDeadline = (Get-Date).AddSeconds(8)
    do {
        $cell = Find-DateCell -Window $Window -Day $Day
        if (-not $cell) { Start-Sleep -Milliseconds 300 }
    } while (-not $cell -and (Get-Date) -lt $openDeadline)
    if (-not $cell) {
        $cells = @(Get-Elements $Window | Where-Object { $_.Current.ClassName -like '*ui-date-picker__cell*' } |
            ForEach-Object { "$($_.Current.Name)[$($_.Current.ClassName)]$(if (Test-Rect $_.Current.BoundingRectangle) { '' } else { '(空矩形)' })" })
        Write-Host "    没找到已渲染的第 $Day 天；日历里的格子：$($cells -join ' ')" -ForegroundColor DarkYellow
        return ''
    }
    Click-Element $cell | Out-Null
    Start-Sleep -Milliseconds 800
    $after = Find-DateTrigger -Window $Window
    $value = if ($after) { $after.Current.Name } else { '' }
    Write-Host "    日期选择器：'$before' → '$value'（点了第 $Day 天）" -ForegroundColor DarkGray
    return $value
}

# ---- 播种（默认每次重播）----
if (-not $explicitWorkspace) {
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
}
if (-not (Test-Path (Join-Path $ws 'transactions.db'))) {
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[link] 播种工作空间: $ws" -ForegroundColor Cyan
}

@{ width = 1500; height = 950; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'ui-link-event.ps1' } |
    ConvertTo-Json | Set-Content -Path (Join-Path $smokeHome '.transactions.json') -Encoding UTF8

$stamp = Get-Date -Format 'HHmmss'
$description = "UIA关联$stamp"
# 选一个"当月里已过去的日子"，避免选到未来日期带来的额外语义
$today = Get-Date
# **要选一个不是今天的日子**：`link_date` 默认就是今天，选今天的话"选择器有没有生效"根本看不出来
# （第一版就是这样，跳过了选择器、断言还全绿）。
$linkDay = if ($today.Day -eq 1) { 2 } else { 1 }
$linkDate = $today.ToString('yyyy-MM-') + $linkDay.ToString('00')

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
    Write-Host "[link] UIA 可读元素 $((Get-Elements $window).Count) 个" -ForegroundColor Cyan

    # ================= 1/3 记一笔 =================
    Write-Host "`n[link] 1/4 记一笔 11.11（$description）"
    Assert-True (Add-Record -Window $window -Description $description -Amount '11.11') '记一笔'
    $records = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_transaction_record' -OutDir $OutDir)
    $rows = @($records | Where-Object { $_.description -eq $description })
    Assert-True ($rows.Count -eq 1) "库里出现这笔记录（$description）"
    if ($rows.Count -lt 1) { throw '没有源记录，无法继续' }
    Assert-True ([string]::IsNullOrEmpty($rows[0].key_event_date)) '初始没有关联（key_event_date 为空）'

    # ================= 2/3 关联事件 =================
    Write-Host "`n[link] 2/4 关联事件：选 $linkDate（当月第 $linkDay 天）"
    $linkButton = Find-RowButton -Window $window -RowText $description -ButtonName '关联事件'
    Assert-True ([bool]$linkButton) '找到「关联事件」按钮'
    if (-not $linkButton) { throw '找不到关联按钮' }
    Click-Element $linkButton | Out-Null
    Start-Sleep -Seconds 2
    Assert-True ([bool](Wait-Element -Root $window -Name '关联事件' -TimeoutSec 10)) '弹窗「关联事件」已打开'
    # **断言选择器自己的值**（触发器可访问名 = 当前值），别只看库里最终写进去什么：
    # 第一版跳过了这一步，结果"关联日期"仍是默认的今天，断言却是绿的。
    $pickedDate = Select-Date -Window $window -Day $linkDay
    Assert-True ($pickedDate -eq $linkDate) "日期选择器变成 $linkDate（实际 '$pickedDate'）"
    $okButton = Wait-Element -Root $window -Name '确认关联' -TimeoutSec 10
    Assert-True ([bool]$okButton) '找到「确认关联」'
    Invoke-Element $okButton | Out-Null
    Start-Sleep -Seconds 4

    $linked = @((Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_transaction_record' -OutDir $OutDir) | Where-Object { $_.description -eq $description })
    Assert-True ($linked.Count -eq 1) '记录还在'
    if ($linked.Count -eq 1) {
        Assert-True ($linked[0].key_event_date -eq $linkDate) "关联日期写进库（期望 $linkDate，实际 $($linked[0].key_event_date)）"
    }
    # 关联到还没有事件的日期会**懒创建**一条空事件
    $events = @((Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_key_event' -OutDir $OutDir) | Where-Object { $_.date -eq $linkDate -and $_.ledger_id -eq $linked[0].ledger_id })
    Assert-True ($events.Count -ge 1) "该日期在库里有一条事件（懒创建，$linkDate）"

    # ================= 3/4 关联交易卡上的删除气泡**要向上弹** =================
    # 这条锁的是一个很容易被顺手删掉的 CSS 规则：`.key-event-linked__card .ui-popconfirm__panel`
    # 在「删除事件」改成弹窗、清掉事件卡那套气泡定位时**差点被一起删掉**
    # （事件卡与关联交易卡相邻，很容易看成一整套规则）。
    # 它的触发器在卡片**右下角**，默认向下弹会顶出卡片、被关联栏的滚动容器裁掉。
    # 别的护栏覆盖不到这条路：ui-crud 删的是图表（另一个页面）。
    # ⚠ 必须在**解除关联之前**做：解除后这张卡就从右栏消失了（那里会变成「暂无关联交易」）。
    # ⚠ 而且得先切到**事件页**：刚才那笔的「关联事件」按钮在记账页的行内，
    #   关联交易卡在事件页的**右栏**（第一版没切页，找一个不存在的按钮找了 15 秒）。
    Write-Host "`n[link] 3/4 事件页右栏：关联交易卡的「删除交易」气泡应向上弹（不被裁掉）"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '事件' -TimeoutSec 20)) '切到「事件」页'
    Start-Sleep -Seconds 3
    # 事件卡上的日期文案走 `format::short_date`（`2026-09-01` → `9-1`）。
    # ⚠ 用 `Find-Like`（按子串找元素）而**不是** `Wait-Element`（按可访问名精确匹配）：
    #   卡片自己没有 aria-label，它的可访问名是浏览器把子节点拼出来的，
    #   直接按 `9-1` 精确匹配会找不到（第一版就这样，白等 15 秒）。
    $shortDate = "$([int]$linkDate.Substring(5, 2))-$([int]$linkDate.Substring(8, 2))"
    $eventCard = $null
    $deadline = (Get-Date).AddSeconds(15)
    do {
        $eventCard = Find-Like -Root $window -Pattern $shortDate
        if (-not $eventCard) { Start-Sleep -Milliseconds 500 }
    } while (-not $eventCard -and (Get-Date) -lt $deadline)
    Assert-True ([bool]$eventCard) "找到 $shortDate 那张事件卡"
    if (-not $eventCard) {
        $names = @(Get-Elements $window | ForEach-Object { $_.Current.Name } | Where-Object { $_ } | Select-Object -First 25)
        Write-Host "    事件页上的元素名（前 25 个）: $($names -join ' | ')" -ForegroundColor DarkYellow
    }
    if ($eventCard) {
        Invoke-Element $eventCard | Out-Null
        Start-Sleep -Seconds 3
    }
    $tradeDelete = $null
    $deadline = (Get-Date).AddSeconds(15)
    do {
        $tradeDelete = Find-RowButton -Window $window -RowText $description -ButtonName '删除交易'
        if (-not $tradeDelete) { Start-Sleep -Milliseconds 500 }
    } while (-not $tradeDelete -and (Get-Date) -lt $deadline)
    Assert-True ([bool]$tradeDelete) '找到关联交易卡上的「删除交易」按钮'
    if ($tradeDelete) {
        $triggerRect = $tradeDelete.Current.BoundingRectangle
        Click-Element $tradeDelete | Out-Null
        Start-Sleep -Milliseconds 1200
        $bubble = Find-First $window '删除这条关联交易？'
        Assert-True ([bool]$bubble) '气泡已弹出（标题「删除这条关联交易？」）'
        if ($bubble) {
            $bubbleRect = $bubble.Current.BoundingRectangle
            $triggerY = $triggerRect.Y + $triggerRect.Height / 2
            $bubbleY = $bubbleRect.Y + $bubbleRect.Height / 2
            Assert-True ($bubbleY -lt $triggerY) "气泡在触发器**上方**（气泡中心 $([int]$bubbleY) < 按钮中心 $([int]$triggerY)）"
            # 只关掉气泡，**不确认删除**（本脚本不验删关联，只验气泡位置）
            $cancel = Find-All $window '取消'
            if ($cancel.Count -gt 0) { Invoke-Element $cancel[$cancel.Count - 1] | Out-Null }
            Start-Sleep -Milliseconds 600
        }
    }

    # ================= 4/4 解除关联（回记账页，按钮在行内）=================
    Write-Host "`n[link] 4/4 解除关联"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '记账' -TimeoutSec 20)) '切回「记账」页'
    Start-Sleep -Seconds 3
    $modifyButton = Find-RowButton -Window $window -RowText $description -ButtonName '修改关联'
    Assert-True ([bool]$modifyButton) '按钮名已变成「修改关联」（说明界面认得这个关联）'
    if ($modifyButton) {
        Click-Element $modifyButton | Out-Null
        Start-Sleep -Seconds 2
        $unlink = Wait-Element -Root $window -Name '解除关联' -TimeoutSec 10
        Assert-True ([bool]$unlink) '弹窗里有「解除关联」'
        if ($unlink) {
            Invoke-Element $unlink | Out-Null
            Start-Sleep -Seconds 4
        }
    }
    $unlinked = @((Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_transaction_record' -OutDir $OutDir) | Where-Object { $_.description -eq $description })
    if ($unlinked.Count -eq 1) {
        Assert-True ([string]::IsNullOrEmpty($unlinked[0].key_event_date)) "解除后 key_event_date 清空（实际 '$($unlinked[0].key_event_date)'）"
    }
}
finally { Stop-TrApp -Process $process -Failures $failures -OutDir $OutDir }

Show-TrSummary -Failures $failures -Tag 'link' -SuccessMessage "[link] 全部通过：关联（含日期选择器与懒创建事件）→ 解除关联"
