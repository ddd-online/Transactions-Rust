# ui-transactions.ps1 —— 消费记录页的四条关键交互：**编辑（先建后删）**、**保存为模板**、**排序** 与 **筛选/统计条**。
#
# 为什么需要它：
#   * 「编辑」是**先建后删**（不是原地 UPDATE）：`transaction_id` 会换一个、旧记录消失、
#     行数不变。这条语义的落库侧已有测试覆盖，但界面这条路径没人走过；
#   * 「保存为模板」是记账弹窗里的第二条出口（`handleConfirmSaveTemplate` → `template_create`），
#     设置页的"新建模板→删除"已由 `fixtures/ui-crud.ps1` 覆盖，但从**记一笔弹窗**存模板这条没人走过：
#     它的名称走 `template_name` 这个子弹窗输入框，类型/分类/标签/描述全部取当前表单；
#   * 「排序」是页面上唯一的排序入口（`TrSortModal` 的 4 个字段 + 升降序），
#     排序字段要过白名单、方向要强制 asc/desc，改坏了页面顺序会悄悄变形；
#   * 「筛选」是页面上唯一的条件查询入口（悬浮按钮 →「筛选消费记录」弹窗 → 条件列表 → 确认），
#     连同「共 N 条记录」一起验：筛完必须只剩唯一那条。⚠ 那个悬浮按钮**带角标（children），
#     可访问名不是 title**，只能按 class `tr-float-secondary` 找。
#
# 用法（pwsh 7；需要 release 产物；本仓库不能有实例在跑）：
#   pwsh -File fixtures/ui-transactions.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

