//! IPC 入参契约锁定测试。
//!
//! 这一层是**契约面**：字段名一旦改动，界面（`tr-ui`）与测试都会静默失配——
//! serde 缺字段会退回默认值，于是查询条件被悄悄丢掉、返回"看起来正常"的错误数据。
//! 本项目已经踩过一次同类问题（记账页查询的 `Effect` 没用 tracked 读取，页面永远空白）。
//!
//! 所以这里用**原 HTTP 层真实接受过的 JSON**（取自 `fixtures/parity/go-driver.ps1`
//! 与 `kernel/api/*_controller.go` 的绑定字段）逐条断言反序列化结果，
//! 包括有意保留的命名不统一（camelCase 与 snake_case 并存）与别名兼容。
//!
//! 注意：凡是直接复用 `tr_domain::dto` 里类型的命令（`tr_create` / `tr_query` /
//! `tr_chart_data` / `diary_upsert` / `template_create` …）**不需要**在这里再锁一遍——
//! 那些类型界面与内核共用同一份定义，字段名不可能漂移。

use super::*;

#[test]
fn ledger_requests_use_the_go_bodies() {
    let list: ledger::LedgerListRequest = serde_json::from_str(r#"{"id":"all"}"#).unwrap();
    assert_eq!(list.id, "all");

    let create: ledger::CreateLedgerRequest =
        serde_json::from_str(r#"{"name":"默认账本","description":"种子数据"}"#).unwrap();
    assert_eq!(create.name.as_deref(), Some("默认账本"));
    assert_eq!(create.description.as_deref(), Some("种子数据"));

    // PATCH /ledgers/:id —— 路径参数并入请求体；name 缺失时命令层报原实现的文案
    let update: ledger::UpdateLedgerRequest =
        serde_json::from_str(r#"{"id":"l1","name":"改名","description":""}"#).unwrap();
    assert_eq!(update.id, "l1");
    assert_eq!(update.name.as_deref(), Some("改名"));
    let missing_name: ledger::UpdateLedgerRequest = serde_json::from_str(r#"{"id":"l1"}"#).unwrap();
    assert!(missing_name.name.is_none());

    let get: ledger::LedgerIdRequest = serde_json::from_str(r#"{"id":"l1"}"#).unwrap();
    assert_eq!(get.id, "l1");

    // 缺参等价于原实现的零值语义（不是错误）
    let empty: ledger::LedgerListRequest = serde_json::from_str("{}").unwrap();
    assert_eq!(empty.id, "");
}

#[test]
fn category_and_tag_requests_keep_camel_case_query_params() {
    // GET /categories?type=&ledgerId= —— 查询参数沿用驼峰；`transactionType` 也接受
    let list: category::CategoryListRequest =
        serde_json::from_str(r#"{"type":"expense","ledgerId":"l1"}"#).unwrap();
    assert_eq!(list.transaction_type, "expense");
    assert_eq!(list.ledger_id, "l1");

    let aliased: category::CategoryListRequest =
        serde_json::from_str(r#"{"transactionType":"income","ledgerId":"l1"}"#).unwrap();
    assert_eq!(aliased.transaction_type, "income");

    // DELETE /categories/:name?type=&ledgerId=
    let delete: category::CategoryDeleteRequest =
        serde_json::from_str(r#"{"name":"餐饮美食","type":"expense","ledgerId":"l1"}"#).unwrap();
    assert_eq!(delete.name, "餐饮美食");

    let init: category::InitializeCategoriesRequest =
        serde_json::from_str(r#"{"ledgerId":"l1"}"#).unwrap();
    assert_eq!(init.ledger_id, "l1");

    // GET /tags?categoryTransactionType=&ledgerId=
    let tags: tag::TagListRequest =
        serde_json::from_str(r#"{"categoryTransactionType":"expense","ledgerId":"l1"}"#).unwrap();
    assert_eq!(tags.category_transaction_type, "expense");

    let tag_delete: tag::TagDeleteRequest = serde_json::from_str(
        r#"{"name":"三餐","categoryTransactionType":"expense","ledgerId":"l1"}"#,
    )
    .unwrap();
    assert_eq!(tag_delete.name, "三餐");
}

#[test]
fn template_and_chart_requests_accept_both_path_and_dto_names() {
    let list: template::TemplateListRequest = serde_json::from_str(r#"{"ledgerId":"l1"}"#).unwrap();
    assert_eq!(list.ledger_id, "l1");

    // 路径参数 `:id` 与 DTO 字段 `templateId` 都接受
    let by_path: template::TemplateIdRequest = serde_json::from_str(r#"{"id":"t1"}"#).unwrap();
    let by_dto: template::TemplateIdRequest =
        serde_json::from_str(r#"{"templateId":"t1"}"#).unwrap();
    assert_eq!(by_path.id, by_dto.id);

    let sort: template::TemplateSortRequest =
        serde_json::from_str(r#"{"id":"t1","ledgerId":"l1","sortOrder":3}"#).unwrap();
    assert_eq!(sort.sort_order, 3);

    let chart_list: chart::ChartListRequest = serde_json::from_str(r#"{"ledgerId":"l1"}"#).unwrap();
    assert_eq!(chart_list.ledger_id, "l1");

    // 图表：DTO 里叫 chartId，路径参数叫 id
    let chart_by_dto: chart::ChartIdRequest = serde_json::from_str(r#"{"chartId":"c1"}"#).unwrap();
    let chart_by_path: chart::ChartIdRequest = serde_json::from_str(r#"{"id":"c1"}"#).unwrap();
    assert_eq!(chart_by_dto.chart_id, chart_by_path.chart_id);
}

#[test]
fn key_event_requests_mix_snake_case_body_with_dto_naming() {
    // 响应 DTO 是 camelCase（`ledgerId`/`createdAt`），但**请求体**沿用 snake_case
    let year: key_event::YearRequest =
        serde_json::from_str(r#"{"year":"2026","ledger_id":"l1"}"#).unwrap();
    assert_eq!(year.year, "2026");

    let date: key_event::KeyEventDateRequest =
        serde_json::from_str(r#"{"date":"2026-02-10","ledger_id":"l1"}"#).unwrap();
    assert_eq!(date.date, "2026-02-10");

    let upsert: key_event::KeyEventUpsertRequest = serde_json::from_str(
        r##"{"ledger_id":"l1","date":"2026-02-10","title":"买了新耳机",
             "content":"# 记录","color":"outlier"}"##,
    )
    .unwrap();
    assert_eq!(upsert.title.as_deref(), Some("买了新耳机"));
    assert_eq!(upsert.color.as_deref(), Some("outlier"));

    // 三个可选字段都可省略（原实现从 map 取值，缺参不报错）
    let minimal: key_event::KeyEventUpsertRequest =
        serde_json::from_str(r#"{"ledger_id":"l1","date":"2026-02-10"}"#).unwrap();
    assert!(minimal.title.is_none() && minimal.content.is_none() && minimal.color.is_none());

    // 图片：`data` 是 base64／dataURL 字符串，可省略
    let image: key_event::KeyEventImageAddRequest =
        serde_json::from_str(r#"{"date":"2026-02-10","ledger_id":"l1","data":"AAAA"}"#).unwrap();
    assert_eq!(image.data.as_deref(), Some("AAAA"));

    let image_id: key_event::KeyEventImageIdRequest =
        serde_json::from_str(r#"{"id":"i1"}"#).unwrap();
    assert_eq!(image_id.id, "i1");
}

#[test]
fn diary_and_transaction_link_requests_are_snake_case() {
    // 日期是路径参数，并入请求体
    let date: diary::DiaryDateRequest = serde_json::from_str(r#"{"date":"2026-02-10"}"#).unwrap();
    assert_eq!(date.date, "2026-02-10");

    let scan: diary::DiaryScanRequest =
        serde_json::from_str(r#"{"directory":"D:\\diary"}"#).unwrap();
    assert_eq!(scan.directory, r"D:\diary");

    let import: diary::DiaryImportFileRequest =
        serde_json::from_str(r#"{"path":"D:\\diary\\a.md","date":"2026-02-10"}"#).unwrap();
    assert_eq!(import.date, "2026-02-10");

    // POST /transactions/link —— 原实现从 map 里取 `transaction_id`（不是 transactionId）
    let link: tr::LinkRequest =
        serde_json::from_str(r#"{"transaction_id":"x1","date":"2026-02-10"}"#).unwrap();
    assert_eq!(link.transaction_id, "x1");

    let unlink: tr::UnlinkRequest = serde_json::from_str(r#"{"transaction_id":"x1"}"#).unwrap();
    assert_eq!(unlink.transaction_id, "x1");

    // GET /transactions/linked/:date?ledger_id=
    let linked: tr::LinkedByDateRequest =
        serde_json::from_str(r#"{"date":"2026-02-10","ledger_id":"l1"}"#).unwrap();
    assert_eq!(linked.ledger_id, "l1");

    let delete: tr::TransactionIdRequest = serde_json::from_str(r#"{"id":"x1"}"#).unwrap();
    assert_eq!(delete.id, "x1");
}

#[test]
fn empty_request_accepts_an_empty_object() {
    // 无参命令（列表类）仍然收一个结构体参数，界面会传 `{}`
    let request: EmptyRequest = serde_json::from_str("{}").unwrap();
    let _ = request;
}
