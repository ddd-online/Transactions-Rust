//! DTO 层：界面（IPC）与内核之间的数据交换结构。
//!
//! 逐字段照抄原 Go `kernel/models/dto/*.go` 的 `json` tag。原实现的命名并不统一
//! （同一文件里 camelCase 与 snake_case 混用），这是既成契约，**不允许统一**。
//!
//! 与模型的区别：模型对应数据库行，DTO 对应 IPC 出入参。转换函数（`From*`）与
//! 校验函数（`validate`）的错误文案都与原实现逐字一致。

pub mod core;
pub mod diary;
pub mod stock;

pub use core::*;
pub use diary::*;
pub use stock::*;
