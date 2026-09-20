//! 消费记录 DAO。
//!
//! 本文件是"行为等价"要求最高的一处：筛选、排序、统计、图表分桶**全部在 SQL 里完成**，
//! 且 SQL 的形状必须逐条固定（否则结果集或排序会漂移）。要点：
//! * 条件项之间是 **OR**，条件项内部是 **AND**；空条件项等价于 `1 = 1`（匹配全部）
//! * 描述匹配用 `instr(description, ?) > 0`（区分大小写、不把 `%_` 当通配符）
//! * 标签用子查询：`all` 走 `COUNT(DISTINCT ...) = n`，否则 `EXISTS`，`tag_not` 加 `NOT`
//! * 排序字段走白名单映射，非法/空则回退 `transaction_at desc`
//! * 统计口径固定为「账本 + 时间范围」，**不随筛选条件变化**
//! * 图表分桶用 `strftime(<格式>, transaction_at, 'unixepoch')`，缺省排除 outlier

use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection};

use tr_domain::consts;
use tr_domain::dto::{
    ChartLineCondition, QueryConditionItem, QueryConditionSortField, TrQueryCondition,
};
use tr_domain::models::TransactionRecord;

pub struct TransactionRecordDao;

/// 按交易类型的金额汇总（income / expense / transfer 三个口径）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrStatistics {
    pub income: i64,
    pub expense: i64,
    pub transfer: i64,
}

/// 过滤 + 排序 + 分页 + 统计结果。
#[derive(Debug, Clone, Default)]
pub struct TrFilterResult {
    pub items: Vec<TransactionRecord>,
    pub total: i64,
    pub statistics: TrStatistics,
}

const COLUMNS: &str = "transaction_id, ledger_id, price, transaction_type, category, description, \
     flags, key_event_date, transaction_at, created_at, updated_at";

