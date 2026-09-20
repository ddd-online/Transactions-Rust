//! tr-store —— 工作空间存储层。
//!
//! **数据兼容纪律（本 crate 最重要的一条）**
//! * 工作空间数据库的 schema 只认"当前格式"：`transactions.db` 不存在时用
//!   `fixtures/schema/fresh.sql` 建库；已存在时先跑**迁移引擎**（[`migrations`]）把更早格式升级到
//!   当前格式，再按当前格式只读校验（[`schema::validate_current`]）。
//! * 迁移是**允许且唯一**的结构变更路径：所有结构变更都必须写成 [`migrations::MIGRATIONS`] 里的一条，
//!   遵守那里的编写规范（事务内、幂等、只碰本次涉及的表、带单测）。除此之外不得就地改写既有库。
//! * 升级前一定先备份（`transactions.db.pre-migration-<时间戳>.bak`，见 [`migrations::apply_all`]）。

pub mod dao;
pub mod migrations;
pub mod schema;
pub mod util;
pub mod workspace;

pub use workspace::{Workspace, WorkspaceError, WsManager};
