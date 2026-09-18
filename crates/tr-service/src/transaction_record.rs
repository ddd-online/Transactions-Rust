//! 消费记录服务。对照 Go `kernel/service/transaction_record_service.go`。
//!
//! 几个容易踩空、但都按原实现保留的细节：
//! * **统计口径**：`items`（筛选条件）不影响 `trStatistics`——它只按「账本 + 时间范围」汇总
//! * **分页**：`limit <= 0` 时 `page_size` 取实际返回条数；`page` 仅在 `limit > 0 && offset >= 0` 时推导
//! * **图表**：先按曲线查桶，再在**全部曲线的最小/最大桶**之间补零生成连续时间轴
//! * **关联关键事件**：目标日期没有关键事件时自动创建一条空事件，并写一条 info 日志
//! * **新建记录不写 `key_event_date`**（关联只能走 link 命令）

use std::collections::BTreeMap;

use tr_domain::consts;
use tr_domain::dto::{
    ChartLineData, ChartQueryRequest, ChartQueryResponse, TrQueryCondition, TrQueryResult,
    TransactionRecordDto,
};
use tr_domain::models::TrTag;
use tr_store::dao::is_not_found;
use tr_store::dao::key_event::KeyEventDao;
use tr_store::dao::transaction_record::TransactionRecordDao;
use tr_store::dao::transaction_record_tag::TrTagDao;
use tr_store::Workspace;

use crate::{ServiceError, ServiceResult};

/// 新建一条消费记录，返回新记录 ID。
pub fn create_tr(workspace: &Workspace, dto: &TransactionRecordDto) -> ServiceResult<String> {
    let transaction_id = tr_store::util::new_uuid();
    let mut record = dto.to_record();
    record.transaction_id = transaction_id.clone();

    let tags: Vec<TrTag> = dto
        .tags
        .iter()
        .map(|tag| TrTag {
            ledger_id: dto.ledger_id.clone(),
            transaction_id: transaction_id.clone(),
            tag: tag.clone(),
        })
        .collect();

    workspace
        .transaction(|conn| {
            TransactionRecordDao::create(conn, &record).map_err(|error| {
                ServiceError::Internal(format!("create transaction record: {error}"))
            })?;
            TrTagDao::create_batch(conn, &tags)
                .map_err(|error| ServiceError::Internal(format!("create tr tags: {error}")))?;
            Ok(())
        })
        .map_err(|error: ServiceError| {
            tracing::error!("创建交易记录失败: {}", error);
            error
        })?;

    Ok(transaction_id)
}

/// 批量新建，返回成功创建条数。
pub fn batch_create_tr(workspace: &Workspace, dtos: &[TransactionRecordDto]) -> ServiceResult<i32> {
    tracing::info!("开始批量创建 {} 条交易记录", dtos.len());

    if dtos.is_empty() {
        return Ok(0);
    }

    let mut records = Vec::with_capacity(dtos.len());
    let mut tags: Vec<TrTag> = Vec::new();
    for dto in dtos {
        let transaction_id = tr_store::util::new_uuid();
        let mut record = dto.to_record();
        record.transaction_id = transaction_id.clone();
        records.push(record);

        for tag in &dto.tags {
            tags.push(TrTag {
                ledger_id: dto.ledger_id.clone(),
                transaction_id: transaction_id.clone(),
                tag: tag.clone(),
            });
        }
    }

    workspace
        .transaction(|conn| {
            TransactionRecordDao::create_batch(conn, &records).map_err(|error| {
                tracing::error!("批量创建: 创建交易记录失败: {}", error);
                ServiceError::Internal(format!("create transaction records: {error}"))
            })?;
            if !tags.is_empty() {
                TrTagDao::create_batch(conn, &tags).map_err(|error| {
                    tracing::error!("批量创建: 创建标签关联失败: {}", error);
                    ServiceError::Internal(format!("create tr tags: {error}"))
                })?;
            }
            Ok(())
        })
        .map_err(|error: ServiceError| {
            tracing::error!("批量创建交易记录失败: {}", error);
            error
        })?;

    tracing::info!("批量创建交易记录成功, 数量: {}", dtos.len());
    Ok(dtos.len() as i32)
}

