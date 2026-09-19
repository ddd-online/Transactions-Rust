//! 全新工作空间的建库 DDL 与既有工作空间的**只读**格式校验。
//!
//! ## 为什么是"只读校验"而不是"迁移"
//!
//! 本仓库只兼容**当前** schema。既有数据库要么已经是当前格式（校验通过、原样使用），
//! 要么是更早格式（明确拒绝并给出可操作的提示）。这样就没有任何"就地改写用户财务数据"的代码路径，
//! 也就不存在迁移脚本写错导致数据损坏的风险。
//!
//! ## 为什么 DDL 要逐字节照抄
//!
//! `fresh.sql` 是当前格式空库的原始 DDL（`sqlite3 transactions.db .schema` 的输出）。
//! SQLite 会把 `CREATE TABLE` 的原文存进 `sqlite_master`，因此只要执行同样的语句，
//! 建出的库在 `.schema` 层面就与基线逐字节一致（由 `cargo xtask schema-diff` 守住这条不变式）。

use rusqlite::Connection;

use crate::workspace::WorkspaceError;

/// 当前空库 DDL：19 张表 + 21 个索引 + 3 条迁移登记记录。
///
/// 文件内**不含任何迁移执行逻辑**，只有建库后的最终状态；末尾 3 条 INSERT 是历史迁移的
/// 登记记录（那些迁移对空库都是空操作），保留它们只为让新建库与已升级到当前格式的库完全一致。
pub const FRESH_SCHEMA_SQL: &str = include_str!("../../../fixtures/schema/fresh.sql");

/// 历史遗留的全局唯一索引名；存在即说明该库尚未升级到 (ledger_id, date) 复合唯一索引。
const LEGACY_GLOBAL_UNIQUE_INDEX: &str = "idx_tbl_billadm_key_event_date";

/// 最新 schema 要求的表与列。顺序与 DDL 一致，便于人工比对。
const REQUIRED_COLUMNS: &[(&str, &[&str])] = &[
    (
        "tbl_billadm_ledger",
        &["id", "name", "description", "created_at", "updated_at"],
    ),
    (
        "tbl_billadm_transaction_record",
        &[
            "transaction_id",
            "ledger_id",
            "price",
            "transaction_type",
            "category",
            "description",
            "flags",
            "key_event_date",
            "transaction_at",
            "created_at",
            "updated_at",
        ],
    ),
    (
        "tbl_billadm_transaction_record_tag",
        &["ledger_id", "transaction_id", "tag"],
    ),
    (
        "tbl_billadm_category",
        &["ledger_id", "name", "transaction_type", "sort_order"],
    ),
    (
        "tbl_billadm_tag",
        &[
            "ledger_id",
            "name",
            "category_transaction_type",
            "sort_order",
        ],
    ),
    (
        "tbl_billadm_transaction_tpl",
        &[
            "template_id",
            "ledger_id",
            "template_name",
            "transaction_type",
            "category",
            "tags",
            "flags",
            "description",
            "sort_order",
            "created_at",
            "updated_at",
        ],
    ),
    (
        "tbl_billadm_chart",
        &[
            "chart_id",
            "ledger_id",
            "title",
            "granularity",
            "chart_lines",
            "chart_type",
            "is_preset",
            "sort_order",
            "created_at",
            "updated_at",
        ],
    ),
    (
        "tbl_billadm_key_event",
        &[
            "id",
            "date",
            "title",
            "content",
            "color",
            "created_at",
            "updated_at",
            "ledger_id",
        ],
    ),
    (
        "tbl_billadm_key_event_image",
        &[
            "id",
            "ledger_id",
            "event_date",
            "file_path",
            "thumb_path",
            "sort_order",
            "created_at",
        ],
    ),
    (
        "tbl_billadm_diary_entry",
        &[
            "id",
            "date",
            "content",
            "word_count",
            "mood",
            "created_at",
            "updated_at",
        ],
    ),
    (
        "tbl_billadm_stock_account",
        &["id", "ledger_id", "principal", "created_at", "updated_at"],
    ),
    (
        "tbl_billadm_stock_fee_setting",
        &[
            "id",
            "ledger_id",
            "commission_rate",
            "min_commission",
            "stamp_duty_rate",
            "transfer_fee_rate",
            "created_at",
            "updated_at",
        ],
    ),
    (
        "tbl_billadm_stock_fund_record",
        &[
            "id",
            "ledger_id",
            "record_date",
            "event_type",
            "event_text",
            "amount_change",
            "cash_balance",
            "net_pnl",
            "remark",
            "created_at",
        ],
    ),
    (
        "tbl_billadm_stock_position",
        &[
            "id",
            "ledger_id",
            "stock_code",
            "stock_name",
            "quantity",
            "total_cost",
            "realized_pnl",
            "review",
            "created_at",
            "updated_at",
        ],
    ),
    (
        "tbl_billadm_stock_trade",
        &[
            "id",
            "ledger_id",
            "stock_code",
            "stock_name",
            "trade_type",
            "round_id",
            "order_id",
            "order_seq",
            "price",
            "lots",
            "shares",
            "amount",
            "fee",
            "commission",
            "stamp_duty",
            "transfer_fee",
            "realized_pnl",
            "trade_time",
            "remark",
            "created_at",
        ],
    ),
    (
        "tbl_billadm_stock_trade_history",
        &[
            "id",
            "ledger_id",
            "stock_code",
            "stock_name",
            "created_at",
            "updated_at",
        ],
    ),
    (
        "tbl_billadm_stock_trade_round",
        &[
            "id",
            "ledger_id",
            "stock_code",
            "history_id",
            "round_no",
            "opened_at",
            "closed_at",
            "tag",
            "review",
            "created_at",
        ],
    ),
    (
        "tbl_billadm_stock_trade_tag_setting",
        &["id", "ledger_id", "tags", "created_at", "updated_at"],
    ),
];

