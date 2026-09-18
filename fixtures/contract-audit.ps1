# contract-audit.ps1 —— IPC 入参契约的**跨 crate 一致性**检查（界面 ↔ 命令）。
#
# 背景：界面侧的请求结构体是**手抄** tr-ipc 的（`tr-ipc` 依赖 tauri，不能编到 wasm，
# 所以 `tr-ui/src/api/*.rs` 里每个命令都有一份自己的 `*Request`）。字段名抄错不会编译报错，
# serde 会静默用默认值——查询条件被丢掉、写入落成空值，界面上看着"正常"。
# 本项目已经踩过一次同类问题（记账页 Effect 用 get_untracked 导致页面永远空白）。
#
# 本脚本做四件事：
#   1. 从 `crates/tr-ipc/src/commands/*.rs` 抽出"命令 → 请求结构体 → serde 字段名集合"；
#   2. 从 `crates/tr-ui/src/api/*.rs` 抽出"命令 → 请求结构体 → serde 字段名集合"；
#   3. 逐命令比较两侧字段名集合，报出缺失/多余（大小写、下划线、别名都算差异）；
#   4. 汇报"界面直接传 tr_domain DTO/共享类型"的命令（这类不可能漂移，跳过比较）。
#
# 用法（pwsh 7）：pwsh -File fixtures/contract-audit.ps1
# 退出码：0 = 一致；1 = 有差异。

param(
    [string]$RepoRoot
)

$ErrorActionPreference = 'Stop'
if (-not $RepoRoot) { $RepoRoot = Split-Path -Parent $PSScriptRoot }

$ipcDir = Join-Path $RepoRoot 'crates\tr-ipc\src\commands'
$uiDir = Join-Path $RepoRoot 'crates\tr-ui\src\api'
foreach ($dir in @($ipcDir, $uiDir)) {
    if (-not (Test-Path $dir)) { throw "找不到目录: $dir" }
}

# ---------- 解析：结构体 → 字段（可接受写法集合 + 是否可选） ----------
# 返回 @{ 字段名 = @{ Accepted = @(名字...); Optional = [bool] } } 的数组形式：
# 每个字段是一个 hashtable，便于逐字段判断"界面有没有用一个被接受的写法"。
function Get-StructFields {
    param([string]$Path)
    $lines = Get-Content $Path
    $structs = @{}
    $current = $null
    $renameAll = ''
    $structDefault = $false
    $pendingRename = $null
    $pendingAlias = @()
    $pendingDefault = $false

    for ($i = 0; $i -lt $lines.Count; $i++) {
        $trimmed = $lines[$i].Trim()

        if ($trimmed -match '^#\[serde\((?<attrs>.*)\)\]') {
            $attrs = $Matches['attrs']
            if ($attrs -match 'rename_all\s*=\s*"(?<v>[^"]+)"') { $renameAll = $Matches['v'] }
            if ($attrs -match '(^|,)\s*default\s*(,|$)') { $pendingDefault = $true; $structDefault = $true }
            if ($attrs -match 'rename\s*=\s*"(?<v>[^"]+)"') { $pendingRename = $Matches['v'] }
            foreach ($a in [regex]::Matches($attrs, 'alias\s*=\s*"(?<v>[^"]+)"')) { $pendingAlias += $a.Groups['v'].Value }
            continue
        }

        if ($trimmed -match '^(pub )?struct\s+(?<name>[A-Za-z0-9_]+)') {
            $current = $Matches['name']
            $structs[$current] = @{ Fields = (New-Object System.Collections.Generic.List[object]); RenameAll = $renameAll; Default = $structDefault }
            $renameAll = ''
            $structDefault = $false
            $pendingRename = $null
            $pendingAlias = @()
            $pendingDefault = $false
            continue
        }

        if ($null -ne $current) {
            if ($trimmed.StartsWith('}')) { $current = $null; continue }
            # 界面侧的请求结构体字段是**私有**的（只 derive Serialize），所以 pub 可选
            if ($trimmed -match '^(pub\s+)?(?<field>[a-z][a-z0-9_]*)\s*:') {
                $field = $Matches['field']
                $primary = $field
                if ($pendingRename) { $primary = $pendingRename }
                elseif ($structs[$current].RenameAll -eq 'camelCase') {
                    $primary = [regex]::Replace($field, '_([a-z])', { param($m) $m.Groups[1].Value.ToUpper() })
                }
                $accepted = @($primary) + $pendingAlias + @($field)
                $structs[$current].Fields.Add(@{
                        Field    = $field
                        Accepted = @($accepted | Sort-Object -Unique)
                        Optional = ($pendingDefault -or $structs[$current].Default)
                    })
                $pendingRename = $null
                $pendingAlias = @()
                $pendingDefault = $false
            }
        }
        else {
            $renameAll = ''
            $structDefault = $false
        }
    }
    return $structs
}

# ---------- 收集 tr-ipc / 外壳侧：命令 → 字段定义 ----------
$ipcStructs = @{}
foreach ($file in Get-ChildItem $ipcDir -File -Filter *.rs) {
    foreach ($entry in (Get-StructFields $file.FullName).GetEnumerator()) {
        $ipcStructs[$entry.Key] = $entry.Value
    }
}

