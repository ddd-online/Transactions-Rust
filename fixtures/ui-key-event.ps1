# ui-key-event.ps1 —— 事件页端到端：**任选日期**新建 + **同一天 upsert（日期唯一）** + 删除 +
# **右栏（关联交易）开合偏好落盘/换页/重启保持**。
#
# 为什么需要它：`fixtures/ui-crud.ps1` 里已经用「添加事件」建过事件，但它走的是**默认日期（今天）**，
# 只验了"能建出来 + 改颜色/写 Markdown/删除"；`fixtures/ui-link-event.ps1` 验的是"记账记录关联到
# 某天 → 该天懒创建空事件"。**从没验过**的两件事正好是这一页的数据语义核心：
#   * 在「添加事件」弹窗里用 DatePicker **任选一个不是今天的日期** → 事件要落在**那一天**；
#   * 同一天再建一次是 **upsert（覆盖）而不是新增**：`(ledger_id, date)` 唯一，**原 id 与 createdAt 保留**，
#     只替换标题/正文/颜色（落库语义已由数据层测试覆盖，这里覆盖界面这条路径）。
# 第 4 步是被用户当场抓到的缺陷的回归：右栏开合原来只活在页面里，换页回来又默认展开；
# 现在偏好写进配置文件 `keyEventLinkedOpen`，判据落在**磁盘 + 重启**上（不是"点完看一眼"）。
#
# 用法（pwsh 7；需要 release 产物；本仓库不能有实例在跑）：
#   pwsh -File fixtures/ui-key-event.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

param(
    [string]$Exe,
    [string]$SmokeHome,
    [string]$Workspace,
    [string]$OutDir
)

$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot 'lib\TrUia.ps1')

# 右栏（关联交易）把手：点到**状态真的变了**为止，几种点法都试。
#
# 这一条以各种面目偶发红过（收起没生效 / 展开没生效都见过），拆出来四个成因，各自的对策：
#   1. **按下与抬起之间不留时间**（真凶）：把手有 1px 按压下沉，`[TrUia]::Click` 零间隔的
#      down/up 会让 up 落在按钮外 → 见 `Click-RailHandle`（带 80ms 间隔）；
#   2. **`title` 提示与把手同名**：指针停在把手上时提示冒出来，按名字取第一个会取到那条
#      132x38 的窄条提示（把手是 36x73 的 Button），点它没反应 → 见 `Find-RailHandle`
#      （只认 Button）+ 点之前先把指针挪开；
#   3. 真实鼠标点**别的窗口**上：脚本在后台跑时窗口被盖住（实测截图：窗口右半边压着 Codex），
#      收起态的把手正好在窗口右缘 → 先置前台 + 写配置时把窗口钉在 (0,0)（见写配置那段）；
#   4. `InvokePattern` 对**刚渲染出来**的元素有时不生效（AGENTS 记过：dev-shot 切侧栏因此
#      改用真实鼠标）→ 真实鼠标之后再用它兜底一次，另外补一条键盘路径（SetFocus + 空格）。
# 判据落在配置与重启上，随便红一次都很误导，所以这里重试到状态真的变了为止。
function Set-RailCollapsed {
    param($Window, [bool]$Collapsed, [int]$Tries = 3)
    $from = if ($Collapsed) { '收起关联交易' } else { '展开关联交易' }
    $to = if ($Collapsed) { '展开关联交易' } else { '收起关联交易' }
    for ($i = 1; $i -le $Tries; $i++) {
        # 先置前台、再把指针挪开：窗口没激活时拿到的矩形点下去可能落在别处，
        # 而指针停在把手上会冒出同名的 `title` 提示。
        [TrUia]::SetForegroundWindow([IntPtr]$Window.Current.NativeWindowHandle) | Out-Null
        [TrUia]::SetCursorPos(30, 30) | Out-Null
        Start-Sleep -Milliseconds 350
        for ($click = 1; $click -le 2; $click++) {
            $handle = Find-RailHandle -Window $Window -Name $from -TimeoutSec 4
            if (-not $handle) { break }
            Click-RailHandle $handle | Out-Null
            if (Wait-Element -Root $Window -Name $to -TimeoutSec 3) { return $true }
            # 真实鼠标没生效 → 用键盘：先把焦点落到把手（SetFocus 不依赖命中测试），再按空格
            try { $handle.SetFocus() } catch { }
            Start-Sleep -Milliseconds 200
            [System.Windows.Forms.SendKeys]::SendWait(' ')
            if (Wait-Element -Root $Window -Name $to -TimeoutSec 3) { return $true }
        }
        # 已经是目标状态（上一次点生效了，只是名字变化比断言慢）
        if (Wait-Element -Root $Window -Name $to -TimeoutSec 2) { return $true }
        $handle = Find-RailHandle -Window $Window -Name $from -TimeoutSec 2
        if ($handle) { Invoke-Element $handle | Out-Null }
        if (Wait-Element -Root $Window -Name $to -TimeoutSec 3) { return $true }
    }
    return $false
}

