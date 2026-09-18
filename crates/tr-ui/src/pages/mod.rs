//! 页面层。
//!
//! | 模块 | 页面 | 对照的原组件 |
//! |---|---|---|
//! | [`transactions`] | 消费记录（列表 + 记一笔/编辑/删除/关联 + 筛选 + 排序） | `tr_view/TransactionRecordView.vue` 等 |
//! | [`data_analysis`] | 数据分析（图表列表 + 新建/删除 + 曲线条件 + 自绘 SVG 渲染） | `da_view/*.vue` |
//! | [`stock`] | 股票交易（账户 / 持仓 / 交易历史 / 交易统计 四个分栏） | `stock_view/*.vue` |
//! | [`key_event`] | 关键事件（年份 + 事件列表 + 详情 + 图片 + 关联消费记录） | `key_event_view/*.vue` |
//! | [`diary`] | 日记管理（日期树 + Markdown 编辑器） | `diary_view/*.vue` |
//! | [`category_tag`] | 分类标签（三栏联动 + 拖拽排序） | `settings_view/TransactionsCategoryTagSetting.vue` |
//! | [`settings`] | 应用设置（5 个分栏） | `settings_view/SettingsView.vue` 等 |
//! | [`placeholders`] | 尚未移植的页面占位（P6-b 后只剩「关于/更新」类的次要入口，若需要） | —— |

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
