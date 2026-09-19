//! DTO 层：界面（IPC）与内核之间的数据交换结构。
//!
//! 字段的 serde 名称是既成契约，**不允许统一**：同一文件里 camelCase 与 snake_case 混用，
//! 改名即破坏契约。
//!
//! 与模型的区别：模型对应数据库行，DTO 对应 IPC 出入参。转换函数（`From*`）与
//! 校验函数（`validate`）的错误文案是用户可见文案（改动即破坏契约）。

pub mod core;
pub mod diary;
pub mod stock;

pub use core::*;
pub use diary::*;
pub use stock::*;
