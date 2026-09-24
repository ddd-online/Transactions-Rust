//! 股票域服务：账户与本金、费用设置、交易标签设置、委托下单与重放、
//! 持仓与交易历史、资金记录链、影响预演。
//!
//! 三处最容易踩空的：
//!
//! 1. **费用按委托一次计收**：最低佣金按「委托」而不是按每笔成交收；算法本身在
//!    [`tr_domain::fee`]（`compute_order_fee` / `allocate_order_fee`），本模块只负责
//!    "先算委托级费用 → 再按各笔成交额比例分摊、末笔吃余数"的调用顺序。
//! 2. **持仓重放**：`rebuild_trades` 按成交时间升序重建持仓、买卖资金记录、轮次挂接与
//!    已实现盈亏；轮次按「股票 + 轮次序号」继承 ID/标签/复盘，因此**幂等**且不丢复盘。
//!    倒填日期的交易不会打乱现金链，因为 `recalculate_cash_chain` 复刻的是
//!    「取当时 (日期, 创建时间, ID) 最大一条的余额」这一既有规则，而不是按日期重排。
//! 3. **影响预演**：`preview_trade_change` 在事务里真的执行改动，最后返回哨兵错误
//!    [`ERR_PREVIEW_ROLLBACK`] 强制回滚，外层捕获后返回预演结果——**绝不落库**。
//!    复用 `ServiceError::Internal` 而不是新增变体，是为了不改动 `error.rs`（其他并发任务在用）。

use std::collections::{BTreeMap, HashMap};

use chrono::Datelike;
use tr_domain::consts;
use tr_domain::dto::{
    round_pnl, StockFundRecordDto, StockFundRecordPage, StockNameDto, StockOperationDto,
    StockOperationRollbackDto, StockOperationRollbackPreviewDto, StockOverviewDto,
    StockPositionDto, StockTradeDto, StockTradeHistoryDetailDto, StockTradeHistoryDto,
    StockTradeHistorySummaryDto, StockTradeImpactDto, StockTradeImpactRoundDto,
    StockTradeTagSettingDto,
};
use tr_domain::error::AppError;
use tr_domain::fee;
use tr_domain::models::{
    StockAccount, StockFeeSetting, StockFundRecord, StockOperation, StockPosition, StockTrade,
    StockTradeHistory, StockTradeRound, StockTradeTagSetting,
};
use tr_store::dao::is_not_found;
use tr_store::dao::stock::{StockDao, STOCK_OPERATION_LIMIT};
use tr_store::Workspace;

use crate::error::db;
use crate::quote::{StockQuoteFetcher, TencentStockQuoteFetcher};
use crate::{ServiceError, ServiceResult};

/// 预演专用哨兵错误文案：事务内执行改动后强制回滚，不对外暴露。
/// 哨兵文案（`trade impact preview rollback`）只在内部流转，不对外暴露。
pub const ERR_PREVIEW_ROLLBACK: &str = "trade impact preview rollback";

/// 委托内的一笔成交（价格单位：分/股）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TradeFill {
    pub price_cents: i64,
    pub lots: i64,
}

/// 建仓 / 加仓（买入方向）。
fn is_buy(trade_type: &str) -> bool {
    trade_type == consts::STOCK_TRADE_OPEN || trade_type == consts::STOCK_TRADE_ADD
}

/// 减仓 / 清仓（卖出方向）。
fn is_sell(trade_type: &str) -> bool {
    trade_type == consts::STOCK_TRADE_REDUCE || trade_type == consts::STOCK_TRADE_CLOSE
}

/// 成交所属委托 ID；存量数据未标记委托时以自身为独立委托。
fn order_key_of(trade: &StockTrade) -> String {
    if trade.order_id.is_empty() {
        trade.id.clone()
    } else {
        trade.order_id.clone()
    }
}

/// 轮次索引键：股票 + 轮次序号。
fn round_meta_key(stock_code: &str, round_no: i64) -> String {
    format!("{stock_code}#{round_no}")
}

// ---------- 数值工具 ----------

/// 占比百分比：`round(part / whole * 10000) / 100`（保留两位小数）。
///
/// `whole <= 0` 时返回 `0.0`（防除零）；调用点仍各自保留原有的 `if whole > 0` 守卫，
/// 因此这里的兜底分支不会被走到，只是让这个纯函数的定义域完整。
pub(crate) fn percent_of(part: i64, whole: i64) -> f64 {
    if whole > 0 {
        ((part as f64 / whole as f64) * 10_000.0).round() / 100.0
    } else {
        0.0
    }
}

/// 减仓按剩余总成本的比例结转成本（四舍五入到分），避免整除截断造成已实现盈亏偏差。
fn cost_basis_of(total_cost: i64, shares: i64, quantity: i64) -> i64 {
    (total_cost as f64 * shares as f64 / quantity as f64).round() as i64
}

// ---------- 日期工具 ----------

/// Unix 秒 → `YYYY-MM-DD`。
///
/// 按 **UTC** 格式化，
/// 与 `strftime(_, _, 'unixepoch')` 的 UTC 口径一致。
pub fn unix_to_date(timestamp: i64) -> String {
    match chrono::DateTime::from_timestamp(timestamp, 0) {
        Some(datetime) => datetime.format("%Y-%m-%d").to_string(),
        // 超出 chrono 可表示范围的极端值：退回 Unix 纪元当天，避免 panic
        None => "1970-01-01".to_string(),
    }
}

/// 当前日期 `YYYY-MM-DD`。
fn today() -> String {
    unix_to_date(tr_store::util::now_unix())
}

/// 归一化资金变化的发生日期；空值按当天记录，非法格式报错。
///
/// 只接受严格的 `YYYY-MM-DD`
/// （`2026/01/05` 与 `2026-1-5` 都会被拒绝）。
pub fn normalize_stock_record_date(date: &str) -> ServiceResult<String> {
    if date.is_empty() {
        return Ok(today());
    }
    let (year, month, day) = parse_strict_date(date)
        .ok_or_else(|| ServiceError::App(AppError::bad_request("日期格式应为 YYYY-MM-DD")))?;
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

/// 严格解析 `YYYY-MM-DD`，返回 (年, 月, 日)；格式或取值非法时返回 `None`。
///
/// 日记服务的日期校验（`diary::is_valid_date`）也复用这里，两条链路口径一致。
pub(crate) fn parse_strict_date(date: &str) -> Option<(i32, u32, u32)> {
    let bytes = date.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if !bytes[0..4].iter().all(u8::is_ascii_digit)
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || !bytes[8..10].iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    let year: i32 = date[0..4].parse().ok()?;
    let month: u32 = date[5..7].parse().ok()?;
    let day: u32 = date[8..10].parse().ok()?;
    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    Some((date.year(), date.month(), date.day()))
}

// ---------- 操作记录（回滚用） ----------

/// 记一次可回滚的操作（在调用方的事务里调用，与操作同生共死）。
///
/// 八个可回滚操作只有两种形状，`kind` 就是这两种形状的判别式：
/// * [`consts::STOCK_OP_KIND_FUND`]  —— 新建了一条资金记录，`target_id` = 资金记录 id；
/// * [`consts::STOCK_OP_KIND_ORDER`] —— 新建了一个委托，`target_id` = `order_id`。
///
/// `action` 是**固定文案**（追加本金 / 支取 / 利息归本 / 建仓 / 加仓 / 减仓 / 清仓），
/// 与资金记录的 `event_text` 同一性质：改动即影响界面。
/// `detail` 是展示摘要（金额或股票与手数），列表弹窗直接显示。
///
/// 每个账本只保留最新 `STOCK_OPERATION_LIMIT` 条（裁剪在 DAO 里随插入一起做）。
fn log_operation(
    conn: &rusqlite::Connection,
    ledger_id: &str,
    kind: &str,
    action: &str,
    detail: String,
    target_id: &str,
) -> ServiceResult<()> {
    let operation = StockOperation {
        id: tr_store::util::new_uuid(),
        ledger_id: ledger_id.to_string(),
        kind: kind.to_string(),
        action: action.to_string(),
        detail,
        target_id: target_id.to_string(),
        created_at: 0,
    };
    db(StockDao::create_operation(conn, &operation))?;
    Ok(())
}

/// 金额摘要（分 → `¥1,234.56`）。
fn amount_detail(amount: i64) -> String {
    format!("¥{}", tr_domain::money::cents_to_yuan(amount))
}

/// 委托类操作的固定文案（与界面上的交易类型标签同一套词）。
fn trade_operation_label(trade_type: &str) -> &'static str {
    match trade_type {
        consts::STOCK_TRADE_OPEN => "建仓",
        consts::STOCK_TRADE_ADD => "加仓",
        consts::STOCK_TRADE_REDUCE => "减仓",
        consts::STOCK_TRADE_CLOSE => "清仓",
        _ => "交易",
    }
}

// ---------- 账户 / 费用设置 / 标签设置 ----------

/// 获取账户，不存在则创建（本金为 0）。
pub fn get_or_create_account(
    workspace: &Workspace,
    ledger_id: &str,
) -> ServiceResult<StockAccount> {
    get_or_create_account_in(&workspace.connection(), ledger_id)
}

/// 获取费用设置，不存在则按默认值创建（万2.354 / 5元 / 0.05% / 0.001%）。
pub fn get_or_create_fee_setting(
    workspace: &Workspace,
    ledger_id: &str,
) -> ServiceResult<StockFeeSetting> {
    get_or_create_fee_setting_in(&workspace.connection(), ledger_id)
}

/// 可用标签的有序 JSON 数组序列化。
fn tags_to_json(tags: &[String]) -> String {
    serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string())
}

/// 返回某账本当前可用的交易标签（有序）；解析异常或空列表时回退默认列表**并修复存储**。
pub fn get_trade_tags(workspace: &Workspace, ledger_id: &str) -> ServiceResult<Vec<String>> {
    get_trade_tags_in(&workspace.connection(), ledger_id)
}

/// 交易标签是否在可用列表里。
pub(crate) fn contains_tag(tags: &[String], tag: &str) -> bool {
    tags.iter().any(|item| item == tag)
}

/// 账户总览（可用现金公式见函数内注释）。
pub fn get_overview(workspace: &Workspace, ledger_id: &str) -> ServiceResult<StockOverviewDto> {
    get_overview_with(workspace, ledger_id, &TencentStockQuoteFetcher::new())
}

/// 账户总览（可注入行情源，便于测试）。
pub fn get_overview_with(
    workspace: &Workspace,
    ledger_id: &str,
    fetcher: &dyn StockQuoteFetcher,
) -> ServiceResult<StockOverviewDto> {
    let account = get_or_create_account(workspace, ledger_id)?;
    let conn = workspace.connection();

    // 已实现总盈亏：Σ 卖出净盈亏（未实现盈亏不计入）
    let realized_pnl = db(StockDao::sum_net_pnl(&conn, ledger_id))?;
    // 累计支取：Σ 支取事件金额
    let withdrawn_total = db(StockDao::sum_withdrawn(&conn, ledger_id))?;
    // 累计利息归本：Σ 利息归本事件金额（本金不变，只进可用现金）
    let interest_total = db(StockDao::sum_interest_principal(&conn, ledger_id))?;
    // 现金余额 = 本金 + 累计利息归本 + 已实现总盈亏 − 累计支取 − 持仓成本（与资金链一致）
    let position_cost = db(StockDao::sum_position_cost(&conn, ledger_id))?;
    let available_cash =
        account.principal + interest_total + realized_pnl - withdrawn_total - position_cost;

    // 持仓市值与浮动盈亏：最新价 × 股数；行情缺失的股票按持仓成本计入并计数
    let held_positions = db(StockDao::list_positions(&conn, ledger_id))?;
    let quotes = fetch_held_quotes(fetcher, &held_positions);
    let (position_market_value, unrealized_pnl, quote_failed_count) =
        compute_held_market_value(&held_positions, &quotes);

    // 总资产 = 可用现金 + 持仓市值（行情全部缺失时市值=成本）
    let total_assets = available_cash + position_market_value;

    // 总盈亏占本金百分比，本金为 0 时按 0 处理（防除零）
    let total_pnl_percent = if account.principal > 0 {
        percent_of(realized_pnl, account.principal)
    } else {
        0.0
    };

    Ok(StockOverviewDto {
        principal: account.principal,
        available_cash,
        position_market_value,
        withdrawn_total,
        interest_total,
        total_assets,
        realized_pnl,
        unrealized_pnl,
        quote_failed_count,
        total_pnl_percent,
    })
}

/// 设置初始本金。
pub fn set_principal(
    workspace: &Workspace,
    ledger_id: &str,
    amount: i64,
) -> ServiceResult<StockOverviewDto> {
    if amount <= 0 {
        return Err(AppError::bad_request("本金必须大于 0").into());
    }
    // 确保账户存在
    get_or_create_account(workspace, ledger_id)?;

    let conn = workspace.connection();
    // 已有资金记录时禁止修改初始本金，避免现金余额链条断裂
    let count = db(StockDao::count_fund_records(&conn, ledger_id))?;
    if count > 0 {
        return Err(AppError::conflict("已有资金变化记录，请使用「追加本金」").into());
    }
    drop(conn);

    if let Err(error) = {
        let conn = workspace.connection();
        StockDao::update_account_principal(&conn, ledger_id, amount)
    } {
        tracing::error!("设置本金失败, ledger: {}, err: {}", ledger_id, error);
        return Err(ServiceError::Database(error));
    }
    tracing::info!(
        "设置股票账户本金, ledger: {}, principal: {}",
        ledger_id,
        amount
    );
    get_overview(workspace, ledger_id)
}

/// 追加本金。
pub fn add_principal(
    workspace: &Workspace,
    ledger_id: &str,
    amount: i64,
) -> ServiceResult<StockOverviewDto> {
    add_principal_at_date(workspace, ledger_id, amount, "")
}

/// 追加本金并指定资金变化的发生日期；`date` 为空时按当天记录。
pub fn add_principal_at_date(
    workspace: &Workspace,
    ledger_id: &str,
    amount: i64,
    date: &str,
) -> ServiceResult<StockOverviewDto> {
    if amount <= 0 {
        return Err(AppError::bad_request("追加金额必须大于 0").into());
    }
    let record_date = normalize_stock_record_date(date)?;

    if let Err(error) = workspace.transaction(|conn| {
        let account = get_or_create_account_in(conn, ledger_id)?;

        // 追加前现金：末条记录余额，无记录则为本金
        let prev_cash = match StockDao::query_latest_fund_record(conn, ledger_id) {
            Ok(latest) => latest.cash_balance,
            Err(error) if is_not_found(&error) => account.principal,
            Err(error) => return Err(ServiceError::Database(error)),
        };

        let new_principal = account.principal + amount;
        db(StockDao::update_account_principal(
            conn,
            ledger_id,
            new_principal,
        ))?;

        let record = StockFundRecord {
            id: tr_store::util::new_uuid(),
            ledger_id: ledger_id.to_string(),
            record_date: record_date.clone(),
            event_type: consts::STOCK_EVENT_ADD_PRINCIPAL.to_string(),
            event_text: "追加本金".to_string(),
            amount_change: amount,
            cash_balance: prev_cash + amount,
            net_pnl: None,
            remark: format!(
                "本金 {} → {}",
                tr_domain::money::cents_to_yuan(account.principal),
                tr_domain::money::cents_to_yuan(new_principal)
            ),
            created_at: 0,
        };
        db(StockDao::create_fund_record(conn, &record))?;
        log_operation(
            conn,
            ledger_id,
            consts::STOCK_OP_KIND_FUND,
            "追加本金",
            amount_detail(amount),
            &record.id,
        )?;
        Ok(())
    }) {
        tracing::error!(
            "追加本金失败, ledger: {}, amount: {}, err: {}",
            ledger_id,
            amount,
            error
        );
        return Err(error);
    }
    tracing::info!(
        "追加股票账户本金, ledger: {}, amount: {}",
        ledger_id,
        amount
    );
    get_overview(workspace, ledger_id)
}

/// 从股票账户支取：现金减少 `amount`，本金保持不变（本金始终是"累计投入"）。
/// 总资产 = 可用现金 + 持仓市值，支取减少可用现金，总资产随之减少。
pub fn add_withdraw(
    workspace: &Workspace,
    ledger_id: &str,
    amount: i64,
) -> ServiceResult<StockOverviewDto> {
    add_withdraw_at_date(workspace, ledger_id, amount, "")
}

/// 从股票账户支取并指定资金变化的发生日期；`date` 为空时按当天记录。
pub fn add_withdraw_at_date(
    workspace: &Workspace,
    ledger_id: &str,
    amount: i64,
    date: &str,
) -> ServiceResult<StockOverviewDto> {
    if amount <= 0 {
        return Err(AppError::bad_request("支取金额必须大于 0").into());
    }
    let record_date = normalize_stock_record_date(date)?;

    if let Err(error) = workspace.transaction(|conn| {
        let account = get_or_create_account_in(conn, ledger_id)?;

        // 当前现金：末条资金记录余额，无记录时为本金
        let prev_cash = match StockDao::query_latest_fund_record(conn, ledger_id) {
            Ok(latest) => latest.cash_balance,
            Err(error) if is_not_found(&error) => account.principal,
            Err(error) => return Err(ServiceError::Database(error)),
        };

        if amount > prev_cash {
            return Err(AppError::bad_request(format!(
                "支取金额不能超过可用现金（{} 元）",
                tr_domain::money::cents_to_yuan(prev_cash)
            ))
            .into());
        }

        let after_cash = prev_cash - amount;
        let record = StockFundRecord {
            id: tr_store::util::new_uuid(),
            ledger_id: ledger_id.to_string(),
            record_date: record_date.clone(),
            event_type: consts::STOCK_EVENT_WITHDRAW.to_string(),
            event_text: "支取".to_string(),
            amount_change: -amount,
            cash_balance: after_cash,
            net_pnl: None,
            remark: format!(
                "现金 {} → {}",
                tr_domain::money::cents_to_yuan(prev_cash),
                tr_domain::money::cents_to_yuan(after_cash)
            ),
            created_at: 0,
        };
        db(StockDao::create_fund_record(conn, &record))?;
        log_operation(
            conn,
            ledger_id,
            consts::STOCK_OP_KIND_FUND,
            "支取",
            amount_detail(amount),
            &record.id,
        )?;
        Ok(())
    }) {
        tracing::error!(
            "支取失败, ledger: {}, amount: {}, err: {}",
            ledger_id,
            amount,
            error
        );
        return Err(error);
    }
    tracing::info!("股票账户支取, ledger: {}, amount: {}", ledger_id, amount);
    get_overview(workspace, ledger_id)
}

/// 利息归本：账户利息 / 分红计入可用现金，`principal`（累计投入本金）保持不变。
///
/// 与「追加本金」的区别是本金口径：追加本金改 `principal`（用户在统计里看到的投入），
/// 利息是账户自己生出来的钱，只累计到 `interest_total` 并直接进可用现金。
pub fn add_interest(
    workspace: &Workspace,
    ledger_id: &str,
    amount: i64,
) -> ServiceResult<StockOverviewDto> {
    add_interest_at_date(workspace, ledger_id, amount, "")
}

/// 利息归本并指定资金变化的发生日期；`date` 为空时按当天记录。
pub fn add_interest_at_date(
    workspace: &Workspace,
    ledger_id: &str,
    amount: i64,
    date: &str,
) -> ServiceResult<StockOverviewDto> {
    if amount <= 0 {
        return Err(AppError::bad_request("利息金额必须大于 0").into());
    }
    let record_date = normalize_stock_record_date(date)?;

    if let Err(error) = workspace.transaction(|conn| {
        let account = get_or_create_account_in(conn, ledger_id)?;

        // 当前现金：末条资金记录余额，无记录时为本金
        let prev_cash = match StockDao::query_latest_fund_record(conn, ledger_id) {
            Ok(latest) => latest.cash_balance,
            Err(error) if is_not_found(&error) => account.principal,
            Err(error) => return Err(ServiceError::Database(error)),
        };

        let after_cash = prev_cash + amount;
        let record = StockFundRecord {
            id: tr_store::util::new_uuid(),
            ledger_id: ledger_id.to_string(),
            record_date: record_date.clone(),
            event_type: consts::STOCK_EVENT_INTEREST_PRINCIPAL.to_string(),
            event_text: "利息归本".to_string(),
            amount_change: amount,
            cash_balance: after_cash,
            net_pnl: None,
            remark: format!(
                "利息 {} 元，现金 {} → {}",
                tr_domain::money::cents_to_yuan(amount),
                tr_domain::money::cents_to_yuan(prev_cash),
                tr_domain::money::cents_to_yuan(after_cash)
            ),
            created_at: 0,
        };
        db(StockDao::create_fund_record(conn, &record))?;
        log_operation(
            conn,
            ledger_id,
            consts::STOCK_OP_KIND_FUND,
            "利息归本",
            amount_detail(amount),
            &record.id,
        )?;
        Ok(())
    }) {
        tracing::error!(
            "利息归本失败, ledger: {}, amount: {}, err: {}",
            ledger_id,
            amount,
            error
        );
        return Err(error);
    }
    tracing::info!(
        "股票账户利息归本, ledger: {}, amount: {}",
        ledger_id,
        amount
    );
    get_overview(workspace, ledger_id)
}

/// 事务内等价 `get_or_create_account`（只依赖连接，供重放 / 资金链复用）。
fn get_or_create_account_in(
    conn: &rusqlite::Connection,
    ledger_id: &str,
) -> ServiceResult<StockAccount> {
    match StockDao::get_account(conn, ledger_id) {
        Ok(account) => Ok(account),
        Err(error) if is_not_found(&error) => {
            let account = StockAccount {
                id: tr_store::util::new_uuid(),
                ledger_id: ledger_id.to_string(),
                ..StockAccount::default()
            };
            db(StockDao::create_account(conn, &account))?;
            Ok(account)
        }
        Err(error) => Err(ServiceError::Database(error)),
    }
}

/// 事务内等价 `get_or_create_fee_setting`。
fn get_or_create_fee_setting_in(
    conn: &rusqlite::Connection,
    ledger_id: &str,
) -> ServiceResult<StockFeeSetting> {
    match StockDao::get_fee_setting(conn, ledger_id) {
        Ok(setting) => Ok(setting),
        Err(error) if is_not_found(&error) => {
            let setting = StockFeeSetting {
                id: tr_store::util::new_uuid(),
                ledger_id: ledger_id.to_string(),
                ..StockFeeSetting::default()
            };
            db(StockDao::create_fee_setting(conn, &setting))?;
            Ok(setting)
        }
        Err(error) => Err(ServiceError::Database(error)),
    }
}

/// 事务内取得标签设置行，不存在时按默认标签列表创建（分析/打板/尾盘/追涨/蓄力）。
fn get_or_create_trade_tag_setting_in(
    conn: &rusqlite::Connection,
    ledger_id: &str,
) -> ServiceResult<StockTradeTagSetting> {
    match StockDao::get_trade_tag_setting(conn, ledger_id) {
        Ok(setting) => Ok(setting),
        Err(error) if is_not_found(&error) => {
            let setting = StockTradeTagSetting {
                id: tr_store::util::new_uuid(),
                ledger_id: ledger_id.to_string(),
                tags: tags_to_json(&consts::default_stock_trade_tags()),
                ..StockTradeTagSetting::default()
            };
            db(StockDao::create_trade_tag_setting(conn, &setting))?;
            Ok(setting)
        }
        Err(error) => Err(ServiceError::Database(error)),
    }
}

/// 事务内等价 `get_trade_tags`。
fn get_trade_tags_in(conn: &rusqlite::Connection, ledger_id: &str) -> ServiceResult<Vec<String>> {
    let setting = get_or_create_trade_tag_setting_in(conn, ledger_id)?;
    match serde_json::from_str::<Vec<String>>(&setting.tags) {
        Ok(tags) if !tags.is_empty() => Ok(tags),
        _ => {
            let defaults = consts::default_stock_trade_tags();
            db(StockDao::update_trade_tag_setting_tags(
                conn,
                ledger_id,
                &tags_to_json(&defaults),
            ))?;
            Ok(defaults)
        }
    }
}

/// 保存费用设置。
pub fn save_fee_settings(
    workspace: &Workspace,
    ledger_id: &str,
    commission_rate: f64,
    min_commission: i64,
    stamp_duty_rate: f64,
    transfer_fee_rate: f64,
) -> ServiceResult<StockFeeSetting> {
    // 佣金费率必须为正；最低佣金、印花税、过户费允许为 0（不收取），但不能为负
    if commission_rate <= 0.0
        || min_commission < 0
        || stamp_duty_rate < 0.0
        || transfer_fee_rate < 0.0
    {
        return Err(AppError::bad_request("佣金费率必须大于 0，最低佣金与费率不能为负").into());
    }

    let mut setting = get_or_create_fee_setting(workspace, ledger_id)?;
    setting.commission_rate = commission_rate;
    setting.min_commission = min_commission;
    setting.stamp_duty_rate = stamp_duty_rate;
    setting.transfer_fee_rate = transfer_fee_rate;

    let conn = workspace.connection();
    if let Err(error) = StockDao::update_fee_setting(&conn, &setting) {
        tracing::error!(
            "保存交易费用设置失败, ledger: {}, err: {}",
            ledger_id,
            error
        );
        return Err(ServiceError::Database(error));
    }
    Ok(setting)
}

/// 返回某账本当前可用的交易标签设置（含默认标签「分析」）。
pub fn get_trade_tags_dto(
    workspace: &Workspace,
    ledger_id: &str,
) -> ServiceResult<StockTradeTagSettingDto> {
    let tags = get_trade_tags(workspace, ledger_id)?;
    Ok(StockTradeTagSettingDto {
        tags,
        default_tag: consts::STOCK_TAG_ANALYSIS.to_string(),
    })
}

