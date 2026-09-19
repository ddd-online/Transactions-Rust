//! 核心业务 DTO（账本 / 消费记录 / 分类 / 标签 / 模板 / 图表 / 查询条件）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::consts;
use crate::error::AppError;
use crate::models::{
    Category, Chart, ChartLine, Ledger, Tag, TransactionRecord, TransactionRecordFlags,
    TransactionTemplate,
};

/// 查询条件项：只保留一份定义并通过 `dto` 与 `models` 一并导出，避免两处漂移
/// （`dto.QueryConditionItem` 与 `models.QueryConditionItem` 解析到同一类型）。
pub use crate::models::QueryConditionItem;

// ------------------------------------------------------------------ 账本

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LedgerDto {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "description")]
    pub description: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

impl From<&Ledger> for LedgerDto {
    fn from(ledger: &Ledger) -> Self {
        Self {
            id: ledger.id.clone(),
            name: ledger.name.clone(),
            description: ledger.description.clone(),
            created_at: ledger.created_at,
            updated_at: ledger.updated_at,
        }
    }
}

// ------------------------------------------------------------------ 分类

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CategoryDto {
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "transactionType")]
    pub transaction_type: String,
    #[serde(rename = "sortOrder")]
    pub sort_order: i32,
    /// 该分类下的记录数（由服务层批量统计后填充）
    #[serde(rename = "recordCount")]
    pub record_count: i32,
}

impl From<&Category> for CategoryDto {
    fn from(category: &Category) -> Self {
        Self {
            ledger_id: category.ledger_id.clone(),
            name: category.name.clone(),
            transaction_type: category.transaction_type.clone(),
            sort_order: category.sort_order,
            record_count: 0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CreateCategoryRequest {
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "transactionType")]
    pub transaction_type: String,
    #[serde(rename = "sortOrder")]
    pub sort_order: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateCategorySortRequest {
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "transactionType")]
    pub transaction_type: String,
    #[serde(rename = "sortOrder")]
    pub sort_order: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InitializeCategoriesResponse {
    #[serde(rename = "categories")]
    pub categories: i32,
    #[serde(rename = "tags")]
    pub tags: i32,
}

// ------------------------------------------------------------------ 标签

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TagDto {
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "categoryTransactionType")]
    pub category_transaction_type: String,
    #[serde(rename = "sortOrder")]
    pub sort_order: i32,
    #[serde(rename = "recordCount")]
    pub record_count: i32,
}

impl From<&Tag> for TagDto {
    fn from(tag: &Tag) -> Self {
        Self {
            ledger_id: tag.ledger_id.clone(),
            name: tag.name.clone(),
            category_transaction_type: tag.category_transaction_type.clone(),
            sort_order: tag.sort_order,
            record_count: 0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CreateTagRequest {
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "categoryTransactionType")]
    pub category_transaction_type: String,
    #[serde(rename = "sortOrder")]
    pub sort_order: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateTagSortRequest {
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "categoryTransactionType")]
    pub category_transaction_type: String,
    #[serde(rename = "sortOrder")]
    pub sort_order: i32,
}

// ------------------------------------------------------------------ 图表

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChartDto {
    #[serde(rename = "chartId")]
    pub chart_id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "granularity")]
    pub granularity: String,
    #[serde(rename = "lines")]
    pub lines: Vec<ChartLine>,
    #[serde(rename = "chartType")]
    pub chart_type: String,
    #[serde(rename = "isPreset")]
    pub is_preset: bool,
    #[serde(rename = "sortOrder")]
    pub sort_order: i32,
}

impl From<&Chart> for ChartDto {
    fn from(chart: &Chart) -> Self {
        Self {
            chart_id: chart.chart_id.clone(),
            ledger_id: chart.ledger_id.clone(),
            title: chart.title.clone(),
            granularity: chart.granularity.clone(),
            // chart_lines 为 JSON 文本；解析失败时返回空曲线列表
            lines: serde_json::from_str(&chart.chart_lines).unwrap_or_default(),
            chart_type: chart.chart_type.clone(),
            is_preset: chart.is_preset,
            sort_order: chart.sort_order,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CreateChartRequest {
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "granularity")]
    pub granularity: String,
    #[serde(rename = "lines")]
    pub lines: Vec<ChartLine>,
    #[serde(rename = "chartType")]
    pub chart_type: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateChartRequest {
    #[serde(rename = "chartId")]
    pub chart_id: String,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "granularity")]
    pub granularity: String,
    #[serde(rename = "lines")]
    pub lines: Vec<ChartLine>,
    #[serde(rename = "chartType")]
    pub chart_type: String,
    #[serde(rename = "sortOrder")]
    pub sort_order: i32,
}