$ipcCommands = @{}
foreach ($file in (Get-ChildItem $ipcDir -File -Filter *.rs) + (Get-ChildItem (Join-Path $RepoRoot 'src-tauri\src') -File -Filter *.rs)) {
    $text = Get-Content $file.FullName -Raw
    foreach ($m in [regex]::Matches($text, '(?s)#\[tauri::command\][^}]*?pub\s+(?:async\s+)?fn\s+(?<cmd>[a-z0-9_]+)\s*\((?<args>.*?)\)\s*(->|\{)')) {
        $cmd = $m.Groups['cmd'].Value
        $args = $m.Groups['args'].Value
        if ($args -match 'req\s*:\s*(?<ty>[A-Za-z0-9_:]+)') { $ipcCommands[$cmd] = $Matches['ty'].Split('::')[-1] }
    }
}
# 外壳命令的请求结构体也定义在 src-tauri 里，一并收进来
foreach ($file in Get-ChildItem (Join-Path $RepoRoot 'src-tauri\src') -File -Filter *.rs) {
    foreach ($entry in (Get-StructFields $file.FullName).GetEnumerator()) {
        if (-not $ipcStructs.ContainsKey($entry.Key)) { $ipcStructs[$entry.Key] = $entry.Value }
    }
}

# ---------- 收集界面侧：命令 → 字段名集合（只处理结构体字面量参数）----------
$uiCommands = @{}
foreach ($file in Get-ChildItem $uiDir -File -Filter *.rs) {
    $structs = Get-StructFields $file.FullName
    $text = Get-Content $file.FullName -Raw
    foreach ($m in [regex]::Matches($text, 'ipc::call[a-z_]*[^(]*\(\s*"(?<cmd>[a-z0-9_]+)"\s*,\s*(?<arg>[A-Za-z_][A-Za-z0-9_]*)\s*\{')) {
        $cmd = $m.Groups['cmd'].Value
        $ty = $m.Groups['arg'].Value
        if ($structs.ContainsKey($ty)) {
            $uiCommands[$cmd] = @($structs[$ty].Fields | ForEach-Object { $_.Accepted[0] })
        }
        else {
            # 结构体不在本文件：基本都是从 tr_domain 引入的 DTO / 共享类型（两侧同一份定义）
            $uiCommands[$cmd] = 'SHARED'
        }
    }
    foreach ($m in [regex]::Matches($text, 'ipc::call[a-z_]*[^(]*\(\s*"(?<cmd>[a-z0-9_]+)"\s*,\s*(?<arg>[a-z_][A-Za-z0-9_]*)\s*[\),]')) {
        if (-not $uiCommands.ContainsKey($m.Groups['cmd'].Value)) { $uiCommands[$m.Groups['cmd'].Value] = 'SHARED' }
    }
}

# ---------- 比较 ----------
$failures = New-Object System.Collections.Generic.List[string]
$warnings = New-Object System.Collections.Generic.List[string]
$compared = 0
$shared = New-Object System.Collections.Generic.List[string]
$onlyIpc = New-Object System.Collections.Generic.List[string]

foreach ($cmd in ($uiCommands.Keys | Sort-Object)) {
    $uiFields = $uiCommands[$cmd]
    if ($uiFields -eq 'SHARED') { $shared.Add($cmd); continue }
    if (-not $ipcCommands.ContainsKey($cmd)) { $onlyIpc.Add($cmd); continue }

    $ipcTy = $ipcCommands[$cmd]
    if (-not $ipcStructs.ContainsKey($ipcTy)) { $shared.Add($cmd); continue }   # 请求类型来自 tr-domain（共享）

    $compared++
    $uiSet = @($uiFields)
    foreach ($field in $ipcStructs[$ipcTy].Fields) {
        $hit = $false
        foreach ($accepted in $field.Accepted) { if ($uiSet -contains $accepted) { $hit = $true; break } }
        if (-not $hit) {
            $names = ($field.Accepted -join ' / ')
            if ($field.Optional) {
                $warnings.Add("$cmd ：界面未提供可选字段 $names（走默认值）")
            }
            else {
                $failures.Add("$cmd （$ipcTy）：界面没有提供必填字段 $names —— 命令会报参数反序列化错误")
            }
        }
    }
    # 反向：界面发出去的字段必须至少被某个字段接受，否则会被 serde 静默忽略
    $allAccepted = @($ipcStructs[$ipcTy].Fields | ForEach-Object { $_.Accepted } | ForEach-Object { $_ })
    foreach ($sent in $uiSet) {
        if ($allAccepted -notcontains $sent) {
            $failures.Add("$cmd （$ipcTy）：界面发送的 `"$sent`" 不被任何字段接受 —— 会被 serde 忽略（静默失效）")
        }
    }
}

Write-Host "[contract-audit] 逐命令比较了 $compared 个结构体请求" -ForegroundColor Cyan
Write-Host "[contract-audit] 跳过（界面直接传 tr_domain 共享类型，不可能漂移）$($shared.Count) 个：$($shared -join ' ')" -ForegroundColor DarkGray
if ($onlyIpc.Count -gt 0) {
    Write-Host "[contract-audit] 界面调用了 tr-ipc / 外壳里没有的命令：$($onlyIpc -join ' ')" -ForegroundColor Yellow
}
if ($warnings.Count -gt 0) {
    Write-Host "[contract-audit] 提示（可选字段，使用默认值）：$($warnings.Count) 条" -ForegroundColor DarkGray
    $warnings | ForEach-Object { Write-Host "  · $_" -ForegroundColor DarkGray }
}

if ($failures.Count -gt 0) {
    Write-Host "`n[contract-audit] ❌ $($failures.Count) 处契约不一致：" -ForegroundColor Red
    $failures | ForEach-Object { "  - $_" }
    exit 1
}

Write-Host "`n[contract-audit] ✅ 界面请求与 tr-ipc/外壳契约逐字段一致（别名与可选字段已计入）" -ForegroundColor Green
exit 0
