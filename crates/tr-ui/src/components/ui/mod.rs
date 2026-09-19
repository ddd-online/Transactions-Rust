//! 通用组件套件（P5 首批 8 个 + P6-a 增补 16 个）。
//!
//! 外观对齐 `DESIGN.md`：
//! 按钮 36px 高 / 8px 圆角 / 主色 `#3964fe`（令牌 `--transactions-color-primary`）、
//! 输入框 36px / 8px / 1px 发丝边框、模态 16px 圆角 + 大阴影、
//! tag 语义色 10% 底 + 语义色文字、`focus-visible` 2px 主色环。
//!
//! 样式全部在 `static/css/ui.css`，类名以 `ui-` 前缀区分。
//!
//! ## P6-a 增补（本轮）
//!
//! 表格与分页（消费记录 / 分类标签 / 模板列表）、表单与表单项、标签页（设置页 5 个分栏）、
//! 分段控制器（交易类型 / 外观）、开关（离群值 / 开发者工具）、二次确认、日期与日期区间选择、
//! 下拉菜单（行内「更多」）、气泡卡片（条件小结 / 说明浮层）、悬浮按钮（右下角「记一笔」）、
//! 抽屉（筛选面板）、分割线、复选框与复选框组、进度条（更新下载）、
//! 拖拽排序项（分类 / 标签 / 模板排序）。
//!
//! ## P6-b 增补（本轮）
//!
//! 折线图（自绘 SVG：多序列 / 类目轴 / 图例 / 轴 tooltip / 虚线参考线）、
//! Markdown 渲染（纯 Rust：先转义再拼标签，无注入面）、
//! 图片选择（隐藏 `input[type=file]` + 触发按钮）、
//! 多文件上传进度条（总进度 + 逐文件 + 重试/跳过）。

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