/// 条件查询：筛选 + 排序 + 分页 + 标签补齐 + 范围统计。
pub fn query_trs_on_condition(
    workspace: &Workspace,
    condition: &TrQueryCondition,
) -> ServiceResult<TrQueryResult> {
    let result = TransactionRecordDao::query_filtered(&workspace.connection(), condition)?;

    let transaction_ids: Vec<String> = result
        .items
        .iter()
        .map(|record| record.transaction_id.clone())
        .collect();
    let tag_map = TrTagDao::query_by_tr_ids(&workspace.connection(), &transaction_ids)?;

    let mut items = Vec::with_capacity(result.items.len());
    for record in &result.items {
        let mut dto = TransactionRecordDto::from_record(record);
        if let Some(tags) = tag_map.get(&record.transaction_id) {
            for tag in tags {
                dto.tags.push(tag.tag.clone());
            }
        }
        items.push(dto);
    }

    // 分页推导（与原实现逐行等价）
    let page_size = if condition.limit <= 0 {
        items.len() as i32
    } else {
        condition.limit as i32
    };
    let total_pages = if page_size > 0 {
        let total = result.total as i32;
        if total % page_size != 0 {
            total / page_size + 1
        } else {
            total / page_size
        }
    } else {
        0
    };
    let page = if condition.limit > 0 && condition.offset >= 0 {
        (condition.offset / condition.limit + 1) as i32
    } else {
        1
    };

    let mut tr_statistics = BTreeMap::new();
    tr_statistics.insert(
        consts::TRANSACTION_TYPE_INCOME.to_string(),
        result.statistics.income,
    );
    tr_statistics.insert(
        consts::TRANSACTION_TYPE_EXPENSE.to_string(),
        result.statistics.expense,
    );
    tr_statistics.insert(
        consts::TRANSACTION_TYPE_TRANSFER.to_string(),
        result.statistics.transfer,
    );

    Ok(TrQueryResult {
        items,
        total: result.total,
        page,
        page_size,
        total_pages,
        tr_statistics,
    })
}

/// 数据分析页：逐曲线分桶聚合 + 连续时间轴补零 + 范围统计。
pub fn query_trs_for_chart(
    workspace: &Workspace,
    request: &ChartQueryRequest,
) -> ServiceResult<ChartQueryResponse> {
    let conn = workspace.connection();

    let mut line_points = Vec::with_capacity(request.lines.len());
    let mut min_bucket = String::new();
    let mut max_bucket = String::new();

    for line in &request.lines {
        let points = TransactionRecordDao::query_chart_line_data(
            &conn,
            &request.ledger_id,
            &request.ts_range,
            &request.granularity,
            line,
        )
        .map_err(|error| {
            ServiceError::Internal(format!("query chart line {:?}: {error}", line.label))
        })?;

        for point in &points {
            if min_bucket.is_empty() || point.time < min_bucket {
                min_bucket = point.time.clone();
            }
            if max_bucket.is_empty() || point.time > max_bucket {
                max_bucket = point.time.clone();
            }
        }
        line_points.push(points);
    }

    let statistics =
        TransactionRecordDao::query_statistics(&conn, &request.ledger_id, &request.ts_range)
            .map_err(|error| ServiceError::Internal(format!("query chart statistics: {error}")))?;

    let labels = chart_time_labels(&min_bucket, &max_bucket, &request.granularity);

    let mut lines = Vec::with_capacity(request.lines.len());
    for (index, line) in request.lines.iter().enumerate() {
        let mut by_bucket: BTreeMap<&str, i64> = BTreeMap::new();
        for point in &line_points[index] {
            by_bucket.insert(point.time.as_str(), point.amount);
        }
        let data = labels
            .iter()
            .map(|label| tr_domain::dto::ChartPoint {
                time: label.clone(),
                amount: by_bucket.get(label.as_str()).copied().unwrap_or(0),
            })
            .collect();
        lines.push(ChartLineData {
            label: line.label.clone(),
            line_type: line.transaction_type.clone(),
            data,
        });
    }

    let mut stats = BTreeMap::new();
    stats.insert(
        consts::TRANSACTION_TYPE_INCOME.to_string(),
        statistics.income,
    );
    stats.insert(
        consts::TRANSACTION_TYPE_EXPENSE.to_string(),
        statistics.expense,
    );
    stats.insert(
        consts::TRANSACTION_TYPE_TRANSFER.to_string(),
        statistics.transfer,
    );

    Ok(ChartQueryResponse {
        lines,
        statistics: stats,
    })
}

