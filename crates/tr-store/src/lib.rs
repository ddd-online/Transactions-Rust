//! tr-store —— 工作空间存储层。
//!
//! **数据兼容纪律（本 crate 最重要的一条）**
//! * 工作空间数据库的 schema 只认"当前格式"：`transactions.db` 不存在时用
//!   `fixtures/schema/fresh.sql` 建库；已存在时**只做只读校验**，绝不执行 DDL/DML 去改结构。
//! * 本仓库**不包含**任何 schema 迁移代码：没有补列/加索引、没有版本化迁移、没有库文件改名。
//!   更早格式的工作空间会被明确拒绝，由用户自行用支持该格式的旧版本升级到当前格式。

pub mod dao;
pub mod schema;
pub mod util;
pub mod workspace;

pub use workspace::{Workspace, WorkspaceError, WsManager};
