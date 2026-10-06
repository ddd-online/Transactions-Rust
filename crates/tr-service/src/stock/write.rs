//! 股票写入聚合：把「写成交 / 资金记录」与「对齐派生数据」收到一处。
//!
//! 股票域有一半数据是**派生**的 —— 持仓数量与成本、每笔成交的已实现盈亏、轮次（round）挂接、
//! 资金记录的现金链余额，都由成交流水推出来。以前"写入之后谁负责把它们对齐"散在四条控制流里：
//! 下单增量维护，改成交 / 删委托写完整体重放，回滚还要按操作类型分流 —— 加一条新的写入路径，
//! 就得重新推一遍"这次要不要重放、重放哪一半"。
//!
//! 这个 module 把那件事收进一个 interface：
//!
//! * 对外只暴露**意图** —— 写一笔委托（[`create_trade_order`] / [`create_trade`]）、
//!   改一笔成交（[`update_trade_fill`]）、删一个委托（[`delete_trade_order`]）、
//!   回滚最近一次操作（[`rollback_latest`]）；另有两条**预演**（[`preview_trade_change`] /
//!   [`preview_rollback`]）：在事务里真的执行改动、比对前后差异，最后用哨兵错误强制回滚，
//!   **绝不落库**。
//! * **一次意图 = 一个事务**：事务边界在这一层；调用方（`tr-ipc` 命令面）看到的与从前逐字相同。
//! * 对齐派生数据的策略**按操作分流**，这些差异是保留的既有行为、不是待统一的缺陷：
//!
//! | 操作 | 对齐方式 |
//! |---|---|
//! | 下单 / 建仓 | **增量维护**：委托级一次计收费用、按成交额分摊，就地更新持仓与资金记录，**不重放** |
//! | 改一笔成交 | 按当前费用设置重算该委托各笔的费用 → 整体重放 → 复算现金链 |
//! | 删一个委托 | 删掉该委托的全部成交 → 整体重放 → 复算现金链 |
//! | 回滚（委托类） | 复用"删一个委托"这条意图 |
//! | 回滚（资金类） | 删掉那条资金记录（追加本金还要把本金改回去）→ 复算现金链 |
//! | 旧数据订正 | 触发条件可证明时整体重放（`repair_legacy_trade_fund_dates`） |
//!
//! `rebuild_trades`（整体重放）与 `recalculate_cash_chain`（现金链复算）只在**本 module 内**被调用；
//! 它们与"逐笔拆开"的事务函数（`update_trade_fill_tx` / `delete_trade_order_tx`）一起是这里的
//! 内部实现，不对外、也不给兄弟 module 用。

use std::collections::{BTreeMap, HashMap};

use tr_domain::consts;
use tr_domain::dto::{
    StockOperationRollbackDto, StockOperationRollbackPreviewDto, StockTradeDto,
    StockTradeImpactDto, StockTradeImpactRoundDto,
};
use tr_domain::error::AppError;
use tr_domain::fee;
use tr_domain::models::{
    StockFundRecord, StockPosition, StockTrade, StockTradeHistory, StockTradeRound,
};
use tr_store::dao::is_not_found;
use tr_store::dao::stock::StockDao;
use tr_store::Workspace;

use super::{
    amount_detail, contains_tag, ensure_stock_history_backfill, get_or_create_account_in,
    get_or_create_fee_setting_in, get_trade_tags, is_buy, is_sell, log_operation,
    parse_strict_date, trade_operation_label, unix_to_date, TradeFill, ERR_PREVIEW_ROLLBACK,
};
use crate::error::db;
use crate::{ServiceError, ServiceResult};

// ---------- 写入与重放辅助 ----------

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

/// 减仓按剩余总成本的比例结转成本（四舍五入到分），避免整除截断造成已实现盈亏偏差。
fn cost_basis_of(total_cost: i64, shares: i64, quantity: i64) -> i64 {
    (total_cost as f64 * shares as f64 / quantity as f64).round() as i64
}

