# 纯绘制算法单开一个 crate（`tr-draw`）

界面里那些**纯算法**——Y 轴范围与刻度、charts-rs 输出 SVG 的定点改写（0 轴分色、面积填充基线）、
类目抽稀、提示框定位、几何拼装、方形裁剪几何——原本住在 `crates/tr-ui/src/components/ui/` 里。
但 `tr-ui` 只编 wasm32（`lib.rs` 顶部 `#![cfg(target_arch = "wasm32")]`），于是 native 上
`cargo test -p tr-ui --lib` 是**绿的 0 个测试**：那些 `#[cfg(test)]` 既不被执行、也不被类型检查，
唯一的真实测试面是两个 PowerShell 脚本（`fixtures/chart-tests.ps1` / `crop-tests.ps1`）——
它们按 `fn <名字>` 扫源码、拼一个 rustc 程序来跑。

**决定**：把这些纯算法搬进新 crate `tr-draw`（native + wasm 双可编、零 I/O 依赖），
测试面回到 `cargo test -p tr-draw`；两个 PowerShell 护栏删除。

搬家当时就露了馅，这些断言**第一次被真正执行**就抓出三类问题：2 条期望值陈旧（百分比不除 100、
提示框放不下要翻到下方）、1 处实现在打自己文档的脸（`ChartGeometry::y_of` 按契约收"分"、
内部却当"元"代入）、7 处类型错误（`vec![100, 300]` 这类整数当 `f64` —— 说明它们此前连类型检查
都没过），外加 3 处 clippy 违规（代码从未被 lint：`tr-ui` 是 wasm-only，而仓库的 clippy 只跑 host）。
界面侧只留渲染：charts-rs 出图、CSS 变量解析、DOM 测量。

## 切在哪条线上

**"纯"还不够，判据是：能在 native 上断言、且不是渲染配置本身。**

只按"不依赖 leptos / web-sys / charts-rs"来切，会连 `ChartConfig`、`POINT_RADIUS`、`ANIM_MS`
一起划进来 —— 它们确实是纯数据，但它们是**渲染器的输入**（画布高度、动画时长、点半径、图例开关、
参考线与上下界），没有任何一个被搬走的算法消费它们，搬过去只会把渲染旋钮摆进数学 crate。

反过来，只要一个函数/类型是为了在 native 上被断言而存在的，它就该在这儿 ——
哪怕名字听起来很界面（`place_tooltip`、`thin_labels`、`hit_index` 都是这么进来的）。

## 考虑过的其它选项

- **塞进 `tr-domain`**：零新 crate，且它已经在标准测试命令里。但 `tr-domain` 是**领域语言**层
  （models / dto / 金额换算 / 费用分摊，后端也用得到），图表像素几何不是领域语言，后端永远不需要它 ——
  放进去等于把"领域层"改写成"什么都装的纯算法层"。
- **让 `tr-ui` 自己变双目标**（把 crate 根那条 `#![cfg(...)]` 下沉到各模块声明）：改动面最小。
  但它会把 `tr-ui` 写进文档的那条身份（"本 crate 是纯界面，只编 wasm32"）悄悄改掉 ——
  `Cargo.toml` 里那套 `cfg(wasm32)` 依赖分组、`AGENTS.md` 的说明、以及"谁往纯模块里 `use leptos`
  一下就让 native 测试编不过"这个新陷阱，都要跟着承担。

代价是多一个 workspace 成员（AGENTS.md 的架构块与常用命令各加一行）。
**再遇到"界面里想被 native 断言的纯算法"，直接放 `tr-draw`**，别退回 wasm-only 的 `tr-ui`。

## 后果

- `tr-ui` 的 `#[cfg(test)]` 现在只剩渲染侧（charts-rs 出图、颜色解析、图例与 tooltip 的 CSS 契约），
  依旧是"给人看的规格说明"；渲染回归靠 `fixtures/ui-shots.ps1` / `ui-smoke.ps1` 那几条真启动的护栏。
- `tr-draw` 不许出现 `leptos` / `web-sys` / `charts-rs` —— 一旦引入，它就编不到 native，ADR 的前提失效。
- 本次搬家后 `fixtures/test.ps1` 的 `chart` / `analysis` / `ui-kit` 三个分组改跑 `test-draw` 这一步，
  组名与覆盖面不变。