/// 在空库上执行完整 DDL。仅在数据库文件**不存在**时调用。
pub fn create_fresh(conn: &Connection) -> Result<(), WorkspaceError> {
    conn.execute_batch(FRESH_SCHEMA_SQL)?;
    Ok(())
}

/// 只读校验：确认既有库已是最新 schema。
///
/// 只查 `sqlite_master` 与 `PRAGMA table_info`，不写任何数据、不改任何结构。
/// 校验失败时返回可操作的提示（引导用户改用其他工作目录，或用能打开该格式的旧版本升级），
/// 而不是就地修复。
pub fn validate_current(conn: &Connection) -> Result<(), WorkspaceError> {
    let mut problems: Vec<String> = Vec::new();

    for (table, columns) in REQUIRED_COLUMNS {
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )?;
        if exists == 0 {
            problems.push(format!("缺少数据表 {table}"));
            continue;
        }

        // table 取自本文件的常量列表，不是外部输入
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let actual: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        for column in *columns {
            if !actual.iter().any(|name| name.as_str() == *column) {
                problems.push(format!("{table} 缺少列 {column}"));
            }
        }
    }

    let legacy_index: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?1",
        [LEGACY_GLOBAL_UNIQUE_INDEX],
        |row| row.get(0),
    )?;
    if legacy_index > 0 {
        problems.push(format!(
            "检测到旧版全局唯一索引 {LEGACY_GLOBAL_UNIQUE_INDEX}"
        ));
    }

    if problems.is_empty() {
        return Ok(());
    }

    Err(WorkspaceError::Incompatible(format!(
        "该工作空间不是当前格式（{}）：{}。请在设置中改用其他工作目录，\
         或用支持该格式的旧版本把它升级到当前格式（本版本不会自动改写既有数据库）。",
        "格式过旧",
        problems.join("；")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        create_fresh(&conn).unwrap();
        conn
    }

    #[test]
    fn fresh_database_has_current_schema() {
        let conn = fresh_conn();
        // 19 张表（18 张数据表 + 迁移登记表）
        let tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 19);
        // 21 个索引
        let indexes: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND sql IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(indexes, 21);
        validate_current(&conn).unwrap();
    }

    #[test]
    fn fresh_database_records_the_three_legacy_migrations_as_applied() {
        let conn = fresh_conn();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tbl_billadm_schema_migration",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);
        let order_id_indexes: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' \
                 AND name = 'idx_stock_trade_ledger_order'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(order_id_indexes, 1);
    }

    #[test]
    fn validation_rejects_older_workspace() {
        // 模拟更早格式建出的库：stock_trade 没有 order_id / order_seq，且存在旧的全局唯一索引
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE `tbl_billadm_stock_trade` (`id` text, `ledger_id` text);
             CREATE UNIQUE INDEX `idx_tbl_billadm_key_event_date` ON `tbl_billadm_stock_trade`(`id`);",
        )
        .unwrap();

        let err = validate_current(&conn).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("order_id"), "message = {message}");
        assert!(
            message.contains("idx_tbl_billadm_key_event_date"),
            "message = {message}"
        );
        assert!(message.contains("不是当前格式"), "message = {message}");
    }

    #[test]
    fn validation_is_read_only() {
        let conn = fresh_conn();
        let before: Vec<(String, String)> = {
            let mut stmt = conn
                .prepare("SELECT type, name FROM sqlite_master ORDER BY type, name")
                .unwrap();
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows
        };
        validate_current(&conn).unwrap();
        let after: Vec<(String, String)> = {
            let mut stmt = conn
                .prepare("SELECT type, name FROM sqlite_master ORDER BY type, name")
                .unwrap();
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows
        };
        assert_eq!(before, after);
    }
}