# 点把手：按下与抬起之间留 80ms。
# `[TrUia]::Click` 是零间隔的 down/up，而把手按下时会下沉 1px（全站的按压反馈）——
# down 之后元素挪了位置，up 可能落在按钮外，click 事件根本不触发。
# 实测：不带间隔的那版 6 次里红 4 次（红的判据是配置没写进去、把手名字没变），
# 带间隔的这版连过 4 次。dev-shot 的 Click 本来也是带间隔的，这里沿用同一做法。
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class TrKeRail {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  public static void Click(int x, int y) {
    SetCursorPos(x, y); System.Threading.Thread.Sleep(80);
    mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero); System.Threading.Thread.Sleep(80);
    mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
  }
}
'@ -Language CSharp -ErrorAction SilentlyContinue

function Click-RailHandle { param($Element)
    if (-not $Element) { return $false }
    $rect = $Element.Current.BoundingRectangle
    if (-not (Test-Rect $rect)) { return $false }
    [TrKeRail]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
    return $true
}

# 右栏把手：**只认 Button** —— `title` 提示与它同名（提示是 132x38 的窄条，把手是 36x73），
# 按名字取第一个会取到提示，点它点不动（实测就是这么红的）。
function Find-RailHandle {
    param($Window, [string]$Name, [int]$TimeoutSec = 5)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $candidate = @($Window.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $Name)))) |
            Where-Object {
                $_.Current.ControlType.ProgrammaticName -eq 'ControlType.Button' -and
                (Test-Rect $_.Current.BoundingRectangle)
            } | Select-Object -First 1
        if ($candidate) { return $candidate }
        Start-Sleep -Milliseconds 300
    }
    return $null
}

$repo = Split-Path -Parent $PSScriptRoot
$explicitWorkspace = -not [string]::IsNullOrWhiteSpace($Workspace)
if (-not $Exe) { $Exe = Join-Path $repo 'target\release\transactions.exe' }
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\tests\ui-key-event\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\tests\ui-key-event\out' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$trPaths = Initialize-TrSmokeHome -SmokeHome $SmokeHome -OutDir $OutDir
$smokeHome = $trPaths.SmokeHome
$OutDir = $trPaths.OutDir
Assert-NoRepoInstance -Repo $repo

$ws = [System.IO.Path]::GetFullPath($Workspace)

# 公共鼠标 P/Invoke 类（TrKe）已统一到 fixtures/lib/TrUia.ps1 的 TrUia（调用点是 [TrUia]::…）

$failures = New-Object System.Collections.Generic.List[string]
function Wait-Element { param($Root, [string]$Name, [int]$TimeoutSec = 20)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $element = Find-First $Root $Name
        if ($element) { return $element }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    return $null
}
function Wait-Like { param($Root, [string]$Pattern, [int]$TimeoutSec = 20)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $element = Find-Like -Root $Root -Pattern $Pattern
        if ($element) { return $element }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    return $null
}
function Set-Value { param($Element, [string]$Value)
    if (-not $Element) { return $false }
    $pattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
        $pattern.SetValue($Value); return $true
    }
    $rect = $Element.Current.BoundingRectangle
    if ($rect.Width -gt 0) {
        [TrUia]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
        Start-Sleep -Milliseconds 200
        [System.Windows.Forms.SendKeys]::SendWait('^a')
        [System.Windows.Forms.SendKeys]::SendWait($Value)
        return $true
    }
    return $false
}
# 行内按钮：先把指针移到该行中心（操作区只在 hover 时显示），再按名字 + 行中心 Y 就近取
function Find-RowButton { param($Window, [string]$RowName, [string]$ButtonName)
    $row = Wait-Like -Root $Window -Pattern $RowName -TimeoutSec 15
    if (-not $row) { return $null }
    $rowRect = $row.Current.BoundingRectangle
    [TrUia]::Click([int]($rowRect.X + 10), [int]($rowRect.Y + $rowRect.Height / 2)) | Out-Null
    Start-Sleep -Milliseconds 500
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
    return $null
}