/// 由实际桶范围生成连续时间轴（字典序即时间序）：month → `"2026-01"`，year → `"2026"`。
fn chart_time_labels(min_bucket: &str, max_bucket: &str, granularity: &str) -> Vec<String> {
    if min_bucket.is_empty() {
        return Vec::new();
    }

    if granularity == "year" {
        let (Ok(min_year), Ok(max_year)) = (min_bucket.parse::<i32>(), max_bucket.parse::<i32>())
        else {
            return Vec::new();
        };
        return (min_year..=max_year).map(|year| year.to_string()).collect();
    }

    let (Some((min_year, min_month)), Some((max_year, max_month))) =
        (parse_year_month(min_bucket), parse_year_month(max_bucket))
    else {
        return Vec::new();
    };

    // 与原实现一致：起点晚于终点时返回空
    if (min_year, min_month) > (max_year, max_month) {
        return Vec::new();
    }

    let mut labels = Vec::new();
    let (mut year, mut month) = (min_year, min_month);
    while (year, month) <= (max_year, max_month) {
        labels.push(format!("{year:04}-{month:02}"));
        month += 1;
        if month > 12 {
            month = 1;
            year += 1;
        }
    }
    labels
}

/// 解析 `YYYY-MM`；非法（含月份越界、非零填充）时返回 `None`，
/// 等价 Go `time.Parse("2006-01", ...)` 的失败路径。
fn parse_year_month(value: &str) -> Option<(i32, u32)> {
    let (year_part, month_part) = value.split_once('-')?;
    if year_part.len() != 4 || month_part.len() != 2 {
        return None;
    }
    let year = year_part.parse::<i32>().ok()?;
    let month = month_part.parse::<u32>().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    Some((year, month))
}

/// 删除记录及其标签关联（同一事务）。
pub fn delete_tr_by_id(workspace: &Workspace, transaction_id: &str) -> ServiceResult<()> {
    workspace
        .transaction(|conn| {
            TrTagDao::delete_by_tr_id(conn, transaction_id)
                .map_err(|error| ServiceError::Internal(format!("delete tr tags: {error}")))?;
            TransactionRecordDao::delete_by_id(conn, transaction_id).map_err(|error| {
                ServiceError::Internal(format!("delete transaction record: {error}"))
            })?;
            Ok(())
        })
        .map_err(|error: ServiceError| {
            tracing::error!("删除交易记录失败: {}", error);
            error
        })
}

/// 关联到关键事件；目标日期没有关键事件时自动创建一条空事件。
pub fn link_to_key_event(
    workspace: &Workspace,
    transaction_id: &str,
    date: &str,
) -> ServiceResult<()> {
    workspace
        .transaction(|conn| {
            let record =
                TransactionRecordDao::query_by_id(conn, transaction_id).map_err(|error| {
                    if is_not_found(&error) {
                        ServiceError::Internal(format!("transaction not found: {transaction_id}"))
                    } else {
                        ServiceError::Internal(format!("query transaction: {error}"))
                    }
                })?;

            TransactionRecordDao::update_key_event_date(conn, transaction_id, date).map_err(
                |error| {
                    if is_not_found(&error) {
                        ServiceError::Internal(format!("transaction not found: {transaction_id}"))
                    } else {
                        ServiceError::Internal(format!("update key event date: {error}"))
                    }
                },
            )?;

            match KeyEventDao::query_by_date(conn, &record.ledger_id, date) {
                Ok(_) => {}
                Err(error) if is_not_found(&error) => {
                    KeyEventDao::upsert(
                        conn,
                        &tr_domain::models::KeyEvent {
                            id: tr_store::util::new_uuid(),
                            date: date.to_string(),
                            ledger_id: record.ledger_id.clone(),
                            ..tr_domain::models::KeyEvent::default()
                        },
                    )
                    .map_err(|error| {
                        ServiceError::Internal(format!("auto-create key event: {error}"))
                    })?;
                    tracing::info!("自动创建空关键事件, 日期: {}", date);
                }
                Err(error) => {
                    return Err(ServiceError::Internal(format!("check key event: {error}")))
                }
            }
            Ok(())
        })
        .map_err(|error: ServiceError| {
            tracing::error!(
                "关联交易 {} 到关键事件 {} 失败: {}",
                transaction_id,
                date,
                error
            );
            error
        })
}