/// 保存某账本可用交易标签：去空白、去重并保留顺序；
/// 默认标签「分析」不可删除，列表至少保留一项，单个标签不超过 8 字、总数不超过 20 个。
pub fn save_trade_tags(
    workspace: &Workspace,
    ledger_id: &str,
    tags: &[String],
) -> ServiceResult<StockTradeTagSettingDto> {
    if ledger_id.is_empty() {
        return Err(AppError::bad_request("ledger_id is required").into());
    }

    let mut normalized: Vec<String> = Vec::with_capacity(tags.len());
    let mut seen: Vec<String> = Vec::with_capacity(tags.len());
    for raw in tags {
        let tag = raw.trim();
        if tag.is_empty() {
            continue;
        }
        if seen.iter().any(|item| item == tag) {
            return Err(AppError::bad_request("标签不能重复").into());
        }
        if tag.chars().count() > 8 {
            return Err(AppError::bad_request("单个标签不能超过 8 个字").into());
        }
        if normalized.len() >= 20 {
            return Err(AppError::bad_request("标签数量不能超过 20 个").into());
        }
        seen.push(tag.to_string());
        normalized.push(tag.to_string());
    }
    if normalized.is_empty() {
        return Err(AppError::bad_request("至少保留一个标签").into());
    }
    if !contains_tag(&normalized, consts::STOCK_TAG_ANALYSIS) {
        return Err(AppError::bad_request("默认标签「分析」不可删除").into());
    }

    // 确保设置行存在（不存在时按默认值创建）
    let conn = workspace.connection();
    get_or_create_trade_tag_setting_in(&conn, ledger_id)?;
    if let Err(error) =
        StockDao::update_trade_tag_setting_tags(&conn, ledger_id, &tags_to_json(&normalized))
    {
        tracing::error!(
            "保存交易标签设置失败, ledger: {}, err: {}",
            ledger_id,
            error
        );
        return Err(ServiceError::Database(error));
    }
    tracing::info!(
        "保存交易标签设置, ledger: {}, tags: {:?}",
        ledger_id,
        normalized
    );
    Ok(StockTradeTagSettingDto {
        tags: normalized,
        default_tag: consts::STOCK_TAG_ANALYSIS.to_string(),
    })
}

// ---------- 资金记录 / 持仓 ----------

/// 资金记录分页。
pub fn list_fund_records(
    workspace: &Workspace,
    ledger_id: &str,
    page: i64,
    page_size: i64,
) -> ServiceResult<StockFundRecordPage> {
    let page = if page < 1 { 1 } else { page };
    let page_size = if page_size < 1 {
        10
    } else if page_size > 100 {
        100
    } else {
        page_size
    };

    let conn = workspace.connection();
    let (records, total) = db(StockDao::query_fund_records(
        &conn, ledger_id, page, page_size,
    ))?;

    let items: Vec<StockFundRecordDto> = records.iter().map(StockFundRecordDto::from).collect();
    Ok(StockFundRecordPage {
        items,
        total,
        page: page as i32,
        page_size: page_size as i32,
    })
}

/// 持仓列表（只含未清仓股票，挂载行情）。
pub fn list_positions(
    workspace: &Workspace,
    ledger_id: &str,
) -> ServiceResult<Vec<StockPositionDto>> {
    list_positions_with(workspace, ledger_id, &TencentStockQuoteFetcher::new())
}

/// 持仓列表（可注入行情源）。
pub fn list_positions_with(
    workspace: &Workspace,
    ledger_id: &str,
    fetcher: &dyn StockQuoteFetcher,
) -> ServiceResult<Vec<StockPositionDto>> {
    let conn = workspace.connection();
    let positions = db(StockDao::list_positions(&conn, ledger_id))?;
    // 已清仓的股票不再出现在持仓列表
    let held: Vec<StockPosition> = positions
        .into_iter()
        .filter(|position| position.quantity > 0)
        .collect();
    let quotes = fetch_held_quotes(fetcher, &held);

    Ok(held
        .iter()
        .map(|position| {
            let mut item = StockPositionDto::from(position);
            if let Some(quote) = quotes.get(&position.stock_code) {
                if quote.latest_price > 0 {
                    item.latest_price = Some(quote.latest_price);
                    item.prev_close = Some(quote.prev_close);
                    item.quote_time = Some(quote.quote_time);
                }
            }
            item
        })
        .collect())
}

/// 保存持仓中的「本轮复盘」（500 字以内，空串清空）。
///
/// 持仓期间还没有轮次记录，复盘先落在持仓上，清仓归档时带入该轮次，形成同一份记录。
pub fn update_position_review(
    workspace: &Workspace,
    ledger_id: &str,
    stock_code: &str,
    review: &str,
) -> ServiceResult<StockPositionDto> {
    if ledger_id.is_empty() {
        return Err(AppError::bad_request("ledger_id is required").into());
    }
    if stock_code.is_empty() {
        return Err(AppError::bad_request("stock_code is required").into());
    }
    let review = review.trim();
    if review.chars().count() > 500 {
        return Err(AppError::bad_request("交易复盘不能超过 500 字").into());
    }

    let conn = workspace.connection();
    let mut position = match StockDao::get_position(&conn, ledger_id, stock_code) {
        Ok(position) => position,
        Err(error) if is_not_found(&error) => {
            return Err(AppError::not_found("持仓不存在").into());
        }
        Err(error) => return Err(ServiceError::Database(error)),
    };
    if position.quantity <= 0 {
        return Err(AppError::not_found("该股票已清仓，请在「交易历史」中编辑本轮复盘").into());
    }

    position.review = review.to_string();
    if let Err(error) = StockDao::update_position(&conn, &position) {
        tracing::error!(
            "保存持仓复盘失败, ledger: {}, code: {}, err: {}",
            ledger_id,
            stock_code,
            error
        );
        return Err(ServiceError::Database(error));
    }
    tracing::info!("保存持仓复盘, ledger: {}, code: {}", ledger_id, stock_code);
    Ok(StockPositionDto::from(&position))
}

/// 仅对当前持仓股票请求行情；无持仓时返回空映射。
fn fetch_held_quotes(
    fetcher: &dyn StockQuoteFetcher,
    held: &[StockPosition],
) -> HashMap<String, tr_domain::dto::StockQuoteDto> {
    let codes: Vec<String> = held
        .iter()
        .filter(|position| position.quantity > 0)
        .map(|position| position.stock_code.clone())
        .collect();
    if codes.is_empty() {
        return HashMap::new();
    }
    fetcher.fetch_quotes(&codes)
}

/// 为已清仓股票批量补充最新价（一次请求）；行情缺失的股票保持为空，
/// 由前端显示占位符，不阻塞历史列表返回。
fn attach_history_quotes(fetcher: &dyn StockQuoteFetcher, items: &mut [StockTradeHistoryDto]) {
    if items.is_empty() {
        return;
    }
    let codes: Vec<String> = items.iter().map(|item| item.stock_code.clone()).collect();
    let quotes = fetcher.fetch_quotes(&codes);
    for item in items.iter_mut() {
        if let Some(quote) = quotes.get(&item.stock_code) {
            if quote.latest_price > 0 {
                item.latest_price = Some(quote.latest_price);
            }
        }
    }
}

/// 汇总持仓市值与浮动盈亏（单位：分）。
///
/// 行情可用的股票按最新价计价；缺失的按持仓成本计入，避免总资产在行情失败时失真，
/// 并返回失败数量供界面提示。
fn compute_held_market_value(
    held: &[StockPosition],
    quotes: &HashMap<String, tr_domain::dto::StockQuoteDto>,
) -> (i64, i64, i64) {
    let mut market_value = 0_i64;
    let mut unrealized_pnl = 0_i64;
    let mut quote_failed_count = 0_i64;
    for position in held {
        if position.quantity <= 0 {
            continue;
        }
        match quotes.get(&position.stock_code) {
            Some(quote) if quote.latest_price > 0 => {
                let value = quote.latest_price * position.quantity;
                market_value += value;
                unrealized_pnl += value - position.total_cost;
            }
            _ => {
                market_value += position.total_cost;
                quote_failed_count += 1;
            }
        }
    }
    (market_value, unrealized_pnl, quote_failed_count)
}

// ---------- 交易列表 ----------

/// 某股交易列表：
/// 持仓中的股票只展示本轮（最近一次清仓之后）的交易，历史轮次留在「交易历史」页。
pub fn list_trades(
    workspace: &Workspace,
    ledger_id: &str,
    stock_code: &str,
) -> ServiceResult<Vec<StockTradeDto>> {
    if stock_code.is_empty() {
        return Err(AppError::bad_request("stock_code is required").into());
    }
    let conn = workspace.connection();
    let trades = db(StockDao::list_trades(&conn, ledger_id, stock_code))?;

    let position = StockDao::get_position(&conn, ledger_id, stock_code);
    match position {
        Ok(position) if position.quantity > 0 => {
            // 倒序 → 升序（按索引倒排，避免再查一次库）
            let asc: Vec<StockTrade> = trades.iter().rev().cloned().collect();
            let mut current = current_round_trades(&asc);
            current.reverse();
            Ok(current.iter().map(StockTradeDto::from).collect())
        }
        Ok(_) => Ok(trades.iter().map(StockTradeDto::from).collect()),
        Err(error) if is_not_found(&error) => Ok(trades.iter().map(StockTradeDto::from).collect()),
        Err(error) => Err(ServiceError::Database(error)),
    }
}

/// 从按时间升序的交易流中切出当前在建轮次（最近一次清仓之后）的交易。
/// 卖出把持仓数量归零时结束一轮，该笔卖出属于已完成的轮次，不计入当前轮。
fn current_round_trades(trades: &[StockTrade]) -> Vec<StockTrade> {
    let mut result: Vec<StockTrade> = Vec::new();
    let mut shares = 0_i64;
    for trade in trades {
        match trade.trade_type.as_str() {
            consts::STOCK_TRADE_OPEN | consts::STOCK_TRADE_ADD => {
                shares += trade.shares;
                result.push(trade.clone());
            }
            consts::STOCK_TRADE_REDUCE | consts::STOCK_TRADE_CLOSE => {
                shares -= trade.shares;
                if shares > 0 {
                    result.push(trade.clone());
                } else {
                    shares = 0;
                    result.clear();
                }
            }
            _ => {}
        }
    }
    result
}

// ---------- 委托创建 ----------

/// 过滤前端未填写的空明细行（价格与手数同时 <= 0）。
fn normalize_trade_fills(fills: &[TradeFill]) -> Vec<TradeFill> {
    fills
        .iter()
        .filter(|fill| !(fill.price_cents <= 0 && fill.lots <= 0))
        .copied()
        .collect()
}

/// 生成资金记录备注：均价 + 笔数（单笔委托退化为「名称 N手 @ 价格」）。
fn trade_order_remark(
    stock_name: &str,
    total_lots: i64,
    total_amount: i64,
    count: usize,
) -> String {
    let shares = total_lots * 100;
    let avg_price = if shares > 0 {
        (total_amount as f64 / shares as f64).round() as i64
    } else {
        0
    };
    if count <= 1 {
        format!(
            "{stock_name} {total_lots}手 @ {}",
            tr_domain::money::cents_to_yuan(avg_price)
        )
    } else {
        format!(
            "{stock_name} {total_lots}手 @ {}（{count}笔成交）",
            tr_domain::money::cents_to_yuan(avg_price)
        )
    }
}

/// 记录一笔委托：可包含多笔成交明细，费用按委托成交总额计算一次后分摊到各笔。
///
/// 买入（建仓/加仓）：现金减少 Σ成交金额+费用；卖出（减仓/清仓）：现金增加 Σ成交金额-费用，
/// 并按平均成本逐笔结转已实现盈亏；全部成交后持仓归零则归档本轮轮次。
///
/// 参数多是有意的：与命令面的字段一一对应，便于逐条比对。
#[allow(clippy::too_many_arguments)]
pub fn create_trade_order(
    workspace: &Workspace,
    ledger_id: &str,
    stock_code: &str,
    stock_name: &str,
    trade_type: &str,
    fills: &[TradeFill],
    trade_time: i64,
    remark: &str,
    tag: &str,
) -> ServiceResult<Vec<StockTradeDto>> {
    let fills = normalize_trade_fills(fills);
    if fills.is_empty() {
        return Err(AppError::bad_request("成交明细不能为空").into());
    }
    for fill in &fills {
        if fill.price_cents <= 0 {
            return Err(AppError::bad_request("成交价必须大于 0").into());
        }
        if fill.lots <= 0 {
            return Err(AppError::bad_request("手数必须大于 0").into());
        }
    }
    if !tag.is_empty() {
        let tags = get_trade_tags(workspace, ledger_id)?;
        if !contains_tag(&tags, tag) {
            return Err(AppError::bad_request("无效的交易标签").into());
        }
    }
    let trade_time = if trade_time <= 0 {
        tr_store::util::now_unix()
    } else {
        trade_time
    };

    let buy = is_buy(trade_type);
    let sell = is_sell(trade_type);
    if !buy && !sell {
        return Err(AppError::bad_request("无效的交易类型").into());
    }

    // 沪市：60（主板）/ 68（科创板）开头
    let is_sh = fee::is_shanghai_code(stock_code);
    let order_id = tr_store::util::new_uuid();

    let mut trades: Vec<StockTrade> = Vec::with_capacity(fills.len());
    let mut amounts: Vec<i64> = Vec::with_capacity(fills.len());
    let mut total_amount = 0_i64;
    let mut total_lots = 0_i64;
    for (index, fill) in fills.iter().enumerate() {
        let shares = fill.lots * 100;
        let amount = fill.price_cents * shares;
        total_amount += amount;
        total_lots += fill.lots;
        amounts.push(amount);
        trades.push(StockTrade {
            id: tr_store::util::new_uuid(),
            ledger_id: ledger_id.to_string(),
            stock_code: stock_code.to_string(),
            stock_name: stock_name.to_string(),
            trade_type: trade_type.to_string(),
            order_id: order_id.clone(),
            order_seq: index as i64 + 1,
            price: fill.price_cents,
            lots: fill.lots,
            shares,
            amount,
            trade_time,
            remark: remark.to_string(),
            ..StockTrade::default()
        });
    }

    let mut realized_total = 0_i64;
    let record_date = unix_to_date(trade_time);

    if let Err(error) = workspace.transaction(|conn| {
        let fee_setting = get_or_create_fee_setting_in(conn, ledger_id)?;
        // 委托级一次计收（最低佣金按委托，不按笔）；印花税与过户费**逐笔**取整再相加，
        // 再按成交额把委托级费用分摊到各笔明细（两份口径一致，见 `tr_domain::fee`）
        let order_fee = fee::compute_order_fee(&amounts, is_sh, &fee_setting, buy);
        let allocated = fee::allocate_order_fee(order_fee, &amounts, is_sh, &fee_setting, buy);

        let mut position = match StockDao::get_position(conn, ledger_id, stock_code) {
            Ok(position) => position,
            Err(error) if is_not_found(&error) => {
                let position = StockPosition {
                    id: tr_store::util::new_uuid(),
                    ledger_id: ledger_id.to_string(),
                    stock_code: stock_code.to_string(),
                    stock_name: stock_name.to_string(),
                    ..StockPosition::default()
                };
                db(StockDao::create_position(conn, &position))?;
                position
            }
            Err(error) => return Err(ServiceError::Database(error)),
        };
        position.stock_name = stock_name.to_string();

        // 当前现金：末条资金记录余额，无记录则为本金
        let account = get_or_create_account_in(conn, ledger_id)?;
        let prev_cash = match StockDao::query_latest_fund_record(conn, ledger_id) {
            Ok(latest) => latest.cash_balance,
            Err(error) if is_not_found(&error) => account.principal,
            Err(error) => return Err(ServiceError::Database(error)),
        };

        // 卖出先按委托总量校验，避免逐笔结算中途才发现超卖
        if sell {
            let sold_shares: i64 = trades.iter().map(|trade| trade.shares).sum();
            if sold_shares > position.quantity {
                return Err(AppError::bad_request(format!(
                    "卖出数量超过持仓（当前 {} 股）",
                    position.quantity
                ))
                .into());
            }
        }

        for (index, trade) in trades.iter_mut().enumerate() {
            trade.fee = allocated[index].total;
            trade.commission = allocated[index].commission;
            trade.stamp_duty = allocated[index].stamp_duty;
            trade.transfer_fee = allocated[index].transfer_fee;

            if buy {
                position.quantity += trade.shares;
                position.total_cost += trade.amount + trade.fee;
                continue;
            }

            // 按剩余总成本的比例结转（四舍五入到分），避免整除截断造成已实现盈亏偏差
            let cost_basis = cost_basis_of(position.total_cost, trade.shares, position.quantity);
            let realized = trade.amount - trade.fee - cost_basis;
            trade.realized_pnl = Some(realized);
            realized_total += realized;

            position.quantity -= trade.shares;
            position.total_cost -= cost_basis;
            position.realized_pnl += realized;
            if position.quantity == 0 {
                position.total_cost = 0;
            }
        }

        // 清仓：把本轮「建仓 → 清仓」的全部交易归档到交易历史
        if sell && position.quantity == 0 {
            let round_id = close_round(
                conn,
                ledger_id,
                stock_code,
                stock_name,
                trade_time,
                tag,
                &position.review,
            )?;
            for trade in trades.iter_mut() {
                trade.round_id = round_id.clone();
            }
            // 持仓期间先写的复盘已归档到本轮次，清空持仓上的草稿，避免下一轮继承
            position.review = String::new();
        }

        db(StockDao::update_position(conn, &position))?;

        let (amount_change, event_type, event_text, net_pnl) = if buy {
            (
                -(total_amount + order_fee.total),
                consts::STOCK_EVENT_BUY,
                format!("买入 {stock_name} {total_lots}手"),
                None,
            )
        } else {
            (
                total_amount - order_fee.total,
                consts::STOCK_EVENT_SELL,
                format!("卖出 {stock_name} {total_lots}手"),
                Some(realized_total),
            )
        };

        let record = StockFundRecord {
            id: tr_store::util::new_uuid(),
            ledger_id: ledger_id.to_string(),
            record_date: record_date.clone(),
            event_type: event_type.to_string(),
            event_text,
            amount_change,
            cash_balance: prev_cash + amount_change,
            net_pnl,
            remark: trade_order_remark(stock_name, total_lots, total_amount, trades.len()),
            created_at: 0,
        };
        db(StockDao::create_fund_record(conn, &record))?;

        for trade in &trades {
            db(StockDao::create_trade(conn, trade))?;
        }
        log_operation(
            conn,
            ledger_id,
            consts::STOCK_OP_KIND_ORDER,
            trade_operation_label(trade_type),
            format!(
                "{stock_name} {stock_code} · {total_lots} 手 · {}",
                amount_detail(total_amount)
            ),
            &order_id,
        )?;
        Ok(())
    }) {
        tracing::error!(
            "记录股票交易失败, ledger: {}, code: {}, err: {}",
            ledger_id,
            stock_code,
            error
        );
        return Err(error);
    }

    Ok(trades.iter().map(StockTradeDto::from).collect())
}

/// 记录单笔成交（等价于只含一笔明细的委托），保持原有调用方不变。
#[allow(clippy::too_many_arguments)]
pub fn create_trade(
    workspace: &Workspace,
    ledger_id: &str,
    stock_code: &str,
    stock_name: &str,
    trade_type: &str,
    price_cents: i64,
    lots: i64,
    trade_time: i64,
    remark: &str,
    tag: &str,
) -> ServiceResult<StockTradeDto> {
    let items = create_trade_order(
        workspace,
        ledger_id,
        stock_code,
        stock_name,
        trade_type,
        &[TradeFill { price_cents, lots }],
        trade_time,
        remark,
        tag,
    )?;
    items
        .into_iter()
        .next()
        .ok_or_else(|| AppError::bad_request("成交明细不能为空").into())
}

// ---------- 轮次归档 ----------

/// 清仓收尾：确保历史集合存在（首次清仓创建，之后复用），
/// 创建本轮次并把该股从建仓到清仓的全部未归档交易挂接进来。
///
/// `review` 为持仓期间先写的本轮复盘（可为空），归档后由持仓侧清空。
fn close_round(
    conn: &rusqlite::Connection,
    ledger_id: &str,
    stock_code: &str,
    stock_name: &str,
    closed_at: i64,
    tag: &str,
    review: &str,
) -> ServiceResult<String> {
    // 兼容存量数据：先归档历史上已完成但未挂接的轮次，避免与当前轮次混淆
    ensure_stock_history_backfill(conn, ledger_id, stock_code)?;

    let mut history = match StockDao::get_trade_history(conn, ledger_id, stock_code) {
        Ok(history) => history,
        Err(error) if is_not_found(&error) => {
            let history = StockTradeHistory {
                id: tr_store::util::new_uuid(),
                ledger_id: ledger_id.to_string(),
                stock_code: stock_code.to_string(),
                stock_name: stock_name.to_string(),
                ..StockTradeHistory::default()
            };
            db(StockDao::create_trade_history(conn, &history))?;
            history
        }
        Err(error) => return Err(ServiceError::Database(error)),
    };
    if history.stock_name != stock_name {
        db(StockDao::update_trade_history_name(
            conn, ledger_id, stock_code, stock_name,
        ))?;
        history.stock_name = stock_name.to_string();
    }

    let count = db(StockDao::count_trade_rounds(conn, &history.id))?;
    let mut opened_at = db(StockDao::min_unattached_trade_time(
        conn, ledger_id, stock_code,
    ))?;
    if opened_at == 0 {
        opened_at = closed_at;
    }
    let tag = if tag.is_empty() {
        consts::STOCK_TAG_ANALYSIS.to_string()
    } else {
        tag.to_string()
    };

    let round = StockTradeRound {
        id: tr_store::util::new_uuid(),
        ledger_id: ledger_id.to_string(),
        stock_code: stock_code.to_string(),
        history_id: history.id,
        round_no: count + 1,
        opened_at,
        closed_at,
        tag,
        review: review.trim().to_string(),
        created_at: 0,
    };
    db(StockDao::create_trade_round(conn, &round))?;
    // 本轮交易此时尚未入库，CreateTrade 末尾会把当前清仓单直接带上 round_id
    db(StockDao::attach_unattached_trades(
        conn, ledger_id, stock_code, &round.id,
    ))?;
    Ok(round.id)
}

// ---------- 历史与详情 ----------

/// 为有交易记录但尚无历史集合的股票补齐历史（幂等）。
pub fn ensure_trade_history_backfill(
    conn: &rusqlite::Connection,
    ledger_id: &str,
) -> ServiceResult<()> {
    let stocks = db(StockDao::list_trade_stocks(conn, ledger_id))?;
    for code in stocks {
        ensure_stock_history_backfill(conn, ledger_id, &code)?;
    }
    Ok(())
}

/// 把某只股票尚未挂接轮次的存量交易按「建仓 → 清仓」切分并归档。
/// 幂等：每次只处理未挂接的交易；全部挂接后直接返回。
pub fn ensure_stock_history_backfill(
    conn: &rusqlite::Connection,
    ledger_id: &str,
    stock_code: &str,
) -> ServiceResult<()> {
    let trades = db(StockDao::list_trades_asc(conn, ledger_id, stock_code))?;
    let unattached: Vec<StockTrade> = trades
        .into_iter()
        .filter(|trade| trade.round_id.is_empty())
        .collect();
    if unattached.is_empty() {
        return Ok(());
    }

    let cycles = derive_trade_cycles(&unattached);
    let history = match StockDao::get_trade_history(conn, ledger_id, stock_code) {
        Ok(history) => Some(history),
        Err(error) if is_not_found(&error) => {
            if cycles.is_empty() {
                return Ok(()); // 尚无完整轮次（在建），不建历史集合
            }
            let history = StockTradeHistory {
                id: tr_store::util::new_uuid(),
                ledger_id: ledger_id.to_string(),
                stock_code: stock_code.to_string(),
                stock_name: unattached[0].stock_name.clone(),
                ..StockTradeHistory::default()
            };
            db(StockDao::create_trade_history(conn, &history))?;
            Some(history)
        }
        Err(error) => return Err(ServiceError::Database(error)),
    };
    if cycles.is_empty() {
        return Ok(());
    }
    let Some(history) = history else {
        return Ok(());
    };

    let count = db(StockDao::count_trade_rounds(conn, &history.id))?;
    for (index, cycle) in cycles.iter().enumerate() {
        let round = StockTradeRound {
            id: tr_store::util::new_uuid(),
            ledger_id: ledger_id.to_string(),
            stock_code: stock_code.to_string(),
            history_id: history.id.clone(),
            round_no: count + index as i64 + 1,
            opened_at: cycle.opened_at,
            closed_at: cycle.closed_at,
            tag: consts::STOCK_TAG_ANALYSIS.to_string(),
            review: String::new(),
            created_at: 0,
        };
        db(StockDao::create_trade_round(conn, &round))?;
        db(StockDao::update_trades_round_id(
            conn, &round.id, &cycle.ids,
        ))?;
    }
    Ok(())
}

/// 一轮「建仓 → 清仓」的原始交易。
struct TradeCycle {
    ids: Vec<String>,
    opened_at: i64,
    closed_at: i64,
}

/// 把按时间升序的交易流切分为完整轮次：持仓数量回到 0 即一轮结束。
/// 尚未回到 0 的在建轮次不返回（保持未挂接，待清仓时归档）。
fn derive_trade_cycles(trades: &[StockTrade]) -> Vec<TradeCycle> {
    let mut pending: Vec<TradeCycle> = Vec::new();
    let mut shares = 0_i64;
    let mut current: Option<usize> = None;
    for trade in trades {
        match trade.trade_type.as_str() {
            consts::STOCK_TRADE_OPEN | consts::STOCK_TRADE_ADD => {
                if shares == 0 {
                    pending.push(TradeCycle {
                        ids: Vec::new(),
                        opened_at: trade.trade_time,
                        closed_at: 0,
                    });
                    current = Some(pending.len() - 1);
                }
                shares += trade.shares;
                if let Some(index) = current {
                    pending[index].ids.push(trade.id.clone());
                }
            }
            consts::STOCK_TRADE_REDUCE | consts::STOCK_TRADE_CLOSE => {
                if current.is_none() {
                    pending.push(TradeCycle {
                        ids: Vec::new(),
                        opened_at: trade.trade_time,
                        closed_at: 0,
                    });
                    current = Some(pending.len() - 1);
                }
                shares -= trade.shares;
                if shares < 0 {
                    shares = 0;
                }
                if let Some(index) = current {
                    pending[index].ids.push(trade.id.clone());
                }
                if shares == 0 {
                    if let Some(index) = current {
                        pending[index].closed_at = trade.trade_time;
                    }
                    current = None;
                }
            }
            _ => {}
        }
    }
    pending
        .into_iter()
        .filter(|cycle| cycle.closed_at > 0)
        .collect()
}

/// 交易历史集合列表（左栏），按最近清仓时间倒序。
pub fn list_trade_histories(
    workspace: &Workspace,
    ledger_id: &str,
) -> ServiceResult<Vec<StockTradeHistoryDto>> {
    list_trade_histories_with(workspace, ledger_id, &TencentStockQuoteFetcher::new())
}

