//! 数据模型：与工作空间 SQLite 表一一对应，同时承担 IPC 序列化契约。
//!
//! **字段名纪律**：每个结构体的 serde 名称都是既成契约的一部分，**不允许"顺手统一"**
//! ——否则前端字段全部失效。命名本身并不统一（核心记账模型是 snake_case，
//! 关键事件/日记/股票模型是 camelCase），这是 IPC 契约的既成事实。
//!
//! 数据库列名与 JSON 名称不同（列名恒为 snake_case），因此列映射在 DAO 层显式书写，
//! 不依赖 serde，避免两套命名互相污染。

pub mod core;
pub mod stock;

pub use core::*;
pub use stock::*;
