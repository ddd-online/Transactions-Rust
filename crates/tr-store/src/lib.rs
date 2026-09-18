//! tr-store —— 工作空间存储层。
//!
//! 对照原 Go 版 `kernel/workspace/*` + `kernel/dao/*` + `kernel/util/database.go`。
//!
//! **数据兼容纪律（本 crate 最重要的一条）**
//! * 工作空间数据库的 schema 只认"最新格式"（v0.27 起）：`transactions.db` 不存在时用
//!   `fixtures/schema/fresh_v0_27.sql` 建库；已存在时**只做只读校验**，绝不执行 DDL/DML 去改结构。
//! * 本仓库**不包含**任何 schema 迁移代码：没有 AutoMigrate 等价物、没有补列/加索引、
//!   没有版本化迁移、没有 `billadm.db` 改名。更早版本的工作空间会被明确拒绝，
//!   由用户自行用 0.27 版打开一次完成升级。

pub mod dao;
pub mod schema;
pub mod util;
pub mod workspace;

pub use workspace::{Workspace, WorkspaceError, WsManager};