/// 交易历史集合列表（可注入行情源）。
pub fn list_trade_histories_with(
    workspace: &Workspace,
    ledger_id: &str,
    fetcher: &dyn StockQuoteFetcher,
) -> ServiceResult<Vec<StockTradeHistoryDto>> {
    let conn = workspace.connection();
    ensure_trade_history_backfill(&conn, ledger_id)?;
    let histories = db(StockDao::list_trade_histories(&conn, ledger_id))?;

    let mut items = Vec::with_capacity(histories.len());
    for history in &histories {
        items.push(build_history_dto(&conn, history)?);
    }
    attach_history_quotes(fetcher, &mut items);
    // 稳定排序：最近清仓时间倒序（同值时保持原有顺序）
    items.sort_by_key(|item| std::cmp::Reverse(item.last_closed_at));
    Ok(items)
}

/// 汇总单只股票的轮次数、累计盈亏与最近清仓时间。
fn build_history_dto(
    conn: &rusqlite::Connection,
    history: &StockTradeHistory,
) -> ServiceResult<StockTradeHistoryDto> {
    let rounds = db(StockDao::list_trade_rounds_by_stock(
        conn,
        &history.ledger_id,
        &history.stock_code,
    ))?;
    let mut item = StockTradeHistoryDto {
        id: history.id.clone(),
        ledger_id: history.ledger_id.clone(),
        stock_code: history.stock_code.clone(),
        stock_name: history.stock_name.clone(),
        round_count: rounds.len() as i64,
        created_at: history.created_at,
        updated_at: history.updated_at,
        ..StockTradeHistoryDto::default()
    };
    let mut total_pnl = 0_i64;
    let mut total_buy_cost = 0_i64;
    for round in &rounds {
        let trades = db(StockDao::list_trades_by_round(conn, &round.id))?;
        let (pnl, _, buy_cost) = round_pnl(&trades);
        total_pnl += pnl;
        total_buy_cost += buy_cost;
        if round.closed_at > item.last_closed_at {
            item.last_closed_at = round.closed_at;
        }
    }
    item.total_pnl = total_pnl;
    if total_buy_cost > 0 {
        item.total_pnl_rate = percent_of(total_pnl, total_buy_cost);
    }
    Ok(item)
}

/// 单只股票的交易历史详情（右栏）：全部轮次 + 每轮交易 + 汇总盈亏。
pub fn get_trade_history_detail(
    workspace: &Workspace,
    ledger_id: &str,
    stock_code: &str,
) -> ServiceResult<StockTradeHistoryDetailDto> {
    let conn = workspace.connection();
    let history = match StockDao::get_trade_history(&conn, ledger_id, stock_code) {
        Ok(history) => history,
        Err(error) if is_not_found(&error) => {
            return Err(AppError::not_found("该股票暂无交易历史").into());
        }
        Err(error) => return Err(ServiceError::Database(error)),
    };
    let rounds = db(StockDao::list_trade_rounds_by_stock(
        &conn, ledger_id, stock_code,
    ))?;

    let mut detail = StockTradeHistoryDetailDto {
        id: history.id.clone(),
        ledger_id: history.ledger_id.clone(),
        stock_code: history.stock_code.clone(),
        stock_name: history.stock_name,
        ..StockTradeHistoryDetailDto::default()
    };
    let mut total_pnl = 0_i64;
    let mut total_buy_cost = 0_i64;
    for round in &rounds {
        let trades = db(StockDao::list_trades_by_round(&conn, &round.id))?;
        let (pnl, pnl_rate, buy_cost) = round_pnl(&trades);
        total_pnl += pnl;
        total_buy_cost += buy_cost;
        if round.closed_at > detail.last_closed_at {
            detail.last_closed_at = round.closed_at;
        }
        if pnl > 0 {
            detail.win_count += 1;
        } else if pnl < 0 {
            detail.loss_count += 1;
        }

        detail.rounds.push(tr_domain::dto::StockTradeRoundDto {
            id: round.id.clone(),
            history_id: round.history_id.clone(),
            round_no: round.round_no,
            opened_at: round.opened_at,
            closed_at: round.closed_at,
            tag: round.tag.clone(),
            review: round.review.clone(),
            pnl,
            pnl_rate,
            trade_count: trades.len() as i64,
            trades: trades.iter().map(StockTradeDto::from).collect(),
        });
    }
    detail.round_count = rounds.len() as i64;
    detail.total_pnl = total_pnl;
    if total_buy_cost > 0 {
        detail.total_pnl_rate = percent_of(total_pnl, total_buy_cost);
    }
    Ok(detail)
}

/// 更新某一已完成轮次的交易复盘（500 字以内，空串清空），
/// 校验轮次属于当前账本后保存，并返回该股最新的历史详情。
pub fn update_round_review(
    workspace: &Workspace,
    ledger_id: &str,
    round_id: &str,
    review: &str,
) -> ServiceResult<StockTradeHistoryDetailDto> {
    if round_id.is_empty() {
        return Err(AppError::bad_request("round_id is required").into());
    }
    let review = review.trim();
    if review.chars().count() > 500 {
        return Err(AppError::bad_request("交易复盘不能超过 500 字").into());
    }

    let conn = workspace.connection();
    let round = match StockDao::get_trade_round(&conn, round_id) {
        Ok(round) => round,
        Err(error) if is_not_found(&error) => {
            return Err(AppError::not_found("轮次不存在").into());
        }
        Err(error) => return Err(ServiceError::Database(error)),
    };
    if round.ledger_id != ledger_id {
        return Err(AppError::not_found("轮次不存在").into());
    }

    if let Err(error) = StockDao::update_trade_round_review(&conn, round_id, review) {
        tracing::error!(
            "保存轮次复盘失败, ledger: {}, round: {}, err: {}",
            ledger_id,
            round_id,
            error
        );
        return Err(ServiceError::Database(error));
    }
    tracing::info!("保存轮次复盘, ledger: {}, round: {}", ledger_id, round_id);
    drop(conn);
    get_trade_history_detail(workspace, ledger_id, &round.stock_code)
}

/// 更新某一已完成轮次的交易标签，标签必须是该账本可用标签列表中的一项；
/// 校验轮次归属后保存，并返回该股最新的历史详情。
pub fn update_round_tag(
    workspace: &Workspace,
    ledger_id: &str,
    round_id: &str,
    tag: &str,
) -> ServiceResult<StockTradeHistoryDetailDto> {
    if round_id.is_empty() {
        return Err(AppError::bad_request("round_id is required").into());
    }
    let tag = tag.trim();
    let tag = if tag.is_empty() {
        consts::STOCK_TAG_ANALYSIS
    } else {
        tag
    };

    let conn = workspace.connection();
    let round = match StockDao::get_trade_round(&conn, round_id) {
        Ok(round) => round,
        Err(error) if is_not_found(&error) => {
            return Err(AppError::not_found("轮次不存在").into());
        }
        Err(error) => return Err(ServiceError::Database(error)),
    };
    if round.ledger_id != ledger_id {
        return Err(AppError::not_found("轮次不存在").into());
    }
    let available_tags = get_trade_tags_in(&conn, ledger_id)?;
    if !contains_tag(&available_tags, tag) {
        return Err(AppError::bad_request("无效的交易标签").into());
    }

    if let Err(error) = StockDao::update_trade_round_tag(&conn, round_id, tag) {
        tracing::error!(
            "保存轮次标签失败, ledger: {}, round: {}, err: {}",
            ledger_id,
            round_id,
            error
        );
        return Err(ServiceError::Database(error));
    }
    tracing::info!(
        "保存轮次标签, ledger: {}, round: {}, tag: {}",
        ledger_id,
        round_id,
        tag
    );
    drop(conn);
    get_trade_history_detail(workspace, ledger_id, &round.stock_code)
}

/// 汇总该账本全部已清仓股票：总盈亏、胜负轮次与总轮次。
pub fn get_trade_history_summary(
    workspace: &Workspace,
    ledger_id: &str,
) -> ServiceResult<StockTradeHistorySummaryDto> {
    let conn = workspace.connection();
    ensure_trade_history_backfill(&conn, ledger_id)?;
    let histories = db(StockDao::list_trade_histories(&conn, ledger_id))?;

    let mut summary = StockTradeHistorySummaryDto::default();
    let mut total_buy_cost = 0_i64;
    for history in &histories {
        summary.stock_count += 1;
        let rounds = db(StockDao::list_trade_rounds_by_stock(
            &conn,
            ledger_id,
            &history.stock_code,
        ))?;
        for round in &rounds {
            let trades = db(StockDao::list_trades_by_round(&conn, &round.id))?;
            let (pnl, _, buy_cost) = round_pnl(&trades);
            summary.round_count += 1;
            summary.total_pnl += pnl;
            total_buy_cost += buy_cost;
            if pnl > 0 {
                summary.win_count += 1;
            } else if pnl < 0 {
                summary.loss_count += 1;
            }
        }
    }
    if total_buy_cost > 0 {
        summary.total_pnl_rate = percent_of(summary.total_pnl, total_buy_cost);
    }
    Ok(summary)
}

// ---------- 股票名 ----------

/// 按股票代码查询股票名称：优先本地已有交易记录，未命中时走外部行情接口兜底。
pub fn lookup_stock_name(workspace: &Workspace, stock_code: &str) -> ServiceResult<StockNameDto> {
    lookup_stock_name_with(workspace, stock_code, &TencentStockQuoteFetcher::new())
}

/// 股票名查询（可注入行情源）。
pub fn lookup_stock_name_with(
    workspace: &Workspace,
    stock_code: &str,
    fetcher: &dyn StockQuoteFetcher,
) -> ServiceResult<StockNameDto> {
    if stock_code.is_empty() {
        return Err(AppError::bad_request("stock_code is required").into());
    }
    let conn = workspace.connection();
    match StockDao::query_stock_name(&conn, stock_code) {
        Ok(name) if !name.is_empty() => Ok(StockNameDto {
            stock_code: stock_code.to_string(),
            stock_name: name,
        }),
        Ok(_) => Ok(StockNameDto {
            stock_code: stock_code.to_string(),
            stock_name: fetcher.fetch_name(stock_code),
        }),
        Err(error) if is_not_found(&error) => Ok(StockNameDto {
            stock_code: stock_code.to_string(),
            stock_name: fetcher.fetch_name(stock_code),
        }),
        Err(error) => {
            tracing::warn!("查询本地股票名称失败, code: {}, err: {}", stock_code, error);
            Ok(StockNameDto {
                stock_code: stock_code.to_string(),
                stock_name: fetcher.fetch_name(stock_code),
            })
        }
    }
}

// ---------- 重置 ----------

/// 清空指定账本的全部股票交易数据。
pub fn reset_data(workspace: &Workspace, ledger_id: &str) -> ServiceResult<()> {
    if let Err(error) = workspace.transaction(|conn| {
        db(StockDao::reset_by_ledger_id(conn, ledger_id))?;
        Ok(())
    }) {
        tracing::error!("重置股票交易数据失败, err: {}", error);
        return Err(error);
    }
    Ok(())
}

// ---------- 成交编辑 / 委托删除 / 重放 ----------

/// 编辑一笔成交（成交价/手数/委托时间）：
/// 按当前费用设置重算所属委托的全部费用后分摊，再重放持仓、资金记录与轮次。
pub fn update_trade_fill(
    workspace: &Workspace,
    ledger_id: &str,
    trade_id: &str,
    price_cents: i64,
    lots: i64,
    trade_time: i64,
) -> ServiceResult<StockTradeDto> {
    if trade_id.is_empty() {
        return Err(AppError::bad_request("trade_id is required").into());
    }
    if price_cents <= 0 {
        return Err(AppError::bad_request("成交价必须大于 0").into());
    }
    if lots <= 0 {
        return Err(AppError::bad_request("手数必须大于 0").into());
    }

    let mut updated: Option<StockTrade> = None;
    if let Err(error) = workspace.transaction(|conn| {
        update_trade_fill_tx(conn, ledger_id, trade_id, price_cents, lots, trade_time)?;
        let trade = db(StockDao::get_trade(conn, trade_id))?;
        updated = Some(trade);
        Ok(())
    }) {
        tracing::error!(
            "编辑股票成交失败, ledger: {}, trade: {}, err: {}",
            ledger_id,
            trade_id,
            error
        );
        return Err(error);
    }
    let updated = updated.ok_or_else(|| ServiceError::Internal("编辑成交后未取到记录".into()))?;
    Ok(StockTradeDto::from(&updated))
}

/// 删除整笔委托（含全部成交明细），并重放重建持仓、资金记录与轮次。
pub fn delete_trade_order(
    workspace: &Workspace,
    ledger_id: &str,
    order_id: &str,
) -> ServiceResult<()> {
    if order_id.is_empty() {
        return Err(AppError::bad_request("order_id is required").into());
    }
    if let Err(error) = workspace.transaction(|conn| {
        delete_trade_order_tx(conn, ledger_id, order_id)?;
        Ok(())
    }) {
        tracing::error!(
            "删除股票委托失败, ledger: {}, order: {}, err: {}",
            ledger_id,
            order_id,
            error
        );
        return Err(error);
    }
    Ok(())
}

/// 预演编辑/删除的影响：同一事务内执行改动并重放后强制回滚，
/// 返回变动后的持仓、可用现金，以及会因此失效（复盘丢失）的轮次。
///
/// 参数与命令面一一对应（`update_trade` / `delete_order` 复用同一入口）。
#[allow(clippy::too_many_arguments)]
pub fn preview_trade_change(
    workspace: &Workspace,
    ledger_id: &str,
    action: &str,
    trade_id: &str,
    order_id: &str,
    price_cents: i64,
    lots: i64,
    trade_time: i64,
) -> ServiceResult<StockTradeImpactDto> {
    let mut impact: Option<StockTradeImpactDto> = None;
    let result = workspace.transaction(|conn| {
        let before = round_meta_index(conn, ledger_id)?;

        let stock_code: String = match action {
            "update_trade" => {
                update_trade_fill_tx(conn, ledger_id, trade_id, price_cents, lots, trade_time)?
            }
            "delete_order" => delete_trade_order_tx(conn, ledger_id, order_id)?,
            _ => {
                return Err::<String, ServiceError>(AppError::bad_request("无效的预演动作").into())
            }
        };

        let after = round_meta_index(conn, ledger_id)?;

        let mut preview = StockTradeImpactDto {
            stock_code: stock_code.clone(),
            removed_rounds: Vec::new(),
            ..StockTradeImpactDto::default()
        };
        match StockDao::get_position(conn, ledger_id, &stock_code) {
            Ok(position) => {
                preview.position_after = position.quantity;
                preview.stock_name = position.stock_name;
            }
            Err(error) if is_not_found(&error) => {}
            Err(error) => return Err(ServiceError::Database(error)),
        }
        match StockDao::query_latest_fund_record(conn, ledger_id) {
            Ok(latest) => preview.cash_after = latest.cash_balance,
            Err(error) if is_not_found(&error) => {}
            Err(error) => return Err(ServiceError::Database(error)),
        }
        for (key, meta) in &before {
            if after.contains_key(key) {
                continue;
            }
            preview.removed_rounds.push(meta.clone());
        }
        preview.removed_rounds.sort_by(|left, right| {
            left.stock_code
                .cmp(&right.stock_code)
                .then(left.round_no.cmp(&right.round_no))
        });
        if preview.stock_name.is_empty() {
            if let Some(round) = preview
                .removed_rounds
                .iter()
                .find(|round| round.stock_code == stock_code)
            {
                preview.stock_name = round.stock_name.clone();
            }
        }
        impact = Some(preview);
        // 哨兵错误：强制回滚，预演绝不落库
        Err(ServiceError::Internal(ERR_PREVIEW_ROLLBACK.to_string()))
    });

    if let Err(error) = result {
        let rollback = matches!(&error, ServiceError::Internal(msg) if msg == ERR_PREVIEW_ROLLBACK);
        if !rollback {
            return Err(error);
        }
    }
    // 附带信息：本函数用到的键序在预览结果里由 sort_by 显式确定（见 removed_rounds 排序）
    impact.ok_or_else(|| AppError::bad_request("预演失败").into())
}

/// 读取当前轮次索引（用于比对编辑/删除后哪些轮次会失效）。
fn round_meta_index(
    conn: &rusqlite::Connection,
    ledger_id: &str,
) -> ServiceResult<BTreeMap<String, StockTradeImpactRoundDto>> {
    let rounds = db(StockDao::list_trade_rounds(conn, ledger_id))?;
    let histories = db(StockDao::list_trade_histories(conn, ledger_id))?;
    let stock_names: HashMap<String, String> = histories
        .into_iter()
        .map(|history| (history.stock_code, history.stock_name))
        .collect();

    let mut index = BTreeMap::new();
    for round in rounds {
        let meta = StockTradeImpactRoundDto {
            round_id: round.id.clone(),
            stock_code: round.stock_code.clone(),
            stock_name: stock_names
                .get(&round.stock_code)
                .cloned()
                .unwrap_or_default(),
            round_no: round.round_no,
            tag: round.tag.clone(),
            has_review: !round.review.trim().is_empty(),
        };
        index.insert(round_meta_key(&round.stock_code, round.round_no), meta);
    }
    Ok(index)
}

/// 事务内编辑一笔成交并重放，返回该成交所属股票代码。
fn update_trade_fill_tx(
    conn: &rusqlite::Connection,
    ledger_id: &str,
    trade_id: &str,
    price_cents: i64,
    lots: i64,
    trade_time: i64,
) -> ServiceResult<String> {
    let trade = match StockDao::get_trade(conn, trade_id) {
        Ok(trade) => trade,
        Err(error) if is_not_found(&error) => {
            return Err(AppError::not_found("交易记录不存在").into());
        }
        Err(error) => return Err(ServiceError::Database(error)),
    };
    if trade.ledger_id != ledger_id {
        return Err(AppError::not_found("交易记录不存在").into());
    }

    let order_id = order_key_of(&trade);
    let mut order_trades = db(StockDao::list_trades_by_order(conn, ledger_id, &order_id))?;
    if order_trades.is_empty() {
        // 兼容未回填委托的历史数据：该成交本身就是一笔独立委托
        let mut single = trade.clone();
        single.order_id = order_id;
        single.order_seq = 1;
        order_trades = vec![single];
    }

    let fee_setting = get_or_create_fee_setting_in(conn, ledger_id)?;
    let is_sh = fee::is_shanghai_code(&trade.stock_code);
    let buy = is_buy(&trade.trade_type);

    let mut amounts: Vec<i64> = Vec::with_capacity(order_trades.len());
    for item in order_trades.iter_mut() {
        if item.id == trade_id {
            item.price = price_cents;
            item.lots = lots;
            item.shares = lots * 100;
        }
        if trade_time > 0 {
            item.trade_time = trade_time;
        }
        item.amount = item.price * item.shares;
        amounts.push(item.amount);
    }

    let allocated = fee::allocate_order_fee(
        fee::compute_order_fee(&amounts, is_sh, &fee_setting, buy),
        &amounts,
        is_sh,
        &fee_setting,
        buy,
    );
    for (index, item) in order_trades.iter_mut().enumerate() {
        item.fee = allocated[index].total;
        item.commission = allocated[index].commission;
        item.stamp_duty = allocated[index].stamp_duty;
        item.transfer_fee = allocated[index].transfer_fee;
        db(StockDao::update_trade(conn, item))?;
    }

    rebuild_trades(conn, ledger_id)?;
    Ok(trade.stock_code)
}

/// 事务内删除整笔委托并重放，返回被删委托所属股票代码。
fn delete_trade_order_tx(
    conn: &rusqlite::Connection,
    ledger_id: &str,
    order_id: &str,
) -> ServiceResult<String> {
    if order_id.is_empty() {
        return Err(AppError::bad_request("order_id is required").into());
    }
    let mut trades = db(StockDao::list_trades_by_order(conn, ledger_id, order_id))?;
    if trades.is_empty() {
        // 兼容未回填委托的历史数据：把 orderId 当作单笔交易 ID
        let trade = match StockDao::get_trade(conn, order_id) {
            Ok(trade) => trade,
            Err(error) if is_not_found(&error) => {
                return Err(AppError::not_found("交易记录不存在").into());
            }
            Err(error) => return Err(ServiceError::Database(error)),
        };
        if trade.ledger_id != ledger_id {
            return Err(AppError::not_found("交易记录不存在").into());
        }
        trades = vec![trade];
    }

    let stock_code = trades[0].stock_code.clone();
    let ids: Vec<String> = trades.iter().map(|trade| trade.id.clone()).collect();
    db(StockDao::delete_trades_by_ids(conn, &ids))?;
    rebuild_trades(conn, ledger_id)?;
    Ok(stock_code)
}

/// 重放过程中累积的一笔委托。
struct ReplayOrder {
    key: String,
    stock_code: String,
    stock_name: String,
    is_buy: bool,
    amount: i64,
    fee: i64,
    lots: i64,
    net_pnl: i64,
    first_time: i64,
    last_time: i64,
    created_at: i64,
    indexes: Vec<usize>,
}

/// 按交易流重放，重建持仓、买卖资金记录、轮次与已实现盈亏等派生数据。
///
/// 费用沿用各笔已存的值（被编辑的委托已在调用前重算），因此重放不会改写历史费用；
/// 轮次标签/复盘按「股票 + 轮次序号」继承，持仓复盘草稿按股票继承。
///
/// 幂等性：轮次行按「股票 + 轮次序号」复用原 ID，重放结束后才删除不再成立的轮次，
/// 因此连续两次重放（成交未变）结果完全一致。
pub fn rebuild_trades(conn: &rusqlite::Connection, ledger_id: &str) -> ServiceResult<()> {
    let mut trades = db(StockDao::list_all_trades_asc(conn, ledger_id))?;

    let existing_rounds = db(StockDao::list_trade_rounds(conn, ledger_id))?;
    let mut round_meta: HashMap<String, StockTradeRound> = existing_rounds
        .iter()
        .map(|round| {
            (
                round_meta_key(&round.stock_code, round.round_no),
                round.clone(),
            )
        })
        .collect();
    let existing_positions = db(StockDao::list_positions(conn, ledger_id))?;

    // 清空派生数据：买卖资金记录、历史集合与轮次；本金/追加/支取记录保留
    db(StockDao::delete_trade_fund_records(conn, ledger_id))?;
    db(StockDao::delete_trade_histories_by_ledger(conn, ledger_id))?;
    // 轮次行按「股票 + 轮次序号」复用，保留原 ID、标签与复盘；重放结束后删除不再成立的轮次
    let mut used_rounds: Vec<String> = Vec::with_capacity(existing_rounds.len());

    let mut positions: HashMap<String, StockPosition> = existing_positions
        .into_iter()
        .map(|mut position| {
            position.quantity = 0;
            position.total_cost = 0;
            position.realized_pnl = 0;
            (position.stock_code.clone(), position)
        })
        .collect();

    let mut histories: HashMap<String, StockTradeHistory> = HashMap::new();
    let mut round_counts: HashMap<String, i64> = HashMap::new();
    let mut cycle_opened_at: HashMap<String, i64> = HashMap::new();
    let mut cycle_indexes: HashMap<String, Vec<usize>> = HashMap::new();
    // 把当前保留记录（本金/追加/支取）的最大时间戳作为重放起点，让重放记录排在其后。
    // （`create_fund_record` 已在 DAO 层保证新记录时间戳严格递增，这里仅用于重放路径。）
    let mut last_record_created_at: i64 =
        db(StockDao::list_fund_records_in_insert_order(conn, ledger_id))?
            .iter()
            .map(|record| record.created_at)
            .max()
            .unwrap_or(0);

    let mut current: Option<ReplayOrder> = None;

    for index in 0..trades.len() {
        // 委托键：与当前委托不同则先冲刷上一笔委托，再以**本笔自己的键**开新委托
        let key = order_key_of(&trades[index]);
        let needs_flush = match &current {
            Some(order) => order.key != key,
            None => true,
        };
        if needs_flush {
            flush_replay_order(
                conn,
                ledger_id,
                current.take(),
                &trades,
                &mut positions,
                &mut histories,
                &mut round_counts,
                &mut cycle_opened_at,
                &mut cycle_indexes,
                &mut round_meta,
                &mut used_rounds,
                &mut last_record_created_at,
            )?;
            let trade = &trades[index];
            current = Some(ReplayOrder {
                key,
                stock_code: trade.stock_code.clone(),
                stock_name: trade.stock_name.clone(),
                is_buy: is_buy(&trade.trade_type),
                amount: 0,
                fee: 0,
                lots: 0,
                net_pnl: 0,
                first_time: trade.trade_time,
                last_time: 0,
                created_at: trade.created_at,
                indexes: Vec::new(),
            });
        }

        {
            let trade = &trades[index];
            if let Some(order) = current.as_mut() {
                if !trade.stock_name.is_empty() {
                    order.stock_name = trade.stock_name.clone();
                }
                order.amount += trade.amount;
                order.fee += trade.fee;
                order.lots += trade.lots;
                order.last_time = trade.trade_time;
                order.indexes.push(index);
            }

            if !positions.contains_key(&trade.stock_code) {
                let position = StockPosition {
                    id: tr_store::util::new_uuid(),
                    ledger_id: ledger_id.to_string(),
                    stock_code: trade.stock_code.clone(),
                    stock_name: trade.stock_name.clone(),
                    ..StockPosition::default()
                };
                db(StockDao::create_position(conn, &position))?;
                positions.insert(trade.stock_code.clone(), position);
            }
        }

        let is_buy_trade = is_buy(&trades[index].trade_type);
        let stock_code = trades[index].stock_code.clone();
        let shares = trades[index].shares;
        let amount = trades[index].amount;
        let fee = trades[index].fee;
        let trade_time = trades[index].trade_time;
        let order_stock_name = current
            .as_ref()
            .map(|order| order.stock_name.clone())
            .unwrap_or_default();

        trades[index].round_id = String::new();
        trades[index].realized_pnl = None;

        let position = positions
            .get_mut(&stock_code)
            .expect("重放前已确保持仓存在");
        if !order_stock_name.is_empty() {
            position.stock_name = order_stock_name;
        }

        if is_buy_trade {
            if position.quantity == 0 {
                cycle_opened_at.insert(stock_code.clone(), trade_time);
                cycle_indexes.insert(stock_code.clone(), Vec::new());
            }
            position.quantity += shares;
            position.total_cost += amount + fee;
            cycle_indexes.entry(stock_code).or_default().push(index);
            continue;
        }

        if cycle_opened_at.get(&stock_code).copied().unwrap_or(0) == 0 {
            cycle_opened_at.insert(stock_code.clone(), trade_time);
        }
        if shares > position.quantity {
            return Err(AppError::bad_request(format!(
                "卖出数量超过持仓（当前 {} 股）",
                position.quantity
            ))
            .into());
        }
        let cost_basis = cost_basis_of(position.total_cost, shares, position.quantity);
        let realized = amount - fee - cost_basis;
        trades[index].realized_pnl = Some(realized);
        if let Some(order) = current.as_mut() {
            order.net_pnl += realized;
        }
        cycle_indexes.entry(stock_code).or_default().push(index);
        position.quantity -= shares;
        position.total_cost -= cost_basis;
        position.realized_pnl += realized;
        if position.quantity == 0 {
            position.total_cost = 0;
        }
    }

    flush_replay_order(
        conn,
        ledger_id,
        current.take(),
        &trades,
        &mut positions,
        &mut histories,
        &mut round_counts,
        &mut cycle_opened_at,
        &mut cycle_indexes,
        &mut round_meta,
        &mut used_rounds,
        &mut last_record_created_at,
    )?;

    // 重放后不再成立的轮次随编辑/删除失效（其复盘内容一并丢失）
    for round in &existing_rounds {
        if used_rounds.iter().any(|id| id == &round.id) {
            continue;
        }
        db(StockDao::delete_trade_round(conn, &round.id))?;
    }

    for position in positions.values_mut() {
        if position.quantity == 0 {
            // 已清仓：本轮复盘已归档到轮次，持仓上的草稿不再保留
            position.review = String::new();
        }
        db(StockDao::update_position(conn, position))?;
    }

    recalculate_cash_chain(conn, ledger_id)
}

