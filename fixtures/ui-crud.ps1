# ui-crud.ps1 —— UI 增删改的端到端验收（分类/标签 · 图表 · 关键事件的配色与描述 · 删除）。
#
# 覆盖快速验收里原先靠人工的第 6、8 项（以及第 7 项的删除半边）：
#   * 分类标签页：新增分类 → 库里多一行 → 删除 → 库里少一行；标签同理
#   * 数据分析页：新增图表 → 库里多一行 → 删除（气泡确认）→ 库里少一行
#   * 关键事件页：点色板改颜色 → 库里 color 变；编辑描述写 Markdown → 保存 → 库里 content 变；
#                 删除事件（气泡确认）→ 库里少一行
# 断言全部落在**数据库**上（不看提示文案），所以和实现的措辞解耦。
#
# 用法（pwsh 7；需要 release 产物；本仓库不能有实例在跑）：
#   pwsh -File fixtures/ui-crud.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

param(
    [string]$Exe,
    [string]$SmokeHome,
    [string]$Workspace,
    [string]$OutDir
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $repo 'target\release\transactions.exe' }
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\crud-smoke\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\crud-smoke' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$smokeHome = [System.IO.Path]::GetFullPath($SmokeHome)
$OutDir = [System.IO.Path]::GetFullPath($OutDir)
if ($smokeHome -eq [System.IO.Path]::GetFullPath($env:USERPROFILE)) {
    throw "拒绝把临时 HOME 指到真实用户目录 —— 冒烟会改写你的配置"
}
foreach ($dir in @($smokeHome, (Join-Path $smokeHome 'Desktop'), $OutDir)) {
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
}

$repoPrefix = $repo.TrimEnd('\') + '\'
$blockers = @(Get-Process -Name transactions -ErrorAction SilentlyContinue | Where-Object {
    $path = try { $_.Path } catch { $null }
    $path -and $path.StartsWith($repoPrefix, [System.StringComparison]::OrdinalIgnoreCase)
})
if ($blockers.Count -gt 0) {
    throw "本仓库已有 Transactions 实例在运行（PID $($blockers.Id -join ', ')），单实例插件会顶掉本次启动。"
}

$ws = [System.IO.Path]::GetFullPath($Workspace)

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
$UIA = [System.Windows.Automation.AutomationElement]

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class TrCrud {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  [DllImport("user32.dll")] public static extern IntPtr SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
  public static void Click(int x, int y) {
    SetCursorPos(x, y);
    mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
    mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
  }
}
'@ -Language CSharp -ErrorAction SilentlyContinue

$failures = New-Object System.Collections.Generic.List[string]
function Assert-True {
    param([bool]$Condition, [string]$Message)
    if ($Condition) { Write-Host "  ✓ $Message" -ForegroundColor Green }
    else { Write-Host "  ✗ $Message" -ForegroundColor Red; $failures.Add($Message) }
}

function Get-Elements { param($Root)
    return $Root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
}
function Find-First { param($Root, [string]$Name)
    $all = $Root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $Name)))
    if ($all.Count -eq 0) { return $null }
    return $all[0]
}
function Find-All { param($Root, [string]$Name)
    return @($Root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $Name))))
}
# 行文案常常不是"只有标题"（卡片上还有日期等），所以按**子串**找行
function Find-ElementLike { param($Root, [string]$Pattern)
    foreach ($element in @($Root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition))) {
        $name = $element.Current.Name
        if ($name -and $name.Contains($Pattern)) { return $element }
    }
    return $null
}
function Wait-ElementLike { param($Root, [string]$Pattern, [int]$TimeoutSec = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $element = Find-ElementLike -Root $Root -Pattern $Pattern
        if ($element) { return $element }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    return $null
}
function Wait-Element { param($Root, [string]$Name, [int]$TimeoutSec = 25)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $element = Find-First $Root $Name
        if ($element) { return $element }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    return $null
}
function Wait-Gone { param($Root, [string]$Name, [int]$TimeoutSec = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        if (-not (Find-First $Root $Name)) { return $true }
        Start-Sleep -Milliseconds 400
    }
    return $false
}
function Invoke-Element { param($Element)
    if (-not $Element) { return $false }
    $pattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
        $pattern.Invoke(); return $true
    }
    $rect = $Element.Current.BoundingRectangle
    if ($rect.Width -gt 0) {
        [TrCrud]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
        return $true
    }
    return $false
}
function Set-Value { param($Element, [string]$Value)
    if (-not $Element) { return $false }
    $pattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
        $pattern.SetValue($Value); return $true
    }
    return $false
}
function Click-Element { param($Element)
    if (-not $Element) { return $false }
    $rect = $Element.Current.BoundingRectangle
    if ($rect.Width -le 0) { return $false }
    [TrCrud]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
    return $true
}
# 拿到**主窗口**而不是"属于该进程的第一个窗口"：启动期会先出现初始化窗口（600×560，
# 没有侧栏），随后才切成主窗口。抓到前者的话，后面所有按名字的查找都会落空
# （实测整轮 26 项全红，而截图里主窗口明明好好的）。这里每轮重新查询窗口元素，
# 天然规避"句柄已失效"。
function Get-ReadyWindow { param([int]$ProcessId, [int]$TimeoutSec = 60)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    $last = $null
    while ((Get-Date) -lt $deadline) {
        $candidate = $UIA::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children,
            (New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $ProcessId)))
        if ($candidate) {
            $last = $candidate
            # 侧栏条目出现即说明是主窗口且界面已挂载（'消费记录' 是默认页）
            if (Find-First $candidate '消费记录') { return $candidate }
        }
        Start-Sleep -Milliseconds 500
    }
    return $last
}