/// `chart_lines` 列 ↔ `Vec<ChartLine>` 的 JSON 互转。
///
/// 放在这里而不是 `tr-service`：该 crate 没有 serde_json 依赖，而 JSON 文本与 DTO 的互转
/// 在本 crate 里已有先例（模板 `tags`、记录 `flags`）。
/// 服务层只负责把失败包成固定文案。
///
/// 字段顺序与紧凑格式必须稳定：预设图表的 `chart_lines` 逐字存入数据库，格式一变即为数据变更。
pub fn encode_chart_lines(lines: &[ChartLine]) -> Result<String, String> {
    serde_json::to_string(lines).map_err(|error| error.to_string())
}

/// [`encode_chart_lines`] 的反向操作。
pub fn decode_chart_lines(text: &str) -> Result<Vec<ChartLine>, String> {
    serde_json::from_str(text).map_err(|error| error.to_string())
}

/// 单条曲线的查询条件。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChartLineCondition {
    #[serde(rename = "label")]
    pub label: String,
    #[serde(rename = "transactionType")]
    pub transaction_type: String,
    #[serde(rename = "includeOutlier")]
    pub include_outlier: bool,
    #[serde(rename = "conditions")]
    pub conditions: Vec<QueryConditionItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChartQueryRequest {
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "tsRange")]
    pub ts_range: Vec<i64>,
    /// "year" 或 "month"
    #[serde(rename = "granularity")]
    pub granularity: String,
    #[serde(rename = "lines")]
    pub lines: Vec<ChartLineCondition>,
}

/// 聚合后的单个时间序列点。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChartPoint {
    #[serde(rename = "time")]
    pub time: String,
    #[serde(rename = "amount")]
    pub amount: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChartLineData {
    #[serde(rename = "label")]
    pub label: String,
    #[serde(rename = "type")]
    pub line_type: String,
    #[serde(rename = "data")]
    pub data: Vec<ChartPoint>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChartQueryResponse {
    #[serde(rename = "lines")]
    pub lines: Vec<ChartLineData>,
    /// 交易类型 → 金额合计
    #[serde(rename = "statistics")]
    pub statistics: BTreeMap<String, i64>,
}

// ------------------------------------------------------------------ 消费记录

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TransactionRecordDto {
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "transactionId")]
    pub transaction_id: String,
    #[serde(rename = "price")]
    pub price: i64,
    #[serde(rename = "transactionType")]
    pub transaction_type: String,
    #[serde(rename = "category")]
    pub category: String,
    #[serde(rename = "description")]
    pub description: String,
    #[serde(rename = "tags")]
    pub tags: Vec<String>,
    #[serde(rename = "transactionAt")]
    pub transaction_at: i64,
    #[serde(rename = "outlier")]
    pub outlier: bool,
    #[serde(rename = "keyEventDate")]
    pub key_event_date: String,
}

impl TransactionRecordDto {
    /// 校验文案（用户可见，改动即破坏契约）。
    pub fn validate(&self) -> Result<(), AppError> {
        if self.ledger_id.trim().is_empty() {
            return Err(AppError::bad_request("LedgerID is empty"));
        }
        if !consts::TRANSACTION_TYPES.contains(&self.transaction_type.as_str()) {
            return Err(AppError::bad_request(format!(
                "invalid TransactionType: {}",
                self.transaction_type
            )));
        }
        Ok(())
    }

    /// 转模型：`outlier` 打包进 `flags` JSON。
    ///
    /// 注意：**不**写入 `key_event_date`——新建记录一律未关联关键事件，
    /// 关联关键事件只能通过 `tr_link` 命令进行（前端即使传了 keyEventDate 也会被忽略）。
    pub fn to_record(&self) -> TransactionRecord {
        let flags = serde_json::to_string(&TransactionRecordFlags {
            outlier: self.outlier,
        })
        .unwrap_or_else(|_| "{}".to_string());
        TransactionRecord {
            transaction_id: self.transaction_id.clone(),
            ledger_id: self.ledger_id.clone(),
            price: self.price,
            transaction_type: self.transaction_type.clone(),
            category: self.category.clone(),
            description: self.description.clone(),
            flags,
            key_event_date: String::new(),
            transaction_at: self.transaction_at,
            created_at: 0,
            updated_at: 0,
        }
    }

