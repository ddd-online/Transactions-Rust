//! tr-draw —— 界面侧的纯算法层（native 与 wasm32 双可编、零 I/O）。
//!
//! 装的是"界面里想被 native 断言的东西"：没有一行 I/O，也不认识 DOM。
//!
//! * [`chart`]：折线图的坐标与刻度（Y 轴范围、刻度的"大整数"、值 → 像素）、
//!   charts-rs 输出 SVG 的定点改写（按 0 轴分色、面积填充基线）、类目抽稀、
//!   提示框定位，以及从测量点拼出的 [`chart::ChartGeometry`]。
//! * [`crop`]：方形裁剪的几何（cover 比例、位移夹紧、裁剪框反解）。
//! * [`query`]：页面取数的决策核心（去重键与 generation 的推进、要不要发这次请求、
//!   缓存复核的判定、失效规则）—— 绘制只是本 crate 的第一批住户，见
//!   `docs/adr/0001-pure-draw-crate.md` 的补充说明。
//! * [`heic`]：HEIC/HEIF（iPhone 照片）→ RGBA8 —— 界面侧解码，
//!   因为 WebView2/系统那条路根本走不通（见该模块的说明）。
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
pub mod heic;
pub mod query;
