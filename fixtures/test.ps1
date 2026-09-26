# test.ps1 —— 测试的统一入口：**单元档**（改了某功能时跑的相关测试）与**全量档**（发布前）
#
# 为什么要有它：
#   * 以前没有入口 —— "改了这个功能该跑哪几个脚本"只写在 AGENTS.md 的表格里，靠记忆挑；
#     跑全量更是手工排顺序，漏一个也不会有人告诉你。
#   * 每个护栏脚本各自往 `target\` 下写自己的临时目录（二十来个），target 顶层很快就糊了。
#   * 没有统一的结果汇总：失败要逐个脚本去翻输出。
#
# 它做四件事：
#   1. 把"功能 → 该跑哪些测试"固化成本文件里的 `$Groups`（唯一事实来源，AGENTS.md 只描述原则）；
#   2. 按固定顺序驱动各步骤，输出落 `target\tests\_runs\<时间>-<档位>\<NN>-<步骤>.log`；
#   3. 跑界面护栏前统一检查前置条件（产物是否存在、是否有实例占着单实例锁）；
#   4. 结尾打印并写出汇总（`summary.md` / `summary.json`），有失败就以非零码退出。
#
# 用法（pwsh 7）：
#   pwsh -File fixtures/test.ps1 -List                        # 列出功能分组与步骤
#   pwsh -File fixtures/test.ps1 -Unit stock                  # 改了股票功能 → 跑相关测试
#   pwsh -File fixtures/test.ps1 -Unit stock,service          # 多个分组
#   pwsh -File fixtures/test.ps1 -Unit changed                # 按 git 改动自动挑分组
#   pwsh -File fixtures/test.ps1 -All                         # 发布前全量（先构建，再跑全部护栏）
#   pwsh -File fixtures/test.ps1 -All -SkipBuild -SkipNetwork # 复用现有产物 / 跳过联网步骤
#   pwsh -File fixtures/test.ps1 -Unit core -DryRun           # 只打印计划，不执行
#
# 目录约定（见 fixtures/README.md）：
#   target\tests\<脚本名>\{home,out,ws}   每个护栏自己的工作目录（可随手删）
#   target\tests\_runs\<时间>-<档位>\     本次运行的日志与汇总