/// 结束当前委托的重放：写一条资金记录，必要时归档轮次，并回写各笔成交的派生字段。
///
/// 抽成独立函数是因为 Rust 里一棵同时借用
/// `trades` 与多个可变 map 的闭包无法通过借用检查。
#[allow(clippy::too_many_arguments)]
fn flush_replay_order(
    conn: &rusqlite::Connection,
    ledger_id: &str,
    order: Option<ReplayOrder>,
    trades: &[StockTrade],
    positions: &mut HashMap<String, StockPosition>,
    histories: &mut HashMap<String, StockTradeHistory>,
    round_counts: &mut HashMap<String, i64>,
    cycle_opened_at: &mut HashMap<String, i64>,
    cycle_indexes: &mut HashMap<String, Vec<usize>>,
    round_meta: &mut HashMap<String, StockTradeRound>,
    used_rounds: &mut Vec<String>,
    last_record_created_at: &mut i64,
) -> ServiceResult<()> {
    let Some(order) = order else {
        return Ok(());
    };

    let (amount_change, event_type, event_text, net_pnl) = if order.is_buy {
        (
            -(order.amount + order.fee),
            consts::STOCK_EVENT_BUY,
            format!("买入 {} {}手", order.stock_name, order.lots),
            None,
        )
    } else {
        (
            order.amount - order.fee,
            consts::STOCK_EVENT_SELL,
            format!("卖出 {} {}手", order.stock_name, order.lots),
            Some(order.net_pnl),
        )
    };
    let mut record = StockFundRecord {
        id: tr_store::util::new_uuid(),
        ledger_id: ledger_id.to_string(),
        record_date: unix_to_date(order.last_time),
        event_type: event_type.to_string(),
        event_text,
        amount_change,
        cash_balance: 0,
        net_pnl,
        remark: trade_order_remark(
            &order.stock_name,
            order.lots,
            order.amount,
            order.indexes.len(),
        ),
        created_at: order.created_at,
    };
    // 录入顺序 = `created_at ASC, id ASC`。created_at 是秒级的，
    // 同一秒录入的多条记录靠随机 UUID 决胜负，顺序不确定；这里在重放时把时间戳
    // 拉成严格递增，使重放结果与资金链重算完全确定（不同秒的原值保持不变）。
    record.created_at = record
        .created_at
        .max(last_record_created_at.saturating_add(1));
    *last_record_created_at = record.created_at;
    db(StockDao::create_fund_record(conn, &record))?;

    // 委托全部成交后持仓归零：归档本轮「建仓 → 清仓」
    let mut round_id = String::new();
    let closed = positions
        .get(&order.stock_code)
        .map(|position| !order.is_buy && position.quantity == 0)
        .unwrap_or(false);

    if closed {
        let history = match histories.get(&order.stock_code) {
            Some(history) => history.clone(),
            None => {
                let history = StockTradeHistory {
                    id: tr_store::util::new_uuid(),
                    ledger_id: ledger_id.to_string(),
                    stock_code: order.stock_code.clone(),
                    stock_name: order.stock_name.clone(),
                    ..StockTradeHistory::default()
                };
                db(StockDao::create_trade_history(conn, &history))?;
                histories.insert(order.stock_code.clone(), history.clone());
                history
            }
        };
        let round_no = round_counts.get(&order.stock_code).copied().unwrap_or(0) + 1;
        let mut opened_at = cycle_opened_at.get(&order.stock_code).copied().unwrap_or(0);
        if opened_at == 0 {
            opened_at = order.first_time;
        }
        let meta_key = round_meta_key(&order.stock_code, round_no);
        match round_meta.get(&meta_key) {
            Some(existing) => {
                // 复用同一轮次行：ID 稳定，标签与复盘原样保留
                db(StockDao::update_trade_round_derived(
                    conn,
                    &existing.id,
                    &history.id,
                    opened_at,
                    order.last_time,
                ))?;
                round_id = existing.id.clone();
            }
            None => {
                let tag = consts::STOCK_TAG_ANALYSIS.to_string();
                let round = StockTradeRound {
                    id: tr_store::util::new_uuid(),
                    ledger_id: ledger_id.to_string(),
                    stock_code: order.stock_code.clone(),
                    history_id: history.id,
                    round_no,
                    opened_at,
                    closed_at: order.last_time,
                    tag,
                    review: String::new(),
                    created_at: 0,
                };
                db(StockDao::create_trade_round(conn, &round))?;
                round_id = round.id.clone();
                // 新轮次加入索引，避免同一股票出现两轮同号时重复创建
                round_meta.insert(meta_key, round);
            }
        }
        used_rounds.push(round_id.clone());
        // 本轮「建仓 → 清仓」的全部成交（可能跨多个委托）一起挂接轮次
        let ids: Vec<String> = cycle_indexes
            .get(&order.stock_code)
            .map(|indexes| {
                indexes
                    .iter()
                    .map(|index| trades[*index].id.clone())
                    .collect()
            })
            .unwrap_or_default();
        db(StockDao::update_trades_round_id(conn, &round_id, &ids))?;
        round_counts.insert(order.stock_code.clone(), round_no);
        cycle_opened_at.remove(&order.stock_code);
        cycle_indexes.remove(&order.stock_code);
    }

    for index in &order.indexes {
        db(StockDao::update_trade_settlement(
            conn,
            &trades[*index].id,
            &round_id,
            trades[*index].realized_pnl,
        ))?;
    }
    Ok(())
}

/// 重放后重算资金记录的现金余额，精确复刻既有的链条规则：
///
/// 按录入顺序逐条结算，每条的前值取「当前已存在记录里 (日期 → 创建时间 → ID) 最大一条」的余额
/// （与 `QueryLatestFundRecord` 口径一致，且**只在前 i 条里找**）；
/// 首条记录的起点 = 本金 − Σ追加本金。
///
/// 该规则在补录历史日期交易时并非单纯按日期排序，因此这里同样逐条取最大值而不是重排序。
pub fn recalculate_cash_chain(conn: &rusqlite::Connection, ledger_id: &str) -> ServiceResult<()> {
    let account = get_or_create_account_in(conn, ledger_id)?;
    let records = db(StockDao::list_fund_records_in_insert_order(conn, ledger_id))?;
    let mut cash = account.principal;
    for record in &records {
        if record.event_type == consts::STOCK_EVENT_ADD_PRINCIPAL {
            cash -= record.amount_change;
        }
    }

    let mut balances = vec![0_i64; records.len()];
    for i in 0..records.len() {
        if i > 0 {
            let mut latest = 0_usize;
            for j in 1..i {
                if fund_record_after(&records[j], &records[latest]) {
                    latest = j;
                }
            }
            cash = balances[latest];
        }
        cash += records[i].amount_change;
        balances[i] = cash;
        if records[i].cash_balance == cash {
            continue;
        }
        db(StockDao::update_fund_record_cash_balance(
            conn,
            &records[i].id,
            cash,
        ))?;
    }
    Ok(())
}

/// 判断 `a` 是否比 `b` 更「新」：(record_date, created_at, id) 三者依次比较。
fn fund_record_after(a: &StockFundRecord, b: &StockFundRecord) -> bool {
    if a.record_date != b.record_date {
        return a.record_date > b.record_date;
    }
    if a.created_at != b.created_at {
        return a.created_at > b.created_at;
    }
    a.id > b.id
}

// ==================================================================== 操作记录与回滚

/// 目标不存在（404）。
///
/// 回滚时用它区分两种失败：目标被别的改动带走了（**跳过**，把记录弹掉）与真的出错（原样上报）。
fn is_missing_target(error: &ServiceError) -> bool {
    matches!(error, ServiceError::App(app) if app.status == 404)
}

/// 某账本的操作记录（最新的在前，最多 [`STOCK_OPERATION_LIMIT`] 条）。
pub fn list_operations(
    workspace: &Workspace,
    ledger_id: &str,
) -> ServiceResult<Vec<StockOperationDto>> {
    let conn = workspace.connection();
    let rows = db(StockDao::list_operations(
        &conn,
        ledger_id,
        STOCK_OPERATION_LIMIT,
    ))?;
    Ok(rows
        .into_iter()
        .map(|operation| StockOperationDto {
            id: operation.id,
            action: operation.action,
            detail: operation.detail,
            created_at: operation.created_at,
        })
        .collect())
}

/// 回滚预演：要撤销的是哪一次操作、会不会让某些轮次失效（复盘随之丢失）。
///
/// 与 [`preview_trade_change`] 同一手法：委托类操作在事务里**真的执行撤销**、比对前后的轮次索引，
/// 最后返回哨兵错误 [`ERR_PREVIEW_ROLLBACK`] 强制回滚 —— **预演绝不落库**。
/// 资金类操作没有轮次影响，只回填操作名与摘要。
pub fn preview_rollback(
    workspace: &Workspace,
    ledger_id: &str,
) -> ServiceResult<StockOperationRollbackPreviewDto> {
    let mut preview: Option<StockOperationRollbackPreviewDto> = None;
    let result = workspace.transaction(|conn| {
        let operation = match StockDao::latest_operation(conn, ledger_id) {
            Ok(operation) => operation,
            Err(error) if is_not_found(&error) => {
                return Err::<(), ServiceError>(AppError::bad_request("没有可回滚的操作").into());
            }
            Err(error) => return Err(ServiceError::Database(error)),
        };
        let mut preview_for_call = StockOperationRollbackPreviewDto {
            action: operation.action.clone(),
            detail: operation.detail.clone(),
            removed_rounds: Vec::new(),
        };
        if operation.kind == consts::STOCK_OP_KIND_ORDER {
            let before = round_meta_index(conn, ledger_id)?;
            // 目标可能已经被别的改动删掉了（例如手动删除整笔委托）：那不是错误，
            // 实际回滚会走"跳过"路径，预演里也就没有失效轮次
            if delete_trade_order_tx(conn, ledger_id, &operation.target_id).is_ok() {
                let after = round_meta_index(conn, ledger_id)?;
                for (key, meta) in &before {
                    if !after.contains_key(key) {
                        preview_for_call.removed_rounds.push(meta.clone());
                    }
                }
                preview_for_call.removed_rounds.sort_by(|left, right| {
                    left.stock_code
                        .cmp(&right.stock_code)
                        .then(left.round_no.cmp(&right.round_no))
                });
            }
        }
        preview = Some(preview_for_call);
        // 哨兵错误：强制回滚，预演绝不落库
        Err(ServiceError::Internal(ERR_PREVIEW_ROLLBACK.to_string()))
    });

    if let Err(error) = result {
        let rollback = matches!(&error, ServiceError::Internal(msg) if msg == ERR_PREVIEW_ROLLBACK);
        if !rollback {
            return Err(error);
        }
    }
    preview.ok_or_else(|| AppError::bad_request("没有可回滚的操作").into())
}