function Save-Screenshot { param([string]$Path)
    try {
        $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
        $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        $graphics.CopyFromScreen($bounds.X, $bounds.Y, 0, 0, $bounds.Size)
        $graphics.Dispose()
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
        $bitmap.Dispose()
        Write-Host "  截图: $Path" -ForegroundColor DarkYellow
    }
    catch { Write-Host "  截图失败: $_" -ForegroundColor DarkYellow }
}

# 同一行里的按钮：先按名字找到"行"元素，再取**中心 Y 最近**、且在该行右侧的那个按钮。
# 用"距离最近"而不是"包围盒相交"：行文字与右侧图标按钮的盒子未必严格重叠（实测踩过）。
# 同一行里的按钮：先按名字找到"行"元素 → **把鼠标移上去** → 再取中心 Y 最近的那个按钮。
#
# 为什么必须 hover：分类/标签行的操作区是 `.ct-item-actions { display: none }`，
# 只在 `:hover` / `.is-active` 时才 `display: flex`——这是有意保留的行为（**不要**因此改 CSS）：
# `display: none` 的元素不进 UIA 树，所以不 hover 就"看不见"那个删除按钮——
# 这也是为什么"新建的那一行"能删（它是 active），别的行不行。
function Find-RowButton { param($Window, [string]$RowName, [string]$ButtonName)
    $row = Wait-ElementLike -Root $Window -Pattern $RowName -TimeoutSec 12
    if (-not $row) {
        Write-Host "    找不到行元素「$RowName」" -ForegroundColor DarkYellow
        return $null
    }
    $rowRect = $row.Current.BoundingRectangle
    [TrCrud]::SetCursorPos([int]($rowRect.X + $rowRect.Width / 2), [int]($rowRect.Y + $rowRect.Height / 2)) | Out-Null
    Start-Sleep -Milliseconds 500

    $rowCenter = $rowRect.Y + $rowRect.Height / 2
    $candidates = @()
    foreach ($button in (Find-All $Window $ButtonName)) {
        $rect = $button.Current.BoundingRectangle
        $center = $rect.Y + $rect.Height / 2
        $candidates += [pscustomobject]@{
            Element  = $button
            Distance = [Math]::Abs($center - $rowCenter)
            RightOf  = $rect.X -ge ($rowRect.X - 40)
            VertIn   = [Math]::Abs($center - $rowCenter) -le 60
        }
    }
    $pick = $candidates | Where-Object { $_.VertIn -and $_.RightOf } | Sort-Object Distance | Select-Object -First 1
    if (-not $pick) { $pick = $candidates | Sort-Object Distance | Select-Object -First 1 }
    if (-not $pick -or $pick.Distance -gt 120) {
        Write-Host "    行「$RowName」$($rowRect) 附近没有「$ButtonName」（同名 $($candidates.Count) 个）：" -ForegroundColor DarkYellow
        foreach ($candidate in $candidates) { Write-Host "      rect=$($candidate.Element.Current.BoundingRectangle) 距离=$([int]$candidate.Distance)" }
        return $null
    }
    return $pick.Element
}