param(
    # 功能分组（可多个）；也接受 'changed'（按 git 改动挑）与单个步骤名（如 ui-stock）
    [string[]]$Unit,
    # 全量档：构建 + 全部分组
    [switch]$All,
    # 列出所有分组与步骤
    [switch]$List,
    # 只打印将要执行的内容
    [switch]$DryRun,
    # 全量档跳过构建（要求产物已存在）
    [switch]$SkipBuild,
    # 跳过依赖外部网络的步骤（真实行情 / GitHub 更新检查）
    [switch]$SkipNetwork,
    # 不检查"是否有实例在占单实例锁"（不推荐，护栏会因为窗口起不来而红）
    [switch]$KeepRunning,
    # 只跑 id 匹配该正则的步骤（调试用）
    [string]$Filter,
    # 覆盖运行日志目录
    [string]$OutDir
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$repo = Split-Path -Parent $PSScriptRoot
$exeRelease = Join-Path $repo 'target\release\transactions.exe'

# 调用方的环境变量会污染测试，这里统一清掉（子进程继承本进程的环境）：
#   * RUST_LOG —— 被测应用用 EnvFilter::try_from_default_env() 读它；外层若设成 warn/error，
#     应用一条 INFO 都不写，smoke / ui-features 之类的"日志断言"就会红（看着像回归，其实是环境）。
#   * NO_COLOR —— trunk 收到 `NO_COLOR=1` 会直接报 `invalid value '1' for '--no-color'`。
Remove-Item Env:RUST_LOG -ErrorAction SilentlyContinue
Remove-Item Env:NO_COLOR -ErrorAction SilentlyContinue

# ---------------------------------------------------------------- 步骤表
# Kind：cargo（cargo 子命令）/ pwsh（脚本，路径相对仓库根）
# Needs：exe —— 需要 target\release\transactions.exe，且必须没有实例在跑（单实例锁）
$Steps = [ordered]@{
    # ---- 构建（全量档默认跑；单元档不跑）----
    'build-ui'          = @{ Title='构建界面（trunk，跳过 wasm-opt）';                Kind='pwsh';  Script='build\build-ui.ps1'; Needs='none' }
    'build-app'         = @{ Title='构建外壳（release + custom-protocol）';           Kind='cargo'; Args=@('build','--release','-p','transactions','--features','tauri/custom-protocol'); Needs='none' }

    # ---- 每次必跑的三条便宜的（+ 契约审计）----
    'fmt'               = @{ Title='cargo fmt --check';                              Kind='cargo'; Args=@('fmt','--check'); Needs='none' }
    'clippy'            = @{ Title='cargo clippy --all-targets -D warnings';         Kind='cargo'; Args=@('clippy','--all-targets','--','-D','warnings'); Needs='none' }
    'design-audit'      = @{ Title='设计令牌审计（tokens.css 之外的硬编码色）';      Kind='pwsh';  Script='fixtures\design-audit.ps1'; Needs='none' }
    'contract-audit'    = @{ Title='IPC 入参契约审计（界面 ↔ tr-ipc）';              Kind='pwsh';  Script='fixtures\contract-audit.ps1'; Needs='none' }

    # ---- Rust 单元测试（按包）----
    'test-domain'       = @{ Title='cargo test -p tr-domain';                        Kind='cargo'; Args=@('test','-p','tr-domain'); Needs='none' }
    'test-store'        = @{ Title='cargo test -p tr-store';                         Kind='cargo'; Args=@('test','-p','tr-store'); Needs='none' }
    'test-service'      = @{ Title='cargo test -p tr-service';                       Kind='cargo'; Args=@('test','-p','tr-service'); Needs='none' }
    'test-ipc'          = @{ Title='cargo test -p tr-ipc';                           Kind='cargo'; Args=@('test','-p','tr-ipc'); Needs='none' }
    'check-ui-wasm'     = @{ Title='cargo check -p tr-ui --target wasm32';           Kind='cargo'; Args=@('check','-p','tr-ui','--target','wasm32-unknown-unknown'); Needs='none' }

    # ---- 结构 / 纯函数 ----
    'schema-diff'       = @{ Title='建库护栏：Rust 建库 vs fixtures/schema/fresh.sql'; Kind='cargo'; Args=@('xtask','schema-diff'); Needs='none' }
    'chart-tests'       = @{ Title='图表纯函数（Y 轴范围 / 填充基线）';              Kind='pwsh';  Script='fixtures\chart-tests.ps1'; Needs='none' }

    # ---- 界面护栏（真实启动 + UIA 驱动）----
    'smoke'             = @{ Title='冒烟：已配置 → 只开主窗口；首启动 → 只开初始化窗口'; Kind='pwsh'; Script='fixtures\smoke.ps1'; Needs='exe' }
    'window-bounds'     = @{ Title='窗口几何：逻辑尺寸 × DPI = 物理尺寸，关闭写回';   Kind='pwsh';  Script='fixtures\window-bounds.ps1'; Needs='exe' }
    'close-behavior'    = @{ Title='关闭行为：quit / tray / 弹「关闭选项」';         Kind='pwsh';  Script='fixtures\close-behavior.ps1'; Needs='exe' }
    'migrate-workspace' = @{ Title='迁移引擎：降级 → 升级 → 备份 → 幂等';           Kind='pwsh';  Script='fixtures\migrate-workspace.ps1'; Needs='exe' }
    'ui-smoke'          = @{ Title='逐页冒烟：5 顶级 + 9 子功能';                    Kind='pwsh';  Script='fixtures\ui-smoke.ps1'; Needs='exe' }
    'ui-shots'          = @{ Title='逐页截图：非空白 + 浅/深色亮度对比';             Kind='pwsh';  Script='fixtures\ui-shots.ps1'; Needs='exe' }
    'ui-transactions'   = @{ Title='记账·记录：记一笔 / 编辑 / 排序 / 筛选 / 模板';  Kind='pwsh';  Script='fixtures\ui-transactions.ps1'; Needs='exe' }
    'ui-crud'           = @{ Title='分类 / 标签 / 图表 / 事件 / 模板的新增与删除';   Kind='pwsh';  Script='fixtures\ui-crud.ps1'; Needs='exe' }
    'ui-drag'           = @{ Title='标签拖拽排序（真实鼠标按下→移动→抬起）';       Kind='pwsh';  Script='fixtures\ui-drag.ps1'; Needs='exe' }
    'ui-key-event'      = @{ Title='事件页：非今天建事件 / upsert / 删除';          Kind='pwsh';  Script='fixtures\ui-key-event.ps1'; Needs='exe' }
    'ui-link-event'     = @{ Title='关联交易到事件：选日期 / 懒创建 / 解除关联';     Kind='pwsh';  Script='fixtures\ui-link-event.ps1'; Needs='exe' }
    'ui-diary-edit'     = @{ Title='日记：防抖自动保存 / 心情 / 预览 / 删除';       Kind='pwsh';  Script='fixtures\ui-diary-edit.ps1'; Needs='exe' }
    'ui-diary-ledger'   = @{ Title='日记的账本隔离（复合唯一键）';                   Kind='pwsh';  Script='fixtures\ui-diary-ledger.ps1'; Needs='exe' }
    'ui-diary-io'       = @{ Title='日记导入导出（只认 .txt，UTF-8/GBK）';          Kind='pwsh';  Script='fixtures\ui-diary-io.ps1'; Needs='exe' }
    'ui-sync-ledger'    = @{ Title='记录同步到其他账本（复制而非移动）';             Kind='pwsh';  Script='fixtures\ui-sync-ledger.ps1'; Needs='exe' }
    'ui-upload'         = @{ Title='图片上传 / 缩略图 / trasset 资产协议';          Kind='pwsh';  Script='fixtures\ui-upload.ps1'; Needs='exe' }
    'ui-proxy'          = @{ Title='代理设置（假代理日志是判据）';                 Kind='pwsh';  Script='fixtures\ui-proxy.ps1'; Needs='exe' }
    'ui-about'          = @{ Title='关于软件 / 版本自报';                          Kind='pwsh';  Script='fixtures\ui-about.ps1'; Needs='exe' }
    'ui-features'       = @{ Title='功能开关（侧栏当场少一项 / 重启后仍生效）';      Kind='pwsh';  Script='fixtures\ui-features.ps1'; Needs='exe' }
    'ui-stock'          = @{ Title='股票全生命周期（含真实行情）';                  Kind='pwsh';  Script='fixtures\ui-stock.ps1'; Needs='exe'; Network=$true }
    'ui-update-restore' = @{ Title='更新下载状态跨页面恢复（依赖 GitHub API）';     Kind='pwsh';  Script='fixtures\ui-update-restore.ps1'; Needs='exe'; Network=$true }
}

# ---------------------------------------------------------------- 功能分组（单元档）
# 一处改动 → 该跑哪些步骤。新增护栏脚本时必须同时登记进某个分组（见 -List 与 AGENTS.md）。
$Groups = [ordered]@{
    'core'         = @('fmt', 'clippy', 'design-audit', 'contract-audit')
    'domain'       = @('test-domain')
    'store'        = @('test-store', 'schema-diff')
    'service'      = @('test-service')
    'ipc'          = @('test-ipc', 'contract-audit')
    'ui'           = @('check-ui-wasm', 'design-audit')
    'chart'        = @('chart-tests')
    'schema'       = @('schema-diff', 'migrate-workspace')
    # 记账 · 记录：记一笔 / 编辑 / 排序 / 筛选 / 模板，以及"同步到其他账本"（ui-sync-ledger 属于这条链路）
    'accounting'   = @('ui-transactions', 'ui-sync-ledger')
    'category-tag' = @('ui-crud', 'ui-drag')
    'analysis'     = @('chart-tests', 'ui-crud')
    'templates'    = @('ui-crud', 'ui-transactions')
    'key-event'    = @('ui-key-event', 'ui-link-event')
    'diary'        = @('ui-diary-edit', 'ui-diary-ledger', 'ui-diary-io')
    'stock'        = @('ui-stock')
    'settings'     = @('ui-proxy', 'ui-about', 'ui-features')
    'assets'       = @('ui-upload')
    'shell'        = @('smoke', 'window-bounds', 'close-behavior')
    # 共享组件 / 全局样式 / 侧栏骨架：影响面按 AGENTS.md 的约定放大到"覆盖到的代表页"
    'ui-kit'       = @('design-audit', 'check-ui-wasm', 'ui-smoke', 'ui-shots', 'ui-transactions', 'ui-stock')
    'update'       = @('ui-update-restore')
}

# ---------------------------------------------------------------- 改动 → 分组（-Unit changed）
# 顺序敏感：从上往下第一个匹配的规则生效。
$PathMap = @(
    @{ Re='^fixtures/lib/TrUia\.ps1$';                    Groups=@('all') }  # 公共 UIA 底座 → 按全量处理
    @{ Re='^fixtures/(?<name>[a-z0-9-]+)\.ps1$';          Script=$true }     # 改了某个护栏 → 跑它自己
    @{ Re='^fixtures/';                                   Groups=@('core') }
    @{ Re='^crates/tr-domain/src/(money|fee|consts|proxy|error)\.rs$'; Groups=@('domain') }
    @{ Re='^crates/tr-domain/src/models/';                Groups=@('domain', 'store') }
    @{ Re='^crates/tr-domain/src/dto/';                   Groups=@('domain', 'ipc') }
    @{ Re='^crates/tr-domain/';                           Groups=@('domain') }
    @{ Re='^crates/tr-store/src/(migrations|schema|workspace)\.rs$'; Groups=@('store', 'schema') }
    @{ Re='^crates/tr-store/';                            Groups=@('store') }
    @{ Re='^crates/tr-service/src/stock';                 Groups=@('stock', 'service') }
    @{ Re='^crates/tr-service/src/quote\.rs$';            Groups=@('stock') }
    @{ Re='^crates/tr-service/src/diary\.rs$';            Groups=@('diary') }
    @{ Re='^crates/tr-service/src/key_event\.rs$';        Groups=@('key-event') }
    @{ Re='^crates/tr-service/src/(category|tag|chart)\.rs$'; Groups=@('category-tag', 'analysis') }
    @{ Re='^crates/tr-service/src/transaction_record\.rs$';    Groups=@('accounting') }
    @{ Re='^crates/tr-service/src/transaction_template\.rs$';  Groups=@('templates') }
    @{ Re='^crates/tr-service/src/assets\.rs$';           Groups=@('assets') }
    @{ Re='^crates/tr-service/src/proxy\.rs$';            Groups=@('settings') }
    @{ Re='^crates/tr-service/';                          Groups=@('service') }
    @{ Re='^crates/tr-ipc/';                              Groups=@('ipc') }
    @{ Re='^crates/tr-ui/src/api/';                       Groups=@('ipc') }
    @{ Re='^crates/tr-ui/src/pages/stock\.rs$';           Groups=@('stock') }
    @{ Re='^crates/tr-ui/src/pages/diary\.rs$';           Groups=@('diary') }
    @{ Re='^crates/tr-ui/src/pages/key_event\.rs$';       Groups=@('key-event') }
    @{ Re='^crates/tr-ui/src/pages/category_tag\.rs$';    Groups=@('category-tag') }
    @{ Re='^crates/tr-ui/src/pages/data_analysis\.rs$';   Groups=@('analysis') }
    @{ Re='^crates/tr-ui/src/pages/templates\.rs$';       Groups=@('templates') }
    @{ Re='^crates/tr-ui/src/pages/(transactions|accounting)\.rs$'; Groups=@('accounting') }
    @{ Re='^crates/tr-ui/src/pages/settings\.rs$';        Groups=@('settings') }
    @{ Re='^crates/tr-ui/src/components/ui/';             Groups=@('ui-kit') }
    @{ Re='^crates/tr-ui/src/(shell|store|icons|notify|error_handler|format|time)\.rs$'; Groups=@('ui-kit') }
    @{ Re='^crates/tr-ui/static/css/';                    Groups=@('ui-kit') }
    @{ Re='^crates/tr-ui/(index\.html|Trunk\.toml)$';     Groups=@('ui-kit') }
    @{ Re='^crates/tr-ui/';                               Groups=@('ui-kit') }
    @{ Re='^src-tauri/';                                  Groups=@('shell') }
    @{ Re='^(Cargo\.toml|Cargo\.lock|\.cargo/config\.toml)$'; Groups=@('core') }
)

# ---------------------------------------------------------------- 小工具
function Write-Head($msg) { Write-Host "`n$msg" -ForegroundColor Magenta }
function Write-Ok($msg) { Write-Host "  ✓ $msg" -ForegroundColor Green }
function Write-Bad($msg) { Write-Host "  ✗ $msg" -ForegroundColor Red }

function Get-Plan {
    # 去重并按 $Steps 的定义顺序排序（保证"先构建、再静态检查、最后界面护栏"）
    param([string[]]$Ids)
    $want = New-Object System.Collections.Generic.HashSet[string]
    foreach ($id in $Ids) { [void]$want.Add($id) }
    return @($Steps.Keys | Where-Object { $want.Contains($_) })
}

function Resolve-Groups {
    param([string[]]$Names)
    $ids = New-Object System.Collections.Generic.List[string]
    $unknown = New-Object System.Collections.Generic.List[string]
    foreach ($name in $Names) {
        # ⚠ 必须写成 [string[]]($Groups[...])：`[string[]]$Groups[$name]` 会被解析成
        # "先把 $Groups 转成数组、再按 $name 索引" → 拿到 $null（PowerShell 的经典坑）
        if ($name -eq 'all') { foreach ($g in $Groups.Keys) { $ids.AddRange([string[]]($Groups[$g])) } }
        elseif ($Groups.Contains($name)) { $ids.AddRange([string[]]($Groups[$name])) }
        elseif ($Steps.Contains($name)) { $ids.Add($name) }
        else { $unknown.Add($name) }
    }
    if ($unknown.Count -gt 0) { throw "未知的分组/步骤：$($unknown -join ', ')（用 -List 看可选值）" }
    return $ids.ToArray()
}

function Resolve-Changed {
    $changed = @(& git -C $repo status --porcelain=v1 2>$null | ForEach-Object {
            $p = $_.Substring(3).Trim('"')
            if ($p -match ' -> ') { $p = ($p -split ' -> ')[-1] }
            $p -replace '\\', '/'
        })
    if ($changed.Count -eq 0) {
        Write-Host '（工作区没有改动 → 只跑 core）' -ForegroundColor DarkGray
        return @('core')
    }
    $hit = New-Object System.Collections.Generic.List[string]
    $ignored = New-Object System.Collections.Generic.List[string]
    foreach ($path in $changed) {
        foreach ($rule in $PathMap) {
            if (-not [regex]::IsMatch($path, $rule.Re)) { continue }
            if ($rule.ContainsKey('Script')) {
                # 改了某个护栏脚本 → 跑它自己；不是测试步骤的脚本（dev-hot / dev-shot / test.ps1 等）忽略
                $scriptId = [System.IO.Path]::GetFileNameWithoutExtension($path)
                if ($Steps.Contains($scriptId)) { $hit.Add($scriptId) } else { $ignored.Add($path) }
            }
            else { foreach ($g in $rule.Groups) { $hit.Add($g) } }
            break
        }
    }
    if ($ignored.Count -gt 0) {
        Write-Host ('（以下改动不是测试步骤，已忽略：' + ($ignored -join ', ') + '）') -ForegroundColor DarkGray
    }
    # 每轮必跑的四条便宜的（fmt / clippy / design-audit / contract-audit）永远带上
    $hit.Add('core')
    return @($hit | Sort-Object -Unique)
}

function Test-NeededExe {
    if (-not (Test-Path $exeRelease)) {
        throw "缺少 $exeRelease —— 先跑 -All（它会构建），或先手动构建：cargo build --release -p transactions --features tauri/custom-protocol"
    }
}

function Assert-NoRunningInstance {
    if ($KeepRunning) { return }
    $running = @(Get-Process -Name transactions -ErrorAction SilentlyContinue | ForEach-Object {
            $p = try { $_.Path } catch { $null }
            if ($p) { "PID $($_.Id): $p" }
        })
    if ($running.Count -gt 0) {
        throw ("有 Transactions 实例在运行，单实例插件会让护栏窗口起不来：`n    " +
            ($running -join "`n    ") +
            "`n  先退出它，或加 -KeepRunning 跳过检查（护栏多半会红）。")
    }
}

function Invoke-Step {
    param([string]$Id, [string]$LogPath)
    $def = $Steps[$Id]
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    Push-Location $repo
    try {
        if ($def.Kind -eq 'cargo') {
            & cargo @($def.Args) *> $LogPath
            $code = $LASTEXITCODE
        }
        else {
            & pwsh -NoProfile -ExecutionPolicy Bypass -File (Join-Path $repo $def.Script) *> $LogPath
            $code = $LASTEXITCODE
        }
    }
    finally { Pop-Location; $sw.Stop() }
    return @{ Code = $code; Seconds = [math]::Round($sw.Elapsed.TotalSeconds, 1) }
}

# ---------------------------------------------------------------- 自检：步骤表与分组表不能脱节
# 构建步骤只属于全量档，不登记进任何功能分组；其余每个步骤都必须（a）脚本存在（b）至少属于一个分组，
# 否则 -All 会静默漏跑它 —— "加了护栏却从没被跑过"正是要靠这条挡住。
# 注意：下面几条消息**不要**写成 markdown 那种反引号引用（双引号串里 `" 会把结束引号转义掉）。
$buildOnly = @('build-ui', 'build-app')
$registered = New-Object System.Collections.Generic.HashSet[string]
foreach ($g in $Groups.Keys) { foreach ($id in $Groups[$g]) { [void]$registered.Add($id) } }
foreach ($id in $Steps.Keys) {
    $def = $Steps[$id]
    if (-not (Test-Path (Join-Path $repo $def.Script))) { throw "步骤 $id 的脚本不存在：$($def.Script)" }
    if ($id -notin $buildOnly -and -not $registered.Contains($id)) {
        throw "步骤 $id 没有登记进任何分组 —— 加护栏时要同时登记 Steps 表与 Groups 表"
    }
}
foreach ($id in $registered) {
    if (-not $Steps.Contains($id)) { throw "分组里引用了不存在的步骤：$id" }
}

# ---------------------------------------------------------------- -List
if ($List) {
    Write-Head '功能分组（单元档）：pwsh -File fixtures/test.ps1 -Unit <分组>'
    foreach ($g in $Groups.Keys) {
        Write-Host ('  {0,-14} {1}' -f $g, (($Groups[$g]) -join ', '))
    }
    Write-Host '  changed        按 git 改动自动挑分组（见本文件 $PathMap）'
    Write-Head '步骤（全量档按此顺序全部跑；单元档只跑分组里点到的）'
    foreach ($id in $Steps.Keys) {
        $s = $Steps[$id]
        $tag = @()
        if ($s.Needs -eq 'exe') { $tag += 'e2e' }
        if ($s.Network) { $tag += 'network' }
        Write-Host ('  {0,-18} {1,-48} [{2}]' -f $id, $s.Title, ($tag -join ','))
    }
    Write-Host ''
    exit 0
}

# ---------------------------------------------------------------- 解析这次跑什么
$tier = 'unit'
if ($All) {
    $tier = 'full'
    $selectedGroups = @($Groups.Keys)
}
elseif ($Unit -and $Unit.Count -gt 0) {
    $selectedGroups = @()
    # `pwsh -File script.ps1 -Unit a,b` 传进来的是一整串 "a,b"（-File 不做数组拆分），这里自己拆
    $unitNames = @($Unit | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
    foreach ($u in $unitNames) {
        if ($u -eq 'changed') { $selectedGroups += (Resolve-Changed) }
        else { $selectedGroups += $u }
    }
}
else {
    Write-Host '没有指定要跑什么 —— 默认只跑 core（每轮必跑的四条）。用 -List 看全部分组。' -ForegroundColor Yellow
    $selectedGroups = @('core')
}

$plannedIds = @(Resolve-Groups $selectedGroups)
# 构建步骤不属于任何功能分组（单元档不该跑构建）：只有全量档显式加上，且排在其它步骤之前
if ($All -and -not $SkipBuild) { $plannedIds += @('build-ui', 'build-app') }
$plan = Get-Plan $plannedIds
if ($Filter) { $plan = @($plan | Where-Object { $_ -match $Filter }) }
if ($plan.Count -eq 0) { throw '这次没有任何步骤要跑（检查 -Unit / -Filter）' }

$runId = (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + $tier
if (-not $OutDir) { $OutDir = Join-Path $repo "target\tests\_runs\$runId" }

Write-Head "测试计划（$tier）：$($plan.Count) 步，日志目录 $OutDir"
$i = 0
foreach ($id in $plan) {
    $i++
    $mark = if ($SkipNetwork -and $Steps[$id].Network) { '⏭ ' } else { '   ' }
    Write-Host ('{0}{1,2}. {2,-18} {3}' -f $mark, $i, $id, $Steps[$id].Title)
}
if ($SkipNetwork) { Write-Host '  （⏭ = -SkipNetwork 会跳过）' -ForegroundColor DarkGray }
if ($DryRun) { Write-Host "`n-dryRun：只打印计划，未执行。" -ForegroundColor Yellow; exit 0 }
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Force -Path $OutDir | Out-Null }

# ---------------------------------------------------------------- 执行
$results = New-Object System.Collections.Generic.List[object]
$exeChecked = $false
$startedAt = Get-Date
$i = 0
foreach ($id in $plan) {
    $i++
    $def = $Steps[$id]
    $log = Join-Path $OutDir ('{0:d2}-{1}.log' -f $i, $id)

    if ($SkipNetwork -and $def.Network) {
        Write-Host ('⏭ [{0}/{1}] {2}（-SkipNetwork 跳过）' -f $i, $plan.Count, $id) -ForegroundColor DarkGray
        $results.Add(@{ Id = $id; Title = $def.Title; Status = '跳过'; Seconds = 0; Log = $log; Code = $null })
        continue
    }

    if ($def.Needs -eq 'exe') {
        Test-NeededExe
        if (-not $exeChecked) { Assert-NoRunningInstance; $exeChecked = $true }
    }

    Write-Host ('▶ [{0}/{1}] {2} —— {3}' -f $i, $plan.Count, $id, $def.Title) -ForegroundColor Cyan
    $r = Invoke-Step -Id $id -LogPath $log
    if ($r.Code -eq 0) {
        Write-Ok ("{0}（{1}s）" -f $id, $r.Seconds)
        $results.Add(@{ Id = $id; Title = $def.Title; Status = '通过'; Seconds = $r.Seconds; Log = $log; Code = 0 })
    }
    else {
        Write-Bad ("{0} 退出码 {1}（{2}s）—— 日志尾部：" -f $id, $r.Code, $r.Seconds)
        Get-Content $log -Tail 15 | ForEach-Object { Write-Host "      $_" -ForegroundColor DarkGray }
        $results.Add(@{ Id = $id; Title = $def.Title; Status = '失败'; Seconds = $r.Seconds; Log = $log; Code = $r.Code })
    }
}

# ---------------------------------------------------------------- 汇总
$failed = @($results | Where-Object { $_.Status -eq '失败' })
$passed = @($results | Where-Object { $_.Status -eq '通过' })
$skipped = @($results | Where-Object { $_.Status -eq '跳过' })
$elapsed = [math]::Round(((Get-Date) - $startedAt).TotalSeconds, 1)

Write-Head '汇总'
foreach ($r in $results) {
    $color = switch ($r.Status) { '通过' { 'Green' } '失败' { 'Red' } default { 'DarkGray' } }
    Write-Host ('  {0,-4} {1,-18} {2,7}s   {3}' -f $r.Status, $r.Id, $r.Seconds, (Split-Path $r.Log -Leaf)) -ForegroundColor $color
}
Write-Host ("`n  档位 {0}：通过 {1} / 失败 {2} / 跳过 {3}，用时 {4}s" -f $tier, $passed.Count, $failed.Count, $skipped.Count, $elapsed) -ForegroundColor $(if ($failed.Count) { 'Red' } else { 'Green' })

$summary = @()
$summary += "# 测试记录（$tier）"
$summary += ''
$summary += "- 时间：$($startedAt.ToString('yyyy-MM-dd HH:mm:ss')) → $(Get-Date -Format 'HH:mm:ss')（$elapsed s）"
$summary += "- 分组：$(($selectedGroups) -join ', ')"
$summary += "- 结果：通过 $($passed.Count) / 失败 $($failed.Count) / 跳过 $($skipped.Count)"
$summary += ''
$summary += '| 结果 | 步骤 | 说明 | 用时(s) | 日志 |'
$summary += '|---|---|---|---|---|'
foreach ($r in $results) {
    $summary += ('| {0} | {1} | {2} | {3} | {4} |' -f $r.Status, $r.Id, $r.Title, $r.Seconds, (Split-Path $r.Log -Leaf))
}
$summary | Set-Content -Encoding UTF8 (Join-Path $OutDir 'summary.md')
@{
    tier      = $tier
    groups    = @($selectedGroups)
    startedAt = $startedAt.ToString('o')
    seconds   = $elapsed
    passed    = $passed.Count
    failed    = $failed.Count
    skipped   = $skipped.Count
    steps     = @($results | ForEach-Object { @{ id = $_.Id; status = $_.Status; seconds = $_.Seconds; log = (Split-Path $_.Log -Leaf); code = $_.Code } })
} | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 (Join-Path $OutDir 'summary.json')

if ($failed.Count) { exit 1 }
exit 0
