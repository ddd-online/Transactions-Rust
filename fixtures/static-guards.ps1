# static-guards.ps1 —— 跨文件的静态护栏（"写进文档也拦不住"的那几条）。
#
# 为什么需要它：这些约定原本各自是 AGENTS.md 里的一段话，而话拦不住人 —— 改一处、忘另一处
# 的时候不会有任何东西变红。这里把它们变成能跑的断言，文档只留一句"去这里看"。
#
# 检查项：
#   1. 开发端口四处一致：`Trunk.toml` 的 `[serve] port`、`tauri.conf.json` 的 `devUrl`、
#      `dev-hot.ps1` 的 `-Port` 默认值、`shell.rs` 导航单测里的 localhost 样例；
#   2. 自定义滚动条禁令：全站滚动条是 WebView2 的**覆盖式**（`shell.rs` 的 WEBVIEW_ARGS），
#      自己写 `::-webkit-scrollbar` / `scrollbar-width` 会让 Blink 退回"占宽度"那条老路；
#      刻意的例外要在规则上方 8 行内写 `scrollbar-ban-exempt` 注释；
#   3. 构建 / 发布的机械纪律：
#      * `tauri.conf.json` 的 `beforeBuildCommand` 必须落到 `build/build-ui.ps1`；
#      * 那个脚本被 **PowerShell 5.1** 调用，必须 ASCII-only（5.1 把无 BOM 的 UTF-8 当 ANSI）；
#      * 含中文的 `build/*.ps1` 必须带 pwsh 7 自举守卫（`$PSVersionTable.PSEdition`）；
#      * 任何脚本里不许出现 `trunk build --release`（缓存里的 wasm-opt 123 校验不了 Rust 1.96
#        的 bulk-memory 指令），发布构建只能走 `build/build-ui.ps1`；
#      * 全量档的 `build-app` 步骤必须带 `tauri/custom-protocol`（否则窗口里是"拒绝连接"）；
#   4. `wasm-bindgen` CLI 的版本 == `Cargo.lock` 里 `wasm-bindgen` crate 的版本
#      （版本不匹配时 trunk 的产物会以各种"界面空白"的面目出现）；
#   5. 分层：`tr-domain` 的依赖树里不许有 I/O 依赖（它同时被 native 与 wasm 依赖）；
#   6. 界面 markup 里写出来的类名必须在 CSS 里有规则（"删了规则忘了改调用点"是静默失效：
#      从前 `.st-pane` 被删、`<div class="st-pane st-about">` 还在，居中就没了）；
#   7. 文档里的引用不烂：反引号引用的仓库内路径必须存在、发布资产名不许写死版本号、
#      "`AGENTS.md` 的「小节」"这类引用要对得上真实标题；
#   8. 发布说明（`build/release_notes.md`，骨架见 `build/release_template.md`）合不合模板：
#      版本标题必须等于 `tauri.conf.json` 的版本（对不上 = 拿上一次的说明发新版）、
#      四节（新增 / 修改 / 调整 / 优化）顺序固定且不许出现别的节、必须是无序列表、
#      每项 ≤50 字、空节不许留标题、同一项不许跨节重复；
#   9. 版本号三处一致：`tauri.conf.json` / `Cargo.toml` 的 `[workspace.package]` /
#      `src-tauri/Cargo.toml` 的 `[package]`。这条从前只写在文档里，而它**看不出来**
#      （关于页与安装包自报的版本都来自 `tauri.conf.json`，Cargo.toml 漂了照报正确版本），
#      所以必须是断言。`Cargo.lock` 刻意不查 —— 理由见那一项自己的注释。
#
# 用法（pwsh 7）：pwsh -File fixtures/static-guards.ps1 [-OutDir <dir>]
# 退出码：0 = 通过，1 = 有失败项。

