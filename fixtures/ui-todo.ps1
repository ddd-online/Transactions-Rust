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
#   7. 删卡片 → 卡片 / 事项 / 进度记录三张表一起清（不留孤儿行）；
#   8. **写进度 / 勾进度不打断页面**：铺一屏放不下的场景、把目标行滚进视口，勾选它的一条进度后断言
#      "行的矩形 Y 没动（页面没被夹回顶部）+ 进度面板仍然展开"。这一条需要 `sqlite3` 在 PATH
#      （只用来铺场景与读表，与 `migrate-workspace.ps1` 同款）。
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
$db = Join-Path $ws 'transactions.db'

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

# 本机 sqlite3（与 migrate-workspace.ps1 同款；护栏只拿它铺场景数据 + 读表）
function Invoke-Sqlite { param([string]$Database, [string]$Sql)
    $file = Join-Path $OutDir 'todo-scenario.sql'
    Set-Content -Path $file -Value $Sql -Encoding UTF8
    & sqlite3 $Database ".read $($file.Replace('\', '/'))"
    if ($LASTEXITCODE -ne 0) { throw "sqlite3 执行失败（exit=$LASTEXITCODE）：$Sql" }
}

# 把某个元素滚进视口：优先 `ScrollItemPattern`（Chromium 的列表项支持），没滚到位再用滚轮兜底。
# `mouse_event` 是共享底座的 public static，直接调即可 —— 不为了这一处去改 `fixtures/lib/TrUia.ps1`
# （那条路径按 `$PathMap` 是"全量档"）。
function Scroll-ElementIntoView {
    param($Window, $Element, [double]$Margin = 12)
    $winRect = $Window.Current.BoundingRectangle
    $pattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern, [ref]$pattern)) {
        try { $pattern.ScrollIntoView() } catch { }
        Start-Sleep -Milliseconds 600
    }
    $rect = $Element.Current.BoundingRectangle
    if ((Test-Rect $rect) -and $rect.Y -gt ($winRect.Y + $Margin) -and
        ($rect.Y + $rect.Height) -lt ($winRect.Y + $winRect.Height - $Margin)) {
        return $true
    }
    [TrUia]::SetForegroundWindow([IntPtr]$Window.Current.NativeWindowHandle) | Out-Null
    [TrUia]::SetCursorPos([int]($winRect.X + $winRect.Width / 2), [int]($winRect.Y + $winRect.Height / 2)) | Out-Null
    for ($i = 0; $i -lt 40; $i++) {
        # 负 delta = 向下滚（WHEEL_DELTA = 120）
        [TrUia]::mouse_event(0x0800, 0, 0, -240, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 120
        $rect = $Element.Current.BoundingRectangle
        if ((Test-Rect $rect) -and $rect.Y -gt ($winRect.Y + $Margin) -and
            ($rect.Y + $rect.Height) -lt ($winRect.Y + $winRect.Height - $Margin)) {
            return $true
        }
    }
    return $false
}

# 按类名 + 名字片段找元素（**不过滤 IsOffscreen**：滚出视口的行也要能拿到矩形，
# 这正是"页面有没有被夹回顶部"的判据）。
function Find-ElementByClassText {
    param($Window, [string]$ClassPart, [string]$TextPart)
    foreach ($element in @($Window.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                [System.Windows.Automation.Condition]::TrueCondition))) {
        if (-not ([string]$element.Current.ClassName).StartsWith($ClassPart)) { continue }
        $name = [string]$element.Current.Name
        if ($name -and $name.Contains($TextPart) -and (Test-Rect $element.Current.BoundingRectangle)) {
            return $element
        }
    }
    return $null
}

