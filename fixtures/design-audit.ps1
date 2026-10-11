# design-audit.ps1 —— 设计令牌合规检查（DESIGN.md 是界面的裁决标准，这里把它机械化）。
#
# 检查项：
#   1. tokens.css 之外**不得**出现硬编码颜色（hex / rgb / rgba / hsl）——否则深色主题必然漏色；
#   2. 所有 `var(--transactions-*)` 引用都必须在 tokens.css 里定义过
#      （未定义会静默失效：颜色变成继承值或透明，界面上很难一眼看出）。
#      引用不只在 CSS 里：Rust 侧的内联 `style` 同样写令牌名（折线图还要把它解析成色值），
#      所以 `crates/tr-ui/src/**/*.rs` 一起收进来 —— 那里拼错一个令牌名同样是静默失效；
#   3. `[data-theme='dark']` 必须覆盖与 `prefers-color-scheme: dark` 兜底**完全相同**的令牌集合
#      （两套深色定义漂移会导致"跟随系统"与"手动深色"看起来不一致）；
#   4. 折线图的候选色池（DESIGN.md 的 Chart categorical palette）：
#      8 色齐全、Rust 里的顺序 == DESIGN.md 表格里的顺序、两套主题色值与表格一致，
#      且池内两两可辨（ΔE76：正常视觉 ≥25、红绿色盲下界 ≥8）、每条与画布的对比度 ≥3:1；
#   5. 未被引用的令牌只作提示（`tokens.css` 里有历史遗留令牌，允许保留）。
#
# 用法（pwsh 7）：
#   pwsh -File fixtures/design-audit.ps1
# 退出码：0 = 通过，1 = 有硬性违规。