param(
    [string]$OutDir
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
if ($OutDir) { New-Item -ItemType Directory -Force -Path $OutDir | Out-Null }

$failures = New-Object System.Collections.Generic.List[string]
function Fail([string]$message) {
    $failures.Add($message)
    Write-Host "  ✗ $message" -ForegroundColor Red
}
function Pass([string]$message) { Write-Host "  ✓ $message" -ForegroundColor Green }

function Read-Text([string]$relative) {
    $path = Join-Path $repo $relative
    if (-not (Test-Path $path)) { throw "找不到文件：$relative" }
    return (Get-Content $path -Raw)
}

# ---------------------------------------------------------------- 1. 开发端口
Write-Host '[static-guards] 1/9 开发端口一致性' -ForegroundColor Cyan
$portSources = [ordered]@{}
$portSources['crates/tr-ui/Trunk.toml'] = @(
    [regex]::Matches((Read-Text 'crates/tr-ui/Trunk.toml'), '(?m)^\s*port\s*=\s*(\d+)') |
        ForEach-Object { $_.Groups[1].Value })
$portSources['src-tauri/tauri.conf.json'] = @(
    [regex]::Matches((Read-Text 'src-tauri/tauri.conf.json'), 'devUrl"\s*:\s*"[^"]*:(\d+)') |
        ForEach-Object { $_.Groups[1].Value })
$portSources['fixtures/dev-hot.ps1'] = @(
    [regex]::Matches((Read-Text 'fixtures/dev-hot.ps1'), '\[int\]\$Port\s*=\s*(\d+)') |
        ForEach-Object { $_.Groups[1].Value })
# shell.rs 的导航单测里写着 localhost / 127.0.0.1 的样例（放行侧），端口也必须对上
$portSources['src-tauri/src/shell.rs'] = @(
    [regex]::Matches((Read-Text 'src-tauri/src/shell.rs'), '://(?:localhost|127\.0\.0\.1):(\d+)') |
        ForEach-Object { $_.Groups[1].Value })

$allPorts = @()
foreach ($entry in $portSources.GetEnumerator()) {
    if ($entry.Value.Count -eq 0) { Fail "$($entry.Key)：没找到端口（判据变了？）"; continue }
    $allPorts += $entry.Value
}
$distinct = @($allPorts | Sort-Object -Unique)
if ($distinct.Count -eq 1) {
    Pass "四处一致：$($distinct[0])（$(($portSources.Keys) -join ' / ')、共 $($allPorts.Count) 处）"
}
else {
    foreach ($entry in $portSources.GetEnumerator()) {
        Write-Host "      $($entry.Key): $($entry.Value -join ', ')" -ForegroundColor DarkGray
    }
    Fail "开发端口不一致（$(($distinct) -join ' vs ')）—— 换端口要同时改 Trunk.toml / tauri.conf.json / dev-hot.ps1（shell.rs 单测样例跟着走）"
}

# ---------------------------------------------------------------- 2. 滚动条禁令
Write-Host '[static-guards] 2/9 自定义滚动条禁令' -ForegroundColor Cyan
$cssFiles = Get-ChildItem (Join-Path $repo 'crates/tr-ui/static/css') -Filter *.css -File
$scrollHits = New-Object System.Collections.Generic.List[string]
foreach ($file in $cssFiles) {
    $lines = Get-Content $file.FullName
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -notmatch '::-webkit-scrollbar|scrollbar-width\s*:|scrollbar-color\s*:') { continue }
        if ($lines[$i].TrimStart().StartsWith('*')) { continue }        # 注释正文（说明这条禁令本身）
        $from = [Math]::Max(0, $i - 8)
        $exempt = ($lines[$from..$i] -join "`n") -match 'scrollbar-ban-exempt'
        if (-not $exempt) { $scrollHits.Add("$($file.Name):$($i + 1)  $($lines[$i].Trim())") }
    }
}
if ($scrollHits.Count -eq 0) { Pass 'CSS 里没有自定义滚动条（例外都有 scrollbar-ban-exempt 标记）' }
else {
    foreach ($hit in $scrollHits) { Write-Host "      $hit" -ForegroundColor DarkGray }
    Fail "有 $($scrollHits.Count) 处自定义滚动条：会把全站的覆盖式滚动条顶掉（刻意的例外请写 scrollbar-ban-exempt 注释）"
}