# ---- DatePicker：触发器不可按占位符找（有值时名字就是那个日期）；格子要**真正渲染出来**的 ----
function Select-Date { param($Window, [int]$Day)
    $trigger = Wait-Element -Root $Window -Name '选择日期' -TimeoutSec 3
    if (-not $trigger) {
        $deadline = (Get-Date).AddSeconds(10)
        do {
            $trigger = Find-DateTrigger -Window $Window
            if (-not $trigger) { Start-Sleep -Milliseconds 400 }
        } while (-not $trigger -and (Get-Date) -lt $deadline)
    }
    if (-not $trigger) { Write-Host '    找不到日期触发器（class=ui-date-picker__trigger）' -ForegroundColor DarkYellow; return '' }
    $before = $trigger.Current.Name
    Invoke-Element $trigger | Out-Null
    $cell = $null
    $deadline = (Get-Date).AddSeconds(8)
    do {
        $cell = Find-DateCell -Window $Window -Day $Day
        if (-not $cell) { Start-Sleep -Milliseconds 300 }
    } while (-not $cell -and (Get-Date) -lt $deadline)
    if (-not $cell) { Write-Host "    没找到已渲染的第 $Day 天" -ForegroundColor DarkYellow; return '' }
    Click-Element $cell | Out-Null
    Start-Sleep -Milliseconds 800
    $after = Find-DateTrigger -Window $Window
    $value = if ($after) { $after.Current.Name } else { '' }
    Write-Host "    日期选择器：'$before' → '$value'"
    return $value
}

# ---- 播种 ----
if (-not $explicitWorkspace) {
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
}
if (-not (Test-Path (Join-Path $ws 'transactions.db'))) {
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[ke] 播种工作空间: $ws" -ForegroundColor Cyan
}

# `x`/`y` 钉在 (0,0)：收起态的右栏把手贴在窗口右缘，而本机屏幕右侧常年停着别的窗口
# （Codex），窗口靠右时把手正好被压住（见 Set-RailCollapsed 的注释）。
@{ width = 1500; height = 950; x = 0; y = 0; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'ui-key-event.ps1' } |
    ConvertTo-Json | Set-Content -Path (Join-Path $smokeHome '.transactions.json') -Encoding UTF8

