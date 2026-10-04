# crop-tests.ps1 —— 真跑 image_crop.rs 里的单元测试（铺满比例 / 位移夹紧 / 裁剪框）
#
# 用法（pwsh 7）：pwsh -File fixtures/crop-tests.ps1
#
# 为什么需要它：`tr-ui` 是 wasm-only crate（`lib.rs` 顶部 `#![cfg(target_arch = "wasm32")]`），
# native 上 `cargo test -p tr-ui --lib` **跑 0 个测试**、wasm 上又没有 runner ——
# 所以那里的 `#[cfg(test)]` 平时只是**给人看的规格说明**。
# 与 `chart-tests.ps1` 同一套路（那边的注释把理由写全了）：把被测函数的**原文**
# 抽出来，配几个桩类型拼成 rustc 直接能编的小程序，在 native 上执行。
#
# 这套几何值得单独立一条护栏：裁剪框算错的那种 bug 表面上"看得见"（图标是歪的），
# 但**很难一眼看出错多少** —— 只有把"拖到边界时取源图的哪一块"钉成断言才拦得住。
$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
$source = 'crates\tr-ui\src\components\ui\image_crop.rs'
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

# 被测函数：纯几何 + 它们的测试
$fns = @(
    'cover_scale', 'max_offset', 'crop_rect',
    'cover_scale_fills_the_shorter_side',
    'centered_crop_takes_the_middle_square',
    'offset_is_clamped_to_the_overflow',
    'zooming_in_allows_larger_offsets_and_stays_inside'
)
$consts = @('CROP_VIEWPORT')

$parts = @()
foreach ($n in $consts) { $parts += "// ---- 原文: const $n"; $parts += (Get-ConstBody $src $n); $parts += '' }
foreach ($n in $fns) { $parts += "// ---- 原文: $n"; $parts += (Get-FnBody $src $n); $parts += '' }

# 裁剪框的返回值类型（原文在 image_crop.rs 里，紧挨着这几个函数）
$header = @'
// 自动生成（fixtures/crop-tests.ps1），不入库。
#![allow(dead_code)]
pub struct CropRect { pub x: f64, pub y: f64, pub size: f64 }

'@

$main = @'

fn main() {
    const COUNT: usize = 4;
    let cases: Vec<(&str, fn())> = vec![
        ("铺满取短边（横图与竖图同比例）", cover_scale_fills_the_shorter_side),
        ("居中裁剪取正中间的方图", centered_crop_takes_the_middle_square),
        ("位移被夹在溢出量之内（图永远盖满视口）", offset_is_clamped_to_the_overflow),
        ("放大后能看四角，裁剪框仍在源图内", zooming_in_allows_larger_offsets_and_stays_inside),
    ];
    assert_eq!(cases.len(), COUNT, "cases 与 COUNT 不一致");
    for (name, case) in cases {
        case();
        println!("  ✓ {name}");
    }
    println!("[crop-tests] ✅ image_crop.rs 的 {COUNT} 条测试全部通过");
}
'@

$dir = Join-Path $repo 'target\tests\crop-tests'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$mainRs = Join-Path $dir 'main.rs'
($header + ($parts -join "`n") + $main) | Set-Content -Path $mainRs -Encoding utf8

rustc -O --edition 2021 -o (Join-Path $dir 'harness.exe') $mainRs 2>&1 | Select-Object -Last 25
if ($LASTEXITCODE -ne 0) { throw "[crop-tests] rustc 编译失败: $LASTEXITCODE（生成的代码在 $mainRs）" }
& (Join-Path $dir 'harness.exe')
if ($LASTEXITCODE -ne 0) { throw "[crop-tests] ✗ 有断言失败: $LASTEXITCODE" }