# 弹窗里的确认按钮：与页面入口同名时取**最后一个**（弹窗在 DOM 末尾）
function Invoke-ModalButton { param($Window, [string]$Name)
    $all = Find-All $Window $Name
    if ($all.Count -eq 0) { return $false }
    return (Invoke-Element $all[$all.Count - 1])
}

function Read-Table { param([string]$Table)
    $dump = Join-Path $OutDir "dump-$Table.json"
    Push-Location $repo
    & cargo -q xtask dump $ws --table $Table *> $dump
    $exit = $LASTEXITCODE
    Pop-Location
    if ($exit -ne 0) { throw "导出 $Table 失败（exit=$exit）" }
    return @((Get-Content $dump -Raw | ConvertFrom-Json).$Table)
}

# ---- 播种（每次重新播种，保证"新增/删除"是干净的基线）----
if (-not $Workspace -or -not (Test-Path (Join-Path $ws 'transactions.db'))) {
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[crud] 播种工作空间: $ws" -ForegroundColor Cyan
}

@{ width = 1400; height = 900; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'ui-crud.ps1' } |
    ConvertTo-Json | Set-Content -Path (Join-Path $smokeHome '.transactions.json') -Encoding UTF8

$stamp = Get-Date -Format 'HHmmss'
$categoryName = "UIA分类$stamp"
$tagName = "UIA标签$stamp"
$chartTitle = "UIA图表$stamp"
$eventTitle = "UIA事件$stamp"
$eventColor = '#4A8E70'
$markdown = "# 标题`n`n- 第一项`n- 第二项"

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

    $deadline = (Get-Date).AddSeconds(40)
    $window = Get-ReadyWindow -ProcessId $process.Id -TimeoutSec 60
    if (-not $window) { throw '启动后 60 秒内没有拿到主窗口' }
    $deadline = (Get-Date).AddSeconds(40)
    do { Start-Sleep -Seconds 1; $elements = Get-Elements $window } while ($elements.Count -lt 8 -and (Get-Date) -lt $deadline)
    Write-Host "[crud] UIA 可读元素 $($elements.Count) 个" -ForegroundColor Cyan

    $hwnd = [IntPtr]$window.Current.NativeWindowHandle
    [TrCrud]::ShowWindow($hwnd, 9) | Out-Null
    [TrCrud]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 500

    # ================= 1/4 分类与标签：新增 → 删除 =================
    Write-Host "`n[crud] 1/4 分类标签页：新增分类 → 删除"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '分类标签')) '打开「分类标签」页'
    Start-Sleep -Seconds 2

    $before = @(Read-Table 'tbl_billadm_category' | Where-Object { $_.name -eq $categoryName })
    Assert-True ($before.Count -eq 0) "初始没有同名分类（$categoryName）"

    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '添加分类')) '点「添加分类」'
    Start-Sleep -Milliseconds 800
    Assert-True (Set-Value (Wait-Element -Root $window -Name '输入分类名称') $categoryName) '填入分类名称'
    Invoke-ModalButton -Window $window -Name '确认' | Out-Null
    Start-Sleep -Seconds 3

    $after = @(Read-Table 'tbl_billadm_category' | Where-Object { $_.name -eq $categoryName })
    Assert-True ($after.Count -eq 1) "库里出现新分类（$categoryName）"

    $deleteButton = Find-RowButton -Window $window -RowName $categoryName -ButtonName '删除'
    Assert-True ([bool]$deleteButton) '找到该行「删除」按钮'
    if ($deleteButton) {
        Click-Element $deleteButton | Out-Null
        Start-Sleep -Milliseconds 1200
        # 删除分类是**弹窗**（标题「删除分类」，确认按钮「删除」）
        $modalDelete = Find-All $window '删除'
        if ($modalDelete.Count -gt 0) { Invoke-Element $modalDelete[$modalDelete.Count - 1] | Out-Null }
        Start-Sleep -Seconds 3
    }
    $gone = @(Read-Table 'tbl_billadm_category' | Where-Object { $_.name -eq $categoryName })
    Assert-True ($gone.Count -eq 0) "删除后库里不再有该分类（$categoryName）"

    # ---- 标签：先选中一个分类，再新增/删除标签 ----
    Write-Host "`n[crud] 2/4 标签：新增 → 删除"
    $someCategory = (Read-Table 'tbl_billadm_category' | Where-Object { $_.transaction_type -eq 'expense' } |
        Sort-Object sort_order | Select-Object -First 1).name
    $categoryRow = Wait-Element -Root $window -Name $someCategory
    Assert-True ([bool]$categoryRow) "选中分类「$someCategory」（标签挂在分类下）"
    if ($categoryRow) {
        Invoke-Element $categoryRow | Out-Null
        Start-Sleep -Seconds 2
    }
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '添加标签')) '点「添加标签」'
    Start-Sleep -Milliseconds 800
    Assert-True (Set-Value (Wait-Element -Root $window -Name '输入标签名称') $tagName) '填入标签名称'
    Invoke-ModalButton -Window $window -Name '确认' | Out-Null
    Start-Sleep -Seconds 3
    $tagAfter = @(Read-Table 'tbl_billadm_tag' | Where-Object { $_.name -eq $tagName })
    Assert-True ($tagAfter.Count -eq 1) "库里出现新标签（$tagName）"

    $tagDeleteButton = Find-RowButton -Window $window -RowName $tagName -ButtonName '删除'
    Assert-True ([bool]$tagDeleteButton) '找到标签行「删除」按钮'
    if ($tagDeleteButton) {
        Click-Element $tagDeleteButton | Out-Null
        Start-Sleep -Milliseconds 1200
        $modalButtons = Find-All $window '删除'
        if ($modalButtons.Count -gt 0) { Invoke-Element $modalButtons[$modalButtons.Count - 1] | Out-Null }
        Start-Sleep -Seconds 3
    }
    $tagGone = @(Read-Table 'tbl_billadm_tag' | Where-Object { $_.name -eq $tagName })
    Assert-True ($tagGone.Count -eq 0) "删除后库里不再有该标签（$tagName）"

    # ================= 3/4 图表：新增 → 删除（气泡确认）=================
    Write-Host "`n[crud] 3/4 数据分析：新增图表 → 删除"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '数据分析')) '打开「数据分析」页'
    Start-Sleep -Seconds 2
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '新增图表')) '点「新增图表」'
    Start-Sleep -Milliseconds 800
    Assert-True (Set-Value (Wait-Element -Root $window -Name '请输入图表名称') $chartTitle) '填入图表名称'
    Invoke-ModalButton -Window $window -Name '确定' | Out-Null
    Start-Sleep -Seconds 3

    $chartAfter = @(Read-Table 'tbl_billadm_chart' | Where-Object { $_.title -eq $chartTitle })
    Assert-True ($chartAfter.Count -eq 1) "库里出现新图表（$chartTitle）"

    $chartDeleteButton = Find-RowButton -Window $window -RowName $chartTitle -ButtonName '删除图表'
    Assert-True ([bool]$chartDeleteButton) '找到图表「删除图表」按钮'
    if ($chartDeleteButton) {
        Click-Element $chartDeleteButton | Out-Null
        Start-Sleep -Milliseconds 1200
        # 这是 Popconfirm 气泡：确认按钮文案「删除」
        $popButtons = Find-All $window '删除'
        Assert-True ($popButtons.Count -gt 0) '确认气泡已弹出'
        if ($popButtons.Count -gt 0) {
            Invoke-Element $popButtons[$popButtons.Count - 1] | Out-Null
        }
        Start-Sleep -Seconds 3
    }
    $chartGone = @(Read-Table 'tbl_billadm_chart' | Where-Object { $_.title -eq $chartTitle })
    Assert-True ($chartGone.Count -eq 0) "删除后库里不再有该图表（$chartTitle）"

    # ================= 4/4 关键事件：配色 / 描述 / 删除 =================
    Write-Host "`n[crud] 4/4 关键事件：建事件 → 改颜色 → 写描述 → 删除"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '关键事件')) '打开「关键事件」页'
    Start-Sleep -Seconds 2
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '添加事件')) '点「添加事件」'
    Start-Sleep -Milliseconds 1000
    Assert-True (Set-Value (Wait-Element -Root $window -Name '事件名称（可选）') $eventTitle) '填入事件名称'
    Invoke-ModalButton -Window $window -Name '确认' | Out-Null
    Start-Sleep -Seconds 3

    $events = @(Read-Table 'tbl_billadm_key_event' | Where-Object { $_.title -eq $eventTitle })
    Assert-True ($events.Count -eq 1) "库里出现新事件（$eventTitle）"
    $eventDate = if ($events.Count -ge 1) { $events[0].date } else { '' }

    # ---- 改颜色：点色板（aria-label 就是色值），点击即保存 ----
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name $eventColor)) "点色板 $eventColor"
    Start-Sleep -Seconds 3
    $colored = @(Read-Table 'tbl_billadm_key_event' | Where-Object { $_.date -eq $eventDate } | Select-Object -First 1)
    Assert-True (($colored.Count -eq 1) -and ($colored[0].color -eq $eventColor)) "库里颜色已写入（$($colored[0].color)）"

    # ---- 编辑描述：写 Markdown → 保存 ----
    $editButton = Find-First $window '编辑描述'
    if ($editButton) {
        Invoke-Element $editButton | Out-Null
        Start-Sleep -Milliseconds 800
    }
    $textarea = Wait-Element -Root $window -Name '输入描述内容…'
    if (-not $textarea) {
        # 有的实现把 placeholder 当值而不是名字：退回取唯一的 textarea（Edit 多行）
        $edits = @(Get-Elements $window | Where-Object { $_.Current.ClassName -eq 'Edit' })
        $textarea = $edits | Sort-Object { $_.Current.BoundingRectangle.Height } -Descending | Select-Object -First 1
    }
    Assert-True ([bool]$textarea) '找到描述输入框'
    Assert-True (Set-Value $textarea $markdown) '填入 Markdown 描述'
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '保存')) '点「保存」'
    Start-Sleep -Seconds 3
    $described = @(Read-Table 'tbl_billadm_key_event' | Where-Object { $_.date -eq $eventDate } | Select-Object -First 1)
    Assert-True (($described.Count -eq 1) -and ($described[0].content -eq $markdown)) "库里描述已写入（$($described[0].content -replace "`n", '\n')）"

    # ---- Markdown **渲染**：正文里的 `# 标题` / `- 第一项` 应被渲染成标题/列表文本，
    #      也就是说页面上能看到「标题」「第一项」，而**不是**原样的 `# 标题`。
    #      （排版好不好看仍要人眼，这里只验"确实过了 Markdown 渲染器"。）
    $renderedMarkdown = $false
    $deadline = (Get-Date).AddSeconds(15)
    do {
        $texts = @(Get-Elements $window | ForEach-Object { $_.Current.Name } | Where-Object { $_ })
        $renderedMarkdown = (@($texts | Where-Object { $_ -eq '标题' }).Count -gt 0) -and
            (@($texts | Where-Object { $_ -eq '第一项' }).Count -gt 0)
        if (-not $renderedMarkdown) { Start-Sleep -Milliseconds 500 }
    } while (-not $renderedMarkdown -and (Get-Date) -lt $deadline)
    Assert-True $renderedMarkdown '描述被渲染成 Markdown（页面上出现「标题」「第一项」）'
    # 只算**可见的 Text 元素**：编辑用的 textarea（`Edit`）里当然还留着原文，
    # 隐藏节点也不算——它们都不代表"渲染结果"。
    $rawLeftovers = @()
    foreach ($element in @(Get-Elements $window)) {
        $name = $element.Current.Name
        if (-not $name -or -not $name.Contains('# 标题')) { continue }
        $type = $element.Current.ControlType.ProgrammaticName.Replace('ControlType.', '')
        if ($type -eq 'Text' -and -not $element.Current.IsOffscreen) {
            $rawLeftovers += "[$type] $name"
        }
    }
    if ($rawLeftovers.Count -gt 0) { Write-Host "  仍显示原文的元素: $($rawLeftovers -join ' | ')" -ForegroundColor DarkYellow }
    Assert-True ($rawLeftovers.Count -eq 0) '可见区域内没有留下 `# 标题` 这种未渲染的原文'

    # ---- 删除事件（列表卡片上的「删除事件」→ 气泡确认）----
    $eventDeleteButton = Find-RowButton -Window $window -RowName $eventTitle -ButtonName '删除事件'
    Assert-True ([bool]$eventDeleteButton) '找到该事件的「删除事件」按钮'
    if ($eventDeleteButton) {
        Click-Element $eventDeleteButton | Out-Null
        Start-Sleep -Milliseconds 1200
        $confirmButtons = Find-All $window '删除'
        if ($confirmButtons.Count -gt 0) { Invoke-Element $confirmButtons[$confirmButtons.Count - 1] | Out-Null }
        Start-Sleep -Seconds 3
    }
    $eventGone = @(Read-Table 'tbl_billadm_key_event' | Where-Object { $_.date -eq $eventDate })
    Assert-True ($eventGone.Count -eq 0) "删除后库里不再有该事件（$eventDate）"
    # ================= 5/5 消费模板：新建 → 删除（设置页）=================
    Write-Host "`n[crud] 5/5 消费模板：新建 → 删除"
    $templateName = "UIA模板$stamp"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '应用设置')) '打开「应用设置」'
    Start-Sleep -Seconds 2
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '消费模板')) '切到「消费模板」页签'
    Start-Sleep -Seconds 2
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '新建模板')) '点「新建模板」'
    Start-Sleep -Milliseconds 1000
    Assert-True (Set-Value (Wait-Element -Root $window -Name '请输入模板名称') $templateName) '填入模板名称'
    # 分类是必填的（前端与后端都会挡）：点开 Select 再选一个已有分类
    $categoryPicker = Wait-Element -Root $window -Name '请选择分类'
    Assert-True ([bool]$categoryPicker) '找到「请选择分类」下拉'
    if ($categoryPicker) {
        Invoke-Element $categoryPicker | Out-Null
        Start-Sleep -Milliseconds 800
        $optionName = (Read-Table 'tbl_billadm_category' | Where-Object { $_.transaction_type -eq 'expense' } |
            Sort-Object sort_order | Select-Object -First 1).name
        $option = Wait-Element -Root $window -Name $optionName
        Assert-True ([bool]$option) "下拉里选中分类「$optionName」"
        if ($option) { Invoke-Element $option | Out-Null }
        Start-Sleep -Milliseconds 800
    }
    Invoke-ModalButton -Window $window -Name '保存' | Out-Null
    Start-Sleep -Seconds 3
    $templateRows = @(Read-Table 'tbl_billadm_transaction_tpl' | Where-Object { $_.template_name -eq $templateName })
    Assert-True ($templateRows.Count -eq 1) "库里出现新模板（$templateName）"

    if ($templateRows.Count -eq 1) {
        $templateDelete = Find-RowButton -Window $window -RowName $templateName -ButtonName '删除'
        Assert-True ([bool]$templateDelete) '找到该模板行「删除」按钮'
        if ($templateDelete) {
            Click-Element $templateDelete | Out-Null
            Start-Sleep -Milliseconds 1200
            $templateConfirm = Find-All $window '删除'
            if ($templateConfirm.Count -gt 0) { Invoke-Element $templateConfirm[$templateConfirm.Count - 1] | Out-Null }
            Start-Sleep -Seconds 3
        }
        $templateGone = @(Read-Table 'tbl_billadm_transaction_tpl' | Where-Object { $_.template_name -eq $templateName })
        Assert-True ($templateGone.Count -eq 0) "删除后库里不再有该模板（$templateName）"
    }
}
finally {
    if ($failures.Count -gt 0) { Save-Screenshot (Join-Path $OutDir 'failure.png') }
    if ($process -and -not $process.HasExited) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        $process.WaitForExit(5000) | Out-Null
    }
}

Write-Host ''
if ($failures.Count -gt 0) {
    Write-Host "[crud] 失败 $($failures.Count) 项：" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "   - $_" -ForegroundColor Red }
    exit 1
}
Write-Host '[crud] 全部通过：分类/标签/图表的增删、事件配色与描述、事件删除都落到库里' -ForegroundColor Green