param(
    # 只用于单独试 1~3 项（换一份 CSS 目录做实验）。第 4 项的规格与实现分别在仓库根的
    # `DESIGN.md` 与 `crates/tr-ui/src`，换了 CSS 目录那几项必然报"找不到"——这是故意的：
    # 宁可红着说清楚，也不要"换了个目录就静默跳过检查"。
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
# Rust 里也有令牌引用（内联 style 与图表的色值字符串），一并收进来做"引用必须有定义"
$uiSrc = Join-Path $repo 'crates\tr-ui\src'
$rustRefs = if (Test-Path $uiSrc) {
    (Get-ChildItem $uiSrc -Recurse -File -Filter *.rs |
        ForEach-Object { Get-Content $_.FullName -Raw }) -join "`n"
}
else { '' }
$failures = New-Object System.Collections.Generic.List[string]

# ---------- 1. 硬编码颜色 ----------
Write-Host '[design-audit] 1/4 硬编码颜色检查' -ForegroundColor Cyan
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
Write-Host '[design-audit] 2/4 令牌引用检查' -ForegroundColor Cyan
$defined = [regex]::Matches($all, '(--transactions-[a-z0-9-]+)\s*:') |
    ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique
$used = [regex]::Matches("$all`n$rustRefs", 'var\(\s*(--transactions-[a-z0-9-]+)') |
    ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique

$undefined = $used | Where-Object { $defined -notcontains $_ }
if ($undefined) {
    foreach ($token in $undefined) { $failures.Add("引用了未定义的令牌: $token") }
    Write-Host "  ✗ $($undefined.Count) 个引用没有定义" -ForegroundColor Red
    $undefined | ForEach-Object { "      $_" } | Write-Host
}
else {
    Write-Host "  ✓ $($used.Count) 个引用全部有定义（CSS + Rust；tokens.css 共 $($defined.Count) 个令牌）" -ForegroundColor Green
}

# ---------- 3. 深色主题与系统兜底一致性 ----------
Write-Host '[design-audit] 3/4 深色主题覆盖检查' -ForegroundColor Cyan

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

$darkBlock = Get-RuleBlock $all "(?m)^\s*\[data-theme='dark'\]|(?m)^\s*\[data-theme=`"dark`"\]"
$dark = Get-TokenNames $darkBlock
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

# ---------- 4. 图表候选色池 ----------
#
# 折线图的候选色池（DESIGN.md 的 Chart categorical palette）是**按位置取色**的：
# 第 N 条曲线 = 第 N 个颜色。于是"顺序"和"色值"分散在三处 —— DESIGN.md 的表格（规格）、
# tokens.css（取值）、chart.rs 的 `series_palette()`（实现）。这三处以前只能靠人眼对齐。
#
# 判据：
#   a. 表格 8 行、序号 1..8；两套主题的色值与 tokens.css 一致；DESIGN.md 头部副本（浅色）也一致；
#   b. Rust 里 `series_palette()` 的顺序 == 表格顺序（顺序变了等于全站图表换色）；
#   c. 池内两两可辨：CIELAB 的 ΔE76，正常视觉 ≥25、红绿色盲（红/绿色盲模拟后）≥8；
#   d. 每条与画布的对比度 ≥3:1（WCAG 1.4.11：2px 折线属于"图形对象"，不是纯装饰）。
#      画布是透明的，实际底色可能落在 `major-background` 或 `card-surface` 上，两个都算。
#
# 为什么是 ΔE76 而不是 ΔE00：这里只需要"别挑两个看着一样的颜色"，欧氏距离足够，
# 也不必在这个脚本里塞一份 CIEDE2000。色觉模拟用 Machado 2009（severity 1.0 的矩阵，
# 作用在**线性** RGB 上）。阈值取得比实测值松一档，留出"换个相近色"的余量。
Write-Host '[design-audit] 4/4 图表候选色池检查' -ForegroundColor Cyan

function Get-TokenValue([string]$block, [string]$token) {
    $m = [regex]::Match($block, [regex]::Escape($token) + '\s*:\s*(#[0-9a-fA-F]{6})')
    if ($m.Success) { $m.Groups[1].Value } else { '' }
}

function Convert-HexToRgb([string]$hex) {
    $h = $hex.TrimStart('#')
    # 每个元素都要自己一对括号：PowerShell 的逗号比除法结合得紧，
    # 写成 `a / 255.0, b / 255.0` 会解析成 `a / (255.0, b) / ...`（实测报 op_Division）
    @(
        ([Convert]::ToInt32($h.Substring(0, 2), 16) / 255.0),
        ([Convert]::ToInt32($h.Substring(2, 2), 16) / 255.0),
        ([Convert]::ToInt32($h.Substring(4, 2), 16) / 255.0)
    )
}

function Get-Linear([double]$channel) {
    if ($channel -le 0.04045) { $channel / 12.92 } else { [math]::Pow(($channel + 0.055) / 1.055, 2.4) }
}

function Get-Srgb([double]$channel) {
    $c = [math]::Max(0.0, [math]::Min(1.0, $channel))
    if ($c -le 0.0031308) { $c * 12.92 } else { 1.055 * [math]::Pow($c, 1 / 2.4) - 0.055 }
}

function Get-Luminance($rgb) {
    0.2126 * (Get-Linear $rgb[0]) + 0.7152 * (Get-Linear $rgb[1]) + 0.0722 * (Get-Linear $rgb[2])
}

function Get-Contrast($a, $b) {
    $la = Get-Luminance $a
    $lb = Get-Luminance $b
    ([math]::Max($la, $lb) + 0.05) / ([math]::Min($la, $lb) + 0.05)
}

function Get-LabChannel([double]$t) {
    if ($t -gt 216 / 24389) { [math]::Pow($t, 1 / 3) } else { (841 / 108) * $t + 4 / 29 }
}

# sRGB → CIELAB（D65）
function Get-Lab($rgb) {
    $r = Get-Linear $rgb[0]
    $g = Get-Linear $rgb[1]
    $b = Get-Linear $rgb[2]
    $x = (0.4124564 * $r + 0.3575761 * $g + 0.1804375 * $b) / 0.95047
    $y = 0.2126729 * $r + 0.7151522 * $g + 0.0721750 * $b
    $z = (0.0193339 * $r + 0.1191920 * $g + 0.9503041 * $b) / 1.08883
    $fx = Get-LabChannel $x
    $fy = Get-LabChannel $y
    $fz = Get-LabChannel $z
    @(
        (116 * $fy - 16),
        (500 * ($fx - $fy)),
        (200 * ($fy - $fz))
    )
}

function Get-DeltaE76($labA, $labB) {
    [math]::Sqrt(
        [math]::Pow($labA[0] - $labB[0], 2) +
        [math]::Pow($labA[1] - $labB[1], 2) +
        [math]::Pow($labA[2] - $labB[2], 2))
}

# 红/绿色盲模拟（Machado 2009，severity 1.0），矩阵作用在线性 RGB 上
$cvdMatrices = @{
    protan = @(0.152286, 1.052583, -0.204868, 0.114503, 0.786281, 0.099216, -0.003882, -0.048116, 1.051998)
    deutan = @(0.367322, 0.860646, -0.227968, 0.280085, 0.672501, 0.047413, -0.011820, 0.042940, 0.968881)
}

function Get-Cvd([string]$kind, $rgb) {
    $m = $cvdMatrices[$kind]
    $r = Get-Linear $rgb[0]
    $g = Get-Linear $rgb[1]
    $b = Get-Linear $rgb[2]
    @(
        (Get-Srgb ($m[0] * $r + $m[1] * $g + $m[2] * $b)),
        (Get-Srgb ($m[3] * $r + $m[4] * $g + $m[5] * $b)),
        (Get-Srgb ($m[6] * $r + $m[7] * $g + $m[8] * $b))
    )
}

$lightBlock = Get-RuleBlock $all '(?m)^:root\s*\{'
$designText = Get-Content (Join-Path $repo 'DESIGN.md') -Raw
# 表格是规格：序号 | 令牌 | 浅色 | 深色 | 色相
$rows = [regex]::Matches($designText,
    '(?m)^\|\s*(\d+)\s*\|\s*`(--transactions-color-chart-[a-z]+)`\s*\|\s*`(#[0-9a-fA-F]{6})`\s*\|\s*`(#[0-9a-fA-F]{6})`\s*\|')
# 卡片头的 colors: 副本只写浅色
$front = [regex]::Matches($designText, '(?m)^\s{2}chart-([a-z]+):\s*"(#[0-9a-fA-F]{6})"')

$paletteFailures = New-Object System.Collections.Generic.List[string]
if ($rows.Count -ne 8) {
    $paletteFailures.Add("DESIGN.md 的 Chart categorical palette 表格应为 8 行，实际 $($rows.Count) 行")
}
else {
    $expected = 1
    foreach ($row in $rows) {
        if ([int]$row.Groups[1].Value -ne $expected) {
            $paletteFailures.Add("DESIGN.md 表格序号不连续：第 $expected 行是 $($row.Groups[1].Value)")
        }
        $expected++
    }
}
if ($front.Count -ne 8) { $paletteFailures.Add("DESIGN.md 头部的 chart-* 颜色是 $($front.Count) 条，应为 8 条") }

$palette = @()
foreach ($row in $rows) {
    $token = $row.Groups[2].Value
    $light = $row.Groups[3].Value
    $dark = $row.Groups[4].Value
    $cssLight = Get-TokenValue $lightBlock $token
    $cssDark = Get-TokenValue $darkBlock $token
    if (-not $cssLight) { $paletteFailures.Add("tokens.css 的浅色里没有 $token") }
    elseif ($cssLight -ne $light) { $paletteFailures.Add("$token 浅色：表格 $light，tokens.css $cssLight") }
    if (-not $cssDark) { $paletteFailures.Add("tokens.css 的深色里没有 $token") }
    elseif ($cssDark -ne $dark) { $paletteFailures.Add("$token 深色：表格 $dark，tokens.css $cssDark") }
    $palette += @{ Token = $token; Light = $light; Dark = $dark }
}

# 头部副本（浅色）必须与表格一致
foreach ($item in $front) {
    $token = "--transactions-color-chart-$($item.Groups[1].Value)"
    $row = $palette | Where-Object { $_.Token -eq $token }
    if (-not $row) { $paletteFailures.Add("DESIGN.md 头部有 $token，表格里没有") }
    elseif ($row.Light -ne $item.Groups[2].Value) {
        $paletteFailures.Add("DESIGN.md 头部 $token 是 $($item.Groups[2].Value)，表格是 $($row.Light)")
    }
}

# Rust 的顺序 = 表格顺序
$chartRsPath = Join-Path $uiSrc 'components\ui\chart.rs'
$rustOrder = if (Test-Path $chartRsPath) {
    $body = [regex]::Match((Get-Content $chartRsPath -Raw),
        '(?s)pub fn series_palette\(\)[^{]*\{(?<body>[^}]*)\}').Groups['body'].Value
    @([regex]::Matches($body, 'var\((--transactions-color-chart-[a-z]+)\)') |
        ForEach-Object { $_.Groups[1].Value })
}
else { @() }
if ($rustOrder.Count -ne $palette.Count) {
    $paletteFailures.Add("chart.rs 的 series_palette() 有 $($rustOrder.Count) 个颜色，DESIGN.md 表格有 $($palette.Count) 个")
}
else {
    for ($i = 0; $i -lt $palette.Count; $i++) {
        if ($rustOrder[$i] -ne $palette[$i].Token) {
            $paletteFailures.Add("第 $($i + 1) 条：chart.rs 是 $($rustOrder[$i])，DESIGN.md 表格是 $($palette[$i].Token)")
        }
    }
}

# 两两可辨 + 与画布的对比度
$minNormal, $minCvd, $minContrast = 25.0, 8.0, 3.0
$measured = @()
foreach ($theme in @(
        @{ Name = '浅色'; Key = 'Light'; Block = $lightBlock },
        @{ Name = '深色'; Key = 'Dark'; Block = $darkBlock })) {
    $values = @($palette | ForEach-Object { $_[$theme.Key] })
    $unique = @($values | Sort-Object -Unique).Count
    if ($unique -ne $values.Count) {
        $paletteFailures.Add("$($theme.Name)色池里有重复色值（$($values.Count) 条里只有 $unique 个不同）")
    }

    $worstContrast = [double]::MaxValue
    $worstContrastToken = ''
    foreach ($surface in @('major-background', 'card-surface')) {
        $surfaceValue = Get-TokenValue $theme.Block "--transactions-color-$surface"
        if (-not $surfaceValue) {
            $paletteFailures.Add("tokens.css 的$($theme.Name)里没有 --transactions-color-$surface")
            continue
        }
        $rgb = Convert-HexToRgb $surfaceValue
        for ($i = 0; $i -lt $values.Count; $i++) {
            $ratio = Get-Contrast (Convert-HexToRgb $values[$i]) $rgb
            if ($ratio -lt $worstContrast) {
                $worstContrast = $ratio
                $worstContrastToken = "$($palette[$i].Token) on $surface"
            }
        }
    }

    $normalWorst, $cvdWorst = [double]::MaxValue, [double]::MaxValue
    $normalPair, $cvdPair = '', ''
    for ($i = 0; $i -lt $values.Count; $i++) {
        for ($j = $i + 1; $j -lt $values.Count; $j++) {
            $a = Convert-HexToRgb $values[$i]
            $b = Convert-HexToRgb $values[$j]
            $de = Get-DeltaE76 (Get-Lab $a) (Get-Lab $b)
            if ($de -lt $normalWorst) {
                $normalWorst = $de
                $normalPair = "$($palette[$i].Token) / $($palette[$j].Token)"
            }
            foreach ($kind in @('protan', 'deutan')) {
                $simA = Get-Cvd $kind $a
                $simB = Get-Cvd $kind $b
                $deCvd = Get-DeltaE76 (Get-Lab $simA) (Get-Lab $simB)
                if ($deCvd -lt $cvdWorst) {
                    $cvdWorst = $deCvd
                    $cvdPair = "$($palette[$i].Token) / $($palette[$j].Token)（$kind）"
                }
            }
        }
    }

    if ($worstContrast -lt $minContrast) {
        $paletteFailures.Add("$($theme.Name)：$worstContrastToken 对比度只有 $([math]::Round($worstContrast, 2)):1（要求 ≥${minContrast}:1）")
    }
    if ($normalWorst -lt $minNormal) {
        $paletteFailures.Add("$($theme.Name)：$normalPair 的 ΔE76 只有 $([math]::Round($normalWorst, 1))（要求 ≥$minNormal）")
    }
    if ($cvdWorst -lt $minCvd) {
        $paletteFailures.Add("$($theme.Name)：$cvdPair 在红/绿色盲下 ΔE76 只有 $([math]::Round($cvdWorst, 1))（要求 ≥$minCvd）")
    }
    $measured += "$($theme.Name) 对比度 ≥$([math]::Round($worstContrast, 2)):1、两两 ΔE76 ≥$([math]::Round($normalWorst, 1))、红绿色盲 ≥$([math]::Round($cvdWorst, 1))"
}

foreach ($item in $paletteFailures) { $failures.Add($item) }
if ($paletteFailures.Count -eq 0) {
    Write-Host "  ✓ $($palette.Count) 色齐全；Rust 顺序与 DESIGN.md 表格一致" -ForegroundColor Green
    $measured | ForEach-Object { Write-Host "  ✓ $_" -ForegroundColor Green }
}
else {
    Write-Host "  ✗ $($paletteFailures.Count) 处不符合候选色池的判据" -ForegroundColor Red
    $paletteFailures | ForEach-Object { "      $_" } | Write-Host
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
