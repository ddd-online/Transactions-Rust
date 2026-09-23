# dev-hot.ps1 —— 前端热更新（改完立刻在桌面窗口里看到）
#
# 为什么需要它：
#   * `trunk serve` 自己会在重建完成后通过 WebSocket 让**浏览器**自动刷新；
#   * 但 Tauri 的 WebView2 窗口**不吃**这个信号（实测：trunk 一直在重建，窗口内容却不变）。
#   * 而 WebView2 **吃键盘刷新**（实测 `Ctrl+R` 后 0.8s 内新内容出现）。
# 所以这个脚本把两头接起来：监视 trunk 的构建日志 → 一看到 "applying new distribution"
# 就给应用窗口发一次 `Ctrl+R`。改一个字符 → 约 3~10 秒后窗口自己变。
#
# 用法（pwsh 7）：
#   pwsh -File fixtures/dev-hot.ps1                      # 只热更新：需要你已经在跑 cargo tauri dev
#   pwsh -File fixtures/dev-hot.ps1 -Launch              # 顺便拉起 dev 外壳（不启动 trunk，用现有服务）
#   pwsh -File fixtures/dev-hot.ps1 -Launch -Workspace target\smoke\ws-dev
#   pwsh -File fixtures/dev-hot.ps1 -Trunk               # 连 trunk serve 也一起管（全自动）
#   pwsh -File fixtures/dev-hot.ps1 -PollMs 300          # 盯得更勤（默认 500ms）
#
# 分工建议：
#   * **改界面（.rs / .css）** → 用本脚本，不重编外壳、不重启应用；
#   * 改 `src-tauri/`（外壳）→ 本脚本刷新没用，必须 `cargo tauri dev` 重启（那部分 Tauri 自己会重编）。
#
# 注意：
#   * `NO_COLOR=1` 会让 trunk 直接报错（`invalid value '1' for '--no-color'`），脚本会先清掉它；
#   * `-Launch` / `-Trunk` 用独立配置文件（`-SmokeHome`，默认 target\smoke\home-hot），
#     **不碰**真实的 `~/.transactions-dev.json`；
#   * 刷新会丢掉界面状态（打开的弹窗、填了一半的表单），这是整页 reload 的固有代价。

param(
    [string]$Exe,
    [string]$Workspace,
    [string]$SmokeHome,
    [string]$TrunkLog,
    [switch]$Launch,
    [switch]$Trunk,
    [int]$PollMs = 500,
    # trunk serve 的端口，必须与 crates/tr-ui/Trunk.toml 的 [serve] port 和
    # src-tauri/tauri.conf.json 的 devUrl 一致（换了就三处一起换）
    [int]$Port = 16000,
    # 给了就把每次刷新后的窗口抓成 <ShotDir>\current.png（免手动跑 dev-shot.ps1）
    [string]$ShotDir
)

$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot 'lib\TrUia.ps1')
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

if (-not $Exe) { $Exe = Join-Path $repo 'target\debug\transactions.exe' }
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\smoke\home-hot' }
if (-not $TrunkLog) { $TrunkLog = Join-Path $repo 'target\trunk-hot.log' }

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class TrDevHot {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  // 公共 P/Invoke 已统一到 fixtures/lib/TrUia.ps1 的 TrUia（本脚本的调用点用 [TrUia]::…）
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  public static void CtrlR() {
    keybd_event(0x11, 0, 0, UIntPtr.Zero);
    keybd_event(0x52, 0, 0, UIntPtr.Zero);
    System.Threading.Thread.Sleep(60);
    keybd_event(0x52, 0, 2, UIntPtr.Zero);
    keybd_event(0x11, 0, 2, UIntPtr.Zero);
  }
}
'@ -ErrorAction SilentlyContinue

