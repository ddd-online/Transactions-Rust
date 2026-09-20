//! 页面层。
//!
//! * [`accounting`]：**记账**（顶级功能，含 记录 / 标签 / 模板 三个子功能 + 左侧子功能图标条）
//! * [`transactions`]：记账 · 记录子功能（列表 + 记一笔/编辑/删除/关联 + 筛选 + 排序）
//! * [`category_tag`]：记账 · 标签子功能（分类 / 标签两栏联动 + 拖拽排序）
//! * [`templates`]：记账 · 模板子功能（消费模板列表 + 新建 / 删除 / 拖拽排序）
//! * [`data_analysis`]：数据分析（图表列表 + 新建/删除 + 曲线条件 + 自绘 SVG 渲染）
//! * [`stock`]：股票交易（账户 / 持仓 / 交易历史 / 交易统计 四个分栏）
//! * [`key_event`]：关键事件（年份 + 事件列表 + 详情 + 图片 + 关联消费记录）
//! * [`diary`]：日记管理（日期树 + 编辑器）
//! * [`settings`]：应用设置（4 个分栏）

pub mod accounting;
pub mod category_tag;
pub mod data_analysis;
pub mod diary;
pub mod key_event;
pub mod settings;
pub mod stock;
pub mod templates;
pub mod transactions;

pub use accounting::AccountingPage;
pub use data_analysis::DataAnalysisPage;
pub use diary::DiaryPage;
pub use key_event::KeyEventPage;
pub use settings::SettingsPage;
pub use stock::StockPage;