# ---------------------------------------------------------------- 3. 构建 / 发布纪律
Write-Host '[static-guards] 3/9 构建与发布纪律' -ForegroundColor Cyan
$tauriConf = Read-Text 'src-tauri/tauri.conf.json'
if ($tauriConf -match '"beforeBuildCommand"\s*:\s*\{[^}]*"script"\s*:\s*"([^"]+)"') {
    $beforeBuild = $Matches[1]
    if ($beforeBuild -match 'build/build-ui\.ps1') { Pass "beforeBuildCommand → build/build-ui.ps1" }
    else { Fail "beforeBuildCommand 指向的不是 build/build-ui.ps1：$beforeBuild" }
}
else { Fail 'tauri.conf.json 里找不到 beforeBuildCommand.script' }

# 被 5.1 调用的那个脚本必须 ASCII-only（5.1 把无 BOM 的 UTF-8 当 ANSI 解码）
$buildUiPath = Join-Path $repo 'build/build-ui.ps1'
$buildUiBytes = [System.IO.File]::ReadAllBytes($buildUiPath)
$nonAscii = @($buildUiBytes | Where-Object { $_ -gt 127 }).Count
if ($nonAscii -eq 0) { Pass 'build/build-ui.ps1 是 ASCII-only（它由 powershell 5.1 调用）' }
else { Fail "build/build-ui.ps1 里有 $nonAscii 个非 ASCII 字节 —— 5.1 会把它当 ANSI 解析" }

# 含中文的 build/*.ps1 必须自己切回 pwsh 7（否则 5.1 下解析错乱、退出码却还是 0）
foreach ($file in Get-ChildItem (Join-Path $repo 'build') -Filter *.ps1 -File) {
    $text = Get-Content $file.FullName -Raw
    if ($text -notmatch '[^\x00-\x7F]') { continue }
    if ($text -match '\$PSVersionTable\.PSEdition') { Pass "$($file.Name)：含中文 + 有 pwsh 7 自举守卫" }
    else { Fail "$($file.Name) 含中文却没有 pwsh 7 自举守卫（5.1 下会被误解析）" }
}

# `trunk build --release` 不许出现在任何脚本里（注释里说明原因是可以的）
$trunkRelease = New-Object System.Collections.Generic.List[string]
foreach ($file in @(Get-ChildItem (Join-Path $repo 'build') -Filter *.ps1 -File) +
    @(Get-ChildItem (Join-Path $repo 'fixtures') -Filter *.ps1 -File) +
    @(Get-ChildItem (Join-Path $repo 'fixtures/lib') -Filter *.ps1 -File)) {
    $lines = Get-Content $file.FullName
    if ($file.Name -eq 'static-guards.ps1') { continue }   # 本脚本自己要点名这条禁令
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $line = $lines[$i]
        if ($line -notmatch 'trunk build --release') { continue }
        if ($line.TrimStart().StartsWith('#')) { continue }
        $trunkRelease.Add("$($file.Name):$($i + 1)")
    }
}
if ($trunkRelease.Count -eq 0) { Pass '没有脚本调用 trunk build --release' }
else { Fail "这些地方在调 trunk build --release：$($trunkRelease -join '、')（wasm-opt 123 校验不了 bulk-memory）" }

$testPs1 = Read-Text 'fixtures/test.ps1'
if ($testPs1 -match "'build-app'[\s\S]{0,400}?tauri/custom-protocol" -and $testPs1 -match "'build-app'[\s\S]{0,400}?--release") {
    Pass 'build-app 步骤带 --release + tauri/custom-protocol'
}
else { Fail 'test.ps1 的 build-app 步骤必须带 --release 与 --features tauri/custom-protocol（界面护栏用的就是这种产物）' }