/// 回滚最新一次操作（撤销成功后把它从记录里弹掉）。
///
/// 撤销就是两个"逆操作"，不引入快照：
/// * **委托类** → [`delete_trade_order_tx`]：删掉该委托的全部成交后重放持仓 / 轮次 / 资金记录；
/// * **资金类** → 删掉那条资金记录；「追加本金」还要把 `principal` 减回去，最后重算现金链。
///
/// **目标已不存在**不算失败：把记录弹掉并返回 `skipped = true`。否则一条指向空气的记录会永远
/// 卡在栈顶，之后每一次回滚都失败。
///
/// 回滚本身**不产生新记录**：它是弹栈，不是新的正向操作（否则"回滚"就成了可回滚的操作）。
pub fn rollback_latest(
    workspace: &Workspace,
    ledger_id: &str,
) -> ServiceResult<StockOperationRollbackDto> {
    let mut outcome: Option<StockOperationRollbackDto> = None;
    if let Err(error) = workspace.transaction(|conn| {
        let operation = match StockDao::latest_operation(conn, ledger_id) {
            Ok(operation) => operation,
            Err(error) if is_not_found(&error) => {
                return Err(AppError::bad_request("没有可回滚的操作").into());
            }
            Err(error) => return Err(ServiceError::Database(error)),
        };

        let mut skipped = false;
        match operation.kind.as_str() {
            consts::STOCK_OP_KIND_ORDER => {
                if let Err(error) = delete_trade_order_tx(conn, ledger_id, &operation.target_id) {
                    if is_missing_target(&error) {
                        skipped = true;
                    } else {
                        return Err(error);
                    }
                }
            }
            consts::STOCK_OP_KIND_FUND => {
                match StockDao::get_fund_record(conn, &operation.target_id) {
                    Ok(record) => {
                        // 追加本金动过本金口径，撤销时要改回去（额度就是这条记录的 amount_change）
                        if record.event_type == consts::STOCK_EVENT_ADD_PRINCIPAL {
                            let account = get_or_create_account_in(conn, ledger_id)?;
                            // `max(0)` 是防御式的：追加本金只会把本金抬高，理论上取不到负值
                            let restored = (account.principal - record.amount_change).max(0);
                            db(StockDao::update_account_principal(
                                conn, ledger_id, restored,
                            ))?;
                        }
                        db(StockDao::delete_fund_record(conn, &operation.target_id))?;
                        recalculate_cash_chain(conn, ledger_id)?;
                    }
                    Err(error) if is_not_found(&error) => skipped = true,
                    Err(error) => return Err(ServiceError::Database(error)),
                }
            }
            // 未知 kind（理论上不会出现）：只弹记录，不做任何撤销
            _ => skipped = true,
        }

        db(StockDao::delete_operation(conn, &operation.id))?;
        outcome = Some(StockOperationRollbackDto {
            action: operation.action.clone(),
            skipped,
        });
        Ok(())
    }) {
        tracing::error!("回滚股票操作失败, ledger: {}, err: {}", ledger_id, error);
        return Err(error);
    }
    outcome.ok_or_else(|| ServiceError::Internal("回滚后未取到结果".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::workspace;
    use tr_domain::dto::StockQuoteDto;

    /// 测试用的账本 / 股票常量。
    const TEST_LEDGER_ID: &str = "ledger-history";
    const TEST_CODE: &str = "600000";
    const TEST_NAME: &str = "浦发银行";
    const TEST_CODE_B: &str = "000001";
    const TEST_NAME_B: &str = "平安银行";
    const TEST_ORDER_CODE: &str = "605258";
    const TEST_ORDER_NAME: &str = "协和电子";

    /// 测试用行情源：只返回预设代码，模拟部分/全部获取失败。
    struct StubQuoteFetcher {
        quotes: HashMap<String, StockQuoteDto>,
        name: String,
    }

    impl StubQuoteFetcher {
        fn empty() -> Self {
            Self {
                quotes: HashMap::new(),
                name: String::new(),
            }
        }

        fn with(quotes: HashMap<String, StockQuoteDto>) -> Self {
            Self {
                quotes,
                name: String::new(),
            }
        }
    }

    impl StockQuoteFetcher for StubQuoteFetcher {
        fn fetch_quotes(&self, codes: &[String]) -> HashMap<String, StockQuoteDto> {
            codes
                .iter()
                .filter_map(|code| {
                    self.quotes
                        .get(code)
                        .map(|quote| (code.clone(), quote.clone()))
                })
                .collect()
        }

        fn fetch_name(&self, _code: &str) -> String {
            self.name.clone()
        }
    }

    fn quote(code: &str, latest: i64, prev_close: i64, quote_time: i64) -> StockQuoteDto {
        StockQuoteDto {
            stock_code: code.to_string(),
            latest_price: latest,
            prev_close,
            quote_time,
        }
    }

    /// 设置本金。
    fn set_principal_amount(workspace: &Workspace, amount: i64) -> ServiceResult<StockOverviewDto> {
        set_principal(workspace, TEST_LEDGER_ID, amount)
    }

    /// 设置本金并断言成功。
    fn seed_principal(workspace: &Workspace, amount: i64) {
        set_principal_amount(workspace, amount).unwrap();
    }

    /// 建仓后立即清仓，产生一条已归档轮次。
    fn close_round_helper(
        workspace: &Workspace,
        code: &str,
        name: &str,
        open_cents: i64,
        close_cents: i64,
    ) {
        create_trade(
            workspace,
            TEST_LEDGER_ID,
            code,
            name,
            consts::STOCK_TRADE_OPEN,
            open_cents,
            10,
            1_700_005_000,
            "",
            "",
        )
        .unwrap();
        create_trade(
            workspace,
            TEST_LEDGER_ID,
            code,
            name,
            consts::STOCK_TRADE_CLOSE,
            close_cents,
            10,
            1_700_005_100,
            "",
            "",
        )
        .unwrap();
    }

    /// 直接用时间戳造一轮干净结算，建仓比清仓早一分钟。
    fn seed_clean_round_from(
        workspace: &Workspace,
        code: &str,
        name: &str,
        open_price: i64,
        close_price: i64,
        lots: i64,
        close_at: i64,
    ) {
        let conn = workspace.connection();
        for (trade_type, price, at) in [
            (
                consts::STOCK_TRADE_OPEN,
                open_price,
                close_at - if close_at > 60 { 60 } else { 0 },
            ),
            (consts::STOCK_TRADE_CLOSE, close_price, close_at),
        ] {
            let trade = StockTrade {
                id: tr_store::util::new_uuid(),
                ledger_id: TEST_LEDGER_ID.to_string(),
                stock_code: code.to_string(),
                stock_name: name.to_string(),
                trade_type: trade_type.to_string(),
                price,
                lots,
                shares: lots * 100,
                amount: price * lots * 100,
                trade_time: at,
                ..StockTrade::default()
            };
            StockDao::create_trade(&conn, &trade).unwrap();
        }
    }

    /// 用 `YYYY-MM-DD HH:MM:SS` 的 UTC 时间造一轮结算（跨月/跨年造数用）。
    fn seed_clean_round_on(
        workspace: &Workspace,
        code: &str,
        name: &str,
        open_price: i64,
        close_price: i64,
        lots: i64,
        close_at: &str,
    ) {
        let timestamp = utc_timestamp(close_at);
        seed_clean_round_from(
            workspace,
            code,
            name,
            open_price,
            close_price,
            lots,
            timestamp,
        );
    }

    /// `YYYY-MM-DD HH:MM:SS`（UTC）→ Unix 秒。
    fn utc_timestamp(text: &str) -> i64 {
        let (date, time) = text.split_once(' ').expect("需要 `YYYY-MM-DD HH:MM:SS`");
        let (year, month, day) = parse_strict_date(date).expect("日期非法");
        let parts: Vec<u32> = time.split(':').map(|p| p.parse().unwrap()).collect();
        chrono::NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(parts[0], parts[1], parts[2])
            .unwrap()
            .and_utc()
            .timestamp()
    }

    /// 备注必须逐笔落库：**非零值字段参与 INSERT**
    /// （曾被误读成"remark 一律不写"，回归对比因此报出 4 行差异）。
    #[test]
    fn create_trade_order_persists_remark_on_every_fill() {
        let (workspace, dir) = workspace("order-remark");
        seed_principal(&workspace, 10_000_000);

        create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_ORDER_CODE,
            TEST_ORDER_NAME,
            consts::STOCK_TRADE_OPEN,
            &[
                TradeFill {
                    price_cents: 3806,
                    lots: 1,
                },
                TradeFill {
                    price_cents: 3806,
                    lots: 2,
                },
            ],
            1_700_000_000,
            "种子建仓",
            "",
        )
        .unwrap();

        let trades = list_trades(&workspace, TEST_LEDGER_ID, TEST_ORDER_CODE).unwrap();
        assert_eq!(trades.len(), 2);
        assert!(
            trades.iter().all(|trade| trade.remark == "种子建仓"),
            "两笔成交都应带上委托备注: {trades:?}"
        );

        // 空备注与列默认值 '' 等价（零值字段省略，走列默认值）。
        create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE_B,
            TEST_NAME_B,
            consts::STOCK_TRADE_OPEN,
            &[TradeFill {
                price_cents: 1000,
                lots: 1,
            }],
            1_700_000_100,
            "",
            "",
        )
        .unwrap();
        let blank = list_trades(&workspace, TEST_LEDGER_ID, TEST_CODE_B).unwrap();
        assert_eq!(blank[0].remark, "");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 回归：`rebuild_trades`（重放）之后对**同一只股票**继续下单，必须读到最新状态。
    ///
    /// 背景：回归验证中曾报告过一个疑似"重放后读到旧快照"的缺陷（预检放行超卖、
    /// 减仓手数被写错），但**这个最小序列（建仓 2 笔 → 编辑成交触发重放 → 减仓 1 手）
    /// 在本仓库上是通过的**，所以那条报告尚未证实。这条测试先把最容易出错的不变量固定住：
    /// 重放之后的下一次下单，持仓与写入的手数都必须是新的。
    #[test]
    fn create_order_after_rebuild_reads_fresh_state() {
        let (workspace, dir) = workspace("fresh-after-rebuild");
        seed_principal(&workspace, 100_000_000);

        // 建仓 2 手（一笔委托、两笔成交）
        let opens = create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            &[
                TradeFill {
                    price_cents: 3806,
                    lots: 1,
                },
                TradeFill {
                    price_cents: 3806,
                    lots: 1,
                },
            ],
            1_700_000_000,
            "建仓",
            "",
        )
        .unwrap();
        assert_eq!(opens.len(), 2);

        // 触发一次重放：编辑第二笔成交的成交价
        update_trade_fill(
            &workspace,
            TEST_LEDGER_ID,
            &opens[1].id,
            3810,
            1,
            1_700_000_000,
        )
        .unwrap();

        // 重放之后再减仓 1 手：预检必须看到"还有 200 股"
        create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_REDUCE,
            &[TradeFill {
                price_cents: 3900,
                lots: 1,
            }],
            1_700_000_600,
            "减仓",
            "",
        )
        .unwrap();

        let conn = workspace.connection();
        let position = StockDao::get_position(&conn, TEST_LEDGER_ID, TEST_CODE).unwrap();
        assert_eq!(position.quantity, 100, "减仓 1 手后应剩 1 手（100 股）");

        let trades = StockDao::list_all_trades_asc(&conn, TEST_LEDGER_ID).unwrap();
        assert_eq!(trades.len(), 3, "建仓 2 笔 + 减仓 1 笔");
        let reduce = trades
            .iter()
            .find(|trade| trade.trade_type == consts::STOCK_TRADE_REDUCE)
            .expect("应当有减仓成交");
        assert_eq!(reduce.lots, 1, "写入库的减仓手数必须是 1");
        assert_eq!(reduce.shares, 100, "写入库的减仓股数必须是 100");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 证伪用：完整重放曾失败的那条序列（只用公开服务 API）。
    ///
    /// 与上一条测试的区别：这里先把「阶段 1/2 结束时的真实状态」搭出来
    /// （一轮已归档的 600519、持仓归零、一次 update_trade_fill、一次 delete_trade_order），
    /// 然后才执行阶段 3 的减仓序列。如果连接池/快照真有「重放后读旧状态」，
    /// 这条应当失败；如果通过，说明阶段 3 的失败来自脚本侧。
    ///
    /// 结论（本测试通过）：**服务层是对的**，阶段 3 的失败源自 seed 脚本的两处笔误——
    /// ① 把两笔减仓写成同一个 `trade_time`（`TRADE_TIME_R3_REDUCE` 用了两次），
    ///    导致同一时间戳、同一 `order_seq` 的两笔卖单在重放里被并成一笔，
    ///    库中出现 `lots=1` 却 `shares=300` 的不自洽行；
    /// ② 手数不守恒（买入 2 手却卖出 1+1+2 手）。
    /// 详见两个工作空间里那条 `RAW-000001 ["open/3/300","reduce/3/300",...]`。
    ///
    /// 序列（手数守恒：买入 3+1=4 手，卖出 1+1+2=4 手，两个减仓时间戳不同）：
    ///   600519: open(2笔) → add → close   （阶段 1，归档第 1 轮）
    ///   600519: open 1 → 编辑成交价        （阶段 2 的 update_trade_fill，触发重放）
    ///   600519: open 1 → 删除该委托        （阶段 2 的 delete_trade_order，再次重放）
    ///   000001: open 3 → reduce 1 → reduce 1 → add 1 → close 2   （阶段 3）
    #[test]
    fn phase3_reduce_sequence_matches_expected_lots() {
        let (workspace, dir) = workspace("phase3-reduce");
        seed_principal(&workspace, 100_000_000);

        // ---- 阶段 1：600519 建仓 2 笔 + 加仓 + 清仓（归档第 1 轮）----
        let opens = create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            &[
                TradeFill {
                    price_cents: 170_000,
                    lots: 1,
                },
                TradeFill {
                    price_cents: 170_150,
                    lots: 1,
                },
            ],
            1_767_657_600,
            "种子建仓",
            "",
        )
        .unwrap();
        create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_ADD,
            &[TradeFill {
                price_cents: 169_500,
                lots: 1,
            }],
            1_768_867_200,
            "种子加仓",
            "",
        )
        .unwrap();
        create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_CLOSE,
            &[TradeFill {
                price_cents: 175_000,
                lots: 3,
            }],
            1_773_100_800,
            "种子清仓",
            "打板",
        )
        .unwrap();

        // ---- 阶段 2a：编辑建仓第 2 笔成交价（触发第一次 rebuild_trades）----
        update_trade_fill(
            &workspace,
            TEST_LEDGER_ID,
            &opens[1].id,
            170_200,
            1,
            1_767_657_600,
        )
        .unwrap();

        // ---- 阶段 2b：再建仓 1 手 → 删除该委托（触发第二次 rebuild_trades）----
        let reopen = create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            &[TradeFill {
                price_cents: 168_000,
                lots: 1,
            }],
            1_775_779_200,
            "二次建仓",
            "",
        )
        .unwrap();
        delete_trade_order(&workspace, TEST_LEDGER_ID, &reopen[0].order_id).unwrap();

        let conn = workspace.connection();
        let position = StockDao::get_position(&conn, TEST_LEDGER_ID, TEST_CODE).unwrap();
        assert_eq!(position.quantity, 0, "阶段 2 结束后 600519 应已清仓");
        drop(conn);

        // ---- 阶段 3：000001 建仓 3 手 → 两次减仓 → 加仓 → 清仓 2 手 ----
        create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            "000001",
            "平安银行",
            consts::STOCK_TRADE_OPEN,
            &[TradeFill {
                price_cents: 112_000,
                lots: 3,
            }],
            1_779_408_000,
            "减仓建仓",
            "",
        )
        .unwrap();
        create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            "000001",
            "平安银行",
            consts::STOCK_TRADE_REDUCE,
            &[TradeFill {
                price_cents: 115_000,
                lots: 1,
            }],
            1_779_494_400,
            "减仓甲",
            "",
        )
        .unwrap();
        create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            "000001",
            "平安银行",
            consts::STOCK_TRADE_REDUCE,
            &[TradeFill {
                price_cents: 108_000,
                lots: 1,
            }],
            1_779_580_800,
            "减仓乙",
            "",
        )
        .unwrap();
        create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            "000001",
            "平安银行",
            consts::STOCK_TRADE_ADD,
            &[TradeFill {
                price_cents: 120_000,
                lots: 1,
            }],
            1_779_667_200,
            "减仓后加仓",
            "",
        )
        .unwrap();
        create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            "000001",
            "平安银行",
            consts::STOCK_TRADE_CLOSE,
            &[TradeFill {
                price_cents: 118_000,
                lots: 2,
            }],
            1_779_753_600,
            "减仓后清仓",
            "尾盘",
        )
        .unwrap();

        // ---- 断言：库里 000001 的每一行都必须是设计的手数 ----
        let conn = workspace.connection();
        let trades = StockDao::list_trades_asc(&conn, TEST_LEDGER_ID, "000001").unwrap();
        let shape: Vec<(String, i64, i64)> = trades
            .iter()
            .map(|trade| (trade.trade_type.clone(), trade.lots, trade.shares))
            .collect();
        assert_eq!(
            shape,
            vec![
                (consts::STOCK_TRADE_OPEN.to_string(), 3, 300),
                (consts::STOCK_TRADE_REDUCE.to_string(), 1, 100),
                (consts::STOCK_TRADE_REDUCE.to_string(), 1, 100),
                (consts::STOCK_TRADE_ADD.to_string(), 1, 100),
                (consts::STOCK_TRADE_CLOSE.to_string(), 2, 200),
            ],
            "000001 的成交手数序列被写坏了"
        );

        let position = StockDao::get_position(&conn, TEST_LEDGER_ID, "000001").unwrap();
        assert_eq!(position.quantity, 0, "清仓后 000001 持仓必须归零");

        // 两次减仓的已实现盈亏必须按「剩余总成本比例」结转（不是整笔）
        let reduces: Vec<i64> = trades
            .iter()
            .filter(|trade| trade.trade_type == consts::STOCK_TRADE_REDUCE)
            .map(|trade| trade.realized_pnl.expect("减仓必须有 realized_pnl"))
            .collect();
        assert_eq!(reduces.len(), 2);
        assert!(
            reduces.iter().all(|pnl| *pnl != 0),
            "两次减仓都应按比例结转出非零盈亏，实际 {reduces:?}"
        );

        // 阶段 1 的 600519 成交必须完好（4 笔），不能被阶段 3 的重放吃掉
        let old = StockDao::list_trades_asc(&conn, TEST_LEDGER_ID, TEST_CODE).unwrap();
        assert_eq!(old.len(), 4, "600519 应保留建仓 2 笔 + 加仓 + 清仓");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 复现：第三轮归档后编辑该轮建仓（1手→3手），重放**不得丢掉该轮的清仓成交**。
    ///
    /// 曾观察到的现象：编辑之后，库里只剩第三轮的 `close`、
    /// 第三轮的 `open` 消失，紧接着的重放报「卖出数量超过持仓（当前 0 股）」。
    #[test]
    fn editing_third_round_open_keeps_its_close_trade() {
        let (workspace, dir) = workspace("edit-r3-open");
        seed_principal(&workspace, 100_000_000);

        // 阶段 1：600519 建仓 2 笔 + 加仓 + 清仓
        create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            &[
                TradeFill {
                    price_cents: 170_000,
                    lots: 1,
                },
                TradeFill {
                    price_cents: 170_150,
                    lots: 1,
                },
            ],
            1_767_657_600,
            "种子建仓",
            "",
        )
        .unwrap();
        create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_ADD,
            &[TradeFill {
                price_cents: 169_500,
                lots: 1,
            }],
            1_768_867_200,
            "种子加仓",
            "",
        )
        .unwrap();
        create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_CLOSE,
            &[TradeFill {
                price_cents: 175_000,
                lots: 3,
            }],
            1_773_100_800,
            "种子清仓",
            "打板",
        )
        .unwrap();

        // 阶段 2：编辑建仓第 2 笔（重放）+ 二次建仓后删除（重放）
        let open_fills =
            StockDao::list_trades_asc(&workspace.connection(), TEST_LEDGER_ID, TEST_CODE).unwrap();
        let second_open = open_fills
            .iter()
            .find(|trade| trade.trade_time == 1_767_657_600 && trade.order_seq == 2)
            .expect("建仓第 2 笔")
            .id
            .clone();
        update_trade_fill(
            &workspace,
            TEST_LEDGER_ID,
            &second_open,
            170_200,
            1,
            1_767_657_600,
        )
        .unwrap();
        let reopen = create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            &[TradeFill {
                price_cents: 168_000,
                lots: 1,
            }],
            1_775_779_200,
            "二次建仓",
            "",
        )
        .unwrap();
        delete_trade_order(&workspace, TEST_LEDGER_ID, &reopen[0].order_id).unwrap();

        // 阶段 3-1：第二轮 建仓3 → 减仓1 → 减仓1 → 加仓1 → 清仓2
        create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            &[TradeFill {
                price_cents: 198_000,
                lots: 3,
            }],
            1_778_198_400,
            "第二轮建仓",
            "",
        )
        .unwrap();
        for (price, at, remark) in [
            (199_000_i64, 1_778_284_800_i64, "第二轮减仓甲"),
            (197_500, 1_778_371_200, "第二轮减仓乙"),
        ] {
            create_trade_order(
                &workspace,
                TEST_LEDGER_ID,
                TEST_CODE,
                TEST_NAME,
                consts::STOCK_TRADE_REDUCE,
                &[TradeFill {
                    price_cents: price,
                    lots: 1,
                }],
                at,
                remark,
                "",
            )
            .unwrap();
        }
        create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_ADD,
            &[TradeFill {
                price_cents: 201_000,
                lots: 1,
            }],
            1_778_457_600,
            "第二轮加仓",
            "",
        )
        .unwrap();
        create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_CLOSE,
            &[TradeFill {
                price_cents: 200_000,
                lots: 2,
            }],
            1_778_544_000,
            "第二轮清仓",
            "追涨",
        )
        .unwrap();

        // 阶段 3-2：第三轮 建仓1 → 清仓1
        let r3 = create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            &[TradeFill {
                price_cents: 190_000,
                lots: 1,
            }],
            1_778_716_800,
            "第三轮建仓",
            "",
        )
        .unwrap();
        create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_CLOSE,
            &[TradeFill {
                price_cents: 195_000,
                lots: 1,
            }],
            1_778_803_200,
            "第三轮清仓",
            "",
        )
        .unwrap();

        // 与 seed 一致的收尾：写出第二轮复盘 + 第三轮标签/复盘
        list_trade_histories(&workspace, TEST_LEDGER_ID).unwrap();
        let detail = get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_CODE).unwrap();
        let round2 = detail
            .rounds
            .iter()
            .find(|round| round.round_no == 2)
            .expect("第二轮")
            .id
            .clone();
        update_round_review(
            &workspace,
            TEST_LEDGER_ID,
            &round2,
            "阶段三：第二轮复盘（编辑后必须保留）",
        )
        .unwrap();
        let round3 = detail
            .rounds
            .iter()
            .find(|round| round.round_no == 3)
            .expect("第三轮")
            .id
            .clone();
        update_round_tag(&workspace, TEST_LEDGER_ID, &round3, "蓄力").unwrap();
        update_round_review(&workspace, TEST_LEDGER_ID, &round3, "阶段三：第三轮复盘").unwrap();

        // 阶段 3-5：**另一只股票** 000001 建仓 1 手 → 清仓 1 手
        // （这一步是必需的：它让 600519 的第三轮在"编辑之前"就已经和另一只股票的
        //   重放共存过，正是出问题时的顺序）
        create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            "000001",
            "平安银行",
            consts::STOCK_TRADE_OPEN,
            &[TradeFill {
                price_cents: 112_000,
                lots: 1,
            }],
            1_778_976_000,
            "第四轮建仓",
            "",
        )
        .unwrap();
        let fourth = create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            "000001",
            "平安银行",
            consts::STOCK_TRADE_CLOSE,
            &[TradeFill {
                price_cents: 118_000,
                lots: 1,
            }],
            1_779_062_400,
            "第四轮清仓",
            "尾盘",
        )
        .unwrap();
        let _ = fourth;

        // 阶段 3-7：先编辑 000001 的建仓价（触发一次包含 12 笔成交的重放）
        let fourth_open_id =
            StockDao::list_trades_asc(&workspace.connection(), TEST_LEDGER_ID, "000001")
                .unwrap()
                .iter()
                .find(|trade| trade.trade_time == 1_778_976_000)
                .expect("000001 建仓")
                .id
                .clone();
        update_trade_fill(
            &workspace,
            TEST_LEDGER_ID,
            &fourth_open_id,
            113_000,
            1,
            1_778_976_000,
        )
        .unwrap();

        let before =
            StockDao::list_trades_asc(&workspace.connection(), TEST_LEDGER_ID, TEST_CODE).unwrap();
        assert_eq!(before.len(), 11, "编辑前 600519 应有 11 笔成交");

        // 阶段 3-4：编辑第三轮建仓 1 手 → 3 手（触发重放）
        update_trade_fill(
            &workspace,
            TEST_LEDGER_ID,
            &r3[0].id,
            190_000,
            3,
            1_778_716_800,
        )
        .unwrap();

        let after =
            StockDao::list_trades_asc(&workspace.connection(), TEST_LEDGER_ID, TEST_CODE).unwrap();
        assert_eq!(after.len(), 11, "编辑不应改变 600519 的成交笔数");
        assert!(
            after.iter().any(|trade| trade.trade_time == 1_778_803_200),
            "第三轮的清仓成交必须仍然存在；实际 {:?}",
            after
                .iter()
                .map(|trade| (trade.trade_type.clone(), trade.trade_time))
                .collect::<Vec<_>>()
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 回归：`preview_trade_change` 是"预演"——**不能**改动任何状态。
    ///
    /// 界面上"删除委托"要先弹影响预演（`delete_order`），确认后才真删。
    /// 曾报告过：预演删除之后再触发任何重放都会报"卖出数量超过持仓"。
    /// 实际排查结论：**那条报错来自预演本身**——删掉建仓委托会让重放看到"只有卖出没有买入"，
    /// 预演与真删都会以同一个 400 拒绝（这是正确行为，不是状态被污染）。
    /// 所以这里用**合法**的预演目标（删清仓委托，删完还剩持仓）来验证"预演零副作用"。
    #[test]
    fn preview_delete_order_keeps_state_intact_for_later_replay() {
        let (workspace, dir) = workspace("preview-delete");
        seed_principal(&workspace, 100_000_000);

        // 建仓 2 手（一笔委托两笔成交）+ 清仓 2 手 → 形成一轮
        let opens = create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            &[
                TradeFill {
                    price_cents: 3806,
                    lots: 1,
                },
                TradeFill {
                    price_cents: 3806,
                    lots: 1,
                },
            ],
            1_700_000_000,
            "建仓",
            "",
        )
        .unwrap();
        let closes = create_trade_order(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_CLOSE,
            &[TradeFill {
                price_cents: 3900,
                lots: 2,
            }],
            1_700_100_000,
            "清仓",
            "",
        )
        .unwrap();

        let before =
            StockDao::list_all_trades_asc(&workspace.connection(), TEST_LEDGER_ID).unwrap();
        assert_eq!(before.len(), 3, "建仓 2 笔 + 清仓 1 笔");

        // ① 非法预演：删建仓委托 → 只剩卖出，预演自己就该以 400 拒绝（真删也一样会拒绝）
        let invalid = preview_trade_change(
            &workspace,
            TEST_LEDGER_ID,
            "delete_order",
            "",
            &opens[0].order_id,
            0,
            0,
            0,
        );
        assert!(
            invalid.is_err(),
            "删掉建仓委托会让成交流不成立，预览应当报错: {invalid:?}"
        );

        // 失败的预演同样不能留下任何痕迹
        let after_invalid =
            StockDao::list_all_trades_asc(&workspace.connection(), TEST_LEDGER_ID).unwrap();
        assert_eq!(after_invalid.len(), 3, "失败的预演不能改成交表");

        // ② 合法预演：删清仓委托 → 删完仍是"持有 2 手"，应当成功返回影响
        let preview = preview_trade_change(
            &workspace,
            TEST_LEDGER_ID,
            "delete_order",
            "",
            &closes[0].order_id,
            0,
            0,
            0,
        );
        assert!(preview.is_ok(), "删清仓委托的预演应当成功: {preview:?}");
        let impact = preview.unwrap();
        assert_eq!(impact.position_after, 200, "删掉清仓后应持有 2 手");

        // 预演必须零副作用
        let after_preview =
            StockDao::list_all_trades_asc(&workspace.connection(), TEST_LEDGER_ID).unwrap();
        assert_eq!(
            after_preview.len(),
            before.len(),
            "预演删除后成交行数不能变（预演绝不落库）"
        );
        let position = StockDao::get_position(&workspace.connection(), TEST_LEDGER_ID, TEST_CODE)
            .expect("持仓行应存在");
        assert_eq!(position.quantity, 0, "预演不能把持仓改成 200");

        // ③ 之后再触发一次重放（编辑任一成交的价格）：必须成功，不能报超卖
        let target = after_preview
            .iter()
            .find(|trade| trade.trade_type == consts::STOCK_TRADE_OPEN)
            .expect("应当还有建仓成交");
        let updated = update_trade_fill(
            &workspace,
            TEST_LEDGER_ID,
            &target.id,
            target.price + 10,
            target.lots,
            target.trade_time,
        );
        assert!(updated.is_ok(), "预演之后的重放应当成功，实际: {updated:?}");

        let final_trades =
            StockDao::list_all_trades_asc(&workspace.connection(), TEST_LEDGER_ID).unwrap();
        assert_eq!(final_trades.len(), 3, "重放后成交行数仍应是 3");
        assert_eq!(
            final_trades
                .iter()
                .filter(|trade| trade.trade_type == consts::STOCK_TRADE_OPEN)
                .count(),
            2,
            "建仓的两笔成交必须都还在"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    fn create_xiehe_order(workspace: &Workspace) -> Vec<StockTradeDto> {
        seed_principal(workspace, 10_000_000);
        create_trade_order(
            workspace,
            TEST_LEDGER_ID,
            TEST_ORDER_CODE,
            TEST_ORDER_NAME,
            consts::STOCK_TRADE_OPEN,
            &[TradeFill {
                price_cents: 3806,
                lots: 2,
            }],
            1_700_000_000,
            "",
            "",
        )
        .unwrap();
        create_trade_order(
            workspace,
            TEST_LEDGER_ID,
            TEST_ORDER_CODE,
            TEST_ORDER_NAME,
            consts::STOCK_TRADE_CLOSE,
            &[
                TradeFill {
                    price_cents: 3667,
                    lots: 1,
                },
                TradeFill {
                    price_cents: 3661,
                    lots: 1,
                },
            ],
            1_700_000_100,
            "",
            "",
        )
        .unwrap()
    }

    // ---------- 账户 / 支取 ----------

    #[test]
    fn withdraw_follows_total_assets_formula() {
        let (workspace, dir) = workspace("withdraw-formula");

        seed_principal(&workspace, 10_000_000);
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            1000,
            10,
            1_700_003_000,
            "",
            "",
        )
        .unwrap();
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_CLOSE,
            1200,
            10,
            1_700_003_100,
            "",
            "",
        )
        .unwrap();

        let before = get_overview(&workspace, TEST_LEDGER_ID).unwrap();
        // 买入成本 1000510，卖出净额 1198888 → 已实现盈亏 198378
        assert_eq!(before.realized_pnl, 198_378);
        assert_eq!(
            before.available_cash,
            before.principal + before.realized_pnl,
            "无持仓支取时可用现金应等于本金+总盈亏"
        );
        assert_eq!(
            before.total_assets, before.available_cash,
            "无支取时总资产应等于可用现金"
        );

        let after = add_withdraw(&workspace, TEST_LEDGER_ID, 200_000).unwrap();
        assert_eq!(after.principal, before.principal, "支取不应改变本金");
        assert_eq!(after.withdrawn_total, 200_000);
        assert_eq!(after.available_cash, before.available_cash - 200_000);
        assert_eq!(
            after.total_assets,
            after.principal + after.realized_pnl - after.withdrawn_total
        );

        // 资金记录链完整：设置本金不计记录，建仓 + 清仓 + 支取 = 3 条
        let page = list_fund_records(&workspace, TEST_LEDGER_ID, 1, 10).unwrap();
        assert_eq!(page.total, 3);
        assert_eq!(page.items[0].event_type, consts::STOCK_EVENT_WITHDRAW);
        assert_eq!(page.items[0].amount_change, -200_000);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn withdraw_rejected_when_exceeds_cash() {
        let (workspace, dir) = workspace("withdraw-over");

        seed_principal(&workspace, 5_000_000);
        add_withdraw(&workspace, TEST_LEDGER_ID, 100_000).unwrap();

        // 支取超过当前现金（4900000 分）应被拒绝，且不产生新记录
        let error = add_withdraw(&workspace, TEST_LEDGER_ID, 4_900_001).unwrap_err();
        assert!(
            error.to_string().contains("不能超过可用现金"),
            "错误信息应说明现金限制, 实际: {error}"
        );
        assert!(
            add_withdraw(&workspace, TEST_LEDGER_ID, 0).is_err(),
            "支取 0 应报错"
        );
        assert!(
            add_withdraw(&workspace, TEST_LEDGER_ID, -100).is_err(),
            "负金额支取应报错"
        );

        let overview = get_overview(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(overview.withdrawn_total, 100_000);
        assert_eq!(overview.available_cash, 4_900_000);
        let page = list_fund_records(&workspace, TEST_LEDGER_ID, 1, 10).unwrap();
        assert_eq!(page.total, 1, "失败支取不应写入记录");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn withdraw_accumulates_without_changing_principal() {
        let (workspace, dir) = workspace("withdraw-accumulate");

        seed_principal(&workspace, 5_000_000);
        add_withdraw(&workspace, TEST_LEDGER_ID, 100_000).unwrap();
        add_withdraw(&workspace, TEST_LEDGER_ID, 200_000).unwrap();
        // 追加本金后再支取，本金始终是累计投入
        add_principal(&workspace, TEST_LEDGER_ID, 1_000_000).unwrap();
        add_withdraw(&workspace, TEST_LEDGER_ID, 500_000).unwrap();

        let overview = get_overview(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(overview.principal, 6_000_000, "本金含追加且不受支取影响");
        assert_eq!(overview.withdrawn_total, 800_000);
        assert_eq!(overview.available_cash, 6_000_000 - 800_000);
        assert_eq!(
            overview.total_assets,
            overview.principal - overview.withdrawn_total
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn interest_principal_lifts_cash_without_changing_principal() {
        let (workspace, dir) = workspace("interest-principal");

        seed_principal(&workspace, 5_000_000);
        // 利息归本：现金 +100000，本金口径不动
        let after = add_interest(&workspace, TEST_LEDGER_ID, 100_000).unwrap();
        assert_eq!(after.principal, 5_000_000, "利息归本不应改变本金");
        assert_eq!(after.interest_total, 100_000);
        assert_eq!(after.withdrawn_total, 0);
        assert_eq!(after.available_cash, 5_100_000, "可用现金要加上利息归本");
        assert_eq!(after.total_assets, 5_100_000);

        // 资金记录：利息事件金额为正、带现金余额，且不是卖出（net_pnl 为空）
        let page = list_fund_records(&workspace, TEST_LEDGER_ID, 1, 10).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(
            page.items[0].event_type,
            consts::STOCK_EVENT_INTEREST_PRINCIPAL
        );
        assert_eq!(page.items[0].event_text, "利息归本");
        assert_eq!(page.items[0].amount_change, 100_000);
        assert_eq!(page.items[0].cash_balance, 5_100_000);
        assert_eq!(page.items[0].net_pnl, None);
        assert!(
            page.items[0].remark.contains("利息 1000"),
            "备注应写明利息金额, 实际: {}",
            page.items[0].remark
        );

        // 利息是账户里真实存在的现金：可以再支取出来
        let withdrawn = add_withdraw(&workspace, TEST_LEDGER_ID, 100_000).unwrap();
        assert_eq!(withdrawn.interest_total, 100_000, "支取不影响累计利息");
        assert_eq!(withdrawn.available_cash, 5_000_000);

        // 非法金额被拦下且不产生记录
        assert!(add_interest(&workspace, TEST_LEDGER_ID, 0).is_err());
        assert!(add_interest(&workspace, TEST_LEDGER_ID, -100).is_err());
        assert!(add_interest_at_date(&workspace, TEST_LEDGER_ID, 100, "not-a-date").is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn interest_principal_accumulates_and_chains_with_other_events() {
        let (workspace, dir) = workspace("interest-principal-chain");

        seed_principal(&workspace, 1_000_000);
        add_interest_at_date(&workspace, TEST_LEDGER_ID, 30_000, "2026-02-01").unwrap();
        add_principal_at_date(&workspace, TEST_LEDGER_ID, 500_000, "2026-02-02").unwrap();
        add_interest_at_date(&workspace, TEST_LEDGER_ID, 20_000, "2026-02-03").unwrap();

        let overview = get_overview(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(overview.principal, 1_500_000, "只有追加本金抬高本金");
        assert_eq!(overview.interest_total, 50_000, "利息归本累计");
        assert_eq!(overview.available_cash, 1_550_000);
        assert_eq!(
            overview.available_cash,
            overview.principal + overview.interest_total + overview.realized_pnl
                - overview.withdrawn_total
        );

        // 资金记录按日期由近及远，现金余额链逐条自洽
        let page = list_fund_records(&workspace, TEST_LEDGER_ID, 1, 10).unwrap();
        assert_eq!(page.total, 3);
        assert_eq!(page.items[0].record_date, "2026-02-03");
        assert_eq!(page.items[0].cash_balance, 1_550_000);
        assert_eq!(page.items[1].cash_balance, 1_530_000);
        assert_eq!(page.items[2].cash_balance, 1_030_000);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn available_cash_subtracts_position_cost() {
        let (workspace, dir) = workspace("available-cash");

        seed_principal(&workspace, 10_000_000);
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            1000,
            10,
            1_700_004_000,
            "",
            "",
        )
        .unwrap();

        let overview =
            get_overview_with(&workspace, TEST_LEDGER_ID, &StubQuoteFetcher::empty()).unwrap();
        // 行情缺失 → 市值按持仓成本计入，总资产 = 可用现金 + 持仓市值 = 本金
        assert_eq!(overview.position_market_value, 1_000_510);
        assert_eq!(
            overview.total_assets,
            overview.available_cash + overview.position_market_value,
            "有持仓时总资产应为可用现金 + 持仓市值"
        );
        assert_eq!(overview.available_cash, 10_000_000 - 1_000_510);

        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_CLOSE,
            1100,
            10,
            1_700_004_100,
            "",
            "",
        )
        .unwrap();
        let after = get_overview(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(
            after.available_cash, after.total_assets,
            "清仓后可用现金应等于总资产"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn add_withdraw_supports_record_date() {
        let (workspace, dir) = workspace("withdraw-date");

        seed_principal(&workspace, 5_000_000);
        add_principal_at_date(&workspace, TEST_LEDGER_ID, 1_000_000, "2026-01-05").unwrap();
        add_withdraw_at_date(&workspace, TEST_LEDGER_ID, 200_000, "2026-01-10").unwrap();

        let page = list_fund_records(&workspace, TEST_LEDGER_ID, 1, 10).unwrap();
        assert_eq!(page.total, 2);
        // 列表按日期由近及远：支取 2026-01-10 在前，追加本金 2026-01-05 在后
        assert_eq!(page.items[0].record_date, "2026-01-10");
        assert_eq!(page.items[0].event_type, consts::STOCK_EVENT_WITHDRAW);
        assert_eq!(page.items[1].record_date, "2026-01-05");
        assert_eq!(page.items[1].event_type, consts::STOCK_EVENT_ADD_PRINCIPAL);

        let overview = get_overview(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(overview.principal, 6_000_000);
        assert_eq!(overview.withdrawn_total, 200_000);

        let error =
            add_principal_at_date(&workspace, TEST_LEDGER_ID, 100_000, "2026/01/05").unwrap_err();
        assert!(
            error.to_string().contains("日期格式"),
            "错误信息应说明日期格式, 实际: {error}"
        );
        assert!(add_withdraw_at_date(&workspace, TEST_LEDGER_ID, 100_000, "not-a-date").is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn set_principal_rejects_after_fund_records_and_requires_positive() {
        let (workspace, dir) = workspace("principal-guard");

        assert!(
            set_principal_amount(&workspace, 0).is_err(),
            "本金为 0 应报错"
        );
        assert!(
            set_principal_amount(&workspace, -1).is_err(),
            "负本金应报错"
        );

        seed_principal(&workspace, 1_000_000);
        add_withdraw(&workspace, TEST_LEDGER_ID, 10_000).unwrap();
        let error = set_principal(&workspace, TEST_LEDGER_ID, 2_000_000).unwrap_err();
        assert_eq!(error.to_string(), "已有资金变化记录，请使用「追加本金」");
        assert_eq!(error.into_app_error().status, 409);

        std::fs::remove_dir_all(&dir).ok();
    }

    // ---------- 标签设置 ----------

    #[test]
    fn trade_tag_setting_defaults() {
        let (workspace, dir) = workspace("tags-default");

        let setting = get_trade_tags_dto(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(setting.default_tag, consts::STOCK_TAG_ANALYSIS);
        assert_eq!(setting.tags, consts::default_stock_trade_tags());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_trade_tags_validation() {
        let (workspace, dir) = workspace("tags-save");

        let saved = save_trade_tags(
            &workspace,
            TEST_LEDGER_ID,
            &[
                consts::STOCK_TAG_ANALYSIS.to_string(),
                consts::STOCK_TAG_DABAN.to_string(),
                consts::STOCK_TAG_XULI.to_string(),
                "低吸".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(
            saved.tags,
            vec!["分析", "打板", "蓄力", "低吸"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        );
        assert_eq!(get_trade_tags(&workspace, TEST_LEDGER_ID).unwrap().len(), 4);

        // 去首尾空白
        let saved = save_trade_tags(
            &workspace,
            TEST_LEDGER_ID,
            &[consts::STOCK_TAG_ANALYSIS.to_string(), " 蓄力 ".to_string()],
        )
        .unwrap();
        assert_eq!(saved.tags[1], consts::STOCK_TAG_XULI);

        let invalid: Vec<(&str, Vec<String>, &str)> = vec![
            ("空列表", vec![], "至少保留一个标签"),
            (
                "删除默认标签",
                vec![consts::STOCK_TAG_DABAN.to_string()],
                "「分析」不可删除",
            ),
            (
                "重复标签",
                vec![
                    consts::STOCK_TAG_ANALYSIS.to_string(),
                    consts::STOCK_TAG_ANALYSIS.to_string(),
                ],
                "标签不能重复",
            ),
            (
                "超长标签",
                vec![
                    consts::STOCK_TAG_ANALYSIS.to_string(),
                    "涨停板接力战法九字".to_string(),
                ],
                "不能超过 8 个字",
            ),
        ];
        for (name, tags, want) in invalid {
            let error = save_trade_tags(&workspace, TEST_LEDGER_ID, &tags).unwrap_err();
            assert!(
                error.to_string().contains(want),
                "{name} 错误文案错误: {error}"
            );
        }

        let mut too_many = vec![consts::STOCK_TAG_ANALYSIS.to_string()];
        for index in 0..20 {
            too_many.push(format!("自定义{}", (b'A' + index) as char));
        }
        assert!(
            save_trade_tags(&workspace, TEST_LEDGER_ID, &too_many).is_err(),
            "超过 20 个标签应被拒绝"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn custom_tag_applied_to_round_and_statistics() {
        let (workspace, dir) = workspace("tags-custom");

        seed_principal(&workspace, 10_000_000);
        seed_clean_round_from(
            &workspace,
            TEST_CODE,
            TEST_NAME,
            1000,
            1100,
            10,
            1_700_000_000,
        );
        seed_clean_round_from(
            &workspace,
            TEST_CODE_B,
            TEST_NAME_B,
            800,
            850,
            10,
            1_700_001_000,
        );

        // 首次统计触发历史回填生成轮次
        crate::stock_statistics::get_statistics(&workspace, TEST_LEDGER_ID).unwrap();
        let detail = get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_CODE).unwrap();
        let round_id = detail.rounds[0].id.clone();

        // 新增自定义标签后即可用于轮次
        let mut custom = consts::default_stock_trade_tags();
        custom.push("低吸".to_string());
        save_trade_tags(&workspace, TEST_LEDGER_ID, &custom).unwrap();
        update_round_tag(&workspace, TEST_LEDGER_ID, &round_id, "低吸").unwrap();
        assert!(
            update_round_tag(&workspace, TEST_LEDGER_ID, &round_id, "不存在的标签").is_err(),
            "列表外标签应被拒绝"
        );

        let stats = crate::stock_statistics::get_statistics_range(
            &workspace,
            TEST_LEDGER_ID,
            "",
            "",
            0,
            "低吸",
        )
        .unwrap();
        assert_eq!(stats.round_count, 1);
        assert_eq!(stats.points[0].stock_code, TEST_CODE);
        assert_eq!(stats.points[0].tag, "低吸");

        // 删除自定义标签后，历史轮次回退可用标签，已删除标签不可再筛选
        save_trade_tags(
            &workspace,
            TEST_LEDGER_ID,
            &consts::default_stock_trade_tags(),
        )
        .unwrap();
        update_round_tag(
            &workspace,
            TEST_LEDGER_ID,
            &round_id,
            consts::STOCK_TAG_DABAN,
        )
        .unwrap();
        assert!(
            crate::stock_statistics::get_statistics_range(
                &workspace,
                TEST_LEDGER_ID,
                "",
                "",
                0,
                "低吸"
            )
            .is_err(),
            "已删除标签不应再可用于筛选"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reset_data_restores_default_trade_tags() {
        let (workspace, dir) = workspace("tags-reset");

        save_trade_tags(
            &workspace,
            TEST_LEDGER_ID,
            &[consts::STOCK_TAG_ANALYSIS.to_string(), "短线".to_string()],
        )
        .unwrap();
        reset_data(&workspace, TEST_LEDGER_ID).unwrap();
        let setting = get_trade_tags(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(setting, consts::default_stock_trade_tags());

        std::fs::remove_dir_all(&dir).ok();
    }

    // ---------- 行情挂载 ----------

    #[test]
    fn list_positions_attaches_quotes() {
        let (workspace, dir) = workspace("quotes-positions");
        let fetcher = StubQuoteFetcher::with(
            [(TEST_CODE.to_string(), quote(TEST_CODE, 1100, 1050, 12345))]
                .into_iter()
                .collect(),
        );

        seed_principal(&workspace, 10_000_000);
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            1000,
            10,
            1_700_004_000,
            "",
            "",
        )
        .unwrap();

        let items = list_positions_with(&workspace, TEST_LEDGER_ID, &fetcher).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].latest_price, Some(1100));
        assert_eq!(items[0].prev_close, Some(1050));
        assert_eq!(items[0].quote_time, Some(12345));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn get_overview_includes_market_value_and_unrealized() {
        let (workspace, dir) = workspace("quotes-overview");
        let fetcher = StubQuoteFetcher::with(
            [(TEST_CODE.to_string(), quote(TEST_CODE, 1100, 1050, 12345))]
                .into_iter()
                .collect(),
        );

        seed_principal(&workspace, 10_000_000);
        // 建仓 10 手 @ 10.00：成本 = 1000000 + 佣金 500 + 过户费 10 = 1000510
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            1000,
            10,
            1_700_004_000,
            "",
            "",
        )
        .unwrap();

        let overview = get_overview_with(&workspace, TEST_LEDGER_ID, &fetcher).unwrap();
        assert_eq!(overview.available_cash, 10_000_000 - 1_000_510);
        assert_eq!(overview.position_market_value, 1100 * 1000);
        assert_eq!(overview.unrealized_pnl, 1100 * 1000 - 1_000_510);
        assert_eq!(
            overview.total_assets,
            overview.available_cash + overview.position_market_value
        );
        assert_eq!(overview.quote_failed_count, 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn get_overview_falls_back_to_cost_when_quote_missing() {
        let (workspace, dir) = workspace("quotes-missing");
        let fetcher = StubQuoteFetcher::empty();

        seed_principal(&workspace, 10_000_000);
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            1000,
            10,
            1_700_004_000,
            "",
            "",
        )
        .unwrap();

        let overview = get_overview_with(&workspace, TEST_LEDGER_ID, &fetcher).unwrap();
        assert_eq!(overview.quote_failed_count, 1);
        assert_eq!(
            overview.position_market_value, 1_000_510,
            "行情缺失按成本计入市值"
        );
        assert_eq!(overview.unrealized_pnl, 0);
        assert_eq!(overview.total_assets, 10_000_000);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn get_overview_partial_quote_failure() {
        let (workspace, dir) = workspace("quotes-partial");
        let fetcher = StubQuoteFetcher::with(
            [(TEST_CODE.to_string(), quote(TEST_CODE, 1100, 1050, 12345))]
                .into_iter()
                .collect(),
        );

        seed_principal(&workspace, 10_000_000);
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            1000,
            10,
            1_700_004_000,
            "",
            "",
        )
        .unwrap();
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE_B,
            TEST_NAME_B,
            consts::STOCK_TRADE_OPEN,
            2000,
            5,
            1_700_004_000,
            "",
            "",
        )
        .unwrap();

        let positions = list_positions_with(&workspace, TEST_LEDGER_ID, &fetcher).unwrap();
        let mut cost600 = 0;
        let mut cost000 = 0;
        let mut qty600 = 0;
        for position in &positions {
            if position.stock_code == TEST_CODE {
                cost600 = position.total_cost;
                qty600 = position.quantity;
            } else {
                cost000 = position.total_cost;
            }
        }

        let overview = get_overview_with(&workspace, TEST_LEDGER_ID, &fetcher).unwrap();
        assert_eq!(overview.quote_failed_count, 1);
        assert_eq!(overview.position_market_value, 1100 * qty600 + cost000);
        assert_eq!(overview.unrealized_pnl, 1100 * qty600 - cost600);
        assert_eq!(
            overview.total_assets,
            overview.available_cash + overview.position_market_value
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_trade_histories_attaches_quotes() {
        let (workspace, dir) = workspace("quotes-history");
        let fetcher = StubQuoteFetcher::with(
            [(
                TEST_CODE.to_string(),
                quote(TEST_CODE, 1234, 1200, 1_700_000_000),
            )]
            .into_iter()
            .collect(),
        );

        close_round_helper(&workspace, TEST_CODE, TEST_NAME, 1000, 1100); // 600000 有行情
        close_round_helper(&workspace, TEST_CODE_B, TEST_NAME_B, 2000, 1900); // 000001 行情缺失

        let items = list_trade_histories_with(&workspace, TEST_LEDGER_ID, &fetcher).unwrap();
        assert_eq!(items.len(), 2);
        let by_code: HashMap<&str, &StockTradeHistoryDto> = items
            .iter()
            .map(|item| (item.stock_code.as_str(), item))
            .collect();
        assert_eq!(by_code[TEST_CODE].latest_price, Some(1234));
        assert_eq!(
            by_code[TEST_CODE_B].latest_price, None,
            "行情缺失时最新价应为空（前端显示占位符）"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // ---------- 委托与费用 ----------

    #[test]
    fn create_trade_order_charges_fee_once_per_order() {
        let (workspace, dir) = workspace("order-fee-once");
        let items = create_xiehe_order(&workspace);

        assert_eq!(items.len(), 2, "应返回 2 笔成交明细");
        assert!(!items[0].order_id.is_empty());
        assert_eq!(
            items[0].order_id, items[1].order_id,
            "同一委托应共用 orderId"
        );
        assert_eq!((items[0].order_seq, items[1].order_seq), (1, 2));
        assert_eq!((items[0].price, items[1].price), (3667, 3661));

        let mut commission = 0;
        let mut stamp_duty = 0;
        let mut transfer_fee = 0;
        let mut fee = 0;
        let mut realized = 0;
        for item in &items {
            fee += item.fee;
            commission += item.commission;
            stamp_duty += item.stamp_duty;
            transfer_fee += item.transfer_fee;
            if let Some(pnl) = item.realized_pnl {
                realized += pnl;
            }
        }
        // 卖出委托：佣金按委托总额 7328 元收一次 = 5.00；
        // 印花税与过户费**逐笔成交**各收一次（0.04 + 0.04 = 0.08），不是按总额取整（那会得 0.07）
        assert_eq!((commission, stamp_duty, transfer_fee), (500, 366, 8));
        assert_eq!(fee, 874, "卖出委托费用应为 874 分（5.00 + 3.66 + 0.08）");
        assert_eq!(realized, -29_782);

        let detail = get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_ORDER_CODE).unwrap();
        assert_eq!(detail.total_pnl, -29_782);

        // 一笔委托只产生一条资金记录
        let page = list_fund_records(&workspace, TEST_LEDGER_ID, 1, 20).unwrap();
        let sell_count = page
            .items
            .iter()
            .filter(|item| item.event_type == consts::STOCK_EVENT_SELL)
            .count();
        assert_eq!(sell_count, 1, "一笔卖出委托应只产生 1 条资金记录");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn create_trade_single_fill_unchanged() {
        let (workspace, dir) = workspace("order-single-fill");

        seed_principal(&workspace, 10_000_000);
        let trade = create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            1000,
            10,
            1_700_000_000,
            "",
            "",
        )
        .unwrap();
        assert!(!trade.order_id.is_empty(), "单笔委托也应有 orderId");
        assert_eq!(trade.order_seq, 1);
        assert_eq!(
            (trade.fee, trade.commission, trade.transfer_fee),
            (510, 500, 10)
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn update_trade_fill_recalculates_order() {
        let (workspace, dir) = workspace("order-update-fill");
        let items = create_xiehe_order(&workspace);

        // 把第二笔成交价从 36.61 改为 36.91（卖出金额增加 30 元）
        let updated = update_trade_fill(
            &workspace,
            TEST_LEDGER_ID,
            &items[1].id,
            3691,
            1,
            1_700_000_100,
        )
        .unwrap();
        assert_eq!(updated.price, 3691);

        let trades = list_trades(&workspace, TEST_LEDGER_ID, TEST_ORDER_CODE).unwrap();
        let mut amount = 0;
        let mut realized = 0;
        for trade in &trades {
            if trade.trade_type == consts::STOCK_TRADE_CLOSE {
                amount += trade.amount;
                if let Some(pnl) = trade.realized_pnl {
                    realized += pnl;
                }
            }
        }
        assert_eq!(amount, 732_800 + 3_000);
        // 盈亏 = 原 -297.82（改价前，含逐笔取整的过户费 0.08）+ 多卖 30 元 − 印花税多收 0.02
        //（36.91×100 = 3691 元的印花税 1.85 vs 原 1.83；过户费两笔仍各 0.04，不变）
        assert_eq!(realized, -29_782 + 3_000 - 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_trade_order_restores_position() {
        let (workspace, dir) = workspace("order-delete");
        let items = create_xiehe_order(&workspace);

        delete_trade_order(&workspace, TEST_LEDGER_ID, &items[0].order_id).unwrap();

        let positions = list_positions(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(positions.len(), 1);
        assert_eq!(
            positions[0].quantity, 200,
            "删除卖出委托后应回到 200 股持仓"
        );

        let trades = list_trades(&workspace, TEST_LEDGER_ID, TEST_ORDER_CODE).unwrap();
        assert!(
            trades
                .iter()
                .all(|trade| trade.trade_type != consts::STOCK_TRADE_CLOSE),
            "卖出成交应已删除"
        );

        let page = list_fund_records(&workspace, TEST_LEDGER_ID, 1, 20).unwrap();
        assert!(
            page.items
                .iter()
                .all(|item| item.event_type != consts::STOCK_EVENT_SELL),
            "卖出资金记录应已移除"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn preview_trade_change_rolls_back() {
        let (workspace, dir) = workspace("order-preview");
        let items = create_xiehe_order(&workspace);

        let impact = preview_trade_change(
            &workspace,
            TEST_LEDGER_ID,
            "delete_order",
            "",
            &items[0].order_id,
            0,
            0,
            0,
        )
        .unwrap();
        assert_eq!(impact.position_after, 200, "预演应显示回滚到 200 股");
        assert_eq!(impact.removed_rounds.len(), 1);
        assert_eq!(impact.removed_rounds[0].round_no, 1);

        // 预演必须回滚：持仓与交易保持原样
        let positions = list_positions(&workspace, TEST_LEDGER_ID).unwrap();
        assert!(positions.is_empty(), "预演不应改变持仓, 实际 {positions:?}");
        let trades = list_trades(&workspace, TEST_LEDGER_ID, TEST_ORDER_CODE).unwrap();
        assert_eq!(trades.len(), 3, "预演不应删除成交");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn preview_trade_change_rejects_unknown_action() {
        let (workspace, dir) = workspace("order-preview-action");
        let error =
            preview_trade_change(&workspace, TEST_LEDGER_ID, "noop", "", "", 0, 0, 0).unwrap_err();
        assert_eq!(error.to_string(), "无效的预演动作");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rebuild_is_idempotent() {
        let (workspace, dir) = workspace("order-rebuild-idempotent");
        let items = create_xiehe_order(&workspace);

        update_trade_fill(
            &workspace,
            TEST_LEDGER_ID,
            &items[0].id,
            items[0].price,
            items[0].lots,
            items[0].trade_time,
        )
        .unwrap();
        let first = list_trades(&workspace, TEST_LEDGER_ID, TEST_ORDER_CODE).unwrap();
        let first_detail =
            get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_ORDER_CODE).unwrap();
        update_trade_fill(
            &workspace,
            TEST_LEDGER_ID,
            &items[0].id,
            items[0].price,
            items[0].lots,
            items[0].trade_time,
        )
        .unwrap();
        let second = list_trades(&workspace, TEST_LEDGER_ID, TEST_ORDER_CODE).unwrap();
        let second_detail =
            get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_ORDER_CODE).unwrap();

        assert_eq!(first.len(), second.len());
        for (left, right) in first.iter().zip(second.iter()) {
            assert_eq!(left.id, right.id, "重放后成交 ID 应稳定");
            assert_eq!(left.fee, right.fee, "重放后费用应一致");
            assert_eq!(left.round_id, right.round_id, "重放后轮次挂接应一致");
        }
        assert_eq!(
            first_detail.total_pnl, second_detail.total_pnl,
            "连续两次重放的盈亏应一致"
        );
        assert_eq!(first_detail.rounds.len(), second_detail.rounds.len());
        assert_eq!(
            first_detail.rounds[0].id, second_detail.rounds[0].id,
            "重放应复用同一轮次 ID"
        );
        assert_eq!(
            first_detail.total_pnl, -29_782,
            "重放后盈亏应保持 -29782 分（含逐笔取整的过户费 0.08）"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rebuild_keeps_round_tag_and_review() {
        let (workspace, dir) = workspace("order-rebuild-round");
        let items = create_xiehe_order(&workspace);

        let detail = get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_ORDER_CODE).unwrap();
        let round_id = detail.rounds[0].id.clone();
        update_round_tag(&workspace, TEST_LEDGER_ID, &round_id, "打板").unwrap();
        update_round_review(&workspace, TEST_LEDGER_ID, &round_id, "本次拆单卖出").unwrap();

        // 编辑成交（不改轮次结构）后，标签与复盘应保留
        update_trade_fill(
            &workspace,
            TEST_LEDGER_ID,
            &items[1].id,
            3661,
            1,
            1_700_000_100,
        )
        .unwrap();
        let after = get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_ORDER_CODE).unwrap();
        assert_eq!(after.rounds.len(), 1);
        assert_eq!(after.rounds[0].tag, "打板");
        assert_eq!(after.rounds[0].review, "本次拆单卖出");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rebuild_keeps_cash_chain_for_backdated_trade() {
        let (workspace, dir) = workspace("order-backdated");

        seed_principal(&workspace, 10_000_000);
        // 先按「今天」追加 5 万元，再补录一笔 2023 年的历史交易
        add_principal_at_date(&workspace, TEST_LEDGER_ID, 5_000_000, "2026-09-18").unwrap();
        let trade = create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            1000,
            10,
            1_700_000_000,
            "",
            "",
        )
        .unwrap();

        let balance_of = |event_type: &str| {
            let page = list_fund_records(&workspace, TEST_LEDGER_ID, 1, 20).unwrap();
            page.items
                .iter()
                .find(|item| item.event_type == event_type)
                .map(|item| item.cash_balance)
                .unwrap_or_else(|| panic!("未找到 {event_type} 资金记录"))
        };

        // 追加后现金 150000；买入 10 手 @10.00 = 1000000 + 佣金 500 + 过户费 10
        assert_eq!(balance_of(consts::STOCK_EVENT_ADD_PRINCIPAL), 15_000_000);
        assert_eq!(balance_of(consts::STOCK_EVENT_BUY), 15_000_000 - 1_000_510);

        // 编辑成交（改为 11.00）后重放，追加本金的余额不能被改写
        update_trade_fill(
            &workspace,
            TEST_LEDGER_ID,
            &trade.id,
            1100,
            10,
            1_700_000_000,
        )
        .unwrap();
        assert_eq!(
            balance_of(consts::STOCK_EVENT_ADD_PRINCIPAL),
            15_000_000,
            "重放后追加本金余额应保持 15000000 分"
        );
        // 1100000 + 佣金 500 + 过户费 11 = 1100511
        assert_eq!(balance_of(consts::STOCK_EVENT_BUY), 15_000_000 - 1_100_511);

        std::fs::remove_dir_all(&dir).ok();
    }

    // ---------- 历史与轮次 ----------

    #[test]
    fn close_archives_round_with_all_trades() {
        let (workspace, dir) = workspace("history-close");

        // 建仓 100 手 → 加仓 100 手 → 减仓 100 手 → 清仓 100 手
        for (trade_type, price, at) in [
            (consts::STOCK_TRADE_OPEN, 1000, 1_700_000_000),
            (consts::STOCK_TRADE_ADD, 1100, 1_700_000_100),
            (consts::STOCK_TRADE_REDUCE, 1200, 1_700_000_200),
            (consts::STOCK_TRADE_CLOSE, 1250, 1_700_000_300),
        ] {
            create_trade(
                &workspace,
                TEST_LEDGER_ID,
                TEST_CODE,
                TEST_NAME,
                trade_type,
                price,
                100,
                at,
                "",
                "",
            )
            .unwrap();
        }
        let close_time = 1_700_000_300;

        let histories = list_trade_histories(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(histories.len(), 1);
        assert_eq!(histories[0].stock_code, TEST_CODE);
        assert_eq!(histories[0].stock_name, TEST_NAME);
        assert_eq!(histories[0].round_count, 1);
        assert_eq!(histories[0].last_closed_at, close_time);

        let detail = get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_CODE).unwrap();
        assert_eq!(detail.rounds.len(), 1);
        let round = &detail.rounds[0];
        assert_eq!(round.round_no, 1);
        assert_eq!(round.opened_at, 1_700_000_000);
        assert_eq!(round.closed_at, close_time);
        assert_eq!(round.trades.len(), 4);

        let want_types = [
            consts::STOCK_TRADE_OPEN,
            consts::STOCK_TRADE_ADD,
            consts::STOCK_TRADE_REDUCE,
            consts::STOCK_TRADE_CLOSE,
        ];
        for (index, want) in want_types.iter().enumerate() {
            assert_eq!(round.trades[index].trade_type, *want);
            assert_eq!(
                round.trades[index].round_id, round.id,
                "第 {index} 笔交易未挂接到轮次"
            );
        }

        // 买入成本 = (10000000+2454) + (11000000+2699) = 21005153
        // 卖出净额 = (12000000-8945) + (12500000-9318) = 24481737
        let want_pnl = 24_481_737_i64 - 21_005_153;
        assert_eq!(round.pnl, want_pnl);
        assert_eq!(detail.total_pnl, want_pnl);
        assert!(round.pnl_rate > 16.0 && round.pnl_rate < 17.0);
        assert_eq!((detail.win_count, detail.loss_count), (1, 0));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn multiple_rounds_reuse_one_history() {
        let (workspace, dir) = workspace("history-multi-round");

        // 第一轮：盈利
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            1000,
            10,
            1_700_001_000,
            "",
            "",
        )
        .unwrap();
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_CLOSE,
            1100,
            10,
            1_700_001_100,
            "",
            "",
        )
        .unwrap();
        // 第二轮：亏损
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            2000,
            10,
            1_700_001_200,
            "",
            "",
        )
        .unwrap();
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_CLOSE,
            1900,
            10,
            1_700_001_300,
            "",
            "",
        )
        .unwrap();

        let histories = list_trade_histories(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(histories.len(), 1, "多次交易应复用同一个历史集合");
        assert_eq!(histories[0].round_count, 2);
        assert_eq!(histories[0].last_closed_at, 1_700_001_300);

        let detail = get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_CODE).unwrap();
        assert_eq!(detail.rounds.len(), 2);
        assert_eq!(
            (detail.rounds[0].round_no, detail.rounds[1].round_no),
            (1, 2)
        );
        assert!(detail.rounds[0].pnl > 0 && detail.rounds[1].pnl < 0);
        assert_eq!((detail.win_count, detail.loss_count), (1, 1));
        assert_eq!(
            detail.total_pnl,
            detail.rounds[0].pnl + detail.rounds[1].pnl
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn history_backfill_for_legacy_trades() {
        let (workspace, dir) = workspace("history-backfill");

        // 模拟功能上线前的存量交易：两轮完整轮次，round_id 均为空
        let conn = workspace.connection();
        for (trade_type, price, at, fee) in [
            (consts::STOCK_TRADE_OPEN, 1000, 1_690_000_000, 500),
            (consts::STOCK_TRADE_CLOSE, 1200, 1_690_000_100, 600),
            (consts::STOCK_TRADE_OPEN, 900, 1_690_000_200, 500),
            (consts::STOCK_TRADE_CLOSE, 800, 1_690_000_300, 600),
        ] {
            let trade = StockTrade {
                id: tr_store::util::new_uuid(),
                ledger_id: TEST_LEDGER_ID.to_string(),
                stock_code: TEST_CODE.to_string(),
                stock_name: TEST_NAME.to_string(),
                trade_type: trade_type.to_string(),
                price,
                lots: 10,
                shares: 1000,
                amount: price * 1000,
                fee,
                trade_time: at,
                ..StockTrade::default()
            };
            StockDao::create_trade(&conn, &trade).unwrap();
        }
        drop(conn);

        let histories = list_trade_histories(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(histories.len(), 1, "存量交易应回填出 1 个历史集合");
        assert_eq!(histories[0].round_count, 2);

        let detail = get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_CODE).unwrap();
        assert_eq!(detail.rounds.len(), 2);
        // 第一轮盈利（买10卖12），第二轮亏损（买9卖8）
        assert!(detail.rounds[0].pnl > 0 && detail.rounds[1].pnl < 0);
        for round in &detail.rounds {
            assert_eq!(round.trades.len(), 2);
            for trade in &round.trades {
                assert_eq!(trade.round_id, round.id, "回填后交易未挂接轮次");
            }
        }

        // 再次查询应幂等，不重复建轮次
        let again = list_trade_histories(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(again[0].round_count, 2, "回填应幂等");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn incomplete_round_not_archived() {
        let (workspace, dir) = workspace("history-incomplete");

        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            1000,
            10,
            1_690_000_000,
            "",
            "",
        )
        .unwrap();
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_ADD,
            1100,
            10,
            1_690_000_100,
            "",
            "",
        )
        .unwrap();

        let histories = list_trade_histories(&workspace, TEST_LEDGER_ID).unwrap();
        assert!(histories.is_empty(), "在建持仓不应进入交易历史");

        // 随后清仓：本轮从第一次建仓开始完整归档
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_CLOSE,
            1200,
            20,
            1_690_000_200,
            "",
            "",
        )
        .unwrap();
        let detail = get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_CODE).unwrap();
        assert_eq!(detail.rounds.len(), 1);
        assert_eq!(detail.rounds[0].trades.len(), 3);
        assert_eq!(detail.rounds[0].opened_at, 1_690_000_000);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_trades_for_held_stock_only_current_round() {
        let (workspace, dir) = workspace("history-current-round");

        // 第一轮：建仓 → 清仓（历史轮次）
        for (trade_type, price, at) in [
            (consts::STOCK_TRADE_OPEN, 1000, 1_690_001_000),
            (consts::STOCK_TRADE_CLOSE, 1200, 1_690_001_100),
            // 第二轮：再次建仓 + 加仓（当前持仓）
            (consts::STOCK_TRADE_OPEN, 1100, 1_690_001_200),
            (consts::STOCK_TRADE_ADD, 1150, 1_690_001_300),
        ] {
            create_trade(
                &workspace,
                TEST_LEDGER_ID,
                TEST_CODE,
                TEST_NAME,
                trade_type,
                price,
                10,
                at,
                "",
                "",
            )
            .unwrap();
        }

        // 持仓中：只返回本轮交易，历史轮次被排除
        let trades = list_trades(&workspace, TEST_LEDGER_ID, TEST_CODE).unwrap();
        assert_eq!(trades.len(), 2);
        assert_eq!(trades[0].trade_type, consts::STOCK_TRADE_ADD);
        assert_eq!(trades[1].trade_type, consts::STOCK_TRADE_OPEN);
        for trade in &trades {
            assert!(trade.round_id.is_empty(), "在建轮次交易不应挂接历史轮次");
        }

        // 清仓后再查：保留查看该股完整交易记录的行为
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_CLOSE,
            1300,
            20,
            1_690_001_400,
            "",
            "",
        )
        .unwrap();
        let all = list_trades(&workspace, TEST_LEDGER_ID, TEST_CODE).unwrap();
        assert_eq!(all.len(), 5);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn trade_history_summary_across_stocks() {
        let (workspace, dir) = workspace("history-summary");

        // 股票 A：第一轮盈利，第二轮亏损
        for (trade_type, price, at) in [
            (consts::STOCK_TRADE_OPEN, 1000, 1_690_002_000),
            (consts::STOCK_TRADE_CLOSE, 1200, 1_690_002_100),
            (consts::STOCK_TRADE_OPEN, 2000, 1_690_002_200),
            (consts::STOCK_TRADE_CLOSE, 1800, 1_690_002_300),
        ] {
            create_trade(
                &workspace,
                TEST_LEDGER_ID,
                TEST_CODE,
                TEST_NAME,
                trade_type,
                price,
                10,
                at,
                "",
                "",
            )
            .unwrap();
        }
        // 股票 B：一轮盈利
        for (trade_type, price, at) in [
            (consts::STOCK_TRADE_OPEN, 500, 1_690_002_400),
            (consts::STOCK_TRADE_CLOSE, 550, 1_690_002_500),
        ] {
            create_trade(
                &workspace,
                TEST_LEDGER_ID,
                TEST_CODE_B,
                TEST_NAME_B,
                trade_type,
                price,
                5,
                at,
                "",
                "",
            )
            .unwrap();
        }

        let summary = get_trade_history_summary(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(summary.stock_count, 2);
        assert_eq!(summary.round_count, 3);
        assert_eq!((summary.win_count, summary.loss_count), (2, 1));

        // 总盈亏 = 各股票详情轮次盈亏之和
        let mut want_pnl = 0;
        for code in [TEST_CODE, TEST_CODE_B] {
            let detail = get_trade_history_detail(&workspace, TEST_LEDGER_ID, code).unwrap();
            for round in &detail.rounds {
                want_pnl += round.pnl;
            }
        }
        assert_eq!(summary.total_pnl, want_pnl);
        assert!(summary.total_pnl_rate > 0.0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn update_round_review_validation() {
        let (workspace, dir) = workspace("history-review");

        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            1000,
            10,
            1_700_000_000,
            "",
            "",
        )
        .unwrap();
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_CLOSE,
            1200,
            10,
            1_700_000_100,
            "",
            "",
        )
        .unwrap();

        let detail = get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_CODE).unwrap();
        assert_eq!(detail.rounds.len(), 1);
        let round_id = detail.rounds[0].id.clone();
        assert_eq!(detail.rounds[0].review, "");

        // 保存复盘：去除首尾空白后随详情返回
        let detail = update_round_review(
            &workspace,
            TEST_LEDGER_ID,
            &round_id,
            "  建仓太急，未等回调；止损执行到位。  ",
        )
        .unwrap();
        assert_eq!(
            detail.rounds[0].review,
            "建仓太急，未等回调；止损执行到位。"
        );

        // 超过 500 字拒绝
        let long = "复".repeat(501);
        let error = update_round_review(&workspace, TEST_LEDGER_ID, &round_id, &long).unwrap_err();
        assert!(error.to_string().contains("不能超过 500 字"));
        // 恰好 500 字通过
        update_round_review(&workspace, TEST_LEDGER_ID, &round_id, &"复".repeat(500)).unwrap();

        // 其他账本不可操作该轮次
        assert!(update_round_review(&workspace, "other-ledger", &round_id, "越权写入").is_err());

        // 空串/纯空白 = 清空
        let detail = update_round_review(&workspace, TEST_LEDGER_ID, &round_id, "   ").unwrap();
        assert_eq!(detail.rounds[0].review, "");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn close_round_tag_default_and_update() {
        let (workspace, dir) = workspace("history-round-tag");

        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            1000,
            10,
            1_700_000_000,
            "",
            "",
        )
        .unwrap();
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_CLOSE,
            1200,
            10,
            1_700_000_100,
            "",
            "",
        )
        .unwrap();

        let detail = get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_CODE).unwrap();
        let round_id = detail.rounds[0].id.clone();
        assert_eq!(
            detail.rounds[0].tag,
            consts::STOCK_TAG_ANALYSIS,
            "未指定标签时应默认「分析」"
        );

        let detail = update_round_tag(
            &workspace,
            TEST_LEDGER_ID,
            &round_id,
            consts::STOCK_TAG_DABAN,
        )
        .unwrap();
        assert_eq!(detail.rounds[0].tag, consts::STOCK_TAG_DABAN);

        let error = update_round_tag(&workspace, TEST_LEDGER_ID, &round_id, "短线").unwrap_err();
        assert!(error.to_string().contains("无效的交易标签"));
        assert!(update_round_tag(
            &workspace,
            "other-ledger",
            &round_id,
            consts::STOCK_TAG_WEIPAN
        )
        .is_err());

        // 空串/纯空白 = 恢复默认「分析」
        let detail = update_round_tag(&workspace, TEST_LEDGER_ID, &round_id, "   ").unwrap();
        assert_eq!(detail.rounds[0].tag, consts::STOCK_TAG_ANALYSIS);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn close_round_saves_provided_tag() {
        let (workspace, dir) = workspace("history-close-tag");

        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            1000,
            10,
            1_700_002_000,
            "",
            "",
        )
        .unwrap();
        // 清仓时指定标签
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_CLOSE,
            1200,
            10,
            1_700_002_100,
            "",
            consts::STOCK_TAG_ZHUIZHANG,
        )
        .unwrap();
        let detail = get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_CODE).unwrap();
        assert_eq!(detail.rounds[0].tag, consts::STOCK_TAG_ZHUIZHANG);

        // 非清仓交易传标签不影响交易本身（标签只在清仓时进入轮次）
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            1000,
            10,
            1_700_002_200,
            "",
            consts::STOCK_TAG_DABAN,
        )
        .unwrap();
        assert_eq!(
            get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_CODE)
                .unwrap()
                .rounds
                .len(),
            1,
            "建仓不应产生新轮次"
        );

        // 非法标签被拒绝且不落库（委托完全不产生副作用）
        assert!(create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_CLOSE,
            1100,
            10,
            1_700_002_300,
            "",
            "打新",
        )
        .is_err());
        let detail = get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_CODE).unwrap();
        assert_eq!(detail.rounds.len(), 1, "非法标签的清仓不应产生轮次");
        assert_eq!(
            StockDao::list_all_trades_asc(&workspace.connection(), TEST_LEDGER_ID)
                .unwrap()
                .len(),
            3,
            "非法标签的委托不应落库"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn position_review_carries_into_closed_round() {
        let (workspace, dir) = workspace("history-position-review");

        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            1000,
            10,
            1_700_006_000,
            "",
            "",
        )
        .unwrap();
        let updated = update_position_review(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            "  建仓理由：回踩年线企稳，等放量  ",
        )
        .unwrap();
        assert_eq!(updated.review, "建仓理由：回踩年线企稳，等放量");

        let positions = list_positions(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].review, "建仓理由：回踩年线企稳，等放量");

        // 超过 500 字拒绝
        let error =
            update_position_review(&workspace, TEST_LEDGER_ID, TEST_CODE, &"复".repeat(501))
                .unwrap_err();
        assert!(error.to_string().contains("不能超过 500 字"));

        // 清仓：复盘归档到本轮次，持仓上的草稿清空（下一轮不继承）
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_CLOSE,
            1200,
            10,
            1_700_006_100,
            "",
            "",
        )
        .unwrap();
        let detail = get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_CODE).unwrap();
        assert_eq!(detail.rounds.len(), 1);
        assert_eq!(detail.rounds[0].review, "建仓理由：回踩年线企稳，等放量");

        let conn = workspace.connection();
        let position = StockDao::get_position(&conn, TEST_LEDGER_ID, TEST_CODE).unwrap();
        assert_eq!(position.review, "", "归档后持仓复盘应清空");
        drop(conn);

        // 已清仓股票不能再从持仓侧写复盘，改在交易历史里编辑
        assert!(update_position_review(&workspace, TEST_LEDGER_ID, TEST_CODE, "补写").is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    // ---------- 统计 ----------

    #[test]
    fn statistics_starts_from_first_settlement() {
        let (workspace, dir) = workspace("stats-first");

        seed_principal(&workspace, 10_000_000);
        // 第 1 笔（A 盈利 +100000）→ 第 2 笔（B 盈利 +50000）→ 第 3 笔（A 亏损 -80000）
        seed_clean_round_from(
            &workspace,
            TEST_CODE,
            TEST_NAME,
            1000,
            1100,
            10,
            1_690_000_000,
        );
        seed_clean_round_from(
            &workspace,
            TEST_CODE_B,
            TEST_NAME_B,
            800,
            850,
            10,
            1_690_001_000,
        );
        seed_clean_round_from(
            &workspace,
            TEST_CODE,
            TEST_NAME,
            2000,
            1920,
            10,
            1_690_002_000,
        );

        let stats = crate::stock_statistics::get_statistics(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(stats.principal, 10_000_000);
        assert_eq!(stats.round_count, 3);
        assert_eq!(stats.points.len(), 3);

        let p1 = &stats.points[0];
        assert_eq!((p1.sequence, p1.closed_at), (1, 1_690_000_000));
        assert_eq!(p1.stock_code, TEST_CODE);
        assert_eq!((p1.pnl, p1.total_pnl), (100_000, 100_000));
        assert_eq!((p1.win_count, p1.loss_count), (1, 0));
        assert_eq!(p1.win_rate, 100.0);
        assert_eq!((p1.avg_win, p1.avg_loss), (100_000, 0));
        assert_eq!(p1.pnl_ratio, None);
        assert_eq!(p1.expectancy, 100_000);
        assert_eq!((p1.max_drawdown, p1.max_drawdown_pct), (0, 0.0));

        let p2 = &stats.points[1];
        assert_eq!((p2.sequence, p2.closed_at), (2, 1_690_001_000));
        assert_eq!(p2.stock_code, TEST_CODE_B);
        assert_eq!((p2.pnl, p2.total_pnl), (50_000, 150_000));
        assert_eq!((p2.win_count, p2.loss_count), (2, 0));
        assert_eq!(p2.win_rate, 100.0);
        assert_eq!(p2.avg_win, 75_000);
        assert_eq!(p2.expectancy, 75_000);
        assert_eq!((p2.max_drawdown, p2.max_drawdown_pct), (0, 0.0));

        let p3 = &stats.points[2];
        assert_eq!((p3.sequence, p3.closed_at), (3, 1_690_002_000));
        assert_eq!(p3.stock_code, TEST_CODE);
        assert_eq!((p3.pnl, p3.total_pnl), (-80_000, 70_000));
        assert_eq!((p3.win_count, p3.loss_count), (2, 1));
        assert!((p3.win_rate - 66.67).abs() < 0.01, "实际 {}", p3.win_rate);
        assert_eq!((p3.avg_win, p3.avg_loss), (75_000, 80_000));
        let ratio = p3.pnl_ratio.expect("应有盈亏比");
        assert!((ratio - 0.9375).abs() < 0.0001, "实际 {ratio}");
        assert_eq!(p3.expectancy, 23_333);
        assert_eq!(p3.max_drawdown, 80_000);
        assert!((p3.max_drawdown_pct - 0.8).abs() < 0.001);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn statistics_counts_breakeven_in_total() {
        let (workspace, dir) = workspace("stats-breakeven");

        seed_principal(&workspace, 10_000_000);
        // 盈利 +100000 → 平局 0（计入总笔数） → 亏损 -60000
        seed_clean_round_from(
            &workspace,
            TEST_CODE,
            TEST_NAME,
            1000,
            1100,
            10,
            1_690_000_000,
        );
        seed_clean_round_from(
            &workspace,
            TEST_CODE_B,
            TEST_NAME_B,
            1000,
            1000,
            10,
            1_690_001_000,
        );
        seed_clean_round_from(
            &workspace,
            TEST_CODE,
            TEST_NAME,
            1000,
            940,
            10,
            1_690_002_000,
        );

        let stats = crate::stock_statistics::get_statistics(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!((stats.round_count, stats.points.len()), (3, 3));

        let p2 = &stats.points[1];
        assert_eq!(p2.sequence, 2);
        assert_eq!(p2.total_pnl, 100_000, "平局计入总笔数但不计入盈亏");
        assert_eq!((p2.win_count, p2.loss_count), (1, 0));
        assert!((p2.win_rate - 50.0).abs() < 0.01, "实际 {}", p2.win_rate);
        assert_eq!((p2.avg_win, p2.expectancy), (100_000, 50_000));

        let p3 = &stats.points[2];
        assert_eq!(p3.total_pnl, 40_000);
        assert_eq!((p3.win_count, p3.loss_count), (1, 1));
        assert!((p3.win_rate - 33.33).abs() < 0.01, "实际 {}", p3.win_rate);
        assert_eq!(p3.avg_loss, 60_000);
        let ratio = p3.pnl_ratio.expect("应有盈亏比");
        assert!((ratio - 1.6667).abs() < 0.001, "实际 {ratio}");
        assert_eq!(p3.expectancy, 13_333);
        assert_eq!(p3.max_drawdown, 60_000);
        assert!((p3.max_drawdown_pct - 0.6).abs() < 0.001);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn statistics_interest_principal_joins_the_equity_curve() {
        let (workspace, dir) = workspace("stats-interest");

        seed_principal(&workspace, 10_000_000);
        // 第 1 轮亏 80_000：本金 10_000_000 → 9_920_000，峰值仍是 10_000_000
        seed_clean_round_from(
            &workspace,
            TEST_CODE,
            TEST_NAME,
            1000,
            920,
            10,
            1_690_000_000,
        );
        // 亏损之后的**次日**记一笔利息归本：9_950_000 没有超过峰值
        add_interest_at_date(
            &workspace,
            TEST_LEDGER_ID,
            30_000,
            &unix_to_date(1_690_100_000),
        )
        .unwrap();
        // 第 2 轮再亏 100_000
        seed_clean_round_from(
            &workspace,
            TEST_CODE_B,
            TEST_NAME_B,
            1000,
            900,
            10,
            1_690_200_000,
        );

        let stats = crate::stock_statistics::get_statistics(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(stats.principal, 10_000_000, "利息归本不改本金口径");

        let p1 = &stats.points[0];
        assert_eq!((p1.pnl, p1.max_drawdown), (-80_000, 80_000));

        let p2 = &stats.points[1];
        assert_eq!(p2.pnl, -100_000);
        // 9_920_000 + 30_000 = 9_950_000 未超过峰值，再亏 100_000 → 回撤 150_000；
        // 利息若不进总资产曲线，这里会算成 180_000
        assert_eq!(p2.max_drawdown, 150_000, "利息归本要进总资产曲线");
        assert!(
            (p2.max_drawdown_pct - 1.5).abs() < 0.001,
            "回撤率分母仍是投入本金, 实际 {}",
            p2.max_drawdown_pct
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn statistics_needs_at_least_one_settlement() {
        let (workspace, dir) = workspace("stats-empty");

        let empty = crate::stock_statistics::get_statistics(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!((empty.round_count, empty.points.len()), (0, 0));

        seed_principal(&workspace, 10_000_000);
        seed_clean_round_from(
            &workspace,
            TEST_CODE,
            TEST_NAME,
            1000,
            1100,
            10,
            1_690_000_000,
        );
        let one = crate::stock_statistics::get_statistics(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!((one.round_count, one.points.len()), (1, 1));
        let p = &one.points[0];
        assert_eq!((p.sequence, p.total_pnl), (1, 100_000));
        assert_eq!(p.win_rate, 100.0);
        assert_eq!(p.avg_win, 100_000);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn statistics_drawdown_uses_principal_at_settlement() {
        let (workspace, dir) = workspace("stats-drawdown");

        seed_principal(&workspace, 10_000_000);
        // 第 1 笔盈利 +100000（2023-07-22）
        seed_clean_round_on(
            &workspace,
            TEST_CODE,
            TEST_NAME,
            1000,
            1100,
            10,
            "2023-07-22 12:00:00",
        );
        // 第 2 笔前追加本金 5,000,000（本金 10,000,000 → 15,000,000）
        {
            let conn = workspace.connection();
            StockDao::update_account_principal(&conn, TEST_LEDGER_ID, 15_000_000).unwrap();
            let record = StockFundRecord {
                id: tr_store::util::new_uuid(),
                ledger_id: TEST_LEDGER_ID.to_string(),
                record_date: "2023-07-25".to_string(),
                event_type: consts::STOCK_EVENT_ADD_PRINCIPAL.to_string(),
                event_text: "追加本金".to_string(),
                amount_change: 5_000_000,
                cash_balance: 15_000_000,
                net_pnl: None,
                remark: String::new(),
                created_at: 0,
            };
            StockDao::create_fund_record(&conn, &record).unwrap();
        }
        // 第 2 笔亏损 -200000（2023-07-28）
        seed_clean_round_on(
            &workspace,
            TEST_CODE,
            TEST_NAME,
            2000,
            1800,
            10,
            "2023-07-28 12:00:00",
        );
        // 第 3 笔前支取 1,000,000（2023-07-29，本金不变）
        {
            let conn = workspace.connection();
            let record = StockFundRecord {
                id: tr_store::util::new_uuid(),
                ledger_id: TEST_LEDGER_ID.to_string(),
                record_date: "2023-07-29".to_string(),
                event_type: consts::STOCK_EVENT_WITHDRAW.to_string(),
                event_text: "支取".to_string(),
                amount_change: -1_000_000,
                cash_balance: 0,
                net_pnl: None,
                remark: String::new(),
                created_at: 0,
            };
            StockDao::create_fund_record(&conn, &record).unwrap();
        }
        // 第 3 笔盈利 +300000（2023-08-01）
        seed_clean_round_on(
            &workspace,
            TEST_CODE,
            TEST_NAME,
            1000,
            1300,
            10,
            "2023-08-01 12:00:00",
        );

        let stats = crate::stock_statistics::get_statistics(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(stats.points.len(), 3);
        assert_eq!(stats.principal, 15_000_000);

        let p1 = &stats.points[0];
        assert_eq!((p1.max_drawdown, p1.max_drawdown_pct), (0, 0.0));

        // 追加后当时本金 15,000,000；峰值总资产 15,100,000，回撤 200,000 → 1.33%
        let p2 = &stats.points[1];
        assert_eq!(p2.max_drawdown, 200_000);
        assert!(
            (p2.max_drawdown_pct - 1.33).abs() < 0.01,
            "实际 {}",
            p2.max_drawdown_pct
        );

        // 支取 1,000,000 计入总资产曲线：峰值 15,100,000 → 支取后 13,900,000 回撤 1,200,000
        let p3 = &stats.points[2];
        assert_eq!(p3.max_drawdown, 1_200_000);
        assert!(
            (p3.max_drawdown_pct - 8.0).abs() < 0.01,
            "实际 {}",
            p3.max_drawdown_pct
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn statistics_month_range_recomputes_window() {
        let (workspace, dir) = workspace("stats-month");

        seed_principal(&workspace, 10_000_000);
        // 2023-07：+100000 → -80000；2023-08：+50000
        seed_clean_round_on(
            &workspace,
            TEST_CODE,
            TEST_NAME,
            1000,
            1100,
            10,
            "2023-07-22 12:00:00",
        );
        seed_clean_round_on(
            &workspace,
            TEST_CODE_B,
            TEST_NAME_B,
            1000,
            920,
            10,
            "2023-07-25 12:00:00",
        );
        seed_clean_round_on(
            &workspace,
            TEST_CODE,
            TEST_NAME,
            1000,
            1050,
            10,
            "2023-08-01 12:00:00",
        );

        let stats = crate::stock_statistics::get_statistics_range(
            &workspace,
            TEST_LEDGER_ID,
            "2023-07",
            "2023-07",
            0,
            "",
        )
        .unwrap();
        assert_eq!((stats.round_count, stats.points.len()), (2, 2));
        let p1 = &stats.points[0];
        assert_eq!(
            (p1.sequence, p1.total_pnl, p1.win_count, p1.max_drawdown),
            (1, 100_000, 1, 0)
        );
        let p2 = &stats.points[1];
        assert_eq!(
            (p2.sequence, p2.total_pnl, p2.win_count, p2.loss_count),
            (2, 20_000, 1, 1)
        );
        assert!((p2.win_rate - 50.0).abs() < 0.01);
        assert_eq!((p2.avg_win, p2.avg_loss), (100_000, 80_000));
        let ratio = p2.pnl_ratio.expect("应有盈亏比");
        assert!((ratio - 1.25).abs() < 0.001);
        assert_eq!(p2.expectancy, 10_000);
        assert_eq!(p2.max_drawdown, 80_000);
        assert!((p2.max_drawdown_pct - 0.8).abs() < 0.001);

        let august = crate::stock_statistics::get_statistics_range(
            &workspace,
            TEST_LEDGER_ID,
            "2023-08",
            "2023-08",
            0,
            "",
        )
        .unwrap();
        assert_eq!((august.round_count, august.points.len()), (1, 1));
        assert_eq!(august.points[0].sequence, 1);
        assert_eq!(august.points[0].total_pnl, 50_000);

        // 跨月区间不应遗漏边界内数据
        let cross = crate::stock_statistics::get_statistics_range(
            &workspace,
            TEST_LEDGER_ID,
            "2023-07",
            "2023-12",
            0,
            "",
        )
        .unwrap();
        assert_eq!(cross.round_count, 3);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn statistics_recent_n_recomputes_window() {
        let (workspace, dir) = workspace("stats-recent");

        seed_principal(&workspace, 10_000_000);
        seed_clean_round_on(
            &workspace,
            TEST_CODE,
            TEST_NAME,
            1000,
            1100,
            10,
            "2023-07-22 12:00:00",
        ); // +100000
        seed_clean_round_on(
            &workspace,
            TEST_CODE_B,
            TEST_NAME_B,
            1000,
            920,
            10,
            "2023-07-25 12:00:00",
        ); // -80000
        seed_clean_round_on(
            &workspace,
            TEST_CODE,
            TEST_NAME,
            1000,
            1050,
            10,
            "2023-08-01 12:00:00",
        ); // +50000

        let stats = crate::stock_statistics::get_statistics_range(
            &workspace,
            TEST_LEDGER_ID,
            "",
            "",
            2,
            "",
        )
        .unwrap();
        assert_eq!((stats.round_count, stats.points.len()), (2, 2));
        assert_eq!(
            (stats.points[0].sequence, stats.points[0].pnl),
            (1, -80_000)
        );
        assert_eq!(
            (stats.points[1].sequence, stats.points[1].total_pnl),
            (2, -30_000)
        );

        // 请求笔数超过总笔数时返回全部
        let all = crate::stock_statistics::get_statistics_range(
            &workspace,
            TEST_LEDGER_ID,
            "",
            "",
            100,
            "",
        )
        .unwrap();
        assert_eq!((all.round_count, all.points.len()), (3, 3));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn statistics_range_drawdown_percent_uses_principal_at_point() {
        let (workspace, dir) = workspace("stats-range-drawdown");

        seed_principal(&workspace, 10_000_000);
        // 第 1 笔亏损 -200000（2023-01-05），随后追加本金 500 万，第 2 笔亏损 -300000（2023-01-20）
        seed_clean_round_on(
            &workspace,
            TEST_CODE,
            TEST_NAME,
            2000,
            1800,
            10,
            "2023-01-05 12:00:00",
        );
        {
            let conn = workspace.connection();
            StockDao::update_account_principal(&conn, TEST_LEDGER_ID, 15_000_000).unwrap();
            let record = StockFundRecord {
                id: tr_store::util::new_uuid(),
                ledger_id: TEST_LEDGER_ID.to_string(),
                record_date: "2023-01-10".to_string(),
                event_type: consts::STOCK_EVENT_ADD_PRINCIPAL.to_string(),
                event_text: "追加本金".to_string(),
                amount_change: 5_000_000,
                cash_balance: 15_000_000,
                net_pnl: None,
                remark: String::new(),
                created_at: 0,
            };
            StockDao::create_fund_record(&conn, &record).unwrap();
        }
        seed_clean_round_on(
            &workspace,
            TEST_CODE_B,
            TEST_NAME_B,
            2000,
            1700,
            10,
            "2023-01-20 12:00:00",
        );

        let stats = crate::stock_statistics::get_statistics_range(
            &workspace,
            TEST_LEDGER_ID,
            "2023-01",
            "2023-01",
            0,
            "",
        )
        .unwrap();
        assert_eq!(stats.round_count, 2);

        // 区间曲线从 0 起步：第 1 笔后累计 -200000，回撤 200000 ÷ 当时本金 1000 万 = 2%
        let p1 = &stats.points[0];
        assert_eq!(p1.max_drawdown, 200_000);
        assert!((p1.max_drawdown_pct - 2.0).abs() < 0.01);

        // 区间曲线峰值仍为 0，第 2 笔后累计 -500000 → 回撤 500000；当时本金 1500 万 → 3.33%
        let p2 = &stats.points[1];
        assert_eq!(p2.max_drawdown, 500_000);
        assert!((p2.max_drawdown_pct - 3.33).abs() < 0.01);
        assert_eq!(p2.total_pnl, -500_000);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn statistics_range_validation() {
        let (workspace, dir) = workspace("stats-validation");

        let cases: Vec<(&str, &str, i64)> = vec![
            ("2023-01", "", 0),
            ("", "2023-01", 0),
            ("2023/01", "2023-02", 0),
            ("2023-03", "2023-01", 0),
            ("2023-01", "2023-02", 5),
            ("", "", -1),
        ];
        for (start, end, recent) in cases {
            assert!(
                crate::stock_statistics::get_statistics_range(
                    &workspace,
                    TEST_LEDGER_ID,
                    start,
                    end,
                    recent,
                    ""
                )
                .is_err(),
                "({start}, {end}, {recent}) 应报错"
            );
        }

        // 区间内无结算：返回空统计且不报错
        let empty = crate::stock_statistics::get_statistics_range(
            &workspace,
            TEST_LEDGER_ID,
            "2023-06",
            "2023-06",
            0,
            "",
        )
        .unwrap();
        assert_eq!((empty.round_count, empty.points.len()), (0, 0));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 造 6 轮带标签的干净结算：
    /// A1(07-01,打板,+100000) B1(07-02,分析,+50000) A2(07-03,尾盘,-80000)
    /// B2(08-01,打板,+50000) A3(08-02,打板,+50000) B3(08-05,追涨,-80000)
    fn seed_tagged_stat_rounds(workspace: &Workspace) {
        seed_principal(workspace, 10_000_000);
        seed_clean_round_on(
            workspace,
            TEST_CODE,
            TEST_NAME,
            1000,
            1100,
            10,
            "2023-07-01 12:00:00",
        );
        seed_clean_round_on(
            workspace,
            TEST_CODE_B,
            TEST_NAME_B,
            800,
            850,
            10,
            "2023-07-02 12:00:00",
        );
        seed_clean_round_on(
            workspace,
            TEST_CODE,
            TEST_NAME,
            2000,
            1920,
            10,
            "2023-07-03 12:00:00",
        );
        seed_clean_round_on(
            workspace,
            TEST_CODE_B,
            TEST_NAME_B,
            800,
            850,
            10,
            "2023-08-01 12:00:00",
        );
        seed_clean_round_on(
            workspace,
            TEST_CODE,
            TEST_NAME,
            1000,
            1050,
            10,
            "2023-08-02 12:00:00",
        );
        seed_clean_round_on(
            workspace,
            TEST_CODE_B,
            TEST_NAME_B,
            2000,
            1920,
            10,
            "2023-08-05 12:00:00",
        );

        // 首次统计触发存量历史回填生成轮次，再按轮设置标签
        crate::stock_statistics::get_statistics(workspace, TEST_LEDGER_ID).unwrap();
        let assign_tags = |code: &str, tags: &[&str]| {
            let detail = get_trade_history_detail(workspace, TEST_LEDGER_ID, code).unwrap();
            assert_eq!(detail.rounds.len(), tags.len());
            for (index, tag) in tags.iter().enumerate() {
                update_round_tag(workspace, TEST_LEDGER_ID, &detail.rounds[index].id, tag).unwrap();
            }
        };
        assign_tags(
            TEST_CODE,
            &[
                consts::STOCK_TAG_DABAN,
                consts::STOCK_TAG_WEIPAN,
                consts::STOCK_TAG_DABAN,
            ],
        );
        assign_tags(
            TEST_CODE_B,
            &[
                consts::STOCK_TAG_ANALYSIS,
                consts::STOCK_TAG_DABAN,
                consts::STOCK_TAG_ZHUIZHANG,
            ],
        );
    }

    #[test]
    fn statistics_tag_field_and_tag_filter() {
        let (workspace, dir) = workspace("stats-tags");
        seed_tagged_stat_rounds(&workspace);

        let all = crate::stock_statistics::get_statistics_range(
            &workspace,
            TEST_LEDGER_ID,
            "",
            "",
            0,
            "",
        )
        .unwrap();
        assert_eq!((all.round_count, all.points.len()), (6, 6));
        let want_tags = [
            consts::STOCK_TAG_DABAN,
            consts::STOCK_TAG_ANALYSIS,
            consts::STOCK_TAG_WEIPAN,
            consts::STOCK_TAG_DABAN,
            consts::STOCK_TAG_DABAN,
            consts::STOCK_TAG_ZHUIZHANG,
        ];
        for (index, want) in want_tags.iter().enumerate() {
            assert_eq!(all.points[index].tag, *want);
            assert_eq!(all.points[index].sequence, index as i64 + 1);
        }

        // 按「打板」筛选：只保留 3 笔，序号从 1 重新累计，累计盈亏只含筛选集合
        let daban = crate::stock_statistics::get_statistics_range(
            &workspace,
            TEST_LEDGER_ID,
            "",
            "",
            0,
            consts::STOCK_TAG_DABAN,
        )
        .unwrap();
        assert_eq!((daban.round_count, daban.points.len()), (3, 3));
        for (index, code) in [TEST_CODE, TEST_CODE_B, TEST_CODE].iter().enumerate() {
            let point = &daban.points[index];
            assert_eq!(point.sequence, index as i64 + 1);
            assert_eq!(point.tag, consts::STOCK_TAG_DABAN);
            assert_eq!(point.stock_code, *code);
        }
        assert_eq!(daban.points[0].total_pnl, 100_000);
        assert_eq!(daban.points[1].total_pnl, 150_000);
        assert_eq!(daban.points[2].total_pnl, 200_000);
        assert_eq!(
            (daban.points[2].win_count, daban.points[2].win_rate),
            (3, 100.0)
        );

        // 其它标签逐一命中；不存在的组合返回空统计
        for tag in [
            consts::STOCK_TAG_ANALYSIS,
            consts::STOCK_TAG_WEIPAN,
            consts::STOCK_TAG_ZHUIZHANG,
        ] {
            let part = crate::stock_statistics::get_statistics_range(
                &workspace,
                TEST_LEDGER_ID,
                "",
                "",
                0,
                tag,
            )
            .unwrap();
            assert_eq!(part.round_count, 1);
            assert_eq!(part.points[0].tag, tag);
        }

        // 非法标签拒绝
        let error = crate::stock_statistics::get_statistics_range(
            &workspace,
            TEST_LEDGER_ID,
            "",
            "",
            0,
            "打新",
        )
        .unwrap_err();
        assert!(error.to_string().contains("无效的交易标签"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn statistics_tag_combines_with_month_and_recent() {
        let (workspace, dir) = workspace("stats-tags-combined");
        seed_tagged_stat_rounds(&workspace);

        // 标签 × 月份：7 月打板 1 笔（A1），8 月打板 2 笔（B2、A3）
        let july = crate::stock_statistics::get_statistics_range(
            &workspace,
            TEST_LEDGER_ID,
            "2023-07",
            "2023-07",
            0,
            consts::STOCK_TAG_DABAN,
        )
        .unwrap();
        assert_eq!(july.round_count, 1);
        assert_eq!(july.points[0].stock_code, TEST_CODE);
        assert_eq!(july.points[0].sequence, 1);

        let august = crate::stock_statistics::get_statistics_range(
            &workspace,
            TEST_LEDGER_ID,
            "2023-08",
            "2023-08",
            0,
            consts::STOCK_TAG_DABAN,
        )
        .unwrap();
        assert_eq!((august.round_count, august.points.len()), (2, 2));
        assert_eq!(august.points[1].total_pnl, 100_000);

        // 标签 × 最近 N：最近 1 笔打板 = A3（2023-08-02）；取最近 100 笔回到全部 3 笔
        let one = crate::stock_statistics::get_statistics_range(
            &workspace,
            TEST_LEDGER_ID,
            "",
            "",
            1,
            consts::STOCK_TAG_DABAN,
        )
        .unwrap();
        assert_eq!(one.round_count, 1);
        assert_eq!(one.points[0].stock_code, TEST_CODE);
        assert_eq!(one.points[0].stock_round_no, 3);

        let all = crate::stock_statistics::get_statistics_range(
            &workspace,
            TEST_LEDGER_ID,
            "",
            "",
            100,
            consts::STOCK_TAG_DABAN,
        )
        .unwrap();
        assert_eq!((all.round_count, all.points.len()), (3, 3));

        std::fs::remove_dir_all(&dir).ok();
    }

    // ---------- 股票名 / 重置 ----------

    #[test]
    fn lookup_stock_name_prefers_local_then_falls_back() {
        let (workspace, dir) = workspace("name-lookup");
        let fetcher = StubQuoteFetcher {
            quotes: HashMap::new(),
            name: "外部名称".to_string(),
        };

        // 本地无记录 → 走外部
        let dto = lookup_stock_name_with(&workspace, TEST_CODE, &fetcher).unwrap();
        assert_eq!(dto.stock_code, TEST_CODE);
        assert_eq!(dto.stock_name, "外部名称");

        // 本地有记录 → 用本地
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            1000,
            10,
            1_700_000_000,
            "",
            "",
        )
        .unwrap();
        let dto = lookup_stock_name_with(&workspace, TEST_CODE, &fetcher).unwrap();
        assert_eq!(dto.stock_name, TEST_NAME);

        assert!(lookup_stock_name(&workspace, "").is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reset_data_clears_trades_and_account() {
        let (workspace, dir) = workspace("reset-data");

        seed_principal(&workspace, 10_000_000);
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            1000,
            10,
            1_700_000_000,
            "",
            "",
        )
        .unwrap();

        reset_data(&workspace, TEST_LEDGER_ID).unwrap();

        let conn = workspace.connection();
        assert!(is_not_found(
            &StockDao::get_account(&conn, TEST_LEDGER_ID).unwrap_err()
        ));
        assert!(StockDao::list_all_trades_asc(&conn, TEST_LEDGER_ID)
            .unwrap()
            .is_empty());
        assert!(StockDao::list_positions(&conn, TEST_LEDGER_ID)
            .unwrap()
            .is_empty());
        drop(conn);

        // 重建账户时本金归零
        let overview = get_overview(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(overview.principal, 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 手写一个把「设置本金」重复调用两次的回归：确认首次调用后再调用会报 409
    /// （对应 `SetPrincipal` 的 count > 0 分支）。
    #[test]
    fn fee_settings_roundtrip_and_validation() {
        let (workspace, dir) = workspace("fee-settings");

        let defaults = get_or_create_fee_setting(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(defaults.commission_rate, 0.0002354);
        assert_eq!(defaults.min_commission, 500);
        assert_eq!(defaults.stamp_duty_rate, 0.0005);
        assert_eq!(defaults.transfer_fee_rate, 0.00001);

        assert!(save_fee_settings(&workspace, TEST_LEDGER_ID, 0.0, 500, 0.0005, 0.00001).is_err());
        assert!(
            save_fee_settings(&workspace, TEST_LEDGER_ID, 0.0001, -1, 0.0005, 0.00001).is_err()
        );

        let saved =
            save_fee_settings(&workspace, TEST_LEDGER_ID, 0.0003, 1000, 0.001, 0.00002).unwrap();
        assert_eq!(saved.commission_rate, 0.0003);
        assert_eq!(saved.min_commission, 1000);
        let reloaded = get_or_create_fee_setting(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(reloaded.commission_rate, 0.0003);
        assert_eq!(reloaded.min_commission, 1000);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_fund_records_pagination_bounds() {
        let (workspace, dir) = workspace("fund-pagination");

        seed_principal(&workspace, 10_000_000);
        for index in 0..12 {
            add_principal_at_date(
                &workspace,
                TEST_LEDGER_ID,
                1000,
                &format!("2026-01-{:02}", index + 1),
            )
            .unwrap();
        }

        // page < 1 → 1；page_size < 1 → 10；page_size > 100 → 100
        let page = list_fund_records(&workspace, TEST_LEDGER_ID, 0, 0).unwrap();
        assert_eq!((page.page, page.page_size), (1, 10));
        assert_eq!(page.total, 12);
        assert_eq!(page.items.len(), 10);

        let page = list_fund_records(&workspace, TEST_LEDGER_ID, 2, 200).unwrap();
        assert_eq!(page.page_size, 100, "page_size 上限为 100");
        // 第二页已越过 12 条记录（100/页），返回空页
        assert!(page.items.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    // ==================================================================== 操作记录与回滚

    /// 股票侧「库内容指纹」：各表行数 + 轮次的标签/复盘 + 本金。
    ///
    /// 用来断言「预演绝不落库」——只比对行数不够（改写复盘也看得见），所以把轮次元数据也带上。
    fn stock_fingerprint(workspace: &Workspace) -> String {
        let conn = workspace.connection();
        let mut out = String::new();
        for table in [
            "tbl_billadm_stock_account",
            "tbl_billadm_stock_position",
            "tbl_billadm_stock_trade",
            "tbl_billadm_stock_trade_round",
            "tbl_billadm_stock_trade_history",
            "tbl_billadm_stock_fund_record",
            "tbl_billadm_stock_operation",
        ] {
            let rows: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            out.push_str(&format!("{table}={rows};"));
        }
        let mut statement = conn
            .prepare(
                "SELECT id, round_no, tag, review FROM tbl_billadm_stock_trade_round ORDER BY id",
            )
            .unwrap();
        let rounds: Vec<String> = statement
            .query_map([], |row| {
                Ok(format!(
                    "{}:{}:{}:{}",
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        out.push_str(&rounds.join("|"));
        let principal: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(principal), 0) FROM tbl_billadm_stock_account",
                [],
                |row| row.get(0),
            )
            .unwrap();
        out.push_str(&format!(";principal={principal}"));
        out
    }

    /// 某账本的资金记录条数。
    fn fund_record_count(workspace: &Workspace) -> i64 {
        db(StockDao::count_fund_records(
            &workspace.connection(),
            TEST_LEDGER_ID,
        ))
        .unwrap()
    }

    /// 某账本某只股票的持仓（行不存在时归零）。
    fn position_of(workspace: &Workspace, code: &str) -> (i64, i64) {
        match StockDao::get_position(&workspace.connection(), TEST_LEDGER_ID, code) {
            Ok(position) => (position.quantity, position.total_cost),
            Err(error) if is_not_found(&error) => (0, 0),
            Err(error) => panic!("读持仓失败: {error}"),
        }
    }

    #[test]
    fn rollback_undoes_add_principal_and_pops_the_log() {
        let (workspace, dir) = workspace("op-rollback-principal");
        seed_principal(&workspace, 1_000_000);
        let before = get_overview(&workspace, TEST_LEDGER_ID).unwrap();

        add_principal(&workspace, TEST_LEDGER_ID, 50_000).unwrap();
        let after = get_overview(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(after.principal, before.principal + 50_000);
        assert_eq!(after.available_cash, before.available_cash + 50_000);
        assert_eq!(fund_record_count(&workspace), 1);

        let operations = list_operations(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0].action, "追加本金");
        assert_eq!(operations[0].detail, "¥500.00");
        assert!(operations[0].created_at > 0);
        assert!(!operations[0].id.is_empty());

        let result = rollback_latest(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(result.action, "追加本金");
        assert!(!result.skipped, "目标在，不该走跳过路径");

        let restored = get_overview(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(restored.principal, before.principal, "本金要还原");
        assert_eq!(restored.available_cash, before.available_cash, "现金要还原");
        assert_eq!(fund_record_count(&workspace), 0, "那条资金记录要删掉");
        assert!(list_operations(&workspace, TEST_LEDGER_ID)
            .unwrap()
            .is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rollback_undoes_withdraw_and_interest_without_touching_principal() {
        let (workspace, dir) = workspace("op-rollback-fund-kinds");
        seed_principal(&workspace, 1_000_000);
        let principal = get_overview(&workspace, TEST_LEDGER_ID).unwrap().principal;

        add_interest(&workspace, TEST_LEDGER_ID, 12_345).unwrap();
        let after_interest = get_overview(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(after_interest.principal, principal, "利息不改本金");
        assert_eq!(
            list_operations(&workspace, TEST_LEDGER_ID).unwrap()[0].action,
            "利息归本"
        );

        rollback_latest(&workspace, TEST_LEDGER_ID).unwrap();
        let restored = get_overview(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(
            restored.available_cash,
            after_interest.available_cash - 12_345
        );
        assert_eq!(restored.principal, principal, "回滚利息也不改本金");
        assert_eq!(fund_record_count(&workspace), 0);

        add_withdraw(&workspace, TEST_LEDGER_ID, 5_000).unwrap();
        let after_withdraw = get_overview(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(
            list_operations(&workspace, TEST_LEDGER_ID).unwrap()[0].action,
            "支取"
        );

        rollback_latest(&workspace, TEST_LEDGER_ID).unwrap();
        let restored = get_overview(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(
            restored.available_cash,
            after_withdraw.available_cash + 5_000
        );
        assert_eq!(restored.principal, principal);
        assert_eq!(fund_record_count(&workspace), 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rollback_undoes_a_trade_order() {
        let (workspace, dir) = workspace("op-rollback-order");
        seed_principal(&workspace, 10_000_000);
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            10_000,
            3,
            1_700_000_000,
            "",
            "",
        )
        .unwrap();
        assert_eq!(position_of(&workspace, TEST_CODE).0, 300);
        assert_eq!(fund_record_count(&workspace), 1, "买入会记一条资金记录");

        let operations = list_operations(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0].action, "建仓");
        assert_eq!(operations[0].detail, "浦发银行 600000 · 3 手 · ¥30000.00");

        let result = rollback_latest(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(result.action, "建仓");
        assert!(!result.skipped);

        assert_eq!(position_of(&workspace, TEST_CODE), (0, 0), "持仓要回到零");
        assert_eq!(fund_record_count(&workspace), 0, "买卖资金记录随之消失");
        assert!(
            db(StockDao::list_trades(
                &workspace.connection(),
                TEST_LEDGER_ID,
                TEST_CODE
            ))
            .unwrap()
            .is_empty(),
            "成交也要一并消失"
        );
        assert!(list_operations(&workspace, TEST_LEDGER_ID)
            .unwrap()
            .is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rollback_pops_one_operation_at_a_time() {
        let (workspace, dir) = workspace("op-rollback-stack");
        seed_principal(&workspace, 10_000_000);
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            10_000,
            3,
            1_700_000_000,
            "",
            "",
        )
        .unwrap();
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_REDUCE,
            12_000,
            1,
            1_700_000_100,
            "",
            "",
        )
        .unwrap();
        assert_eq!(position_of(&workspace, TEST_CODE).0, 200);

        let operations = list_operations(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(operations.len(), 2);
        assert_eq!(operations[0].action, "减仓", "最新的在前");
        assert_eq!(operations[1].action, "建仓");

        // 第一次回滚撤销「减仓」：持仓回到 300，记录只剩「建仓」
        let first = rollback_latest(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(first.action, "减仓");
        assert_eq!(position_of(&workspace, TEST_CODE).0, 300);
        let left = list_operations(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].action, "建仓");

        // 第二次回滚撤销「建仓」：持仓归零，记录清空
        let second = rollback_latest(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(second.action, "建仓");
        assert_eq!(position_of(&workspace, TEST_CODE), (0, 0));
        assert!(list_operations(&workspace, TEST_LEDGER_ID)
            .unwrap()
            .is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn operation_log_keeps_only_the_newest_ten() {
        let (workspace, dir) = workspace("op-log-cap");
        seed_principal(&workspace, 10_000_000);
        for index in 1..=11 {
            add_principal(&workspace, TEST_LEDGER_ID, index * 100).unwrap();
        }

        let operations = list_operations(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(operations.len(), 10, "最多留 10 条");
        assert_eq!(operations[0].detail, "¥11.00", "最新一条在最前");
        assert_eq!(operations[9].detail, "¥2.00", "留下的是第 2..11 次");
        assert!(
            !operations.iter().any(|item| item.detail == "¥1.00"),
            "最旧的那条（第 1 次）已经被裁掉"
        );

        // 裁剪掉的只是记录：本金仍然是 11 次追加的总和
        let overview = get_overview(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(overview.principal, 10_000_000 + (1..=11).sum::<i64>() * 100);

        // 回滚一次只撤销最新那条（第 11 次 = ¥11.00）
        let result = rollback_latest(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(result.action, "追加本金");
        let overview = get_overview(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(overview.principal, 10_000_000 + (1..=10).sum::<i64>() * 100);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rollback_without_operations_is_a_bad_request() {
        let (workspace, dir) = workspace("op-rollback-empty");
        seed_principal(&workspace, 1_000_000);

        let error = rollback_latest(&workspace, TEST_LEDGER_ID)
            .unwrap_err()
            .into_app_error();
        assert_eq!(error.status, 400);
        assert_eq!(error.msg, "没有可回滚的操作");

        let error = preview_rollback(&workspace, TEST_LEDGER_ID)
            .unwrap_err()
            .into_app_error();
        assert_eq!(error.status, 400);
        assert_eq!(error.msg, "没有可回滚的操作");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rollback_skips_when_the_target_is_already_gone() {
        let (workspace, dir) = workspace("op-rollback-skipped");
        seed_principal(&workspace, 10_000_000);
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            10_000,
            3,
            1_700_000_000,
            "",
            "",
        )
        .unwrap();

        // 用「删除整笔委托」把目标拿掉：这不是一次被记录的操作，记录因此指向空气
        let order_id = list_trades(&workspace, TEST_LEDGER_ID, TEST_CODE).unwrap()[0]
            .order_id
            .clone();
        delete_trade_order(&workspace, TEST_LEDGER_ID, &order_id).unwrap();

        let result = rollback_latest(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(result.action, "建仓");
        assert!(result.skipped, "目标不在 → 跳过而不是报错");
        assert!(
            list_operations(&workspace, TEST_LEDGER_ID)
                .unwrap()
                .is_empty(),
            "记录要弹掉，否则一条死记录会把栈顶卡住"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rollback_preview_lists_lost_rounds_and_never_writes() {
        let (workspace, dir) = workspace("op-preview");
        seed_principal(&workspace, 10_000_000);
        close_round_helper(&workspace, TEST_CODE, TEST_NAME, 10_000, 12_000);
        let detail = get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_CODE).unwrap();
        let round_id = detail.rounds[0].id.clone();
        update_round_review(&workspace, TEST_LEDGER_ID, &round_id, "本轮复盘原文").unwrap();

        let operations_before = list_operations(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(operations_before.len(), 2, "建仓 + 清仓");
        let fingerprint = stock_fingerprint(&workspace);

        let preview = preview_rollback(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(preview.action, "清仓");
        assert_eq!(preview.removed_rounds.len(), 1);
        assert_eq!(preview.removed_rounds[0].round_no, 1);
        assert!(
            preview.removed_rounds[0].has_review,
            "这一轮写了复盘，确认框要说得出来"
        );

        // 预演绝不落库：行数、轮次标签/复盘、本金一个都没动，记录也没被弹掉
        assert_eq!(stock_fingerprint(&workspace), fingerprint);
        assert_eq!(
            list_operations(&workspace, TEST_LEDGER_ID).unwrap().len(),
            operations_before.len()
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reset_clears_the_operation_log() {
        let (workspace, dir) = workspace("op-reset-clears-log");
        seed_principal(&workspace, 1_000_000);
        add_principal(&workspace, TEST_LEDGER_ID, 50_000).unwrap();
        create_trade(
            &workspace,
            TEST_LEDGER_ID,
            TEST_CODE,
            TEST_NAME,
            consts::STOCK_TRADE_OPEN,
            10_000,
            1,
            1_700_000_000,
            "",
            "",
        )
        .unwrap();
        assert_eq!(
            list_operations(&workspace, TEST_LEDGER_ID).unwrap().len(),
            2
        );

        reset_data(&workspace, TEST_LEDGER_ID).unwrap();
        assert!(
            list_operations(&workspace, TEST_LEDGER_ID)
                .unwrap()
                .is_empty(),
            "重置必须把操作记录一起清掉，否则会留下指向已删数据的陈旧记录"
        );
        // 清干净之后回滚是明确的 400，而不是拿着陈旧 target 去删别的东西
        assert_eq!(
            rollback_latest(&workspace, TEST_LEDGER_ID)
                .unwrap_err()
                .into_app_error()
                .msg,
            "没有可回滚的操作"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rollback_of_close_removes_the_round_and_its_review() {
        let (workspace, dir) = workspace("op-rollback-close");
        seed_principal(&workspace, 10_000_000);
        close_round_helper(&workspace, TEST_CODE, TEST_NAME, 10_000, 12_000);
        let detail = get_trade_history_detail(&workspace, TEST_LEDGER_ID, TEST_CODE).unwrap();
        let round_id = detail.rounds[0].id.clone();
        update_round_review(&workspace, TEST_LEDGER_ID, &round_id, "本轮复盘原文").unwrap();

        let result = rollback_latest(&workspace, TEST_LEDGER_ID).unwrap();
        assert_eq!(result.action, "清仓");
        assert!(!result.skipped);

        // 清仓被撤销后该轮次不再成立：轮次行随之消失（这正是确认框要说明的"复盘会丢"）
        assert!(
            db(StockDao::list_trade_rounds(
                &workspace.connection(),
                TEST_LEDGER_ID
            ))
            .unwrap()
            .is_empty(),
            "轮次不再成立"
        );
        assert_eq!(
            position_of(&workspace, TEST_CODE).0,
            1000,
            "回到清仓之前的持仓"
        );
        assert_eq!(
            list_operations(&workspace, TEST_LEDGER_ID).unwrap()[0].action,
            "建仓",
            "栈里只剩「建仓」"
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
