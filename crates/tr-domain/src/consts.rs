//! 全局常量。
//! 字符串字面量是数据兼容的一部分（数据库中存的是这些值），修改即为破坏性变更。

/// 工作空间内日志文件名。
pub const LOG_NAME: &str = "transactions.log";

/// 工作空间数据库文件名。
pub const DB_NAME: &str = "transactions.db";

/// `id=all` 的查询语义。
pub const ALL: &str = "all";

/// 所有数据表共用前缀。
pub const TABLE_PREFIX: &str = "tbl_billadm_";

// ---------- 交易类型 ----------

pub const TRANSACTION_TYPE_INCOME: &str = "income";
pub const TRANSACTION_TYPE_EXPENSE: &str = "expense";
pub const TRANSACTION_TYPE_TRANSFER: &str = "transfer";

/// 合法的交易类型集合（校验用，顺序为 income / expense / transfer）。
pub const TRANSACTION_TYPES: [&str; 3] = [
    TRANSACTION_TYPE_INCOME,
    TRANSACTION_TYPE_EXPENSE,
    TRANSACTION_TYPE_TRANSFER,
];

// ---------- 股票资金事件类型 ----------

/// 追加本金
pub const STOCK_EVENT_ADD_PRINCIPAL: &str = "add_principal";
/// 支取（从股票账户现金中取出，本金不变）
pub const STOCK_EVENT_WITHDRAW: &str = "withdraw";
/// 利息归本（账户利息 / 分红计入可用现金；本金不变，单独累计展示）
pub const STOCK_EVENT_INTEREST_PRINCIPAL: &str = "interest_principal";
/// 买入
pub const STOCK_EVENT_BUY: &str = "buy";
/// 卖出
pub const STOCK_EVENT_SELL: &str = "sell";

// ---------- 持仓交易类型 ----------

/// 建仓
pub const STOCK_TRADE_OPEN: &str = "open";
/// 加仓
pub const STOCK_TRADE_ADD: &str = "add";
/// 减仓
pub const STOCK_TRADE_REDUCE: &str = "reduce";
/// 清仓
pub const STOCK_TRADE_CLOSE: &str = "close";

// ---------- 轮次交易标签（策略分类，每轮一个；不设置时默认「分析」）----------

pub const STOCK_TAG_ANALYSIS: &str = "分析";
pub const STOCK_TAG_DABAN: &str = "打板";
pub const STOCK_TAG_WEIPAN: &str = "尾盘";
pub const STOCK_TAG_ZHUIZHANG: &str = "追涨";
pub const STOCK_TAG_XULI: &str = "蓄力";

/// 新账本默认可用交易标签（有序）。
pub fn default_stock_trade_tags() -> Vec<String> {
    vec![
        STOCK_TAG_ANALYSIS.to_string(),
        STOCK_TAG_DABAN.to_string(),
        STOCK_TAG_WEIPAN.to_string(),
        STOCK_TAG_ZHUIZHANG.to_string(),
        STOCK_TAG_XULI.to_string(),
    ]
}

/// 标签匹配策略：任一命中。
pub const TAG_POLICY_ANY: &str = "any";
/// 标签匹配策略：全部命中。
pub const TAG_POLICY_ALL: &str = "all";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tags_are_stable_and_ordered() {
        assert_eq!(
            default_stock_trade_tags(),
            vec!["分析", "打板", "尾盘", "追涨", "蓄力"]
        );
        // 列表内的字面量必须始终存在，否则会破坏既有账本的标签校验
        assert!(default_stock_trade_tags().contains(&STOCK_TAG_ANALYSIS.to_string()));
    }
}