/// 解除关键事件关联（把 `key_event_date` 清空）。
pub fn unlink_from_key_event(workspace: &Workspace, transaction_id: &str) -> ServiceResult<()> {
    TransactionRecordDao::update_key_event_date(&workspace.connection(), transaction_id, "")
        .map_err(|error| {
            if is_not_found(&error) {
                ServiceError::Internal(format!("transaction not found: {transaction_id}"))
            } else {
                ServiceError::Internal(format!("unlink key event date: {error}"))
            }
        })
}

/// 某账本下关联到指定日期的全部记录（含标签）。
pub fn query_linked_by_date(
    workspace: &Workspace,
    ledger_id: &str,
    date: &str,
) -> ServiceResult<Vec<TransactionRecordDto>> {
    let conn = workspace.connection();
    let records = TransactionRecordDao::query_by_key_event_date(&conn, ledger_id, date)
        .map_err(|error| ServiceError::Internal(format!("query by key event date: {error}")))?;

    let transaction_ids: Vec<String> = records
        .iter()
        .map(|record| record.transaction_id.clone())
        .collect();
    let tag_map = TrTagDao::query_by_tr_ids(&conn, &transaction_ids)
        .map_err(|error| ServiceError::Internal(format!("query tr tags: {error}")))?;

    let mut dtos = Vec::with_capacity(records.len());
    for record in &records {
        let mut dto = TransactionRecordDto::from_record(record);
        if let Some(tags) = tag_map.get(&record.transaction_id) {
            for tag in tags {
                dto.tags.push(tag.tag.clone());
            }
        }
        dtos.push(dto);
    }
    Ok(dtos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tr_domain::dto::QueryConditionItem;
    use tr_store::util::new_uuid;

    fn workspace(tag: &str) -> (Workspace, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "tr-service-tr-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        (Workspace::open(&dir).unwrap(), dir)
    }

    fn dto(
        price: i64,
        transaction_type: &str,
        category: &str,
        at: i64,
        tags: &[&str],
    ) -> TransactionRecordDto {
        TransactionRecordDto {
            ledger_id: "l1".to_string(),
            price,
            transaction_type: transaction_type.to_string(),
            category: category.to_string(),
            description: format!("备注{price}"),
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
            transaction_at: at,
            ..TransactionRecordDto::default()
        }
    }

    #[test]
    fn create_persists_record_and_tags() {
        let (workspace, dir) = workspace("create");
        let id = create_tr(
            &workspace,
            &dto(12345, "expense", "餐饮美食", 100, &["三餐", "外卖"]),
        )
        .unwrap();

        let result = query_trs_on_condition(
            &workspace,
            &TrQueryCondition {
                ledger_id: "l1".into(),
                ..TrQueryCondition::default()
            },
        )
        .unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.items[0].transaction_id, id);
        assert_eq!(result.items[0].tags, vec!["三餐", "外卖"]);
        // 新建时 key_event_date 必须为空
        assert_eq!(result.items[0].key_event_date, "");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn batch_create_returns_count_and_skips_empty() {
        let (workspace, dir) = workspace("batch");
        assert_eq!(batch_create_tr(&workspace, &[]).unwrap(), 0);

        let dtos = vec![
            dto(100, "expense", "餐饮美食", 10, &["三餐"]),
            dto(200, "income", "工资奖金", 20, &[]),
        ];
        assert_eq!(batch_create_tr(&workspace, &dtos).unwrap(), 2);

        let result = query_trs_on_condition(
            &workspace,
            &TrQueryCondition {
                ledger_id: "l1".into(),
                ..TrQueryCondition::default()
            },
        )
        .unwrap();
        assert_eq!(result.total, 2);
        assert_eq!(result.tr_statistics["expense"], 100);
        assert_eq!(result.tr_statistics["income"], 200);
        assert_eq!(result.tr_statistics["transfer"], 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn pagination_metadata_matches_original_math() {
        let (workspace, dir) = workspace("page");
        for index in 0..5 {
            create_tr(
                &workspace,
                &dto(100 + index, "expense", "餐饮美食", 10 * index, &[]),
            )
            .unwrap();
        }

        // 无分页参数：page_size = 返回条数，page = 1
        let result = query_trs_on_condition(
            &workspace,
            &TrQueryCondition {
                ledger_id: "l1".into(),
                ..TrQueryCondition::default()
            },
        )
        .unwrap();
        assert_eq!(result.page, 1);
        assert_eq!(result.page_size, 5);
        assert_eq!(result.total_pages, 1);

        // limit=2, offset=2 → 第 2 页
        let result = query_trs_on_condition(
            &workspace,
            &TrQueryCondition {
                ledger_id: "l1".into(),
                offset: 2,
                limit: 2,
                ..TrQueryCondition::default()
            },
        )
        .unwrap();
        assert_eq!(result.items.len(), 2);
        assert_eq!(result.total, 5);
        assert_eq!(result.page, 2);
        assert_eq!(result.page_size, 2);
        assert_eq!(result.total_pages, 3);

        // 空结果 + limit<=0：page_size=0 且不触发除零
        let result = query_trs_on_condition(
            &workspace,
            &TrQueryCondition {
                ledger_id: "other".into(),
                ..TrQueryCondition::default()
            },
        )
        .unwrap();
        assert_eq!(result.total, 0);
        assert_eq!(result.page_size, 0);
        assert_eq!(result.total_pages, 0);
        assert_eq!(result.page, 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn chart_response_zero_fills_the_axis_across_all_lines() {
        let (workspace, dir) = workspace("chart");
        // 1 月与 3 月各一条支出（2 月缺失，应补 0）；收入只在 3 月
        let january = 1_768_435_200_i64;
        let march = 1_773_590_400_i64;
        create_tr(&workspace, &dto(100, "expense", "餐饮美食", january, &[])).unwrap();
        create_tr(&workspace, &dto(300, "expense", "餐饮美食", march, &[])).unwrap();
        create_tr(&workspace, &dto(900, "income", "工资奖金", march, &[])).unwrap();

        let response = query_trs_for_chart(
            &workspace,
            &ChartQueryRequest {
                ledger_id: "l1".into(),
                ts_range: vec![],
                granularity: "month".into(),
                lines: vec![
                    tr_domain::dto::ChartLineCondition {
                        label: "支出".into(),
                        transaction_type: "expense".into(),
                        include_outlier: true,
                        conditions: vec![],
                    },
                    tr_domain::dto::ChartLineCondition {
                        label: "收入".into(),
                        transaction_type: "income".into(),
                        include_outlier: true,
                        conditions: vec![],
                    },
                ],
            },
        )
        .unwrap();

        assert_eq!(response.lines.len(), 2);
        let expense: Vec<(&str, i64)> = response.lines[0]
            .data
            .iter()
            .map(|point| (point.time.as_str(), point.amount))
            .collect();
        assert_eq!(
            expense,
            vec![("2026-01", 100), ("2026-02", 0), ("2026-03", 300)]
        );

        // 第二条曲线沿用同一时间轴（跨曲线取最小/最大桶）
        let income: Vec<(&str, i64)> = response.lines[1]
            .data
            .iter()
            .map(|point| (point.time.as_str(), point.amount))
            .collect();
        assert_eq!(
            income,
            vec![("2026-01", 0), ("2026-02", 0), ("2026-03", 900)]
        );

        assert_eq!(response.statistics["expense"], 400);
        assert_eq!(response.statistics["income"], 900);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn chart_labels_handle_year_granularity_and_empty_range() {
        assert!(chart_time_labels("", "", "month").is_empty());
        assert_eq!(
            chart_time_labels("2025", "2027", "year"),
            vec!["2025", "2026", "2027"]
        );
        assert_eq!(
            chart_time_labels("2026-11", "2027-02", "month"),
            vec!["2026-11", "2026-12", "2027-01", "2027-02"]
        );
        // 非法桶：返回空而不是 panic（等价 Go 的 time.Parse 失败路径）
        assert!(chart_time_labels("2026-13", "2027-01", "month").is_empty());
        assert!(chart_time_labels("abc", "2027", "year").is_empty());
        // 起止倒置
        assert!(chart_time_labels("2027-02", "2026-01", "month").is_empty());
    }

    #[test]
    fn delete_removes_record_and_tags() {
        let (workspace, dir) = workspace("delete");
        let id = create_tr(&workspace, &dto(100, "expense", "餐饮美食", 10, &["三餐"])).unwrap();
        delete_tr_by_id(&workspace, &id).unwrap();

        let conn = workspace.connection();
        let records: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tbl_billadm_transaction_record",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let tags: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tbl_billadm_transaction_record_tag",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(records, 0);
        assert_eq!(tags, 0, "删除记录必须同时清掉标签关联");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn link_creates_missing_key_event_and_unlink_clears_it() {
        let (workspace, dir) = workspace("link");
        let id = create_tr(&workspace, &dto(100, "expense", "餐饮美食", 10, &[])).unwrap();

        link_to_key_event(&workspace, &id, "2026-01-01").unwrap();

        let conn = workspace.connection();
        let events: i64 = conn
            .query_row("SELECT COUNT(*) FROM tbl_billadm_key_event", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(events, 1, "缺失的关键事件应被自动创建");

        let linked = query_linked_by_date(&workspace, "l1", "2026-01-01").unwrap();
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].transaction_id, id);
        assert_eq!(linked[0].key_event_date, "2026-01-01");

        // 幂等：同一天再关联一次不重复创建
        link_to_key_event(&workspace, &id, "2026-01-01").unwrap();
        let events: i64 = conn
            .query_row("SELECT COUNT(*) FROM tbl_billadm_key_event", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(events, 1);

        unlink_from_key_event(&workspace, &id).unwrap();
        assert!(query_linked_by_date(&workspace, "l1", "2026-01-01")
            .unwrap()
            .is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn link_missing_transaction_reports_original_message() {
        let (workspace, dir) = workspace("link-missing");
        let error = link_to_key_event(&workspace, "absent", "2026-01-01").unwrap_err();
        assert_eq!(error.to_string(), "transaction not found: absent");

        let error = unlink_from_key_event(&workspace, "absent").unwrap_err();
        assert_eq!(error.to_string(), "transaction not found: absent");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn condition_filter_and_statistics_are_independent() {
        let (workspace, dir) = workspace("filter");
        create_tr(&workspace, &dto(100, "expense", "餐饮美食", 10, &["三餐"])).unwrap();
        create_tr(&workspace, &dto(200, "expense", "购物消费", 20, &[])).unwrap();
        create_tr(&workspace, &dto(300, "income", "工资奖金", 30, &[])).unwrap();

        let result = query_trs_on_condition(
            &workspace,
            &TrQueryCondition {
                ledger_id: "l1".into(),
                limit: 1,
                items: vec![QueryConditionItem {
                    category: "餐饮美食".into(),
                    ..QueryConditionItem::default()
                }],
                ..TrQueryCondition::default()
            },
        )
        .unwrap();

        assert_eq!(result.total, 1);
        assert_eq!(result.items.len(), 1);
        // 统计不受 items 影响
        assert_eq!(result.tr_statistics["expense"], 300);
        assert_eq!(result.tr_statistics["income"], 300);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn create_ignores_key_event_date_from_payload() {
        let (workspace, dir) = workspace("ignore-key-event");
        let mut payload = dto(100, "expense", "餐饮美食", 10, &[]);
        payload.key_event_date = "2026-05-05".into();
        let id = create_tr(&workspace, &payload).unwrap();

        let conn = workspace.connection();
        let date: String = conn
            .query_row(
                "SELECT key_event_date FROM tbl_billadm_transaction_record WHERE transaction_id = ?1",
                [&id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(date, "");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn new_uuid_is_used_for_each_record() {
        // 防回归：create / batch_create 都必须生成新的记录 ID，而不是沿用入参
        let (workspace, dir) = workspace("uuid");
        let mut payload = dto(100, "expense", "餐饮美食", 10, &[]);
        payload.transaction_id = "client-supplied".into();
        let first = create_tr(&workspace, &payload).unwrap();
        let second = create_tr(&workspace, &payload).unwrap();
        assert_ne!(first, "client-supplied");
        assert_ne!(first, second);
        assert_ne!(new_uuid(), new_uuid());

        std::fs::remove_dir_all(&dir).ok();
    }
}
