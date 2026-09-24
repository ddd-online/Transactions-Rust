//! 股票域 DAO。
//!
//! 要点（SQL 形状与落库口径逐条固定下来，不要随意改动）：
//! * 表名列名恒为 snake_case（`tbl_billadm_stock_*`），列映射在此显式书写，不依赖 serde；
//! * 单行查询（账户 / 费用设置 / 标签设置 / 持仓 / 交易 / 历史集合 / 轮次）都补
//!   `ORDER BY id LIMIT 1`（按主键升序取第一行），
//!   补上它才能保证唯一索引被破坏时取到的行是确定的；
//! * 时间戳由 DAO 写入（`created_at` / `updated_at` 都是秒级 Unix 秒），
//!   服务层构造的返回值因此不需要自己补时间；
//! * `create_trade` 写 `remark`：只 INSERT 非零值字段，**零值列退回 SQL 默认值**，
//!   所以带备注的建仓/加仓/清仓备注必须落库。
//!   这一条曾被误读成"一律不写 remark"，落库回归对比因此抓到 4 行 remark 差异。
//!
//! `reset_by_ledger_id` 用**一条** `DELETE` 覆盖全部股票表：历史遗留表
//! `tbl_billadm_stock_journal`（已下线功能）在当前 schema 里已不存在，且本仓库不做迁移，
//! 因此这里不再逐表探测或删除它（见 AGENTS.md 的"无迁移"纪律）。

use rusqlite::{params, params_from_iter, Connection};

use tr_domain::consts;
use tr_domain::models::{
    StockAccount, StockFeeSetting, StockFundRecord, StockOperation, StockPosition, StockTrade,
    StockTradeHistory, StockTradeRound, StockTradeTagSetting,
};

pub struct StockDao;

const ACCOUNT_COLUMNS: &str = "id, ledger_id, principal, created_at, updated_at";
const FEE_COLUMNS: &str = "id, ledger_id, commission_rate, min_commission, stamp_duty_rate, \
     transfer_fee_rate, created_at, updated_at";
const TAG_SETTING_COLUMNS: &str = "id, ledger_id, tags, created_at, updated_at";
const FUND_COLUMNS: &str = "id, ledger_id, record_date, event_type, event_text, amount_change, \
     cash_balance, net_pnl, remark, created_at";
const POSITION_COLUMNS: &str = "id, ledger_id, stock_code, stock_name, quantity, total_cost, \
     realized_pnl, review, created_at, updated_at";
const TRADE_COLUMNS: &str =
    "id, ledger_id, stock_code, stock_name, trade_type, round_id, order_id, \
     order_seq, price, lots, shares, amount, fee, commission, stamp_duty, transfer_fee, \
     realized_pnl, trade_time, remark, created_at";
const HISTORY_COLUMNS: &str = "id, ledger_id, stock_code, stock_name, created_at, updated_at";
const ROUND_COLUMNS: &str =
    "id, ledger_id, stock_code, history_id, round_no, opened_at, closed_at, \
     tag, review, created_at";
const OPERATION_COLUMNS: &str = "id, ledger_id, kind, action, detail, target_id, created_at";

/// 每个账本保留的操作记录条数上限（超出的最旧记录在写入时丢弃）。
pub const STOCK_OPERATION_LIMIT: i64 = 10;

/// 「未挂接轮次」的判定片段（`round_id` 为空串或 NULL），常量拼接，值仍走占位符。
const UNATTACHED_ROUND_SQL: &str = "(round_id = '' OR round_id IS NULL)";

/// 重置时清空的股票表（这个顺序是有意的：避免外键/触发器顺序差异）。
const STOCK_TABLES: [&str; 9] = [
    "tbl_billadm_stock_fund_record",
    "tbl_billadm_stock_fee_setting",
    "tbl_billadm_stock_trade_tag_setting",
    "tbl_billadm_stock_trade",
    "tbl_billadm_stock_trade_round",
    "tbl_billadm_stock_trade_history",
    "tbl_billadm_stock_position",
    "tbl_billadm_stock_operation",
    "tbl_billadm_stock_account",
];

impl StockDao {
    // ---------- 账户 ----------

    /// 取某账本的账户；不存在返回 `QueryReturnedNoRows`。
    pub fn get_account(conn: &Connection, ledger_id: &str) -> rusqlite::Result<StockAccount> {
        conn.query_row(
            &format!(
                "SELECT {ACCOUNT_COLUMNS} FROM tbl_billadm_stock_account WHERE ledger_id = ?1 \
                 ORDER BY id LIMIT 1"
            ),
            [ledger_id],
            account_from_row,
        )
    }

    /// 新建账户（时间戳由 DAO 补齐）。
    pub fn create_account(conn: &Connection, account: &StockAccount) -> rusqlite::Result<()> {
        let now = crate::util::now_unix();
        conn.execute(
            "INSERT INTO tbl_billadm_stock_account (id, ledger_id, principal, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![account.id, account.ledger_id, account.principal, now],
        )?;
        Ok(())
    }

