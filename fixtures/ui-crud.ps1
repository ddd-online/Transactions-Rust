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

. (Join-Path $PSScriptRoot 'lib\TrUia.ps1')

$repo = Split-Path -Parent $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $repo 'target\release\transactions.exe' }
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\crud-smoke\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\crud-smoke' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$trPaths = Initialize-TrSmokeHome -SmokeHome $SmokeHome -OutDir $OutDir
$smokeHome = $trPaths.SmokeHome
$OutDir = $trPaths.OutDir
Assert-NoRepoInstance -Repo $repo

$ws = [System.IO.Path]::GetFullPath($Workspace)

# 公共鼠标 P/Invoke 类（TrCrud）已统一到 fixtures/lib/TrUia.ps1 的 TrUia（调用点是 [TrUia]::…）

$failures = New-Object System.Collections.Generic.List[string]

function Wait-ElementLike { param($Root, [string]$Pattern, [int]$TimeoutSec = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $element = Find-ElementLike -Root $Root -Pattern $Pattern
        if ($element) { return $element }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    return $null
}
function Invoke-Element { param($Element)
    if (-not $Element) { return $false }
    $pattern = $null
    # UIA 的 InvokePattern 在元素刚被重新渲染过时会抛 "无法识别的错误"（实测：弹窗刚打开就点确认、
    # 列表刚刷新就点行内按钮），这不是"按钮不可点"，而是句柄过期。失败时退回**真实鼠标点击**。
    try {
        if ($Element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
            $pattern.Invoke(); return $true
        }
    } catch {
        Write-Host "    InvokePattern 失败，改用鼠标点击：$($_.Exception.Message)" -ForegroundColor DarkYellow
    }
    $rect = $Element.Current.BoundingRectangle
    if (-not [double]::IsFinite($rect.X) -or -not [double]::IsFinite($rect.Y)) { return $false }
    if ($rect.Width -gt 0 -and $rect.Height -gt 0) {
        [TrUia]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
        return $true
    }
    return $false
}
function Click-Element { param($Element)
    if (-not $Element) { return $false }
    $rect = $Element.Current.BoundingRectangle
    if ($rect.Width -le 0) { return $false }
    # 点之前先把窗口拉到前台：鼠标点击是按**屏幕坐标**投递的，本机的应用窗口常常只占屏幕一部分
    # （实测窗口不在前台时点击落到别的窗口上，"点了没反应"）。另外确认目标落在窗口矩形内。
    $window = $Element | Get-ParentWindow
    if ($window) {
        $wr = $window.Current.BoundingRectangle
        [TrUia]::SetForegroundWindow([IntPtr]$window.Current.NativeWindowHandle) | Out-Null
        if ($rect.X -lt $wr.X -or ($rect.X + $rect.Width) -gt ($wr.X + $wr.Width) -or
            $rect.Y -lt $wr.Y -or ($rect.Y + $rect.Height) -gt ($wr.Y + $wr.Height)) {
            Write-Host "    目标元素不在窗口矩形内，跳过点击：$rect vs $wr" -ForegroundColor DarkYellow
            return $false
        }
    }
    [TrUia]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
    return $true
}
# 从任意元素往上找顶层窗口（`ControlViewWalker` 到根）
function Get-ParentWindow { param($Element)
    $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
    $node = $Element
    while ($node) {
        $parent = $walker.GetParent($node)
        if (-not $parent) { return $node }
        $node = $parent
    }
    return $null
}
# 按名字找**输入框**：本项目的表单在弹窗里用 `<p class="modal-form-label">` 做标签，
# 它与输入框的 UIA 名可能**同名**（例如「图表名称」），按名字取第一个会拿到标签（Text），
# `Set-Value` 对它必然失败。所以判据要写在 ControlType/ClassName 上。
function Wait-EditLike { param($Window, [string]$Name, [int]$TimeoutSec = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        foreach ($el in @(Get-Elements $Window)) {
            $isEdit = ($el.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit) -or
                ($el.Current.ClassName -eq 'Edit')
            if ($isEdit -and $el.Current.Name -eq $Name -and -not $el.Current.IsOffscreen) { return $el }
        }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    return $null
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
    [TrUia]::SetCursorPos([int]($rowRect.X + $rowRect.Width / 2), [int]($rowRect.Y + $rowRect.Height / 2)) | Out-Null
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

# 弹窗里的确认按钮。
#
# ⚠ 不要只按"同名取最后一个"来点：页面入口与弹窗确认键经常同名（「新增分类」页签按钮
# 与弹窗里的「新增」只是不同名），而 UIA 树里还会留下**屏幕上没显示**的同名元素
# （未展开的面板、别的分栏里的按钮），对它们 Invoke 会静默无效或直接抛
# "Invoke 无法识别的错误"——现象是"点了确认但库里没有新行"。
# 这里只认**真正可见**且矩形落在窗口内的候选，再取在 DOM 中靠后的那个（弹窗在末尾）。
function Invoke-ModalButton { param($Window, [string]$Name)
    $windowRect = $Window.Current.BoundingRectangle
    $visible = @()
    foreach ($element in @(Find-All $Window $Name)) {
        if ($element.Current.IsOffscreen) { continue }
        $rect = $element.Current.BoundingRectangle
        if (-not [double]::IsFinite($rect.X) -or -not [double]::IsFinite($rect.Y)) { continue }
        if ($rect.Width -le 0 -or $rect.Height -le 0) { continue }
        if ($rect.Y -lt $windowRect.Y -or ($rect.Y + $rect.Height) -gt ($windowRect.Y + $windowRect.Height)) { continue }
        if ($rect.X -lt $windowRect.X -or ($rect.X + $rect.Width) -gt ($windowRect.X + $windowRect.Width)) { continue }
        $visible += $element
    }
    if ($visible.Count -eq 0) {
        $allCount = (Find-All $Window $Name).Count
        Write-Host "    弹窗里没有可见的「$Name」按钮（同名元素 $allCount 个）" -ForegroundColor DarkYellow
        return $false
    }
    return (Invoke-Element $visible[$visible.Count - 1])
}

# 弹窗主按钮（确认键）的可访问名会随文案调整而变（「确认」/「新增」/「保存」/「添加」…），
# 而页面入口可能正好同名。这里按"**可见 + 在窗口内 + 落在窗口下半部**"筛（弹窗居中，
# 其底部按钮必然在视口下半部），再取最靠下的一个 —— 这样改文案不会让用例变成假红/假绿。
function Invoke-ModalPrimaryButton { param($Window)
    $windowRect = $Window.Current.BoundingRectangle
    $candidates = @()
    foreach ($name in @('确认', '新增', '添加', '保存', '创建')) {
        foreach ($element in @(Find-All $Window $name)) {
            if ($element.Current.IsOffscreen) { continue }
            $rect = $element.Current.BoundingRectangle
            if (-not [double]::IsFinite($rect.X) -or -not [double]::IsFinite($rect.Y)) { continue }
            if ($rect.Width -le 0 -or $rect.Height -le 0) { continue }
            if ($rect.Y -lt $windowRect.Y -or ($rect.Y + $rect.Height) -gt ($windowRect.Y + $windowRect.Height)) { continue }
            if ($rect.X -lt $windowRect.X -or ($rect.X + $rect.Width) -gt ($windowRect.X + $windowRect.Width)) { continue }
            if (($rect.Y + $rect.Height / 2) -lt ($windowRect.Y + $windowRect.Height * 0.45)) { continue }
            $candidates += [pscustomobject]@{ Element = $element; Name = $name; Y = $rect.Y }
        }
    }
    if ($candidates.Count -eq 0) {
        Write-Host '    找不到弹窗主按钮（确认/新增/添加/保存）' -ForegroundColor DarkYellow
        return $false
    }
    $pick = $candidates | Sort-Object Y | Select-Object -Last 1
    Write-Host "    点弹窗主按钮「$($pick.Name)」" -ForegroundColor DarkGray
    return (Invoke-Element $pick.Element)
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
    [TrUia]::ShowWindow($hwnd, 9) | Out-Null
    [TrUia]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 500

    # ================= 1/4 分类与标签：新增 → 删除 =================
    Write-Host "`n[crud] 1/4 记账 · 标签子功能：新增分类 → 删除"
    Assert-True (Invoke-SubFunction -Window $window -Name '标签') '切到记账页的「标签」子功能'
    Start-Sleep -Seconds 2

    $before = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_category' -OutDir $OutDir | Where-Object { $_.name -eq $categoryName })
    Assert-True ($before.Count -eq 0) "初始没有同名分类（$categoryName）"

    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '新增分类')) '点「新增分类」'
    Start-Sleep -Milliseconds 800
    Assert-True (Set-Value (Wait-Element -Root $window -Name '输入分类名称') $categoryName) '填入分类名称'
    Invoke-ModalPrimaryButton -Window $window | Out-Null
    Start-Sleep -Seconds 3

    $after = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_category' -OutDir $OutDir | Where-Object { $_.name -eq $categoryName })
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
    $gone = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_category' -OutDir $OutDir | Where-Object { $_.name -eq $categoryName })
    Assert-True ($gone.Count -eq 0) "删除后库里不再有该分类（$categoryName）"

    # ---- 标签：先选中一个分类，再新增/删除标签 ----
    Write-Host "`n[crud] 2/4 标签：新增 → 删除"
    $someCategory = (Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_category' -OutDir $OutDir | Where-Object { $_.transaction_type -eq 'expense' } |
        Sort-Object sort_order | Select-Object -First 1).name
    $categoryRow = Wait-Element -Root $window -Name $someCategory
    Assert-True ([bool]$categoryRow) "选中分类「$someCategory」（标签挂在分类下）"
    if ($categoryRow) {
        Invoke-Element $categoryRow | Out-Null
        Start-Sleep -Seconds 2
    }
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '新增标签')) '点「新增标签」'
    Start-Sleep -Milliseconds 800
    Assert-True (Set-Value (Wait-Element -Root $window -Name '输入标签名称') $tagName) '填入标签名称'
    Invoke-ModalPrimaryButton -Window $window | Out-Null
    Start-Sleep -Seconds 3
    $tagAfter = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_tag' -OutDir $OutDir | Where-Object { $_.name -eq $tagName })
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
    $tagGone = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_tag' -OutDir $OutDir | Where-Object { $_.name -eq $tagName })
    Assert-True ($tagGone.Count -eq 0) "删除后库里不再有该标签（$tagName）"

    # ================= 3/4 图表：新增 → 删除（气泡确认）=================
    Write-Host "`n[crud] 3/4 数据分析：新增图表 → 删除"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '数据分析')) '打开「数据分析」页'
    Start-Sleep -Seconds 2
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '新增图表')) '点「新增图表」'
    Start-Sleep -Milliseconds 800
    # ⚠ 用 Wait-EditLike：弹窗里的「图表名称」既是标签（Text）也是输入框（Edit）
    $chartInput = Wait-EditLike -Window $window -Name '图表名称'
    Assert-True (Set-Value $chartInput $chartTitle) '填入图表名称'
    Invoke-ModalPrimaryButton -Window $window | Out-Null
    Start-Sleep -Seconds 3

    $chartAfter = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_chart' -OutDir $OutDir | Where-Object { $_.title -eq $chartTitle })
    Assert-True ($chartAfter.Count -eq 1) "库里出现新图表（$chartTitle）"

    $chartDeleteButton = Find-RowButton -Window $window -RowName $chartTitle -ButtonName '删除图表'
    Assert-True ([bool]$chartDeleteButton) '找到图表「删除图表」按钮'
    if ($chartDeleteButton) {
        Click-Element $chartDeleteButton | Out-Null
        # 气泡的**确认键**按"可见且在窗口内"轮询（UIA 树是惰性建立的，固定 sleep 后查一次会查空）。
        # ⚠ 不断言"气泡标题可见"：浮层节点的 UIA 可见性本身就不稳定，断言它会把这条用例变成随机红。
        # 真正的判据在下面：确认后库里那一行必须消失。
        $deadline = (Get-Date).AddSeconds(6)
        $popButtons = @()
        do {
            $popButtons = @(Find-All $window '删除' | Where-Object { -not $_.Current.IsOffscreen })
            if ($popButtons.Count -eq 0) { Start-Sleep -Milliseconds 300 }
        } while ($popButtons.Count -eq 0 -and (Get-Date) -lt $deadline)
        if ($popButtons.Count -gt 0) { Invoke-Element $popButtons[$popButtons.Count - 1] | Out-Null }
        Start-Sleep -Seconds 3
    }
    $chartGone = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_chart' -OutDir $OutDir | Where-Object { $_.title -eq $chartTitle })
    Assert-True ($chartGone.Count -eq 0) "删除后库里不再有该图表（$chartTitle）"

    # ================= 4/4 关键事件：配色 / 描述 / 删除 =================
    Write-Host "`n[crud] 4/4 关键事件：建事件 → 改颜色 → 写描述 → 删除"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '关键事件')) '打开「关键事件」页'
    Start-Sleep -Seconds 2
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '新增事件')) '点「新增事件」'
    Start-Sleep -Milliseconds 1000
    Assert-True (Set-Value (Wait-Element -Root $window -Name '事件名称（可选）') $eventTitle) '填入事件名称'
    Invoke-ModalPrimaryButton -Window $window | Out-Null
    Start-Sleep -Seconds 3

    $events = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_key_event' -OutDir $OutDir | Where-Object { $_.title -eq $eventTitle })
    Assert-True ($events.Count -eq 1) "库里出现新事件（$eventTitle）"
    $eventDate = if ($events.Count -ge 1) { $events[0].date } else { '' }

    # ---- 改颜色：点色板（aria-label 就是色值），点击即保存 ----
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name $eventColor)) "点色板 $eventColor"
    Start-Sleep -Seconds 3
    $colored = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_key_event' -OutDir $OutDir | Where-Object { $_.date -eq $eventDate } | Select-Object -First 1)
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
    $described = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_key_event' -OutDir $OutDir | Where-Object { $_.date -eq $eventDate } | Select-Object -First 1)
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
    $eventGone = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_key_event' -OutDir $OutDir | Where-Object { $_.date -eq $eventDate })
    Assert-True ($eventGone.Count -eq 0) "删除后库里不再有该事件（$eventDate）"
    # ================= 5/5 记账 · 模板子功能：新建 → 删除 =================
    Write-Host "`n[crud] 5/5 记账 · 模板子功能：新建 → 删除"
    $templateName = "UIA模板$stamp"
    Assert-True (Invoke-SubFunction -Window $window -Name '模板') '切到记账页的「模板」子功能'
    Start-Sleep -Seconds 2
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '新建模板')) '点「新建模板」'
    Start-Sleep -Milliseconds 1000
    Assert-True (Set-Value (Wait-Element -Root $window -Name '请输入模板名称') $templateName) '填入模板名称'
    # 分类是必填的（前端与后端都会挡）：点开 Select 再选一个已有分类
    $categoryPicker = Wait-Element -Root $window -Name '选择消费分类'
    Assert-True ([bool]$categoryPicker) '找到「选择消费分类」下拉'
    if ($categoryPicker) {
        Invoke-Element $categoryPicker | Out-Null
        Start-Sleep -Milliseconds 800
        $optionName = (Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_category' -OutDir $OutDir | Where-Object { $_.transaction_type -eq 'expense' } |
            Sort-Object sort_order | Select-Object -First 1).name
        $option = Wait-Element -Root $window -Name $optionName
        Assert-True ([bool]$option) "下拉里选中分类「$optionName」"
        if ($option) { Invoke-Element $option | Out-Null }
        Start-Sleep -Milliseconds 800
    }
    Invoke-ModalButton -Window $window -Name '保存' | Out-Null
    Start-Sleep -Seconds 3
    $templateRows = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_transaction_tpl' -OutDir $OutDir | Where-Object { $_.template_name -eq $templateName })
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
        $templateGone = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_transaction_tpl' -OutDir $OutDir | Where-Object { $_.template_name -eq $templateName })
        Assert-True ($templateGone.Count -eq 0) "删除后库里不再有该模板（$templateName）"
    }
}
finally { Stop-TrApp -Process $process -Failures $failures -OutDir $OutDir }

Show-TrSummary -Failures $failures -Tag 'crud' -SuccessMessage "[crud] 全部通过：分类/标签/图表的增删、事件配色与描述、事件删除都落到库里"