impl TransactionRecordDao {
    /// 新建一条记录（自动填充时间戳）。
    pub fn create(conn: &Connection, record: &TransactionRecord) -> rusqlite::Result<()> {
        let now = crate::util::now_unix();
        conn.execute(
            "INSERT INTO tbl_billadm_transaction_record \
             (transaction_id, ledger_id, price, transaction_type, category, description, flags, \
              key_event_date, transaction_at, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            rusqlite::params![
                record.transaction_id,
                record.ledger_id,
                record.price,
                record.transaction_type,
                record.category,
                record.description,
                record.flags,
                record.key_event_date,
                record.transaction_at,
                now,
            ],
        )?;
        Ok(())
    }

    /// 批量新建：在调用方的事务内逐条插入，与"500 条一批的多行插入"在
    /// 行集合与时间戳口径上完全等价。
    pub fn create_batch(conn: &Connection, records: &[TransactionRecord]) -> rusqlite::Result<()> {
        for record in records {
            Self::create(conn, record)?;
        }
        Ok(())
    }

    /// 按 ID 查询；不存在返回 `QueryReturnedNoRows`。
    pub fn query_by_id(
        conn: &Connection,
        transaction_id: &str,
    ) -> rusqlite::Result<TransactionRecord> {
        conn.query_row(
            &format!(
                "SELECT {COLUMNS} FROM tbl_billadm_transaction_record WHERE transaction_id = ?1"
            ),
            [transaction_id],
            from_row,
        )
    }

    /// 删除记录本身（标签关联由服务层在同一事务里清理）。
    pub fn delete_by_id(conn: &Connection, transaction_id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_transaction_record WHERE transaction_id = ?1",
            [transaction_id],
        )?;
        Ok(())
    }

    /// 更新关键事件关联日期。命中 0 行时返回 `QueryReturnedNoRows`
    /// （把"影响 0 行"显式转成"查无记录"）。
    pub fn update_key_event_date(
        conn: &Connection,
        transaction_id: &str,
        date: &str,
    ) -> rusqlite::Result<()> {
        let affected = conn.execute(
            "UPDATE tbl_billadm_transaction_record SET key_event_date = ?2, updated_at = ?3 \
             WHERE transaction_id = ?1",
            rusqlite::params![transaction_id, date, crate::util::now_unix()],
        )?;
        if affected == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        Ok(())
    }

    /// 某账本下关联到指定关键事件日期的记录（按交易时间倒序）。
    pub fn query_by_key_event_date(
        conn: &Connection,
        ledger_id: &str,
        date: &str,
    ) -> rusqlite::Result<Vec<TransactionRecord>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM tbl_billadm_transaction_record \
             WHERE ledger_id = ?1 AND key_event_date = ?2 ORDER BY transaction_at desc"
        ))?;
        let rows = statement.query_map([ledger_id, date], from_row)?;
        rows.collect()
    }

    /// 某账本的记录数。
    pub fn count_by_ledger_id(conn: &Connection, ledger_id: &str) -> rusqlite::Result<i64> {
        conn.query_row(
            "SELECT COUNT(*) FROM tbl_billadm_transaction_record WHERE ledger_id = ?1",
            [ledger_id],
            |row| row.get(0),
        )
    }

    /// 条件查询：先取总数，再排序分页，最后按「账本 + 时间范围」统计。
    pub fn query_filtered(
        conn: &Connection,
        condition: &TrQueryCondition,
    ) -> rusqlite::Result<TrFilterResult> {
        let (where_sql, args) = build_where(condition);

        let total: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM tbl_billadm_transaction_record {where_sql}"),
            params_from_iter(args.clone()),
            |row| row.get(0),
        )?;

        let mut sql = format!(
            "SELECT {COLUMNS} FROM tbl_billadm_transaction_record {where_sql} ORDER BY {}",
            build_sort_clause(&condition.sort_fields)
        );
        // offset 需要 LIMIT 才生效（SQLite 里 LIMIT -1 表示不限制）
        match (condition.limit > 0, condition.offset > 0) {
            (true, true) => sql.push_str(&format!(
                " LIMIT {} OFFSET {}",
                condition.limit, condition.offset
            )),
            (true, false) => sql.push_str(&format!(" LIMIT {}", condition.limit)),
            (false, true) => sql.push_str(&format!(" LIMIT -1 OFFSET {}", condition.offset)),
            (false, false) => {}
        }

        let mut statement = conn.prepare(&sql)?;
        let items: Vec<TransactionRecord> = statement
            .query_map(params_from_iter(args), from_row)?
            .collect::<Result<Vec<_>, _>>()?;

        let statistics = Self::query_statistics(conn, &condition.ledger_id, &condition.ts_range)?;

        Ok(TrFilterResult {
            items,
            total,
            statistics,
        })
    }

    /// 按交易类型汇总指定账本与时间范围内的金额（不受筛选条件影响）。
    pub fn query_statistics(
        conn: &Connection,
        ledger_id: &str,
        ts_range: &[i64],
    ) -> rusqlite::Result<TrStatistics> {
        let mut sql = String::from(
            "SELECT transaction_type, SUM(price) FROM tbl_billadm_transaction_record \
             WHERE ledger_id = ?",
        );
        let mut args: Vec<Value> = vec![Value::Text(ledger_id.to_string())];
        if ts_range.len() == 2 {
            sql.push_str(" AND transaction_at >= ? AND transaction_at <= ?");
            args.push(Value::Integer(ts_range[0]));
            args.push(Value::Integer(ts_range[1]));
        }
        sql.push_str(" GROUP BY transaction_type");

        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(args), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?.unwrap_or(0),
            ))
        })?;

        let mut statistics = TrStatistics::default();
        for row in rows {
            let (transaction_type, total) = row?;
            match transaction_type.as_str() {
                consts::TRANSACTION_TYPE_INCOME => statistics.income = total,
                consts::TRANSACTION_TYPE_EXPENSE => statistics.expense = total,
                consts::TRANSACTION_TYPE_TRANSFER => statistics.transfer = total,
                _ => {}
            }
        }
        Ok(statistics)
    }

    /// 单条曲线的分桶聚合序列。
    pub fn query_chart_line_data(
        conn: &Connection,
        ledger_id: &str,
        ts_range: &[i64],
        granularity: &str,
        line: &ChartLineCondition,
    ) -> rusqlite::Result<Vec<tr_domain::dto::ChartPoint>> {
        let mut sql = String::from(
            "SELECT strftime(?, transaction_at, 'unixepoch') AS time, SUM(price) AS amount \
             FROM tbl_billadm_transaction_record WHERE ledger_id = ?",
        );
        // 分桶：strftime(<格式串>, transaction_at, 'unixepoch')
        // strftime 的格式串就是粒度（"%Y-%m" / "%Y"）
        let time_format = if granularity == "year" { "%Y" } else { "%Y-%m" };
        let mut args: Vec<Value> = vec![
            Value::Text(time_format.to_string()),
            Value::Text(ledger_id.to_string()),
        ];

        if ts_range.len() == 2 {
            sql.push_str(" AND transaction_at >= ? AND transaction_at <= ?");
            args.push(Value::Integer(ts_range[0]));
            args.push(Value::Integer(ts_range[1]));
        }

        sql.push_str(" AND transaction_type = ?");
        args.push(Value::Text(line.transaction_type.clone()));

        if !line.include_outlier {
            // flags 存的是 JSON（{"outlier":true}）；json_extract 缺失或非法时返回 NULL，行保留
            sql.push_str(" AND json_extract(flags, '$.outlier') IS NOT 1");
        }

        let (items_sql, items_args) = build_items_clause(&line.conditions);
        if !items_sql.is_empty() {
            sql.push_str(&format!(" AND ({items_sql})"));
            args.extend(items_args);
        }

        sql.push_str(" GROUP BY time");

        let mut statement = conn.prepare(&sql)?;
        let points = statement
            .query_map(params_from_iter(args), |row| {
                Ok(tr_domain::dto::ChartPoint {
                    time: row.get(0)?,
                    amount: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(points)
    }
}

/// 组装 WHERE 子句（账本 + 可选时间范围 + 条件项）。
fn build_where(condition: &TrQueryCondition) -> (String, Vec<Value>) {
    let mut clauses = vec!["ledger_id = ?".to_string()];
    let mut args: Vec<Value> = vec![Value::Text(condition.ledger_id.clone())];

    if condition.ts_range.len() == 2 {
        clauses.push("transaction_at >= ?".to_string());
        args.push(Value::Integer(condition.ts_range[0]));
        clauses.push("transaction_at <= ?".to_string());
        args.push(Value::Integer(condition.ts_range[1]));
    }

    let (items_sql, items_args) = build_items_clause(&condition.items);
    if !items_sql.is_empty() {
        clauses.push(format!("({items_sql})"));
        args.extend(items_args);
    }

    (format!("WHERE {}", clauses.join(" AND ")), args)
}

/// 条件项列表 → SQL 片段（OR 语义）。空列表返回空串（不加任何限制）。
fn build_items_clause(items: &[QueryConditionItem]) -> (String, Vec<Value>) {
    if items.is_empty() {
        return (String::new(), Vec::new());
    }

    let mut or_clauses: Vec<String> = Vec::with_capacity(items.len());
    let mut args: Vec<Value> = Vec::new();

    for item in items {
        let mut sub: Vec<String> = Vec::with_capacity(4);
        let mut sub_args: Vec<Value> = Vec::with_capacity(4);

        if !item.transaction_type.is_empty() {
            sub.push("transaction_type = ?".to_string());
            sub_args.push(Value::Text(item.transaction_type.clone()));
        }
        if !item.category.is_empty() {
            sub.push("category = ?".to_string());
            sub_args.push(Value::Text(item.category.clone()));
        }
        if !item.description.is_empty() {
            sub.push("instr(description, ?) > 0".to_string());
            sub_args.push(Value::Text(item.description.clone()));
        }
        if !item.tags.is_empty() {
            sub.push(build_tag_condition(
                &item.tags,
                &item.tag_policy,
                item.tag_not,
            ));
            for tag in &item.tags {
                sub_args.push(Value::Text(tag.clone()));
            }
        }

        if sub.is_empty() {
            // 空条件项在内存实现中匹配所有记录（OR 语义），保持等价
            or_clauses.push("1 = 1".to_string());
            continue;
        }

        or_clauses.push(format!("({})", sub.join(" AND ")));
        args.extend(sub_args);
    }

    (or_clauses.join(" OR "), args)
}

/// 标签匹配子查询：`all` 要求同时包含全部标签，否则按 `any` 处理。
fn build_tag_condition(tags: &[String], policy: &str, negate: bool) -> String {
    let placeholders = vec!["?"; tags.len()].join(",");
    let expr = if policy == consts::ALL {
        format!(
            "(SELECT COUNT(DISTINCT t.tag) FROM tbl_billadm_transaction_record_tag t \
             WHERE t.transaction_id = tbl_billadm_transaction_record.transaction_id \
             AND t.tag IN ({placeholders})) = {}",
            tags.len()
        )
    } else {
        format!(
            "EXISTS (SELECT 1 FROM tbl_billadm_transaction_record_tag t \
             WHERE t.transaction_id = tbl_billadm_transaction_record.transaction_id \
             AND t.tag IN ({placeholders}))"
        )
    };

    if negate {
        format!("NOT {expr}")
    } else {
        expr
    }
}

/// 排序子句：字段白名单 + 默认 `transaction_at desc`。
fn build_sort_clause(sort_fields: &[QueryConditionSortField]) -> String {
    if sort_fields.is_empty() {
        return "transaction_at desc".to_string();
    }

    let mut clauses: Vec<String> = Vec::with_capacity(sort_fields.len());
    for field in sort_fields {
        let column = match field.field.as_str() {
            "transactionAt" => "transaction_at",
            "transactionType" => "transaction_type",
            "price" => "price",
            "category" => "category",
            _ => continue,
        };
        let order = if field.order == "asc" { "asc" } else { "desc" };
        clauses.push(format!("{column} {order}"));
    }

    if clauses.is_empty() {
        "transaction_at desc".to_string()
    } else {
        clauses.join(", ")
    }
}

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TransactionRecord> {
    Ok(TransactionRecord {
        transaction_id: row.get(0)?,
        ledger_id: row.get(1)?,
        price: row.get(2)?,
        transaction_type: row.get(3)?,
        category: row.get(4)?,
        description: row.get(5)?,
        flags: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
        key_event_date: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        transaction_at: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(
        id: &str,
        price: i64,
        transaction_type: &str,
        category: &str,
        at: i64,
        outlier: bool,
    ) -> TransactionRecord {
        TransactionRecord {
            transaction_id: id.to_string(),
            ledger_id: "l1".to_string(),
            price,
            transaction_type: transaction_type.to_string(),
            category: category.to_string(),
            description: format!("备注{id}"),
            flags: if outlier {
                r#"{"outlier":true}"#.to_string()
            } else {
                r#"{"outlier":false}"#.to_string()
            },
            key_event_date: String::new(),
            transaction_at: at,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn seed_tags(conn: &Connection, tr_id: &str, tags: &[&str]) {
        for tag in tags {
            conn.execute(
                "INSERT INTO tbl_billadm_transaction_record_tag (ledger_id, transaction_id, tag) \
                 VALUES ('l1', ?1, ?2)",
                rusqlite::params![tr_id, tag],
            )
            .unwrap();
        }
    }

    fn base_condition() -> TrQueryCondition {
        TrQueryCondition {
            ledger_id: "l1".to_string(),
            ..TrQueryCondition::default()
        }
    }

    #[test]
    fn create_and_query_by_id_roundtrip() {
        let (workspace, dir) = crate::dao::test_workspace("dao-tr-roundtrip");
        let conn = workspace.connection();
        TransactionRecordDao::create(
            &conn,
            &record("t1", 12345, "expense", "餐饮美食", 100, false),
        )
        .unwrap();

        let loaded = TransactionRecordDao::query_by_id(&conn, "t1").unwrap();
        assert_eq!(loaded.price, 12345);
        assert_eq!(loaded.category, "餐饮美食");
        assert!(loaded.created_at > 0);
        assert!(super::super::is_not_found(
            &TransactionRecordDao::query_by_id(&conn, "nope").unwrap_err()
        ));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn filtered_query_supports_tag_policies_and_or_items() {
        let (workspace, dir) = crate::dao::test_workspace("dao-tr-filter");
        let conn = workspace.connection();
        TransactionRecordDao::create(&conn, &record("t1", 100, "expense", "餐饮美食", 10, false))
            .unwrap();
        TransactionRecordDao::create(&conn, &record("t2", 200, "expense", "购物消费", 20, false))
            .unwrap();
        TransactionRecordDao::create(&conn, &record("t3", 300, "income", "工资奖金", 30, false))
            .unwrap();
        seed_tags(&conn, "t1", &["三餐", "外卖"]);
        seed_tags(&conn, "t2", &["三餐"]);

        // any：命中任一标签
        let mut condition = base_condition();
        condition.items = vec![QueryConditionItem {
            tags: vec!["三餐".into()],
            tag_policy: "any".into(),
            ..QueryConditionItem::default()
        }];
        let result = TransactionRecordDao::query_filtered(&conn, &condition).unwrap();
        assert_eq!(result.total, 2);

        // all：必须同时含两个标签
        let mut condition = base_condition();
        condition.items = vec![QueryConditionItem {
            tags: vec!["三餐".into(), "外卖".into()],
            tag_policy: "all".into(),
            ..QueryConditionItem::default()
        }];
        let result = TransactionRecordDao::query_filtered(&conn, &condition).unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.items[0].transaction_id, "t1");

        // not：取反
        let mut condition = base_condition();
        condition.items = vec![QueryConditionItem {
            tags: vec!["三餐".into()],
            tag_policy: "any".into(),
            tag_not: true,
            ..QueryConditionItem::default()
        }];
        let result = TransactionRecordDao::query_filtered(&conn, &condition).unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.items[0].transaction_id, "t3");

        // 条件项之间是 OR
        let mut condition = base_condition();
        condition.items = vec![
            QueryConditionItem {
                category: "餐饮美食".into(),
                ..QueryConditionItem::default()
            },
            QueryConditionItem {
                category: "工资奖金".into(),
                ..QueryConditionItem::default()
            },
        ];
        let result = TransactionRecordDao::query_filtered(&conn, &condition).unwrap();
        assert_eq!(result.total, 2);

        // 空条件项匹配全部
        let mut condition = base_condition();
        condition.items = vec![QueryConditionItem::default()];
        let result = TransactionRecordDao::query_filtered(&conn, &condition).unwrap();
        assert_eq!(result.total, 3);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn description_match_uses_instr_not_like() {
        let (workspace, dir) = crate::dao::test_workspace("dao-tr-instr");
        let conn = workspace.connection();
        let mut first = record("t1", 100, "expense", "餐饮美食", 10, false);
        first.description = "午餐 100%".to_string();
        TransactionRecordDao::create(&conn, &first).unwrap();
        TransactionRecordDao::create(&conn, &record("t2", 200, "expense", "餐饮美食", 20, false))
            .unwrap();

        let mut condition = base_condition();
        condition.items = vec![QueryConditionItem {
            description: "午餐".into(),
            ..QueryConditionItem::default()
        }];
        assert_eq!(
            TransactionRecordDao::query_filtered(&conn, &condition)
                .unwrap()
                .total,
            1
        );

        // `%` 不被当作通配符（LIKE 会误命中全部）
        let mut condition = base_condition();
        condition.items = vec![QueryConditionItem {
            description: "%".into(),
            ..QueryConditionItem::default()
        }];
        assert_eq!(
            TransactionRecordDao::query_filtered(&conn, &condition)
                .unwrap()
                .total,
            1
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn statistics_ignore_items_and_respect_time_range() {
        let (workspace, dir) = crate::dao::test_workspace("dao-tr-stats");
        let conn = workspace.connection();
        TransactionRecordDao::create(&conn, &record("t1", 100, "expense", "餐饮美食", 10, false))
            .unwrap();
        TransactionRecordDao::create(&conn, &record("t2", 200, "income", "工资奖金", 20, false))
            .unwrap();
        TransactionRecordDao::create(&conn, &record("t3", 300, "transfer", "五险一金", 30, false))
            .unwrap();
        TransactionRecordDao::create(&conn, &record("t4", 400, "expense", "购物消费", 40, false))
            .unwrap();

        let statistics = TransactionRecordDao::query_statistics(&conn, "l1", &[]).unwrap();
        assert_eq!(statistics.income, 200);
        assert_eq!(statistics.expense, 500);
        assert_eq!(statistics.transfer, 300);

        let statistics = TransactionRecordDao::query_statistics(&conn, "l1", &[15, 25]).unwrap();
        assert_eq!(statistics.income, 200);
        assert_eq!(statistics.expense, 0);

        // 带筛选条件时统计口径不变（仍是账本 + 时间范围全量）
        let mut condition = base_condition();
        condition.limit = 1;
        condition.items = vec![QueryConditionItem {
            category: "餐饮美食".into(),
            ..QueryConditionItem::default()
        }];
        let result = TransactionRecordDao::query_filtered(&conn, &condition).unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.statistics.expense, 500);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sort_whitelist_and_default_order() {
        let (workspace, dir) = crate::dao::test_workspace("dao-tr-sort");
        let conn = workspace.connection();
        TransactionRecordDao::create(&conn, &record("t1", 300, "expense", "b", 10, false)).unwrap();
        TransactionRecordDao::create(&conn, &record("t2", 100, "expense", "a", 30, false)).unwrap();
        TransactionRecordDao::create(&conn, &record("t3", 200, "expense", "c", 20, false)).unwrap();

        // 默认：transaction_at desc
        let result = TransactionRecordDao::query_filtered(&conn, &base_condition()).unwrap();
        let ids: Vec<&str> = result
            .items
            .iter()
            .map(|item| item.transaction_id.as_str())
            .collect();
        assert_eq!(ids, vec!["t2", "t3", "t1"]);

        // 白名单字段 + asc
        let mut condition = base_condition();
        condition.sort_fields = vec![QueryConditionSortField {
            field: "price".into(),
            order: "asc".into(),
        }];
        let result = TransactionRecordDao::query_filtered(&conn, &condition).unwrap();
        let ids: Vec<&str> = result
            .items
            .iter()
            .map(|item| item.transaction_id.as_str())
            .collect();
        assert_eq!(ids, vec!["t2", "t3", "t1"]);

        // 非法字段被忽略 → 回退默认排序
        let mut condition = base_condition();
        condition.sort_fields = vec![QueryConditionSortField {
            field: "drop table".into(),
            order: "asc".into(),
        }];
        let result = TransactionRecordDao::query_filtered(&conn, &condition).unwrap();
        let ids: Vec<&str> = result
            .items
            .iter()
            .map(|item| item.transaction_id.as_str())
            .collect();
        assert_eq!(ids, vec!["t2", "t3", "t1"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn chart_line_data_buckets_by_month_and_year_excluding_outliers() {
        let (workspace, dir) = crate::dao::test_workspace("dao-tr-chart");
        let conn = workspace.connection();
        // 2026-01-15 与 2026-02-15（UTC 秒）
        let january = 1_768_435_200_i64;
        let february = 1_771_113_600_i64;
        TransactionRecordDao::create(&conn, &record("t1", 100, "expense", "a", january, false))
            .unwrap();
        TransactionRecordDao::create(&conn, &record("t2", 150, "expense", "a", january, true))
            .unwrap();
        TransactionRecordDao::create(&conn, &record("t3", 300, "expense", "a", february, false))
            .unwrap();
        TransactionRecordDao::create(&conn, &record("t4", 999, "income", "b", february, false))
            .unwrap();

        let line = ChartLineCondition {
            label: "支出".into(),
            transaction_type: "expense".into(),
            include_outlier: false,
            conditions: Vec::new(),
        };
        let points =
            TransactionRecordDao::query_chart_line_data(&conn, "l1", &[], "month", &line).unwrap();
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].time, "2026-01");
        assert_eq!(points[0].amount, 100, "outlier 记录应被排除");
        assert_eq!(points[1].time, "2026-02");
        assert_eq!(points[1].amount, 300);

        // include_outlier = true 时包含 outlier
        let line = ChartLineCondition {
            include_outlier: true,
            ..line
        };
        let points =
            TransactionRecordDao::query_chart_line_data(&conn, "l1", &[], "month", &line).unwrap();
        assert_eq!(points[0].amount, 250);

        // 年度粒度
        let line = ChartLineCondition {
            label: "支出".into(),
            transaction_type: "expense".into(),
            include_outlier: false,
            conditions: Vec::new(),
        };
        let points =
            TransactionRecordDao::query_chart_line_data(&conn, "l1", &[], "year", &line).unwrap();
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].time, "2026");
        assert_eq!(points[0].amount, 400);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn update_key_event_date_reports_missing_row() {
        let (workspace, dir) = crate::dao::test_workspace("dao-tr-link");
        let conn = workspace.connection();
        TransactionRecordDao::create(&conn, &record("t1", 100, "expense", "a", 10, false)).unwrap();

        TransactionRecordDao::update_key_event_date(&conn, "t1", "2026-01-01").unwrap();
        assert_eq!(
            TransactionRecordDao::query_by_id(&conn, "t1")
                .unwrap()
                .key_event_date,
            "2026-01-01"
        );
        assert!(super::super::is_not_found(
            &TransactionRecordDao::update_key_event_date(&conn, "absent", "2026-01-01")
                .unwrap_err()
        ));

        std::fs::remove_dir_all(&dir).ok();
    }
}
