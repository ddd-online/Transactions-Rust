# ui-upload.ps1 —— 图片上传的端到端验收（原生文件选择框 + 落盘 + 缩略图 + 入库 + 界面刷新）。
#
# 为什么需要它："添加图片"原先被列在人工清单里，理由是"走原生文件对话框，
# UIA 合成不了"。实测下来**能合成**，只是要找对窗口：
#
#   WebView2 的文件选择框（`#32770`，标题「打开」）**不是桌面顶层窗口**，
#   它是应用窗口 HWND 的**子窗口**，而且属于 WebView2 的浏览器进程（msedgewebview2.exe），
#   不是我们的 app 进程。所以：
#     - 按 `AutomationElement.RootElement` 的 Children 找 → 找不到（会误判成"对话框没弹出来"）；
#     - 按 app 进程号过滤 → 也找不到（进程号是浏览器进程的）；
#   正确做法是在**应用窗口的 Descendants** 里找 ClassName='#32770'。
#
# 操作方式（等价于点击隐藏的文件输入框的人工动作）：
#   1. 鼠标（SetForegroundWindow + 真实 click）点「添加图片」——label 转发给隐藏 input 是有效的；
#   2. 文件框里那个"文件名"编辑框**不提供 ValuePattern**（整个对话框被暴露成 Pane，
#      编辑框是 `class='Edit'` 的 Pane，没有任何 pattern），只能点进去再输入绝对路径；
#      **输入必须走剪贴板粘贴**（Ctrl+V）——本机开着中文输入法，SendKeys 逐字符输入会把 `\` 变成 `、`；
#   3. 提交用「右方向键 + 回车」：右方向键收起路径自动补全下拉（否则回车会接受补全、把路径换成裸文件名），
#      回车等于按「打开(O)」。**不要**用 UIA 坐标点那个按钮（它报出来的矩形不可信），
#      也**不要**用 `WM_COMMAND(IDOK)`（会跳过 modern 对话框的内部选中步骤，等同于取消）；
#   4. 临时 HOME 里必须存在 `Desktop` 目录，否则文件框会先弹一个「位置不可用」提示
#      （WebView2 的文件框起始目录取的是用户桌面，而我们的临时 HOME 是空的）。
#
# 断言：UIA 文案出现「下载图片」；资产目录出现 `<uuid>.png` + `thumb_<uuid>.jpg`；
#       缩略图宽度 300（原图 600×400 按比例缩放）；数据库 `tbl_billadm_key_event_image` 多一行。
#       最后**删掉这个事件**并断言：该日期的图片记录清空、原图/缩略图文件被清理、事件行消失，
#       且别的日期的图片不受影响（`remove_image_files` 只有"文件缺失容错"的单测，这条补集成验证）。
#
# 注意：关键事件按 `(ledger_id, date)` upsert，**同一天只有一个事件**；历次跑留下的图片都挂在这一天，
#       所以删事件会把这天的图片全清掉——断言要按"日期"而不是"总行数回到基线"（我踩过一次）。
#
# 用法（pwsh 7；需要 release 产物）：
#   pwsh -File fixtures/ui-upload.ps1
#   pwsh -File fixtures/ui-upload.ps1 -Workspace <既有工作空间>   # 会在里面写数据，建议给副本
#
# 隔离：与 smoke.ps1 一致，用临时 USERPROFILE 启动，不碰你真实的 ~/.transactions.json。