param(
    [string]$Exe,
    [string]$SmokeHome,
    [string]$Workspace,
    [string]$OutDir
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$explicitWorkspace = -not [string]::IsNullOrWhiteSpace($Workspace)
if (-not $Exe) { $Exe = Join-Path $repo 'target\release\transactions.exe' }
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\tr-smoke\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\tr-smoke' }
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
public class TrTr {
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
function Find-Like { param($Root, [string]$Pattern)
    foreach ($element in @(Get-Elements $Root)) {
        $name = $element.Current.Name
        if ($name -and $name.Contains($Pattern)) { return $element }
    }
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
function Wait-Like { param($Root, [string]$Pattern, [int]$TimeoutSec = 25)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $element = Find-Like -Root $Root -Pattern $Pattern
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
    if ($rect.Width -gt 0) {
        [TrTr]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
        return $true
    }
    return $false
}
function Click-Element { param($Element)
    if (-not $Element) { return $false }
    $rect = $Element.Current.BoundingRectangle
    if ($rect.Width -le 0) { return $false }
    [TrTr]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
    return $true
}
function Set-Value { param($Element, [string]$Value)
    if (-not $Element) { return $false }
    $pattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
        $pattern.SetValue($Value); return $true
    }
    return $false
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

function Get-ReadyWindow { param([int]$ProcessId, [int]$TimeoutSec = 60)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    $last = $null
    while ((Get-Date) -lt $deadline) {
        $candidate = $UIA::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children,
            (New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $ProcessId)))
        if ($candidate) {
            $last = $candidate
            if (Find-First $candidate '消费记录') { return $candidate }
        }
        Start-Sleep -Milliseconds 500
    }
    return $last
}

# 行内按钮：按"名字 + 与该行文案同一水平带"定位（表格里每行都有「编辑记录」）
function Find-RowButton { param($Window, [string]$RowText, [string]$ButtonName)
    $row = Wait-Like -Root $Window -Pattern $RowText -TimeoutSec 15
    if (-not $row) {
        Write-Host "    找不到行元素「$RowText」" -ForegroundColor DarkYellow
        return $null
    }
    $rowRect = $row.Current.BoundingRectangle
    [TrTr]::SetCursorPos([int]($rowRect.X + $rowRect.Width / 2), [int]($rowRect.Y + $rowRect.Height / 2)) | Out-Null
    Start-Sleep -Milliseconds 400
    $rowCenter = $rowRect.Y + $rowRect.Height / 2
    $best = $null; $bestDistance = [double]::MaxValue
    foreach ($button in (Find-All $Window $ButtonName)) {
        if ($button.Current.IsOffscreen) { continue }
        $rect = $button.Current.BoundingRectangle
        if ($rect.Width -le 0) { continue }
        $distance = [Math]::Abs(($rect.Y + $rect.Height / 2) - $rowCenter)
        if ($distance -lt $bestDistance) { $best = $button; $bestDistance = $distance }
    }
    if ($best -and $bestDistance -le 40) { return $best }
    Write-Host "    行「$RowText」附近没有「$ButtonName」（最近距离 $([int]$bestDistance)）" -ForegroundColor DarkYellow
    return $null
}

# 弹窗里的输入框：本项目的 Input 渲染成 class='ui-input__control'，
# 判据要用 **ControlType.Edit**（ClassName 不是 'Edit'）。
function Get-ModalEdits { param($Window, [string]$ModalTitle)
    $title = Wait-Element -Root $Window -Name $ModalTitle -TimeoutSec 15
    if (-not $title) { return @() }
    $titleRect = $title.Current.BoundingRectangle
    $edits = @()
    foreach ($element in @(Get-Elements $Window)) {
        $isEdit = ($element.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit) -or
            ($element.Current.ClassName -eq 'Edit')
        if (-not $isEdit -or $element.Current.IsOffscreen) { continue }
        $rect = $element.Current.BoundingRectangle
        if ($rect.Width -le 0) { continue }
        # 弹窗整体在标题下方
        if ($rect.Y -lt $titleRect.Y) { continue }
        $edits += [pscustomobject]@{ Element = $element; Rect = $rect; Name = $element.Current.Name }
    }
    return $edits
}

# 记一笔：点入口 → 填描述与金额 → 确认
function Add-Record { param($Window, [string]$Description, [string]$Amount)
    if (-not (Invoke-Element (Wait-Element -Root $Window -Name '记一笔'))) { return $false }
    Start-Sleep -Seconds 2
    $okDesc = Set-Value (Wait-Element -Root $Window -Name '描述消费内容') $Description
    $okAmount = Set-Value (Wait-Element -Root $Window -Name '0.00') $Amount
    Start-Sleep -Milliseconds 800
    $confirm = Find-All $Window '确认'
    if ($confirm.Count -gt 0) { Invoke-Element $confirm[$confirm.Count - 1] | Out-Null }
    Start-Sleep -Seconds 3
    return ($okDesc -and $okAmount)
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

# ---- 播种（断言绝对状态，默认每次都重播）----
if (-not $explicitWorkspace) {
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
}
if (-not (Test-Path (Join-Path $ws 'transactions.db'))) {
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[tr] 播种工作空间: $ws" -ForegroundColor Cyan
}

@{ width = 1500; height = 950; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'ui-transactions.ps1' } |
    ConvertTo-Json | Set-Content -Path (Join-Path $smokeHome '.transactions.json') -Encoding UTF8

$stamp = Get-Date -Format 'HHmmss'
$sourceDesc = "UIA编辑源$stamp"
$editedDesc = "UIA编辑后$stamp"
$secondDesc = "UIA排序乙$stamp"
$thirdDesc = "UIA排序丙$stamp"

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
    Write-Host "[tr] UIA 可读元素 $((Get-Elements $window).Count) 个" -ForegroundColor Cyan

    # ================= 1/5 记三笔（准备数据：排序需要多行才有意义）=================
    Write-Host "`n[tr] 1/5 记三笔：77.77 / 12.34 / 45.67"
    Assert-True (Add-Record -Window $window -Description $sourceDesc -Amount '77.77') "记一笔 77.77（$sourceDesc）"
    Assert-True (Add-Record -Window $window -Description "$secondDesc" -Amount '12.34') "记一笔 12.34（$secondDesc）"
    Assert-True (Add-Record -Window $window -Description "$thirdDesc" -Amount '45.67') "记一笔 45.67（$thirdDesc）"

    $records = @(Read-Table 'tbl_billadm_transaction_record')
    $sourceRows = @($records | Where-Object { $_.description -eq $sourceDesc })
    Assert-True ($sourceRows.Count -eq 1) "库里出现这笔记录（$sourceDesc）"
    $oldId = if ($sourceRows.Count -ge 1) { $sourceRows[0].transaction_id } else { '' }
    $countBefore = $records.Count
    if ($sourceRows.Count -ge 1) {
        Assert-True ($sourceRows[0].price -eq 7777) "金额按分存（77.77 → 7777，实际 $($sourceRows[0].price)）"
    }

    # ================= 2/5 编辑：先建后删 =================
    Write-Host "`n[tr] 2/5 编辑这笔记录：金额 77.77 → 88.88、描述改名"
    $editButton = Find-RowButton -Window $window -RowText $sourceDesc -ButtonName '编辑记录'
    Assert-True ([bool]$editButton) '找到该行的「编辑记录」按钮'
    if (-not $editButton) { throw '找不到编辑按钮，后续无法继续' }
    Click-Element $editButton | Out-Null
    Start-Sleep -Seconds 2

    $edits = Get-ModalEdits -Window $window -ModalTitle '记一笔'
    Write-Host "    弹窗输入框：$(($edits | ForEach-Object { "'$($_.Name)'" }) -join ' / ')"
    # 金额：优先名字是 '0.00' 的（预填时名字可能变成数值，退化为"名字像小数"的那个）
    $amountInput = ($edits | Where-Object { $_.Name -eq '0.00' } | Select-Object -First 1)
    if (-not $amountInput) { $amountInput = ($edits | Where-Object { $_.Name -match '^\d+(\.\d+)?$' } | Select-Object -First 1) }
    # 描述：优先占位符名，其次名字等于原描述
    $descInput = ($edits | Where-Object { $_.Name -eq '描述消费内容' } | Select-Object -First 1)
    if (-not $descInput) { $descInput = ($edits | Where-Object { $_.Name -eq $sourceDesc } | Select-Object -First 1) }
    if (-not $descInput) { $descInput = ($edits | Where-Object { $_.Name -and $_.Name -notmatch '^\d+(\.\d+)?$' } | Select-Object -First 1) }
    Assert-True ([bool]$amountInput) '找到金额输入框'
    Assert-True ([bool]$descInput) '找到描述输入框'
    if ($amountInput) { Assert-True (Set-Value $amountInput.Element '88.88') '把金额改成 88.88' }
    if ($descInput) { Assert-True (Set-Value $descInput.Element $editedDesc) '把描述改成新名字' }
    Start-Sleep -Milliseconds 800
    $confirm = Find-All $window '确认'
    if ($confirm.Count -gt 0) { Invoke-Element $confirm[$confirm.Count - 1] | Out-Null }
    Start-Sleep -Seconds 4

    $recordsAfter = @(Read-Table 'tbl_billadm_transaction_record')
    $editedRows = @($recordsAfter | Where-Object { $_.description -eq $editedDesc })
    Assert-True ($editedRows.Count -eq 1) "库里出现改后的记录（$editedDesc）"
    if ($editedRows.Count -ge 1) {
        Assert-True ($editedRows[0].price -eq 8888) "改后的金额是 88.88（实际 $($editedRows[0].price) 分）"
        Assert-True ($editedRows[0].transaction_id -ne $oldId) '编辑换了一个 transaction_id（先建后删）'
    }
    Assert-True (@($recordsAfter | Where-Object { $_.transaction_id -eq $oldId }).Count -eq 0) '原记录已被删除'
    Assert-True (@($recordsAfter | Where-Object { $_.description -eq $sourceDesc }).Count -eq 0) '旧描述不复存在'
    Assert-True ($recordsAfter.Count -eq $countBefore) "总条数不变（编辑 = 先建后删，$countBefore → $($recordsAfter.Count)）"

    # ================= 3/5 保存为模板 =================
    # 从**记一笔弹窗**里存模板（设置页那条已由 ui-crud 覆盖）：名称走子弹窗的输入框，
    # 类型/分类/标签/描述取当前表单 —— 所以这里复用刚编辑过的那笔记录（它类型/分类都在）。
    Write-Host "`n[tr] 3/5 保存为模板：从编辑弹窗存一个模板"
    $templateName = "UIA表单模板$stamp"
    $editAgain = Find-RowButton -Window $window -RowText $editedDesc -ButtonName '编辑记录'
    Assert-True ([bool]$editAgain) '再次找到「编辑记录」按钮'
    if ($editAgain) {
        Click-Element $editAgain | Out-Null
        Start-Sleep -Seconds 2
        # 「保存为模板」在「模板」这一项里，且表单类型/分类为空时是 disabled 的
        $saveTplButton = Wait-Element -Root $window -Name '保存为模板' -TimeoutSec 10
        Assert-True ([bool]$saveTplButton) '弹窗里找到「保存为模板」'
        if ($saveTplButton) {
            Invoke-Element $saveTplButton | Out-Null
            Start-Sleep -Seconds 2
            $tplNameInput = Wait-Element -Root $window -Name '请输入模板名称' -TimeoutSec 10
            Assert-True ([bool]$tplNameInput) '弹出「保存为模板」子弹窗'
            if ($tplNameInput) {
                Assert-True (Set-Value $tplNameInput $templateName) '填入模板名称'
                Start-Sleep -Milliseconds 600
                # 子弹窗的确认按钮是「保存」（记账弹窗自己的是「确认」，不会混）
                $saveTplOk = Wait-Element -Root $window -Name '保存' -TimeoutSec 10
                Assert-True ([bool]$saveTplOk) '找到「保存」'
                if ($saveTplOk) { Invoke-Element $saveTplOk | Out-Null }
                Start-Sleep -Seconds 3
            }
        }
        # 关掉记账弹窗（不保存这次编辑）
        $cancel = @(Find-All $window '取消' | Where-Object { -not $_.Current.IsOffscreen })
        if ($cancel.Count -gt 0) { Invoke-Element $cancel[$cancel.Count - 1] | Out-Null }
        Start-Sleep -Seconds 2
    }

    $templateRows = @(Read-Table 'tbl_billadm_transaction_tpl' | Where-Object { $_.template_name -eq $templateName })
    Assert-True ($templateRows.Count -eq 1) "库里出现从表单存下来的模板（$templateName）"
    if ($templateRows.Count -eq 1 -and $editedRows.Count -ge 1) {
        # 类型/分类/描述必须取当前表单（四个字段直接落到模板）
        Assert-True ($templateRows[0].transaction_type -eq $editedRows[0].transaction_type) `
            "模板类型跟着表单（$($templateRows[0].transaction_type)）"
        Assert-True ($templateRows[0].category -eq $editedRows[0].category) `
            "模板分类跟着表单（$($templateRows[0].category)）"
        Assert-True ($templateRows[0].description -eq $editedDesc) "模板描述取当前表单（$editedDesc）"
    }

    # ================= 4/5 排序 =================
    Write-Host "`n[tr] 4/5 排序：加一条「金额 降序」并应用"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '排序')) '点「排序」'
    Start-Sleep -Seconds 2
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '重置')) '点「重置」（回到日期+降序）'
    Start-Sleep -Seconds 1
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '添加排序条件')) '点「添加排序条件」'
    Start-Sleep -Seconds 1
    # 新加的排序行字段默认是「日期」，点开它的下拉选「金额」
    $fieldPicker = Wait-Like -Root $window -Pattern '日期'
    Assert-True ([bool]$fieldPicker) '找到排序行的字段下拉'
    if ($fieldPicker) {
        Invoke-Element $fieldPicker | Out-Null
        Start-Sleep -Milliseconds 800
        $amountOption = Wait-Element -Root $window -Name '金额'
        Assert-True ([bool]$amountOption) '下拉里有「金额」'
        if ($amountOption) { Invoke-Element $amountOption | Out-Null }
        Start-Sleep -Milliseconds 800
    }
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '应用')) '点「应用」'
    Start-Sleep -Seconds 4

    # 界面上的金额顺序应当是**非递增**（金额降序）。
    # 只取**表格区**里的金额：窗口底部状态栏也有「收入/支出/结余」这类数字，
    # 一起收进来会把序列搅乱（实测拿到 -88.88, 0, 88.88, 0）。
    # 表格区 = 表头「金额」那行的 Y 到最后一个行内「编辑记录」按钮的底边。
    $headerAmount = Wait-Element -Root $window -Name '金额'
    $headerY = if ($headerAmount) { $headerAmount.Current.BoundingRectangle.Y } else { 0 }
    $tableBottom = 0
    foreach ($button in (Find-All $window '编辑记录')) {
        $rect = $button.Current.BoundingRectangle
        $bottom = $rect.Y + $rect.Height
        if ($bottom -gt $tableBottom) { $tableBottom = $bottom }
    }
    $amountsOnScreen = @()
    foreach ($element in @(Get-Elements $window)) {
        $name = $element.Current.Name
        if (-not $name) { continue }
        if ($name -notmatch '^-?¥?\s?(\d{1,3}(,\d{3})*|\d+)\.\d{2}$') { continue }
        $rect = $element.Current.BoundingRectangle
        if ($tableBottom -gt 0 -and ($rect.Y -lt $headerY -or $rect.Y -gt $tableBottom)) { continue }
        $amountsOnScreen += [double]($name -replace '[¥,\s]', '')
    }
    Write-Host "    表格区金额序列: $($amountsOnScreen -join ', ')"
    Assert-True ($amountsOnScreen.Count -ge 3) "表格区至少读到 3 个金额（实际 $($amountsOnScreen.Count)）"
    $descending = $true
    for ($i = 1; $i -lt $amountsOnScreen.Count; $i++) {
        # 支出在界面上是负数，所以按**绝对值**比较降序
        if ([Math]::Abs($amountsOnScreen[$i]) -gt [Math]::Abs($amountsOnScreen[$i - 1]) + 0.001) { $descending = $false }
    }
    Assert-True $descending '界面金额序列按降序排列（排序生效）'
    # 与库对照：今天这笔 88.88 若是最大，则应排在金额序列最前面
    $todayMax = ($recordsAfter | Where-Object { $_.description -eq $editedDesc } | Select-Object -First 1).price
    if ($todayMax) {
        Assert-True ($amountsOnScreen.Count -ge 1 -and [Math]::Abs([Math]::Abs($amountsOnScreen[0]) - ($todayMax / 100.0)) -lt 0.001) `
            "金额最大的排在第一（期望 $($todayMax / 100.0)，实际 $($amountsOnScreen[0])）"
    }
    # ================= 5/5 筛选与统计条 =================
    # 「共 N 条记录」要与库里当前账本的条数一致，再用关键词筛出唯一那条。
    Write-Host "`n[tr] 5/5 筛选：按关键词筛出唯一那条，并核对「共 N 条记录」"
    $ledgerId = ($editedRows | Select-Object -First 1).ledger_id
    Assert-True ([bool]$ledgerId) "从改后的记录上反推当前账本（$ledgerId）"
    $ledgerRows = @($recordsAfter | Where-Object { $_.ledger_id -eq $ledgerId })
    $totalText = $null
    $deadline = (Get-Date).AddSeconds(10)
    do {
        $totalText = Get-Elements $window | ForEach-Object { $_.Current.Name } |
            Where-Object { $_ -and $_ -like '共 * 条记录' } | Select-Object -First 1
        if (-not $totalText) { Start-Sleep -Milliseconds 400 }
    } while (-not $totalText -and (Get-Date) -lt $deadline)
    Assert-True ([bool]$totalText) "页面上有「共 N 条记录」（$totalText）"
    if ($totalText) {
        $shown = [int]($totalText -replace '[^\d]', '')
        # 页面默认带**时间范围**筛选（种子里那些更早的记录不在范围内），所以只断言
        # "至少包含我们这三笔、且不超过库里该账本的总数"，精确相等留给下面的关键词筛选。
        Assert-True ($shown -ge 3 -and $shown -le $ledgerRows.Count) `
            "统计条落在合理范围（页面 $shown 条 / 该账本共 $($ledgerRows.Count) 条）"
    }

    # 打开筛选（悬浮按钮）→ 关键词 = 那笔唯一描述 → 添加条件 → 确认
    # ⚠ 这个悬浮按钮**带 children（角标），可访问名不是 title**（UIA 里只有「排序」「记一笔」有名字），
    # 所以按 class `tr-float-secondary` 找。
    $filterButton = $null
    foreach ($element in @(Get-Elements $window)) {
        if ($element.Current.ControlType -ne [System.Windows.Automation.ControlType]::Button) { continue }
        if ($element.Current.IsOffscreen) { continue }
        if ($element.Current.ClassName -notlike '*tr-float-secondary*') { continue }
        $rect = $element.Current.BoundingRectangle
        if ($rect.Width -le 0) { continue }
        $filterButton = $element
        break
    }
    Assert-True ([bool]$filterButton) '找到悬浮按钮「筛选条件」（class=tr-float-secondary）'
    if ($filterButton) { Invoke-Element $filterButton | Out-Null }
    Start-Sleep -Seconds 2
    Assert-True ([bool](Wait-Element -Root $window -Name '筛选消费记录' -TimeoutSec 10)) '弹窗「筛选消费记录」已打开'
    Assert-True (Set-Value (Wait-Element -Root $window -Name '输入关键词') $editedDesc) "「描述包含」填 $editedDesc"
    Start-Sleep -Milliseconds 600
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '+ 添加筛选条件' -TimeoutSec 10)) '点「+ 添加筛选条件」'
    Start-Sleep -Milliseconds 800
    # 注：这里**不要**断言"弹窗里出现了这条关键词"——背后表格行本来就含这个描述，会假绿；
    # 真正的判据是"确认后只剩 1 条"（下面）。
    $applyButton = @(Find-All $window '确认' | Where-Object { -not $_.Current.IsOffscreen })
    Assert-True ($applyButton.Count -gt 0) '找到筛选弹窗的「确认」'
    if ($applyButton.Count -gt 0) { Invoke-Element $applyButton[$applyButton.Count - 1] | Out-Null }
    Start-Sleep -Seconds 4

    $filteredText = $null
    $deadline = (Get-Date).AddSeconds(12)
    do {
        $filteredText = Get-Elements $window | ForEach-Object { $_.Current.Name } |
            Where-Object { $_ -and $_ -like '共 * 条记录' } | Select-Object -First 1
        if ($filteredText -notlike '共 1 条*') { Start-Sleep -Milliseconds 400 }
    } while ($filteredText -notlike '共 1 条*' -and (Get-Date) -lt $deadline)
    Assert-True ($filteredText -like '共 1 条*') "筛选后只剩 1 条（实际 '$filteredText'）"
    $visibleDesc = @(Get-Elements $window | ForEach-Object { $_.Current.Name } |
        Where-Object { $_ -and $_.Contains($editedDesc) })
    Assert-True ($visibleDesc.Count -gt 0) "列表里就是筛出来的那条（$editedDesc）"
    # 支出金额应当还在（88.88）—— 说明筛选没有把行内容搞丢
    $amountAfterFilter = @(Get-Elements $window | ForEach-Object { $_.Current.Name } |
        Where-Object { $_ -and $_ -match '88\.88' })
    Assert-True ($amountAfterFilter.Count -gt 0) '筛出来的行仍显示金额 88.88'
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
    Write-Host "[tr] 失败 $($failures.Count) 项：" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "   - $_" -ForegroundColor Red }
    exit 1
}
Write-Host '[tr] 全部通过：编辑=先建后删 + 保存为模板 + 排序（金额降序）+ 筛选（关键词收敛到 1 条）' -ForegroundColor Green
