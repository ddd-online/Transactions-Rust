//! 待办：卡片（主题） / 事项 / 进度记录。
//!
//! 三张表都按账本隔离（`ledger_id`），列名恒为 snake_case；结构体上的 serde 名称是
//! IPC 契约的一部分（与关键事件、日记同款 camelCase），**不允许"顺手统一"**。
//!
//! 「完成」不是搬表：事项留在 `tbl_billadm_todo_item` 里，只把 `status` 置成 `done`
//! 并记 `completed_at` —— 卡片视图只取进行中的、历史视图只取已完成的，
//! 于是「移到历史」是**同一张表的两个视图**，不做数据搬运（编辑/删除仍只有一条路径）。

use serde::{Deserialize, Serialize};

/// 待办卡片（一个「主题」）。表 `tbl_billadm_todo_card`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TodoCard {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    /// 卡片主题（用户创建卡片时指定，最长 200 字符）
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

/// 待办事项。表 `tbl_billadm_todo_item`。
///
/// `start_date` / `due_date` 是**日粒度**的 `YYYY-MM-DD`（空串 = 没填），
/// 与「事件日期」「股票委托时间」同口径。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TodoItem {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    /// 所属卡片
    #[serde(rename = "cardId")]
    pub card_id: String,
    /// 事项
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "startDate")]
    pub start_date: String,
    #[serde(rename = "dueDate")]
    pub due_date: String,
    /// 紧急度（-5..=5）
    #[serde(rename = "urgency")]
    pub urgency: i32,
    /// 重要度（-5..=5）
    #[serde(rename = "importance")]
    pub importance: i32,
    /// 状态：`doing` / `done`
    #[serde(rename = "status")]
    pub status: String,
    /// 完成时刻（未完成 = 0）
    #[serde(rename = "completedAt")]
    pub completed_at: i64,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

/// 一条进度记录。表 `tbl_billadm_todo_progress`。
///
/// 只增不写：记录写完不提供"编辑"（要改就删了重记），`created_at` 就是记录时间。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TodoProgress {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    /// 所属事项
    #[serde(rename = "itemId")]
    pub item_id: String,
    /// 进度正文（最长 2000 字符）
    #[serde(rename = "content")]
    pub content: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}
