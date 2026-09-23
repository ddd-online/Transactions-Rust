---
version: 1
slug: "crates-tr-ui-src-pages-key-event-rs"
primary_target: "crates/tr-ui/src/pages/key_event.rs"
related_targets: ["crates/tr-ui/static/css/key_event.css"]
---

# Surface brief — 事件页：详情卡 + 可收起的关联交易边轨（crates/tr-ui/src/pages/key_event.rs）

**Scope / visitor mode:** Operate。事件页的中栏详情（主题色 / 图片 / 描述 / 底部动作）与右栏关联交易列表。
使用者是自己记账的人：打开事件是为了**读这一篇**，右栏只是佐证；偶尔需要把宽度全留给正文。

## Direction contract

**THESIS:** 详情是**一张卡**——主题色、图片、描述、动作落在同一张纸面上，中栏从"画布上散着的内容"变成一件可被指认的物件，与左栏那排小事件卡形成"条目 / 容器"两档；右栏关联交易是**可收起的边轨**，宽度该让给正在读的那件事时就让。

**OWN-WORLD:** 卡沿用本页已有的卡片语言：Paper 面 + 1px 发丝 + 12px 圆角 + `shadow-sm`；卡内**不再套第二张卡**，层次靠色阶——描述区保留软灰内嵌井、图片区保留虚线框。收起把手是骑在中栏/右栏那条竖线上的 24×48 药丸（Paper + 发丝，静置不投影、悬停抬到 `shadow-md`；24px 同时是点按目标下限），只画一枚 chevron，方向和"栏会往哪边走"一致。

**STORY:** 打开事件 → 一眼看出这块属于这篇事件（卡片 + 卡顶那条主题色带，与左栏卡片左侧那条同色条呼应）；正文太长或想让图片更大时收掉右栏，中栏吃满宽度；把手始终停在原处，随时收回来。空态与加载态都在同一张卡里，卡片形状不跳。

**FIRST VIEWPORT:** 三栏结构不变（280 / 自适应 / 280）。中栏 = 一张卡（距分栏线 16px，卡内 24px）：**主题色带压在卡的上边缘**（4px 通栏、跟着卡片的 12px 圆角走，与左栏事件卡左侧那条同色同粗；事件没设颜色时透明、卡面高度不跳），其下是 24px 内缩的色板行 + 一条发丝线，中间图片区与描述井按内容分配高度（灰井只包住文字，长文在井内滚动，省下的高度留成卡面），底部一条发丝线收住右对齐的动作。把手垂直居中、骑在竖线上；收起后第三列宽 0、竖线隐去、中栏吃满，把手贴到窗口右缘。

**FORM:** 既有世界的局部延伸（不新造世界、不重构页面）：结构不动，只把中栏内容收进容器，并给右栏加一个可逆的显隐开关。

**FINISH:** unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance.

## 未决 / 备注

- 收起状态**写进用户配置**（`keyEventLinkedOpen`，缺省展开）：外壳 `config_set_key_event_linked_open`
  落盘，界面读全局 `store::AppStores::key_event_linked_open`。放在全局状态而不是页面局部，
  是为了换页回来不必等 IPC 往返、也不会先展开再收起闪一帧。端到端见 `fixtures/ui-key-event.ps1` 第 4 步。
- 卡片取 DESIGN.md 的 12px 圆角（`radius-lg`），左栏小事件卡保持既有 8px：容器与条目两档，不为了统一去改既有卡片。
- 收起时右栏用 `visibility: hidden`（延迟到动画结束才切），既退出无障碍树与键盘序，又保住滚动位置。
