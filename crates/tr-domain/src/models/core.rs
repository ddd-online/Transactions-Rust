//! 核心记账模型（账本 / 消费记录 / 分类 / 标签 / 模板 / 图表 / 关键事件 / 日记）。

use serde::{Deserialize, Serialize};

/// 账本。表 `tbl_billadm_ledger`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ledger {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "description")]
    pub description: String,
    #[serde(rename = "created_at")]
    pub created_at: i64,
    #[serde(rename = "updated_at")]
    pub updated_at: i64,
}

/// 消费记录标记集（序列化进 `transaction_record.flags` 列，形如 `{"outlier":true}`）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionRecordFlags {
    #[serde(rename = "outlier")]
    pub outlier: bool,
}

/// 消费记录。表 `tbl_billadm_transaction_record`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TransactionRecord {
    #[serde(rename = "transaction_id")]
    pub transaction_id: String,
    #[serde(rename = "ledger_id")]
    pub ledger_id: String,
    /// 交易金额（分）
    #[serde(rename = "price")]
    pub price: i64,
    #[serde(rename = "transaction_type")]
    pub transaction_type: String,
    #[serde(rename = "category")]
    pub category: String,
    #[serde(rename = "description")]
    pub description: String,
    /// 标记集 JSON
    #[serde(rename = "flags")]
    pub flags: String,
    #[serde(rename = "key_event_date")]
    pub key_event_date: String,
    #[serde(rename = "transaction_at")]
    pub transaction_at: i64,
    #[serde(rename = "created_at")]
    pub created_at: i64,
    #[serde(rename = "updated_at")]
    pub updated_at: i64,
}

/// 消费记录 ↔ 标签关联。表 `tbl_billadm_transaction_record_tag`（**无主键**，结构保持不变）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrTag {
    #[serde(rename = "ledger_id")]
    pub ledger_id: String,
    #[serde(rename = "transaction_id")]
    pub transaction_id: String,
    #[serde(rename = "tag")]
    pub tag: String,
}

/// 分类。表 `tbl_billadm_category`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Category {
    #[serde(rename = "ledger_id")]
    pub ledger_id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "transaction_type")]
    pub transaction_type: String,
    #[serde(rename = "sort_order")]
    pub sort_order: i32,
}

/// 标签。表 `tbl_billadm_tag`。`category_transaction_type` 形如 `餐饮美食:expense`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tag {
    #[serde(rename = "ledger_id")]
    pub ledger_id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "category_transaction_type")]
    pub category_transaction_type: String,
    #[serde(rename = "sort_order")]
    pub sort_order: i32,
}

/// 消费模板。表 `tbl_billadm_transaction_tpl`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TransactionTemplate {
    #[serde(rename = "template_id")]
    pub template_id: String,
    #[serde(rename = "ledger_id")]
    pub ledger_id: String,
    #[serde(rename = "template_name")]
    pub template_name: String,
    #[serde(rename = "transaction_type")]
    pub transaction_type: String,
    #[serde(rename = "category")]
    pub category: String,
    /// 标签集 JSON 数组字符串
    #[serde(rename = "tags")]
    pub tags: String,
    #[serde(rename = "flags")]
    pub flags: String,
    #[serde(rename = "description")]
    pub description: String,
    #[serde(rename = "sort_order")]
    pub sort_order: i32,
    #[serde(rename = "created_at")]
    pub created_at: i64,
    #[serde(rename = "updated_at")]
    pub updated_at: i64,
}

/// 图表曲线上的一个查询条件项。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueryConditionItem {
    #[serde(rename = "transactionType", default)]
    pub transaction_type: String,
    #[serde(rename = "category", default)]
    pub category: String,
    #[serde(rename = "tags", default)]
    pub tags: Vec<String>,
    #[serde(rename = "tagPolicy", default)]
    pub tag_policy: String,
    #[serde(rename = "tagNot", default)]
    pub tag_not: bool,
    #[serde(rename = "description", default)]
    pub description: String,
}

/// 图表曲线配置（持久化为 `chart.chart_lines` 的 JSON 元素）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChartLine {
    #[serde(rename = "label", default)]
    pub label: String,
    #[serde(rename = "transactionType", default)]
    pub transaction_type: String,
    #[serde(rename = "includeOutlier", default)]
    pub include_outlier: bool,
    #[serde(rename = "conditions", default)]
    pub conditions: Vec<QueryConditionItem>,
}

