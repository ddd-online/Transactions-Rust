//! 全局常量。
//! 字符串字面量是数据兼容的一部分（数据库中存的是这些值），修改即为破坏性变更。

/// 工作空间内日志文件名。
pub const LOG_NAME: &str = "transactions.log";

/// 工作空间数据库文件名。
pub const DB_NAME: &str = "transactions.db";

/// `id=all` 的查询语义。
pub const ALL: &str = "all";

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

// ---------- 可回滚操作的种类 ----------
//
// 「操作记录」里的 `kind`：决定回滚时用哪条逆路径（见 `tr-service` 的 `rollback_latest`）。
// 八种可回滚操作只有两种形状，所以只有两个取值：
//   * `fund`  —— 追加本金 / 支取 / 利息归本：新建了一条资金记录（`target_id` = 资金记录 id），
//                追加本金另外把 `principal` 加上金额；
//   * `order` —— 建仓 / 加仓 / 减仓 / 清仓：新建了一个委托（`target_id` = `order_id`）。
/// 资金类操作（追加本金 / 支取 / 利息归本）
pub const STOCK_OP_KIND_FUND: &str = "fund";
/// 委托类操作（建仓 / 加仓 / 减仓 / 清仓）
pub const STOCK_OP_KIND_ORDER: &str = "order";

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

// ---------- 待办 ----------

/// 待办事项：进行中（卡片视图里显示的那批）
pub const TODO_STATUS_DOING: &str = "doing";
/// 待办事项：已完成（落在历史里）
pub const TODO_STATUS_DONE: &str = "done";

/// 合法的待办状态集合（校验用，顺序为 doing / done）。
pub const TODO_STATUSES: [&str; 2] = [TODO_STATUS_DOING, TODO_STATUS_DONE];

/// 紧急度 / 重要度的**六档**（弱 → 强）：低 / 中低 / 次低 / 次高 / 中高 / 高。
///
/// 值是**既有类型** `integer`（沿用 `-5..=5` 里的奇数），所以老数据不用换列；
/// 六档之外的老值（`0` / `±2` / `±4`，来自上一版的 11 档下拉与"没填"默认值）
/// 由迁移 `20260929_todo_levels_snap` 就近归档。四象限图的坐标范围仍是 `-5..=5`。
pub const TODO_LEVELS: [i32; 6] = [-5, -3, -1, 1, 3, 5];

/// 与 [`TODO_LEVELS`] **一一对应**的档位文案（顺序一致；界面与错误提示共用这一份）。
pub const TODO_LEVEL_LABELS: [&str; 6] = ["低", "中低", "次低", "次高", "中高", "高"];

/// 没评估过的默认档（次低）：既不假装紧急 / 重要，也不假装最低。
///
/// 上一版这一档是 `0`（"没填"），六档里没有 `0`，所以新建事项与老数据回填都落在这儿。
pub const TODO_LEVEL_DEFAULT: i32 = -1;

/// 就近取档：幅度向上取到相邻的奇数（`±2→±3`、`±4→±5`），超出 ±5 夹到端点，
/// `0` 落到 [`TODO_LEVEL_DEFAULT`]。
pub fn snap_todo_level(value: i32) -> i32 {
    let magnitude = value.unsigned_abs();
    let snapped = if magnitude >= 5 {
        5
    } else {
        (magnitude + (1 - magnitude % 2)) as i32
    };
    match value.cmp(&0) {
        std::cmp::Ordering::Less => -snapped,
        std::cmp::Ordering::Equal => TODO_LEVEL_DEFAULT,
        std::cmp::Ordering::Greater => snapped,
    }
}

/// 档位文案。不在六档里的值先就近取档 —— 手工改过库（或迁移没跑到）也只会看到六档文案，
/// 不会把裸数字渲染到界面上。
pub fn todo_level_label(value: i32) -> &'static str {
    let snapped = snap_todo_level(value);
    TODO_LEVELS
        .iter()
        .position(|level| *level == snapped)
        .map(|index| TODO_LEVEL_LABELS[index])
        .unwrap_or(TODO_LEVEL_LABELS[0])
}

/// 卡片主题最长字符数。
pub const TODO_CARD_TITLE_MAX: usize = 200;
/// 事项最长字符数。
pub const TODO_ITEM_TITLE_MAX: usize = 500;
/// 单条进度记录最长字符数。
pub const TODO_PROGRESS_MAX: usize = 2000;

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

    /// 六档的取档规则与文案（迁移与界面都依赖它，改规则必须同时改这条）。
    #[test]
    fn todo_levels_snap_and_label() {
        assert_eq!(TODO_LEVELS.len(), TODO_LEVEL_LABELS.len());
        for (level, label) in TODO_LEVELS.iter().zip(TODO_LEVEL_LABELS) {
            assert_eq!(snap_todo_level(*level), *level, "{level} 已是档位");
            assert_eq!(todo_level_label(*level), label);
        }
        // 幅度向上取 + 0 落到次低
        for (raw, snapped) in [
            (0, -1),
            (-2, -3),
            (-4, -5),
            (2, 3),
            (4, 5),
            (6, 5),
            (-7, -5),
        ] {
            assert_eq!(snap_todo_level(raw), snapped, "{raw} 应当取成 {snapped}");
        }
        assert_eq!(todo_level_label(4), "高");
        assert_eq!(todo_level_label(-2), "中低");
    }
}