/// `YYYY-MM-DD` → 后一天（解析失败返回 `None`）。
///
/// 只给旧数据订正用（判断"记录日期是否恰好早一天"）。
fn day_after(date: &str) -> Option<String> {
    let (year, month, day) = parse_strict_date(date)?;
    let next = chrono::NaiveDate::from_ymd_opt(year, month, day)?.succ_opt()?;
    Some(next.format("%Y-%m-%d").to_string())
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
    let shares = tr_domain::stock::shares_of(total_lots);
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
        let shares = tr_domain::stock::shares_of(fill.lots);
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
            item.shares = tr_domain::stock::shares_of(lots);
        }
        if trade_time > 0 {
            item.trade_time = trade_time;
        }
        item.amount = tr_domain::stock::amount_of(item.price, item.shares);
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
///
/// **私有**：重放是写入路径的内部步骤（下单 / 改成交 / 删委托 / 回滚各调一次），
/// 不是对外 interface —— 它收 `&rusqlite::Connection`，一旦公开就等于让调用方替这里
/// 操心事务边界与调用顺序。要触发重放，请走那些写入路径。
fn rebuild_trades(conn: &rusqlite::Connection, ledger_id: &str) -> ServiceResult<()> {
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
///
/// **私有**：与 [`rebuild_trades`] 同理，它是写入路径的一部分，不对外。
fn recalculate_cash_chain(conn: &rusqlite::Connection, ledger_id: &str) -> ServiceResult<()> {
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
///
/// 判据本身在 `tr_domain::fund::is_newer`（纯逻辑、native 真跑，并把"倒填记录会被跳过"这条现状
/// 钉成了断言）—— 这里只是把模型字段取出来，避免第二份实现（#36）。
fn fund_record_after(a: &StockFundRecord, b: &StockFundRecord) -> bool {
    tr_domain::fund::is_newer(
        (&a.record_date, a.created_at, &a.id),
        (&b.record_date, b.created_at, &b.id),
    )
}

// ==================================================================== 旧数据订正

/// 订正旧版本（≤ 0.6.0）写下的买卖资金记录日期，返回**被重放的账本数**。
///
/// 旧版本的资金记录日期按 UTC 取（见 [`unix_to_date`] 的注释），而委托时间戳是本地 00:00：
/// 东区（UTC+8）算出来比用户选的那天**早一天**。这个错位不止"日期显示错"——它还会让
/// **下一笔**记录在现金链里认不到它：`recalculate_cash_chain` 取的是
/// 「(日期, 创建时间, ID) 最大一条」的余额，被倒填日期的买/卖记录不是最大那条，
/// 于是它的金额被跳过，可用现金偏大（用户真实数据上偏了一笔建仓的钱）。
///
/// 订正方式不是去改那一个日期字符串，而是**重放**：日期与现金链一起按当前口径重算。
/// 触发条件是**可证明的**，不看宿主时区、也不看不认识的形状：
///
/// * 找出该记录的对应委托（同账本、同买卖方向、股票名出现在 `event_text` 里、
///   `created_at` 相差 ≤ 2 秒——资金记录与成交在同一个事务里写入，正常时间戳相等，
///   DAO 的严格递增最多把它挪后 1 秒）；
/// * 候选委托必须**都落在同一天**（多笔成交同一委托，或匹配到多条候选就跳过）；
/// * 那一天的本地日期必须**恰好是记录日期 + 1 天**。
///
/// 于是：当前版本写入的记录（日期 = 委托的本地日期）永远不触发；
/// 宿主在 UTC 或西区时旧记录本来就没错（本地 00:00 落在同一天）→ 什么都不做。
/// 匹配不到、或有歧义就**跳过**——宁可不动，也不猜。
///
/// 幂等：重放后日期等于委托的本地日期，条件不再成立。可以每次打开工作空间都跑一遍。
pub fn repair_legacy_trade_fund_dates(workspace: &Workspace) -> ServiceResult<usize> {
    workspace.transaction(|conn| {
        let mut repaired = 0_usize;
        for ledger_id in db(StockDao::list_trade_fund_ledger_ids(conn))? {
            if !has_legacy_trade_fund_date(conn, &ledger_id)? {
                continue;
            }
            // 重放会把该账本的买卖资金记录按当前口径全部重建（日期 + 现金链 + 持仓 / 轮次）
            rebuild_trades(conn, &ledger_id)?;
            tracing::info!("股票资金记录日期已按本地时区订正（账本 {ledger_id}）");
            repaired += 1;
        }
        Ok(repaired)
    })
}

/// 该账本是否存在「日期比对应委托的本地日期早一天」的买卖资金记录。
fn has_legacy_trade_fund_date(conn: &rusqlite::Connection, ledger_id: &str) -> ServiceResult<bool> {
    let records = db(StockDao::list_fund_records_in_insert_order(conn, ledger_id))?;
    let has_trade_record = records.iter().any(|record| {
        record.event_type == consts::STOCK_EVENT_BUY
            || record.event_type == consts::STOCK_EVENT_SELL
    });
    if !has_trade_record {
        return Ok(false);
    }

    let trades = db(StockDao::list_all_trades_asc(conn, ledger_id))?;
    for record in &records {
        let is_buy_record = record.event_type == consts::STOCK_EVENT_BUY;
        if !is_buy_record && record.event_type != consts::STOCK_EVENT_SELL {
            continue;
        }
        let mut dates: Vec<String> = Vec::new();
        for trade in &trades {
            if trade.stock_name.is_empty()
                || is_buy(&trade.trade_type) != is_buy_record
                || !record.event_text.contains(&trade.stock_name)
                || trade.created_at.abs_diff(record.created_at) > 2
            {
                continue;
            }
            let date = unix_to_date(trade.trade_time);
            if !dates.contains(&date) {
                dates.push(date);
            }
        }
        // 候选必须唯一（歧义就跳过），且恰好比记录日期晚一天
        if dates.len() == 1 && day_after(&record.record_date).as_deref() == Some(dates[0].as_str())
        {
            return Ok(true);
        }
    }
    Ok(false)
}

// ---------- 回滚 ----------

/// 目标不存在（404）。
///
/// 回滚时用它区分两种失败：目标被别的改动带走了（**跳过**，把记录弹掉）与真的出错（原样上报）。
fn is_missing_target(error: &ServiceError) -> bool {
    matches!(error, ServiceError::App(app) if app.status == 404)
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
