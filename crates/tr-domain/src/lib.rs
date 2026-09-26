//! tr-domain —— Transactions 的纯领域层。
//!
//! 分层纪律：
//! * 本 crate **不做任何 I/O**：不连数据库、不发网络请求、不碰文件系统、不依赖 tauri。
//! * 因此它可以同时被 `tr-store`/`tr-service`（native）与 `tr-ui`（wasm32）依赖，
//!   界面与后端共享**同一份**金额换算、费用分摊与时间段算法，杜绝两侧漂移。
//! * 序列化字段名是 IPC 契约的一部分，必须逐字段保持稳定（见各模块注释）。

pub mod consts;
pub mod dto;
pub mod error;
pub mod fee;
pub mod models;
pub mod money;
pub mod proxy;
pub mod util;
pub mod wire;
