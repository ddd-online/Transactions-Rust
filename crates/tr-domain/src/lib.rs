//! tr-domain —— Transactions 的纯领域层。
//!
//! 分层纪律（对照原 Go 版 `models` + `models/dto` + 纯计算工具）：
//! * 本 crate **不做任何 I/O**：不连数据库、不发网络请求、不碰文件系统、不依赖 tauri。
//! * 因此它可以同时被 `tr-store`/`tr-service`（native）与 `tr-ui`（wasm32）依赖，
//!   界面与后端共享**同一份**金额换算、费用分摊与时间段算法，杜绝两侧漂移。
//! * 序列化字段名必须与原 Go 结构体逐字段一致（见各模块注释），这是 IPC 契约的一部分。

pub mod consts;
pub mod dto;
pub mod error;
pub mod fee;
pub mod models;
pub mod money;
