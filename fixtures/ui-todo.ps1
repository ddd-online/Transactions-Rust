# ui-todo.ps1 —— 待办的端到端验收（真实界面 + 数据库断言）。
#
# 一条链把这一页的四层串起来（每层都落库断言，不看"点到了没有"）：
#   1. 建卡片（主题）→ `tbl_billadm_todo_card` 一行；
#   2. 卡片下加事项（含紧急度「中高」/ 重要度「中低」、开始与截止）→ `tbl_billadm_todo_item`
#      的 status=doing、urgency/importance 与两个日期都按界面上的选择落库；
#   3. 展开进度 → 记一条 → `tbl_billadm_todo_progress` 一行（时间 + 正文）；
#      打勾 / 取消打勾 → 同一条的 `done` 在 1 / 0 之间跟着变；
#   4. 四象限图分栏：进行中的事项应当被画出来（判据 = 工具栏下那句说明 + SVG 撑满版心）；
#      象限名是按钮（role=button）：点开 = 该象限的事项列表，紧急度降序 → 重要度降序
#      （每格名字里的条数、列表里的行与顺序都拿库里的 `urgency` / `importance` 当判据）；
#   5. 勾选完成 → status=done 且 completed_at>0，**卡片视图里它消失**、历史里出现
#      （主题名跟着走）→ 历史里的「查看」弹窗能读到那条进度正文；
#   6. 「退回进行中」→ status 回到 doing（误点的退路）；
#   7. 删卡片 → 卡片 / 事项 / 进度记录三张表一起清（不留孤儿行）。
#
# 用法（pwsh 7；需要 release 产物；本仓库不能有实例在跑）：
#   pwsh -File fixtures/ui-todo.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

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
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\tests\ui-todo\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\tests\ui-todo\out' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }
if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$trPaths = Initialize-TrSmokeHome -SmokeHome $SmokeHome -OutDir $OutDir
$smokeHome = $trPaths.SmokeHome
$OutDir = $trPaths.OutDir
Assert-NoRepoInstance -Repo $repo
$ws = [System.IO.Path]::GetFullPath($Workspace)

if (-not $explicitWorkspace) {
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
}
if (-not (Test-Path (Join-Path $ws 'transactions.db'))) {
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[todo] 播种工作空间: $ws" -ForegroundColor Cyan
}

@{ width = 1500; height = 950; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'ui-todo.ps1' } |
    ConvertTo-Json | Set-Content -Path (Join-Path $smokeHome '.transactions.json') -Encoding UTF8

$stamp = Get-Date -Format 'HHmmss'
$cardTitle = "UIA待办$stamp"
$itemTitle = "写完整理稿$stamp"
$progressText = "记了第一条进度 $stamp"
$failures = New-Object System.Collections.Generic.List[string]

function Get-AppWindow {
    param([int]$ProcessId, [int]$TimeoutSec = 60)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $candidate = $UIA::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children,
            (New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $ProcessId)))
        if ($candidate -and (Find-First $candidate '记账')) { return $candidate }
        Start-Sleep -Milliseconds 700
    } while ((Get-Date) -lt $deadline)
    return $null
}

function Find-VisibleButton {
    param($Window, [string]$Name, [switch]$Last, [int]$TimeoutSec = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $buttons = @($Window.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $Name)))) |
            Where-Object {
                $_.Current.ControlType.ProgrammaticName -eq 'ControlType.Button' -and
                (Test-Rect $_.Current.BoundingRectangle) -and -not $_.Current.IsOffscreen
            }
        $button = if ($Last) { $buttons | Select-Object -Last 1 } else { $buttons | Select-Object -First 1 }
        if ($button) { return $button }
        Start-Sleep -Milliseconds 300
    }
    return $null
}

