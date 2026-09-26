# chart-tests.ps1 —— 真跑 chart.rs 里的单元测试（Y 轴范围 / 面积填充基线 / 0 轴分色）
#
# 用法（pwsh 7）：pwsh -File fixtures/chart-tests.ps1
#
# 为什么需要它：`tr-ui` 是 wasm-only crate（`lib.rs` 顶部 `#![cfg(target_arch = "wasm32")]`），
# native 上 `cargo test -p tr-ui --lib` **跑 0 个测试**、wasm 上又没有 runner ——
# 所以 `chart.rs` 里的 `#[cfg(test)]` 平时只是**给人看的规格说明**，改了算法它们不会拦住你。
#
# 做法：把被测函数的**原文**（连注释一起）抽出来，配几个桩类型（`ChartSeries` / `display_value`
# / MARGIN / Y_AXIS_MAX_SPLITS）拼成一个 rustc 直接能编的小程序，在 native 上执行。
# 抽查的是原文而不是复制品 —— 函数一改，这里跑的就是改后的代码；函数被改名/删掉则直接报错。
# 生成的代码落在 `target\tests\chart-tests\`（不入库），失败时以非零码退出：
# 断言写错或算法回归都会当场 panic（这套脚手架真抓到过一条我脑补错的期望值）。
#
# 新增被测函数时：把函数名加进下面的 $fns（常量加进 $consts），并在生成的 main 里加一行调用。
$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
$source = 'crates\tr-ui\src\components\ui\chart.rs'
if (-not (Test-Path $source)) { throw "找不到 $source（请在仓库根下运行）" }
$src = Get-Content $source -Raw

function Get-FnBody([string]$text, [string]$name) {
    $idx = $text.IndexOf("fn $name")
    if ($idx -lt 0) { throw "not found: fn $name" }
    $brace = $text.IndexOf('{', $idx)
    $depth = 0; $i = $brace
    while ($i -lt $text.Length) {
        if ($text[$i] -eq '{') { $depth++ } elseif ($text[$i] -eq '}') { $depth--; if ($depth -eq 0) { break } }
        $i++
    }
    return $text.Substring($idx, $i - $idx + 1)
}

function Get-ConstBody([string]$text, [string]$name) {
    $idx = $text.IndexOf("const $name")
    if ($idx -lt 0) { throw "not found: const $name" }
    $semi = $text.IndexOf(';', $idx)
    if ($semi -lt 0) { throw "not found: const $name" }
    return $text.Substring($idx, $semi - $idx + 1)
}

# 被测函数：纯函数 + 它们依赖的测试辅助函数 + 测试本身
$fns = @(
    'display_range', 'nice_steps', 'nice_axis_range', 'format_value',
    'path_points', 'fmt_coord', 'attr_value', 'replace_attr', 'rewrite_tags',
    'zero_axis_y', 'rewrite_fill_baseline', 'anchor_fill_at_zero',
    'split_at_zero', 'polyline_d', 'fill_d', 'split_fill', 'split_line', 'sign_dot', 'side_color',
    'fill_tag', 'paint_by_sign',
    'ticks', 'fill_path_d',
    'axis_ticks_are_round_numbers_for_positive_money',
    'axis_range_hugs_the_data_when_it_crosses_zero',
    'percent_bounds_hold_the_axis_within_one_hundred',
    'ratio_and_percent_keep_two_decimals',
    'axis_range_covers_the_data_and_keeps_a_strict_upper_bound',
    'fill_baseline_moves_to_the_zero_axis',
    'fill_baseline_is_untouched_when_zero_is_an_edge_of_the_axis',
    'fill_baseline_is_left_alone_without_a_trustworthy_axis',
    'zero_axis_geometry_agrees_with_the_data_points',
    'fill_and_line_split_at_the_zero_axis',
    'split_stays_untouched_without_a_trustworthy_zero_axis'
)
# 被测常量（`PROBE_SVG` 是 charts-rs 1.0.0 的真实输出，填充基线的定点断言就钉在它上面）
$consts = @('FILL_OPACITY_ATTR', 'SVG_PATH_OPEN', 'FILL_NONE_ATTR', 'AXIS_LIMIT_EPSILON', 'Y_SPLITS', 'PROBE_SVG')

