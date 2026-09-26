# ui-proxy.ps1 —— 「应用设置 → 通用设置 → 代理」端到端：**证明出网请求真的走了代理**。
#
# 为什么需要它：代理这类"配置项"最容易变成只写进配置文件的摆设 —— 界面显示得好好的，
# 请求其实还是直连。这条脚本用一个**本机假 HTTP 代理**（记录每条请求行）来当判据：
#   1. 界面把手动代理指向假代理 → 行情查询（`qt.gtimg.cn`）与更新检查（`api.github.com`）
#      必须**出现在假代理的请求日志里**，并且行情名称显示的是假代理返回的固定名字
#      （说明响应也确实来自代理，不只是"连了一下"）；
#   2. 切回「不使用代理」→ 再查一次行情 → 假代理**一条新请求都收不到**（直连）。
# 两条合起来才说明"设置 → 生效"是通的；只断言配置文件里写了什么等于什么都没验。
#
# 边界：脚本**不修改系统代理设置**（`auto` 模式只读注册表）；假代理只监听 127.0.0.1 的随机端口。
#
# 用法（pwsh 7；需要 release 产物；本仓库不能有实例在跑）：
#   pwsh -File fixtures/ui-proxy.ps1 [-Exe <exe>] [-Workspace <ws>] [-OutDir <dir>]

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
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\tests\ui-proxy\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\tests\ui-proxy\out' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$trPaths = Initialize-TrSmokeHome -SmokeHome $SmokeHome -OutDir $OutDir
$smokeHome = $trPaths.SmokeHome
$OutDir = $trPaths.OutDir
Assert-NoRepoInstance -Repo $repo

$ws = [System.IO.Path]::GetFullPath($Workspace)
$configPath = Join-Path $smokeHome '.transactions.json'

# 假代理返回的固定行情：**纯 ASCII** 就够了（`decode_gbk` 对 ASCII 是恒等变换），
# 字段顺序按腾讯行情：`v_<市场><代码>="<未知>~<名称>~<代码>~<最新价>~<昨收>~…"`。
$fakeName = 'AGENTPROXY'
# ⚠ 股票代码必须选**种子里没有的**：`lookup_stock_name` 优先查本地交易记录，
# 命中就不发网络请求（用 600519 会查到种子里的「贵州茅台」，整条代理断言会假红/假绿）。
$quoteCode = '601398'
$fakePayload = "v_sh${quoteCode}=`"1~$fakeName~$quoteCode~1700.00~1690.00~1700.00~0~0~0~0~0~0~0~0~0~0~0~0~0~0~0~0~0~0~0~0~0~0~0~0~20260921150000`";"

$proxyLog = Join-Path $OutDir 'fake-proxy.log'
$proxyPortFile = Join-Path $OutDir 'fake-proxy.port'
Remove-Item $proxyLog, $proxyPortFile -ErrorAction SilentlyContinue

$proxyJob = $null
$process = $null
$failures = New-Object System.Collections.Generic.List[string]

# ---------------------------------------------------------------- 假 HTTP 代理
# 端口交给系统分配（`port 0`）再回传，避免"挑一个端口恰好被占"的偶发红。
#
# ⚠ **ureq 对 `http://` 目标也走 CONNECT 隧道**（实测请求行是 `CONNECT qt.gtimg.cn:80`，
# 不是"绝对 URI 的 GET"），所以这里要按隧道处理：先回 `200 Connection Established`，
# 再从隧道里读真正的请求、把固定行情写回去。只回 502 的话应用侧只会"静默查不到名字"。
# `:443`（GitHub）需要真 TLS，这里只回报"连到了代理"，应用会得到一个可预期的失败。
$proxyJob = Start-Job -ScriptBlock {
    param($LogPath, $PortFile, $Payload)
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $port = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
    Set-Content -Path $PortFile -Value $port

    function Send-Raw { param($Stream, [string]$Text)
        $bytes = [System.Text.Encoding]::ASCII.GetBytes($Text)
        $Stream.Write($bytes, 0, $bytes.Length)
        $Stream.Flush()
    }
    function Send-Canned {
        param($Stream)
        $body = [System.Text.Encoding]::ASCII.GetBytes($Payload)
        $head = "HTTP/1.1 200 OK`r`nContent-Type: text/plain; charset=GBK`r`nContent-Length: $($body.Length)`r`nConnection: close`r`n`r`n"
        $Stream.Write([System.Text.Encoding]::ASCII.GetBytes($head), 0, $head.Length)
        $Stream.Write($body, 0, $body.Length)
        $Stream.Flush()
    }
    function Read-Headers { param($Reader)
        while ($true) {
            $line = $Reader.ReadLine()
            if ($null -eq $line -or $line -eq '') { return }
        }
    }

    while ($true) {
        $client = $listener.AcceptTcpClient()
        try {
            $stream = $client.GetStream()
            $reader = New-Object System.IO.StreamReader($stream, [System.Text.Encoding]::ASCII)
            $requestLine = $reader.ReadLine()
            if (-not $requestLine) { continue }
            Add-Content -Path $LogPath -Value $requestLine
            Read-Headers -Reader $reader

            if ($requestLine.StartsWith('CONNECT')) {
                $target = ($requestLine -split ' ')[1]
                $targetPort = if ($target -match ':(\d+)$') { [int]$Matches[1] } else { 443 }
                if ($targetPort -ne 80) {
                    Send-Raw -Stream $stream -Text "HTTP/1.1 502 Bad Gateway`r`nContent-Length: 0`r`nConnection: close`r`n`r`n"
                    continue
                }
                Send-Raw -Stream $stream -Text "HTTP/1.1 200 Connection Established`r`n`r`n"
                $inner = $reader.ReadLine()
                if ($inner) { Add-Content -Path $LogPath -Value "  -> $inner" }
                Read-Headers -Reader $reader
                Send-Canned -Stream $stream
            }
            else {
                Send-Canned -Stream $stream
            }
        }
        finally {
            $client.Close()
        }
    }
} -ArgumentList $proxyLog, $proxyPortFile, $fakePayload