# 弹窗底栏的按钮：类名以 `ui-btn` 开头（别按名字点「关闭」——外壳窗口三键那颗也叫这个）
function Invoke-ModalButton {
    param($Window, [string]$Name, [int]$TimeoutSec = 10)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $button = @($Window.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $Name)))) |
            Where-Object {
                $_.Current.ControlType.ProgrammaticName -eq 'ControlType.Button' -and
                ([string]$_.Current.ClassName).StartsWith('ui-btn') -and
                (Test-Rect $_.Current.BoundingRectangle)
            } | Select-Object -Last 1
        if ($button) {
            Invoke-Element $button | Out-Null
            return $true
        }
        Start-Sleep -Milliseconds 300
    }
    return $false
}

# 弹窗里的下拉触发器（`ui-select__trigger`）：按 X 排——左=紧急度、右=重要度。
# 选完要**读回触发器的名字**确认真的生效（下拉面板里的选项点偏一次很常见，静默失败最难查）。
# 六档（低 / 中低 / 次低 / 次高 / 中高 / 高）**没有搜索框**：面板 260px 装得下全部六个选项，
# 开面板后直接按文案点那一项就行。
function Select-Level {
    param($Window, [ValidateSet('left', 'right')][string]$Which, [string]$Value, [int]$Tries = 3)
    for ($attempt = 1; $attempt -le $Tries; $attempt++) {
        $deadline = (Get-Date).AddSeconds(10)
        $triggers = @()
        while ($triggers.Count -lt 2 -and (Get-Date) -lt $deadline) {
            $triggers = @(Get-Elements $Window) | Where-Object {
                $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button -and
                ([string]$_.Current.ClassName).Contains('ui-select__trigger') -and
                (Test-Rect $_.Current.BoundingRectangle) -and -not $_.Current.IsOffscreen
            } | Sort-Object { $_.Current.BoundingRectangle.X }
            if ($triggers.Count -lt 2) { Start-Sleep -Milliseconds 400 }
        }
        if ($triggers.Count -lt 2) { return $false }
        $trigger = if ($Which -eq 'left') { $triggers[0] } else { $triggers[$triggers.Count - 1] }
        if ($trigger.Current.Name -eq $Value) { return $true }
        # 开面板（InvokePattern 优先，没开成退回真实鼠标）
        $opened = $false
        $pattern = $null
        if ($trigger.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
            try { $pattern.Invoke(); $opened = $true } catch { $opened = $false }
        }
        if (-not $opened) { Click-Element $trigger | Out-Null }
        Start-Sleep -Milliseconds 500
        $option = Wait-Element -Root $Window -Name $Value -TimeoutSec 6
        if (-not $option) {
            # InvokePattern 没开面板时退回真实鼠标再试一次
            Click-Element $trigger | Out-Null
            Start-Sleep -Milliseconds 700
            $option = Wait-Element -Root $Window -Name $Value -TimeoutSec 6
        }
        if ($option) {
            Click-Element $option | Out-Null
            Start-Sleep -Milliseconds 600
        }
        # 面板可能没关掉：关一下再读（Esc）
        [System.Windows.Forms.SendKeys]::SendWait('{ESC}')
        Start-Sleep -Milliseconds 300
        $after = @(Get-Elements $Window) | Where-Object {
            $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button -and
            ([string]$_.Current.ClassName).Contains('ui-select__trigger') -and
            (Test-Rect $_.Current.BoundingRectangle) -and -not $_.Current.IsOffscreen
        } | Sort-Object { $_.Current.BoundingRectangle.X }
        $current = if ($Which -eq 'left') { $after[0] } else { $after[$after.Count - 1] }
        if ($current -and $current.Current.Name -eq $Value) { return $true }
        Write-Host "    下拉($Which) 第 $attempt 次没选中「$Value」（当前 '$($current.Current.Name)'）" -ForegroundColor DarkYellow
    }
    return $false
}

