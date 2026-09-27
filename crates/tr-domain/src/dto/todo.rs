//! 待办 DTO。
//!
//! 界面只发两个查询：**卡片视图**（卡片 + 卡片下进行中的事项 + 它们的进度记录）
//! 与**历史**（已完成的事项 + 主题名 + 它们的进度记录）。两张视图都把进度记录**内嵌**下来，
//! 展开与弹窗都不再发请求 —— 单个账本的待办量级很小，省一次往返比省字节值。

use serde::{Deserialize, Serialize};

use crate::models::{TodoCard, TodoItem, TodoProgress};

/// 一条进度记录（对外不带 `ledger_id`：它属于哪个账本由事项决定）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TodoProgressDto {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "itemId")]
    pub item_id: String,
    #[serde(rename = "content")]
    pub content: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

impl From<&TodoProgress> for TodoProgressDto {
    fn from(progress: &TodoProgress) -> Self {
        Self {
            id: progress.id.clone(),
            item_id: progress.item_id.clone(),
            content: progress.content.clone(),
            created_at: progress.created_at,
        }
    }
}

/// 卡片视图里的一条事项（进行中）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TodoItemDto {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "cardId")]
    pub card_id: String,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "startDate")]
    pub start_date: String,
    #[serde(rename = "dueDate")]
    pub due_date: String,
    #[serde(rename = "urgency")]
    pub urgency: i32,
    #[serde(rename = "importance")]
    pub importance: i32,
    #[serde(rename = "status")]
    pub status: String,
    #[serde(rename = "completedAt")]
    pub completed_at: i64,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
    /// 进度记录（按时间升序）
    #[serde(rename = "progress")]
    pub progress: Vec<TodoProgressDto>,
}

impl TodoItemDto {
    /// 由事项行 + 它的进度记录组装。
    pub fn from_item(item: &TodoItem, progress: Vec<TodoProgressDto>) -> Self {
        Self {
            id: item.id.clone(),
            card_id: item.card_id.clone(),
            title: item.title.clone(),
            start_date: item.start_date.clone(),
            due_date: item.due_date.clone(),
            urgency: item.urgency,
            importance: item.importance,
            status: item.status.clone(),
            completed_at: item.completed_at,
            created_at: item.created_at,
            updated_at: item.updated_at,
            progress,
        }
    }
}

/// 一张卡片 + 它下面进行中的事项（卡片没有事项时 `items` 为空）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TodoCardDto {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
    #[serde(rename = "items")]
    pub items: Vec<TodoItemDto>,
}

impl TodoCardDto {
    pub fn from_card(card: &TodoCard, items: Vec<TodoItemDto>) -> Self {
        Self {
            id: card.id.clone(),
            title: card.title.clone(),
            created_at: card.created_at,
            updated_at: card.updated_at,
            items,
        }
    }
}

/// 历史里的一条：已完成的事项 + **主题名**（卡片被删时这条跟着删，不会出现空主题）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TodoHistoryDto {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "cardId")]
    pub card_id: String,
    #[serde(rename = "cardTitle")]
    pub card_title: String,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "startDate")]
    pub start_date: String,
    #[serde(rename = "dueDate")]
    pub due_date: String,
    #[serde(rename = "urgency")]
    pub urgency: i32,
    #[serde(rename = "importance")]
    pub importance: i32,
    #[serde(rename = "completedAt")]
    pub completed_at: i64,
    #[serde(rename = "progress")]
    pub progress: Vec<TodoProgressDto>,
}

impl TodoHistoryDto {
    pub fn from_item(item: &TodoItem, card_title: &str, progress: Vec<TodoProgressDto>) -> Self {
        Self {
            id: item.id.clone(),
            card_id: item.card_id.clone(),
            card_title: card_title.to_string(),
            title: item.title.clone(),
            start_date: item.start_date.clone(),
            due_date: item.due_date.clone(),
            urgency: item.urgency,
            importance: item.importance,
            completed_at: item.completed_at,
            progress,
        }
    }
}
