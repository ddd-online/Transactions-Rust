# ui-diary-io.ps1 —— 日记「选择目录导入 / 导出」的端到端验收（真实原生选目录对话框）。
#
# 背景：`docs/ACCEPTANCE.md` 把日记导入导出列为人工项（"走原生对话框"）。数据链路早已有单测
# （`cargo test -p tr-service diary::`：编码回退链、导出→扫描→导入逐字节还原），
# 这里补上**最后一段**：真的点按钮 → 真的弹出选目录框 → 真的选目录 → 落库 / 落盘。
#
# 选目录框与选文件框是同一个 Win32 对话框（`#32770`），因此沿用 `fixtures/ui-upload.ps1` 的结论：
#   * 它是**应用窗口的子窗口**、属于 WebView2 浏览器进程 → 在应用窗口 Descendants 里按 ClassName 找；
#   * 路径必须**走剪贴板**（中文输入法会把 `\` 变成 `、`）；
#   * 提交用**右方向键 + 回车**（直接回车会接受 shell 自动补全，把完整路径换成裸名字）。
#
# 用法（pwsh 7；需要 release 产物；本仓库不能有实例在跑）：
#   pwsh -File fixtures/ui-diary-io.ps1 [-Workspace <ws>] [-OutDir <dir>]

param(
    [string]$Exe,
    [string]$SmokeHome,
    [string]$Workspace,
    [string]$OutDir
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $repo 'target\release\transactions.exe' }
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\diary-smoke\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\diary-smoke' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$smokeHome = [System.IO.Path]::GetFullPath($SmokeHome)
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
public class TrDiaryMouse {
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
function Invoke-Element { param($Element)
    if (-not $Element) { return $false }
    $pattern = $null
    if ($Element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
        $pattern.Invoke(); return $true
    }
    $rect = $Element.Current.BoundingRectangle
    if ($rect.Width -gt 0) {
        [TrDiaryMouse]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
        return $true
    }
    return $false
}
function Click-Element { param($Element)
    if (-not $Element) { return $false }
    $rect = $Element.Current.BoundingRectangle
    if ($rect.Width -le 0) { return $false }
    [TrDiaryMouse]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
    return $true
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
        Write-Host "  失败截图: $Path" -ForegroundColor DarkYellow
    }
    catch { Write-Host "  截图失败: $_" -ForegroundColor DarkYellow }
}

# 选目录框去哪儿找？**两种原生对话框的归属不一样**（都实测过）：
#   * WebView2 自己的文件框（`<input type=file>`，见 `ui-upload.ps1`）：
#     **应用窗口的子窗口**，`ProcessId` 属于 msedgewebview2.exe → 在应用窗口 Descendants 里找；
#   * Tauri `dialog_open` 插件的选目录/选文件框（本条脚本）：
#     **桌面顶层窗口**，`ProcessId` 就是应用自己 → 在桌面 Children 里按 ClassName='#32770' + 进程号找。
# 找错地方的表现都是"对话框好像没弹出来"。
function Find-FolderDialog { param([int]$ProcessId, [int]$TimeoutSec = 20)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $cond = New-Object System.Windows.Automation.PropertyCondition($UIA::ClassNameProperty, '#32770')
        foreach ($dialog in @($UIA::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $cond))) {
            if ($dialog.Current.ProcessId -eq $ProcessId) { return $dialog }
        }
        Start-Sleep -Milliseconds 500
    }
    return $null
}