# 分栏页签在 UIA 里是 **TabItem**（不是 Button）：按 Button 找会静默失败，
# 用 SelectionItemPattern 选中（见 AGENTS.md 的同一句提醒）。
function Select-TabItem {
    param($Window, [string]$Name, [int]$TimeoutSec = 10)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $tab = @($Window.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $Name)))) |
            Where-Object {
                $_.Current.ControlType.ProgrammaticName -eq 'ControlType.TabItem' -and
                (Test-Rect $_.Current.BoundingRectangle)
            } | Select-Object -First 1
        if ($tab) {
            $pattern = $null
            if ($tab.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) {
                $pattern.Select()
            } else {
                Click-Element $tab | Out-Null
            }
            return $true
        }
        Start-Sleep -Milliseconds 300
    }
    return $false
}

# 只读导出，直接用共享底座的 Read-Table（断言落在库上）
function Get-TodoRows {
    param([string]$Table)
    return @(Read-Table -Repo $repo -Workspace $ws -Table $Table -OutDir $OutDir)
}

$proc = $null
$window = $null
try {
    $proc = Start-App -SmokeHome $smokeHome -Exe $Exe
    $window = Get-AppWindow -ProcessId $proc.Id
    if (-not $window) { throw '启动后 60 秒内没有拿到应用主窗口' }
    Write-Host "[todo] 主窗口已就绪" -ForegroundColor Cyan

    # ================= 1/7 打开待办页 =================
    Write-Host "`n[todo] 1/7 打开「待办」页"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '待办' -TimeoutSec 20)) '侧栏「待办」可点开'
    Start-Sleep -Seconds 2
    Assert-True ([bool](Wait-Element -Root $window -Name '待办视图' -TimeoutSec 15)) '"待办视图"分栏在工具栏里'

    # ================= 2/7 建卡片 =================
    Write-Host "`n[todo] 2/7 新建卡片「$cardTitle」"
    $newCard = Find-VisibleButton -Window $window -Name '新建卡片'
    Assert-True ([bool]$newCard) '找到「新建卡片」'
    if ($newCard) { Invoke-Element $newCard | Out-Null }
    Assert-True ([bool](Wait-Element -Root $window -Name '卡片主题' -TimeoutSec 10)) '弹窗「新建卡片」已打开'
    Assert-True (Set-InputByPaste -Window $window -Name '如：需求开发' -Text $cardTitle) '填入卡片主题'
    Assert-True (Invoke-ModalButton -Window $window -Name '创建') '点「创建」'
    Start-Sleep -Seconds 2
    $cards = Get-TodoRows -Table 'tbl_billadm_todo_card' | Where-Object { $_.title -eq $cardTitle }
    Assert-True ($cards.Count -eq 1) "库里恰好一张卡片（实际 $($cards.Count) 张）"

    # ================= 3/7 加事项 =================
    Write-Host "`n[todo] 3/7 在卡片下添加事项（紧急「中高」/ 重要「中低」）"
    $addItem = Find-VisibleButton -Window $window -Name '添加事项'
    Assert-True ([bool]$addItem) '卡片上有「添加事项」'
    if ($addItem) { Invoke-Element $addItem | Out-Null }
    Assert-True ([bool](Wait-Element -Root $window -Name '紧急度' -TimeoutSec 10)) '事项弹窗已打开'
    Assert-True (Set-InputByPaste -Window $window -Name '如：AI辅助研发' -Text $itemTitle) '填入事项'
    # 六档的档位文案即选项 label（值是 -5 -3 -1 1 3 5）
    Assert-True (Select-Level -Window $window -Which left -Value '中高') '紧急度选「中高」'
    Assert-True (Select-Level -Window $window -Which right -Value '中低') '重要度选「中低」'
    Assert-True (Invoke-ModalButton -Window $window -Name '保存') '点「保存」'
    Start-Sleep -Seconds 2
    $items = Get-TodoRows -Table 'tbl_billadm_todo_item' | Where-Object { $_.title -eq $itemTitle }
    Assert-True ($items.Count -eq 1) "库里恰好一条事项（实际 $($items.Count) 条）"
    if ($items.Count -eq 1) {
        Assert-True ($items[0].urgency -eq 3) "紧急度落库 3=中高（实际 $($items[0].urgency)）"
        Assert-True ($items[0].importance -eq -3) "重要度落库 -3=中低（实际 $($items[0].importance)）"
        Assert-True ($items[0].status -eq 'doing') "新事项是进行中（实际 '$($items[0].status)'）"
        Assert-True (([string]$items[0].card_id).Length -gt 0) '事项挂在卡片下'
    }

    # ================= 4/7 记一条进度 =================
    Write-Host "`n[todo] 4/7 展开进度并记一条"
    $progressBtn = Find-VisibleButton -Window $window -Name '进度（0）'
    Assert-True ([bool]$progressBtn) '事项行上有「进度（0）」'
    if ($progressBtn) { Invoke-Element $progressBtn | Out-Null }
    Start-Sleep -Milliseconds 600
    Assert-True (Set-InputByPaste -Window $window -Name '写一条进度…' -Text $progressText) '填入进度正文'
    $addProgress = Find-VisibleButton -Window $window -Name '添加'
    Assert-True ([bool]$addProgress) '找到「添加」'
    if ($addProgress) { Invoke-Element $addProgress | Out-Null }
    Start-Sleep -Seconds 2
    $progressRows = Get-TodoRows -Table 'tbl_billadm_todo_progress' | Where-Object { $_.content -eq $progressText }
    Assert-True ($progressRows.Count -eq 1) "库里恰好一条进度记录（实际 $($progressRows.Count) 条）"
    if ($progressRows.Count -eq 1) {
        Assert-True ([int64]$progressRows[0].created_at -gt 0) '进度记录带时间'
        Assert-True ([string]$progressRows[0].item_id -eq [string]$items[0].id) '进度记录挂在刚才那条事项下'
    }

    # 进度打勾 / 取消打勾（界面上是那条进度前面的勾选框）
    $progressCheck = Find-VisibleButton -Window $window -Name '标记这条进度已完成'
    Assert-True ([bool]$progressCheck) '进度条目上有打勾按钮'
    if ($progressCheck) { Invoke-Element $progressCheck | Out-Null }
    Start-Sleep -Seconds 2
    $checked = Get-TodoRows -Table 'tbl_billadm_todo_progress' | Where-Object { $_.content -eq $progressText }
    Assert-True ($checked.Count -eq 1 -and [int]$checked[0].done -eq 1) '打勾后 done 落库 = 1'
    $progressUndo = Find-VisibleButton -Window $window -Name '取消打勾'
    Assert-True ([bool]$progressUndo) '打勾后按钮变成「取消打勾」'
    if ($progressUndo) { Invoke-Element $progressUndo | Out-Null }
    Start-Sleep -Seconds 2
    $unchecked = Get-TodoRows -Table 'tbl_billadm_todo_progress' | Where-Object { $_.content -eq $progressText }
    Assert-True ($unchecked.Count -eq 1 -and [int]$unchecked[0].done -eq 0) '取消打勾后 done 落库 = 0'
    Save-Screenshot (Join-Path $OutDir '01-board.png')

    # ================= 5/7 四象限图 =================
    Write-Host "`n[todo] 5/7 切到「四象限图」"
    Assert-True (Select-TabItem -Window $window -Name '四象限图') '切到「四象限图」分栏'
    Start-Sleep -Seconds 2
    # 判据用工具栏下的那句说明（SVG 里的 <text> 不保证进 UIA 树，别拿象限名当标记）
    Assert-True ([bool](Wait-Like -Root $window -Pattern '横轴紧急度' -TimeoutSec 10)) `
        '四象限图画出来了（说明文案在）'
    # 卡片要**撑满版心**：曾经它只跟着自己的内容高（固定 viewBox 按宽度缩放），窗口越高，
    # 卡片底下空出来的一大截越明显。SVG 的高度是这件事在 UIA 里唯一量得到的判据
    # （`role="group"` + aria-label；`role="img"` 会让子元素在无障碍树里变装饰，象限名就点不到了）。
    $quad = @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants,
            (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, '进行中事项的四象限分布')))) |
        Where-Object { Test-Rect $_.Current.BoundingRectangle } | Select-Object -First 1
    Assert-True ([bool]$quad) '四象限图的 SVG 在 UIA 树里（role=group）'
    if ($quad) {
        $quadHeight = $quad.Current.BoundingRectangle.Height
        $windowHeight = $window.Current.BoundingRectangle.Height
        Assert-True ($quadHeight -gt $windowHeight * 0.7) `
            "四象限图铺满版心（图高 $([int]$quadHeight) / 窗口高 $([int]$windowHeight)）"
    }

    # 象限名是**按钮**：点开 = 这一格里的事项列表（紧急度降序 → 重要度降序）。
    # 判据全落在库上：每格名字里的条数按 `urgency` / `importance` 的正负算出来，分类或计数对不上
    # 就找不到那颗按钮；列表的顺序也是拿库里两行的档位逐对比出来的。
    Write-Host "`n[todo] 5/7·b 点象限名 → 事项列表"
    $doing = @(Get-TodoRows -Table 'tbl_billadm_todo_item') | Where-Object { $_.status -eq 'doing' }
    # 六档（-5 -3 -1 1 3 5）里没有 0：**正负就是象限**（与界面 `Quadrant::of` 同一口径）
    $delegateRows = @($doing | Where-Object { [int]$_.importance -le 0 -and [int]$_.urgency -gt 0 })
    $planRows = @($doing | Where-Object { [int]$_.importance -gt 0 -and [int]$_.urgency -le 0 })
    $delegateButton = Find-VisibleButton -Window $window -Name "授权做 · $($delegateRows.Count) 项进行中"
    Assert-True ([bool]$delegateButton) `
        "「授权做 · $($delegateRows.Count) 项进行中」在 UIA 里是按钮（库里有 $($delegateRows.Count) 条落这格）"
    if ($delegateButton) {
        Click-Element $delegateButton | Out-Null
        $modalTitle = Wait-Like -Root $window -Pattern '授权做 · 紧急但不重要' -TimeoutSec 10
        Assert-True ([bool]$modalTitle) '弹窗标题 = 象限名 + 含义'
        # "N 项进行中 · 按紧急度、重要度降序" 这行只在有事项时才画（空格子走空态）
        if ($delegateRows.Count -gt 0) {
            Assert-True ([bool](Wait-Like -Root $window -Pattern '· 按紧急度、重要度降序' -TimeoutSec 10)) `
                '弹窗写明条数与排序口径'
        }
        if ($modalTitle) {
            # 一行是 `li.todo-quadrant-modal__row`（UIA = ListItem，名字 = 整行的文字）：
            # 按类名取、按 Y 排，就是画面上的先后。
            $levelLabel = @{ -5 = '低'; -3 = '中低'; -1 = '次低'; 1 = '次高'; 3 = '中高'; 5 = '高' }
            $rowEls = @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                    [System.Windows.Automation.Condition]::TrueCondition)) |
                Where-Object {
                    ([string]$_.Current.ClassName).StartsWith('todo-quadrant-modal__row') -and
                    (Test-Rect $_.Current.BoundingRectangle)
                } | Sort-Object { $_.Current.BoundingRectangle.Y }
            Assert-True ($rowEls.Count -eq $delegateRows.Count) `
                "弹窗里列全这一格的事项（库里 $($delegateRows.Count) 条，弹窗里 $($rowEls.Count) 行）"
            $shown = @()
            $index = 0
            foreach ($rowEl in $rowEls) {
                $index++
                $text = [string]$rowEl.Current.Name
                $found = @($delegateRows | Where-Object { $text.Contains([string]$_.title) })
                Assert-True ($found.Count -eq 1) "第 $index 行对得上库里的一条事项（行文字 '$text'）"
                if ($found.Count -ne 1) { continue }
                $item = $found[0]
                Assert-True ($text.Contains("紧急 $($levelLabel[[int]$item.urgency])") -and
                    $text.Contains("重要 $($levelLabel[[int]$item.importance])")) `
                    "第 $index 行写着库里的两档（紧急 $($item.urgency) / 重要 $($item.importance)）"
                $shown += $item
            }
            # 顺序：紧急度降序 → 重要度降序（同档不要求次序，稳定排序保持卡片视图的顺序）
            for ($i = 1; $i -lt $shown.Count; $i++) {
                $prev = $shown[$i - 1]
                $curr = $shown[$i]
                $ordered = ([int]$prev.urgency -gt [int]$curr.urgency) -or
                    ([int]$prev.urgency -eq [int]$curr.urgency -and [int]$prev.importance -ge [int]$curr.importance)
                Assert-True $ordered `
                    "第 $i 行排在后面那行之前（紧急 $($prev.urgency)/重要 $($prev.importance) → 紧急 $($curr.urgency)/重要 $($curr.importance)）"
            }
            Save-Screenshot (Join-Path $OutDir '03-quadrant-modal.png')
        }
        # 换一格之前**必须先关掉弹窗**：遮罩盖着整张图，直接点下一格的按钮会点到遮罩上
        # （干跑时踩到：那条路径会静默关掉弹窗、后面整段连坐变红）。
        Assert-True (Invoke-ModalButton -Window $window -Name '关闭') '按类名点弹窗底栏「关闭」'
        Start-Sleep -Milliseconds 700
        # 库里这格是空的就该给空态（象限归属算错会在这条红）
        $planButton = Wait-Element -Root $window -Name "计划做 · $($planRows.Count) 项进行中" -TimeoutSec 10
        Assert-True ([bool]$planButton) "「计划做 · $($planRows.Count) 项进行中」按钮在（与库一致）"
        if ($planButton) {
            Click-Element $planButton | Out-Null
            Start-Sleep -Milliseconds 700
            if ($planRows.Count -eq 0) {
                Assert-True ([bool](Wait-Like -Root $window -Pattern '还没有进行中的事项' -TimeoutSec 10)) `
                    '空的象限给空态'
            }
            Assert-True (Invoke-ModalButton -Window $window -Name '关闭') '再关一次弹窗'
            Start-Sleep -Milliseconds 600
        }
    }
    Save-Screenshot (Join-Path $OutDir '02-quadrant.png')

    # ================= 6/7 完成 → 历史 =================
    Write-Host "`n[todo] 6/7 勾选完成 → 历史里出现（含主题名与进度弹窗）"
    Select-TabItem -Window $window -Name '待办视图' | Out-Null
    Start-Sleep -Seconds 2
    $check = Find-VisibleButton -Window $window -Name '标记为已完成'
    Assert-True ([bool]$check) '事项行上有勾选按钮'
    if ($check) { Invoke-Element $check | Out-Null }
    Start-Sleep -Seconds 2
    $doneItems = Get-TodoRows -Table 'tbl_billadm_todo_item' | Where-Object { $_.title -eq $itemTitle }
    Assert-True ($doneItems[0].status -eq 'done') "完成后 status=done（实际 '$($doneItems[0].status)'）"
    Assert-True ([int64]$doneItems[0].completed_at -gt 0) '完成后写了完成时刻'

    Assert-True (Invoke-SubFunction -Window $window -Name '历史' -TimeoutSec 15) '切到「历史」子功能'
    Start-Sleep -Seconds 2
    Assert-True ([bool](Wait-Element -Root $window -Name '已完成的事项' -TimeoutSec 15)) '历史页渲染出来了'
    $historyNames = @(Get-Elements $window | ForEach-Object { $_.Current.Name })
    Assert-True ($historyNames -contains $itemTitle) "历史里有这条事项（$itemTitle）"
    Assert-True ($historyNames -contains $cardTitle) "历史里带主题名（$cardTitle）"

    $viewButton = Find-VisibleButton -Window $window -Name '查看（1）'
    Assert-True ([bool]$viewButton) '历史行上有「查看（1）」'
    if ($viewButton) { Invoke-Element $viewButton | Out-Null }
    Start-Sleep -Seconds 2
    Assert-True ([bool](Wait-Like -Root $window -Pattern "进度记录 · $itemTitle" -TimeoutSec 8)) `
        '进度记录弹窗已打开（标题带事项名）'
    Assert-True ([bool](Wait-Like -Root $window -Pattern $progressText -TimeoutSec 8)) `
        '进度弹窗里读到了那条进度正文'
    Save-Screenshot (Join-Path $OutDir '03-history.png')
    Invoke-ModalButton -Window $window -Name '关闭' | Out-Null
    Start-Sleep -Milliseconds 600

    # ================= 7/7 退回 → 删卡片 =================
    Write-Host "`n[todo] 7/7 退回进行中 → 删卡片（三张表一起清）"
    $restore = Find-VisibleButton -Window $window -Name '退回进行中'
    Assert-True ([bool]$restore) '历史行上有「退回进行中」'
    if ($restore) { Invoke-Element $restore | Out-Null }
    Start-Sleep -Seconds 2
    $restored = Get-TodoRows -Table 'tbl_billadm_todo_item' | Where-Object { $_.title -eq $itemTitle }
    Assert-True ($restored[0].status -eq 'doing') "退回后 status=doing（实际 '$($restored[0].status)'）"
    Assert-True ([int64]$restored[0].completed_at -eq 0) '退回后清掉完成时刻'

    Assert-True (Invoke-SubFunction -Window $window -Name '记录' -TimeoutSec 15) '切回「记录」子功能'
    Start-Sleep -Seconds 2
    $deleteCard = Find-VisibleButton -Window $window -Name '删除卡片'
    Assert-True ([bool]$deleteCard) '卡片上有「删除卡片」'
    if ($deleteCard) { Invoke-Element $deleteCard | Out-Null }
    Start-Sleep -Milliseconds 600
    Assert-True (Invoke-ModalButton -Window $window -Name '删除') '点弹窗的「删除」'
    Start-Sleep -Seconds 2
    Assert-True ((Get-TodoRows -Table 'tbl_billadm_todo_card' | Where-Object { $_.title -eq $cardTitle }).Count -eq 0) `
        '卡片已删除'
    Assert-True ((Get-TodoRows -Table 'tbl_billadm_todo_item' | Where-Object { $_.title -eq $itemTitle }).Count -eq 0) `
        '事项随卡片一起删除'
    Assert-True ((Get-TodoRows -Table 'tbl_billadm_todo_progress' | Where-Object { $_.content -eq $progressText }).Count -eq 0) `
        '进度记录随卡片一起删除（不留孤儿行）'
}
catch {
    $failures.Add("异常: $_")
    Write-Host "  异常: $_" -ForegroundColor Red
}
finally {
    Stop-TrApp -Process $proc -Failures $failures -OutDir $OutDir
}

Show-TrSummary -Failures $failures -Tag 'todo' -SuccessMessage "[todo] 全部通过：建卡片 → 加事项（紧急「中高」/ 重要「中低」）→ 记进度（打勾 / 取消打勾）→ 四象限图 → 完成进历史（带主题名 + 进度弹窗）→ 退回进行中 → 删卡片（三张表一起清）"