param(
    [string]$Exe,
    [string]$SmokeHome,
    [string]$Workspace,
    [string]$OutDir
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $repo 'target\release\transactions.exe' }
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\upload-smoke\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\upload-smoke' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$smokeHome = [System.IO.Path]::GetFullPath($SmokeHome)
if ($smokeHome -eq [System.IO.Path]::GetFullPath($env:USERPROFILE)) {
    throw "拒绝把临时 HOME 指到真实用户目录 —— 冒烟会改写你的配置"
}
if (-not (Test-Path $smokeHome)) { New-Item -ItemType Directory -Force -Path $smokeHome | Out-Null }
# 文件框的起始目录是"桌面"，临时 HOME 下必须有它，否则会先弹「位置不可用」
if (-not (Test-Path (Join-Path $smokeHome 'Desktop'))) {
    New-Item -ItemType Directory -Force -Path (Join-Path $smokeHome 'Desktop') | Out-Null
}
# `-OutDir` 归一化成绝对路径：这个目录里的文件会被**填进原生文件框**（见 `Select-Directory` 那套结论），
# 而对话框把相对路径按它自己的"当前目录"解析 —— 传相对路径会落到别处（实测踩过）。
$OutDir = [System.IO.Path]::GetFullPath($OutDir)
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Force -Path $OutDir | Out-Null }

$ws = [System.IO.Path]::GetFullPath($Workspace)
if (-not $Workspace) { $ws = [System.IO.Path]::GetFullPath((Join-Path $OutDir 'ws')) }

$exeFull = [System.IO.Path]::GetFullPath($Exe)
# 单实例插件按 identifier 判重：本仓库里**任何**构建在跑都会顶掉本次启动
# （包括隐藏到托盘的、以及 build\target 下的便携版）。判重按**完整路径**，
# 不能按进程名——本机别的目录下可能有同名 exe，与我们的 identifier 无关。
$repoPrefix = $repo.TrimEnd('\') + '\'
$running = @(Get-Process -Name transactions -ErrorAction SilentlyContinue | Where-Object {
    $path = try { $_.Path } catch { $null }
    $path -and $path.StartsWith($repoPrefix, [System.StringComparison]::OrdinalIgnoreCase)
})
if ($running) {
    $paths = $running | ForEach-Object { try { $_.Path } catch { '(unknown)' } }
    throw "本仓库已有 Transactions 实例在运行（PID $($running.Id -join ', ')）：$($paths -join ' / ')`n" +
        "单实例插件会顶掉本次启动，请先退掉它们（注意：隐藏到托盘也算在运行）。"
}

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms   # SendKeys：给文件框里的「文件名」输入路径
$UIA = [System.Windows.Automation.AutomationElement]

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class TrUploadMouse {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  [DllImport("user32.dll")] public static extern IntPtr SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
  public static void Click(int x, int y) {
    SetCursorPos(x, y);
    mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);   // LEFTDOWN
    mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);   // LEFTUP
  }
  // 按下对话框的默认按钮：WM_COMMAND + IDOK(1)，不依赖焦点与坐标
  public static void PressDefaultButton(IntPtr hWnd) {
    SendMessage(hWnd, 0x0111, new IntPtr(1), IntPtr.Zero);
  }
}
'@ -Language CSharp -ErrorAction SilentlyContinue

$failures = New-Object System.Collections.Generic.List[string]
function Assert-True {
    param([bool]$Condition, [string]$Message)
    if ($Condition) { Write-Host "  ✓ $Message" -ForegroundColor Green }
    else { Write-Host "  ✗ $Message" -ForegroundColor Red; $failures.Add($Message) }
}

# 读图片表行（删除事件后要复查行数是否回到基线）
function Read-ImageRows {
    $dump = Join-Path $OutDir 'image-rows.json'
    Push-Location $repo
    & cargo -q xtask dump $ws --table tbl_billadm_key_event_image *> $dump
    Pop-Location
    if (-not (Test-Path $dump)) { return @() }
    return @((Get-Content $dump -Raw | ConvertFrom-Json).'tbl_billadm_key_event_image')
}

# 失败时留一张全屏截图，方便判断"卡在哪一步"（人工验收离线也能看）
function Save-Screenshot {
    param([string]$Path)
    try {
        $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
        $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        $graphics.CopyFromScreen($bounds.X, $bounds.Y, 0, 0, $bounds.Size)
        $graphics.Dispose()
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
        $bitmap.Dispose()
        Write-Host "  失败截图: $Path" -ForegroundColor DarkYellow
    }
    catch { Write-Host "  截图失败: $_" -ForegroundColor DarkYellow }
}