    /// 由模型填充；`tags` 由调用方另行补齐（默认空数组而非 null）。
    pub fn from_record(record: &TransactionRecord) -> Self {
        let outlier = serde_json::from_str::<TransactionRecordFlags>(&record.flags)
            .map(|flags| flags.outlier)
            .unwrap_or(false);
        Self {
            ledger_id: record.ledger_id.clone(),
            transaction_id: record.transaction_id.clone(),
            price: record.price,
            transaction_type: record.transaction_type.clone(),
            category: record.category.clone(),
            description: record.description.clone(),
            tags: Vec::new(),
            transaction_at: record.transaction_at,
            outlier,
            key_event_date: record.key_event_date.clone(),
        }
    }
}

/// 消费记录查询条件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrQueryCondition {
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    /// 默认 -1（表示不分页）
    pub offset: i64,
    pub limit: i64,
    #[serde(rename = "tsRange")]
    pub ts_range: Vec<i64>,
    pub items: Vec<QueryConditionItem>,
    #[serde(rename = "sortFields")]
    pub sort_fields: Vec<QueryConditionSortField>,
}

impl Default for TrQueryCondition {
    fn default() -> Self {
        Self {
            ledger_id: String::new(),
            offset: -1,
            limit: -1,
            ts_range: Vec::new(),
            items: Vec::new(),
            sort_fields: Vec::new(),
        }
    }
}