# 在选目录框里挑一个目录：把绝对路径粘进"文件夹:"框 → 右方向键收起补全 → 回车
function Select-Directory {
    param($Dialog, [string]$Directory, [int]$ProcessId)
    # 路径输入框的 AutomationId 与对话框种类有关（都实测过）：
    #   选**文件**框 = 1148（`文件名(N):` 组合框）；选**目录**框 = 1152（`文件夹(F):` 编辑框）。
    # 依次尝试，最后退化成"任意一个 class='Edit' 的后代"。
    $pathBox = $null
    foreach ($automationId in '1152', '1148') {
        $pathBox = $Dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
            (New-Object System.Windows.Automation.PropertyCondition($UIA::AutomationIdProperty, $automationId)))
        if ($pathBox) { break }
    }
    if (-not $pathBox) {
        foreach ($element in @($Dialog.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition))) {
            if ($element.Current.ClassName -eq 'Edit') { $pathBox = $element; break }
        }
    }
    if (-not $pathBox) { throw '选目录框里找不到路径输入框（试过 1152 / 1148 / class=Edit）' }

    $dialogHwnd = [IntPtr]$Dialog.Current.NativeWindowHandle
    [TrDiaryMouse]::SetForegroundWindow($dialogHwnd) | Out-Null
    Start-Sleep -Milliseconds 400
    Click-Element $pathBox | Out-Null
    Start-Sleep -Milliseconds 500

    Set-Clipboard -Value $Directory
    Start-Sleep -Milliseconds 300
    [System.Windows.Forms.SendKeys]::SendWait('^a')
    Start-Sleep -Milliseconds 250
    [System.Windows.Forms.SendKeys]::SendWait('^v')
    Start-Sleep -Milliseconds 700
    [System.Windows.Forms.SendKeys]::SendWait('{RIGHT}')      # 收起自动补全下拉，保留完整路径
    Start-Sleep -Milliseconds 400

    # 选**目录**时回车只是"进入/选中"该目录，必须点「选择文件夹」才会返回；
    # （选**文件**框相反：回车就等于按「打开」。）
    $confirm = $null
    foreach ($element in @($Dialog.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition))) {
        if ($element.Current.ClassName -ne 'Button') { continue }
        if ($element.Current.Name -match '选择文件夹|选择|打开|确定') { $confirm = $element; break }
    }
    if ($confirm) { Click-Element $confirm | Out-Null }
    else { [System.Windows.Forms.SendKeys]::SendWait('{ENTER}') }
    Start-Sleep -Seconds 3

    # 路径不存在时 Windows 会再弹一个提示框：点掉它，让调用方看到"没提交成功"
    foreach ($tip in @($UIA::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,
            (New-Object System.Windows.Automation.PropertyCondition($UIA::ClassNameProperty, '#32770'))))) {
        if ($tip.Current.ProcessId -ne $ProcessId) { continue }
        foreach ($element in @($tip.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition))) {
            if ($element.Current.ClassName -eq 'Button' -and $element.Current.Name -match '确定|OK') {
                Write-Host "  选目录被拒（提示框：$($tip.Current.Name)）" -ForegroundColor DarkYellow
                Click-Element $element | Out-Null
                Start-Sleep -Milliseconds 600
                break
            }
        }
    }

    return -not (Find-FolderDialog -ProcessId $ProcessId -TimeoutSec 2)
}

# 后端 `char_count` 数的是 Unicode **标量值**（Rust `chars().count()`），
# 而 .NET 的 String.Length 数的是 UTF-16 码元（emoji 会被算成 2）——这里按标量值数。
function Get-CharCount { param([string]$Text)
    return ($Text -replace '[\uD800-\uDBFF][\uDC00-\uDFFF]', 'x').Length
}

function Read-DiaryRows {
    $dump = Join-Path $OutDir 'diary-entry.json'
    Push-Location $repo
    & cargo -q xtask dump $ws --table tbl_billadm_diary_entry *> $dump
    $exit = $LASTEXITCODE
    Pop-Location
    if ($exit -ne 0) { throw "导出日记表失败（exit=$exit）" }
    return @((Get-Content $dump -Raw | ConvertFrom-Json).'tbl_billadm_diary_entry' | Sort-Object date)
}

# ---- 播种 ----
if (-not $Workspace -or -not (Test-Path (Join-Path $ws 'transactions.db'))) {
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[diary] 播种工作空间: $ws" -ForegroundColor Cyan
}

