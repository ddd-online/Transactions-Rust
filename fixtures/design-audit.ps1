# design-audit.ps1 —— 设计令牌合规检查（DESIGN.md 是界面的裁决标准，这里把它机械化）。
#
# 检查项：
#   1. tokens.css 之外**不得**出现硬编码颜色（hex / rgb / rgba / hsl）——否则深色主题必然漏色；
#   2. 所有 `var(--transactions-*)` 引用都必须在 tokens.css 里定义过
#      （未定义会静默失效：颜色变成继承值或透明，界面上很难一眼看出）；
#   3. `[data-theme='dark']` 必须覆盖与 `prefers-color-scheme: dark` 兜底**完全相同**的令牌集合
#      （两套深色定义漂移会导致"跟随系统"与"手动深色"看起来不一致）；
#   4. 未被引用的令牌只作提示（`tokens.css` 里有历史遗留令牌，允许保留）。
#
# 用法（pwsh 7）：
#   pwsh -File fixtures/design-audit.ps1
# 退出码：0 = 通过，1 = 有硬性违规。

param(
    [string]$CssDir
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
if (-not $CssDir) { $CssDir = Join-Path $repo 'crates\tr-ui\static\css' }
if (-not (Test-Path $CssDir)) { throw "找不到 CSS 目录: $CssDir" }

$files = Get-ChildItem $CssDir -File -Filter *.css
if ($files.Count -eq 0) { throw "$CssDir 下没有 .css 文件" }

function Get-TokenNames([string]$block) {
    [regex]::Matches($block, '(--transactions-[a-z0-9-]+)\s*:') |
        ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique
}

$all = ($files | ForEach-Object { Get-Content $_.FullName -Raw }) -join "`n"
$failures = New-Object System.Collections.Generic.List[string]

# ---------- 1. 硬编码颜色 ----------
Write-Host '[design-audit] 1/3 硬编码颜色检查' -ForegroundColor Cyan
foreach ($file in $files) {
    if ($file.Name -eq 'tokens.css') { continue }   # 令牌文件本身就是颜色定义处
    $text = Get-Content $file.FullName -Raw
    $hits = @()
    $hits += [regex]::Matches($text, '#[0-9a-fA-F]{3,8}\b') | ForEach-Object { $_.Value }
    $hits += [regex]::Matches($text, '\bhsla?\([^)]*\)') | ForEach-Object { $_.Value }
    $hits += [regex]::Matches($text, '\brgba?\([^)]*\)') | ForEach-Object { $_.Value }
    if ($hits.Count -gt 0) {
        $sample = ($hits | Select-Object -First 6) -join ', '
        $failures.Add("$($file.Name) 里有 $($hits.Count) 处硬编码颜色：$sample")
        Write-Host "  ✗ $($file.Name)：$($hits.Count) 处" -ForegroundColor Red
    }
}
if ($failures.Count -eq 0) { Write-Host '  ✓ 除 tokens.css 外无硬编码颜色' -ForegroundColor Green }

# ---------- 2. 令牌引用与定义 ----------
Write-Host '[design-audit] 2/3 令牌引用检查' -ForegroundColor Cyan
$defined = [regex]::Matches($all, '(--transactions-[a-z0-9-]+)\s*:') |
    ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique
$used = [regex]::Matches($all, 'var\(\s*(--transactions-[a-z0-9-]+)') |
    ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique

$undefined = $used | Where-Object { $defined -notcontains $_ }
if ($undefined) {
    foreach ($token in $undefined) { $failures.Add("引用了未定义的令牌: $token") }
    Write-Host "  ✗ $($undefined.Count) 个引用没有定义" -ForegroundColor Red
    $undefined | ForEach-Object { "      $_" } | Write-Host
}
else {
    Write-Host "  ✓ $($used.Count) 个引用全部有定义（tokens.css 共 $($defined.Count) 个令牌）" -ForegroundColor Green
}

# ---------- 3. 深色主题与系统兜底一致性 ----------
Write-Host '[design-audit] 3/3 深色主题覆盖检查' -ForegroundColor Cyan

# 取 selector 后的第一个平衡大括号块（用花括号计数，避免被块内注释/嵌套误导）
function Get-RuleBlock([string]$text, [string]$pattern) {
    $match = [regex]::Match($text, $pattern)
    if (-not $match.Success) { return '' }
    $start = $text.IndexOf('{', $match.Index)
    if ($start -lt 0) { return '' }
    $depth = 0
    for ($i = $start; $i -lt $text.Length; $i++) {
        if ($text[$i] -eq '{') { $depth++ }
        elseif ($text[$i] -eq '}') {
            $depth--
            if ($depth -eq 0) { return $text.Substring($start + 1, $i - $start - 1) }
        }
    }
    return ''
}

$dark = Get-TokenNames (Get-RuleBlock $all "(?m)^\s*\[data-theme='dark'\]|(?m)^\s*\[data-theme=`"dark`"\]")
# 兜底写法：@media (prefers-color-scheme: dark) { html:not([data-theme]) { ... } }
# 两个 pattern 都必须**锚定到行首的选择器/at-rule**：文件头注释里同样出现过
# "prefers-color-scheme: dark" 与 "[data-theme]"，不锚定会抽到注释里的花括号。
$mediaInner = Get-RuleBlock $all '(?m)^\s*@media\s*\(\s*prefers-color-scheme:\s*dark\s*\)'
$fallback = Get-TokenNames (Get-RuleBlock $mediaInner '(?m)^\s*html:not\(\[data-theme\]\)')

Write-Host "  [data-theme='dark'] 覆盖 $($dark.Count) 个令牌；系统兜底覆盖 $($fallback.Count) 个"
if ($dark.Count -eq 0) {
    $failures.Add("没有找到 [data-theme='dark'] 令牌覆盖块")
    Write-Host '  ✗ 深色主题未定义' -ForegroundColor Red
}
elseif ($fallback.Count -eq 0) {
    $failures.Add("没有找到 @media (prefers-color-scheme: dark) 里的 html:not([data-theme]) 覆盖块")
    Write-Host '  ✗ 系统兜底深色未定义（或选择器写法变了，需同步本脚本）' -ForegroundColor Red
}
else {
    $diff = Compare-Object $dark $fallback
    if ($diff) {
        $failures.Add("深色主题与系统兜底的令牌集合不一致（$($diff.Count) 处）")
        Write-Host '  ✗ 两套深色定义漂移：' -ForegroundColor Red
        $diff | ForEach-Object { "      $($_.SideIndicator) $($_.InputObject)" } | Write-Host
    }
    else {
        Write-Host '  ✓ 深色主题与系统兜底覆盖同一组令牌' -ForegroundColor Green
    }
}

# ---------- 提示：未被引用的令牌 ----------
$unused = $defined | Where-Object { $used -notcontains $_ }
if ($unused) {
    Write-Host "[design-audit] 提示：$($unused.Count) 个令牌定义了但未被引用（tokens.css 里的历史遗留，允许保留）" -ForegroundColor Yellow
}

if ($failures.Count -gt 0) {
    Write-Host ''
    Write-Host "[design-audit] ❌ $($failures.Count) 项不通过：" -ForegroundColor Red
    $failures | ForEach-Object { "  - $_" }
    exit 1
}

Write-Host ''
Write-Host '[design-audit] ✅ 设计令牌检查通过' -ForegroundColor Green
exit 0
