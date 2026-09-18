# UI 组件套件清单（对照原实现）

这份清单用来核对"33 组件 UI 套件"这条交付。口径说明（可自行复核）：

- `crates/tr-ui/src/components/ui/` 共 **28 个文件**；
- `mod.rs` 对外导出 **47 个名字**，其中**可渲染组件 32 个**，其余是配置类型（`ButtonSize`、
  `ChartConfig`、`TabItem` …）与工具函数（`render_markdown`、`parse_ymd`、`convert_heic_to_jpeg` …）。
  32 与计划里的"33 组件"差 1：`UploadProgressBar` 被**合并**进 `image_picker.rs`（见第三节），
  没有单列文件。

原 Electron 版的界面由 **48 个 Vue 组件**组成（`app/src/components/**`），下表给出对应关系。

判定口径：**不追求 1:1 搬运文件**。原实现依赖 `ant-design-vue` 4.2.6 与若干 JS 库（`echarts`、
`marked`、`highlight.js`、`dompurify`、`sortablejs`、`heic-to`、`dayjs`），本项目是"界面无 Node"的纯 Rust 栈，
因此第三方组件在 Rust 侧重写，但**外观、交互与文案**以原实现与 `DESIGN.md` 为准。
下表"原实现对应"一栏是按原项目 `app/package.json` 与实际使用的标签核对过的（例如 `a-button` 用了 75 处、
`a-input` 27 处、`a-tooltip` 31 处）；标"套件补充"的是原实现没直接用、但套件里保留的通用件。
字体沿用原项目的 `@fontsource/inter` 与 `@fontsource/jetbrains-mono`（见 `static/css/fonts.css`）。

## 一、基础组件套件（`components/ui/`）

| Rust 文件 | 导出 | 原实现对应 |
|---|---|---|
| `button.rs` | `Button`(+Size/Variant) | `a-button`（含 primary/danger/text 等形态） |
| `input.rs` | `Input` | `a-input` |
| `textarea.rs` | `Textarea` | `a-textarea` |
| `select.rs` | `Select`(+Option) | `a-select` |
| `checkbox.rs` | `Checkbox` / `CheckboxGroup` | **套件补充**（原实现未直接使用 `a-checkbox`） |
| `switch.rs` | `Switch` | `a-switch` |
| `segmented.rs` | `Segmented` | `a-segmented` |
| `tabs.rs` | `Tabs` / `TabPane` | `a-tabs` |
| `tag.rs` | `Tag` | `a-tag`（含颜色样式） |
| `divider.rs` | `Divider` | `a-divider` |
| `empty.rs` | `Empty` | `a-empty` |
| `spin.rs` | `Spin` | `a-spin` |
| `progress.rs` | `Progress` | `a-progress` + `UploadProgressBar.vue` 的百分比条 |
| `tooltip.rs` | `Tooltip` | `a-tooltip` |
| `popover.rs` | `Popover` | `a-popover` |
| `popconfirm.rs` | `Popconfirm` | `a-popconfirm` |
| `dropdown.rs` | `Dropdown`(+Item) | `a-dropdown` |
| `drawer.rs` | `Drawer` | **套件补充**（原实现未直接使用 `a-drawer`） |
| `modal.rs` | `Modal` | `a-modal` + `TransactionRecordModal` / `KeyEventAddModal` / `StockTradeEditModal` / `TrSortModal` 的外壳 |
| `form.rs` | `Form` / `FormItem` / `FormLayout` | `a-form` / `a-form-item` |
| `table.rs` | `Table`(+Column/Align) | `a-table` + `TransactionRecordTable.vue` |
| `pagination.rs` | `Pagination`(+`page_slots`) | `a-pagination`（每页 15/30/50/100，`共 N 条记录`） |
| `date_picker.rs` | `DatePicker` / `DateRangePicker` | `a-date-picker` / `a-range-picker` + `TransactionsTimeRangePicker.vue` |
| `float_button.rs` | `FloatButton` | `a-float-button`（返回顶部） |
| `drag_sort.rs` | `DragSortState` / `DragSortItem` | `sortablejs`（原分类/标签/模板拖拽排序用的就是它） |
| `chart.rs` | `LineChart`(+Config/Series/Point) | `TransactionsChart.vue` 等（原用 `echarts` + `vue-echarts`，这里自绘 SVG） |
| `markdown.rs` | `Markdown` / `render_markdown` | `MarkdownViewer.vue`（原用 `marked` + `highlight.js` + `dompurify`） |
| `image_picker.rs` | `ImagePicker` | `KeyEventImageGallery.vue` 的 `<input type=file>` + `heic-to` 转码 + 上传状态机 |

## 二、页面与其原组件（`crates/tr-ui/src/pages/`）