# 抓当前窗口到 PNG（`-ShotDir` 时每次刷新后调用一次，省得手动跑 dev-shot.ps1）
function Save-CurrentShot {
    param($Window, [string]$Path)
    try {
        $handle = [IntPtr]$Window.Current.NativeWindowHandle
        $rect = New-Object TrDevHot+RECT
        [void][TrDevHot]::GetWindowRect($handle, [ref]$rect)
        $w = $rect.Right - $rect.Left
        $h = $rect.Bottom - $rect.Top
        if ($w -le 0 -or $h -le 0) { return }
        $bmp = New-Object System.Drawing.Bitmap($w, $h)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $hdc = $g.GetHdc()
        $ok = [TrDevHot]::PrintWindow($handle, $hdc, 2)
        $g.ReleaseHdc($hdc)
        if (-not $ok) {
            [void][TrUia]::SetForegroundWindow($handle)
            Start-Sleep -Milliseconds 300
            $g.CopyFromScreen($rect.Left, $rect.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
        }
        # ⚠ 先存临时文件再替换：直接覆盖 $Path 时，调用方（dev-shot）可能正读着那张图，
        # GDI+ 会抛 "A generic error occurred in GDI+"（实测踩过）。
        $temp = "$Path.tmp.png"
        $bmp.Save($temp, [System.Drawing.Imaging.ImageFormat]::Png)
        $g.Dispose(); $bmp.Dispose()
        Move-Item -Force $temp $Path
        Write-Host ("[dev-hot] 📷 已存图 $Path") -ForegroundColor DarkGray
    } catch {
        Write-Host "[dev-hot] 抓图失败：$($_.Exception.Message)" -ForegroundColor Yellow
    }
}

function Get-RepoAppWindow {
    # 只认**本仓库**的 transactions.exe（别的目录可能也装了同名程序）
    $procs = @(Get-Process transactions -ErrorAction SilentlyContinue | Where-Object {
            $p = try { $_.Path } catch { $null }
            $p -and $p.StartsWith($repo, [StringComparison]::OrdinalIgnoreCase)
        })
    foreach ($proc in $procs) {
        $cond = New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $proc.Id)
        foreach ($candidate in @($UIA::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $cond))) {
            # 侧栏条目「记账」出现 = 这是主窗口（而不是 600×560 的初始化窗口）
            $names = @($candidate.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                    (New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, '记账'))))
            if ($names.Count -gt 0) { return $candidate }
        }
    }
    return $null
}

# 端口能不能绑：能不能起 trunk 就看这一件事，先问清楚再起，别等 240 秒超时才报错。
function Assert-PortBindable {
    try {
        $probe = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
        $probe.Start()
        $probe.Stop()
    } catch {
        $reason = $_.Exception.InnerException.Message
        Write-Host "[dev-hot] ✗ 端口 $Port 绑不上：$reason" -ForegroundColor Red
        Write-Host '          本机 Windows 的保留端口段（WinNAT/Hyper-V）会漂，落在里面的端口非提权进程绑不了。' -ForegroundColor Yellow
        Write-Host '          查：netsh interface ipv4 show excludedportrange protocol=tcp' -ForegroundColor Yellow
        Write-Host "          解：把 Trunk.toml 的 [serve] port、tauri.conf.json 的 devUrl 与 -Port 一起换成段外的端口" -ForegroundColor Yellow
        throw "端口 $Port 不可用（os error 10013）"
    }
}

