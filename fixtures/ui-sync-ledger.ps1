# ui-sync-ledger.ps1 —— 「同步到其他账本」端到端（界面 + 数据库）。
#
# 为什么需要它：这条路径**此前没有任何覆盖**——IPC 命令面里也没有 `sync`（界面是
# "复制一份 DTO、换上目标账本、清空 id，再调 `tr_create`"），所以既不在单测里，也不在黄金对比里。
# 风险正好落在这类"复制但不完全复制"的地方：新 id 有没有生成、目标账本对不对、
# 价格/类型/分类有没有带过去、**源记录是否原样保留**。
#
# 用法（pwsh 7；需要 release 产物；本仓库不能有实例在跑）：
#   pwsh -File fixtures/ui-sync-ledger.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

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
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\sync-smoke\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\sync-smoke' }
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
public class TrSync {
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
        [TrSync]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
        return $true
    }
    return $false
}
function Click-Element { param($Element)
    if (-not $Element) { return $false }
    $rect = $Element.Current.BoundingRectangle
    if ($rect.Width -le 0) { return $false }
    [TrSync]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
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

# 行内按钮：名字 + 与该行文案同一水平带（表格里每行都有同名按钮）
function Find-RowButton { param($Window, [string]$RowText, [string]$ButtonName)
    $row = Wait-Like -Root $Window -Pattern $RowText -TimeoutSec 15
    if (-not $row) {
        Write-Host "    找不到行元素「$RowText」" -ForegroundColor DarkYellow
        return $null
    }
    $rowRect = $row.Current.BoundingRectangle
    [TrSync]::SetCursorPos([int]($rowRect.X + $rowRect.Width / 2), [int]($rowRect.Y + $rowRect.Height / 2)) | Out-Null
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

# ---- 播种（默认每次重播；种子里有两个账本，同步才有目标）----
if (-not $explicitWorkspace) {
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
}
if (-not (Test-Path (Join-Path $ws 'transactions.db'))) {
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[sync] 播种工作空间: $ws" -ForegroundColor Cyan
}

@{ width = 1500; height = 950; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'ui-sync-ledger.ps1' } |
    ConvertTo-Json | Set-Content -Path (Join-Path $smokeHome '.transactions.json') -Encoding UTF8

$stamp = Get-Date -Format 'HHmmss'
$description = "UIA同步源$stamp"

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
    Write-Host "[sync] UIA 可读元素 $((Get-Elements $window).Count) 个" -ForegroundColor Cyan

    $ledgers = @(Read-Table 'tbl_billadm_ledger')
    Assert-True ($ledgers.Count -ge 2) "种子里至少有 2 个账本（实际 $($ledgers.Count)）"

    # ================= 1/3 在当前账本记一笔 =================
    Write-Host "`n[sync] 1/3 在当前账本记一笔 66.66（$description）"
    Assert-True (Add-Record -Window $window -Description $description -Amount '66.66') '记一笔'

    $records = @(Read-Table 'tbl_billadm_transaction_record')
    $sourceRows = @($records | Where-Object { $_.description -eq $description })
    Assert-True ($sourceRows.Count -eq 1) "库里出现源记录（$description）"
    if ($sourceRows.Count -lt 1) { throw '没有源记录，无法继续' }
    $source = $sourceRows[0]
    $currentLedgerId = $source.ledger_id
    $currentLedger = @($ledgers | Where-Object { $_.id -eq $currentLedgerId })
    Assert-True ($currentLedger.Count -eq 1) "当前账本 = $($currentLedger[0].name)"
    $targetCandidates = @($ledgers | Where-Object { $_.id -ne $currentLedgerId })
    Assert-True ($targetCandidates.Count -ge 1) '存在"其他账本"可作为同步目标'
    $target = $targetCandidates[0]
    Write-Host "    目标账本：$($target.name)"

    # ================= 2/3 界面点「同步到其他账本」→ 选目标账本 =================
    Write-Host "`n[sync] 2/3 点该行的「同步到其他账本」并选中目标账本"
    $syncButton = Find-RowButton -Window $window -RowText $description -ButtonName '同步到其他账本'
    Assert-True ([bool]$syncButton) '找到该行的「同步到其他账本」按钮'
    if (-not $syncButton) { throw '找不到同步按钮' }
    Click-Element $syncButton | Out-Null
    Start-Sleep -Seconds 2

    # 面板里列出的是"其他账本"的名字；点它即触发同步
    $targetButton = Wait-Element -Root $window -Name $target.name -TimeoutSec 10
    Assert-True ([bool]$targetButton) "同步面板里出现目标账本「$($target.name)」"
    if ($targetButton) {
        Invoke-Element $targetButton | Out-Null
    }
    Start-Sleep -Seconds 4

    # ================= 3/3 断言：目标账本多了一份副本、源记录原样保留 =================
    Write-Host "`n[sync] 3/3 校验落库结果"
    $after = @(Read-Table 'tbl_billadm_transaction_record')
    $synced = @($after | Where-Object { $_.description -eq $description -and $_.ledger_id -eq $target.id })
    Assert-True ($synced.Count -eq 1) "目标账本里出现副本（$($target.name)）"
    if ($synced.Count -eq 1) {
        $copy = $synced[0]
        Assert-True ($copy.transaction_id -ne $source.transaction_id) '副本是新的 transaction_id（不是同一行）'
        Assert-True ($copy.price -eq $source.price) "金额一致（$($copy.price) 分）"
        Assert-True ($copy.transaction_type -eq $source.transaction_type) "类型一致（$($copy.transaction_type)）"
        Assert-True ($copy.category -eq $source.category) "分类一致（$($copy.category)）"
        Assert-True ($copy.transaction_at -eq $source.transaction_at) "记录时间一致（$($copy.transaction_at)）"
    }
    $sourceStillThere = @($after | Where-Object { $_.transaction_id -eq $source.transaction_id })
    Assert-True ($sourceStillThere.Count -eq 1) '源记录仍然存在（同步是复制不是移动）'
    Assert-True (@($after | Where-Object { $_.description -eq $description }).Count -eq 2) "同描述共 2 行（源 + 副本）"

    # 切到目标账本，界面里应该能看到这笔
    Write-Host "`n[sync] 额外：切到目标账本后界面上能看到副本"
    $ledgerSwitcher = Wait-Like -Root $window -Pattern $currentLedger[0].name -TimeoutSec 10
    Assert-True ([bool]$ledgerSwitcher) "找到账本选择器（当前显示 $($currentLedger[0].name)）"
    if ($ledgerSwitcher) {
        Invoke-Element $ledgerSwitcher | Out-Null
        Start-Sleep -Seconds 1
        $option = Wait-Element -Root $window -Name $target.name -TimeoutSec 10
        Assert-True ([bool]$option) "下拉里能选到「$($target.name)」"
        if ($option) { Invoke-Element $option | Out-Null }
        Start-Sleep -Seconds 4
        $visible = $false
        $deadline = (Get-Date).AddSeconds(15)
        do {
            $texts = @(Get-Elements $window | ForEach-Object { $_.Current.Name } | Where-Object { $_ })
            $visible = @($texts | Where-Object { $_ -like "*$description*" }).Count -gt 0
            if (-not $visible) { Start-Sleep -Milliseconds 500 }
        } while (-not $visible -and (Get-Date) -lt $deadline)
        Assert-True $visible "切到目标账本后列表里能看到 $description"
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
    Write-Host "[sync] 失败 $($failures.Count) 项：" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "   - $_" -ForegroundColor Red }
    exit 1
}
Write-Host '[sync] 全部通过：同步到其他账本 = 新 id 的副本 + 源记录保留 + 字段一致 + 界面可见' -ForegroundColor Green
