//! 记账 · **看板**的传输结构（结论式回顾：打开即答，不做配置）。
//!
//! ## 口径（改字段前先读）
//!
//! * **支出** = `expense` 合计（全部支出，不分类别）；
//! * **收入** = `income` 合计（本人记的是税后净额）；
//! * **结余** = 收入 − 支出，**转账不参与**（没有对手方，既不是收入也不是支出）。
//!
//! 「支出构成」是支出的下钻，两档合计都等于本期支出。
//!
//! ## 为什么卡片值都是 `Option`
//!
//! 一期里一条记录都没有时（新账本 / 翻到开始记账之前的月份），`None` 让界面显示 `—`：
//! 「花了 ¥0」和「没记账」是两件事，不能长得一样。

use serde::{Deserialize, Serialize};

// ------------------------------------------------------------------ 请求

/// 看板取数：**一期**（一个月或一整年）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountingOverviewRequest {
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    /// `"month"` | `"year"`（其它值按 month 处理）
    #[serde(rename = "periodKind")]
    pub period_kind: String,
    /// `"2026-10"` / `"2026"`；空串 = 当期（今天所在的月 / 年）
    #[serde(rename = "periodKey")]
    pub period_key: String,
}

// ------------------------------------------------------------------ 响应

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AccountingOverviewDto {
    pub period: OverviewPeriodDto,
    pub cards: OverviewCardsDto,
    /// 支出按分类的构成（金额降序，合计等于 `cards.expense`）
    pub categories: Vec<OverviewSliceDto>,
}

/// 这一期是什么、走到哪了。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OverviewPeriodDto {
    /// `"month"` | `"year"`
    pub kind: String,
    /// `"2026-10"` / `"2026"`
    pub key: String,
    /// 标题栏文案：`"2026年10月"` / `"2026年"`
    pub label: String,
    /// 是不是"现在正在走的那一期"（决定要不要按同期截断）
    pub is_current: bool,
    /// 月视图：这一期走到第几天（`None` = 已走完的月，或年视图）
    pub through_day: Option<i32>,
    /// 这一期属于哪一年
    pub year: i32,
}

/// 收支卡上的三个数。金额单位都是**分**。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OverviewCardsDto {
    /// 支出（本期）
    pub expense: Option<i64>,
    /// 收入（本期）
    pub income: Option<i64>,
    /// 结余（今年至今，不是本期 —— 半个月的结余是伪指标）
    pub balance_ytd: Option<i64>,
}

/// 构成里的一片：一个分类的金额与笔数。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OverviewSliceDto {
    pub label: String,
    pub amount: i64,
    pub count: i64,
    /// 是不是尾部合并出来的「其他」（界面据此弱化，别拿 `label == "其他"` 判 ——
    /// 真的有分类叫「其他」时会被误伤）
    pub is_other: bool,
}