/// 图表配置。表 `tbl_billadm_chart`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Chart {
    #[serde(rename = "chart_id")]
    pub chart_id: String,
    #[serde(rename = "ledger_id")]
    pub ledger_id: String,
    #[serde(rename = "title")]
    pub title: String,
    /// 时间粒度 year / month
    #[serde(rename = "granularity")]
    pub granularity: String,
    /// 曲线配置 JSON
    #[serde(rename = "chart_lines")]
    pub chart_lines: String,
    /// 图表类型 line / bar
    #[serde(rename = "chart_type")]
    pub chart_type: String,
    #[serde(rename = "is_preset")]
    pub is_preset: bool,
    #[serde(rename = "sort_order")]
    pub sort_order: i32,
    #[serde(rename = "created_at")]
    pub created_at: i64,
    #[serde(rename = "updated_at")]
    pub updated_at: i64,
}

/// 关键事件（账本 + 日期唯一）。表 `tbl_billadm_key_event`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyEvent {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "date")]
    pub date: String,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "content")]
    pub content: String,
    #[serde(rename = "color")]
    pub color: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
}

/// 关键事件图片。表 `tbl_billadm_key_event_image`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyEventImage {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "eventDate")]
    pub event_date: String,
    /// 原图相对路径（相对 `data/assets`）
    #[serde(rename = "filePath")]
    pub file_path: String,
    /// 缩略图相对路径（相对 `data/assets`）
    #[serde(rename = "thumbPath")]
    pub thumb_path: String,
    #[serde(rename = "sortOrder")]
    pub sort_order: i32,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// 日记条目（**按账本隔离**，同账本内一天一篇）。表 `tbl_billadm_diary_entry`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiaryEntry {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "date")]
    pub date: String,
    #[serde(rename = "content")]
    pub content: String,
    /// 字数（Unicode 字符数，不是字节数）
    #[serde(rename = "wordCount")]
    pub word_count: i64,
    #[serde(rename = "mood")]
    pub mood: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
    /// 所属账本（日记的可见范围：只在本账本里出现）
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
}

/// 日记日期列表项（返回给前端构建树）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiaryDateItem {
    #[serde(rename = "date")]
    pub date: String,
    #[serde(rename = "wordCount")]
    pub word_count: i64,
    #[serde(rename = "mood")]
    pub mood: String,
}

impl From<DiaryEntry> for DiaryDateItem {
    fn from(entry: DiaryEntry) -> Self {
        Self {
            date: entry.date,
            word_count: entry.word_count,
            mood: entry.mood,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_json_uses_snake_case_like_go() {
        let value = serde_json::to_value(Ledger {
            id: "l1".into(),
            name: "默认账本".into(),
            description: String::new(),
            created_at: 1,
            updated_at: 2,
        })
        .unwrap();
        assert_eq!(value["id"], "l1");
        assert_eq!(value["created_at"], 1);
        assert_eq!(value["updated_at"], 2);
    }

    #[test]
    fn key_event_and_diary_json_use_camel_case_like_go() {
        let event = serde_json::to_value(KeyEvent {
            ledger_id: "l1".into(),
            created_at: 3,
            updated_at: 4,
            ..KeyEvent::default()
        })
        .unwrap();
        assert_eq!(event["ledgerId"], "l1");
        assert_eq!(event["createdAt"], 3);
        assert_eq!(event["updatedAt"], 4);

        let diary = serde_json::to_value(DiaryEntry {
            word_count: 7,
            ..DiaryEntry::default()
        })
        .unwrap();
        assert_eq!(diary["wordCount"], 7);
    }

    #[test]
    fn query_condition_item_roundtrips_camel_case() {
        let json = r#"{"transactionType":"expense","category":"餐饮美食","tags":["三餐"],
            "tagPolicy":"all","tagNot":false,"description":"午餐"}"#;
        let item: QueryConditionItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.transaction_type, "expense");
        assert_eq!(item.tags, vec!["三餐".to_string()]);
        assert_eq!(item.tag_policy, "all");
        assert_eq!(serde_json::to_value(&item).unwrap()["tagNot"], false);
    }
}
