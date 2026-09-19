//! 页面层。
//!
//! * [`transactions`]：消费记录（列表 + 记一笔/编辑/删除/关联 + 筛选 + 排序）
//! * [`data_analysis`]：数据分析（图表列表 + 新建/删除 + 曲线条件 + 自绘 SVG 渲染）
//! * [`stock`]：股票交易（账户 / 持仓 / 交易历史 / 交易统计 四个分栏）
//! * [`key_event`]：关键事件（年份 + 事件列表 + 详情 + 图片 + 关联消费记录）
//! * [`diary`]：日记管理（日期树 + Markdown 编辑器）
//! * [`category_tag`]：分类标签（三栏联动 + 拖拽排序）
//! * [`settings`]：应用设置（5 个分栏）
//! * [`placeholders`]：占位页（P6-b 后只剩「关于/更新」类的次要入口，若需要）

pub mod category_tag;
pub mod data_analysis;
pub mod diary;
pub mod key_event;
pub mod placeholders;
pub mod settings;
pub mod stock;
pub mod transactions;

pub use category_tag::CategoryTagPage;
pub use data_analysis::DataAnalysisPage;
pub use diary::DiaryPage;
pub use key_event::KeyEventPage;
pub use settings::SettingsPage;
pub use stock::StockPage;
pub use transactions::TransactionsPage;
