//! tr-service —— 业务服务层。
//!
//! 调用链：api（命令面）→ service（业务规则）→ dao（SQL）→ models。
//! 本层不依赖 tauri，因此可以用普通 `cargo test` 对全部业务规则做无界面验证。
//!
//! 各业务服务随阶段补齐：
//! * P2：账本 / 消费记录（含筛选、统计、图表分桶）/ 分类 / 标签 / 模板 / 图表
//! * P3：关键事件 / 资产与缩略图 / 日记
//! * P4：股票（账户 / 委托与费用 / 持仓重放 / 轮次历史 / 统计 / 行情）

pub mod assets;
pub mod category;
pub mod chart;
pub mod diary;
pub mod error;
pub mod key_event;
pub mod ledger;
pub mod quote;
pub mod stock;
pub mod stock_statistics;
pub mod tag;
pub mod transaction_record;
pub mod transaction_template;

pub use error::{ServiceError, ServiceResult};