$parts = @()
foreach ($n in $consts) { $parts += "// ---- 原文: const $n"; $parts += (Get-ConstBody $src $n); $parts += '' }
foreach ($n in $fns) { $parts += "// ---- 原文: $n"; $parts += (Get-FnBody $src $n); $parts += '' }

$header = @'
// 自动生成（fixtures/chart-tests.ps1），不入库。
#![allow(dead_code)]
#[derive(Clone, Copy)]
pub enum ChartValueKind { Money, Percent, Ratio, Count }
pub struct ChartSeries { pub data: Vec<f64> }
pub fn display_value(value: f64, kind: ChartValueKind) -> f32 {
    match kind {
        ChartValueKind::Money => (value / 100.0) as f32,
        ChartValueKind::Percent | ChartValueKind::Ratio | ChartValueKind::Count => value as f32,
    }
}
/// 与 chart.rs 同一个常量（那里是 `const MARGIN: f32 = 8.0;`）
const MARGIN: f32 = 8.0;
/// 与 chart.rs 同一个常量
const Y_AXIS_MAX_SPLITS: usize = 5;

'@

$main = @'

fn main() {
    // ⚠ 这里的条数要与 cases 的条数一致（下面打印用）
    const COUNT: usize = 11;
    let cases: Vec<(&str, fn())> = vec![
        ("Y 轴刻度是正整数（用户报的那张图）", axis_ticks_are_round_numbers_for_positive_money),
        ("跨零时范围各自贴住数据（不强制对称）", axis_range_hugs_the_data_when_it_crosses_zero),
        ("百分比上下界停在 0..100", percent_bounds_hold_the_axis_within_one_hundred),
        ("盈亏比保留两位小数", ratio_and_percent_keep_two_decimals),
        ("范围盖住数据且上界严格更大", axis_range_covers_the_data_and_keeps_a_strict_upper_bound),
        ("填充基线挪到 0 轴", fill_baseline_moves_to_the_zero_axis),
        ("0 在轴端点时填充基线不动", fill_baseline_is_untouched_when_zero_is_an_edge_of_the_axis),
        ("没有可信范围时不动填充", fill_baseline_is_left_alone_without_a_trustworthy_axis),
        ("0 轴几何与数据点一致", zero_axis_geometry_agrees_with_the_data_points),
        ("面积/折线/数据点按 0 轴分色", fill_and_line_split_at_the_zero_axis),
        ("没有可信 0 轴时不分色", split_stays_untouched_without_a_trustworthy_zero_axis),
    ];
    assert_eq!(cases.len(), COUNT, "cases 与 COUNT 不一致");
    for (name, case) in cases {
        case();
        println!("  ✓ {name}");
    }
    println!("[chart-tests] ✅ chart.rs 的 {COUNT} 条测试全部通过");
}
'@

$dir = Join-Path $repo 'target\tests\chart-tests'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$mainRs = Join-Path $dir 'main.rs'
($header + ($parts -join "`n") + $main) | Set-Content -Path $mainRs -Encoding utf8

rustc -O --edition 2021 -o (Join-Path $dir 'harness.exe') $mainRs 2>&1 | Select-Object -Last 25
if ($LASTEXITCODE -ne 0) { throw "[chart-tests] rustc 编译失败: $LASTEXITCODE（生成的代码在 $mainRs）" }
& (Join-Path $dir 'harness.exe')
if ($LASTEXITCODE -ne 0) { throw "[chart-tests] ✗ 有断言失败: $LASTEXITCODE" }