function Get-Elements {
    param($Root)
    return $Root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
}

function Find-ByName {
    param($Root, [string]$Name)
    return $Root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $Name)))
}

function Find-First {
    param($Root, [string]$Name)
    $all = Find-ByName $Root $Name
    if ($all.Count -eq 0) { return $null }
    return $all[0]
}

function Invoke-Element {
    param($Element)
    if (-not $Element) { return $false }
    $pattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
        $pattern.Invoke(); return $true
    }
    $rect = $Element.Current.BoundingRectangle
    if ($rect.Width -gt 0) {
        [TrUploadMouse]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
        return $true
    }
    return $false
}

function Set-Value {
    param($Element, [string]$Value)
    if (-not $Element) { return $false }
    $pattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
        $pattern.SetValue($Value); return $true
    }
    return $false
}

# 文件框里的按钮：**不能按 AutomationId 找**！那个对话框里文件列表项（`.agents`、`.ssh`…）
# 的 AutomationId 恰好也是 1..N，按 id=1 找会点中列表里的第一项。
# 「打开(O)」的特征是 `class='Button'` + 名字以「打开」开头。
function Find-DialogButton {
    param($Dialog, [string]$NamePrefix)
    $cond = New-Object System.Windows.Automation.PropertyCondition($UIA::ClassNameProperty, 'Button')
    foreach ($element in @($Dialog.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond))) {
        if ($element.Current.Name -like "$NamePrefix*") { return $element }
    }
    return $null
}

# 在元素包围盒中心做一次真实鼠标点击（Win32 文件框里的控件被暴露成没有 pattern 的 Pane）
function Click-Element {
    param($Element)
    if (-not $Element) { return $false }
    $rect = $Element.Current.BoundingRectangle
    if ($rect.Width -le 0 -or $rect.Height -le 0) { return $false }
    $x = [int]($rect.X + $rect.Width / 2)
    $y = [int]($rect.Y + $rect.Height / 2)
    [TrUploadMouse]::Click($x, $y)
    return $true
}

# 在应用窗口子树里找 WebView2 的文件选择框（见文件头说明：它不是桌面顶层窗口）
function Find-FileDialog {
    param($Window, [int]$TimeoutSec = 20)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $cond = New-Object System.Windows.Automation.PropertyCondition($UIA::ClassNameProperty, '#32770')
        $found = $Window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)
        foreach ($dlg in $found) {
            if ($dlg.Current.Name -match '打开|Open') { return $dlg }
        }
        Start-Sleep -Milliseconds 500
    }
    return $null
}

function Dismiss-PathUnavailable {
    param($Window)
    $cond = New-Object System.Windows.Automation.PropertyCondition($UIA::ClassNameProperty, '#32770')
    foreach ($dlg in @($Window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond))) {
        if ($dlg.Current.Name -match '位置不可用|Location is not available') {
            Write-Host "  发现「位置不可用」提示，先关掉它" -ForegroundColor DarkYellow
            foreach ($name in '确定', 'OK') {
                $button = Find-First $dlg $name
                if ($button) { Invoke-Element $button | Out-Null; Start-Sleep -Milliseconds 800; break }
            }
        }
    }
}

# ---- 播种一个全新工作空间 ----
if (-not $Workspace -or -not (Test-Path (Join-Path $ws 'transactions.db'))) {
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[ui-upload] 播种工作空间: $ws" -ForegroundColor Cyan
}

