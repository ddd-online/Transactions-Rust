//! 记账 · **看板**的传输结构（结论式回顾：打开即答，不做配置）。
//!
//! ## 口径（改字段前先读）
//!
//! * **日常支出** = 支出里除「固定支出」以外的部分 —— 看板的主指标；
//! * **固定支出** = 分类上标了 `is_fixed` 的支出（房贷、贷款还款这类按月重复的）；
//! * **收入** = `income` 合计（本人记的是税后净额）；
//! * **结余** = 收入 − 日常 − 固定，**转账不参与**（没有对手方，既不是收入也不是支出）。
//!
//! 「日常 + 固定 = 支出合计」是硬等式，任何一侧改了另一侧必须跟着改。
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
    pub series: OverviewSeriesDto,
    pub composition: OverviewCompositionDto,
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
    /// 这一期属于哪一年（界面用它给"结余"标注范围：今年 / 某一年）
    pub year: i32,
    /// 是不是**今年**（决定结余卡写「今年至今」还是「2025 年」）
    pub is_current_year: bool,
}

/// 四张卡。金额单位都是**分**。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OverviewCardsDto {
    /// 日常支出（本期）
    pub daily_expense: Option<i64>,
    /// 固定支出（本期）
    pub fixed_expense: Option<i64>,
    /// 收入（本期）
    pub income: Option<i64>,
    /// 结余（今年至今，不是本期 —— 半个月的结余是伪指标）
    pub balance_ytd: Option<i64>,
    /// 去年同期：日常支出（按同期日数截断；去年那条为 `None` = 去年这个月没有记录）
    pub daily_expense_last_year: Option<i64>,
    /// 去年同期：收入
    pub income_last_year: Option<i64>,
    /// 固定支出的分类构成（房贷 / 还款 …），金额降序
    pub fixed_breakdown: Vec<OverviewSliceDto>,
}

/// 构成里的一片：分类或标签。
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

/// 折线的两段数据，下标一一对应。
///
/// 月视图 `mode = "daily"`：`labels` 是 `1..=31`，两段都是**日累计**；
/// 年视图 `mode = "monthly"`：`labels` 是 `1月..12月`，两段是当月发生额之和。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OverviewSeriesDto {
    pub labels: Vec<String>,
    /// 本期（单位：分，交给 `LineChart` 的 f64 序列）
    pub current: Vec<f64>,
    /// 去年同期（同样的横轴；长度与 `current` 一致）
    pub last_year: Vec<f64>,
    pub mode: String,
}

/// 支出构成两档：分类 / 标签（界面一个两档切换）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OverviewCompositionDto {
    /// 日常支出按分类（Top N + 「其他」由服务层合并）
    pub by_category: Vec<OverviewSliceDto>,
    /// 日常支出按标签（同一笔记录挂多个标签时**会各算一次** —— 标签档的百分比以这一档合计为分母）
    pub by_tag: Vec<OverviewSliceDto>,
}
