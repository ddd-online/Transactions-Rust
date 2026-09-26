# ui-diary-edit.ps1 —— 日记页**编辑链路**端到端：写内容 → 心情 → 改内容（同一天 upsert）→ 删除。
#
# 为什么需要它：`fixtures/ui-diary-io.ps1` 覆盖的是**导入/导出**（原生选目录框 + 编码回退），
# 而"在界面上写日记"这条路此前没人走过。日记编辑器的契约有几处不看代码猜不到：
#   * **没有保存按钮**：输入或切心情后 **1500ms 防抖**自动保存，`Ctrl+S` 立即保存（`on_save_shortcut`）；
#   * 日记页**始终是可编辑的**（没有预览/编辑切换，也不做 Markdown 渲染）；
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

. (Join-Path $PSScriptRoot 'lib\TrUia.ps1')

$repo = Split-Path -Parent $PSScriptRoot
$explicitWorkspace = -not [string]::IsNullOrWhiteSpace($Workspace)
if (-not $Exe) { $Exe = Join-Path $repo 'target\release\transactions.exe' }
if (-not $SmokeHome) { $SmokeHome = Join-Path $repo 'target\tests\ui-diary-edit\home' }
if (-not $OutDir) { $OutDir = Join-Path $repo 'target\tests\ui-diary-edit\out' }
if (-not $Workspace) { $Workspace = Join-Path $OutDir 'ws' }

if (-not (Test-Path $Exe)) { throw "找不到可执行文件: $Exe（先跑 cargo build --release -p transactions）" }

$trPaths = Initialize-TrSmokeHome -SmokeHome $SmokeHome -OutDir $OutDir
$smokeHome = $trPaths.SmokeHome
$OutDir = $trPaths.OutDir
Assert-NoRepoInstance -Repo $repo

$ws = [System.IO.Path]::GetFullPath($Workspace)

# 公共鼠标 P/Invoke 类（TrDiary）已统一到 fixtures/lib/TrUia.ps1 的 TrUia（调用点是 [TrUia]::…）

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
# 编辑器里的多行文本域 / 粘贴 / 写内容 / Ctrl+S 这四个助手已抽到 `fixtures/lib/TrUia.ps1`
# （`Get-DiaryTextarea` / `Paste-Text` / `Set-DiaryContent` / `Save-Now`），与 ui-diary-ledger.ps1 共用。

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
    [TrUia]::ShowWindow($hwnd, 9) | Out-Null
    [TrUia]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Seconds 1