# ---------------------------------------------------------------- 4. wasm-bindgen 版本
Write-Host '[static-guards] 4/9 wasm-bindgen CLI 版本' -ForegroundColor Cyan
$lock = Read-Text 'Cargo.lock'
if ($lock -match '(?m)^name = "wasm-bindgen"\r?\nversion = "([^"]+)"') {
    $lockVersion = $Matches[1]
    $cli = (Get-Command wasm-bindgen -ErrorAction SilentlyContinue)
    if (-not $cli) {
        Fail "PATH 里没有 wasm-bindgen（Cargo.lock 要 $lockVersion）：cargo install wasm-bindgen-cli --version $lockVersion --locked"
    }
    else {
        $cliVersion = ((& wasm-bindgen --version) -replace '^wasm-bindgen\s+', '').Trim()
        if ($cliVersion -eq $lockVersion) { Pass "CLI $cliVersion == Cargo.lock $lockVersion" }
        else { Fail "wasm-bindgen CLI $cliVersion != Cargo.lock $lockVersion（版本必须一致）" }
    }
}
else { Fail 'Cargo.lock 里读不到 wasm-bindgen 的版本' }

# ---------------------------------------------------------------- 5. 分层：tr-domain 无 I/O
Write-Host '[static-guards] 5/9 tr-domain 的依赖树（不许有 I/O）' -ForegroundColor Cyan
$treeFile = Join-Path ([System.IO.Path]::GetTempPath()) "tr-tree-$([guid]::NewGuid().ToString('N')).txt"
try {
    cargo tree -p tr-domain -e normal --prefix none *> $treeFile
    $tree = Get-Content $treeFile -Raw
    $banned = @('rusqlite', 'libsqlite3-sys', 'reqwest', 'ureq', 'tauri', 'wry', 'tokio', 'hyper', 'sqlx', 'mio', 'socket2')
    $found = @($banned | Where-Object { $tree -match "(?m)^\s*$([regex]::Escape($_))\s+v" })
    if ($found.Count -eq 0) { Pass "依赖树干净（查了 $($banned.Count) 个 I/O 依赖）" }
    else { Fail "tr-domain 的依赖树里有 I/O 依赖：$($found -join '、')" }
}
finally { Remove-Item $treeFile -Force -ErrorAction SilentlyContinue }

# ---------------------------------------------------------------- 6. markup 类名 → CSS 规则
Write-Host '[static-guards] 6/9 markup 里的类名都有 CSS 规则' -ForegroundColor Cyan
# 例外：**刻意**留着的钩子/块名（每条都在这里给理由，不写进文档）
$classAllow = @{
    'ui-popconfirm--end'    = '意图标记：贴哪条边对齐由组件算（popconfirm.rs 的 align_end），见 ui.css 注释'
    'todo-quadrant__ground' = 'BEM 块名：规则挂在四个 --now/--plan/--delegate/--less 修饰符上'
    'stock-trade-edit'      = 'BEM 块名：规则挂在 __head / __name 上'
    'todo-page'             = 'FeaturePage 的页面级样式钩子（class 插槽），当前没有页面级规则'
    'key-event-markdown'    = 'Markdown 组件的 class 插槽（与 md-body 叠加），当前没有额外规则'
}
$cssText = ($cssFiles | ForEach-Object { Get-Content $_.FullName -Raw }) -join "`n"
$cssText += Read-Text 'crates/tr-ui/index.html'
$defined = [System.Collections.Generic.HashSet[string]]::new()
foreach ($m in [regex]::Matches($cssText, '\.([A-Za-z][A-Za-z0-9_-]*)')) { [void]$defined.Add($m.Groups[1].Value) }

