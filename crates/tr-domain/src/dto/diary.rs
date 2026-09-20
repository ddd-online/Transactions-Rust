//! 日记相关 DTO：导入扫描、导出结果与写入请求的结构定义。

use serde::{Deserialize, Serialize};

/// 一个待导入的日记文件（`POST /diary/import/scan` 的条目）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiaryFileItem {
    /// YYYY-MM-DD
    #[serde(rename = "date")]
    pub date: String,
    /// 文件绝对路径
    #[serde(rename = "path")]
    pub path: String,
}

/// 扫描结果，形状为 `{"files": [...]}`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiaryScanResponse {
    #[serde(rename = "files")]
    pub files: Vec<DiaryFileItem>,
}

/// 导出失败的单个文件。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiaryExportFileError {
    #[serde(rename = "date")]
    pub date: String,
    #[serde(rename = "error")]
    pub error: String,
}

/// 导出结果汇总。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiaryExportResult {
    #[serde(rename = "total")]
    pub total: i32,
    #[serde(rename = "success")]
    pub success: i32,
    #[serde(rename = "failed")]
    pub failed: Vec<DiaryExportFileError>,
}

/// 日记写入请求体：账本、日期、正文与心情。
/// 缺失字段按空串处理，因此这里用 `Option`；`ledger_id` 缺席时由 IPC 层拒绝（`ledger_id is required`）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiaryUpsertRequest {
    #[serde(rename = "ledger_id")]
    pub ledger_id: String,
    #[serde(rename = "date")]
    pub date: String,
    #[serde(rename = "content")]
    pub content: Option<String>,
    #[serde(rename = "mood")]
    pub mood: Option<String>,
}

/// `POST /diary/export` 请求体：`year`/`month` 缺省为 0（表示不限）。
///
/// 导出**只导 `ledger_id` 这一个账本**的日记（与导入对称）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiaryExportRequest {
    #[serde(rename = "ledger_id")]
    pub ledger_id: String,
    #[serde(rename = "directory")]
    pub directory: String,
    #[serde(rename = "year")]
    pub year: Option<i64>,
    #[serde(rename = "month")]
    pub month: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_response_json_shape() {
        let value = serde_json::to_value(DiaryScanResponse {
            files: vec![DiaryFileItem {
                date: "2026-01-01".into(),
                path: "D:\\diary\\2026-01-01.md".into(),
            }],
        })
        .unwrap();
        assert_eq!(value["files"][0]["date"], "2026-01-01");
        assert_eq!(value["files"][0]["path"], "D:\\diary\\2026-01-01.md");
    }

    #[test]
    fn export_result_json_shape() {
        let value = serde_json::to_value(DiaryExportResult {
            total: 3,
            success: 2,
            failed: vec![DiaryExportFileError {
                date: "2026-01-03".into(),
                error: "磁盘已满".into(),
            }],
        })
        .unwrap();
        assert_eq!(value["total"], 3);
        assert_eq!(value["success"], 2);
        assert_eq!(value["failed"][0]["error"], "磁盘已满");
    }

    #[test]
    fn export_request_defaults_year_and_month_to_zero() {
        let request: DiaryExportRequest =
            serde_json::from_str(r#"{"ledger_id":"l1","directory":"D:\\out"}"#).unwrap();
        assert_eq!(request.ledger_id, "l1");
        assert_eq!(request.year.unwrap_or(0), 0);
        assert_eq!(request.month.unwrap_or(0), 0);
    }

    #[test]
    fn diary_requests_read_ledger_id_and_tolerate_missing_it() {
        let upsert: DiaryUpsertRequest =
            serde_json::from_str(r#"{"ledger_id":"l1","date":"2026-02-10","content":"正文"}"#)
                .unwrap();
        assert_eq!(upsert.ledger_id, "l1");
        assert_eq!(upsert.date, "2026-02-10");

        // 老客户端不带 ledger_id：字段退化成空串，由 IPC 层的 `require_ledger_id` 拦下
        let legacy: DiaryUpsertRequest = serde_json::from_str(r#"{"date":"2026-02-10"}"#).unwrap();
        assert!(legacy.ledger_id.is_empty());

        let export: DiaryExportRequest =
            serde_json::from_str(r#"{"ledger_id":"l2","directory":"D:\\out","year":2026}"#)
                .unwrap();
        assert_eq!(export.ledger_id, "l2");
        assert_eq!(export.year, Some(2026));
    }
}