# 等端口回传
$deadline = (Get-Date).AddSeconds(15)
while (-not (Test-Path $proxyPortFile) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
if (-not (Test-Path $proxyPortFile)) { throw '假代理没能在 15 秒内起来' }
$proxyPort = (Get-Content $proxyPortFile -Raw).Trim()
$proxyUrl = "http://127.0.0.1:$proxyPort"
Write-Host "[proxy] 假代理已就绪：$proxyUrl（日志 $proxyLog）" -ForegroundColor Cyan

# ---------------------------------------------------------------- 界面小工具
function Find-Named { param($Window, [string]$Name, [switch]$Right)
    $win = $Window.Current.BoundingRectangle
    $hits = @()
    foreach ($el in @(Get-Elements $Window)) {
        if ($el.Current.Name -ne $Name) { continue }
        if ($el.Current.IsOffscreen) { continue }
        if (-not (Test-Rect $el.Current.BoundingRectangle)) { continue }
        if ($Right -and (($el.Current.BoundingRectangle.X - $win.X) -le 260)) { continue }
        $hits += $el
    }
    return $hits
}
# 页签是 TabItem（不是 Button）：优先 SelectionItemPattern
function Select-Tab { param($Window, [string]$Name, [int]$TimeoutSec = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        foreach ($el in (Find-Named -Window $Window -Name $Name -Right)) {
            $pattern = $null
            if ($el.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) {
                $pattern.Select(); Start-Sleep -Milliseconds 800; return $true
            }
            if ($el.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
                $pattern.Invoke(); Start-Sleep -Milliseconds 800; return $true
            }
            Click-Element $el | Out-Null; Start-Sleep -Milliseconds 800; return $true
        }
        Start-Sleep -Milliseconds 300
    } while ((Get-Date) -lt $deadline)
    return $false
}
function Invoke-Named { param($Window, [string]$Name, [switch]$Right, [switch]$Last, [int]$TimeoutSec = 20)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        $hits = @(Find-Named -Window $Window -Name $Name -Right:$Right)
        if ($hits.Count -gt 0) {
            $el = if ($Last) { $hits[$hits.Count - 1] } else { $hits[0] }
            $pattern = $null
            if ($el.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
                $pattern.Invoke(); return $true
            }
            Click-Element $el | Out-Null
            return $true
        }
        Start-Sleep -Milliseconds 300
    } while ((Get-Date) -lt $deadline)
    return $false
}
# 下单弹窗里「股票名称 / 股票代码」两个输入框在 UIA 里没有名字，只能取全部 Edit 的值
function Get-UnnamedEditValues { param($Window)
    $values = @()
    foreach ($el in @(Get-Elements $Window)) {
        $isEdit = ($el.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit) -or
            ($el.Current.ClassName -eq 'Edit')
        if (-not $isEdit) { continue }
        if ($el.Current.IsOffscreen) { continue }
        if ($el.Current.Name) { continue }
        $pattern = $null
        if ($el.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
            $values += $pattern.Current.Value
        }
    }
    return $values
}
function Get-UnnamedEditRight { param($Window)
    $edits = @()
    foreach ($el in @(Get-Elements $Window)) {
        $isEdit = ($el.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit) -or
            ($el.Current.ClassName -eq 'Edit')
        if (-not $isEdit) { continue }
        if ($el.Current.IsOffscreen) { continue }
        if ($el.Current.Name) { continue }
        if (-not (Test-Rect $el.Current.BoundingRectangle)) { continue }
        $edits += $el
    }
    # 同一水平带里靠右那个是「股票代码」
    return ($edits | Sort-Object { $_.Current.BoundingRectangle.X } | Select-Object -Last 1)
}
function Get-ProxyLog { if (Test-Path $proxyLog) { return @(Get-Content $proxyLog) } else { return @() } }
function Wait-ProxyLog { param([string]$Pattern, [int]$TimeoutSec = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    do {
        if (@(Get-ProxyLog) | Where-Object { $_ -like "*$Pattern*" }) { return $true }
        Start-Sleep -Milliseconds 400
    } while ((Get-Date) -lt $deadline)
    return $false
}
function Get-ConfigProxy {
    if (-not (Test-Path $configPath)) { return $null }
    $json = Get-Content $configPath -Raw | ConvertFrom-Json
    return $json.proxy
}

# ---- 工作空间（股票链路要一个已播种的库：账本 + 账户） ----
if (-not $explicitWorkspace) {
    if (Test-Path $ws) { Remove-Item $ws -Recurse -Force }
}
if (-not (Test-Path (Join-Path $ws 'transactions.db'))) {
    Push-Location $repo
    & cargo -q xtask seed $ws *> (Join-Path $OutDir 'seed.log')
    $seedExit = $LASTEXITCODE
    Pop-Location
    if ($seedExit -ne 0) { Get-Content (Join-Path $OutDir 'seed.log') -Tail 8; throw "播种失败（exit=$seedExit）" }
    Write-Host "[proxy] 播种工作空间: $ws" -ForegroundColor Cyan
}

@{ width = 1500; height = 950; workspaceDir = $ws; closeBehavior = 'quit'
   appearance = 'light'; smokeTestMarker = 'ui-proxy.ps1' } |
    ConvertTo-Json | Set-Content -Path $configPath -Encoding UTF8

try {
    $process = Start-App -SmokeHome $smokeHome -Exe $Exe
    $window = Get-ReadyWindow -ProcessId $process.Id -TimeoutSec 60
    if (-not $window) { throw '启动后 60 秒内没有拿到主窗口' }
    [TrUia]::ShowWindow([IntPtr]$window.Current.NativeWindowHandle, 9) | Out-Null
    [TrUia]::SetForegroundWindow([IntPtr]$window.Current.NativeWindowHandle) | Out-Null
    Start-Sleep -Seconds 2

    # ================= 1/4 把界面切到「手动代理」并指向假代理 =================
    Write-Host "`n[proxy] 1/4 通用设置 → 代理：手动 $proxyUrl"
    Assert-True (Invoke-Named -Window $window -Name '应用设置') '打开「应用设置」页'
    Start-Sleep -Seconds 2
    Assert-True (Select-Tab -Window $window -Name '通用设置') '切到「通用设置」分栏'
    Start-Sleep -Seconds 2

    Assert-True (Invoke-Named -Window $window -Name '手动' -Right) '点「手动」'
    Start-Sleep -Seconds 1
    # 手动地址现在是**三段**：协议（下拉框，目前只有 HTTP）/ 域名 / 端口。
    # 两个输入框用共享的 `Set-InputByPaste`（真实粘贴 + 读回校验）而不是 `ValuePattern.SetValue`：
    # 它们是**受控**输入（Leptos 的 `<input value=signal>`），不触发 `input` 事件就白填
    # （见 AGENTS.md「受控 input 要把值真的打进去」）。名字是**占位符**（空值时可访问名即它）。
    Assert-True ([bool](Find-Named -Window $window -Name 'HTTP' -Right)) '协议控件在（显示 HTTP）'
    Assert-True (Set-InputByPaste -Window $window -Name '127.0.0.1' -Text '127.0.0.1') '填入域名 127.0.0.1'
    Assert-True (Set-InputByPaste -Window $window -Name '7890' -Text $proxyPort) "填入端口 $proxyPort"
    Start-Sleep -Milliseconds 500
    Assert-True (Invoke-Named -Window $window -Name '保存' -Right) '点「保存」'
    Start-Sleep -Seconds 3

    $saved = Get-ConfigProxy
    Assert-True (($saved.mode -eq 'manual') -and ($saved.url -eq $proxyUrl)) `
        "配置里写入了手动代理（实际: mode=$($saved.mode) url=$($saved.url)）"
    # 三段拼出来的一条地址经后端归一化后会**拆回**表单：端口框里应当还是那个端口。
    # ⚠ 不能按名字找：输入框有值以后占位符不再当可访问名（空值时才是），所以按类名找这一行里
    # **最靠右**的输入框，用 ValuePattern 读值。
    $portEdit = @(Get-Elements $window) | Where-Object {
        $_.Current.ClassName -eq 'ui-input__control' -and -not $_.Current.IsOffscreen -and
            (Test-Rect $_.Current.BoundingRectangle)
    } | Sort-Object { $_.Current.BoundingRectangle.X } | Select-Object -Last 1
    $portValue = ''
    if ($portEdit) {
        $pattern = $null
        if ($portEdit.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
            $portValue = $pattern.Current.Value
        }
    }
    Assert-True ($portValue -eq $proxyPort) "保存后端口原样回填（实际 '$portValue'）"

    # 「检测」的结果要**弹一条提示**（用户报过"点了没反应"——原来只更新卡片里那行灰字）。
    # ⚠ 提示的文案与卡片里那行灰字**一字不差**，所以只能按**位置**区分：提示挂在窗口底部的
    # 消息栈里（`.notice-stack-message`，那条 div 在 UIA 里被剪掉、只剩无类名的 Text 节点），
    # 灰字在设置卡片里、靠上。判据：同文案里 y 落在窗口下五分之一的那个。
    Assert-True (Invoke-Named -Window $window -Name '检测' -Right) '点「检测」'
    $win = $window.Current.BoundingRectangle
    $toastBottom = $win.Y + $win.Height * 0.8
    $toast = $null
    $deadline = (Get-Date).AddSeconds(6)
    do {
        $toast = @(Get-Elements $window) | Where-Object {
            $_.Current.Name -and $_.Current.Name.Contains($proxyUrl) -and
                -not $_.Current.IsOffscreen -and (Test-Rect $_.Current.BoundingRectangle) -and
                ($_.Current.BoundingRectangle.Y -gt $toastBottom)
        } | Select-Object -First 1
        if (-not $toast) { Start-Sleep -Milliseconds 300 }
    } while (-not $toast -and (Get-Date) -lt $deadline)
    Assert-True ([bool]$toast) "检测结果在窗口底部弹出了消息提示（带 $proxyUrl）"

    # ================= 2/4 行情查询必须打到假代理 =================
    Write-Host "`n[proxy] 2/4 股票：建仓 → 填代码后失焦自动查名（应经假代理）"
    Assert-True (Invoke-Named -Window $window -Name '股票') '打开「股票」页'
    Start-Sleep -Seconds 3
    Assert-True (Select-Tab -Window $window -Name '持仓') '切到「持仓」页签'
    Start-Sleep -Seconds 2
    Assert-True (Invoke-Named -Window $window -Name '建仓' -Last) '点「建仓」'
    Start-Sleep -Seconds 2

    $codeInput = Get-UnnamedEditRight -Window $window
    Assert-True ([bool]$codeInput) '找到「股票代码」输入框'
    Assert-True (Set-Value $codeInput $quoteCode) "填入股票代码 $quoteCode"
    Start-Sleep -Milliseconds 600
    # 名称靠**失焦自动查**（「查询股票名称」按钮已去掉）：聚焦代码框 → Tab 离开它
    $codeInput.SetFocus()
    Start-Sleep -Milliseconds 300
    [System.Windows.Forms.SendKeys]::SendWait('{TAB}')

    Assert-True (Wait-ProxyLog -Pattern 'qt.gtimg.cn' -TimeoutSec 20) `
        '假代理收到了行情请求（说明行情确实走了代理）'
    Write-Host "[proxy] 假代理收到的请求：" -ForegroundColor DarkGray
    foreach ($line in @(Get-ProxyLog)) { Write-Host "    $line" -ForegroundColor DarkGray }
    Start-Sleep -Seconds 2
    $names = @(Get-UnnamedEditValues -Window $window)
    Assert-True ($names -contains $fakeName) `
        "界面显示的是假代理返回的名称（实际输入框值: $($names -join ' / ')）"

    # 关掉建仓弹窗（不落库）：后面的步骤要切页面
    Invoke-Named -Window $window -Name '取消' -Last | Out-Null
    Start-Sleep -Seconds 1

    # ================= 3/4 更新检查也必须打到假代理 =================
    Write-Host "`n[proxy] 3/4 关于软件：自动检查更新（应经假代理，走 CONNECT）"
    Assert-True (Invoke-Named -Window $window -Name '应用设置') '回到「应用设置」页'
    Start-Sleep -Seconds 2
    Assert-True (Select-Tab -Window $window -Name '关于软件') '切到「关于软件」分栏'
    Assert-True (Wait-ProxyLog -Pattern 'api.github.com' -TimeoutSec 25) `
        '假代理收到了 GitHub API 请求（说明更新检查也走了代理）'

    # ================= 4/4 切「不使用代理」后必须直连 =================
    Write-Host "`n[proxy] 4/4 切回「不使用」→ 行情不再经过假代理"
    Assert-True (Select-Tab -Window $window -Name '通用设置') '切回「通用设置」分栏'
    Start-Sleep -Seconds 2
    Assert-True (Invoke-Named -Window $window -Name '不使用' -Right) '点「不使用」'
    Start-Sleep -Seconds 3
    $saved = Get-ConfigProxy
    Assert-True ($saved.mode -eq 'off') "配置切到不使用（实际: mode=$($saved.mode)）"

    # 记录日志条数后重新查一次行情：直连的请求不该再出现在假代理日志里
    # （不清空日志：留证据，避免"截断与写入竞争"这种偶发）
    $before = @(Get-ProxyLog | Where-Object { $_.Trim() }).Count
    Assert-True (Invoke-Named -Window $window -Name '股票') '回到「股票」页'
    Start-Sleep -Seconds 3
    Assert-True (Select-Tab -Window $window -Name '持仓') '切到「持仓」页签'
    Start-Sleep -Seconds 2
    Assert-True (Invoke-Named -Window $window -Name '建仓' -Last) '再点「建仓」'
    Start-Sleep -Seconds 2
    $codeInput = Get-UnnamedEditRight -Window $window
    if ($codeInput) { Set-Value $codeInput $quoteCode | Out-Null }
    # 同上：填完代码按 Tab 失焦，触发那次（应当直连的）行情查询
    if ($codeInput) { $codeInput.SetFocus() }
    Start-Sleep -Milliseconds 300
    [System.Windows.Forms.SendKeys]::SendWait('{TAB}')
    Start-Sleep -Seconds 6
    $after = @(Get-ProxyLog | Where-Object { $_.Trim() })
    Assert-True ($after.Count -eq $before) `
        "不使用代理时假代理不应再收到请求（前 $before 条 → 后 $($after.Count) 条：$($after -join ' | ')）"
}
finally {
    Stop-TrApp -Process $process -Failures $failures -OutDir $OutDir
    if ($proxyJob) {
        Stop-Job $proxyJob -ErrorAction SilentlyContinue
        Remove-Job $proxyJob -Force -ErrorAction SilentlyContinue
    }
}

Show-TrSummary -Failures $failures -Tag 'proxy' `
    -SuccessMessage "[proxy] 全部通过：手动代理下行情与更新检查都经代理，切「不使用」后不再经过"