impl TrQueryCondition {
    pub fn validate(&self) -> Result<(), AppError> {
        if self.ledger_id.is_empty() {
            return Err(AppError::bad_request(format!(
                "账本Id不可为空: {}",
                self.ledger_id
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueryConditionSortField {
    #[serde(rename = "field")]
    pub field: String,
    #[serde(rename = "order")]
    pub order: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrQueryResult {
    #[serde(rename = "items")]
    pub items: Vec<TransactionRecordDto>,
    #[serde(rename = "total")]
    pub total: i64,
    #[serde(rename = "page")]
    pub page: i32,
    #[serde(rename = "page_size")]
    pub page_size: i32,
    #[serde(rename = "total_pages")]
    pub total_pages: i32,
    #[serde(rename = "trStatistics")]
    pub tr_statistics: BTreeMap<String, i64>,
}

// ------------------------------------------------------------------ 模板

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TransactionTemplateDto {
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
    #[serde(rename = "tags")]
    pub tags: Vec<String>,
    #[serde(rename = "flags")]
    pub flags: String,
    #[serde(rename = "description")]
    pub description: String,
    #[serde(rename = "sort_order")]
    pub sort_order: i32,
}

impl TransactionTemplateDto {
    pub fn validate(&self) -> Result<(), AppError> {
        if self.template_name.is_empty() {
            return Err(AppError::bad_request("模板名称不能为空"));
        }
        if !consts::TRANSACTION_TYPES.contains(&self.transaction_type.as_str()) {
            return Err(AppError::bad_request(format!(
                "invalid transaction type: {}",
                self.transaction_type
            )));
        }
        if self.category.is_empty() {
            return Err(AppError::bad_request("分类不能为空"));
        }
        Ok(())
    }

    pub fn to_template(&self) -> TransactionTemplate {
        TransactionTemplate {
            template_id: self.template_id.clone(),
            ledger_id: self.ledger_id.clone(),
            template_name: self.template_name.clone(),
            transaction_type: self.transaction_type.clone(),
            category: self.category.clone(),
            tags: serde_json::to_string(&self.tags).unwrap_or_else(|_| "[]".to_string()),
            flags: self.flags.clone(),
            description: self.description.clone(),
            sort_order: self.sort_order,
            created_at: 0,
            updated_at: 0,
        }
    }

    pub fn from_template(template: &TransactionTemplate) -> Self {
        Self {
            template_id: template.template_id.clone(),
            ledger_id: template.ledger_id.clone(),
            template_name: template.template_name.clone(),
            transaction_type: template.transaction_type.clone(),
            category: template.category.clone(),
            tags: serde_json::from_str(&template.tags).unwrap_or_default(),
            flags: template.flags.clone(),
            description: template.description.clone(),
            sort_order: template.sort_order,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_dto_uses_camel_case() {
        let dto = LedgerDto::from(&Ledger {
            id: "l1".into(),
            created_at: 5,
            ..Ledger::default()
        });
        let value = serde_json::to_value(dto).unwrap();
        assert_eq!(value["createdAt"], 5);
        assert_eq!(value["updatedAt"], 0);
    }

    #[test]
    fn template_dto_stays_snake_case() {
        let dto = TransactionTemplateDto {
            template_id: "t1".into(),
            sort_order: 3,
            ..TransactionTemplateDto::default()
        };
        let value = serde_json::to_value(dto).unwrap();
        assert_eq!(value["template_id"], "t1");
        assert_eq!(value["sort_order"], 3);
    }

    #[test]
    fn tr_query_condition_documented_defaults() {
        let condition = TrQueryCondition::default();
        assert_eq!(condition.offset, -1);
        assert_eq!(condition.limit, -1);
        let json = r#"{"ledgerId":"l1"}"#;
        let parsed: TrQueryCondition = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.ledger_id, "l1");
        assert_eq!(parsed.offset, -1);
        assert_eq!(parsed.limit, -1);
        parsed.validate().unwrap();
    }

    #[test]
    fn validation_messages_are_stable() {
        let dto = TransactionRecordDto::default();
        assert_eq!(dto.validate().unwrap_err().msg, "LedgerID is empty");

        let dto = TransactionRecordDto {
            ledger_id: "l1".into(),
            transaction_type: "unknown".into(),
            ..TransactionRecordDto::default()
        };
        assert_eq!(
            dto.validate().unwrap_err().msg,
            "invalid TransactionType: unknown"
        );

        let template = TransactionTemplateDto::default();
        assert_eq!(template.validate().unwrap_err().msg, "模板名称不能为空");

        let template = TransactionTemplateDto {
            template_name: "模板".into(),
            transaction_type: "bad".into(),
            ..TransactionTemplateDto::default()
        };
        assert_eq!(
            template.validate().unwrap_err().msg,
            "invalid transaction type: bad"
        );

        let template = TransactionTemplateDto {
            template_name: "模板".into(),
            transaction_type: "expense".into(),
            ..TransactionTemplateDto::default()
        };
        assert_eq!(template.validate().unwrap_err().msg, "分类不能为空");
    }

    #[test]
    fn outlier_flag_roundtrips_through_flags_json() {
        let original = TransactionRecordDto {
            ledger_id: "l1".into(),
            transaction_type: "expense".into(),
            outlier: true,
            ..TransactionRecordDto::default()
        };
        let record = original.to_record();
        assert_eq!(record.flags, r#"{"outlier":true}"#);
        let restored = TransactionRecordDto::from_record(&record);
        assert!(restored.outlier);
    }

    #[test]
    fn create_ignores_key_event_date() {
        // to_record 不写 key_event_date：新建记录一律未关联关键事件，
        // 关联只能走 tr_link。若这里被"顺手补上"，会破坏这个约定。
        let dto = TransactionRecordDto {
            ledger_id: "l1".into(),
            transaction_type: "expense".into(),
            key_event_date: "2026-01-01".into(),
            ..TransactionRecordDto::default()
        };
        assert_eq!(dto.to_record().key_event_date, "");

        // 但读取时要能带出关联日期
        let record = TransactionRecord {
            key_event_date: "2026-01-01".into(),
            ..TransactionRecord::default()
        };
        assert_eq!(
            TransactionRecordDto::from_record(&record).key_event_date,
            "2026-01-01"
        );
    }

    #[test]
    fn chart_dto_parses_lines_json() {
        let chart = Chart {
            chart_lines: r#"[{"label":"支出","transactionType":"expense",
                "includeOutlier":false,"conditions":[]}]"#
                .into(),
            ..Chart::default()
        };
        let dto = ChartDto::from(&chart);
        assert_eq!(dto.lines.len(), 1);
        assert_eq!(dto.lines[0].label, "支出");
        assert_eq!(dto.lines[0].transaction_type, "expense");
    }

    #[test]
    fn tr_query_result_keeps_mixed_naming() {
        let value = serde_json::to_value(TrQueryResult::default()).unwrap();
        assert!(value.get("total_pages").is_some());
        assert!(value.get("trStatistics").is_some());
    }

    #[test]
    fn template_tags_roundtrip_as_json_array() {
        let dto = TransactionTemplateDto {
            tags: vec!["三餐".into(), "外卖".into()],
            ..TransactionTemplateDto::default()
        };
        let template = dto.to_template();
        assert_eq!(template.tags, r#"["三餐","外卖"]"#);
        let restored = TransactionTemplateDto::from_template(&template);
        assert_eq!(restored.tags, vec!["三餐".to_string(), "外卖".to_string()]);
    }
}
