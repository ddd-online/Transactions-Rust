# ui-diary-edit.ps1 —— 日记页**编辑链路**端到端：写内容 → 心情 → 改内容（同一天 upsert）→ 预览 → 删除。
#
# 为什么需要它：`fixtures/ui-diary-io.ps1` 覆盖的是**导入/导出**（原生选目录框 + 编码回退），
# 而"在界面上写日记"这条路此前没人走过。日记编辑器的契约有几处不看代码猜不到：
#   * **没有保存按钮**：输入或切心情后 **1500ms 防抖**自动保存，`Ctrl+S` 立即保存（`on_save_shortcut`）；
#   * 切日期/首次进入是**预览态**，要写作得先点页脚那个 `编辑/预览` 切换按钮；
#   * 同一天再写是 **upsert**：`id` 不变、`word_count` 按 Unicode 标量值重算；
#   * 心情是 6 个 emoji 按钮（入库值就是 emoji，`aria-label` 是中文，如「开心」）；
#   * 删除走 `Modal`（标题「确认删除」，确认按钮也叫「删除」）。
#
# 用法（pwsh 7；需要 release 产物；本仓库不能有实例在跑）：
#   pwsh -File fixtures/ui-diary-edit.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

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
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\diary-edit\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\diary-edit' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$smokeHome = [System.IO.Path]::GetFullPath($SmokeHome)
$OutDir = [System.IO.Path]::GetFullPath($OutDir)
if ($smokeHome -eq [System.IO.Path]::GetFullPath($env:USERPROFILE)) {
    throw '拒绝把临时 HOME 指到真实用户目录 —— 冒烟会改写你的配置'
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
public class TrDiary {
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
function Invoke-Element { param($Element)
    if (-not $Element) { return $false }
    $pattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
        $pattern.Invoke(); return $true
    }
    $rect = $Element.Current.BoundingRectangle
    if ($rect.Width -gt 0) {
        [TrDiary]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
        return $true
    }
    return $false
}
function Click-Element { param($Element)
    if (-not $Element) { return $false }
    $rect = $Element.Current.BoundingRectangle
    if ($rect.Width -le 0) { return $false }
    [TrDiary]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
    return $true
}
# 编辑器里的多行文本域：placeholder 只在空的时候是它的可访问名，写进内容后就变了，
# 所以先按 placeholder 找，找不到就退回"编辑器区域里唯一的 Edit"。
function Get-DiaryTextarea { param($Window)
    $byPlaceholder = Find-First $Window '写下今天的日记…'
    if ($byPlaceholder) { return $byPlaceholder }
    foreach ($element in @(Get-Elements $Window)) {
        $isEdit = ($element.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit) -or
            ($element.Current.ClassName -eq 'Edit')
        if (-not $isEdit -or $element.Current.IsOffscreen) { continue }
        $rect = $element.Current.BoundingRectangle
        if ($rect.Width -lt 200 -or $rect.Height -lt 60) { continue }
        return $element
    }
    return $null
}
# 用**真实粘贴**把文本塞进去：ValuePattern 塞值不一定触发 input 事件，
# 而自动保存是挂在 input 上的（`on_input` → 1500ms 防抖）。
function Paste-Text { param($Element, [string]$Text)
    Set-Clipboard -Value $Text
    $Element.SetFocus()
    Start-Sleep -Milliseconds 300
    # 焦点没进去的话（例如刚点过心情按钮、条目被服务端响应重建过），
    # 后面那串 Ctrl+A/Ctrl+V 就贴到别处去了 —— 实测会导致"内容没改但保存成功"的假绿。
    $focused = [System.Windows.Automation.AutomationElement]::FocusedElement
    $isEdit = $focused -and (($focused.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit) -or
        ($focused.Current.ClassName -eq 'Edit'))
    if (-not $isEdit) {
        Write-Host '    粘贴前焦点不在文本域，重试一次 SetFocus' -ForegroundColor DarkYellow
        $Element.SetFocus()
        Start-Sleep -Milliseconds 400
    }
    [System.Windows.Forms.SendKeys]::SendWait('^a')
    Start-Sleep -Milliseconds 200
    [System.Windows.Forms.SendKeys]::SendWait('^v')
    Start-Sleep -Milliseconds 500
}
# 写内容：**两条腿走路** —— 先用 ValuePattern 把值塞进去，再走一次真实粘贴（触发 input → 防抖保存），
# 最后用 ValuePattern 读回来校验。只靠粘贴会偶发失败（窗口不是前台时 SetFocus 静默无效，
# 于是"内容没改、保存却成功了"，看起来像假绿/假红）。
function Set-DiaryContent { param($Window, [string]$Text)
    $textarea = Get-DiaryTextarea -Window $Window
    if (-not $textarea) { return $false }
    $pattern = $null
    if ($textarea.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
        try { $pattern.SetValue($Text) } catch { Write-Host "    ValuePattern 塞值失败: $_" -ForegroundColor DarkYellow }
    }
    [TrDiary]::SetForegroundWindow([IntPtr]$Window.Current.NativeWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 300
    Paste-Text -Element $textarea -Text $Text
    $fresh = Get-DiaryTextarea -Window $Window
    $readBack = ''
    $valuePattern = $null
    if ($fresh -and $fresh.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$valuePattern)) {
        $readBack = $valuePattern.Current.Value
    }
    if ($readBack -ne $Text) { Write-Host "    文本域读回不一致（'$readBack'）" -ForegroundColor DarkYellow }
    return ($readBack -eq $Text)
}
function Save-Now { param($Window)
    # Ctrl+S：编辑器把快捷保存挂在 Textarea 上（`on_save_shortcut`）
    [TrDiary]::SetForegroundWindow([IntPtr]$Window.Current.NativeWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 200
    [System.Windows.Forms.SendKeys]::SendWait('^s')
    Start-Sleep -Seconds 2
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
function Read-Table { param([string]$Table)
    $dump = Join-Path $OutDir "dump-$Table.json"
    Push-Location $repo
    & cargo -q xtask dump $ws --table $Table *> $dump
    $exit = $LASTEXITCODE
    Pop-Location
    if ($exit -ne 0) { throw "导出 $Table 失败（exit=$exit）" }
    return @((Get-Content $dump -Raw | ConvertFrom-Json).$Table)
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
    Write-Host "[diary] 播种工作空间: $ws" -ForegroundColor Cyan
}

@{ width = 1500; height = 950; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'ui-diary-edit.ps1' } |
    ConvertTo-Json | Set-Content -Path (Join-Path $smokeHome '.transactions.json') -Encoding UTF8

$today = (Get-Date).ToString('yyyy-MM-dd')
$stamp = Get-Date -Format 'HHmmss'
$contentA = "# UIA日记$stamp`n第一行内容"
$contentB = "# UIA日记$stamp`n改过的第二行"
$moodEmoji = '😊'

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
    [TrDiary]::ShowWindow($hwnd, 9) | Out-Null
    [TrDiary]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Seconds 1

    # ================= 1/4 打开日记页：今天应已就绪（预览态）=================
    Write-Host "`n[diary] 1/4 打开「日记管理」，进入编辑态"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '日记管理')) '打开「日记管理」'
    Start-Sleep -Seconds 3
    # 页脚的编辑/预览切换：预览态时按钮文案是「编辑」
    $toggle = Wait-Element -Root $window -Name '编辑' -TimeoutSec 15
    Assert-True ([bool]$toggle) '找到页脚的「编辑」（说明当前是预览态）'
    if ($toggle) { Invoke-Element $toggle | Out-Null }
    Start-Sleep -Seconds 1
    $textarea = Get-DiaryTextarea -Window $window
    Assert-True ([bool]$textarea) '编辑态下找到日记文本域'
    if (-not $textarea) { throw '找不到日记文本域，后续无法继续' }

    # ================= 2/4 写内容 → Ctrl+S → 落库 =================
    Write-Host "`n[diary] 2/4 写入内容并保存（$contentA）"
    $rows = @()
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        $typed = Set-DiaryContent -Window $window -Text $contentA
        Save-Now -Window $window
        $rows = @(Read-Table 'tbl_billadm_diary_entry' | Where-Object { $_.date -eq $today })
        if ($rows.Count -eq 1 -and $rows[0].content -eq $contentA) { break }
        Write-Host "    第 $attempt 次写入没生效（typed=$typed，库里 $($rows.Count) 条），重试" -ForegroundColor DarkYellow
    }
    Assert-True ($rows.Count -eq 1) "库里出现今天这一条（$today，实际 $($rows.Count) 条）"
    if ($rows.Count -ge 1) {
        Assert-True ($rows[0].content -eq $contentA) "正文逐字节一致（实际 '$($rows[0].content)'）"
        Assert-True ($rows[0].word_count -eq $contentA.Length) `
            "字数按 Unicode 标量值算（期望 $($contentA.Length)，实际 $($rows[0].word_count)）"
        Assert-True ([string]::IsNullOrEmpty($rows[0].mood)) "刚写时没有心情（实际 '$($rows[0].mood)'）"
    }
    $firstId = if ($rows.Count -ge 1) { $rows[0].id } else { '' }
    # 界面上的字数提示也要跟上
    $wordLabel = Get-Elements $window | ForEach-Object { $_.Current.Name } |
        Where-Object { $_ -eq "$($contentA.Length)字" } | Select-Object -First 1
    Assert-True ([bool]$wordLabel) "编辑器右上角显示「$($contentA.Length)字」"
    # 保存状态标签：「已保存」会一直显示——这是"自动保存真的跑了"的界面证据
    $savedLabel = Get-Elements $window | ForEach-Object { $_.Current.Name } |
        Where-Object { $_ -eq '已保存' } | Select-Object -First 1
    Assert-True ([bool]$savedLabel) '页脚保存状态显示「已保存」'

    # ================= 3/4 选心情 + 改内容：同一天 upsert（id 不变）=================
    Write-Host "`n[diary] 3/4 选心情「开心」并改写内容 → 同一天 upsert"
    $moodButton = Wait-Element -Root $window -Name '开心' -TimeoutSec 10
    Assert-True ([bool]$moodButton) '找到心情按钮「开心」'
    if ($moodButton) { Invoke-Element $moodButton | Out-Null }
    Start-Sleep -Seconds 3   # 切心情走 1500ms 防抖保存
    $afterMood = @(Read-Table 'tbl_billadm_diary_entry' | Where-Object { $_.date -eq $today }) | Select-Object -First 1
    Assert-True ([bool]$afterMood) '选心情后条目仍在'
    if ($afterMood) {
        Assert-True ($afterMood.mood -eq $moodEmoji) "心情落库为 emoji（实际 '$($afterMood.mood)'）"
        Assert-True ($afterMood.id -eq $firstId) '同一天是 upsert：id 没变'
        Assert-True ($afterMood.content -eq $contentA) '选心情没有把正文弄丢'
    }
    $afterEdit = $null
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        Set-DiaryContent -Window $window -Text $contentB | Out-Null
        Save-Now -Window $window
        $afterEdit = @(Read-Table 'tbl_billadm_diary_entry' | Where-Object { $_.date -eq $today }) | Select-Object -First 1
        if ($afterEdit -and $afterEdit.content -eq $contentB) { break }
        Write-Host "    第 $attempt 次改写没生效，重试" -ForegroundColor DarkYellow
    }
    if ($afterEdit) {
        Assert-True ($afterEdit.content -eq $contentB) "改后的正文落库（实际 '$($afterEdit.content)'）"
        Assert-True ($afterEdit.word_count -eq $contentB.Length) `
            "字数重算（期望 $($contentB.Length)，实际 $($afterEdit.word_count)）"
        Assert-True ($afterEdit.id -eq $firstId) '改内容仍是同一条（upsert，id 不变）'
        Assert-True ($afterEdit.mood -eq $moodEmoji) '改内容没有把心情弄丢'
    }

    # ================= 4/4 预览渲染 + 删除 =================
    Write-Host "`n[diary] 4/4 切到预览（Markdown 渲染）→ 删除"
    $previewToggle = Wait-Element -Root $window -Name '预览' -TimeoutSec 10
    Assert-True ([bool]$previewToggle) '找到「预览」切换'
    if ($previewToggle) { Invoke-Element $previewToggle | Out-Null }
    Start-Sleep -Seconds 1
    # Markdown 的 `# 标题` 渲染成标题元素，文本应当能在界面上读到
    $rendered = @(Get-Elements $window | ForEach-Object { $_.Current.Name } |
        Where-Object { $_ -and $_.Contains("UIA日记$stamp") })
    Assert-True ($rendered.Count -gt 0) "预览里渲染出了标题文本（UIA日记$stamp）"
    # 注：心情按钮的可访问名是 `aria-label`（「开心」），emoji 只是文本内容；
    # 而这个 Chromium 版本**不把 `aria-pressed` 暴露成 TogglePattern**（实测 n/a），
    # 左树里的 emoji 标记又在折叠的月份里 —— 所以"心情选中态"这条没有可用的界面判据，
    # 心情是否真的生效由上面的**落库断言**（mood='😊'、正文没丢、id 不变）负责。

    $deleteButton = Wait-Element -Root $window -Name '删除' -TimeoutSec 10
    Assert-True ([bool]$deleteButton) '找到页脚「删除」'
    if ($deleteButton) { Invoke-Element $deleteButton | Out-Null }
    Start-Sleep -Seconds 1
    Assert-True ([bool](Wait-Element -Root $window -Name '确认删除' -TimeoutSec 10)) '弹窗「确认删除」已打开'
    $confirmDelete = @(Find-All $window '删除' | Where-Object { -not $_.Current.IsOffscreen })
    Assert-True ($confirmDelete.Count -gt 0) '弹窗里有确认「删除」'
    if ($confirmDelete.Count -gt 0) { Invoke-Element $confirmDelete[$confirmDelete.Count - 1] | Out-Null }
    Start-Sleep -Seconds 3
    $gone = @(Read-Table 'tbl_billadm_diary_entry' | Where-Object { $_.date -eq $today })
    Assert-True ($gone.Count -eq 0) "删除后库里这一天没了（实际 $($gone.Count) 条）"
    # 编辑器回到空态（"选择左侧日期开始写作"）
    $emptyHint = Wait-Like -Root $window -Pattern '选择左侧日期开始写作' -TimeoutSec 8
    Assert-True ([bool]$emptyHint) '删除后编辑器回到空态提示'
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
    Write-Host "[diary] 失败 $($failures.Count) 项：" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "   - $_" -ForegroundColor Red }
    exit 1
}
Write-Host '[diary] 全部通过：写内容 → 心情 → 改内容（同一天 upsert）→ Markdown 预览 → 删除' -ForegroundColor Green