# ---- 准备待导入的目录：三种编码 + 多行正文，覆盖面尽量宽 ----
$importDir = Join-Path $OutDir 'import-src'
if (Test-Path $importDir) { Remove-Item $importDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $importDir | Out-Null
$utf8Content = "导入的第一行`n导入的第二行 🙂"
$utf8Path = Join-Path $importDir '2027-03-01.md'
[System.IO.File]::WriteAllText($utf8Path, $utf8Content, (New-Object System.Text.UTF8Encoding($false)))
$gbkPath = Join-Path $importDir '2027-03-02.txt'
[System.IO.File]::WriteAllBytes($gbkPath, [System.Text.Encoding]::GetEncoding('GB18030').GetBytes('GBK 编码的日记'))
# 文件名不合法 → 必须被扫描跳过（不报错、不入库）
[System.IO.File]::WriteAllText((Join-Path $importDir 'not-a-date.md'), 'should be skipped', (New-Object System.Text.UTF8Encoding($false)))
Write-Host "[diary] 导入源目录: $importDir" -ForegroundColor Cyan

$exportDir = Join-Path $OutDir 'export-out'
if (Test-Path $exportDir) { Remove-Item $exportDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $exportDir | Out-Null   # 选目录框只接受**已存在**的目录

@{ width = 1280; height = 860; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'ui-diary-io.ps1' } |
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

    $deadline = (Get-Date).AddSeconds(40)
    $window = $null
    while ((Get-Date) -lt $deadline -and -not $window) {
        $window = $UIA::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children,
            (New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $process.Id)))
        if (-not $window) { Start-Sleep -Milliseconds 500 }
    }
    if (-not $window) { throw '启动后 40 秒内没有拿到应用窗口' }

    $deadline = (Get-Date).AddSeconds(40)
    do { Start-Sleep -Seconds 1; $elements = Get-Elements $window } while ($elements.Count -lt 8 -and (Get-Date) -lt $deadline)
    Write-Host "[diary] UIA 可读元素 $($elements.Count) 个" -ForegroundColor Cyan

    $hwnd = [IntPtr]$window.Current.NativeWindowHandle
    [TrDiaryMouse]::ShowWindow($hwnd, 9) | Out-Null
    [TrDiaryMouse]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 500

    Write-Host "`n[diary] 1/2 打开「应用设置 → 日记配置」并点「选择目录导入」"
    Assert-True (Invoke-Element (Find-First $window '应用设置')) '打开「应用设置」'
    Start-Sleep -Seconds 2
    Assert-True (Invoke-Element (Find-First $window '日记配置')) '切到「日记配置」页签'
    Start-Sleep -Seconds 2

    $importButton = Find-First $window '选择目录导入'
    Assert-True ([bool]$importButton) '找到「选择目录导入」按钮'
    if (-not $importButton) { throw '找不到「选择目录导入」按钮' }
    $rect = $importButton.Current.BoundingRectangle
    [TrDiaryMouse]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 300
    [TrDiaryMouse]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))

    $before = Read-DiaryRows
    Write-Host "  导入前日记行数: $($before.Count)"

    $dialog = Find-FolderDialog -ProcessId $process.Id -TimeoutSec 20
    Assert-True ([bool]$dialog) '原生选目录对话框已弹出'
    if (-not $dialog) { throw '选目录对话框没有出现' }
    Write-Host "  对话框: name='$($dialog.Current.Name)'"

    Assert-True (Select-Directory -Dialog $dialog -Directory $importDir -ProcessId $process.Id) '选目录框已关闭（提交被接受）'

    # 导入是异步的：轮询库里的行数
    $expectedDates = @('2027-03-01', '2027-03-02')
    $deadline = (Get-Date).AddSeconds(30)
    do {
        Start-Sleep -Seconds 1
        $after = Read-DiaryRows
        $dates = @($after | ForEach-Object { $_.date })
    } while ((-not ($expectedDates | Where-Object { $dates -notcontains $_ })) -eq $false -and (Get-Date) -lt $deadline)

    $imported = @($after | Where-Object { $expectedDates -contains $_.date })
    Assert-True ($imported.Count -eq 2) "库里新增 2 篇日记（实际 $($imported.Count)）"
    if ($imported.Count -eq 2) {
        $md = $imported | Where-Object { $_.date -eq '2027-03-01' }
        $txt = $imported | Where-Object { $_.date -eq '2027-03-02' }
        Assert-True ($md.content -eq $utf8Content) 'UTF-8 文件正文逐字节导入（含 emoji 与换行）'
        Assert-True ($txt.content -eq 'GBK 编码的日记') 'GBK 文件按编码回退链正确解码'
        Assert-True ($md.word_count -eq (Get-CharCount $utf8Content)) "word_count 按 Unicode 标量值计数（期望 $(Get-CharCount $utf8Content)，实际 $($md.word_count)）"
    }
    Assert-True (@($after | Where-Object { $_.content -eq 'should be skipped' }).Count -eq 0) '文件名不合法的文件被跳过（不入库、不报错）'

    Write-Host "`n[diary] 2/2 点「选择目录导出」导出到空目录"
    $beforeExport = Read-DiaryRows
    $exportButton = Find-First $window '选择目录导出'
    Assert-True ([bool]$exportButton) '找到「选择目录导出」按钮'
    if (-not $exportButton) { throw '找不到「选择目录导出」按钮' }
    $rect = $exportButton.Current.BoundingRectangle
    [TrDiaryMouse]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 300
    [TrDiaryMouse]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))

    $dialog2 = Find-FolderDialog -ProcessId $process.Id -TimeoutSec 20
    Assert-True ([bool]$dialog2) '导出时原生选目录框已弹出'
    if ($dialog2) {
        Assert-True (Select-Directory -Dialog $dialog2 -Directory $exportDir -ProcessId $process.Id) '导出目录已选定'
    }

    # 导出是异步的：轮询目录里出现的 .md 文件数
    $deadline = (Get-Date).AddSeconds(30)
    do {
        Start-Sleep -Seconds 1
        $exported = @(Get-ChildItem $exportDir -File -Filter '*.md' -ErrorAction SilentlyContinue)
    } while ($exported.Count -lt $beforeExport.Count -and (Get-Date) -lt $deadline)

    Assert-True ($exported.Count -eq $beforeExport.Count) "导出文件数 = 库里日记数（期望 $($beforeExport.Count)，实际 $($exported.Count)）"
    $roundTrip = $beforeExport | Where-Object { $_.date -eq '2027-03-01' }
    if ($roundTrip) {
        $exportedFile = Join-Path $exportDir '2027-03-01.md'
        Assert-True (Test-Path $exportedFile) '导出的文件名是 <日期>.md'
        if (Test-Path $exportedFile) {
            $content = [System.IO.File]::ReadAllText($exportedFile)
            Assert-True ($content -eq $roundTrip.content) '导出正文与库内容逐字节一致（可原样再导入）'
        }
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
    Write-Host "[diary] 失败 $($failures.Count) 项：" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "   - $_" -ForegroundColor Red }
    exit 1
}
Write-Host '[diary] 全部通过：选目录导入（UTF-8/GBK）→ 落库 → 导出 → 正文逐字节一致' -ForegroundColor Green