$undefined = [ordered]@{}
foreach ($file in Get-ChildItem (Join-Path $repo 'crates/tr-ui/src') -Filter *.rs -Recurse -File) {
    $lines = @(Get-Content $file.FullName | Where-Object { -not $_.TrimStart().StartsWith('//') })
    $body = $lines -join "`n"
    # 两种写法都要查：`class="a b"` 与 Leptos 的条件类 `class:is-x=…`。
    # 后者漏过一次：`.todo-item` 上挂着 `class:is-expanded`，全仓库没有一条规则用它 ——
    # 类名在、样式不在，正是这条检查该抓的"死钩子"，只是当时没扫到。
    $tokens = New-Object System.Collections.Generic.List[string]
    foreach ($m in [regex]::Matches($body, 'class="([^"{}]*)"')) {
        foreach ($token in $m.Groups[1].Value.Split(' ', [System.StringSplitOptions]::RemoveEmptyEntries)) {
            $tokens.Add($token)
        }
    }
    foreach ($m in [regex]::Matches($body, 'class:([A-Za-z][A-Za-z0-9_-]*)\s*=')) {
        $tokens.Add($m.Groups[1].Value)
    }
    foreach ($token in $tokens) {
        if ($defined.Contains($token) -or $classAllow.ContainsKey($token)) { continue }
        if (-not $undefined.Contains($token)) {
            $undefined[$token] = "$($file.Name) 等"
        }
    }
}
if ($undefined.Count -eq 0) { Pass "markup 里的类名都在 CSS 里有规则（例外 $($classAllow.Count) 条）" }
else {
    foreach ($entry in $undefined.GetEnumerator()) {
        Write-Host "      $($entry.Key)  ($($entry.Value))" -ForegroundColor DarkGray
    }
    Fail "有 $($undefined.Count) 个类名在 CSS 里没有规则 —— 要么补规则、要么删掉这个类名；刻意留的钩子加进本脚本的 `$classAllow"
}

# ---------------------------------------------------------------- 7. 文档引用
Write-Host '[static-guards] 7/9 文档引用（路径 / 版本号 / 小节）' -ForegroundColor Cyan
# 只查**还在维护的**文档。不查的两种：`docs/adr/` 记录的是当时的决策
# （里面提到后来被删掉的文件正是它该有的样子）；`docs/agents/` 与 `fixtures/heic/README.md`
# 描述的是别处的口径（skill 的通用约定、上游仓库的目录结构）。
# 发布说明那两份也算维护中的文档：模板里写的路径烂了，下次发布就照着一份错的说明抄。
$docPaths = @(
    'AGENTS.md', 'README.md', 'DESIGN.md', 'PRODUCT.md', 'fixtures/README.md',
    'build/release_template.md', 'build/release_notes.md'
) | Where-Object { Test-Path (Join-Path $repo $_) }