$proc = $null
$window = $null
try {
    $proc = Start-App -SmokeHome $smokeHome -Exe $Exe
    $window = Get-AppWindow -ProcessId $proc.Id
    if (-not $window) { throw '启动后 60 秒内没有拿到应用主窗口' }
    Write-Host "[todo] 主窗口已就绪" -ForegroundColor Cyan

    # ================= 1/8 打开待办页 =================
    Write-Host "`n[todo] 1/8 打开「待办」页"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '待办' -TimeoutSec 20)) '侧栏「待办」可点开'
    Start-Sleep -Seconds 2
    Assert-True ([bool](Wait-Element -Root $window -Name '待办视图' -TimeoutSec 15)) '"待办视图"分栏在工具栏里'

    # ================= 2/8 建卡片 =================
    Write-Host "`n[todo] 2/8 新建卡片「$cardTitle」"
    $newCard = Find-VisibleButton -Window $window -Name '新建卡片'
    Assert-True ([bool]$newCard) '找到「新建卡片」'
    if ($newCard) { Invoke-Element $newCard | Out-Null }
    Assert-True ([bool](Wait-Element -Root $window -Name '卡片主题' -TimeoutSec 10)) '弹窗「新建卡片」已打开'
    Assert-True (Set-InputByPaste -Window $window -Name '如：需求开发' -Text $cardTitle) '填入卡片主题'
    Assert-True (Invoke-ModalButton -Window $window -Name '创建') '点「创建」'
    Start-Sleep -Seconds 2
    $cards = Get-TodoRows -Table 'tbl_billadm_todo_card' | Where-Object { $_.title -eq $cardTitle }
    Assert-True ($cards.Count -eq 1) "库里恰好一张卡片（实际 $($cards.Count) 张）"

    # ================= 3/8 加事项 =================
    Write-Host "`n[todo] 3/8 在卡片下添加事项（紧急「中高」/ 重要「中低」）"
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

    # ================= 4/8 记一条进度 =================
    Write-Host "`n[todo] 4/8 展开进度并记一条"
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

    # ================= 5/8 四象限图 =================
    Write-Host "`n[todo] 5/8 切到「四象限图」"
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
    Write-Host "`n[todo] 5/8·b 点象限名 → 事项列表"
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

    # ================= 6/8 完成 → 历史 =================
    Write-Host "`n[todo] 6/8 勾选完成 → 历史里出现（含主题名与进度弹窗）"
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

    # ================= 7/8 退回 → 删卡片 =================
    Write-Host "`n[todo] 7/8 退回进行中 → 删卡片（三张表一起清）"
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

    # ================= 8/8 写进度 / 勾完成不打断页面（滚动位置 + 面板状态） =================
    # 真实缺陷：每次写入成功后都**整表重拉**（`load(())`），而列表区的四态判定把"重拉中"当成 Loading
    # （`tr-draw::section::section_state` 的第一条规则）⇒ 整块列表被换成「正在加载…」⇒ 页面高度塌成
    # 一行 ⇒ 浏览器把滚动位置**夹回顶部且不会恢复**、展开着的进度面板也一起消失，看起来像"页面刷新了"。
    # 判据只能是**滚动位置**（UIA 里量元素的矩形 Y：页面在顶部时第一张卡片会重新出现在视口里）
    # + 面板是否还展开 —— 库里的 `done` / `status` 本来就是对的，只断言它等于没断。
    # 所以先用 sqlite3 铺出"重拉会跨过一帧"的场景，把目标行滚进视口，再动手。
    Write-Host "`n[todo] 8/8 写进度 / 勾完成：页面不滚动、进度面板不关闭"
    $ledgerId = @(Get-TodoRows -Table 'tbl_billadm_ledger' | Sort-Object created_at | Select-Object -First 1)[0].id
    Assert-True (-not [string]::IsNullOrWhiteSpace($ledgerId)) '能读到当前账本 id（铺场景要用）'
    $progressMarker = "滚动场景的进度$stamp"
    $now = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    # **很多卡片**（每张一条事项）+ 最后一张挂一条进度：两个作用叠在一起
    #   ① 一屏放不下 ⇒ 必须滚动；
    #   ② 卡片的读法是"先取卡片、再逐卡取事项"（`list_cards` 的 N+1）⇒ 卡片一多，这次重拉的耗时
    #      就跨过一帧。这一点是这条判据成立的前提：铺十来条事项时，重拉在一个帧内就恢复了，
    #      塌下去的那一帧根本没被画出来，修前也是绿的（实测 20 / 300 / 3000 条事项都如此）。
    $scrollCardCount = 600
    $lastSeq = '{0:d4}' -f $scrollCardCount
    $cardValues = (1..$scrollCardCount | ForEach-Object {
            $seq = '{0:d4}' -f $_
            "('uia-todo-scroll-card-$seq', '$ledgerId', '滚动卡片 $seq', $($now + $_), $($now + $_), $_)"
        }) -join ",`n  "
    $itemValues = (1..$scrollCardCount | ForEach-Object {
            $seq = '{0:d4}' -f $_
            $at = $now + $_
            "('uia-todo-scroll-item-$seq', '$ledgerId', 'uia-todo-scroll-card-$seq', '滚动事项 $seq', '', '', 3, 3, 'doing', 0, $at, $at)"
        }) -join ",`n  "
    Invoke-Sqlite -Database $db -Sql @"