$stamp = Get-Date -Format 'HHmmss'
$titleA = "UIA事件甲$stamp"
$titleB = "UIA事件乙$stamp"
# 选一个**不是今天**的日子（选今天就看不出 DatePicker 有没有生效 —— 弹窗默认就是今天）
$today = Get-Date
$day = if ($today.Day -eq 20) { 21 } else { 20 }
$eventDate = $today.ToString('yyyy-MM-') + $day.ToString('00')

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
    $hwnd = [IntPtr]$window.Current.NativeWindowHandle
    [TrUia]::ShowWindow($hwnd, 9) | Out-Null
    [TrUia]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Seconds 1

    Write-Host "`n[ke] 打开「事件」页（目标日期 $eventDate）"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '事件')) '打开「事件」页'
    Start-Sleep -Seconds 3

    # ================= 1/3 任选日期新建 =================
    Write-Host "[ke] 1/3 用 DatePicker 选 $eventDate 并新建「$titleA」"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '新增事件' -TimeoutSec 15)) '点「新增事件」'
    Start-Sleep -Seconds 2
    Assert-True ([bool](Wait-Element -Root $window -Name '新增事件' -TimeoutSec 8)) '弹窗「新增事件」已打开'
    $picked = Select-Date -Window $window -Day $day
    Assert-True ($picked -eq $eventDate) "日期选择器选中 $eventDate（实际 '$picked'）"
    Assert-True (Set-Value (Wait-Element -Root $window -Name '事件名称（可选）') $titleA) "填入事件名称「$titleA」"
    Start-Sleep -Milliseconds 600
    $confirmAdd = @(Find-All $window '新增' | Where-Object { -not $_.Current.IsOffscreen })
    Assert-True ($confirmAdd.Count -gt 0) '找到弹窗「新增」'
    if ($confirmAdd.Count -gt 0) { Invoke-Element $confirmAdd[$confirmAdd.Count - 1] | Out-Null }
    Start-Sleep -Seconds 3

    $created = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_key_event' -OutDir $OutDir | Where-Object { $_.date -eq $eventDate })
    Assert-True ($created.Count -eq 1) "库里 $eventDate 恰好一条事件（实际 $($created.Count) 条）"
    $eventId = if ($created.Count -ge 1) { $created[0].id } else { '' }
    $ledgerId = if ($created.Count -ge 1) { $created[0].ledger_id } else { '' }
    if ($created.Count -ge 1) {
        Assert-True ($created[0].title -eq $titleA) "标题落库「$titleA」（实际 '$($created[0].title)'）"
    }
    $titleShown = Wait-Like -Root $window -Pattern $titleA -TimeoutSec 10
    Assert-True ([bool]$titleShown) '左侧事件列表里出现该事件'

    # ================= 2/3 同一天再建一次：upsert =================
    Write-Host "[ke] 2/3 同一天（$eventDate）再建一次，标题换成「$titleB」"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '新增事件' -TimeoutSec 15)) '再次点「新增事件」'
    Start-Sleep -Seconds 2
    $pickedAgain = Select-Date -Window $window -Day $day
    Assert-True ($pickedAgain -eq $eventDate) "再次选中同一天 $eventDate（实际 '$pickedAgain'）"
    Assert-True (Set-Value (Wait-Element -Root $window -Name '事件名称（可选）') $titleB) "填入新标题「$titleB」"
    Start-Sleep -Milliseconds 600
    $confirmAgain = @(Find-All $window '新增' | Where-Object { -not $_.Current.IsOffscreen })
    if ($confirmAgain.Count -gt 0) { Invoke-Element $confirmAgain[$confirmAgain.Count - 1] | Out-Null }
    Start-Sleep -Seconds 3

    $upserted = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_key_event' -OutDir $OutDir | Where-Object { $_.date -eq $eventDate })
    Assert-True ($upserted.Count -eq 1) "同一天仍是**一条**（upsert 而不是新增，实际 $($upserted.Count) 条）"
    if ($upserted.Count -eq 1) {
        Assert-True ($upserted[0].title -eq $titleB) "标题被覆盖为「$titleB」（实际 '$($upserted[0].title)'）"
        Assert-True ($upserted[0].id -eq $eventId) 'id 保留（覆盖写不是删了重建）'
        Assert-True ($upserted[0].ledger_id -eq $ledgerId) '账本没变'
    }
    $newShown = Wait-Like -Root $window -Pattern $titleB -TimeoutSec 10
    Assert-True ([bool]$newShown) '列表里显示新标题'
    $oldShown = @(Get-Elements $window | ForEach-Object { $_.Current.Name } |
        Where-Object { $_ -and $_.Contains($titleA) })
    Assert-True ($oldShown.Count -eq 0) '列表里不再显示旧标题'

    # ================= 3/3 删除（卡片删除 → **弹窗**二次确认）=================
    Write-Host "[ke] 3/3 删除事件「$titleB」"
    $deleteButton = Find-RowButton -Window $window -RowName $titleB -ButtonName '删除事件'
    Assert-True ([bool]$deleteButton) '找到该事件的「删除事件」按钮'
    if ($deleteButton) {
        Click-Element $deleteButton | Out-Null
        # 二次确认是弹窗（原为 Popconfirm 气泡）：标题带事件名，用它断言弹窗真的开了
        $modalTitle = Wait-Like -Root $window -Pattern "删除事件「$titleB」" -TimeoutSec 10
        Assert-True ([bool]$modalTitle) '弹出「删除事件」二次确认弹窗'
        Start-Sleep -Milliseconds 600
        # 弹窗主按钮是「删除」（与卡片上的「删除事件」不同名，不会认错）
        $confirmDelete = Find-All $window '删除'
        if ($confirmDelete.Count -gt 0) { Invoke-Element $confirmDelete[$confirmDelete.Count - 1] | Out-Null }
        Start-Sleep -Seconds 3
    }
    $gone = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_key_event' -OutDir $OutDir | Where-Object { $_.date -eq $eventDate })
    Assert-True ($gone.Count -eq 0) "删除后 $eventDate 在库里没有事件了（实际 $($gone.Count) 条）"

    # ================= 4/4 右栏偏好：落盘 + 换页保持 + 重启保持 =================
    # 这一条是被用户当场抓到的缺陷：原来右栏开合只活在页面组件里，换页回来就默认又展开了。
    # 现在偏好写进配置文件（`keyEventLinkedOpen`），判据落在**磁盘 + 重启**上。
    Write-Host "[ke] 4/4 右栏（关联交易）偏好：收起 → 写配置 → 换页保持 → 重启仍保持"
    $configPath = Join-Path $smokeHome '.transactions.json'
    Assert-True (Set-RailCollapsed -Window $window -Collapsed $true) `
        '收起后把手变成「展开关联交易」'
    $savedConfig = Get-Content $configPath -Raw | ConvertFrom-Json
    Assert-True ($savedConfig.keyEventLinkedOpen -eq $false) `
        "配置里写入 keyEventLinkedOpen=false（实际 '$($savedConfig.keyEventLinkedOpen)'）"

    # 换页（事件 → 记账 → 事件）：右栏应当仍是收起的
    Invoke-Element (Wait-Element -Root $window -Name '记账' -TimeoutSec 10) | Out-Null
    Start-Sleep -Seconds 2
    Invoke-Element (Wait-Element -Root $window -Name '事件' -TimeoutSec 10) | Out-Null
    Start-Sleep -Seconds 3
    Assert-True ([bool](Wait-Element -Root $window -Name '展开关联交易' -TimeoutSec 6)) `
        '换页回来后右栏仍是收起的'

    # 重启：同一个临时 HOME/工作空间，偏好应当被读回来
    Stop-TrApp -Process $process -Failures (New-Object System.Collections.Generic.List[string]) -OutDir $OutDir
    $envSaved = @{ USERPROFILE = $env:USERPROFILE; HOME = $env:HOME }
    try {
        $env:USERPROFILE = $smokeHome
        $env:HOME = $smokeHome
        $process = Start-Process -FilePath $Exe -PassThru
    }
    finally {
        $env:USERPROFILE = $envSaved.USERPROFILE
        $env:HOME = $envSaved.HOME
    }
    $window = Get-ReadyWindow -ProcessId $process.Id -TimeoutSec 60
    Assert-True ([bool]$window) '重启后拿到主窗口'
    if ($window) {
        [TrUia]::ShowWindow([IntPtr]$window.Current.NativeWindowHandle, 9) | Out-Null
        [TrUia]::SetForegroundWindow([IntPtr]$window.Current.NativeWindowHandle) | Out-Null
        Invoke-Element (Wait-Element -Root $window -Name '事件' -TimeoutSec 15) | Out-Null
        Start-Sleep -Seconds 3
        Assert-True ([bool](Wait-Element -Root $window -Name '展开关联交易' -TimeoutSec 8)) `
            '重启后右栏仍是收起的（偏好被记住）'

        # 反向也要落盘：展开后配置写回 true（别只验一个方向）
        Assert-True (Set-RailCollapsed -Window $window -Collapsed $false) '再展开后把手变回「收起关联交易」'
        $restoredConfig = Get-Content $configPath -Raw | ConvertFrom-Json
        Assert-True ($restoredConfig.keyEventLinkedOpen -eq $true) `
            "再展开后写回 true（实际 '$($restoredConfig.keyEventLinkedOpen)'）"
    }
}
finally { Stop-TrApp -Process $process -Failures $failures -OutDir $OutDir }

Show-TrSummary -Failures $failures -Tag 'ke' -SuccessMessage "[ke] 全部通过：任选日期新建 → 同一天 upsert（id 保留）→ 删除 → 右栏偏好落盘/换页/重启保持"