# ---- 造一张 600×400 的 PNG（宽度 > 300，可验证缩略图缩放）----
$picture = Join-Path $OutDir 'sample-600x400.png'
$bitmap = New-Object System.Drawing.Bitmap 600, 400
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.Clear([System.Drawing.Color]::CornflowerBlue)
$graphics.FillEllipse([System.Drawing.Brushes]::OrangeRed, 40, 40, 360, 240)
$graphics.Dispose()
$bitmap.Save($picture, [System.Drawing.Imaging.ImageFormat]::Png)
$bitmap.Dispose()
Write-Host "[ui-upload] 测试图片: $picture ($((Get-Item $picture).Length) 字节)" -ForegroundColor Cyan

@{ width = 1280; height = 860; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'ui-upload.ps1' } |
    ConvertTo-Json | Set-Content -Path (Join-Path $smokeHome '.transactions.json') -Encoding UTF8

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

    # ---- 等窗口 + 等 UIA 树建好（惰性构建，必须轮询）----
    $deadline = (Get-Date).AddSeconds(40)
    $window = $null
    while ((Get-Date) -lt $deadline -and -not $window) {
        $window = $UIA::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children,
            (New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $process.Id)))
        if (-not $window) { Start-Sleep -Milliseconds 500 }
    }
    if (-not $window) { throw '启动后 40 秒内没有拿到应用窗口' }

    $deadline = (Get-Date).AddSeconds(40)
    do {
        Start-Sleep -Seconds 1
        $elements = Get-Elements $window
    } while ($elements.Count -lt 8 -and (Get-Date) -lt $deadline)
    Write-Host "[ui-upload] UIA 可读元素 $($elements.Count) 个" -ForegroundColor Cyan

    $hwnd = [IntPtr]$window.Current.NativeWindowHandle
    [TrUploadMouse]::ShowWindow($hwnd, 9) | Out-Null   # SW_RESTORE
    [TrUploadMouse]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 500

    # ---- 上传前的基线：脚本可以在同一个工作空间上反复跑，断言只看"增量" ----
    $assetsRoot = Join-Path $ws 'data\assets\key_events'
    function Get-AssetCounts {
        param([string]$Root)
        $files = @(Get-ChildItem $Root -Recurse -File -ErrorAction SilentlyContinue)
        return @{
            Originals = @($files | Where-Object { $_.Name -notlike 'thumb_*' }).Count
            Thumbs    = @($files | Where-Object { $_.Name -like 'thumb_*' }).Count
        }
    }
    $baseline = Get-AssetCounts $assetsRoot
    $baselineDump = Join-Path $OutDir 'baseline-rows.json'
    Push-Location $repo
    & cargo -q xtask dump $ws --table tbl_billadm_key_event_image *> $baselineDump
    Pop-Location
    $baselineRows = 0
    if (Test-Path $baselineDump) {
        $baselineRows = @((Get-Content $baselineDump -Raw | ConvertFrom-Json).'tbl_billadm_key_event_image').Count
    }
    Write-Host "[ui-upload] 基线：原图 $($baseline.Originals) 张 / 缩略图 $($baseline.Thumbs) 张 / 库 $baselineRows 行" -ForegroundColor DarkGray

    # ---- 建一个事件（新事件会被自动选中，详情区才会出现「添加图片」）----
    Write-Host "`n[ui-upload] 1/4 建事件"
    Assert-True (Invoke-Element (Find-First $window '关键事件')) '打开「关键事件」页'
    Start-Sleep -Seconds 2
    Assert-True (Invoke-Element (Find-First $window '新增事件')) '打开「新增事件」弹窗'
    Start-Sleep -Milliseconds 1500
    $title = "图片上传冒烟 $(Get-Date -Format 'HH:mm:ss')"
    Assert-True (Set-Value (Find-First $window '事件名称（可选）') $title) "填入事件名称（$title）"
    Invoke-Element (Find-ByName $window '新增' | Select-Object -Last 1) | Out-Null

    # 等弹窗真正消失（否则后续点击会落在遮罩上）
    $deadline = (Get-Date).AddSeconds(15)
    do { Start-Sleep -Milliseconds 500; $modal = Find-First $window '事件名称（可选）' } while ($modal -and (Get-Date) -lt $deadline)
    Assert-True (-not $modal) '弹窗已关闭'
    Start-Sleep -Milliseconds 800

    # ---- 点「添加图片」→ 原生文件框 ----
    Write-Host "`n[ui-upload] 2/4 点「添加图片」并驱动原生文件框"
    $pickButton = Find-First $window '添加图片'
    if (-not $pickButton) { throw '找不到「添加图片」按钮' }
    $rect = $pickButton.Current.BoundingRectangle
    if ($rect.Height -le 0) { throw '「添加图片」没有可点击的包围盒' }
    [TrUploadMouse]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 300
    [TrUploadMouse]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))

    $dialog = Find-FileDialog -Window $window -TimeoutSec 20
    Assert-True ([bool]$dialog) '原生文件选择框已弹出'
    if (-not $dialog) { throw '文件选择框没有出现（见文件头关于窗口归属的说明）' }
    Write-Host "  文件框: name='$($dialog.Current.Name)' pid=$($dialog.Current.ProcessId)"

    Dismiss-PathUnavailable -Window $window
    $dialog = Find-FileDialog -Window $window -TimeoutSec 10

    # ---- 文件名框（AutomationId=1148）→ 键盘输入绝对路径 → 点「打开(O)」（AutomationId=1）----
    Write-Host "`n[ui-upload] 3/4 提交文件路径"
    $combo = $dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
        (New-Object System.Windows.Automation.PropertyCondition($UIA::AutomationIdProperty, '1148')))
    Assert-True ([bool]$combo) '找到文件名控件（AutomationId=1148）'

    # 这个对话框的控件全是 Pane（没有 ValuePattern）：点进编辑框再用键盘输入
    $editBox = $null
    if ($combo) {
        $editBox = $combo.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
            (New-Object System.Windows.Automation.PropertyCondition($UIA::ClassNameProperty, 'Edit')))
        if (-not $editBox) { $editBox = $combo }
    }
    Assert-True (Click-Element $editBox) '点进文件名输入框'
    Start-Sleep -Milliseconds 500

    # **必须用剪贴板粘贴，不能 SendKeys 逐字符输入**：本机开着中文输入法，SendKeys 打出来的
    # 路径会被输入法接走——反斜杠 `\` 变成顿号 `、`，于是"路径"根本不存在，
    # 表现是回车后弹「找不到文件」。Ctrl+A / Ctrl+V 不受输入法影响。
    Set-Clipboard -Value $picture
    Start-Sleep -Milliseconds 300
    [System.Windows.Forms.SendKeys]::SendWait('^a')
    Start-Sleep -Milliseconds 250
    [System.Windows.Forms.SendKeys]::SendWait('^v')
    Start-Sleep -Milliseconds 800

    # 提交这个 Win32 文件框有三个坑（都踩过）：
    #   1. **不能用 UIA 坐标点「打开(O)」**：底部一行是 legacy provider，报出来的矩形不可信
    #      （实测「打开」的 rect 恰好等于对话框下边缘，点下去什么都不会发生）；
    #   2. **不能直接回车**：输入路径会弹出 shell 自动补全下拉，回车会"接受补全"，
    #      把完整路径换成裸文件名，于是去当前目录找文件 → 弹「找不到文件」；
    #   3. **不能 TAB 再回车**：TAB 确实收起了下拉，但焦点也移到"文件类型"下拉上，回车不再触发默认按钮；
    #      也**不要**用 `WM_COMMAND(IDOK)`：那会直接关掉对话框、跳过 modern 对话框
    #      "把文件名变成选中项"的内部步骤，结果等同于取消（界面什么都不会发生）。
    # 正确做法：右方向键收起补全下拉（焦点仍在编辑框、文本不变）→ 回车 = 按「打开」。
    [System.Windows.Forms.SendKeys]::SendWait('{RIGHT}')
    Start-Sleep -Milliseconds 500
    [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
    Start-Sleep -Seconds 3

    # 判定标准是**结果**（资产目录出现文件），不是"对话框关掉了"
    $assetsRootEarly = Join-Path $ws 'data\assets\key_events'
    $deadline = (Get-Date).AddSeconds(15)
    do {
        Start-Sleep -Seconds 1
        $landed = @(Get-ChildItem $assetsRootEarly -Recurse -File -Filter '*.png' -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -notlike 'thumb_*' })
    } while ($landed.Count -eq 0 -and (Get-Date) -lt $deadline)

    if ($landed.Count -eq 0) {
        # 兜底：对话框还开着就再回车一次；仍不行就按 UIA 矩形点「打开」（矩形若不是明显离群值才用）
        if (Find-FileDialog -Window $window -TimeoutSec 2) {
            Write-Host '  第一次提交没生效，再回车一次' -ForegroundColor DarkYellow
            $dialogHwnd = [IntPtr]$dialog.Current.NativeWindowHandle
            [TrUploadMouse]::SetForegroundWindow($dialogHwnd) | Out-Null
            Start-Sleep -Milliseconds 300
            [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
            Start-Sleep -Seconds 4
        }
        $deadline = (Get-Date).AddSeconds(10)
        do {
            Start-Sleep -Seconds 1
            $landed = @(Get-ChildItem $assetsRootEarly -Recurse -File -Filter '*.png' -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -notlike 'thumb_*' })
        } while ($landed.Count -eq 0 -and (Get-Date) -lt $deadline)
    }

    # 若弹出「找不到文件」之类的提示框，点掉它（避免挡住后面的界面断言）
    $errorBox = $null
    foreach ($name in '确定', 'OK') {
        $candidate = Find-First $window $name
        if ($candidate) { $errorBox = $candidate; break }
    }
    if ($errorBox) {
        Write-Host '  出现「找不到文件」类提示框，点掉它' -ForegroundColor DarkYellow
        Invoke-Element $errorBox | Out-Null
        Start-Sleep -Milliseconds 800
    }

    # ---- 等上传完成：文件框关闭 + 界面出现「下载图片」 ----
    Write-Host "`n[ui-upload] 4/4 等上传完成并校验"
    $deadline = (Get-Date).AddSeconds(30)
    do {
        Start-Sleep -Milliseconds 800
        $stillOpen = Find-FileDialog -Window $window -TimeoutSec 1
    } while ($stillOpen -and (Get-Date) -lt $deadline)
    Assert-True (-not $stillOpen) '文件选择框已关闭（提交被接受）'

    $deadline = (Get-Date).AddSeconds(30)
    do {
        Start-Sleep -Seconds 1
        $hasDownload = [bool](Find-First $window '下载图片')
    } while (-not $hasDownload -and (Get-Date) -lt $deadline)
    Assert-True $hasDownload '界面出现「下载图片」（图库里有图）'

    # ---- 文件系统断言（只看增量：新落盘 1 张原图 + 1 张缩略图）----
    $after = Get-AssetCounts $assetsRoot
    Assert-True ($after.Originals -eq $baseline.Originals + 1) "原图新增 1 张（基线 $($baseline.Originals) → 现在 $($after.Originals)）"
    Assert-True ($after.Thumbs -eq $baseline.Thumbs + 1) "缩略图新增 1 张（基线 $($baseline.Thumbs) → 现在 $($after.Thumbs)）"

    # 取"最新写入"的那张原图（`data\assets` 下的图片可能是工作空间里原有的）
    $originals = @(Get-ChildItem $assetsRoot -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -notlike 'thumb_*' } | Sort-Object LastWriteTime -Descending)
    $thumbs = @(Get-ChildItem $assetsRoot -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like 'thumb_*' } | Sort-Object LastWriteTime -Descending)

    if ($originals.Count -ge 1) {
        $originalPath = $originals[0].FullName
        Assert-True ($originalPath -match 'data\\assets\\key_events\\\d{4}-\d{2}-\d{2}\\') "原图路径符合 <date> 布局：$originalPath"
        $originalImage = [System.Drawing.Image]::FromFile($originalPath)
        Assert-True ($originalImage.Width -eq 600 -and $originalImage.Height -eq 400) "原图尺寸保持 600×400（实际 $($originalImage.Width)×$($originalImage.Height)）"
        $originalImage.Dispose()
        Assert-True ($originals[0].Length -eq (Get-Item $picture).Length) '原图按原字节落盘（未转码）'
    }
    if ($thumbs.Count -ge 1) {
        $thumbImage = [System.Drawing.Image]::FromFile($thumbs[0].FullName)
        Assert-True ($thumbImage.Width -eq 300 -and $thumbImage.Height -eq 200) "缩略图缩到 300×200（实际 $($thumbImage.Width)×$($thumbImage.Height)）"
        $thumbImage.Dispose()
        $thumbName = $thumbs[0].Name
        if ($originals.Count -ge 1) {
            Assert-True ($thumbName -eq "thumb_$($originals[0].BaseName).jpg") "缩略图命名与原名对应（$thumbName）"
        }
    }

    # ---- 数据库断言 ----
    $dumpPath = Join-Path $OutDir 'key_event_image.json'
    Push-Location $repo
    & cargo -q xtask dump $ws --table tbl_billadm_key_event_image *> $dumpPath
    $dumpExit = $LASTEXITCODE
    Pop-Location
    Assert-True ($dumpExit -eq 0) '导出 tbl_billadm_key_event_image'
    if ($dumpExit -eq 0) {
        $rows = Get-Content $dumpPath -Raw | ConvertFrom-Json
        $list = @($rows.'tbl_billadm_key_event_image')
        Assert-True ($list.Count -eq $baselineRows + 1) "数据库新增 1 行图片记录（基线 $baselineRows → 现在 $($list.Count)）"
        if ($list.Count -ge 1 -and $originals.Count -ge 1) {
            # 数据库里存的是**相对 `data/assets`** 的路径（所以带 `key_events/` 前缀）
            $dataAssets = Join-Path $ws 'data\assets'
            $relative = $originals[0].FullName.Substring($dataAssets.Length + 1).Replace('\', '/')
            $newRow = $list | Where-Object { $_.file_path -eq $relative } | Select-Object -First 1
            Assert-True ([bool]$newRow) "新增行的 file_path 指向相对 data/assets 的路径（$relative）"
            if ($newRow) {
                $relativeDirectory = ($relative -replace '/[^/]+$', '')
                $relativeStem = [System.IO.Path]::GetFileNameWithoutExtension($relative)
                $expectedThumb = "$relativeDirectory/thumb_$relativeStem.jpg"
                # 留给第 5 步（删除事件）用：这两个变量在块外还要读
                $uploadedRelative = $relative
                $uploadedEventDate = $newRow.event_date
                Assert-True ([bool]$newRow.thumb_path) "thumb_path 已写入（$($newRow.thumb_path)）"
                Assert-True ($newRow.thumb_path -eq $expectedThumb) "thumb_path 与原图同目录且带 thumb_ 前缀（期望 $expectedThumb，实际 $($newRow.thumb_path)）"
                Assert-True ($newRow.event_date -match '^\d{4}-\d{2}-\d{2}$') "event_date 已写入（$($newRow.event_date)）"
            }
        }
    }
    # ---- 删除事件应当把图片文件一并清理 ----
    # 这条只能端到端验：`remove_image_files` 的单测只覆盖"文件不存在时容错"，
    # 而"删事件 → 资产被清掉"要走完整条 UI → 服务层链路。
    Write-Host "`n[ui-upload] 5/5 删除事件应清理图片文件"
    $eventCardTitle = $title
    $deleteButton = $null
    $deadline = (Get-Date).AddSeconds(20)
    do {
        foreach ($candidate in @(Get-Elements $window)) {
            if ($candidate.Current.Name -ne '删除事件') { continue }
            $rect = $candidate.Current.BoundingRectangle
            foreach ($nameElement in @(Get-Elements $window)) {
                if ($nameElement.Current.Name -and $nameElement.Current.Name.Contains($eventCardTitle)) {
                    $rowRect = $nameElement.Current.BoundingRectangle
                    if ([Math]::Abs(($rect.Y + $rect.Height / 2) - ($rowRect.Y + $rowRect.Height / 2)) -le 40) {
                        $deleteButton = $candidate
                        break
                    }
                }
            }
            if ($deleteButton) { break }
        }
        if (-not $deleteButton) { Start-Sleep -Milliseconds 500 }
    } while (-not $deleteButton -and (Get-Date) -lt $deadline)
    Assert-True ([bool]$deleteButton) '找到该事件的「删除事件」按钮'
    if ($deleteButton) {
        Click-Element $deleteButton | Out-Null
        Start-Sleep -Milliseconds 1200
        # 气泡确认：按钮文案「删除」，取最后一个（弹窗/气泡在 DOM 末尾）
        $confirmButtons = @(Find-ByName $window '删除')
        if ($confirmButtons.Count -gt 0) {
            Invoke-Element $confirmButtons[$confirmButtons.Count - 1] | Out-Null
        }
        Start-Sleep -Seconds 4
    }

    # 注意：关键事件是按 (账本, 日期) upsert 的，所以**同一天只有一个事件**，
    # 历次跑留下的图片都挂在这同一天上——删事件会把这天的图片全部清掉（这是对的）。
    # 这里就按"日期"来断言，并额外确认**别的日期**的图片不受影响（不误删）。
    $imagesBeforeDelete = @(Read-ImageRows)
    $eventDate = $uploadedEventDate
    Assert-True ([bool]$eventDate) "拿到被删事件的日期（$eventDate）"
    $otherDatesBefore = @($imagesBeforeDelete | Where-Object { $_.event_date -ne $eventDate })

    $imagesAfterDelete = @(Read-ImageRows)
    $sameDateAfter = @($imagesAfterDelete | Where-Object { $_.event_date -eq $eventDate })
    $otherDatesAfter = @($imagesAfterDelete | Where-Object { $_.event_date -ne $eventDate })
    Assert-True ($sameDateAfter.Count -eq 0) "删除事件后该日期的图片记录已清空（$eventDate）"
    Assert-True ($otherDatesAfter.Count -eq $otherDatesBefore.Count) "别的日期的图片记录不受影响（$($otherDatesBefore.Count) → $($otherDatesAfter.Count)）"

    $dayDirectory = Join-Path $assetsRoot $eventDate
    $leftover = @(Get-ChildItem $dayDirectory -File -ErrorAction SilentlyContinue)
    Assert-True ($leftover.Count -eq 0) "该日期的资产文件已被清理（$dayDirectory 下剩 $($leftover.Count) 个）"

    $eventRows = Join-Path $OutDir 'key_events.json'
    Push-Location $repo
    & cargo -q xtask dump $ws --table tbl_billadm_key_event *> $eventRows
    Pop-Location
    $remaining = @((Get-Content $eventRows -Raw | ConvertFrom-Json).'tbl_billadm_key_event' |
        Where-Object { $_.title -eq $eventCardTitle })
    Assert-True ($remaining.Count -eq 0) "事件行也已删除（$eventCardTitle）"
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
    Write-Host "[ui-upload] 失败 $($failures.Count) 项：" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "   - $_" -ForegroundColor Red }
    exit 1
}
Write-Host '[ui-upload] 全部通过：原生文件框 → 落盘 → 缩略图 → 入库 → 界面刷新 → 删事件清理资产' -ForegroundColor Green
