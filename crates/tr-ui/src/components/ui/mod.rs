//! 通用组件套件（P5 首批 8 个 + P6-a 增补 16 个）。
//!
//! 外观对齐 `DESIGN.md` 与原 `index.scss` / `_components.scss`：
//! 按钮 36px 高 / 8px 圆角 / 主色 `#3964fe`（令牌 `--transactions-color-primary`）、
//! 输入框 36px / 8px / 1px 发丝边框、模态 16px 圆角 + 大阴影、
//! tag 语义色 10% 底 + 语义色文字、`focus-visible` 2px 主色环。
//!
//! 样式全部在 `static/css/ui.css`，类名以 `ui-` 前缀区分。
//!
//! ## P6-a 增补（本轮）
//!
//! | 组件 | 对应原 Ant Design Vue | 主要用途 |
//! |---|---|---|
//! | [`Table`] | `a-table` | 消费记录 / 分类标签 / 模板列表 |
//! | [`Pagination`] | `a-pagination` | 消费记录分页（含每页条数） |
//! | [`Form`] / [`FormItem`] | `a-form` / `a-form-item` | 设置页与弹窗的字段排版 |
//! | [`Tabs`] / [`TabPane`] | `a-tabs` | 设置页 5 个分栏 |
//! | [`Segmented`] | `a-segmented` | 交易类型 / 外观 |
//! | [`Switch`] | `a-switch` | 离群值 / 开发者工具 |
//! | [`Popconfirm`] | `a-popconfirm` | 删除二次确认 |
//! | [`DatePicker`] / [`DateRangePicker`] | `a-date-picker` / `a-range-picker` | 交易日期 / 筛选时间范围 |
//! | [`Dropdown`] | `a-dropdown` | 行内「更多」操作 |
//! | [`Popover`] | `a-popover` | 条件小结 / 说明浮层 |
//! | [`FloatButton`] | `a-float-button` | 右下角「记一笔」 |
//! | [`Drawer`] | `a-drawer` | 筛选面板（消费记录页） |
//! | [`Divider`] | `a-divider` | 设置页分块 |
//! | [`Checkbox`] / [`CheckboxGroup`] | `a-checkbox` / `a-checkbox-group` | 多选标签 / 文件勾选 |
//! | [`Progress`] | `a-progress` | 更新下载进度 |
//! | [`DragSortItem`] | （原文 `useListDragSort.ts`） | 分类/标签/模板排序 |
//!
//! ## P6-b 增补（本轮）
//!
//! | 组件 | 对应原实现 | 主要用途 |
//! |---|---|---|
//! | [`LineChart`] | `da_view/TransactionsChart.vue`（ECharts） | 自绘 SVG 折线图（多序列 / 类目轴 / 图例 / 轴 tooltip / 虚线参考线） |
//! | [`Markdown`] | `utils/markdown.ts` + `MarkdownViewer.vue` | 纯 Rust Markdown 渲染（先转义再拼标签，无注入面） |
//! | [`ImagePicker`] | `KeyEventImageGallery.vue` 的文件输入 | 隐藏 `input[type=file]` + 触发按钮 |
//! | [`UploadProgressBar`] | `key_event_view/UploadProgressBar.vue` | 多文件上传进度（总进度 + 逐文件 + 重试/跳过） |

mod button;
mod chart;
mod checkbox;
mod date_picker;
mod divider;
mod drag_sort;
mod drawer;
mod dropdown;
mod empty;
mod float_button;
mod form;
mod image_picker;
mod input;
mod markdown;
mod modal;
mod pagination;
mod popconfirm;
mod popover;
mod progress;
mod segmented;
mod select;
mod spin;
mod switch;
mod table;
mod tabs;
mod tag;
mod textarea;
mod tooltip;

pub use button::{Button, ButtonSize, ButtonVariant};
pub use chart::{ChartConfig, ChartPoint, ChartSeries, ChartValueKind, LineChart};
pub use checkbox::{Checkbox, CheckboxGroup, CheckboxOption};
pub use date_picker::{add_months, parse_ymd, today, DatePicker, DateRangePicker, Ymd};
pub use divider::Divider;
pub use drag_sort::{DragSortItem, DragSortState};
pub use drawer::Drawer;
pub use dropdown::{Dropdown, DropdownItem};
pub use empty::Empty;
pub use float_button::FloatButton;
pub use form::{Form, FormItem, FormLayout};
pub use image_picker::{
    blob_to_data_url, convert_heic_to_jpeg, is_heic, read_as_data_url, FileStatus, ImagePicker,
    UploadFileProgress, UploadProgress, UploadProgressBar, UploadStatus, HEIC_CONVERT_FAILED,
};
pub use input::Input;
pub use markdown::{render_markdown, Markdown};
pub use modal::Modal;
pub use pagination::{page_slots, Pagination, PAGE_SIZE_OPTIONS};
pub use popconfirm::Popconfirm;
pub use popover::Popover;
pub use progress::Progress;
pub use segmented::{Segmented, SegmentedOption};
pub use select::{Select, SelectOption};
pub use spin::{Spin, SpinSize};
pub use switch::Switch;
pub use table::{Table, TableAlign, TableColumn};
pub use tabs::{TabItem, TabPane, Tabs};
pub use tag::{Tag, TagKind};
pub use textarea::Textarea;
pub use tooltip::Tooltip;