    /// 更新本金（不改动其它字段）。
    pub fn update_account_principal(
        conn: &Connection,
        ledger_id: &str,
        principal: i64,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE tbl_billadm_stock_account SET principal = ?2, updated_at = ?3 \
             WHERE ledger_id = ?1",
            params![ledger_id, principal, crate::util::now_unix()],
        )?;
        Ok(())
    }

    // ---------- 费用设置 ----------

    pub fn get_fee_setting(
        conn: &Connection,
        ledger_id: &str,
    ) -> rusqlite::Result<StockFeeSetting> {
        conn.query_row(
            &format!(
                "SELECT {FEE_COLUMNS} FROM tbl_billadm_stock_fee_setting WHERE ledger_id = ?1 \
                 ORDER BY id LIMIT 1"
            ),
            [ledger_id],
            fee_setting_from_row,
        )
    }

    pub fn create_fee_setting(
        conn: &Connection,
        setting: &StockFeeSetting,
    ) -> rusqlite::Result<()> {
        let now = crate::util::now_unix();
        conn.execute(
            "INSERT INTO tbl_billadm_stock_fee_setting \
             (id, ledger_id, commission_rate, min_commission, stamp_duty_rate, transfer_fee_rate, \
              created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![
                setting.id,
                setting.ledger_id,
                setting.commission_rate,
                setting.min_commission,
                setting.stamp_duty_rate,
                setting.transfer_fee_rate,
                now,
            ],
        )?;
        Ok(())
    }

    /// 只更新四个费率字段（部分更新，不动其它列）。
    pub fn update_fee_setting(
        conn: &Connection,
        setting: &StockFeeSetting,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE tbl_billadm_stock_fee_setting SET commission_rate = ?2, min_commission = ?3, \
             stamp_duty_rate = ?4, transfer_fee_rate = ?5, updated_at = ?6 WHERE ledger_id = ?1",
            params![
                setting.ledger_id,
                setting.commission_rate,
                setting.min_commission,
                setting.stamp_duty_rate,
                setting.transfer_fee_rate,
                crate::util::now_unix(),
            ],
        )?;
        Ok(())
    }

    // ---------- 标签设置 ----------

    pub fn get_trade_tag_setting(
        conn: &Connection,
        ledger_id: &str,
    ) -> rusqlite::Result<StockTradeTagSetting> {
        conn.query_row(
            &format!(
                "SELECT {TAG_SETTING_COLUMNS} FROM tbl_billadm_stock_trade_tag_setting \
                 WHERE ledger_id = ?1 ORDER BY id LIMIT 1"
            ),
            [ledger_id],
            tag_setting_from_row,
        )
    }

    pub fn create_trade_tag_setting(
        conn: &Connection,
        setting: &StockTradeTagSetting,
    ) -> rusqlite::Result<()> {
        let now = crate::util::now_unix();
        conn.execute(
            "INSERT INTO tbl_billadm_stock_trade_tag_setting \
             (id, ledger_id, tags, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
            params![setting.id, setting.ledger_id, setting.tags, now],
        )?;
        Ok(())
    }

    pub fn update_trade_tag_setting_tags(
        conn: &Connection,
        ledger_id: &str,
        tags_json: &str,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE tbl_billadm_stock_trade_tag_setting SET tags = ?2, updated_at = ?3 \
             WHERE ledger_id = ?1",
            params![ledger_id, tags_json, crate::util::now_unix()],
        )?;
        Ok(())
    }

    // ---------- 资金记录 ----------

    /// 新增一条资金记录。
    ///
    /// **`created_at` 单调递增**：时间戳只有秒级精度，
    /// 同一秒内录入的多条记录时间戳完全相同；而资金链重算按 `created_at ASC, id ASC`
    /// 取「录入顺序」，id 是随机 UUID，排序会变得不确定。因此这里在 SQL 里取
    /// `MAX(now, 本账本已有最大 created_at + 1)`，让录入顺序确定（秒级时间戳会并列，
    /// 必须严格递增才能让录入顺序可复现；这一秒级边界上的并列本身不稳定，
    /// 锁定顺序不改变任何可观察的业务口径）。
    ///
    /// 服务层在重放时显式给出 `created_at`（复刻被重放记录的录入位置），此时原样采用。
    pub fn create_fund_record(conn: &Connection, record: &StockFundRecord) -> rusqlite::Result<()> {
        // 单调递增：同一秒录入的多条记录必须能按 created_at 区分（见上方文档）。
        // 服务层显式给出 `created_at` 时原样采用，否则才查本账本已有最大值再自增
        // （避免依赖 SQL 参数编号在子查询里的绑定行为）。
        let created_at = if record.created_at > 0 {
            record.created_at
        } else {
            let max_existing: i64 = conn.query_row(
                "SELECT COALESCE(MAX(created_at), 0) FROM tbl_billadm_stock_fund_record \
                 WHERE ledger_id = ?1",
                [&record.ledger_id],
                |row| row.get(0),
            )?;
            crate::util::now_unix().max(max_existing + 1)
        };
        conn.execute(
            "INSERT INTO tbl_billadm_stock_fund_record \
             (id, ledger_id, record_date, event_type, event_text, amount_change, cash_balance, \
              net_pnl, remark, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                record.id,
                record.ledger_id,
                record.record_date,
                record.event_type,
                record.event_text,
                record.amount_change,
                record.cash_balance,
                record.net_pnl,
                record.remark,
                created_at,
            ],
        )?;
        Ok(())
    }

    /// 末条资金记录：`record_date DESC, created_at DESC, id DESC`。
    pub fn query_latest_fund_record(
        conn: &Connection,
        ledger_id: &str,
    ) -> rusqlite::Result<StockFundRecord> {
        conn.query_row(
            &format!(
                "SELECT {FUND_COLUMNS} FROM tbl_billadm_stock_fund_record WHERE ledger_id = ?1 \
                 ORDER BY record_date DESC, created_at DESC, id DESC LIMIT 1"
            ),
            [ledger_id],
            fund_from_row,
        )
    }

    /// 按 id 取一条资金记录；不存在返回 `QueryReturnedNoRows`。
    ///
    /// 回滚「追加本金 / 支取 / 利息归本」时要先读到它才能决定怎么撤（见 `tr-service` 的
    /// `rollback_latest`）：`event_type` 决定要不要动 `principal`，`amount_change` 是要还原的额度。
    pub fn get_fund_record(conn: &Connection, id: &str) -> rusqlite::Result<StockFundRecord> {
        conn.query_row(
            &format!(
                "SELECT {FUND_COLUMNS} FROM tbl_billadm_stock_fund_record WHERE id = ?1 \
                 ORDER BY id LIMIT 1"
            ),
            [id],
            fund_from_row,
        )
    }

    /// 按 id 删一条资金记录（回滚资金类操作；调用方随后要 `recalculate_cash_chain`）。
    pub fn delete_fund_record(conn: &Connection, id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_stock_fund_record WHERE id = ?1",
            [id],
        )?;
        Ok(())
    }

    /// 资金记录分页（`page` 从 1 起，不做边界钳制）。
    pub fn query_fund_records(
        conn: &Connection,
        ledger_id: &str,
        page: i64,
        page_size: i64,
    ) -> rusqlite::Result<(Vec<StockFundRecord>, i64)> {
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tbl_billadm_stock_fund_record WHERE ledger_id = ?1",
            [ledger_id],
            |row| row.get(0),
        )?;

        let mut statement = conn.prepare(&format!(
            "SELECT {FUND_COLUMNS} FROM tbl_billadm_stock_fund_record WHERE ledger_id = ?1 \
             ORDER BY record_date DESC, created_at DESC, id DESC LIMIT ?2 OFFSET ?3"
        ))?;
        let records = statement
            .query_map(
                params![ledger_id, page_size, (page - 1) * page_size],
                fund_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok((records, total))
    }

    /// Σ 卖出净盈亏（`COALESCE(SUM(net_pnl), 0)`）。
    pub fn sum_net_pnl(conn: &Connection, ledger_id: &str) -> rusqlite::Result<i64> {
        conn.query_row(
            "SELECT COALESCE(SUM(net_pnl), 0) FROM tbl_billadm_stock_fund_record \
             WHERE ledger_id = ?1",
            [ledger_id],
            |row| row.get(0),
        )
    }

    /// 累计支取金额：`amount_change` 存负数，取反求和。
    pub fn sum_withdrawn(conn: &Connection, ledger_id: &str) -> rusqlite::Result<i64> {
        conn.query_row(
            "SELECT COALESCE(SUM(-amount_change), 0) FROM tbl_billadm_stock_fund_record \
             WHERE ledger_id = ?1 AND event_type = ?2",
            params![ledger_id, consts::STOCK_EVENT_WITHDRAW],
            |row| row.get(0),
        )
    }

    /// 累计利息归本金额：`amount_change` 存正数，直接求和。
    pub fn sum_interest_principal(conn: &Connection, ledger_id: &str) -> rusqlite::Result<i64> {
        conn.query_row(
            "SELECT COALESCE(SUM(amount_change), 0) FROM tbl_billadm_stock_fund_record \
             WHERE ledger_id = ?1 AND event_type = ?2",
            params![ledger_id, consts::STOCK_EVENT_INTEREST_PRINCIPAL],
            |row| row.get(0),
        )
    }

    /// 当前持仓成本：Σ 未清仓（`quantity > 0`）持仓的总成本。
    pub fn sum_position_cost(conn: &Connection, ledger_id: &str) -> rusqlite::Result<i64> {
        conn.query_row(
            "SELECT COALESCE(SUM(total_cost), 0) FROM tbl_billadm_stock_position \
             WHERE ledger_id = ?1 AND quantity > 0",
            [ledger_id],
            |row| row.get(0),
        )
    }

    pub fn count_fund_records(conn: &Connection, ledger_id: &str) -> rusqlite::Result<i64> {
        conn.query_row(
            "SELECT COUNT(*) FROM tbl_billadm_stock_fund_record WHERE ledger_id = ?1",
            [ledger_id],
            |row| row.get(0),
        )
    }

    /// 按录入顺序（`created_at ASC, id ASC`）返回全部资金记录。
    ///
    /// 重算现金链时**必须**用这个顺序，而不是按日期排序：链条规则是
    /// 「每条记录的前值 = 已存在记录里 (日期 → 创建时间 → ID) 最大一条的余额」，
    /// 补录历史日期的交易时两者结果不同（见 `recalculate_cash_chain`）。
    pub fn list_fund_records_in_insert_order(
        conn: &Connection,
        ledger_id: &str,
    ) -> rusqlite::Result<Vec<StockFundRecord>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {FUND_COLUMNS} FROM tbl_billadm_stock_fund_record WHERE ledger_id = ?1 \
             ORDER BY created_at ASC, id ASC"
        ))?;
        let records = statement
            .query_map([ledger_id], fund_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    /// 清空买卖产生的资金记录，保留本金 / 追加 / 支取记录。
    pub fn delete_trade_fund_records(conn: &Connection, ledger_id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_stock_fund_record WHERE ledger_id = ?1 AND event_type IN (?2, ?3)",
            params![
                ledger_id,
                consts::STOCK_EVENT_BUY,
                consts::STOCK_EVENT_SELL
            ],
        )?;
        Ok(())
    }

    /// 只回写资金记录的现金余额（重算链条使用）。
    pub fn update_fund_record_cash_balance(
        conn: &Connection,
        id: &str,
        cash_balance: i64,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE tbl_billadm_stock_fund_record SET cash_balance = ?2 WHERE id = ?1",
            params![id, cash_balance],
        )?;
        Ok(())
    }

    // ---------- 持仓 ----------

    pub fn get_position(
        conn: &Connection,
        ledger_id: &str,
        stock_code: &str,
    ) -> rusqlite::Result<StockPosition> {
        conn.query_row(
            &format!(
                "SELECT {POSITION_COLUMNS} FROM tbl_billadm_stock_position \
                 WHERE ledger_id = ?1 AND stock_code = ?2 ORDER BY id LIMIT 1"
            ),
            params![ledger_id, stock_code],
            position_from_row,
        )
    }

    pub fn create_position(conn: &Connection, position: &StockPosition) -> rusqlite::Result<()> {
        let now = crate::util::now_unix();
        conn.execute(
            "INSERT INTO tbl_billadm_stock_position \
             (id, ledger_id, stock_code, stock_name, quantity, total_cost, realized_pnl, review, \
              created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![
                position.id,
                position.ledger_id,
                position.stock_code,
                position.stock_name,
                position.quantity,
                position.total_cost,
                position.realized_pnl,
                position.review,
                now,
            ],
        )?;
        Ok(())
    }

    /// 只更新派生字段（数量 / 成本 / 已实现盈亏 / 名称 / 复盘），不动 `ledger_id`、`stock_code`。
    pub fn update_position(conn: &Connection, position: &StockPosition) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE tbl_billadm_stock_position SET quantity = ?2, total_cost = ?3, \
             realized_pnl = ?4, stock_name = ?5, review = ?6, updated_at = ?7 WHERE id = ?1",
            params![
                position.id,
                position.quantity,
                position.total_cost,
                position.realized_pnl,
                position.stock_name,
                position.review,
                crate::util::now_unix(),
            ],
        )?;
        Ok(())
    }

    /// 某账本全部持仓：`quantity DESC, created_at ASC`（已清仓的也会返回，由服务层过滤）。
    pub fn list_positions(
        conn: &Connection,
        ledger_id: &str,
    ) -> rusqlite::Result<Vec<StockPosition>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {POSITION_COLUMNS} FROM tbl_billadm_stock_position WHERE ledger_id = ?1 \
             ORDER BY quantity DESC, created_at ASC"
        ))?;
        let positions = statement
            .query_map([ledger_id], position_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(positions)
    }

    // ---------- 交易 ----------

    pub fn create_trade(conn: &Connection, trade: &StockTrade) -> rusqlite::Result<()> {
        // 只 INSERT 非零值字段：零值列退回 SQL 默认值，
        // 非空 `remark` 会照写；空备注写 '' 与列默认值等价。
        conn.execute(
            "INSERT INTO tbl_billadm_stock_trade \
             (id, ledger_id, stock_code, stock_name, trade_type, round_id, order_id, order_seq, \
              price, lots, shares, amount, fee, commission, stamp_duty, transfer_fee, realized_pnl, \
              trade_time, remark, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, \
              ?18, ?19, ?20)",
            params![
                trade.id,
                trade.ledger_id,
                trade.stock_code,
                trade.stock_name,
                trade.trade_type,
                trade.round_id,
                trade.order_id,
                trade.order_seq,
                trade.price,
                trade.lots,
                trade.shares,
                trade.amount,
                trade.fee,
                trade.commission,
                trade.stamp_duty,
                trade.transfer_fee,
                trade.realized_pnl,
                trade.trade_time,
                trade.remark,
                crate::util::now_unix(),
            ],
        )?;
        Ok(())
    }

    /// 某股全部交易（倒序）：`trade_time DESC, created_at DESC`。
    pub fn list_trades(
        conn: &Connection,
        ledger_id: &str,
        stock_code: &str,
    ) -> rusqlite::Result<Vec<StockTrade>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {TRADE_COLUMNS} FROM tbl_billadm_stock_trade \
             WHERE ledger_id = ?1 AND stock_code = ?2 ORDER BY trade_time DESC, created_at DESC"
        ))?;
        let trades = statement
            .query_map(params![ledger_id, stock_code], trade_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(trades)
    }

    /// 某股全部交易（升序，历史回填 / 轮次归并使用）。
    pub fn list_trades_asc(
        conn: &Connection,
        ledger_id: &str,
        stock_code: &str,
    ) -> rusqlite::Result<Vec<StockTrade>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {TRADE_COLUMNS} FROM tbl_billadm_stock_trade \
             WHERE ledger_id = ?1 AND stock_code = ?2 \
             ORDER BY trade_time ASC, created_at ASC, id ASC"
        ))?;
        let trades = statement
            .query_map(params![ledger_id, stock_code], trade_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(trades)
    }

    /// 整个账本的全部交易（升序，重放重建使用）。
    ///
    /// 同一委托内的多笔成交按 `order_seq` 排序，保证重放顺序确定。
    pub fn list_all_trades_asc(
        conn: &Connection,
        ledger_id: &str,
    ) -> rusqlite::Result<Vec<StockTrade>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {TRADE_COLUMNS} FROM tbl_billadm_stock_trade WHERE ledger_id = ?1 \
             ORDER BY trade_time ASC, created_at ASC, order_seq ASC, id ASC"
        ))?;
        let trades = statement
            .query_map([ledger_id], trade_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(trades)
    }

    pub fn get_trade(conn: &Connection, trade_id: &str) -> rusqlite::Result<StockTrade> {
        conn.query_row(
            &format!(
                "SELECT {TRADE_COLUMNS} FROM tbl_billadm_stock_trade WHERE id = ?1 \
                 ORDER BY id LIMIT 1"
            ),
            [trade_id],
            trade_from_row,
        )
    }

    /// 同一委托下的全部成交明细，按 `order_seq` 升序。
    pub fn list_trades_by_order(
        conn: &Connection,
        ledger_id: &str,
        order_id: &str,
    ) -> rusqlite::Result<Vec<StockTrade>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {TRADE_COLUMNS} FROM tbl_billadm_stock_trade \
             WHERE ledger_id = ?1 AND order_id = ?2 ORDER BY order_seq ASC, created_at ASC, id ASC"
        ))?;
        let trades = statement
            .query_map(params![ledger_id, order_id], trade_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(trades)
    }

    pub fn delete_trades_by_order(
        conn: &Connection,
        ledger_id: &str,
        order_id: &str,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_stock_trade WHERE ledger_id = ?1 AND order_id = ?2",
            params![ledger_id, order_id],
        )?;
        Ok(())
    }

    /// 按 ID 集合删除交易（空集合直接返回）。
    pub fn delete_trades_by_ids(conn: &Connection, ids: &[String]) -> rusqlite::Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let placeholders = vec!["?"; ids.len()].join(",");
        conn.execute(
            &format!("DELETE FROM tbl_billadm_stock_trade WHERE id IN ({placeholders})"),
            params_from_iter(ids.iter()),
        )?;
        Ok(())
    }

    /// 更新成交本身（编辑成交时按当前费用设置重算后落库）。
    pub fn update_trade(conn: &Connection, trade: &StockTrade) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE tbl_billadm_stock_trade SET trade_type = ?2, round_id = ?3, order_id = ?4, \
             order_seq = ?5, price = ?6, lots = ?7, shares = ?8, amount = ?9, fee = ?10, \
             commission = ?11, stamp_duty = ?12, transfer_fee = ?13, realized_pnl = ?14, \
             trade_time = ?15 WHERE id = ?1",
            params![
                trade.id,
                trade.trade_type,
                trade.round_id,
                trade.order_id,
                trade.order_seq,
                trade.price,
                trade.lots,
                trade.shares,
                trade.amount,
                trade.fee,
                trade.commission,
                trade.stamp_duty,
                trade.transfer_fee,
                trade.realized_pnl,
                trade.trade_time,
            ],
        )?;
        Ok(())
    }

    /// 只回写重放派生字段（轮次挂接与已实现盈亏），不触碰成交本身。
    pub fn update_trade_settlement(
        conn: &Connection,
        trade_id: &str,
        round_id: &str,
        realized_pnl: Option<i64>,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE tbl_billadm_stock_trade SET round_id = ?2, realized_pnl = ?3 WHERE id = ?1",
            params![trade_id, round_id, realized_pnl],
        )?;
        Ok(())
    }

    // ---------- 交易历史集合 ----------

    pub fn get_trade_history(
        conn: &Connection,
        ledger_id: &str,
        stock_code: &str,
    ) -> rusqlite::Result<StockTradeHistory> {
        conn.query_row(
            &format!(
                "SELECT {HISTORY_COLUMNS} FROM tbl_billadm_stock_trade_history \
                 WHERE ledger_id = ?1 AND stock_code = ?2 ORDER BY id LIMIT 1"
            ),
            params![ledger_id, stock_code],
            history_from_row,
        )
    }

    pub fn create_trade_history(
        conn: &Connection,
        history: &StockTradeHistory,
    ) -> rusqlite::Result<()> {
        let now = crate::util::now_unix();
        conn.execute(
            "INSERT INTO tbl_billadm_stock_trade_history \
             (id, ledger_id, stock_code, stock_name, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![
                history.id,
                history.ledger_id,
                history.stock_code,
                history.stock_name,
                now,
            ],
        )?;
        Ok(())
    }

    /// 列表按 `updated_at DESC, created_at DESC`。
    pub fn list_trade_histories(
        conn: &Connection,
        ledger_id: &str,
    ) -> rusqlite::Result<Vec<StockTradeHistory>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {HISTORY_COLUMNS} FROM tbl_billadm_stock_trade_history WHERE ledger_id = ?1 \
             ORDER BY updated_at DESC, created_at DESC"
        ))?;
        let histories = statement
            .query_map([ledger_id], history_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(histories)
    }

    /// 清空账本的历史集合（重放重建时按交易流重新生成）。
    pub fn delete_trade_histories_by_ledger(
        conn: &Connection,
        ledger_id: &str,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_stock_trade_history WHERE ledger_id = ?1",
            [ledger_id],
        )?;
        Ok(())
    }

    pub fn update_trade_history_name(
        conn: &Connection,
        ledger_id: &str,
        stock_code: &str,
        stock_name: &str,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE tbl_billadm_stock_trade_history SET stock_name = ?3, updated_at = ?4 \
             WHERE ledger_id = ?1 AND stock_code = ?2",
            params![ledger_id, stock_code, stock_name, crate::util::now_unix()],
        )?;
        Ok(())
    }

    /// 存在交易记录的股票代码列表（不做排序，历史回填使用；SQL 是去重的 `stock_code` 列表）。
    pub fn list_trade_stocks(conn: &Connection, ledger_id: &str) -> rusqlite::Result<Vec<String>> {
        let mut statement = conn.prepare(
            "SELECT DISTINCT stock_code FROM tbl_billadm_stock_trade WHERE ledger_id = ?1",
        )?;
        let codes = statement
            .query_map([ledger_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(codes)
    }

    // ---------- 轮次 ----------

    pub fn count_trade_rounds(conn: &Connection, history_id: &str) -> rusqlite::Result<i64> {
        conn.query_row(
            "SELECT COUNT(*) FROM tbl_billadm_stock_trade_round WHERE history_id = ?1",
            [history_id],
            |row| row.get(0),
        )
    }

    pub fn create_trade_round(conn: &Connection, round: &StockTradeRound) -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO tbl_billadm_stock_trade_round \
             (id, ledger_id, stock_code, history_id, round_no, opened_at, closed_at, tag, review, \
              created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                round.id,
                round.ledger_id,
                round.stock_code,
                round.history_id,
                round.round_no,
                round.opened_at,
                round.closed_at,
                round.tag,
                round.review,
                crate::util::now_unix(),
            ],
        )?;
        Ok(())
    }

    /// 某股全部轮次：`round_no ASC`。
    pub fn list_trade_rounds_by_stock(
        conn: &Connection,
        ledger_id: &str,
        stock_code: &str,
    ) -> rusqlite::Result<Vec<StockTradeRound>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {ROUND_COLUMNS} FROM tbl_billadm_stock_trade_round \
             WHERE ledger_id = ?1 AND stock_code = ?2 ORDER BY round_no ASC"
        ))?;
        let rounds = statement
            .query_map(params![ledger_id, stock_code], round_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rounds)
    }

    /// 整个账本的全部轮次（重放重建时用于保留标签与复盘）：`stock_code ASC, round_no ASC`。
    pub fn list_trade_rounds(
        conn: &Connection,
        ledger_id: &str,
    ) -> rusqlite::Result<Vec<StockTradeRound>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {ROUND_COLUMNS} FROM tbl_billadm_stock_trade_round WHERE ledger_id = ?1 \
             ORDER BY stock_code ASC, round_no ASC"
        ))?;
        let rounds = statement
            .query_map([ledger_id], round_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rounds)
    }

    pub fn delete_trade_round(conn: &Connection, round_id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_stock_trade_round WHERE id = ?1",
            [round_id],
        )?;
        Ok(())
    }

    /// 回写轮次的派生字段（历史集合与起止时间），标签与复盘保持不变。
    pub fn update_trade_round_derived(
        conn: &Connection,
        round_id: &str,
        history_id: &str,
        opened_at: i64,
        closed_at: i64,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE tbl_billadm_stock_trade_round SET history_id = ?2, opened_at = ?3, \
             closed_at = ?4 WHERE id = ?1",
            params![round_id, history_id, opened_at, closed_at],
        )?;
        Ok(())
    }

    pub fn get_trade_round(conn: &Connection, round_id: &str) -> rusqlite::Result<StockTradeRound> {
        conn.query_row(
            &format!(
                "SELECT {ROUND_COLUMNS} FROM tbl_billadm_stock_trade_round WHERE id = ?1 \
                 ORDER BY id LIMIT 1"
            ),
            [round_id],
            round_from_row,
        )
    }

    pub fn update_trade_round_review(
        conn: &Connection,
        round_id: &str,
        review: &str,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE tbl_billadm_stock_trade_round SET review = ?2 WHERE id = ?1",
            params![round_id, review],
        )?;
        Ok(())
    }

    pub fn update_trade_round_tag(
        conn: &Connection,
        round_id: &str,
        tag: &str,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE tbl_billadm_stock_trade_round SET tag = ?2 WHERE id = ?1",
            params![round_id, tag],
        )?;
        Ok(())
    }

    /// 某轮次的全部交易：`trade_time ASC, created_at ASC, id ASC`。
    pub fn list_trades_by_round(
        conn: &Connection,
        round_id: &str,
    ) -> rusqlite::Result<Vec<StockTrade>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {TRADE_COLUMNS} FROM tbl_billadm_stock_trade WHERE round_id = ?1 \
             ORDER BY trade_time ASC, created_at ASC, id ASC"
        ))?;
        let trades = statement
            .query_map([round_id], trade_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(trades)
    }

    /// 某股尚未挂接轮次的最小成交时间；全部已挂接时返回 0。
    pub fn min_unattached_trade_time(
        conn: &Connection,
        ledger_id: &str,
        stock_code: &str,
    ) -> rusqlite::Result<i64> {
        conn.query_row(
            &format!(
                "SELECT COALESCE(MIN(trade_time), 0) FROM tbl_billadm_stock_trade \
                 WHERE ledger_id = ?1 AND stock_code = ?2 AND {UNATTACHED_ROUND_SQL}"
            ),
            params![ledger_id, stock_code],
            |row| row.get(0),
        )
    }

    /// 把某股全部未挂接的交易挂到指定轮次（清仓时收尾）。
    pub fn attach_unattached_trades(
        conn: &Connection,
        ledger_id: &str,
        stock_code: &str,
        round_id: &str,
    ) -> rusqlite::Result<()> {
        conn.execute(
            &format!(
                "UPDATE tbl_billadm_stock_trade SET round_id = ?3 \
                 WHERE ledger_id = ?1 AND stock_code = ?2 AND {UNATTACHED_ROUND_SQL}"
            ),
            params![ledger_id, stock_code, round_id],
        )?;
        Ok(())
    }

    /// 批量把指定交易挂到轮次（历史回填使用；空集合直接返回）。
    pub fn update_trades_round_id(
        conn: &Connection,
        round_id: &str,
        ids: &[String],
    ) -> rusqlite::Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let placeholders = vec!["?"; ids.len()].join(",");
        conn.execute(
            &format!(
                "UPDATE tbl_billadm_stock_trade SET round_id = ? WHERE id IN ({placeholders})"
            ),
            params_from_iter(std::iter::once(round_id).chain(ids.iter().map(String::as_str))),
        )?;
        Ok(())
    }

    /// 从已有交易记录查询股票名称（**跨账本**，按最近成交优先）。
    pub fn query_stock_name(conn: &Connection, stock_code: &str) -> rusqlite::Result<String> {
        let name: Option<String> = conn.query_row(
            "SELECT stock_name FROM tbl_billadm_stock_trade WHERE stock_code = ?1 \
             ORDER BY created_at DESC, id DESC LIMIT 1",
            [stock_code],
            |row| row.get(0),
        )?;
        // 空名称按"查无记录"处理，让上层回退到外部行情接口。
        match name {
            Some(name) if !name.is_empty() => Ok(name),
            _ => Err(rusqlite::Error::QueryReturnedNoRows),
        }
    }

    // ---------- 操作记录（回滚用） ----------

    /// 新增一条操作记录，并把该账本裁剪到最近 [`STOCK_OPERATION_LIMIT`] 条。
    ///
    /// **`created_at` 单调递增**：与 `create_fund_record` 同一条 idiom —— 秒级时间戳在同一秒内
    /// 会并列，而「最新一次操作」完全依赖这个顺序，并列时排序会落到随机 UUID 上、不确定。
    /// 所以取 `MAX(now, 本账本已有最大值 + 1)`。
    ///
    /// 裁剪与插入在**同一次调用**里完成：调用方都在事务中，超限的旧记录随本次操作一起提交/回滚。
    pub fn create_operation(conn: &Connection, operation: &StockOperation) -> rusqlite::Result<()> {
        let max_existing: i64 = conn.query_row(
            "SELECT COALESCE(MAX(created_at), 0) FROM tbl_billadm_stock_operation \
             WHERE ledger_id = ?1",
            [&operation.ledger_id],
            |row| row.get(0),
        )?;
        let created_at = crate::util::now_unix().max(max_existing + 1);
        conn.execute(
            "INSERT INTO tbl_billadm_stock_operation \
             (id, ledger_id, kind, action, detail, target_id, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                operation.id,
                operation.ledger_id,
                operation.kind,
                operation.action,
                operation.detail,
                operation.target_id,
                created_at,
            ],
        )?;
        // 只留最新的 N 条；`created_at` 严格递增，所以"更旧的"就是 created_at 更小的那些
        conn.execute(
            "DELETE FROM tbl_billadm_stock_operation \
              WHERE ledger_id = ?1 AND id NOT IN (\
                    SELECT id FROM tbl_billadm_stock_operation WHERE ledger_id = ?1 \
                     ORDER BY created_at DESC, id DESC LIMIT ?2)",
            params![operation.ledger_id, STOCK_OPERATION_LIMIT],
        )?;
        Ok(())
    }

    /// 操作记录列表：最新的在前。
    pub fn list_operations(
        conn: &Connection,
        ledger_id: &str,
        limit: i64,
    ) -> rusqlite::Result<Vec<StockOperation>> {
        let mut statement = conn.prepare(&format!(
            "SELECT {OPERATION_COLUMNS} FROM tbl_billadm_stock_operation WHERE ledger_id = ?1 \
             ORDER BY created_at DESC, id DESC LIMIT ?2"
        ))?;
        let rows = statement
            .query_map(params![ledger_id, limit], operation_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 最新一条操作记录；没有记录时返回 `QueryReturnedNoRows`。
    pub fn latest_operation(
        conn: &Connection,
        ledger_id: &str,
    ) -> rusqlite::Result<StockOperation> {
        conn.query_row(
            &format!(
                "SELECT {OPERATION_COLUMNS} FROM tbl_billadm_stock_operation WHERE ledger_id = ?1 \
                 ORDER BY created_at DESC, id DESC LIMIT 1"
            ),
            [ledger_id],
            operation_from_row,
        )
    }

    /// 删一条操作记录（回滚成功后把它弹掉）。
    pub fn delete_operation(conn: &Connection, id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM tbl_billadm_stock_operation WHERE id = ?1",
            [id],
        )?;
        Ok(())
    }

    // ---------- 删除与重置 ----------

    /// 「重置」：在**单个事务**里清空某账本的全部股票表。
    ///
    /// 逐表 `DELETE` 清空；历史遗留表 `tbl_billadm_stock_journal`（已下线功能）
    /// 在当前 schema 里已不存在，且本仓库不做迁移，因此这里不探测也不删除它。
    pub fn reset_by_ledger_id(conn: &Connection, ledger_id: &str) -> rusqlite::Result<()> {
        for table in STOCK_TABLES {
            conn.execute(
                &format!("DELETE FROM {table} WHERE ledger_id = ?1"),
                [ledger_id],
            )?;
        }
        Ok(())
    }
}

