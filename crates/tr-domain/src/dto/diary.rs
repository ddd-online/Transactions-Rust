//! 日记相关 DTO。对照 Go `kernel/service/diary_service.go` 里随服务定义的结构，
//! 以及 `kernel/api/diary_controller.go` 的响应包装。

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

/// 扫描结果。原控制器返回的是 `{"files": [...]}`。
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

/// `PUT /diary/:date` 请求体：日期来自路径，正文与心情来自 body。
/// 原实现从 `map[string]any` 取值，缺失字段按空串处理，因此这里用 `Option`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiaryUpsertRequest {
    #[serde(rename = "date")]
    pub date: String,
    #[serde(rename = "content")]
    pub content: Option<String>,
    #[serde(rename = "mood")]
    pub mood: Option<String>,
}

/// `POST /diary/export` 请求体：`year`/`month` 缺省为 0（表示不限）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiaryExportRequest {
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
    fn scan_response_matches_go_shape() {
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
    fn export_result_matches_go_shape() {
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
            serde_json::from_str(r#"{"directory":"D:\\out"}"#).unwrap();
        assert_eq!(request.year.unwrap_or(0), 0);
        assert_eq!(request.month.unwrap_or(0), 0);
    }
}