| Rust 页面 | 原 Vue 组件 |
|---|---|
| `transactions.rs` | `TransactionRecordView` / `TransactionRecordTable` / `TransactionRecordModal` / `TransactionRecordFilter` / `TrSortModal` / `TransactionsPageHeader` / `TransactionsPageLayout` / `TransactionsStatisticsFooter` |
| `data_analysis.rs` | `DataAnalysisView` / `TransactionsChartView` / `TransactionsChartList` / `TransactionsChartLines` |
| `stock.rs` | `StockTradingView` / `StockAccountView` / `StockPositionView` / `StockTradeRecordView` / `StockStatisticsView` / `StockStatisticsRangeFilter` / `StockStatisticsTagFilter` / `StockTradeEditModal` |
| `key_event.rs` | `KeyEventView` / `KeyEventList` / `KeyEventDetail` / `KeyEventAddModal` / `KeyEventImageGallery` / `KeyEventLinkedTr` / `UploadProgressBar` |
| `diary.rs` | `DiaryView` / `DiaryTree` / `DiaryEditor` |
| `category_tag.rs` | `TransactionsCategoryTagSetting` / `CategoryColumn` / `TagColumn` |
| `settings.rs` | `SettingsView` / `GeneralSetting` / `AboutSetting` / `DiarySetting` / `StockTradingSetting` / `TransactionsTemplateSetting` / `SettingsPageWrapper` |
| `shell.rs`（外壳，非页面） | `App.vue` / `Layout.vue` 的侧栏、顶栏、状态栏与账本选择器 |

## 三、有意未搬运 / 合并的部分

| 原组件能力 | 处理 | 理由 |
|---|---|---|
| `UploadProgressBar.vue` 的"跳过"分支 | 合并进 `image_picker.rs`，状态改为"已跳过" | 原实现跳过后该行会永久停在"上传中"（既有缺陷，未照抄） |
| Markdown 语法高亮（highlight.js） | **未实现** | 界面无 Node，不引入 JS 库；代码块按等宽 + 底色渲染 |
| Markdown 的 `data:` / `asset:` / `trasset:` URL | **降级为纯文本** | 白名单比原 DOMPurify 更严，无注入面 |
| ECharts 的 dataZoom / 图例点击开关 | **未实现** | 原 `da_view` 也没注册 `DataZoomComponent`，属等价；图例点击原实现也没有 |
| 内核状态指示灯、内核重启恢复 | **不做** | 本仓库没有子进程内核，进程即应用（见 AGENTS.md） |
| 智能助手入口 | **不做** | 原版 v0.27.0 已移除该功能 |

## 四、如何自检

```powershell
# 组件清单（文件数与导出）
Get-ChildItem crates/tr-ui/src/components/ui -File -Filter *.rs | Where-Object Name -ne 'mod.rs'
Get-Content crates/tr-ui/src/components/ui/mod.rs

# 设计令牌合规（零硬编码颜色、深色主题覆盖完整）
pwsh -File fixtures/design-audit.ps1

# 逐页渲染（UIA 结构 + 像素内容 + 主题切换）
pwsh -File fixtures/ui-smoke.ps1 -Workspace <ws> -WriteFlow
pwsh -File fixtures/ui-shots.ps1  -Workspace <ws>
```

## 五、改界面时的五条审计（都是踩过或差点踩过的类）

这几个月里真正出过问题的都不是"写不出来"，而是**静默失效**。下面五条用几条 grep 就能扫一遍，
建议改完界面（尤其动共享组件）后跑一次：

| 审计 | 怎么查 | 反面样例 |
|---|---|---|
| **事件阶段** | 共享组件若把"打开/切换"挂在**冒泡阶段**的包裹元素上，而调用方的子元素写了 `stop_propagation()`，点击会被吃掉、气泡永不弹 | `Popconfirm` 曾经的三个删除操作全部点不动（已改 `on:click:capture`） |
| **effect 追踪** | 找出所有 `Effect::new`，看它是否真的读了该追踪的信号（用 `.get()` 而不是 `get_untracked()`；通过 helper 读取也可以，但要确认 helper 里是 `.get()`） | 记账页曾因 effect 用 `get_untracked()` 导致列表永远空白 |
| **错误被吞** | `if let Ok(...) = api::…` 是否只是"刷新失败保留旧数据"的合理语义；`let _ = api::…` 是否真的无关紧要 | 删/改类操作静默失败会让用户以为生效了 |
| **金额** | 界面不得自行 `/100`（只允许 `format.rs` 的展示辅助）；**提交**路径必须走 `tr_domain::money::yuan_to_cents` | 元/分混用会让金额差 100 倍 |
| **时区** | 记录时间照抄原实现是"所选日期**本地 12:00**"（`hour(12)`）；筛选区间是"本地 00:00:00 ~ 23:59:59 闭区间"；两者都必须用 `js_sys::Date`（本地）而不是纯整数 UTC 运算 | 换成 UTC 会让跨时区用户的记录落到相邻日期 |

