---
version: 1
slug: "crates-tr-ui-src-components-ui-chart-rs"
primary_target: "crates/tr-ui/src/components/ui/chart.rs"
related_targets: []
---

# Surface brief — 图表组件（crates/tr-ui/src/components/ui/chart.rs）

**Scope / visitor mode:** Operate。数据分析页与股票交易页的折线图，覆盖"读趋势、比量级、看某月/某笔明细"。

## Direction contract

**THESIS:** 图表由**引擎产 SVG + 设计令牌主题**驱动；拒绝的类别默认是"直接吃图表库自带配色与缩写刻度"（那会让图表变成一件外来物）。

**OWN-WORLD:** 令牌即主题 —— 网格取 divider、轴线取 window-border、文字取 text-secondary、序列色只取语义色；刻度与图例等宽数字 12px；发丝线网格；面积填充**只给单序列**；图例画在图表内、左对齐。

**STORY:** 打开页面的人先读出走势与量级（刻度是全值，不是 38.5k），再悬停看整列数值与合计；浅色深色都成立。

**FIRST VIEWPORT:** 图表铺满所在画布（按容器实测像素渲染，不是固定逻辑尺寸再缩放）；**直连折线**（不平滑：平滑会在采样点之间造出数据里没有的起伏）+ 每个类目 2.5px 标记点；顶部左对齐图例；只保留横向发丝网格。

**FORM:** 组件级替换（既有设计世界内的延伸，不新造世界）。引擎选 `charts-rs 1.0`（纯 Rust、默认特性 0、可编 wasm）—— 拿到 Chart.js 那套现代观感，同时守住"界面零 JS 图表库、离线可用、颜色只来自令牌"。

**FINISH:** unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance.

## 未决 / 备注

- 派生图表类型（柱/饼）未纳入本轮；引擎已支持，接入时沿用同一套令牌主题。
- charts-rs 的面积填充接近实色，多序列叠加会糊：已按"单序列才填充"处理；若将来要渐变填充，需在 SVG 串里注入 `linearGradient`。