# 工作树里的真实文件与目录（排除生成物与被忽略的目录），用来判"文档引用的路径是否存在"。
# 按**顶层目录**剪枝再递归：`target/` 底下十几万个文件，整树扫一遍要好几秒。
$skipDirs = @('target', '.git', '.dsh', 'node_modules', 'dist', '.trunk')
$files = New-Object System.Collections.Generic.HashSet[string]
$dirs = New-Object System.Collections.Generic.HashSet[string]
foreach ($top in Get-ChildItem $repo -Force -ErrorAction SilentlyContinue) {
    if ($skipDirs -contains $top.Name) { continue }
    if (-not $top.PSIsContainer) { [void]$files.Add($top.Name); continue }
    [void]$dirs.Add($top.Name)
    foreach ($child in Get-ChildItem $top.FullName -Recurse -Force -ErrorAction SilentlyContinue) {
        $rel = $child.FullName.Substring($repo.Length + 1).Replace('\', '/')
        if ($skipDirs | Where-Object { $rel -eq $_ -or $rel.StartsWith("$_/") }) { continue }
        if ($child.PSIsContainer) { [void]$dirs.Add($rel) } else { [void]$files.Add($rel) }
    }
}

$badRefs = New-Object System.Collections.Generic.List[string]
foreach ($doc in $docPaths) {
    $text = Read-Text $doc
    # 先把**围栏代码块**整段去掉：一是代码块里的反引号会让"成对配对"错位，
    # 二是块里的路径多是示例（`target\tests\dev-ws` 之类）。
    $prose = [regex]::Replace($text, '(?s)```.*?```', '')
    # 8a. 反引号里的仓库内路径必须存在（允许省略前缀，`tr-domain/src/fee.rs` 也算对得上）
    foreach ($m in [regex]::Matches($prose, '`([^`]+)`')) {
        $candidate = $m.Groups[1].Value.Trim().TrimEnd('/')
        if ($candidate.Length -lt 3) { continue }
        if ($candidate -match '[<>{}*?$|~]' -or $candidate -match '://' -or $candidate -match '\s') { continue }
        if ($candidate -notmatch '[/\\]') { continue }              # 单文件名（LICENSE 之类）不查
        # 只认"看起来是文件"的：末段带扩展名。`Move/ButtonDown/ButtonUp`、`try/catch` 这类
        # 方法名清单与 `build/` 这类泛指都靠这条挡掉。
        if ($candidate -notmatch '\.[A-Za-z0-9]{1,6}$') { continue }
        $candidate = $candidate.TrimStart('./').Replace('\', '/')
        if ($candidate -match '^(target|dist|build/target)/' -or $candidate -match '/dist/') { continue }
        if ($files.Contains($candidate) -or $dirs.Contains($candidate)) { continue }
        if (@($files | Where-Object { $_.EndsWith("/$candidate") }).Count -gt 0) { continue }
        $badRefs.Add(('{0}: 引用了不存在的路径 —— {1}' -f $doc, $candidate))
    }
    # 8b. 发布资产名不许写死版本号（版本号只有 tauri.conf.json 一处）
    if ($text -match 'Transactions-x64-v\d') { $badRefs.Add(('{0}: 写死了资产名的版本号（写 <版本>；版本号只有 tauri.conf.json 一处）' -f $doc)) }
    # 8c. "AGENTS.md 的「小节」" 这类引用要对得上真实标题
    foreach ($m in [regex]::Matches($prose, '`?([A-Za-z0-9_./-]+\.md)`?\s*的\s*「([^」]{3,})」')) {
        $target = $m.Groups[1].Value
        $section = $m.Groups[2].Value
        if (-not (Test-Path (Join-Path $repo $target))) { continue }
        $headings = (Get-Content (Join-Path $repo $target) | Where-Object { $_ -match '^#{1,4}\s' }) -join "`n"
        if ($headings -notmatch [regex]::Escape($section)) { $badRefs.Add(('{0}: 引用了 {1} 的「{2}」，但那个文件里没有这样的小节' -f $doc, $target, $section)) }
    }
}
if ($badRefs.Count -eq 0) { Pass "文档引用干净（扫了 $($docPaths.Count) 份：路径 / 版本号 / 小节）" }
else {
    foreach ($hit in $badRefs) { Write-Host "      $hit" -ForegroundColor DarkGray }
    Fail "文档里有 $($badRefs.Count) 处过期引用 —— 改了脚本/小节/版本号，记得回头改文档"
}

# ---------------------------------------------------------------- 8. 发布说明
Write-Host '[static-guards] 8/9 发布说明（build/release_notes.md）' -ForegroundColor Cyan
# 这份说明就是 Release 正文（`build/release.ps1` 取它），也就是应用内「更新说明」显示的文字。
# 骨架是 `build/release_template.md`；逐条判据见本文件头注释第 8 条。
$notesRel = 'build/release_notes.md'
$notesPath = Join-Path $repo $notesRel
if (-not (Test-Path $notesPath)) {
    Fail "找不到 $notesRel —— 发布说明的当次内容写在这儿，骨架见 build/release_template.md"
}
else {
    $sectionNames = @('新增', '修改', '调整', '优化')
    $items = [ordered]@{}
    $order = New-Object System.Collections.Generic.List[string]
    $current = $null
    $notesVersion = $null
    $badNotes = New-Object System.Collections.Generic.List[string]

    foreach ($line in (Get-Content $notesPath)) {
        if ($line -match '^##\s*\[([0-9]+\.[0-9]+\.[0-9]+)\]\s*-\s*[0-9]{4}-[0-9]{2}-[0-9]{2}\s*$') {
            if ($notesVersion) { $badNotes.Add('版本标题出现了两次') }
            $notesVersion = $Matches[1]
            continue
        }
        if ($line -match '^###\s*(.+?)\s*$') {
            $name = $Matches[1].Trim()
            # 节名前后的 `###` 与空白已剥掉；模板里的占位标题（如「新增」带说明）不该出现
            if ($sectionNames -notcontains $name) {
                $badNotes.Add("节名「$name」不在四节里（新增 / 修改 / 调整 / 优化）")
                $current = $null
            }
            elseif ($items.Contains($name)) {
                $badNotes.Add("「$name」这一节出现了两次")
                $current = $name
            }
            else {
                $items[$name] = New-Object System.Collections.Generic.List[string]
                $order.Add($name)
                $current = $name
            }
            continue
        }
        $entry = $null
        if ($line -match '^\s*[-*+]\s*(.*)$') { $entry = $Matches[1].Trim() }
        elseif ($line.Trim()) {
            if ($current) { $badNotes.Add("「$current」里有不是列表项的内容：$($line.Trim())") }
            else { $badNotes.Add("有内容既不在版本标题下、也不在任何节里：$($line.Trim())") }
            continue
        }
        if ($null -eq $entry) { continue }                      # 空行
        if (-not $current) { $badNotes.Add("列表项出现在节标题之前：$entry"); continue }
        if (-not $entry) { continue }                           # 骨架里的空占位 `- `
        if ($line -match '^\s*[*+]\s') { $badNotes.Add("「$current」里用了 $($line.Trim()[0]) 做列表符号，统一用 -") }
        $items[$current].Add($entry)
    }

    # 9a. 版本标题存在且等于 tauri.conf.json（防止拿上一次的说明发新版）
    $confVersion = [regex]::Match((Read-Text 'src-tauri/tauri.conf.json'), '"version"\s*:\s*"([^"]+)"').Groups[1].Value
    if (-not $notesVersion) {
        $badNotes.Add("缺版本标题（第一行要写 ``## [$confVersion] - YYYY-MM-DD``）")
    }
    elseif ($notesVersion -ne $confVersion) {
        $badNotes.Add("版本标题是 $notesVersion，而 tauri.conf.json 是 $confVersion —— 说明还是上一版的")
    }

    # 9b. 出现的节必须按固定顺序、且一个都不许是空的
    $idx = -1
    foreach ($name in $order) {
        $now = [array]::IndexOf($sectionNames, $name)
        if ($now -le $idx) { $badNotes.Add("节的顺序不对：$($order -join ' / ')（固定顺序是 $($sectionNames -join ' / ')）") }
        $idx = $now
        if ($items[$name].Count -eq 0) { $badNotes.Add("「$name」是空的 —— 没有变更就整节删掉，别留标题") }
    }
    if ($order.Count -eq 0) { $badNotes.Add('一节都没有（四节里至少要有一节）') }

    # 9c. 每项 ≤50 字，且同一项不许跨节重复
    $seen = @{}
    foreach ($name in $order) {
        foreach ($item in $items[$name]) {
            if ($item.Length -gt 50) {
                $badNotes.Add("「$name」有一项 $($item.Length) 字（上限 50）：$item")
            }
            $key = ($item -replace '[`*。，、；：\s]', '')
            if ($seen.ContainsKey($key)) { $badNotes.Add("同一项出现在两节里（$($seen[$key]) / $name）：$item") }
            else { $seen[$key] = $name }
        }
    }

    if ($badNotes.Count -eq 0) {
        $total = 0; foreach ($name in $order) { $total += $items[$name].Count }
        Pass "v$notesVersion 合规：$($order -join ' / ')（共 $total 项，最长 $(($items.Values | ForEach-Object { $_.ToArray() } | ForEach-Object { $_ } | ForEach-Object { $_.Length } | Measure-Object -Maximum).Maximum) 字）"
    }
    else {
        foreach ($hit in $badNotes) { Write-Host "      $hit" -ForegroundColor DarkGray }
        Fail "发布说明有 $($badNotes.Count) 处不合模板 —— 照 build/release_template.md 改 $notesRel"
    }
}

# ---------------------------------------------------------------- 9. 版本号一致
Write-Host '[static-guards] 9/9 版本号（tauri.conf.json / 两处 Cargo.toml）' -ForegroundColor Cyan
# AGENTS.md 写着"版本号唯一来源是 `src-tauri/tauri.conf.json`，`Cargo.toml` 的 workspace 与
# src-tauri 两处也要同步"—— 但从前没有任何东西在查它，而它**看不出来**：
#   * 关于页显示的版本来自 `app.package_info()`，也就是 tauri.conf.json，Cargo.toml 漂了照报正确；
#   * 安装包自报的 ProductVersion 是 tauri 按 tauri.conf.json 写进 NSIS 的。
# 六个 crate 与 xtask 都写 `version.workspace = true`，所以要动的字面量只有两处。
#
# ⚠ 这里**故意不查 `Cargo.lock`**：本脚本第 5 项要跑 `cargo tree`，而 cargo 会顺手把锁文件刷成
# 新的版本号 —— 写一条"锁跟上没"的断言，它永远只能通过（实测：把两处 Cargo.toml 改掉、锁不动，
# 跑完第 5 项锁里就变成新版本了）。**不会失败的断言比没有断言更糟**，它给的是假保证。
# 锁文件归 cargo 管；"别把刷过的锁落下"靠提交前 `git status --short` 看一眼。
$versionSources = [ordered]@{
    'src-tauri/tauri.conf.json'      = [regex]::Match((Read-Text 'src-tauri/tauri.conf.json'), '"version"\s*:\s*"([^"]+)"').Groups[1].Value
    'Cargo.toml [workspace.package]' = [regex]::Match((Read-Text 'Cargo.toml'), '(?ms)^\[workspace\.package\]\s*$.*?^version\s*=\s*"([^"]+)"').Groups[1].Value
    'src-tauri/Cargo.toml [package]' = [regex]::Match((Read-Text 'src-tauri/Cargo.toml'), '(?ms)^\[package\]\s*$.*?^version\s*=\s*"([^"]+)"').Groups[1].Value
}
$unreadable = @($versionSources.GetEnumerator() | Where-Object { -not $_.Value })
if ($unreadable.Count -gt 0) {
    foreach ($u in $unreadable) { Fail "$($u.Key)：读不到 version（判据变了？）" }
}
else {
    $distinct = @($versionSources.Values | Select-Object -Unique)
    if ($distinct.Count -ne 1) {
        foreach ($e in $versionSources.GetEnumerator()) {
            Write-Host "      $($e.Key) = $($e.Value)" -ForegroundColor DarkGray
        }
        Fail "三处版本号不一致 —— 改版本号要三处一起改（`crates/*` 与 `xtask` 写的是 version.workspace = true，跟着 workspace 走）"
    }
    else {
        Pass "三处都是 $($distinct[0])（$($versionSources.Count) 处字面量；crates/* 与 xtask 跟着 workspace 走）"
    }
}

# ---------------------------------------------------------------- 汇总
if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "[static-guards] ❌ $($failures.Count) 项失败" -ForegroundColor Red
    exit 1
}
Write-Host ""
Write-Host '[static-guards] ✅ 跨文件静态护栏全部通过' -ForegroundColor Green
exit 0
