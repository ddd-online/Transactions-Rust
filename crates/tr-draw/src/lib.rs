//! tr-draw —— 纯绘制算法层。
//!
//! 装的是"画之前的数学"：没有一行 I/O，也不认识 DOM 与图表引擎。
//!
//! * [`chart`]：折线图的坐标与刻度（Y 轴范围、刻度的"大整数"、值 → 像素）、
//!   charts-rs 输出 SVG 的定点改写（按 0 轴分色、面积填充基线）、类目抽稀、
//!   提示框定位，以及从测量点拼出的 [`chart::ChartGeometry`]。
//! * [`crop`]：方形裁剪的几何（cover 比例、位移夹紧、裁剪框反解）。
//!
//! 为什么单开一个 crate，见 `docs/adr/0001-pure-draw-crate.md`。
//!
//! 两条纪律：
//!
//! * 只写 native 与 wasm32 都能编的东西 —— **不许** `use leptos` / `web_sys` / `charts_rs`。
//! * `cargo test -p tr-draw` 是这些算法的唯一测试面：改算法先在这里红，
//!   界面侧（`tr-ui`）只负责把 DOM 测量值喂进来、把结果画出去。

pub mod chart;
pub mod crop;