# ================= 1/4 打开日记页：今天应已就绪（始终可编辑）=================
    Write-Host "`n[diary] 1/4 打开「日记」，进入编辑态"
    Assert-True (Invoke-Element (Wait-Element -Root $window -Name '日记')) '打开「日记」'
    Start-Sleep -Seconds 3
    # 页脚的编辑/预览切换：预览态时按钮文案是「编辑」
    # 新行为：没有「编辑/预览」切换按钮（也没有可点的 toggle），页面本身就是编辑态。
    # 原来是 `$toggle = $true` 再 `Invoke-Element $toggle` —— 那是一段残留死代码，
    # 真跑起来会在 Invoke-Element 里炸（`[bool]` 没有 TryGetCurrentPattern），
    # 于是本脚本**从来跑不到后面任何断言**。这里直接按"已经是编辑态"往下走。
    Start-Sleep -Seconds 1
    $textarea = Get-DiaryTextarea -Window $window
    Assert-True ([bool]$textarea) '日记页直接就是编辑态：找到日记文本域'
    if (-not $textarea) { throw '找不到日记文本域，后续无法继续' }

    # ================= 2/4 写内容 → Ctrl+S → 落库 =================
    Write-Host "`n[diary] 2/4 写入内容并保存（$contentA）"
    $rows = @()
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        $typed = Set-DiaryContent -Window $window -Text $contentA
        Save-Now -Window $window
        $rows = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_diary_entry' -OutDir $OutDir | Where-Object { $_.date -eq $today })
        if ($rows.Count -eq 1 -and $rows[0].content -eq $contentA) { break }
        Write-Host "    第 $attempt 次写入没生效（typed=$typed，库里 $($rows.Count) 条），重试" -ForegroundColor DarkYellow
    }
    Assert-True ($rows.Count -eq 1) "库里出现今天这一条（$today，实际 $($rows.Count) 条）"
    if ($rows.Count -ge 1) {
        Assert-True ($rows[0].content -eq $contentA) "正文逐字节一致（实际 '$($rows[0].content)'）"
        Assert-True ($rows[0].word_count -eq $contentA.Length) `
            "字数按 Unicode 标量值算（期望 $($contentA.Length)，实际 $($rows[0].word_count)）"
        Assert-True ([string]::IsNullOrEmpty($rows[0].mood)) "刚写时没有心情（实际 '$($rows[0].mood)'）"
        # 日记按账本隔离：写入必须落在**当前账本** —— 用它里面的种子那两篇（写在主账本）
        # 反查当前账本的 id，避免把账本名字写死在断言里（种子后续阶段会改账本名）。
        $seeded = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_diary_entry' -OutDir $OutDir |
            Where-Object { $_.date -eq '2026-02-10' })
        Assert-True ($seeded.Count -eq 1) '种子里有 2026-02-10 那篇日记（用来确定当前账本）'
        if ($seeded.Count -eq 1) {
            Assert-True ($rows[0].ledger_id -eq $seeded[0].ledger_id) `
                "日记落在当前账本（期望 $($seeded[0].ledger_id)，实际 '$($rows[0].ledger_id)'）"
        }
    }
    $firstId = if ($rows.Count -ge 1) { $rows[0].id } else { '' }
    # 界面上的字数提示也要跟上
    $wordLabel = Get-Elements $window | ForEach-Object { $_.Current.Name } |
        Where-Object { $_ -eq "$($contentA.Length)字" } | Select-Object -First 1
    Assert-True ([bool]$wordLabel) "编辑器右上角显示「$($contentA.Length)字」"
    # 保存状态标签：三种状态如实反映——「编辑中」（改动进草稿、防抖窗口内）→
    # 「保存中…」→「已保存」。Ctrl+S 后必须是「已保存」。
    # 注意：Wait-Like 是**字面子串**匹配（`$name.Contains($Pattern)`），不是正则，别写 '^已保存$'。
    $savedLabel = Wait-Element -Root $window -Name '已保存' -TimeoutSec 8
    Assert-True ([bool]$savedLabel) '保存完成后状态标签显示「已保存」'

    # ================= 3/4 选心情 + 改内容：同一天 upsert（id 不变）=================
    Write-Host "`n[diary] 3/4 选心情「开心」并改写内容 → 同一天 upsert"
    $moodButton = Wait-Element -Root $window -Name '开心' -TimeoutSec 10
    Assert-True ([bool]$moodButton) '找到心情按钮「开心」'
    if ($moodButton) { Invoke-Element $moodButton | Out-Null }
    # 切心情也走同一条自动保存链路：点完在防抖窗口内必须是「编辑中」
    $editingLabel = Wait-Element -Root $window -Name '编辑中' -TimeoutSec 2
    Assert-True ([bool]$editingLabel) '改动未落库时状态标签是「编辑中」'
    Start-Sleep -Seconds 3   # 切心情走 1500ms 防抖保存
    $savedAgain = Wait-Element -Root $window -Name '已保存' -TimeoutSec 8
    Assert-True ([bool]$savedAgain) '防抖保存完成后状态标签回到「已保存」'
    $afterMood = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_diary_entry' -OutDir $OutDir | Where-Object { $_.date -eq $today }) | Select-Object -First 1
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
        $afterEdit = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_diary_entry' -OutDir $OutDir | Where-Object { $_.date -eq $today }) | Select-Object -First 1
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

    # ================= 4/4 删除 =================
    Write-Host "`n[diary] 4/4 删除"
    
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
    $gone = @(Read-Table -Repo $repo -Workspace $ws -Table 'tbl_billadm_diary_entry' -OutDir $OutDir | Where-Object { $_.date -eq $today })
    Assert-True ($gone.Count -eq 0) "删除后库里这一天没了（实际 $($gone.Count) 条）"
    # 编辑器回到空态（"选择左侧日期开始写作"）
    $emptyHint = Wait-Like -Root $window -Pattern '选择左侧日期开始写作' -TimeoutSec 8
    Assert-True ([bool]$emptyHint) '删除后编辑器回到空态提示'
}
finally { Stop-TrApp -Process $process -Failures $failures -OutDir $OutDir }

Show-TrSummary -Failures $failures -Tag 'diary' -SuccessMessage "[diary] 全部通过：写内容 → 心情 → 改内容（同一天 upsert）→ 删除"