BEGIN IMMEDIATE;
DELETE FROM tbl_billadm_todo_progress WHERE content = '$progressMarker';
DELETE FROM tbl_billadm_todo_item WHERE card_id LIKE 'uia-todo-scroll-card-%';
DELETE FROM tbl_billadm_todo_card WHERE id LIKE 'uia-todo-scroll-card-%';
INSERT INTO tbl_billadm_todo_card (id, ledger_id, title, created_at, updated_at, sort_order)
  VALUES
  $cardValues;
INSERT INTO tbl_billadm_todo_item
  (id, ledger_id, card_id, title, start_date, due_date, urgency, importance, status, completed_at, created_at, updated_at)
  VALUES
  $itemValues;
INSERT INTO tbl_billadm_todo_progress (id, ledger_id, item_id, content, created_at, done)
  VALUES ('uia-todo-scroll-progress', '$ledgerId', 'uia-todo-scroll-item-$lastSeq', '$progressMarker', $($now + 100), 0);
COMMIT;
"@
    # 让页面重新取数：切子功能会重建视图（信号随 owner 释放）⇒ 重新挂 `ListQuery`
    Assert-True (Invoke-SubFunction -Window $window -Name '历史' -TimeoutSec 15) '切到「历史」子功能（强制重取数）'
    Start-Sleep -Seconds 2
    Assert-True (Invoke-SubFunction -Window $window -Name '记录' -TimeoutSec 15) '切回「记录」子功能'
    Start-Sleep -Seconds 3
    Assert-True ([bool](Wait-Element -Root $window -Name "滚动卡片 $lastSeq" -TimeoutSec 15)) `
        "铺的场景出现在界面上（最后一张卡片「滚动卡片 $lastSeq」）"

    # 目标：最后一条事项（也就是挂了进度的那条）——先滚进视口，记下它的矩形
    $targetTitle = "滚动事项 $lastSeq"
    $targetRow = Find-ElementByClassText -Window $window -ClassPart 'todo-item' -TextPart $targetTitle
    Assert-True ([bool]$targetRow) "找到最后一条事项的行（$targetTitle）"
    if ($targetRow) {
        Assert-True (Scroll-ElementIntoView -Window $window -Element $targetRow) '把目标行滚进视口'
        $beforeRect = $targetRow.Current.BoundingRectangle
        Write-Host "    滚动后目标行 Y=$([int]$beforeRect.Y)（窗口底 $([int]($window.Current.BoundingRectangle.Y + $window.Current.BoundingRectangle.Height))）"

        # 展开它的进度面板（点整行 = 展开/收起，与用户的操作同一路径）
        Click-Element $targetRow | Out-Null
        Start-Sleep -Milliseconds 800
        $progressText = Wait-Like -Root $window -Pattern $progressMarker -TimeoutSec 10
        Assert-True ([bool]$progressText) '展开后能看到那条进度'
        $panelInput = Find-ElementByClassText -Window $window -ClassPart 'Edit' -TextPart '写一条进度…'
        if (-not $panelInput) {
            # Chromium 里输入框的 ClassName 不一定是 Edit —— 按名字直接再找一次
            $panelInput = @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                    [System.Windows.Automation.Condition]::TrueCondition)) |
                Where-Object { ([string]$_.Current.Name) -like '*写一条进度*' } | Select-Object -First 1
        }
        Assert-True ([bool]$panelInput) '进度面板是展开的（「写一条进度…」输入框在）'

        # ---- 勾选这条进度为已完成 ----
        # 按**这一行**的包围盒找那颗勾选框：进度行是 `li.todo-progress__row`，
        # 名字里带标记正文的那一行就是它；一屏可能有别的同名按钮，按名字取第一个是碰运气。
        $progressRow = Find-ElementByClassText -Window $window -ClassPart 'todo-progress__row' -TextPart $progressMarker
        Assert-True ([bool]$progressRow) '找到那条进度所在的行'
        # 面板是**向下展开**的：展开之后这一行可能已经贴到窗口底边（勾选框的中心落到窗口外，
        # 点下去等于点到别的窗口上）。先把进度行本身滚进视口，再找勾选框。
        if ($progressRow) { Scroll-ElementIntoView -Window $window -Element $progressRow | Out-Null }
        $checkButton = $null
        if ($progressRow) {
            $progressRect = $progressRow.Current.BoundingRectangle
            $checkButton = @(Find-All $window '标记这条进度已完成') | Where-Object {
                $rect = $_.Current.BoundingRectangle
                (Test-Rect $rect) -and $rect.Y -ge ($progressRect.Y - 4) -and
                ($rect.Y + $rect.Height) -le ($progressRect.Y + $progressRect.Height + 4) -and
                $rect.X -ge $progressRect.X -and ($rect.X + $rect.Width) -le ($progressRect.X + $progressRect.Width)
            } | Select-Object -First 1
        }
        Assert-True ([bool]$checkButton) '进度条目上有打勾按钮'
        # ⚠ 从这个点往后不能再自己滚页面（上面为了点得到勾选框滚过两次）：判据是"页面还在原来那个
        # 位置"，所以基准必须在**所有自己的滚动之后**、动手之前量。
        $firstCard = Find-ElementByClassText -Window $window -ClassPart 'todo-card__title' -TextPart '滚动卡片 0001'
        Assert-True ([bool]$firstCard) '找到第一张卡片（当"页面滚到哪儿了"的基准）'
        $beforeRect = $targetRow.Current.BoundingRectangle
        $winTop = $window.Current.BoundingRectangle.Y
        $firstBefore = if ($firstCard) { $firstCard.Current.BoundingRectangle.Y } else { 0 }
        Write-Host "    动手前：目标行 Y=$([int]$beforeRect.Y)，第一张卡片 Y=$([int]$firstBefore)（窗口顶 $([int]$winTop)）"
        Assert-True ($firstBefore -lt $winTop) '动手前页面已经滚过第一张卡片（否则后面的判据是空转）'
        # 点一下不一定落：勾选框在行首，滚到页面底部时它的矩形刚被重读过（面板是刚展开的），
        # 偶尔会点到行上（行的 `on:click` 只是收起/展开）。这里按"库里 done 变了"重试最多 3 次 ——
        # 断言本身仍落在库上，重试只是别让一次落空把后面两条滚动判据变成空转（没有写入就没有重拉）。
        for ($attempt = 1; $attempt -le 3; $attempt++) {
            if ($checkButton) {
                $checkRect = $checkButton.Current.BoundingRectangle
                Write-Host "    第 $attempt 次点勾选框（X=$([int]$checkRect.X) Y=$([int]$checkRect.Y)）"
                Click-Element $checkButton | Out-Null
            }
            Start-Sleep -Seconds 3
            $afterRows = Get-TodoRows -Table 'tbl_billadm_todo_progress' | Where-Object { $_.content -eq $progressMarker }
            if ($afterRows.Count -eq 1 -and [int]$afterRows[0].done -eq 1) { break }
            # 重新取一次元素（上一点可能点到了行上、或元素被重渲染过）
            $progressRow = Find-ElementByClassText -Window $window -ClassPart 'todo-progress__row' -TextPart $progressMarker
            $checkButton = $null
            if ($progressRow) {
                $progressRect = $progressRow.Current.BoundingRectangle
                $checkButton = @(Find-All $window '标记这条进度已完成') | Where-Object {
                    $rect = $_.Current.BoundingRectangle
                    (Test-Rect $rect) -and $rect.Y -ge ($progressRect.Y - 4) -and
                    ($rect.Y + $rect.Height) -le ($progressRect.Y + $progressRect.Height + 4)
                } | Select-Object -First 1
            }
        }
        Assert-True ($afterRows.Count -eq 1 -and [int]$afterRows[0].done -eq 1) `
            "勾选后 done 落库 = 1（行为本身对；实际 $($afterRows.Count) 条，done=$($afterRows[0].done)）"

        # ---- 判据一：页面还停在原地（第一张卡片仍在视口上方 ⇒ 没被夹回顶部）----
        # 这条比"某一行 Y 不动"稳：页面回顶部时第一张卡片会重新出现在视口里（Y 变回窗口内）。
        $firstAfter = Find-ElementByClassText -Window $window -ClassPart 'todo-card__title' -TextPart '滚动卡片 0001'
        Assert-True ([bool]$firstAfter) '勾选之后第一张卡片还在 UIA 树里'
        $afterRow = Find-ElementByClassText -Window $window -ClassPart 'todo-item' -TextPart $targetTitle
        Assert-True ([bool]$afterRow) '勾选之后目标行还在 UIA 树里'
        if ($firstAfter -and $afterRow) {
            $firstY = $firstAfter.Current.BoundingRectangle.Y
            $afterRect = $afterRow.Current.BoundingRectangle
            Write-Host "    动手后：目标行 Y=$([int]$afterRect.Y)，第一张卡片 Y=$([int]$firstY)"
            Assert-True ($firstY -lt $winTop) `
                "勾选之后页面没有回到顶部（第一张卡片 Y $([int]$firstBefore) → $([int]$firstY)，窗口顶 $([int]$winTop)）"
            # 面板里多了一行"已完成"的样式，行高可能变几像素；只卡住"没被推下去"这一侧
            Assert-True ($afterRect.Y -le ($beforeRect.Y + 24)) `
                "勾选之后目标行没有被推下去（Y $([int]$beforeRect.Y) → $([int]$afterRect.Y)）"
        }
        # ---- 判据二：进度面板仍然展开 ----
        Assert-True ([bool](Wait-Like -Root $window -Pattern '写一条进度…' -TimeoutSec 5)) `
            '勾选之后进度面板仍然展开（没有被重拉关掉）'
        Save-Screenshot (Join-Path $OutDir '05-progress-keep.png')

        # ---- 判据三：**完成事项**也不跳（用户报的另一半：勾完事项整页回到顶部）----
        # 行内那颗勾选按钮按包围盒落在**当前这一行**里找：一屏有几百个同名按钮，按名字取第一个
        # 是碰运气；重拉之后元素是新的一批，所以矩形要重新读（旧元素的矩形已经不在了）。
        $liveRow = Find-ElementByClassText -Window $window -ClassPart 'todo-item' -TextPart $targetTitle
        $completeBtn = $null
        if ($liveRow) {
            $rowRect = $liveRow.Current.BoundingRectangle
            $completeBtn = @(Find-All $window '标记为已完成') | Where-Object {
                $rect = $_.Current.BoundingRectangle
                (Test-Rect $rect) -and $rect.Y -ge ($rowRect.Y - 4) -and
                ($rect.Y + $rect.Height) -le ($rowRect.Y + $rowRect.Height + 4) -and
                $rect.X -ge $rowRect.X -and ($rect.X + $rect.Width) -le ($rowRect.X + $rowRect.Width)
            } | Select-Object -First 1
        }
        Assert-True ([bool]$completeBtn) '目标行里有「标记为已完成」'
        if ($completeBtn) {
            Click-Element $completeBtn | Out-Null
            Start-Sleep -Seconds 3
            $doneRow = Get-TodoRows -Table 'tbl_billadm_todo_item' | Where-Object { $_.title -eq $targetTitle }
            Assert-True ($doneRow.Count -eq 1 -and $doneRow[0].status -eq 'done') '完成后 status=done（行为本身对）'
            # 少了一行 ⇒ 它上面的行只会**往上**走（Δ 负）；"回到顶部"会让 Y 变大，所以卡住上界
            $stillRow = Find-ElementByClassText -Window $window -ClassPart 'todo-item' -TextPart "滚动事项 $('{0:d4}' -f ($scrollCardCount - 1))"
            $stillFirst = Find-ElementByClassText -Window $window -ClassPart 'todo-card__title' -TextPart '滚动卡片 0001'
            Assert-True ([bool]$stillRow -and [bool]$stillFirst) '完成后相邻的事项行与第一张卡片都还在'
            if ($stillRow -and $stillFirst) {
                $afterComplete = $stillRow.Current.BoundingRectangle
                $stillFirstY = $stillFirst.Current.BoundingRectangle.Y
                Write-Host "    完成后：相邻行 Y=$([int]$afterComplete.Y)，第一张卡片 Y=$([int]$stillFirstY)"
                Assert-True ($stillFirstY -lt $winTop) `
                    "完成事项后页面没有回到顶部（第一张卡片 Y $([int]$firstBefore) → $([int]$stillFirstY)，窗口顶 $([int]$winTop)）"
                Assert-True ($afterComplete.Y -le ($beforeRect.Y + 24)) `
                    "完成事项后相邻行没有被推下去（Y $([int]$beforeRect.Y) → $([int]$afterComplete.Y)）"
            }
        }

        # 收尾：把铺的场景删掉（这条步骤自己造的数据自己清）
        Invoke-Sqlite -Database $db -Sql @"
BEGIN IMMEDIATE;
DELETE FROM tbl_billadm_todo_progress WHERE item_id LIKE 'uia-todo-scroll-%';
DELETE FROM tbl_billadm_todo_item WHERE card_id LIKE 'uia-todo-scroll-card-%';
DELETE FROM tbl_billadm_todo_card WHERE id LIKE 'uia-todo-scroll-card-%';
COMMIT;
"@
    }
}
catch {
    $failures.Add("异常: $_")
    Write-Host "  异常: $_" -ForegroundColor Red
}
finally {
    Stop-TrApp -Process $proc -Failures $failures -OutDir $OutDir
}

Show-TrSummary -Failures $failures -Tag 'todo' -SuccessMessage "[todo] 全部通过：建卡片 → 加事项（紧急「中高」/ 重要「中低」）→ 记进度（打勾 / 取消打勾）→ 四象限图 → 完成进历史（带主题名 + 进度弹窗）→ 退回进行中 → 删卡片（三张表一起清）→ 写进度 / 勾完成不打断页面（滚动位置 + 面板状态）"