fn account_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StockAccount> {
    Ok(StockAccount {
        id: row.get(0)?,
        ledger_id: row.get(1)?,
        principal: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

fn fee_setting_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StockFeeSetting> {
    Ok(StockFeeSetting {
        id: row.get(0)?,
        ledger_id: row.get(1)?,
        commission_rate: row.get(2)?,
        min_commission: row.get(3)?,
        stamp_duty_rate: row.get(4)?,
        transfer_fee_rate: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn tag_setting_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StockTradeTagSetting> {
    Ok(StockTradeTagSetting {
        id: row.get(0)?,
        ledger_id: row.get(1)?,
        tags: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

fn fund_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StockFundRecord> {
    Ok(StockFundRecord {
        id: row.get(0)?,
        ledger_id: row.get(1)?,
        record_date: row.get(2)?,
        event_type: row.get(3)?,
        event_text: row.get(4)?,
        amount_change: row.get(5)?,
        cash_balance: row.get(6)?,
        net_pnl: row.get(7)?,
        remark: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
        created_at: row.get(9)?,
    })
}

fn operation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StockOperation> {
    Ok(StockOperation {
        id: row.get(0)?,
        ledger_id: row.get(1)?,
        kind: row.get(2)?,
        action: row.get(3)?,
        detail: row.get(4)?,
        target_id: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        created_at: row.get(6)?,
    })
}

fn position_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StockPosition> {
    Ok(StockPosition {
        id: row.get(0)?,
        ledger_id: row.get(1)?,
        stock_code: row.get(2)?,
        stock_name: row.get(3)?,
        quantity: row.get(4)?,
        total_cost: row.get(5)?,
        realized_pnl: row.get(6)?,
        review: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn trade_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StockTrade> {
    Ok(StockTrade {
        id: row.get(0)?,
        ledger_id: row.get(1)?,
        stock_code: row.get(2)?,
        stock_name: row.get(3)?,
        trade_type: row.get(4)?,
        round_id: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        order_id: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
        order_seq: row.get(7)?,
        price: row.get(8)?,
        lots: row.get(9)?,
        shares: row.get(10)?,
        amount: row.get(11)?,
        fee: row.get(12)?,
        commission: row.get(13)?,
        stamp_duty: row.get(14)?,
        transfer_fee: row.get(15)?,
        realized_pnl: row.get(16)?,
        trade_time: row.get(17)?,
        remark: row.get::<_, Option<String>>(18)?.unwrap_or_default(),
        created_at: row.get(19)?,
    })
}

fn history_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StockTradeHistory> {
    Ok(StockTradeHistory {
        id: row.get(0)?,
        ledger_id: row.get(1)?,
        stock_code: row.get(2)?,
        stock_name: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn round_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StockTradeRound> {
    Ok(StockTradeRound {
        id: row.get(0)?,
        ledger_id: row.get(1)?,
        stock_code: row.get(2)?,
        history_id: row.get(3)?,
        round_no: row.get(4)?,
        opened_at: row.get(5)?,
        closed_at: row.get(6)?,
        tag: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        review: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
        created_at: row.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(ledger_id: &str, principal: i64) -> StockAccount {
        StockAccount {
            id: crate::util::new_uuid(),
            ledger_id: ledger_id.to_string(),
            principal,
            ..StockAccount::default()
        }
    }

    fn trade(id: &str, order_id: &str, order_seq: i64, trade_time: i64) -> StockTrade {
        StockTrade {
            id: id.to_string(),
            ledger_id: "l1".to_string(),
            stock_code: "600000".to_string(),
            stock_name: "浦发银行".to_string(),
            trade_type: consts::STOCK_TRADE_OPEN.to_string(),
            order_id: order_id.to_string(),
            order_seq,
            price: 1000,
            lots: 10,
            shares: 1000,
            amount: 1_000_000,
            trade_time,
            remark: "不应落库的备注".to_string(),
            ..StockTrade::default()
        }
    }

    #[test]
    fn account_roundtrip_and_missing_row() {
        let (workspace, dir) = crate::dao::test_workspace("dao-stock-account");
        let conn = workspace.connection();

        StockDao::create_account(&conn, &account("l1", 10_000_000)).unwrap();
        let loaded = StockDao::get_account(&conn, "l1").unwrap();
        assert_eq!(loaded.principal, 10_000_000);
        assert!(loaded.created_at > 0);
        assert!(super::super::is_not_found(
            &StockDao::get_account(&conn, "absent").unwrap_err()
        ));

        StockDao::update_account_principal(&conn, "l1", 20_000_000).unwrap();
        assert_eq!(
            StockDao::get_account(&conn, "l1").unwrap().principal,
            20_000_000
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn trade_create_persists_remark_like_go() {
        let (workspace, dir) = crate::dao::test_workspace("dao-stock-trade-remark");
        let conn = workspace.connection();

        StockDao::create_trade(&conn, &trade("t1", "o1", 1, 100)).unwrap();
        let loaded = StockDao::get_trade(&conn, "t1").unwrap();
        // 只 INSERT 非零值字段：零值列退回 SQL 默认值；非空备注照写
        // （这条差异曾经被落库回归对比抓到过，不要改回去）
        assert_eq!(loaded.remark, "不应落库的备注");
        assert!(loaded.created_at > 0);
        assert_eq!(loaded.realized_pnl, None);

        // 空备注与列默认值 '' 等价
        let mut blank = trade("t2", "o2", 1, 200);
        blank.remark = String::new();
        StockDao::create_trade(&conn, &blank).unwrap();
        assert_eq!(StockDao::get_trade(&conn, "t2").unwrap().remark, "");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 池化连接必须能看到**其它连接**刚提交的写入。
    ///
    /// 落库回归对比里 `rebuild_trades` 读到的是上一次写入前的快照（少一行），
    /// 导致「卖出数量超过持仓」——根因是 WAL 下连接被复用后仍固定旧快照。
    #[test]
    fn pooled_connection_sees_writes_from_other_connections() {
        let (workspace, dir) = crate::dao::test_workspace("dao-stock-read-your-writes");
        let reader = workspace.connection();

        for round in 0..4 {
            let before = StockDao::list_all_trades_asc(&reader, "l1").unwrap().len();
            assert_eq!(before, round, "第 {round} 轮读取旧快照");
            {
                let writer = workspace.connection();
                StockDao::create_trade(
                    &writer,
                    &trade(&format!("t{round}"), &format!("o{round}"), 1, 100),
                )
                .unwrap();
            }
            assert_eq!(
                StockDao::list_all_trades_asc(&reader, "l1").unwrap().len(),
                round + 1,
                "第 {round} 轮 reader 读到了旧快照"
            );
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn trade_order_ordering_uses_order_seq() {
        let (workspace, dir) = crate::dao::test_workspace("dao-stock-trade-order");
        let conn = workspace.connection();

        StockDao::create_trade(&conn, &trade("t2", "o1", 2, 100)).unwrap();
        StockDao::create_trade(&conn, &trade("t1", "o1", 1, 100)).unwrap();
        StockDao::create_trade(&conn, &trade("t3", "o2", 1, 200)).unwrap();

        let order = StockDao::list_trades_by_order(&conn, "l1", "o1").unwrap();
        assert_eq!(
            order.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec!["t1", "t2"]
        );
        // 全部交易升序：trade_time → created_at → order_seq → id
        let all = StockDao::list_all_trades_asc(&conn, "l1").unwrap();
        assert_eq!(
            all.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec!["t1", "t2", "t3"]
        );

        StockDao::delete_trades_by_order(&conn, "l1", "o1").unwrap();
        assert_eq!(StockDao::list_all_trades_asc(&conn, "l1").unwrap().len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fund_record_created_at_is_monotonic_per_ledger() {
        let (workspace, dir) = crate::dao::test_workspace("dao-stock-fund-monotonic");
        let conn = workspace.connection();

        for index in 0..3 {
            let record = StockFundRecord {
                id: format!("f{index}"),
                ledger_id: "l1".to_string(),
                record_date: "2026-01-01".to_string(),
                event_type: consts::STOCK_EVENT_ADD_PRINCIPAL.to_string(),
                event_text: String::new(),
                amount_change: 100,
                cash_balance: 0,
                net_pnl: None,
                remark: String::new(),
                created_at: 0,
            };
            StockDao::create_fund_record(&conn, &record).unwrap();
        }
        // 另一个账本互不影响
        StockDao::create_fund_record(
            &conn,
            &StockFundRecord {
                id: "other".to_string(),
                ledger_id: "l2".to_string(),
                record_date: "2026-01-01".to_string(),
                event_type: consts::STOCK_EVENT_ADD_PRINCIPAL.to_string(),
                event_text: String::new(),
                amount_change: 100,
                cash_balance: 0,
                net_pnl: None,
                remark: String::new(),
                created_at: 0,
            },
        )
        .unwrap();

        let ordered = StockDao::list_fund_records_in_insert_order(&conn, "l1").unwrap();
        assert_eq!(
            ordered.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["f0", "f1", "f2"],
            "created_at 必须严格递增，录入顺序才确定"
        );
        let stamps: Vec<i64> = ordered.iter().map(|r| r.created_at).collect();
        assert!(stamps[0] < stamps[1] && stamps[1] < stamps[2], "{stamps:?}");

        // 显式给出的 created_at（重放路径）原样保留
        StockDao::create_fund_record(
            &conn,
            &StockFundRecord {
                id: "replay".to_string(),
                ledger_id: "l1".to_string(),
                record_date: "2026-01-01".to_string(),
                event_type: consts::STOCK_EVENT_BUY.to_string(),
                event_text: String::new(),
                amount_change: -100,
                cash_balance: 0,
                net_pnl: None,
                remark: String::new(),
                created_at: 12345,
            },
        )
        .unwrap();
        assert_eq!(
            StockDao::list_fund_records_in_insert_order(&conn, "l1")
                .unwrap()
                .iter()
                .find(|r| r.id == "replay")
                .unwrap()
                .created_at,
            12345
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fund_record_chain_helpers() {
        let (workspace, dir) = crate::dao::test_workspace("dao-stock-fund");
        let conn = workspace.connection();
        let record =
            |id: &str, date: &str, event: &str, change: i64, balance: i64| StockFundRecord {
                id: id.to_string(),
                ledger_id: "l1".to_string(),
                record_date: date.to_string(),
                event_type: event.to_string(),
                event_text: event.to_string(),
                amount_change: change,
                cash_balance: balance,
                net_pnl: None,
                remark: String::new(),
                created_at: 0,
            };

        StockDao::create_fund_record(
            &conn,
            &record("f1", "2026-01-05", "add_principal", 100, 1100),
        )
        .unwrap();
        StockDao::create_fund_record(&conn, &record("f2", "2026-01-10", "withdraw", -50, 1050))
            .unwrap();
        StockDao::create_fund_record(&conn, &record("f3", "2026-01-10", "buy", -1000, 50)).unwrap();
        StockDao::create_fund_record(
            &conn,
            &record(
                "f4",
                "2026-01-11",
                consts::STOCK_EVENT_INTEREST_PRINCIPAL,
                25,
                75,
            ),
        )
        .unwrap();

        // 末条：日期最大，同日按 created_at DESC, id DESC
        let latest = StockDao::query_latest_fund_record(&conn, "l1").unwrap();
        assert_eq!(latest.id, "f4");

        assert_eq!(StockDao::count_fund_records(&conn, "l1").unwrap(), 4);
        assert_eq!(StockDao::sum_withdrawn(&conn, "l1").unwrap(), 50);
        assert_eq!(StockDao::sum_net_pnl(&conn, "l1").unwrap(), 0);
        // 利息归本取正数求和；不并入「累计支取」
        assert_eq!(StockDao::sum_interest_principal(&conn, "l1").unwrap(), 25);

        let (page, total) = StockDao::query_fund_records(&conn, "l1", 1, 2).unwrap();
        assert_eq!(total, 4);
        assert_eq!(page.len(), 2);
        let (page2, _) = StockDao::query_fund_records(&conn, "l1", 2, 2).unwrap();
        assert_eq!(page2.len(), 2);

        // 录入顺序：created_at ASC, id ASC
        let ordered = StockDao::list_fund_records_in_insert_order(&conn, "l1").unwrap();
        assert_eq!(
            ordered.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["f1", "f2", "f3", "f4"]
        );

        StockDao::update_fund_record_cash_balance(&conn, "f3", 999).unwrap();
        assert_eq!(
            StockDao::list_fund_records_in_insert_order(&conn, "l1")
                .unwrap()
                .iter()
                .find(|r| r.id == "f3")
                .unwrap()
                .cash_balance,
            999
        );

        // 只删买卖记录，保留本金/支取/利息归本
        StockDao::delete_trade_fund_records(&conn, "l1").unwrap();
        assert_eq!(StockDao::count_fund_records(&conn, "l1").unwrap(), 3);
        assert_eq!(StockDao::sum_interest_principal(&conn, "l1").unwrap(), 25);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn position_helpers_and_cost_sum() {
        let (workspace, dir) = crate::dao::test_workspace("dao-stock-position");
        let conn = workspace.connection();

        let position = StockPosition {
            id: crate::util::new_uuid(),
            ledger_id: "l1".to_string(),
            stock_code: "600000".to_string(),
            stock_name: "浦发银行".to_string(),
            quantity: 1000,
            total_cost: 1_000_510,
            realized_pnl: 0,
            review: "本轮草稿".to_string(),
            ..StockPosition::default()
        };
        StockDao::create_position(&conn, &position).unwrap();
        assert_eq!(StockDao::sum_position_cost(&conn, "l1").unwrap(), 1_000_510);

        let mut updated = position.clone();
        updated.quantity = 0;
        updated.total_cost = 0;
        updated.realized_pnl = 12345;
        updated.review = String::new();
        StockDao::update_position(&conn, &updated).unwrap();

        let loaded = StockDao::get_position(&conn, "l1", "600000").unwrap();
        assert_eq!(loaded.quantity, 0);
        assert_eq!(loaded.realized_pnl, 12345);
        assert_eq!(loaded.stock_name, "浦发银行", "更新不应丢掉名称");
        // 已清仓不计入持仓成本
        assert_eq!(StockDao::sum_position_cost(&conn, "l1").unwrap(), 0);

        // 列表排序 quantity DESC, created_at ASC
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn round_and_history_helpers() {
        let (workspace, dir) = crate::dao::test_workspace("dao-stock-round");
        let conn = workspace.connection();

        let history = StockTradeHistory {
            id: "h1".to_string(),
            ledger_id: "l1".to_string(),
            stock_code: "600000".to_string(),
            stock_name: "浦发银行".to_string(),
            ..StockTradeHistory::default()
        };
        StockDao::create_trade_history(&conn, &history).unwrap();
        assert_eq!(
            StockDao::get_trade_history(&conn, "l1", "600000")
                .unwrap()
                .id,
            "h1"
        );

        let round = StockTradeRound {
            id: "r1".to_string(),
            ledger_id: "l1".to_string(),
            stock_code: "600000".to_string(),
            history_id: "h1".to_string(),
            round_no: 1,
            opened_at: 100,
            closed_at: 200,
            tag: consts::STOCK_TAG_ANALYSIS.to_string(),
            review: String::new(),
            ..StockTradeRound::default()
        };
        StockDao::create_trade_round(&conn, &round).unwrap();
        assert_eq!(StockDao::count_trade_rounds(&conn, "h1").unwrap(), 1);

        StockDao::update_trade_round_review(&conn, "r1", "复盘").unwrap();
        StockDao::update_trade_round_tag(&conn, "r1", consts::STOCK_TAG_DABAN).unwrap();
        let loaded = StockDao::get_trade_round(&conn, "r1").unwrap();
        assert_eq!(loaded.review, "复盘");
        assert_eq!(loaded.tag, consts::STOCK_TAG_DABAN);

        // 派生回写不动标签与复盘
        StockDao::update_trade_round_derived(&conn, "r1", "h1", 111, 222).unwrap();
        let loaded = StockDao::get_trade_round(&conn, "r1").unwrap();
        assert_eq!((loaded.opened_at, loaded.closed_at), (111, 222));
        assert_eq!(loaded.tag, consts::STOCK_TAG_DABAN);
        assert_eq!(loaded.review, "复盘");

        // 挂接未归档交易
        StockDao::create_trade(&conn, &trade("t1", "o1", 1, 100)).unwrap();
        assert_eq!(
            StockDao::min_unattached_trade_time(&conn, "l1", "600000").unwrap(),
            100
        );
        StockDao::attach_unattached_trades(&conn, "l1", "600000", "r1").unwrap();
        assert_eq!(
            StockDao::min_unattached_trade_time(&conn, "l1", "600000").unwrap(),
            0
        );
        assert_eq!(
            StockDao::list_trades_by_round(&conn, "r1").unwrap()[0].round_id,
            "r1"
        );

        StockDao::update_trades_round_id(&conn, "r1", &["t1".to_string()]).unwrap();
        // 空列表是空操作
        StockDao::update_trades_round_id(&conn, "r1", &[]).unwrap();
        StockDao::delete_trades_by_ids(&conn, &[]).unwrap();

        StockDao::delete_trade_histories_by_ledger(&conn, "l1").unwrap();
        assert!(super::super::is_not_found(
            &StockDao::get_trade_history(&conn, "l1", "600000").unwrap_err()
        ));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn stock_name_lookup_is_cross_ledger_and_rejects_empty() {
        let (workspace, dir) = crate::dao::test_workspace("dao-stock-name");
        let conn = workspace.connection();

        assert!(super::super::is_not_found(
            &StockDao::query_stock_name(&conn, "600000").unwrap_err()
        ));

        let mut empty_name = trade("t1", "o1", 1, 100);
        empty_name.stock_name = String::new();
        StockDao::create_trade(&conn, &empty_name).unwrap();
        assert!(
            super::super::is_not_found(&StockDao::query_stock_name(&conn, "600000").unwrap_err()),
            "空名称要显式转成未找到"
        );

        let mut named = trade("t2", "o2", 1, 200);
        named.stock_name = "浦发银行".to_string();
        named.ledger_id = "other-ledger".to_string();
        StockDao::create_trade(&conn, &named).unwrap();
        assert_eq!(
            StockDao::query_stock_name(&conn, "600000").unwrap(),
            "浦发银行"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reset_by_ledger_id_clears_only_that_ledger() {
        let (workspace, dir) = crate::dao::test_workspace("dao-stock-reset");
        let conn = workspace.connection();

        for ledger in ["l1", "l2"] {
            StockDao::create_account(&conn, &account(ledger, 100)).unwrap();
            let mut item = trade("t", "o", 1, 100);
            item.id = format!("{ledger}-trade");
            item.ledger_id = ledger.to_string();
            StockDao::create_trade(&conn, &item).unwrap();
        }

        // 活路径：重置账本 l1 后本账本清零、其它账本不受影响
        StockDao::reset_by_ledger_id(&conn, "l1").unwrap();
        assert!(super::super::is_not_found(
            &StockDao::get_account(&conn, "l1").unwrap_err()
        ));
        assert_eq!(StockDao::get_account(&conn, "l2").unwrap().principal, 100);
        assert_eq!(StockDao::list_all_trades_asc(&conn, "l2").unwrap().len(), 1);

        // 反向再验一次：另一账本仍可被单独重置（账本隔离不依赖调用顺序）
        StockDao::reset_by_ledger_id(&conn, "l2").unwrap();
        assert!(super::super::is_not_found(
            &StockDao::get_account(&conn, "l2").unwrap_err()
        ));
        assert!(StockDao::list_all_trades_asc(&conn, "l2")
            .unwrap()
            .is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tag_setting_roundtrip_keeps_json_untouched() {
        let (workspace, dir) = crate::dao::test_workspace("dao-stock-tag-setting");
        let conn = workspace.connection();

        let setting = StockTradeTagSetting {
            id: crate::util::new_uuid(),
            ledger_id: "l1".to_string(),
            tags: r#"["分析","打板"]"#.to_string(),
            ..StockTradeTagSetting::default()
        };
        StockDao::create_trade_tag_setting(&conn, &setting).unwrap();
        assert_eq!(
            StockDao::get_trade_tag_setting(&conn, "l1").unwrap().tags,
            r#"["分析","打板"]"#
        );

        StockDao::update_trade_tag_setting_tags(&conn, "l1", r#"["分析"]"#).unwrap();
        assert_eq!(
            StockDao::get_trade_tag_setting(&conn, "l1").unwrap().tags,
            r#"["分析"]"#
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