function Start-TrunkServe {
    Remove-Item Env:NO_COLOR -ErrorAction SilentlyContinue
    $listening = Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue
    if ($listening) {
        Write-Host "[dev-hot] $Port 已有服务在跑，直接用它" -ForegroundColor DarkGray
        if (-not (Test-Path $TrunkLog)) {
            Write-Host "[dev-hot] ⚠ 没找到 trunk 日志 $TrunkLog —— 那个服务不是本脚本起的，无法监视它。" -ForegroundColor Yellow
            Write-Host '          请改用：pwsh -File fixtures/dev-hot.ps1 -Trunk（由本脚本接管 trunk）' -ForegroundColor Yellow
        }
        return
    }
    Assert-PortBindable
    Write-Host '[dev-hot] 启动 trunk serve（首次编译约 1~2 分钟）…' -ForegroundColor Cyan
    Start-Process -FilePath 'trunk' -ArgumentList @('serve', '--config', 'crates/tr-ui/Trunk.toml') `
        -RedirectStandardOutput $TrunkLog -RedirectStandardError "$TrunkLog.err" -WindowStyle Hidden | Out-Null
    $deadline = (Get-Date).AddSeconds(240)
    $checked = 0
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 800
        if ((Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue)) {
            Write-Host "[dev-hot] trunk 已在 http://127.0.0.1:$Port 就绪" -ForegroundColor Green
            return
        }
        # trunk 把"构建失败"和"绑不上端口"都只写进日志然后自己退出，不主动看就会干等 240 秒
        foreach ($f in @($TrunkLog, "$TrunkLog.err")) {
            if (-not (Test-Path $f)) { continue }
            $tail = Get-Content $f -Tail 80 -ErrorAction SilentlyContinue
            if ($tail -match 'os error 10013') {
                throw "trunk 起不来：端口 $Port 被系统保留段占用（os error 10013），详见 $TrunkLog"
            }
            if ($tail -match 'invalid value .1. for .--no-color|error: could not compile') {
                throw "trunk 起不来，日志尾部见 $f（常见原因：NO_COLOR、编译错误）"
            }
        }
        if ((++$checked % 15) -eq 0) {
            Write-Host ("[dev-hot] 还在等 trunk… {0}s" -f [int]((240 - ($deadline - (Get-Date)).TotalSeconds))) -ForegroundColor DarkGray
        }
    }
    throw "trunk serve 240 秒内没起来（看 $TrunkLog）"
}

function Start-DevShell {
    if (-not (Test-Path $Exe)) { throw "找不到 dev 外壳: $Exe（先跑 cargo build -p transactions）" }
    # devUrl 是**编译期**读进外壳的（tauri.conf.json），不是运行时读的：外壳比 Trunk.toml /
    # tauri.conf.json 旧就直接 throw，否则窗口里是 ERR_CONNECTION_REFUSED，
    # 看着像界面坏了（实测踩过：trunk 在 16000 上服务着，外壳却还按旧的 1520 连）。
    $stale = @('crates\tr-ui\Trunk.toml', 'src-tauri\tauri.conf.json') | Where-Object {
        (Test-Path (Join-Path $repo $_)) -and (Get-Item (Join-Path $repo $_)).LastWriteTime -gt (Get-Item $Exe).LastWriteTime
    }
    if ($stale) {
        throw ("dev 外壳比界面配置旧（$($stale -join '、')）→ 窗口会 ERR_CONNECTION_REFUSED。" +
            "先跑：cargo build -p transactions")
    }
    New-Item -ItemType Directory -Force -Path $SmokeHome | Out-Null
    # 一次性 HOME 里必须先有 `Desktop`：`USERPROFILE` 指向这里以后，Windows 的
    # 文件夹/文件对话框（选工作空间、日记导入导出、上传图片）默认落在 `%USERPROFILE%\Desktop`，
    # 那个目录不存在时会先弹一个「位置不可用」的模态框挡住流程（实测踩过）。
    foreach ($sub in @('Desktop', 'AppData')) {
        New-Item -ItemType Directory -Force -Path (Join-Path $SmokeHome $sub) | Out-Null
    }
    $cfg = Join-Path $SmokeHome '.transactions-dev.json'
    $wsAbs = if ($Workspace) { [System.IO.Path]::GetFullPath($Workspace) } else { '' }
    if (-not $wsAbs -and (Test-Path $cfg)) {
        # 不给 -Workspace 时沿用上次那个工作空间：这份配置是**跨轮次复用**的，
        # 直接写空会把已经配好的工作空间丢掉 —— 外壳于是进首启动流程、只有初始化窗口，
        # 主循环找不到侧栏「记账」就干等 60 秒（实测踩过；要换工作空间请显式传 -Workspace）。
        try { $wsAbs = [string](Get-Content $cfg -Raw | ConvertFrom-Json).workspaceDir } catch { $wsAbs = '' }
        if ($wsAbs) { Write-Host "[dev-hot] 沿用配置里的工作空间：$wsAbs（要换用 -Workspace <dir>）" -ForegroundColor DarkGray }
    }
    @{ width = 1500; height = 1000; workspaceDir = $wsAbs; closeBehavior = 'quit'; appearance = 'light' } |
        ConvertTo-Json | Set-Content -Path $cfg -Encoding UTF8
    if (-not $wsAbs) {
        Write-Host '[dev-hot] ⚠ 工作空间为空 —— 外壳会进首启动流程、只开 600×560 的初始化窗口，' -ForegroundColor Yellow
        Write-Host '          主循环要的侧栏「记账」不会出现。请传 -Workspace <dir>（可先 cargo xtask seed 一份）。' -ForegroundColor Yellow
    }
    $saved = $env:USERPROFILE
    try {
        $env:USERPROFILE = $SmokeHome
        $proc = Start-Process -FilePath $Exe -PassThru
    } finally {
        $env:USERPROFILE = $saved
    }
    Write-Host ("[dev-hot] 已启动 dev 外壳 PID {0}（配置目录 {1}）" -f $proc.Id, $SmokeHome) -ForegroundColor Cyan
}

# ---------------------------------------------------------------- 主循环

if ($Trunk) { Start-TrunkServe }
if ($Launch) {
    $env:USERPROFILE = $SmokeHome   # Start-Process 继承当前环境；子进程读 USERPROFILE 找配置
    Start-DevShell
    Start-Sleep -Seconds 3
}

$window = $null
$deadline = (Get-Date).AddSeconds(60)
while ((Get-Date) -lt $deadline -and -not $window) {
    $window = Get-RepoAppWindow
    if (-not $window) { Start-Sleep -Milliseconds 800 }
}
if (-not $window) {
    $hint = '没找到本仓库的应用主窗口。先把它跑起来：cargo tauri dev（或本脚本加 -Launch -Trunk）'
    $cfg = Join-Path $SmokeHome '.transactions-dev.json'
    if ((Test-Path $cfg) -and -not ((Get-Content $cfg -Raw | ConvertFrom-Json).workspaceDir)) {
        $hint = '没找到主窗口：配置里的工作空间是空的，外壳停在 600×560 的初始化窗口。请加 -Workspace <dir> 重跑。'
    }
    throw $hint
}
Write-Host '[dev-hot] 已锁定应用窗口，开始监视 trunk 构建…（Ctrl+C 退出）' -ForegroundColor Green
Write-Host '          改 crates/tr-ui 下的 .rs / .css，窗口会自己刷新' -ForegroundColor DarkGray

$lastLength = if (Test-Path $TrunkLog) { (Get-Item $TrunkLog).Length } else { 0 }
$lastSuccess = (Get-Date).AddSeconds(-1)
$reloads = 0

while ($true) {
    Start-Sleep -Milliseconds $PollMs
    if (-not (Test-Path $TrunkLog)) { continue }
    $file = Get-Item $TrunkLog
    if ($file.Length -lt $lastLength) { $lastLength = 0 }      # 日志被截断（trunk 重启）
    if ($file.Length -eq $lastLength) { continue }

    $stream = [System.IO.File]::Open($TrunkLog, 'Open', 'Read', 'ReadWrite')
    try {
        $stream.Seek($lastLength, 'Begin') | Out-Null
        $reader = New-Object System.IO.StreamReader($stream)
        $chunk = $reader.ReadToEnd()
        $lastLength = $stream.Position
    } finally { $stream.Dispose() }
    if (-not $chunk) { continue }

    if ($chunk -match 'error(\[|:)|error: could not compile') {
        Write-Host '[dev-hot] ✗ 构建失败（看 target\trunk-hot.log），等下一次改动' -ForegroundColor Red
        continue
    }
    if ($chunk -match 'applying new distribution') {
        # 给 WebView2 一点时间把 dist 落稳，再刷新
        Start-Sleep -Milliseconds 250
        $window = Get-RepoAppWindow
        if (-not $window) { continue }
        $handle = [IntPtr]$window.Current.NativeWindowHandle
        [TrUia]::SetForegroundWindow($handle) | Out-Null
        Start-Sleep -Milliseconds 150
        [TrDevHot]::CtrlR()
        $reloads++
        Write-Host ("[dev-hot] ↻ 第 {0} 次刷新 {1:HH:mm:ss}" -f $reloads, (Get-Date)) -ForegroundColor Green
        if ($ShotDir) {
            Start-Sleep -Milliseconds 700      # 等浏览器把新 DOM 画完
            New-Item -ItemType Directory -Force -Path $ShotDir | Out-Null
            Save-CurrentShot -Window $window -Path (Join-Path $ShotDir 'current.png')
        }
    }
}
