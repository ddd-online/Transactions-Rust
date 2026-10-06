//! 账本级联删除的 SQL 清单（候选 8 / #38）。
//!
//! 从 `tr-service/src/ledger.rs` 搬来：分层纪律要求 SQL 只出现在 `tr-store`，
//! 而它同时是**一条护栏的规格** —— `tr-service` 里那条"凡是带 `ledger_id` 的表都必须出现在
//! 这张清单里"的覆盖断言读的就是这里（`cascade_statements()`），所以清单本身也要搬过来。
//!
//! 顺序**逐条固定**（先子表后父表、最后账本行），新增业务表时必须同步补进来，否则那条断言会红。

use rusqlite::Connection;

use super::ledger::LedgerDao;

/// 删除账本时的级联清理顺序。
pub const LEDGER_CASCADE: &[&str] = &[
    "DELETE FROM tbl_billadm_transaction_record_tag WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_transaction_record WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_category WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_tag WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_chart WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_transaction_tpl WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_key_event_image WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_key_event WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_diary_entry WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_stock_fund_record WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_stock_fee_setting WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_stock_trade_tag_setting WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_stock_trade WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_stock_trade_round WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_stock_trade_history WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_stock_position WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_stock_account WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_stock_operation WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_todo_progress WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_todo_item WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_todo_card WHERE ledger_id = ?1",
    "DELETE FROM tbl_billadm_ledger WHERE id = ?1",
];

impl LedgerDao {
    /// 级联删除一个账本的全部业务数据（调用方负责事务边界）。
    pub fn delete_cascade(conn: &Connection, ledger_id: &str) -> rusqlite::Result<()> {
        for sql in LEDGER_CASCADE {
            conn.execute(sql, [ledger_id])?;
        }
        Ok(())
    }

    /// 级联清单（给守护测试用：新增带 ledger_id 的表时必须出现在这里）。
    pub fn cascade_statements() -> &'static [&'static str] {
        LEDGER_CASCADE
    }
}
