//! 股票页（`/stock_view`）—— 五个子功能共用一个版心与左侧图标条。
//!
//! 与记账页同构：版心骨架见 `components/ui/feature_page.rs`，左侧 `.page-rail` 是**子功能图标条**
//! （不是内容里的栏目），点图标切换子功能；五个子功能共用标题栏，标题固定为「股票」。
//! 各子功能都只负责"工具栏 + 内容区（+ 可选底栏）"。
//!
//! ## 页面组成
//!
//! | 组成 | 职责 |
//! |---|---|
//! | [`StockPage`] | 只决定"当前是哪个子功能"（定义见 [`StockSub`]） |
//! | [`StockSubRail`] | 子功能图标条（`FeaturePage` 的 `rail` 插槽内容） |
//! | [`account_view`] | 总资产卡 + 资金记录分页；工具栏 = 支取 / 利息归本 / 追加本金 |
//! | [`position_view`] | 持仓卡片 + 行情面板 + 本轮复盘 + 下单弹窗 + 成交表 + 影响预演；工具栏 = 建仓 |
//! | [`edit_modal`] / [`impact_modal`] | 编辑成交 / 删除委托 + 影响预演确认 |
//! | [`history_view`] | 全局汇总 + 已清仓股票列表 + 轮次 + 成交表 + 轮次复盘/标签（无工具栏） |
//! | [`statistics_view`] | 结算统计 + 自绘 SVG 曲线（统计）/ 逐笔结算明细分页（明细）；工具栏 = 分栏页签 + 区间/笔数/标签筛选 |
//! | [`settings_view`] | 交易标签 + 交易费用设置 + 操作记录（查看 / 回滚）+ 重置股票数据 |
//! | 状态组织 | 本文件直接持有信号（不引入 store；与其余页面一致） |
//!
//! 切子功能会**重建**该子功能的视图（信号随组件 owner 一起释放），因此来回切会重新拉数据 ——
//! 与"切一个页面"的行为一致，不会留下一份隐形的旧状态。各子功能里那个
//! `sub.get() == StockSub::Xxx` 的 `Effect` 仍要保留：同一子功能**重渲染**时（行情回来了、
//! 选中股变了）不该重发请求，只有"从别的子功能切回来"才刷。
//!
//! ## 单位与命名纪律
//!
//! * **金额恒为分**（`i64`），展示一律走 [`crate::format`]（内部用 `tr_domain::money`）。
//! * **价格入参按元**：`stock_trade_create` / `stock_trade_update` / `stock_trade_impact` 的
//!   `price` 是元（`f64`），后端负责 ×100 四舍五入（见 `tr-ipc/src/commands/stock.rs`）。
//! * 请求字段 snake_case、响应字段 camelCase —— 逐字照抄命令面，不做归一化。
//! * **A 股红涨绿跌**：本页作用域内的 `.amount-income` / `.amount-expense` 被 CSS 反向映射
//!   （盈 → 红、亏 → 绿），与记账域语义相反（本页作用域的覆盖写在 `stock.css`）。
//! * 「设置」子功能的费用设置与交易标签是**按账本**存的（不是应用级配置），
//!   金额换算一律过 `tr_domain::money`（`cents_to_yuan` / `yuan_to_cents`），不自行 `/100`。
//!
//! ## 行情降级的硬要求
//!
//! `latestPrice` / `prevClose` 为 `None`（行情接口失败）时：
//! * 现价显示 `-`（[`crate::format::quote_text`]）
//! * 浮动盈亏 / 当日涨跌显示 `—`（[`crate::format::optional_signed_percent`]）
//! * 持仓市值按**持仓成本**计入
//! * 全程不 `unwrap`、不 panic

use std::collections::BTreeSet;

use leptos::prelude::*;
use leptos::tachys::view::any_view::{AnyView, IntoAny};
use tr_domain::dto::{
    StockFundRecordDto, StockFundRecordPage, StockOperationDto, StockOperationRollbackPreviewDto,
    StockOverviewDto, StockPositionDto, StockStatisticsDto, StockStatisticsPointDto, StockTradeDto,
    StockTradeHistoryDetailDto, StockTradeHistoryDto, StockTradeHistorySummaryDto,
    StockTradeImpactDto, StockTradeImpactRoundDto, StockTradeRoundDto,
};
use tr_domain::fee::{compute_order_fee, is_shanghai_code, is_valid_stock_code};
use tr_domain::models::StockFeeSetting;
use tr_domain::money::{cents_to_yuan, yuan_to_cents};

use crate::api;
use crate::components::ui::{
    Button, ButtonSize, ButtonVariant, ChartConfig, ChartSeries, ChartValueKind, DatePicker, Empty,
    FeaturePage, Form, FormItem, FormLayout, Input, LineChart, Modal, ModalSize, Pagination,
    Segmented, SegmentedOption, Select, SelectOption, TabItem, TabPane, Tabs, Textarea, Tooltip,
};
use crate::error_handler::notify_error;
use crate::format::{self, lots_of};
use crate::icons::{self, Icon};
use crate::notify::Notifier;
use crate::store::AppStores;
use crate::time::{format_timestamp, today_ymd};

/// 页面标题（固定文案，改动即影响界面）。侧栏条目名也用这一个来源。
pub const PAGE_TITLE: &str = "股票";

/// 股票页的五个子功能（左侧图标条切换，顺序即渲染顺序）。
///
/// 与记账页同构：子功能走**图标条**，不进侧栏，也不再用顶部页签。
/// 「设置」是原「应用设置 → 股票」整块（费用设置 / 交易标签 / 重置股票数据）的迁入地。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockSub {
    /// 账户：总资产 + 支取/利息归本/追加本金 + 资金变化记录
    Account,
    /// 持仓：持仓卡片 + 行情 + 本轮复盘 + 下单 + 成交表
    Position,
    /// 记录：已清仓股票的轮次历史（原「成交记录」分栏）
    Trade,
    /// 统计：结算统计 + 曲线 + 逐笔结算明细（原「交易统计」分栏）
    Statistics,
    /// 设置：交易费用 / 交易标签 / 操作记录与回滚 / 重置股票数据
    Setting,
}

impl StockSub {
    /// 图标条顺序（顺序即渲染顺序）。
    pub const ALL: [StockSub; 5] = [
        Self::Account,
        Self::Position,
        Self::Trade,
        Self::Statistics,
        Self::Setting,
    ];

    /// 子功能名 —— 同时用作悬停提示与 `aria-label`（也是 fixtures 点它的可访问名）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Account => "账户",
            Self::Position => "持仓",
            Self::Trade => "记录",
            Self::Statistics => "统计",
            Self::Setting => "设置",
        }
    }

    /// 图标条上的图标（与全站同一套 Ant Design outlined 图标）。
    ///
    /// ⚠ 选图标时核两件事：**语义**与**路径坐标是否落在 896 格里**。「记录」早前用的
    /// `Icon::Sync` 路径 x 到 1133（896 格只到 960），渲染出来是一条扭曲的线 —— 换成了
    /// `Icon::History`（官方 outlined，坐标正常）。
    pub fn icon(self) -> Icon {
        match self {
            Self::Account => Icon::User,
            Self::Position => Icon::Stock,
            Self::Trade => Icon::History,
            Self::Statistics => Icon::BarChart,
            Self::Setting => Icon::Setting,
        }
    }
}

/// 资金记录每页条数（默认 10 条）。
const FUND_PAGE_SIZE: i64 = 10;

/// 本轮复盘模板（固定文案，改动即影响界面）。
const ROUND_REVIEW_TEMPLATE: &str = "判断层\n\n买入理由：\n卖出理由：\n\n改进层\n\n交易心得\n";

/// 复盘占位文案（固定文案，改动即影响界面）。
const REVIEW_PLACEHOLDER: &str = "写下本轮的操作依据、得失与可改进之处（500 字以内）";

/// 委托内一笔成交的输入行（元 / 手）。
#[derive(Debug, Clone, Copy, PartialEq)]
struct FillRow {
    price: f64,
    lots: i64,
}

impl FillRow {
    fn amount_cents(self) -> i64 {
        let price_cents = (self.price * 100.0).round() as i64;
        price_cents.saturating_mul(self.lots).saturating_mul(100)
    }
}

/// 费用预估结果（界面侧只用于**下单前预估**，与后端 `tr-domain::fee` 同口径）。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct FeeEstimate {
    commission: i64,
    stamp_duty: i64,
    transfer_fee: i64,
}

impl FeeEstimate {
    fn total(self) -> i64 {
        self.commission
            .saturating_add(self.stamp_duty)
            .saturating_add(self.transfer_fee)
    }
}

/// 预估本次委托的费用（与后端 `tr_domain::fee` 是**同一份算法**）。
///
/// 传的是**各笔成交额**而不是总额：印花税与过户费要逐笔取整再相加，
/// 只看总额会少算一分（见 `tr_domain::fee` 模块头的对照表）。
///
/// 保留"没有有效成交"的守卫：域函数在金额为 0 时会取最低佣金，
/// 而"还没填价格/手数"时预估应当是 0。
fn estimate_fee(fills: &[i64], is_buy: bool, code: &str, fee: &StockFeeSetting) -> FeeEstimate {
    let total: i64 = fills.iter().sum();
    if fills.is_empty() || total <= 0 {
        return FeeEstimate::default();
    }
    let breakdown = compute_order_fee(fills, is_shanghai_code(code), fee, is_buy);
    FeeEstimate {
        commission: breakdown.commission,
        stamp_duty: breakdown.stamp_duty,
        transfer_fee: breakdown.transfer_fee,
    }
}

/// 影响预演弹窗里要展示的信息。
#[derive(Debug, Clone, PartialEq)]
struct ImpactPrompt {
    /// `update_trade` | `delete_order`
    action: String,
    title: String,
    summary: String,
    /// 被编辑/删除的成交（`update_trade` 用）
    trade: Option<StockTradeDto>,
    impact: StockTradeImpactDto,
}

// ==================================================================== 页面外壳

// 统计视图是**普通函数**（不是组件）：父级每次重渲染都会重新调用它，函数体里的信号
// 跟着重建 —— 所以"数据/加载中/去重状态"都**不能放在组件信号里**，必须放模块级，
// 否则新挂载的那份看到的永远是空的（实测：去重挡住了请求风暴，页面却仍停在
// 「正在加载」/「暂无统计」，因为回来的数据写进了**上一份**已经被丢掉的信号）。
thread_local! {
    static STATS_DEDUPE: std::cell::RefCell<(String, Option<String>)> =
        const { std::cell::RefCell::new((String::new(), None)) };
    /// 最近一次成功取到的统计（跨视图重建保留）。
    ///
    /// **它是缓存，不是真相**：只在切账本与筛选变化时更新，而数据可能在别处被改掉
    /// （下单 / 改成交 / 减仓 / 清仓 / 重置股票数据）。所以 `统计` 视图每次挂载都会
    /// **复核一次**（见 ledger Effect 的 `!switched` 分支）；复核回来若与手上的值一致，
    /// 就不写信号 —— 否则整块图表会重建、入场动画重播。
    static STATS_DATA: std::cell::RefCell<Option<StockStatisticsDto>> =
        const { std::cell::RefCell::new(None) };
    /// 是否有统计请求在飞行中（跨视图重建保留）
    static STATS_LOADING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// 标签下拉的选项（跨视图重建保留，否则每次重建都要重新拉一次）。
    /// 与 `STATS_DATA` 同一条口径：同样是缓存，挂载时随数据一起复核
    /// （交易标签可以在「设置」子功能里被改掉）。
    static STATS_TAGS: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// **已经为哪个账本初始化过**。
    ///
    /// 必须有它：`统计` 视图每次重建都会把里面那个 Effect **重新创建**，而 Effect 自己的
    /// `prev` 参数是新建的、每次都从 `None` 开始 —— 用 `prev` 判断"账本有没有换"
    /// 等于每次重建都判定为"换了"，于是重跑：清缓存 → 重新拉标签 → 写 `tags` →
    /// 上层视图重渲染 → 再重建 → **自激循环**。
    /// 实测：统计页每秒重渲染 30+ 次，每轮都重建整块图表 SVG、**入场动画不断重播**，
    /// 用户看到的就是"曲线一直在闪"（闪的是动画起点那一段）。
    static STATS_LEDGER: std::cell::RefCell<String> =
        const { std::cell::RefCell::new(String::new()) };
}

/// 切账本时清掉统计缓存与去重状态（否则新账本会看到上一个账本的数据/被旧键挡住）。
///
/// 注意**不清 `STATS_LEDGER`**：它由下面那个 Effect 负责推进，两边都动会互相抵消。
fn reset_stats_cache() {
    STATS_DEDUPE.with(|state| {
        let mut state = state.borrow_mut();
        state.0.clear();
        state.1 = None;
    });
    STATS_DATA.with(|data| *data.borrow_mut() = None);
    STATS_LOADING.with(|loading| loading.set(false));
    STATS_TAGS.with(|tags| tags.borrow_mut().clear());
}

/// 这次统计请求要不要真的发出去（纯函数）。
///
/// 返回 `true` 表示"发"，并把 `in_flight` 置为当前键。
/// * 同一份键**已经请求过**（`state.0`）→ `false`，结果就在手上，不必再算一遍；
/// * `force` = **复核缓存**（见下面 ledger Effect 的 `!switched` 分支）：无视"已经请求过"，
///   但仍然**不重复发一份正在飞行中的同键请求**（`state.1`）—— 来回切子功能时那份请求
///   还在路上，再发一次只是让服务端把同一条结算序列重算一遍。
fn stats_request_needed(state: &mut (String, Option<String>), key: &str, force: bool) -> bool {
    if state.1.as_deref() == Some(key) {
        return false;
    }
    if force || state.0 != key {
        state.0 = key.to_string();
        state.1 = Some(key.to_string());
        return true;
    }
    false
}

/// 与记账页的 `AccountingPage` 同一套写法：五个子功能各自渲染一份 [`FeaturePage`]
/// （含左侧图标条），切子功能即重建该子功能的视图（信号随组件 owner 一起释放），
/// 因此来回切会重新拉数据 —— 与"切一个页面"的行为一致，不会留下一份隐形的旧状态。
#[component]
pub fn StockPage() -> impl IntoView {
    let sub = RwSignal::new(StockSub::Account);
    view! {
        {move || {
            match sub.get() {
                StockSub::Account => account_view(sub),
                StockSub::Position => position_view(sub),
                StockSub::Trade => history_view(sub),
                StockSub::Statistics => statistics_view(sub),
                StockSub::Setting => settings_view(sub),
            }
        }}
    }
}

/// 子功能图标条 —— `FeaturePage` 的 `rail` 插槽内容（外层 `.page-rail` 由它渲染）。
///
/// 每个子功能各渲染一份（与记账页的 `SubFunctionRail` 一致）：同一时刻只挂载一个。
#[component]
pub fn StockSubRail(sub: RwSignal<StockSub>) -> impl IntoView {
    view! {
        <nav class="page-rail-nav" aria-label="股票子功能">
            {StockSub::ALL
                .iter()
                .map(|item| {
                    let value = *item;
                    view! {
                        <button
                            type="button"
                            class="page-rail-btn"
                            class:is-active=move || sub.get() == value
                            title=item.label()
                            aria-label=item.label()
                            on:click=move |_| sub.set(value)
                        >
                            <span class="page-rail-btn-icon">{icons::icon(item.icon())}</span>
                        </button>
                    }
                })
                .collect_view()}
        </nav>
    }
}

// ==================================================================== 子功能一：账户

/// 账户子功能的三个资金操作（共用同一套「金额 + 日期」弹窗与提交逻辑）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FundAction {
    /// 支取：现金减少，本金不变
    Withdraw,
    /// 利息归本：账户利息计入可用现金（本金不变，单独累计展示）
    Interest,
    /// 追加本金：现金增加并抬高本金口径
    Principal,
}

impl FundAction {
    /// 工具栏按钮名 / 弹窗标题（同一个词，两处一致）。
    fn label(self) -> &'static str {
        match self {
            Self::Withdraw => "支取",
            Self::Interest => "利息归本",
            Self::Principal => "追加本金",
        }
    }

    /// 弹窗确认键文案（与标题分开：「追加本金」的确认键一直是更短的「追加」）。
    fn ok_text(self) -> &'static str {
        match self {
            Self::Withdraw => "支取",
            Self::Interest => "利息归本",
            Self::Principal => "追加",
        }
    }

    /// 弹窗里金额输入项的标签。
    fn amount_label(self) -> &'static str {
        match self {
            Self::Withdraw => "支取金额",
            Self::Interest => "利息金额",
            Self::Principal => "追加金额",
        }
    }

    /// 提交成功 / 失败的提示文案。
    fn success_text(self) -> &'static str {
        match self {
            Self::Withdraw => "支取成功",
            Self::Interest => "利息归本成功",
            Self::Principal => "追加本金成功",
        }
    }

    fn failure_text(self) -> &'static str {
        match self {
            Self::Withdraw => "支取失败",
            Self::Interest => "利息归本失败",
            Self::Principal => "追加本金失败",
        }
    }
}

/// 账户子功能：总览卡 + **工具栏**（支取 / 利息归本 / 追加本金）+ 资金变化记录。
///
/// 三个资金操作是本子功能的主操作，放在工具栏里（与其余子功能一致）；
/// 总资产卡里只留指标，不再重复放这些入口。
fn account_view(sub: RwSignal<StockSub>) -> AnyView {
    let stores = AppStores::global();
    let overview = RwSignal::new(StockOverviewDto::default());
    let overview_loading = RwSignal::new(false);
    let fund_page = RwSignal::new(StockFundRecordPage {
        page: 1,
        page_size: FUND_PAGE_SIZE as i32,
        ..StockFundRecordPage::default()
    });
    // 分页控件的页码是独立信号（`Pagination` 需要 `RwSignal<i32>`）：
    // 每次成功加载后从服务端 DTO 回写，用户点击时再触发查询。
    let fund_page_signal = RwSignal::new(1_i32);
    let records_loading = RwSignal::new(false);
    // 资金记录为空时表格里只有一行空态：给它一个钩子类，好让表格撑满表格区、
    // 空态在表头与底栏之间垂直居中（见 stock.css 的 `.stock-table-wrap--empty`）。
    let funds_empty = Signal::derive(move || fund_page.get().items.is_empty());
    let fee_settings = RwSignal::new(Option::<StockFeeSetting>::None);
    let mutating = RwSignal::new(false);

    // 三个资金操作的弹窗（共用 amount_text / amount_date）
    let principal_open = RwSignal::new(false);
    let interest_open = RwSignal::new(false);
    let withdraw_open = RwSignal::new(false);
    let amount_text = RwSignal::new(String::new());
    let amount_date = RwSignal::new(today_ymd());

    let load_overview = move || {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            return;
        }
        overview_loading.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::overview(&ledger_id).await {
                Ok(data) => overview.set(data),
                Err(error) => notify_error("查询股票账户总览失败", &error),
            }
            overview_loading.set(false);
        });
    };

    let load_fund_records = move |page: i64, page_size: i64| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            return;
        }
        records_loading.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::fund_records(&ledger_id, page, page_size).await {
                Ok(data) => {
                    fund_page_signal.set(data.page);
                    fund_page.set(data);
                }
                Err(error) => notify_error("查询资金变化记录失败", &error),
            }
            records_loading.set(false);
        });
    };

    // 费用设置只**读取**：编辑入口在本页的「设置」子功能，
    // 这里读回来是给下单弹窗估算费用用的（不在本子功能渲染表单）。
    let load_fee_settings = move || {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            match api::stock::fee_settings_get(&ledger_id).await {
                Ok(data) => fee_settings.set(Some(data)),
                Err(error) => notify_error("查询交易费用设置失败", &error),
            }
        });
    };

    // 账本切换 → 重载总览 / 费用设置，并把资金记录重置回第 1 页
    Effect::new(move |prev: Option<String>| {
        let ledger_id = stores.current_ledger_id.get();
        if prev.as_deref() == Some(ledger_id.as_str()) {
            return ledger_id;
        }
        if ledger_id.is_empty() {
            overview.set(StockOverviewDto::default());
            fund_page.set(StockFundRecordPage {
                page: 1,
                page_size: FUND_PAGE_SIZE as i32,
                ..StockFundRecordPage::default()
            });
            // 页码一并收回第 1 页，避免切回账本时先闪一下上一个账本的页码
            fund_page_signal.set(1);
            fee_settings.set(None);
            return ledger_id;
        }
        load_overview();
        load_fee_settings();
        // 资金记录由下面那个 Effect 统一查（账本 / 页码都是它的依赖）
        fund_page_signal.set(1);
        ledger_id
    });

    // 账本 / 页码 任一变化都要重查资金记录。
    //
    // 分页控件（`Pagination`）只写 `fund_page_signal`，**不会**自己去查数据；
    // 早前资金记录只在"账本切换"和"提交资金操作"时查询，于是点「上一页 / 下一页 / 页码」
    // 页码确实变了（按钮高亮跟着走），表格却还是原来那几行 —— 就是这里缺一个依赖页码的 Effect。
    Effect::new(move |prev: Option<(String, i32)>| {
        let ledger_id = stores.current_ledger_id.get();
        let page = fund_page_signal.get();
        // 账本换了就回第 1 页（不沿用上一个账本的页码）
        let target = match prev.as_ref() {
            Some((previous_ledger, _)) if previous_ledger == &ledger_id => page,
            _ => 1,
        };
        let current = (ledger_id, target);
        // 空账本不发查询；`prev == current` 时也不重查（服务端回写页码会再次触发本 Effect）
        if current.0.is_empty() || prev.as_ref() == Some(&current) {
            return current;
        }
        load_fund_records(target as i64, FUND_PAGE_SIZE);
        current
    });

    Effect::new(move |prev: Option<bool>| {
        let is_active = sub.get() == StockSub::Account;
        if is_active && prev == Some(false) {
            load_overview();
        }
        is_active
    });

    let submit_amount = move |action: FundAction| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        let raw = amount_text.get_untracked();
        let date = amount_date.get_untracked();
        if ledger_id.is_empty() {
            return;
        }
        let cents = match tr_domain::money::yuan_to_cents(&raw) {
            Ok(value) => value,
            Err(_) => {
                Notifier::global().error("请输入有效的金额".to_string(), None);
                return;
            }
        };
        if cents <= 0 {
            Notifier::global().error("金额必须大于 0".to_string(), None);
            return;
        }
        mutating.set(true);
        leptos::task::spawn_local(async move {
            let result = match action {
                FundAction::Withdraw => api::stock::withdraw(&ledger_id, cents, &date).await,
                FundAction::Interest => api::stock::interest_add(&ledger_id, cents, &date).await,
                FundAction::Principal => api::stock::principal_add(&ledger_id, cents, &date).await,
            };
            match result {
                Ok(data) => {
                    overview.set(data);
                    Notifier::global().success(action.success_text().to_string(), None);
                    principal_open.set(false);
                    interest_open.set(false);
                    withdraw_open.set(false);
                    amount_text.set(String::new());
                    // 回到第 1 页看新记录：页码变了由上面的 Effect 查，
                    // 本来就在第 1 页时页码信号不变，这里显式补一次查询
                    let was_first_page = fund_page_signal.get_untracked() == 1;
                    fund_page_signal.set(1);
                    if was_first_page {
                        load_fund_records(1, FUND_PAGE_SIZE);
                    }
                }
                Err(error) => notify_error(action.failure_text(), &error),
            }
            mutating.set(false);
        });
    };

    // 工具栏三个入口共用一套弹窗状态：清空金额、回到今天，再开对应的弹窗
    let open_amount_modal = move |action: FundAction| {
        amount_text.set(String::new());
        amount_date.set(today_ymd());
        match action {
            FundAction::Withdraw => withdraw_open.set(true),
            FundAction::Interest => interest_open.set(true),
            FundAction::Principal => principal_open.set(true),
        }
    };

    let page_snapshot = move || fund_page.get();
    let total_pages = Signal::derive(move || {
        let snapshot = page_snapshot();
        let size = snapshot.page_size.max(1) as i64;
        ((snapshot.total + size - 1) / size).max(1) as i32
    });

    // 版心两块：工具栏（本子功能的三个资金操作）/ 内容区，各自建好再交给 `FeaturePage`
    let toolbar = view! {
        <div class="stock-toolbar">
            <Button
                variant=ButtonVariant::Secondary
                loading=Signal::derive(move || mutating.get())
                on_click=move |_| open_amount_modal(FundAction::Withdraw)
            >
                {FundAction::Withdraw.label()}
            </Button>
            <Button
                variant=ButtonVariant::Secondary
                loading=Signal::derive(move || mutating.get())
                on_click=move |_| open_amount_modal(FundAction::Interest)
            >
                {FundAction::Interest.label()}
            </Button>
            <Button
                variant=ButtonVariant::Primary
                loading=Signal::derive(move || mutating.get())
                on_click=move |_| open_amount_modal(FundAction::Principal)
            >
                {FundAction::Principal.label()}
            </Button>
        </div>
    }
    .into_any();

    let content = view! {
        <div class="stock-body">
        <div class="stock-account">
            <div class="stock-overview">
                <div class="stock-overview__head">
                    <h3 class="stock-overview__title">"总资产"</h3>
                </div>

                <div class="stock-overview__lead">
                    {move || {
                        if overview_loading.get() {
                            view! { <span class="stock-skeleton stock-skeleton--lead"></span> }
                                .into_any()
                        } else {
                            view! {
                                <span class="stock-amount stock-amount--large">
                                    {format!("¥{}", format::amount(overview.get().total_assets))}
                                </span>
                            }
                                .into_any()
                        }
                    }}
                </div>

                <div class="stock-overview__stats">
                    {move || {
                        let data = overview.get();
                        let loading = overview_loading.get();
                        let cards: Vec<(&str, String, &str, &str)> = vec![
                            (
                                "本金",
                                format!("¥{}", format::amount(data.principal)),
                                "",
                                "",
                            ),
                            (
                                "持仓市值",
                                format!("¥{}", format::amount(data.position_market_value)),
                                "",
                                "持仓市值 = Σ（最新价 × 股数）；行情获取失败的持仓按持仓成本计入",
                            ),
                            (
                                "浮动盈亏",
                                format::signed_yuan(data.unrealized_pnl),
                                format::pnl_class(data.unrealized_pnl),
                                "浮动盈亏 = Σ（最新价 × 股数 − 持仓总成本（含买入手续费））；未卖出持仓的账面盈亏",
                            ),
                            (
                                "已实现盈亏",
                                format::signed_yuan(data.realized_pnl),
                                format::pnl_class(data.realized_pnl),
                                "已实现盈亏为卖出净盈亏合计；不含持仓浮动盈亏",
                            ),
                            (
                                "累计支取",
                                format!("¥{}", format::amount(data.withdrawn_total)),
                                "",
                                "从股票账户支取出的累计金额，支取会相应减少总资产",
                            ),
                            (
                                "利息归本",
                                format!("¥{}", format::amount(data.interest_total)),
                                "",
                                "「利息归本」的累计金额；利息计入可用现金，本金口径不变",
                            ),
                            (
                                "可用现金",
                                format!("¥{}", format::amount(data.available_cash)),
                                "",
                                "可用现金为账户实际现金余额 = 本金 + 累计利息归本 + 已实现盈亏 − 累计支取 − 持仓成本（等于总资产 − 持仓市值，行情缺失部分按成本计入）",
                            ),
                        ];
                        cards
                            .into_iter()
                            .map(|(label, value, class, tip)| {
                                view! {
                                    <div class="stock-stat" title=tip>
                                        <span class="stock-stat__label">{label}</span>
                                        {if loading {
                                            view! {
                                                <span class="stock-skeleton stock-skeleton--md"></span>
                                            }
                                                .into_any()
                                        } else {
                                            view! {
                                                <span class=format!("stock-stat__value {class}")>
                                                    {value}
                                                </span>
                                            }
                                                .into_any()
                                        }}
                                    </div>
                                }
                            })
                            .collect_view()
                    }}
                </div>

                <Show when=move || overview.get().quote_failed_count.is_positive()>
                    <div class="stock-overview__notice">
                        <span>
                            {move || {
                                format!(
                                    "{} 只持仓未获取到行情，已按持仓成本计入",
                                    overview.get().quote_failed_count,
                                )
                            }}
                        </span>
                        <button
                            type="button"
                            class="ui-btn ui-btn--link ui-btn--sm"
                            on:click=move |_| load_overview()
                        >
                            "重试"
                        </button>
                    </div>
                </Show>

                <Show when=move || !overview_loading.get() && overview.get().principal == 0>
                    <div class="stock-overview__hint">
                        "还没有资金记录 — 先「追加本金」开始"
                    </div>
                </Show>
            </div>

            <div class="stock-account__grid">
                <div class="stock-panel stock-panel--records">
                    <div class="stock-panel__head">
                        <h4 class="stock-panel__title">"资金变化记录"</h4>
                    </div>
                    <div class="stock-table-wrap" class:stock-table-wrap--empty=funds_empty>
                        <table class="stock-table">
                            <thead>
                                <tr>
                                    <th class="is-center" style="width: 150px;">"日期"</th>
                                    <th style="min-width: 180px;">"事件"</th>
                                    <th class="is-right" style="width: 130px;">"金额变化"</th>
                                    <th class="is-right" style="width: 130px;">"现金余额"</th>
                                    <th class="is-right" style="min-width: 180px;">"备注"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {move || {
                                    let items = fund_page.get().items;
                                    if items.is_empty() {
                                        return view! {
                                            <tr>
                                                <td colspan="5">
                                                    <Empty
                                                        title="暂无资金记录"
                                                        description="追加本金或买入/卖出后，每一笔资金变动都会显示在这里"
                                                    />
                                                </td>
                                            </tr>
                                        }
                                            .into_any();
                                    }
                                    items
                                        .into_iter()
                                        .map(|record| {
                                            fund_row(record)
                                        })
                                        .collect_view()
                                        .into_any()
                                }}
                            </tbody>
                        </table>
                    </div>
                    <div class="stock-panel__footer">
                    <span class="stock-panel__total">
                    {move || format!("共 {} 条", fund_page.get().total)}
                    </span>
                    <Pagination
                    page=fund_page_signal
                    total_pages=total_pages
                    disabled=Signal::derive(move || records_loading.get())
                    />
                    </div>
                </div>
            </div>

            {amount_modal(
                withdraw_open,
                FundAction::Withdraw,
                amount_text,
                amount_date,
                mutating,
                UnsyncCallback::new(move |()| submit_amount(FundAction::Withdraw)),
            )}
            {amount_modal(
                interest_open,
                FundAction::Interest,
                amount_text,
                amount_date,
                mutating,
                UnsyncCallback::new(move |()| submit_amount(FundAction::Interest)),
            )}
            {amount_modal(
                principal_open,
                FundAction::Principal,
                amount_text,
                amount_date,
                mutating,
                UnsyncCallback::new(move |()| submit_amount(FundAction::Principal)),
            )}
        </div>
        </div>
    }
    .into_any();

    view! {
        <FeaturePage
            title=PAGE_TITLE
            class="stock-page"
            rail=view! { <StockSubRail sub=sub /> }.into_any()
            toolbar=toolbar
            content=content
        />
    }
    .into_any()
}

/// 金额语义化文本（资金记录的一行）。
fn fund_row(record: StockFundRecordDto) -> AnyView {
    view! {
        <tr>
            <td class="is-center">{record.record_date.clone()}</td>
            <td>
                <span class="stock-ellipsis" title=record.event_text.clone()>
                    {record.event_text.clone()}
                </span>
            </td>
            <td class="is-right">
                <span class=format!("stock-amount {}", format::pnl_class(record.amount_change))>
                    {format::signed_yuan(record.amount_change)}
                </span>
            </td>
            <td class="is-right">
                <span class="stock-amount">
                    {format!("¥{}", format::amount(record.cash_balance))}
                </span>
            </td>
            <td class="is-right">
                <span
                    class="stock-ellipsis"
                    title=if record.remark.is_empty() {
                        "-".to_string()
                    } else {
                        record.remark.clone()
                    }
                >
                    {if record.remark.is_empty() {
                        "-".to_string()
                    } else {
                        record.remark.clone()
                    }}
                </span>
            </td>
        </tr>
    }
    .into_any()
}

/// 资金操作弹窗（追加本金 / 利息归本 / 支取共用同一套字段，只有文案不同）。
fn amount_modal(
    open: RwSignal<bool>,
    action: FundAction,
    amount: RwSignal<String>,
    date: RwSignal<String>,
    mutating: RwSignal<bool>,
    on_ok: UnsyncCallback<()>,
) -> AnyView {
    let title = action.label();
    view! {
        <Modal
            open=Signal::derive(move || open.get())
            title=title
            size=ModalSize::Small
            ok_text=action.ok_text()
            cancel_text="取消"
            ok_loading=Signal::derive(move || mutating.get())
            on_close=move || open.set(false)
            on_ok=move || on_ok.run(())
        >
            <div class="modal-form-item">
                <p class="modal-form-label">{action.amount_label()}</p>
                <Input value=amount placeholder="请输入金额" />
            </div>
            <div class="modal-form-item">
                <p class="modal-form-label">"发生日期"</p>
                <DatePicker value=date />
            </div>
        </Modal>
    }
    .into_any()
}

// ==================================================================== 子功能二：持仓

/// 持仓子功能：持仓卡片 + 行情面板 + 本轮复盘 + 成交表 + 编辑/删除。
///
/// 「建仓」是本子功能的主操作，放在工具栏里（空态文案里的"先点下方「建仓」"已随之改为工具栏口径）。
fn position_view(sub: RwSignal<StockSub>) -> AnyView {
    let stores = AppStores::global();
    let positions = RwSignal::new(Vec::<StockPositionDto>::new());
    let positions_loading = RwSignal::new(false);
    let selected_code = RwSignal::new(String::new());
    let trades = RwSignal::new(Vec::<StockTradeDto>::new());
    let trades_loading = RwSignal::new(false);
    let fee_settings = RwSignal::new(Option::<StockFeeSetting>::None);
    let tags = RwSignal::new(Vec::<String>::new());
    let default_tag = RwSignal::new("分析".to_string());

    // 复盘编辑态
    let review_open = RwSignal::new(true);
    let review_editing = RwSignal::new(false);
    let review_draft = RwSignal::new(String::new());
    let review_saving = RwSignal::new(false);

    // 下单弹窗
    let trade_open = RwSignal::new(false);
    let trade_type = RwSignal::new("open".to_string());
    let trade_code = RwSignal::new(String::new());
    let trade_name = RwSignal::new(String::new());
    let trade_rows = FillState::new();
    let trade_date = RwSignal::new(today_ymd());
    let trade_tag = RwSignal::new(String::new());
    let trade_mutating = RwSignal::new(false);
    // 名称是"按代码自动查来的"还是用户手打的：只有换过代码才允许被覆盖（见查询回调）
    let trade_queried_code = RwSignal::new(String::new());

    // 编辑 / 删除
    let edit_target = RwSignal::new(Option::<StockTradeDto>::None);
    let edit_siblings = RwSignal::new(Vec::<StockTradeDto>::new());
    let edit_price = RwSignal::new(String::new());
    let edit_lots = RwSignal::new(String::new());
    let edit_date = RwSignal::new(today_ymd());
    let edit_saving = RwSignal::new(false);
    let impact = RwSignal::new(Option::<ImpactPrompt>::None);
    let impact_running = RwSignal::new(false);

    // 折叠展开的委托
    let collapsed_orders = RwSignal::new(BTreeSet::<String>::new());
    // 编辑成交（无失效轮次时直接写库）后的重取计数
    let reload_trades = RwSignal::new(0_u32);

    let current_position = move || {
        let code = selected_code.get();
        positions
            .get()
            .into_iter()
            .find(|position| position.stock_code == code)
    };

    let load_fee_settings = move || {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            return;
        }
        let ledger_for_tags = ledger_id.clone();
        leptos::task::spawn_local(async move {
            if let Ok(data) = api::stock::fee_settings_get(&ledger_id).await {
                fee_settings.set(Some(data));
            }
        });
        leptos::task::spawn_local(async move {
            match api::stock::tag_settings_get(&ledger_for_tags).await {
                Ok(data) => {
                    default_tag.set(if data.default_tag.is_empty() {
                        "分析".to_string()
                    } else {
                        data.default_tag.clone()
                    });
                    tags.set(data.tags);
                }
                Err(error) => notify_error("查询交易标签失败", &error),
            }
        });
    };

    let load_trades = move |code: String| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() || code.is_empty() {
            trades.set(Vec::new());
            return;
        }
        trades_loading.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::trades(&ledger_id, &code).await {
                Ok(items) => trades.set(items),
                Err(error) => {
                    trades.set(Vec::new());
                    notify_error("查询成交记录失败", &error);
                }
            }
            trades_loading.set(false);
        });
    };

    let load_positions = move |prefer: Option<String>| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            positions.set(Vec::new());
            return;
        }
        positions_loading.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::positions(&ledger_id).await {
                Ok(items) => {
                    let current = selected_code.get_untracked();
                    let still_exists = items.iter().any(|item| item.stock_code == current);
                    positions.set(items.clone());
                    if !still_exists {
                        let next = prefer
                            .filter(|code| items.iter().any(|item| &item.stock_code == code))
                            .or_else(|| items.first().map(|item| item.stock_code.clone()))
                            .unwrap_or_default();
                        selected_code.set(next.clone());
                        if next.is_empty() {
                            trades.set(Vec::new());
                        } else {
                            load_trades(next);
                        }
                    }
                }
                Err(error) => {
                    positions.set(Vec::new());
                    notify_error("查询持仓失败", &error);
                }
            }
            positions_loading.set(false);
        });
    };

    // 账本切换 → 重置选中并全量重载
    Effect::new(move |prev: Option<String>| {
        let ledger_id = stores.current_ledger_id.get();
        if prev.as_deref() == Some(ledger_id.as_str()) {
            return ledger_id;
        }
        selected_code.set(String::new());
        trades.set(Vec::new());
        if ledger_id.is_empty() {
            positions.set(Vec::new());
            return ledger_id;
        }
        load_positions(None);
        load_fee_settings();
        ledger_id
    });

    // 切回本子功能 → 刷新行情（重取持仓 + 账户总览）
    Effect::new(move |prev: Option<bool>| {
        let is_active = sub.get() == StockSub::Position;
        if is_active && prev == Some(false) && !stores.current_ledger_id.get_untracked().is_empty()
        {
            load_positions(Some(selected_code.get_untracked()));
        }
        is_active
    });

    // 选中变化 → 刷新该股行情（原先靠工具栏的「刷新行情」按钮；现在点卡片即刷新）。
    // 守卫写法与本文件其他 Effect 一致：值没变就直接返回，避免重复请求。
    Effect::new(move |prev: Option<String>| {
        let code = selected_code.get();
        if prev.as_deref() == Some(code.as_str()) {
            return code;
        }
        if !code.is_empty() {
            load_positions(Some(code.clone()));
        }
        code
    });
    // 编辑成交后重取该股成交（写入计数即触发）
    Effect::new(move |prev: Option<u32>| {
        let revision = reload_trades.get();
        if prev == Some(revision) {
            return revision;
        }
        let code = selected_code.get_untracked();
        if !code.is_empty() {
            load_trades(code);
        }
        revision
    });

    // 选中股票变化 → 重置复盘与成交表
    Effect::new(move |prev: Option<String>| {
        let code = selected_code.get();
        if prev.as_deref() == Some(code.as_str()) {
            return code;
        }
        review_editing.set(false);
        review_open.set(true);
        review_draft.set(String::new());
        if !code.is_empty() {
            load_trades(code.clone());
        }
        code
    });

    // ---- 本轮复盘保存 ----
    let save_review = move || {
        let ledger_id = stores.current_ledger_id.get_untracked();
        let code = selected_code.get_untracked();
        if ledger_id.is_empty() || code.is_empty() {
            return;
        }
        let review = review_draft.get_untracked();
        review_saving.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::position_review(&ledger_id, &code, &review).await {
                Ok(updated) => {
                    positions.update(|items| {
                        if let Some(existing) = items
                            .iter_mut()
                            .find(|item| item.stock_code == updated.stock_code)
                        {
                            existing.review = updated.review.clone();
                        }
                    });
                    review_editing.set(false);
                    Notifier::global().success("本轮复盘已保存".to_string(), None);
                }
                Err(error) => notify_error("保存本轮复盘失败", &error),
            }
            review_saving.set(false);
        });
    };

    // ---- 下单 ----
    // 把当前持仓写进模块级快照，供 `trade_modal`（自由函数）读取可用手数
    Effect::new(move |_| {
        let snapshot = positions.get();
        POSITION_SNAPSHOT.with(|slot| {
            *slot.borrow_mut() = Some(snapshot);
        });
    });

    let available_lots = move |code: &str| -> i64 {
        positions
            .get()
            .into_iter()
            .find(|position| position.stock_code == code)
            .map(|position| lots_of(position.quantity))
            .unwrap_or(0)
    };

    let open_trade = move |next_type: &str| {
        let code = selected_code.get_untracked();
        let position = current_position();
        let lots_label_value = available_lots(&code);
        trade_type.set(next_type.to_string());
        trade_code.set(if next_type == "open" {
            String::new()
        } else {
            code.clone()
        });
        // **建仓时名称也必须是空的**：它以前无条件取"当前选中那条持仓"的名字，
        // 于是点「建仓」会带着上一只票的名字（用户报的"不该有山子高科四个字"）。
        // 减仓 / 清仓是就着某条持仓下单，名称代码本来就要预填 —— 只有 open 清空。
        trade_name.set(if next_type == "open" {
            String::new()
        } else {
            position
                .as_ref()
                .map(|item| item.stock_name.clone())
                .unwrap_or_default()
        });
        trade_queried_code.set(String::new());
        // 清仓时预填全仓手数
        trade_rows.reset(vec![(
            String::new(),
            if next_type == "close" && lots_label_value > 0 {
                lots_label_value.to_string()
            } else {
                String::new()
            },
        )]);
        trade_date.set(today_ymd());
        trade_tag.set(default_tag.get_untracked());
        trade_open.set(true);
    };

    let submit_trade = move || {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            return;
        }
        let code = trade_code.get_untracked().trim().to_string();
        let name = trade_name.get_untracked().trim().to_string();
        let mut submit_type = trade_type.get_untracked();
        if name.is_empty() {
            Notifier::global().error("请输入股票名称".to_string(), None);
            return;
        }
        if !is_valid_stock_code(&code) {
            Notifier::global().error(
                "请输入有效的沪深股票代码（沪 60/68、深 00/30 开头）".to_string(),
                None,
            );
            return;
        }
        let raw_count = trade_rows.rows_text.get_untracked().len();
        let fills = trade_rows.parsed();
        if fills.len() != raw_count || fills.is_empty() {
            Notifier::global().error("请填写完整的成交价与手数".to_string(), None);
            return;
        }
        let total_lots: i64 = fills.iter().map(|fill| fill.lots).sum();
        let available = available_lots(&code);
        if submit_type == "reduce" && total_lots > available {
            Notifier::global().error(format!("减仓手数不能超过可用手数（{available} 手）"), None);
            return;
        }
        // 减仓正好等于可用手数 → 视为清仓
        if submit_type == "reduce" && available > 0 && total_lots == available {
            submit_type = "close".to_string();
        }
        let tag = if submit_type == "close" {
            trade_tag.get_untracked()
        } else {
            String::new()
        };
        let trade_time = crate::time::ymd_to_seconds(&trade_date.get_untracked())
            .unwrap_or_else(crate::time::now_seconds);
        trade_mutating.set(true);
        let submit_type_for_async = submit_type.clone();
        leptos::task::spawn_local(async move {
            let inputs = fills
                .iter()
                .map(|fill| api::stock::TradeFillRequest::new(fill.price, fill.lots))
                .collect::<Vec<_>>();
            match api::stock::trade_create(
                &ledger_id,
                &code,
                &name,
                &submit_type_for_async,
                trade_time,
                &tag,
                inputs,
            )
            .await
            {
                Ok(_) => {
                    Notifier::global().success("委托已记录".to_string(), None);
                    trade_open.set(false);
                    load_positions(Some(code.clone()));
                }
                Err(error) => notify_error("记录委托失败", &error),
            }
            trade_mutating.set(false);
        });
    };

    // ---- 编辑成交（含影响预演）----
    let open_edit = move |(trade, siblings): (StockTradeDto, Vec<StockTradeDto>)| {
        edit_price.set(format::amount(trade.price));
        edit_lots.set(trade.lots.to_string());
        edit_date.set(format_timestamp(trade.trade_time, "YYYY-MM-DD"));
        edit_target.set(Some(trade));
        edit_siblings.set(siblings);
    };

    let submit_edit = move || {
        let ledger_id = stores.current_ledger_id.get_untracked();
        let Some(trade) = edit_target.get_untracked() else {
            return;
        };
        let price: f64 = match edit_price.get_untracked().trim().parse() {
            Ok(value) => value,
            Err(_) => {
                Notifier::global().error("请输入有效的成交价".to_string(), None);
                return;
            }
        };
        if price.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            Notifier::global().error("请输入有效的成交价".to_string(), None);
            return;
        }
        let lots: i64 = match edit_lots.get_untracked().trim().parse() {
            Ok(value) => value,
            Err(_) => {
                Notifier::global().error("请输入有效的手数".to_string(), None);
                return;
            }
        };
        if lots <= 0 {
            Notifier::global().error("请输入有效的手数".to_string(), None);
            return;
        }
        let trade_time =
            crate::time::ymd_to_seconds(&edit_date.get_untracked()).unwrap_or(trade.trade_time);
        edit_saving.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::trade_impact(
                &ledger_id,
                "update_trade",
                &trade.id,
                "",
                price,
                lots,
                trade_time,
            )
            .await
            {
                Ok(preview) => {
                    if preview.removed_rounds.is_empty() {
                        // 无失效轮次 → 直接写库
                        match api::stock::trade_update(
                            &ledger_id, &trade.id, price, lots, trade_time,
                        )
                        .await
                        {
                            Ok(_) => {
                                Notifier::global().success("成交已更新".to_string(), None);
                                edit_target.set(None);
                                reload_trades.update(|value| *value += 1);
                            }
                            Err(error) => notify_error("保存成交失败", &error),
                        }
                        edit_saving.set(false);
                    } else {
                        impact.set(Some(ImpactPrompt {
                            action: "update_trade".to_string(),
                            title: "确认修改这笔成交？".to_string(),
                            summary: impact_summary(&preview, &trade, lots, None),
                            trade: Some(StockTradeDto {
                                price: (price * 100.0).round() as i64,
                                lots,
                                trade_time,
                                ..trade.clone()
                            }),
                            impact: preview,
                        }));
                        edit_saving.set(false);
                    }
                }
                Err(error) => {
                    notify_error("预演交易影响失败", &error);
                    edit_saving.set(false);
                }
            }
        });
    };

    let confirm_impact = move || {
        let ledger_id = stores.current_ledger_id.get_untracked();
        let Some(prompt) = impact.get_untracked() else {
            return;
        };
        impact_running.set(true);
        match prompt.action.as_str() {
            "update_trade" => {
                let Some(trade) = prompt.trade else {
                    impact_running.set(false);
                    return;
                };
                let price = trade.price as f64 / 100.0;
                leptos::task::spawn_local(async move {
                    match api::stock::trade_update(
                        &ledger_id,
                        &trade.id,
                        price,
                        trade.lots,
                        trade.trade_time,
                    )
                    .await
                    {
                        Ok(_) => {
                            Notifier::global().success("成交已更新".to_string(), None);
                            impact.set(None);
                            edit_target.set(None);
                            load_positions(Some(selected_code.get_untracked()));
                        }
                        Err(error) => notify_error("保存成交失败", &error),
                    }
                    impact_running.set(false);
                });
            }
            _ => {
                let order_id = prompt
                    .trade
                    .as_ref()
                    .map(|trade| {
                        if trade.order_id.is_empty() {
                            trade.id.clone()
                        } else {
                            trade.order_id.clone()
                        }
                    })
                    .unwrap_or_default();
                leptos::task::spawn_local(async move {
                    match api::stock::trade_order_delete(&ledger_id, &order_id).await {
                        Ok(_) => {
                            Notifier::global().success("委托已删除".to_string(), None);
                            impact.set(None);
                            load_positions(Some(selected_code.get_untracked()));
                        }
                        Err(error) => notify_error("删除委托失败", &error),
                    }
                    impact_running.set(false);
                });
            }
        }
    };

    // 本子功能**没有页面级操作**：「建仓」属于左侧持仓列表（空态文案写着"先点下方「建仓」"，
    // 按钮就贴在列表底栏），所以不给 `FeaturePage` 的 `toolbar` —— 空工具栏只会白占一条发丝线。
    let content = view! {
        <div class="stock-body">
        <div class="stock-position">
            <div class="stock-position__grid">
                <div class="stock-panel stock-panel--list">
                    <Show
                        when=move || !positions.get().is_empty()
                        fallback=move || {
                            view! {
                                <div class="stock-empty">
                                    {move || {
                                        if positions_loading.get() {
                                            "正在加载持仓…"
                                        } else {
                                            "暂无持仓，先点下方「建仓」"
                                        }
                                    }}
                                </div>
                            }
                        }
                    >
                        <div class="stock-position__cards">
                            {move || {
                                positions
                                    .get()
                                    .into_iter()
                                    .map(|position| {
                                        let code = position.stock_code.clone();
                                        let is_active = code == selected_code.get();
                                        let has_quote = format::has_quote(position.latest_price);
                                        let day_change = match (position.latest_price, position.prev_close) {
                                            (Some(latest), Some(prev)) if prev > 0 => Some(latest - prev),
                                            _ => None,
                                        };
                                        let float_pnl = match position.latest_price {
                                            Some(latest) if latest > 0 => {
                                                Some(latest.saturating_mul(position.quantity).saturating_sub(position.total_cost))
                                            }
                                            _ => None,
                                        };
                                        let float_rate = float_pnl.and_then(|pnl| {
                                            if position.total_cost > 0 {
                                                Some(pnl as f64 / position.total_cost as f64 * 100.0)
                                            } else {
                                                None
                                            }
                                        });
                                        let day_rate = day_change.and_then(|change| {
                                            position.prev_close.and_then(|prev| {
                                                if prev > 0 {
                                                    Some(change as f64 / prev as f64 * 100.0)
                                                } else {
                                                    None
                                                }
                                            })
                                        });
                                        let class = format::pnl_class(day_change.unwrap_or(0));
                                        let float_class = format::pnl_class(float_pnl.unwrap_or(0));
                                        let code_for_click = code.clone();
                                        view! {
                                            <button
                                                type="button"
                                                class="stock-position-card"
                                                class:is-active=is_active
                                                on:click=move |_| selected_code.set(code_for_click.clone())
                                            >
                                                <div class="stock-position-card__head">
                                                    <span class="stock-position-card__name">
                                                        {position.stock_name.clone()}
                                                    </span>
                                                    <span class="stock-position-card__meta">
                                                        <span class="stock-mono">
                                                            {position.stock_code.clone()}
                                                        </span>
                                                        <span>
                                                            {format!("持仓 {}手", lots_of(position.quantity))}
                                                        </span>
                                                    </span>
                                                </div>
                                                {if has_quote {
                                                    view! {
                                                        <div class="stock-position-card__row">
                                                            <span class="stock-position-card__label">"现价"</span>
                                                            <span class="stock-position-card__value">
                                                                {format!("¥{}", format::amount(position.latest_price.unwrap_or(0)))}
                                                            </span>
                                                            <span class=format!("stock-position-card__rate {class}")>
                                                                {format::optional_signed_percent(day_rate)}
                                                            </span>
                                                        </div>
                                                        <div class="stock-position-card__row">
                                                            <span class="stock-position-card__label">"浮动盈亏"</span>
                                                            <span class=format!("stock-position-card__value {float_class}")>
                                                                {format::signed_yuan(float_pnl.unwrap_or(0))}
                                                            </span>
                                                            <span class=format!("stock-position-card__rate {float_class}")>
                                                                {format::optional_signed_percent(float_rate)}
                                                            </span>
                                                        </div>
                                                    }
                                                        .into_any()
                                                } else {
                                                    view! {
                                                        <div class="stock-position-card__row is-muted">
                                                            <span class="stock-position-card__label">"现价"</span>
                                                            <span class="stock-position-card__value">"—"</span>
                                                            <span class="stock-position-card__rate">"—"</span>
                                                        </div>

                                                        <div class="stock-position-card__row is-muted">
                                                            <span class="stock-position-card__label">"浮动盈亏"</span>
                                                            <span class="stock-position-card__value">"—"</span>
                                                            <span class="stock-position-card__rate">"—"</span>
                                                        </div>
                                                    }
                                                        .into_any()
                                                }}
                                            </button>
                                        }
                                    })
                                    .collect_view()
                            }}
                        </div>
                    </Show>
                    // 主操作「建仓」回到左栏底栏（`.stock-panel__footer` 自带顶部分隔线，
                    // 与「资金变化记录」的底栏同一套）—— 空态文案说的"下方"就是这里。
                    <div class="stock-panel__footer">
                        <Button
                            variant=ButtonVariant::Primary
                            block=true
                            on_click=move |_| open_trade("open")
                        >
                            "建仓"
                        </Button>
                    </div>
                </div>

                <div class="stock-panel stock-panel--detail">
                    {move || {
                        let Some(position) = current_position() else {
                            return view! {
                                <div class="stock-empty">"选择左侧持仓查看详情"</div>
                            }
                                .into_any();
                        };
                        let has_quote = format::has_quote(position.latest_price);
                        let day_change = match (position.latest_price, position.prev_close) {
                            (Some(latest), Some(prev)) if prev > 0 => Some(latest - prev),
                            _ => None,
                        };
                        let day_rate = day_change.and_then(|change| {
                            position.prev_close.and_then(|prev| {
                                if prev > 0 {
                                    Some(change as f64 / prev as f64 * 100.0)
                                } else {
                                    None
                                }
                            })
                        });
                        let market_value = match position.latest_price {
                            Some(latest) if latest > 0 => {
                                latest.saturating_mul(position.quantity)
                            }
                            _ => position.total_cost,
                        };
                        let float_pnl = match position.latest_price {
                            Some(latest) if latest > 0 => Some(
                                latest
                                    .saturating_mul(position.quantity)
                                    .saturating_sub(position.total_cost),
                            ),
                            _ => None,
                        };
                        let float_rate = float_pnl.and_then(|pnl| {
                            if position.total_cost > 0 {
                                Some(pnl as f64 / position.total_cost as f64 * 100.0)
                            } else {
                                None
                            }
                        });
                        let day_class = format::pnl_class(day_change.unwrap_or(0));
                        let float_class = format::pnl_class(float_pnl.unwrap_or(0));
                        view! {
                            <div class="stock-detail__head">
                                <div class="stock-detail__identity">
                                    <span class="stock-detail__name">{position.stock_name.clone()}</span>
                                    <span class="stock-mono stock-detail__code">
                                        {position.stock_code.clone()}
                                    </span>
                                </div>
                                <div class="stock-detail__actions">
                                    <Button
                                        variant=ButtonVariant::PrimaryDanger
                                        size=ButtonSize::Small
                                        // **不要在这里 `selected_code.set(code)`**：详情区就是按
                                        // `current_position()`（= `selected_code` 命中的那条）渲染的，
                                        // 写的是同一个值；而 RwSignal 同值写入依然会通知订阅者，
                                        // 于是点击瞬间详情子树重渲染，交易弹窗反而**弹不出来**（实测）。
                                        on_click=move |_| open_trade("close")
                                    >
                                        "清仓"
                                    </Button>
                                    <Button
                                        variant=ButtonVariant::Secondary
                                        size=ButtonSize::Small
                                        // 同上：这里写 selected_code 会让「减仓」弹窗弹不出来
                                        on_click=move |_| open_trade("reduce")
                                    >
                                        "减仓"
                                    </Button>
                                    <Button
                                        variant=ButtonVariant::Secondary
                                        size=ButtonSize::Small
                                        // 同上
                                        on_click=move |_| open_trade("add")
                                    >
                                        "加仓"
                                    </Button>
                                </div>
                            </div>

                            {if has_quote {
                                view! {
                                    <div class="stock-quote">
                                        <div class="stock-quote__cell">
                                            <span class="stock-quote__label">"现价"</span>
                                            <span class="stock-quote__value">
                                                {format!("¥{}", format::amount(position.latest_price.unwrap_or(0)))}
                                            </span>
                                        </div>
                                        <div class="stock-quote__cell">
                                            <span class="stock-quote__label">"当日涨跌"</span>
                                            <span class=format!("stock-quote__value {day_class}")>
                                                {format!("{} {}", format::signed_yuan(day_change.unwrap_or(0)), format::optional_signed_percent(day_rate))}
                                            </span>
                                        </div>
                                        <div class="stock-quote__cell">
                                            <span class="stock-quote__label">"持仓市值"</span>
                                            <span class="stock-quote__value">
                                                {format!("¥{}", format::amount(market_value))}
                                            </span>
                                        </div>
                                        <div class="stock-quote__cell">
                                            <span class="stock-quote__label">"浮动盈亏"</span>
                                            <span class=format!("stock-quote__value {float_class}")>
                                                {format!("{} {}", format::signed_yuan(float_pnl.unwrap_or(0)), format::optional_signed_percent(float_rate))}
                                            </span>
                                        </div>
                                    </div>
                                }
                                    .into_any()
                            } else {
                                view! {
                                    <div class="stock-quote stock-quote--missing">
                                        "暂未获取到行情"
                                    </div>
                                }
                                    .into_any()
                            }}

                            <div class="stock-review">
                                <div class="stock-review__head">
                                    <button
                                        type="button"
                                        class="stock-review__toggle"
                                        aria-expanded=move || review_open.get()
                                        on:click=move |_| review_open.update(|value| *value = !*value)
                                    >
                                        <span
                                            class="diary-tree__caret"
                                            class:is-open=move || review_open.get()
                                        >
                                            {icons::icon(Icon::CaretRight)}
                                        </span>
                                        <span class="stock-review__title">"本轮复盘"</span>
                                        <Show when=move || {
                                            !review_open.get()
                                                && current_position()
                                                    .map(|item| !item.review.is_empty())
                                                    .unwrap_or(false)
                                        }>
                                            <span class="stock-review__summary">
                                                {current_position()
                                                    .map(|item| item.review)
                                                    .unwrap_or_default()}
                                            </span>
                                        </Show>
                                    </button>
                                    <Show when=move || !review_editing.get()>
                                        <button
                                            type="button"
                                            class="ui-btn ui-btn--text ui-btn--sm"
                                            on:click=move |_| {
                                                let current = current_position()
                                                    .map(|item| item.review.clone())
                                                    .unwrap_or_default();
                                                review_draft.set(if current.is_empty() {
                                                    ROUND_REVIEW_TEMPLATE.to_string()
                                                } else {
                                                    current
                                                });
                                                review_open.set(true);
                                                review_editing.set(true);
                                            }
                                        >
                                            <span class="ui-btn__icon">{icons::icon(Icon::Edit)}</span>
                                            {if position.review.is_empty() { "写复盘" } else { "编辑" }}
                                        </button>
                                    </Show>
                                </div>
                                <Show when=move || review_open.get()>
                                    <Show
                                        when=move || review_editing.get()
                                        fallback=move || {
                                            view! {
                                                {
                                                    let review_text = current_position()
                                                        .map(|item| item.review)
                                                        .unwrap_or_default();
                                                    if review_text.is_empty() {
                                                        view! {
                                                            <p class="stock-review__hint">
                                                                "还没有写本轮复盘，可随时记录建仓理由与操作计划。"
                                                            </p>
                                                        }
                                                            .into_any()
                                                    } else {
                                                        view! {
                                                            <p class="stock-review__text">
                                                                {review_text.clone()}
                                                            </p>
                                                        }
                                                            .into_any()
                                                    }
                                                }
                                            }
                                        }
                                    >
                                        {review_editor(
                                            review_draft,
                                            review_saving,
                                            UnsyncCallback::new(move |()| {
                                                review_editing.set(false);
                                                review_draft.set(String::new());
                                            }),
                                            UnsyncCallback::new(move |()| save_review()),
                                        )}
                                    </Show>
                                </Show>
                            </div>

                            <div class="stock-trades">
                                <div class="stock-panel__head">
                                    <h4 class="stock-panel__title">"成交记录"</h4>
                                    <Show when=move || trades_loading.get()>
                                        <span class="stock-loading-hint">"加载中…"</span>
                                    </Show>
                                </div>
                                <div class="stock-table-wrap">
                                    <table class="stock-table stock-table--trades">
                                        <thead>
                                            <tr>
                                                <th class="is-center" style="width: 150px;">"时间"</th>
                                                <th class="is-center" style="width: 90px;">"类型"</th>
                                                <th class="is-right" style="width: 110px;">"成交价"</th>
                                                <th class="is-center" style="width: 90px;">"手数"</th>
                                                <th class="is-right" style="width: 120px;">"成交金额"</th>
                                                <th style="min-width: 220px;">"费用"</th>
                                                <th class="is-right" style="width: 120px;">"资金变动"</th>
                                                <th class="is-center" style="width: 110px;">"操作"</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            {move || {
                                                trade_table_rows(
                                                    trades,
                                                    collapsed_orders,
                                                    UnsyncCallback::new(move |(trade, siblings): (
                                                        StockTradeDto,
                                                        Vec<StockTradeDto>,
                                                    )| open_edit((trade, siblings))),
                                                    UnsyncCallback::new(move |trades: Vec<StockTradeDto>| {
                                                        start_delete_order(&stores, trades, impact);
                                                    }),
                                                )
                                            }}
                                        </tbody>
                                    </table>
                                </div>
                            </div>
                        }
                            .into_any()
                    }}
                </div>
            </div>

            {trade_modal(
                trade_open,
                trade_type,
                trade_code,
                trade_name,
                trade_rows,
                trade_date,
                trade_tag,
                tags,
                default_tag,
                fee_settings,
                trade_mutating,
                UnsyncCallback::new(move |code: String| {
                    let ledger_id = stores.current_ledger_id.get_untracked();
                    if code.is_empty() || ledger_id.is_empty() {
                        return;
                    }
                    // 已经为这个代码查过一次、且用户没再改代码 → 不再覆盖（手改过的名称要留住）
                    let changed = trade_queried_code.get_untracked() != code;
                    trade_queried_code.set(code.clone());
                    leptos::task::spawn_local(async move {
                        // 静默失败（查不到名称就保持为空）
                        if let Ok(data) = api::stock::stock_name(&code).await {
                            if !data.stock_name.is_empty()
                                && (trade_name.get_untracked().trim().is_empty() || changed)
                            {
                                trade_name.set(data.stock_name);
                            }
                        }
                    });
                }),
                UnsyncCallback::new(move |()| submit_trade()),
            )}

            {edit_modal(
                edit_target,
                edit_siblings,
                edit_price,
                edit_lots,
                edit_date,
                edit_saving,
                UnsyncCallback::new(move |()| submit_edit()),
            )}

            {impact_modal(impact, impact_running, UnsyncCallback::new(move |()| confirm_impact()))}
        </div>
        </div>
    }
    .into_any();
    view! {
        <FeaturePage
            title=PAGE_TITLE
            class="stock-page"
            rail=view! { <StockSubRail sub=sub /> }.into_any()
            content=content
        />
    }
    .into_any()
}

/// 本轮复盘编辑块：持仓区与历史轮次区各一处，结构逐行相同，只有「取消 / 保存」不同。
///
/// 刻意**不**统一两处的「取消」写法：持仓区是 `review_editing.set(false)`（`RwSignal<bool>`），
/// 轮次区是 `review_editing.set(String::new())`（`RwSignal<String>` 存的是轮次 id），语义不同。
fn review_editor(
    draft: RwSignal<String>,
    saving: RwSignal<bool>,
    on_cancel: UnsyncCallback<()>,
    on_save: UnsyncCallback<()>,
) -> AnyView {
    view! {
        <Textarea
            value=draft
            rows=3
            maxlength=500
            placeholder=REVIEW_PLACEHOLDER
            class="stock-review__textarea"
        />
        <div class="stock-review__actions">
            <Button
                variant=ButtonVariant::Secondary
                size=ButtonSize::Small
                disabled=Signal::derive(move || saving.get())
                on_click=move |_| on_cancel.run(())
            >
                "取消"
            </Button>
            <Button
                variant=ButtonVariant::Primary
                size=ButtonSize::Small
                loading=Signal::derive(move || saving.get())
                on_click=move |_| on_save.run(())
            >
                "保存"
            </Button>
        </div>
    }
    .into_any()
}

/// 发起「删除整笔委托」的影响预演。
fn start_delete_order(
    stores: &AppStores,
    order_trades: Vec<StockTradeDto>,
    impact: RwSignal<Option<ImpactPrompt>>,
) {
    let ledger_id = stores.current_ledger_id.get_untracked();
    if ledger_id.is_empty() || order_trades.is_empty() {
        return;
    }
    let first = order_trades.first().cloned().unwrap_or_default();
    let order_id = if first.order_id.is_empty() {
        first.id.clone()
    } else {
        first.order_id.clone()
    };
    let lots: i64 = order_trades.iter().map(|trade| trade.lots).sum();
    let amount: i64 = order_trades.iter().map(|trade| trade.amount).sum();
    let count = order_trades.len();
    leptos::task::spawn_local(async move {
        match api::stock::trade_impact(&ledger_id, "delete_order", "", &order_id, 0.0, 0, 0).await {
            Ok(preview) => {
                impact.set(Some(ImpactPrompt {
                    action: "delete_order".to_string(),
                    title: "删除整笔委托？".to_string(),
                    summary: format!(
                        "{} {} {}手 · 成交金额 ¥{} · 共 {} 笔成交\n删除后该股持仓、资金记录、轮次与统计都会重算。\n{}",
                        if preview.stock_name.is_empty() {
                            first.stock_name.clone()
                        } else {
                            preview.stock_name.clone()
                        },
                        format::trade_type_label(&first.trade_type),
                        lots,
                        format::amount(amount),
                        count,
                        removed_rounds_text(&preview.removed_rounds),
                    ),
                    // 删除时只需要 orderId
                    trade: Some(StockTradeDto {
                        id: String::new(),
                        order_id: order_id.clone(),
                        ..first.clone()
                    }),
                    impact: preview,
                }));
            }
            Err(error) => notify_error("预演交易影响失败", &error),
        }
    });
}

/// 「失效轮次」段落固定文案（改动即影响界面）。
///
/// 两个调用点共用同一份文案：编辑/删除成交的影响预演（[`impact_summary`]）与
/// 回滚确认框（回滚清仓/减仓会让轮次不再成立，该轮的复盘与标签随之丢失）。
fn removed_rounds_text(removed_rounds: &[StockTradeImpactRoundDto]) -> String {
    if removed_rounds.is_empty() {
        return "不会影响任何一轮的复盘。".to_string();
    }
    let numbers = removed_rounds
        .iter()
        .map(|round| round.round_no.to_string())
        .collect::<Vec<_>>()
        .join("、");
    let loses_review = removed_rounds.iter().any(|round| round.has_review);
    if loses_review {
        format!("第 {numbers} 轮不再成立，该轮复盘会一并丢失。")
    } else {
        format!("第 {numbers} 轮不再成立。")
    }
}

/// 编辑成交时的确认弹窗摘要。
fn impact_summary(
    impact: &StockTradeImpactDto,
    _trade: &StockTradeDto,
    _lots: i64,
    _unused: Option<()>,
) -> String {
    let position = format!(
        "{} 变动后持仓 {} 手",
        impact.stock_name,
        lots_of(impact.position_after)
    );
    format!(
        "{position}\n持仓、资金记录、轮次与统计都会按新数据重算。\n{}",
        removed_rounds_text(&impact.removed_rounds)
    )
}

/// 影响预演确认弹窗。
fn impact_modal(
    impact: RwSignal<Option<ImpactPrompt>>,
    running: RwSignal<bool>,
    on_ok: UnsyncCallback<()>,
) -> AnyView {
    view! {
        <Modal
            open=Signal::derive(move || impact.get().is_some())
            title=move || {
                impact
                    .get()
                    .map(|prompt| prompt.title.clone())
                    .unwrap_or_default()
            }
            size=ModalSize::Small
            ok_text="确认"
            cancel_text="取消"
            ok_danger=true
            ok_loading=Signal::derive(move || running.get())
            on_close=move || impact.set(None)
            on_ok=move || on_ok.run(())
        >
            <p class="stock-impact__text">
                {move || {
                    impact.get().map(|prompt| prompt.summary.clone()).unwrap_or_default()
                }}
            </p>
        </Modal>
    }
    .into_any()
}

/// 编辑成交弹窗（含「共 N 笔成交」明细与底部提示）。
fn edit_modal(
    target: RwSignal<Option<StockTradeDto>>,
    siblings: RwSignal<Vec<StockTradeDto>>,
    price: RwSignal<String>,
    lots: RwSignal<String>,
    date: RwSignal<String>,
    saving: RwSignal<bool>,
    on_ok: UnsyncCallback<()>,
) -> AnyView {
    view! {
        <Modal
            open=Signal::derive(move || target.get().is_some())
            title="编辑成交"
            size=ModalSize::Medium
            ok_text=move || if saving.get() { "保存中".to_string() } else { "保存".to_string() }
            cancel_text="取消"
            ok_loading=Signal::derive(move || saving.get())
            on_close=move || target.set(None)
            on_ok=move || on_ok.run(())
        >
            {move || {
                let Some(trade) = target.get() else {
                    return ().into_any();
                };
                let mut ordered = siblings.get();
                ordered.sort_by_key(|item| item.order_seq);
                view! {
                    <div class="stock-trade-edit">
                        <div class="stock-trade-edit__head">
                            <span class="stock-trade-edit__name">{trade.stock_name.clone()}</span>
                            <span class="stock-mono">{trade.stock_code.clone()}</span>
                            <span class=format!(
                                "stock-trade-type {}",
                                if format::is_buy(&trade.trade_type) { "type-buy" } else { "type-sell" },
                            )>{format::trade_type_label(&trade.trade_type)}</span>
                            <span class="stock-trade-edit__count">
                                {format!("共 {} 笔成交", ordered.len())}
                            </span>
                        </div>
                        <div class="stock-trade-edit__list">
                            {ordered
                                .into_iter()
                                .map(|item| {
                                    let is_current = item.id == trade.id;
                                    view! {
                                        <div
                                            class="stock-trade-edit__row"
                                            class:is-current=is_current
                                        >
                                            <span class="stock-trade-edit__tag">
                                                {if is_current {
                                                    "本笔".to_string()
                                                } else {
                                                    format!("第 {} 笔", item.order_seq)
                                                }}
                                            </span>
                                            <span class="stock-mono">
                                                {format!("¥{}", format::amount(item.price))}
                                            </span>
                                            <span>{format!("{}手", item.lots)}</span>
                                            <span class="stock-mono">
                                                {format!("¥{}", format::amount(item.amount))}
                                            </span>
                                        </div>
                                    }
                                })
                                .collect_view()}
                        </div>
                        <div class="modal-form-item">
                            <p class="modal-form-label">"成交价"</p>
                            <Input value=price placeholder="成交价（元/股）" />
                        </div>
                        <div class="modal-form-item">
                            <p class="modal-form-label">"手数"</p>
                            <Input value=lots placeholder="手数（手）" />
                        </div>
                        <div class="modal-form-item">
                            <p class="modal-form-label">"委托时间（同步到本委托全部成交）"</p>
                            <DatePicker value=date />
                        </div>
                        <p class="stock-trade-edit__hint">
                            "保存后按当前费用设置重算本笔委托的费用，并重新计算该股持仓、资金记录与轮次。"
                        </p>
                    </div>
                }
                    .into_any()
            }}
        </Modal>
    }
    .into_any()
}

thread_local! {
    /// 持仓快照（只读旁路）。
    ///
    /// `trade_modal` / `round_card` 这些自由函数拿不到 `position_view` 的闭包，
    /// 而「可用手数」与「持仓名称」又必须来自持仓列表；因此 `position_view`
    /// 每次渲染时把当前持仓写进这个槽位，供这些函数读取。
    static POSITION_SNAPSHOT: std::cell::RefCell<Option<Vec<StockPositionDto>>> =
        const { std::cell::RefCell::new(None) };
}

/// 下单弹窗里的「成交明细」编辑状态。
/// 为什么不把每行的输入框直接绑到 `rows_text`：那样"读 `rows_text` → 写 `rows_text`"
/// 会形成无限重渲染循环。这里让**每一行持有自己的 `RwSignal<String>`**，
/// 提交/汇总时用 [`FillState::parsed`] 读出当前快照。
#[derive(Clone, Copy)]
struct FillState {
    rows_text: RwSignal<Vec<(String, String)>>,
    price_signals: RwSignal<Vec<RwSignal<String>>>,
    lots_signals: RwSignal<Vec<RwSignal<String>>>,
}

impl FillState {
    fn new() -> Self {
        Self {
            rows_text: RwSignal::new(Vec::new()),
            price_signals: RwSignal::new(Vec::new()),
            lots_signals: RwSignal::new(Vec::new()),
        }
    }

    /// 重置为给定的初始行（`(价格, 手数)`，均为元/手的字符串）。
    fn reset(&self, rows: Vec<(String, String)>) {
        let mut prices = Vec::with_capacity(rows.len());
        let mut lots = Vec::with_capacity(rows.len());
        for (price, lot) in &rows {
            prices.push(RwSignal::new(price.clone()));
            lots.push(RwSignal::new(lot.clone()));
        }
        self.price_signals.set(prices);
        self.lots_signals.set(lots);
        self.rows_text.set(rows);
    }

    /// 追加一行空成交。
    fn push_empty(&self) {
        self.price_signals
            .update(|items| items.push(RwSignal::new(String::new())));
        self.lots_signals
            .update(|items| items.push(RwSignal::new(String::new())));
        self.rows_text
            .update(|items| items.push((String::new(), String::new())));
    }

    /// 删除一行（至少保留一行）。
    fn remove(&self, index: usize) {
        if self.rows_text.get_untracked().len() <= 1 {
            return;
        }
        self.price_signals.update(|items| {
            if index < items.len() {
                items.remove(index);
            }
        });
        self.lots_signals.update(|items| {
            if index < items.len() {
                items.remove(index);
            }
        });
        self.rows_text.update(|items| {
            if index < items.len() {
                items.remove(index);
            }
        });
    }

    /// 当前的有效成交快照（价格 / 手数都必须是正数）。
    fn parsed(&self) -> Vec<FillRow> {
        let prices = self.price_signals.get_untracked();
        let lots = self.lots_signals.get_untracked();
        prices
            .iter()
            .zip(lots.iter())
            .filter_map(|(price, lot)| {
                let price_value: f64 = price.get_untracked().trim().parse().ok()?;
                let lots_value: i64 = lot.get_untracked().trim().parse().ok()?;
                if price_value > 0.0 && lots_value > 0 {
                    Some(FillRow {
                        price: price_value,
                        lots: lots_value,
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

/// 下单弹窗（一笔委托可含多笔成交）。
#[allow(clippy::too_many_arguments)]
fn trade_modal(
    open: RwSignal<bool>,
    trade_type: RwSignal<String>,
    code: RwSignal<String>,
    name: RwSignal<String>,
    rows: FillState,
    date: RwSignal<String>,
    tag: RwSignal<String>,
    tags: RwSignal<Vec<String>>,
    default_tag: RwSignal<String>,
    fee_settings: RwSignal<Option<StockFeeSetting>>,
    mutating: RwSignal<bool>,
    on_code_blur: UnsyncCallback<String>,
    on_ok: UnsyncCallback<()>,
) -> AnyView {
    view! {
        <Modal
            open=Signal::derive(move || open.get())
            title=move || { let label = format::trade_type_label(&trade_type.get()); format!("委托{label}") }
            size=ModalSize::Medium
            ok_text=move || format::trade_type_label(&trade_type.get())
            cancel_text="取消"
            ok_loading=Signal::derive(move || mutating.get())
            on_close=move |_| open.set(false)
            on_ok=move || on_ok.run(())
        >
            <div class="stock-trade-form__grid">
                <div class="modal-form-item">
                    <p class="modal-form-label">"股票名称"</p>
                    <Input
                        value=name
                        disabled=Signal::derive(move || trade_type.get() != "open")
                    />
                </div>
                <div class="modal-form-item">
                    <p class="modal-form-label">"股票代码"</p>
                    // **失焦即查名**：填完代码离开这一格，名称自动填上（原来要点右侧的
                    // 「查询股票名称」按钮）。查不到就静默留空，不挡用户手填。
                    <Input
                        value=code
                        disabled=Signal::derive(move || trade_type.get() != "open")
                        on_blur=UnsyncCallback::new(move |()| {
                            on_code_blur.run(code.get_untracked())
                        })
                    />
                </div>
            </div>

            <div class="stock-trade-form__fills">
                <div class="stock-trade-form__fills-head">
                    <span class="modal-form-label">"成交明细"</span>
                    <button
                        type="button"
                        class="ui-btn ui-btn--link ui-btn--sm"
                        on:click=move |_| {
                            rows.push_empty();
                        }
                    >
                        "+ 添加一笔成交"
                    </button>
                </div>
                {move || {
                    let prices = rows.price_signals.get();
                    let lots = rows.lots_signals.get();
                    prices
                        .iter()
                        .zip(lots.iter())
                        .enumerate()
                        .map(|(index, (price_signal, lots_signal))| {
                            let price_signal = *price_signal;
                            let lots_signal = *lots_signal;
                            let lots_label = {
                                let current = trade_type.get();
                                // 可用手数只能从持仓推出来。`trade_modal` 是自由函数、
                                // 拿不到 `position_view` 的闭包，因此通过模块级快照读取
                                // （由 `position_view` 在每次渲染时写入）。
                                let code_text = code.get();
                                let available = POSITION_SNAPSHOT.with(|slot| {
                                    slot.borrow()
                                        .as_ref()
                                        .and_then(|positions| {
                                            positions
                                                .iter()
                                                .find(|item| item.stock_code == code_text)
                                                .map(|item| lots_of(item.quantity))
                                        })
                                        .unwrap_or(0)
                                });
                                match current.as_str() {
                                    "close" if available > 0 => {
                                        format!("手数（全仓 {available} 手）")
                                    }
                                    "reduce" if available > 0 => {
                                        format!("手数（可用 {available} 手）")
                                    }
                                    _ => "手数".to_string(),
                                }
                            };
                            let amount_value = {
                                let price_value: f64 =
                                    price_signal.get().trim().parse().unwrap_or(0.0);
                                let lots_value: i64 =
                                    lots_signal.get().trim().parse().unwrap_or(0);
                                if price_value > 0.0 && lots_value > 0 {
                                    format!(
                                        "¥{}",
                                        format::amount(
                                            ((price_value * 100.0).round() as i64)
                                                .saturating_mul(lots_value)
                                                .saturating_mul(100),
                                        ),
                                    )
                                } else {
                                    "—".to_string()
                                }
                            };
                            let count = rows.rows_text.get().len();
                            view! {
                                <div class="stock-fill-row">
                                    <Input value=price_signal placeholder="成交价（元/股）" />
                                    <Input value=lots_signal placeholder=lots_label />
                                    <span class="stock-fill-row__amount">{amount_value}</span>
                                    {if count > 1 {
                                        view! {
                                            <button
                                                type="button"
                                                class="ui-btn ui-btn--text-danger ui-btn--sm"
                                                on:click=move |_| rows.remove(index)
                                            >
                                                "删除"
                                            </button>
                                        }
                                            .into_any()
                                    } else {
                                        view! { <span class="stock-fill-row__spacer"></span> }
                                            .into_any()
                                    }}
                                </div>
                            }
                        })
                        .collect_view()
                }}
            </div>

            <div class="stock-trade-form__summary">
                {move || {
                    // 依赖每行的输入信号：任一改动都会重算汇总
                    for signal in rows.price_signals.get().iter() {
                        let _ = signal.get();
                    }
                    for signal in rows.lots_signals.get().iter() {
                        let _ = signal.get();
                    }
                    let valid = rows.parsed();
                    let total_lots: i64 = valid.iter().map(|fill| fill.lots).sum();
                    let fill_amounts: Vec<i64> =
                        valid.iter().map(|fill| fill.amount_cents()).collect();
                    let total_amount: i64 = fill_amounts.iter().sum();
                    let is_buy = format::is_buy(&trade_type.get());
                    let estimate = match fee_settings.get() {
                        Some(fee) if total_amount > 0 => {
                            estimate_fee(&fill_amounts, is_buy, &code.get(), &fee)
                        }
                        _ => FeeEstimate::default(),
                    };
                    let fee_total = estimate.total();
                    let net = if is_buy {
                        total_amount.saturating_add(fee_total)
                    } else {
                        total_amount.saturating_sub(fee_total)
                    };
                    let fee_text = if total_amount <= 0 || fee_settings.get().is_none() {
                        "—".to_string()
                    } else if is_buy {
                        format!(
                            "佣金 ¥{} + 过户费 ¥{} = ¥{}",
                            format::amount(estimate.commission),
                            format::amount(estimate.transfer_fee),
                            format::amount(fee_total),
                        )
                    } else {
                        format!(
                            "佣金 ¥{} + 印花税 ¥{} + 过户费 ¥{} = ¥{}",
                            format::amount(estimate.commission),
                            format::amount(estimate.stamp_duty),
                            format::amount(estimate.transfer_fee),
                            format::amount(fee_total),
                        )
                    };
                    let min_commission = fee_settings
                        .get()
                        .map(|fee| fee.min_commission)
                        .unwrap_or(0);
                    view! {
                        <div class="stock-summary-row">
                            <span class="stock-summary-row__label">"合计"</span>
                            <span class="stock-summary-row__value">
                                {format!(
                                    "{} 笔 · {} 手 · ¥{}",
                                    valid.len(),
                                    total_lots,
                                    format::amount(total_amount),
                                )}
                            </span>
                        </div>
                        <div class="stock-summary-row">
                            <span class="stock-summary-row__label">"预估费用"</span>
                            <span class="stock-summary-row__value">{fee_text}</span>
                        </div>
                        <Show when=move || rows.rows_text.get().len().gt(&1)>
                            <div class="stock-summary-hint">
                                {format!(
                                    "佣金按委托收取一次，不足 ¥{} 按 ¥{} 计；印花税与过户费按每笔成交各收一次",
                                    format::amount(min_commission),
                                    format::amount(min_commission),
                                )}
                            </div>
                        </Show>
                        <div class="stock-summary-row is-total">
                            <span class="stock-summary-row__label">
                                {if is_buy { "预计支出" } else { "预计到手" }}
                            </span>
                            <span class="stock-summary-row__value">
                                {format!("¥{}", format::amount(net))}
                            </span>
                        </div>
                    }
                }}
            </div>

            <div class="stock-trade-form__grid">
                <div class="modal-form-item">
                    <p class="modal-form-label">"委托时间"</p>
                    <DatePicker value=date />
                </div>
                <div class="modal-form-item">
                    <p class="modal-form-label">"交易标签"</p>
                    <Select
                        value=tag
                        options=Signal::derive(move || {
                            tags.get()
                                .into_iter()
                                .map(SelectOption::same)
                                .collect()
                        })
                        .get()
                    />
                </div>
            </div>
            <p class="stock-trade-form__hint">
                {move || {
                    let default = default_tag.get();
                    if default.is_empty() {
                        "清仓时需选择交易标签".to_string()
                    } else {
                        format!("清仓时默认使用标签「{default}」")
                    }
                }}
            </p>
        </Modal>
    }
    .into_any()
}

// ==================================================================== 成交表渲染

/// 一条渲染行：委托内的多笔成交聚合成父行 + 子行。
#[derive(Debug, Clone, PartialEq)]
struct TradeRow {
    key: String,
    is_group: bool,
    is_child: bool,
    trades: Vec<StockTradeDto>,
    trade_type: String,
    price: i64,
    lots: i64,
    amount: i64,
    fee: i64,
    commission: i64,
    stamp_duty: i64,
    transfer_fee: i64,
    trade_time: i64,
    realized_pnl: Option<i64>,
}

/// 按 `orderId` 分组（键 = `orderId || id`，组内按 `orderSeq` 升序）。
fn group_trades(trades: &[StockTradeDto]) -> Vec<TradeRow> {
    let mut groups: Vec<(String, Vec<StockTradeDto>)> = Vec::new();
    for trade in trades {
        let key = if trade.order_id.is_empty() {
            trade.id.clone()
        } else {
            trade.order_id.clone()
        };
        match groups.iter_mut().find(|(existing, _)| *existing == key) {
            Some((_, items)) => items.push(trade.clone()),
            None => groups.push((key, vec![trade.clone()])),
        }
    }

    groups
        .into_iter()
        .map(|(key, mut items)| {
            items.sort_by_key(|trade| trade.order_seq);
            let first = items.first().cloned().unwrap_or_default();
            let shares: i64 = items.iter().map(|trade| trade.shares).sum();
            let amount: i64 = items.iter().map(|trade| trade.amount).sum();
            let fee: i64 = items.iter().map(|trade| trade.fee).sum();
            let commission: i64 = items.iter().map(|trade| trade.commission).sum();
            let stamp_duty: i64 = items.iter().map(|trade| trade.stamp_duty).sum();
            let transfer_fee: i64 = items.iter().map(|trade| trade.transfer_fee).sum();
            let lots: i64 = items.iter().map(|trade| trade.lots).sum();
            let pnl_values: Vec<i64> = items
                .iter()
                .filter_map(|trade| trade.realized_pnl)
                .collect();
            let realized_pnl = if pnl_values.is_empty() {
                None
            } else {
                Some(pnl_values.iter().sum())
            };
            let price = if shares > 0 {
                ((amount as f64) / (shares as f64)).round() as i64
            } else {
                0
            };
            let single = items.len() == 1;
            TradeRow {
                key: if single {
                    first.id.clone()
                } else {
                    format!("order-{key}")
                },
                is_group: !single,
                is_child: false,
                trades: items,
                trade_type: first.trade_type.clone(),
                price: if single { first.price } else { price },
                lots,
                amount,
                fee,
                commission,
                stamp_duty,
                transfer_fee,
                trade_time: first.trade_time,
                realized_pnl,
            }
        })
        .collect()
}

/// 渲染成交表的所有行（父行 + 子行）。
fn trade_table_rows(
    trades: RwSignal<Vec<StockTradeDto>>,
    collapsed: RwSignal<BTreeSet<String>>,
    on_edit: UnsyncCallback<(StockTradeDto, Vec<StockTradeDto>)>,
    on_delete: UnsyncCallback<Vec<StockTradeDto>>,
) -> AnyView {
    let groups = move || group_trades(&trades.get());

    view! {
        {move || {
            let rows = groups();
            if rows.is_empty() {
                return view! {
                    <tr>
                        <td colspan="8">
                            <Empty title="暂无成交记录" />
                        </td>
                    </tr>
                }
                    .into_any();
            }
            rows.into_iter()
                .map(|row| {
                    let is_collapsed = collapsed.get().contains(&row.key);
                    let siblings = row.trades.clone();
                    let order_trades = row.trades.clone();
                    let main = trade_row_view(
                        &row,
                        false,
                        UnsyncCallback::new({
                            let row_trades = row.trades.clone();
                            move |()| {
                                if let Some(trade) = row_trades.first().cloned() {
                                    on_edit.run((trade, row_trades.clone()));
                                }
                            }
                        }),
                        UnsyncCallback::new({
                            let order_trades = order_trades.clone();
                            move |()| on_delete.run(order_trades.clone())
                        }),
                        row.is_group,
                    );
                    let children = if row.is_group && !is_collapsed {
                        siblings
                            .iter()
                            .map(|trade| {
                                let child = TradeRow {
                                    key: trade.id.clone(),
                                    is_group: false,
                                    is_child: true,
                                    trades: vec![trade.clone()],
                                    trade_type: trade.trade_type.clone(),
                                    price: trade.price,
                                    lots: trade.lots,
                                    amount: trade.amount,
                                    fee: trade.fee,
                                    commission: trade.commission,
                                    stamp_duty: trade.stamp_duty,
                                    transfer_fee: trade.transfer_fee,
                                    trade_time: trade.trade_time,
                                    realized_pnl: trade.realized_pnl,
                                };
                                let child_siblings = siblings.clone();
                                let trade_for_edit = trade.clone();
                                view! {
                                    {trade_row_view(
                                        &child,
                                        true,
                                        UnsyncCallback::new(move |()| {
                                            on_edit
                                                .run((
                                                    trade_for_edit.clone(),
                                                    child_siblings.clone(),
                                                ))
                                        }),
                                        UnsyncCallback::new(|_| {}),
                                        false,
                                    )}
                                }
                            })
                            .collect_view()
                            .into_any()
                    } else {
                        ().into_any()
                    };
                    view! { <>{main}{children}</> }
                })
                .collect_view()
                .into_any()
        }}
    }
    .into_any()
}

/// 一行成交（父行 / 单行 / 子行）。
fn trade_row_view(
    row: &TradeRow,
    is_child: bool,
    on_edit: UnsyncCallback<()>,
    on_delete: UnsyncCallback<()>,
    is_group: bool,
) -> AnyView {
    let trade_type = row.trade_type.clone();
    let buy = format::is_buy(&trade_type);
    let change = if is_child {
        None
    } else if buy {
        Some(row.amount.saturating_add(row.fee).saturating_neg())
    } else {
        Some(row.amount.saturating_sub(row.fee))
    };
    let fee_text = if row.commission + row.stamp_duty + row.transfer_fee > 0 {
        if buy {
            format!(
                "佣金 ¥{} + 过户费 ¥{}",
                format::amount(row.commission),
                format::amount(row.transfer_fee),
            )
        } else {
            format!(
                "佣金 ¥{} + 印花税 ¥{} + 过户费 ¥{}",
                format::amount(row.commission),
                format::amount(row.stamp_duty),
                format::amount(row.transfer_fee),
            )
        }
    } else {
        format!("¥{}", format::amount(row.fee))
    };
    let type_label = format!("{}手", row.lots);
    let count_note = row.trades.len();

    view! {
        <tr
            class="stock-trade-row"
            class:is-child=is_child
            class:is-group=is_group
        >
            <td class="is-center stock-mono">
                {format_timestamp(row.trade_time, "YYYY-MM-DD HH:mm")}
            </td>
            <td class="is-center">
                {if is_child {
                    ().into_any()
                } else {
                    view! {
                        <span class=format!(
                            "stock-trade-type {}",
                            if buy { "type-buy" } else { "type-sell" },
                        )>{format::trade_type_label(&trade_type)}</span>
                    }
                        .into_any()
                }}
            </td>
            <td class="is-right stock-mono">
                {if is_group {
                    format!("均价 {}", format::amount(row.price))
                } else {
                    format::amount(row.price)
                }}
            </td>
            <td class="is-right">
                {type_label}
                {if is_group {
                    view! {
                        <span class="stock-lots-note">{format!("{count_note} 笔")}</span>
                    }
                        .into_any()
                } else {
                    ().into_any()
                }}
            </td>
            <td class="is-right stock-mono">{format::amount(row.amount)}</td>
            <td class="stock-mono stock-fee-cell">{fee_text}</td>
            <td class="is-right">
                {match change {
                    Some(value) => {
                        view! {
                            <span class=format!("stock-amount {}", format::pnl_class(value))>
                                {format::signed_yuan(value)}
                            </span>
                        }
                            .into_any()
                    }
                    None => view! { <span class="is-muted">"—"</span> }.into_any(),
                }}
            </td>
            <td class="is-center">
                <div class="stock-row-actions">
                    {if is_group {
                        ().into_any()
                    } else {
                        view! {
                            <button
                                type="button"
                                class="ui-btn ui-btn--link ui-btn--sm"
                                on:click=move |_| on_edit.run(())
                            >
                                "编辑"
                            </button>
                        }
                            .into_any()
                    }}
                    {if is_child {
                        ().into_any()
                    } else {
                        view! {
                            <button
                                type="button"
                                class="ui-btn ui-btn--link ui-btn--sm is-danger"
                                on:click=move |_| on_delete.run(())
                            >
                                "删除"
                            </button>
                        }
                            .into_any()
                    }}
                </div>
            </td>
        </tr>
    }
    .into_any()
}

// ==================================================================== 子功能三：记录（已清仓的轮次历史）

/// 记录子功能：全局汇总 + 已清仓股票列表 + 轮次 + 成交表 + 轮次复盘/标签。
///
/// 本子功能没有页面级操作，所以**不给工具栏**（`FeaturePage` 不传 `toolbar`，
/// 免得多出一条空发丝线）。行内「编辑/删除」在持仓子功能的详情区，不在这里。
fn history_view(sub: RwSignal<StockSub>) -> AnyView {
    let stores = AppStores::global();
    let histories = RwSignal::new(Vec::<StockTradeHistoryDto>::new());
    let summary = RwSignal::new(StockTradeHistorySummaryDto::default());
    let detail = RwSignal::new(Option::<StockTradeHistoryDetailDto>::None);
    let selected_code = RwSignal::new(String::new());
    let histories_loading = RwSignal::new(false);
    let detail_loading = RwSignal::new(false);
    let tags = RwSignal::new(Vec::<String>::new());
    let collapsed_rounds = RwSignal::new(BTreeSet::<String>::new());
    let collapsed_reviews = RwSignal::new(BTreeSet::<String>::new());
    let review_editing = RwSignal::new(String::new());
    let review_draft = RwSignal::new(String::new());
    let review_saving = RwSignal::new(false);
    let tag_saving = RwSignal::new(false);

    let load_history = move || {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            return;
        }
        histories_loading.set(true);
        let ledger_for_summary = ledger_id.clone();
        let ledger_for_tags = ledger_id.clone();
        leptos::task::spawn_local(async move {
            match api::stock::history(&ledger_id).await {
                Ok(items) => {
                    let current = selected_code.get_untracked();
                    let still_exists = items.iter().any(|item| item.stock_code == current);
                    histories.set(items.clone());
                    if !still_exists {
                        let next = items
                            .first()
                            .map(|item| item.stock_code.clone())
                            .unwrap_or_default();
                        selected_code.set(next);
                    }
                }
                Err(error) => notify_error("查询交易历史失败", &error),
            }
            histories_loading.set(false);
        });
        leptos::task::spawn_local(async move {
            match api::stock::history_summary(&ledger_for_summary).await {
                Ok(data) => summary.set(data),
                Err(error) => notify_error("查询交易历史总览失败", &error),
            }
        });
        leptos::task::spawn_local(async move {
            match api::stock::tag_settings_get(&ledger_for_tags).await {
                Ok(data) => tags.set(data.tags),
                Err(error) => notify_error("查询交易标签失败", &error),
            }
        });
    };

    let load_detail = move |code: String| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() || code.is_empty() {
            detail.set(None);
            return;
        }
        detail_loading.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::history_detail(&ledger_id, &code).await {
                Ok(data) => detail.set(Some(data)),
                Err(error) => {
                    detail.set(None);
                    notify_error("查询交易历史详情失败", &error);
                }
            }
            detail_loading.set(false);
        });
    };

    Effect::new(move |prev: Option<String>| {
        let ledger_id = stores.current_ledger_id.get();
        if prev.as_deref() == Some(ledger_id.as_str()) {
            return ledger_id;
        }
        selected_code.set(String::new());
        detail.set(None);
        if ledger_id.is_empty() {
            histories.set(Vec::new());
            return ledger_id;
        }
        load_history();
        ledger_id
    });

    Effect::new(move |prev: Option<String>| {
        let code = selected_code.get();
        if prev.as_deref() == Some(code.as_str()) {
            return code;
        }
        // 切换股票 → 展开态与草稿全部重置
        review_editing.set(String::new());
        review_draft.set(String::new());
        collapsed_rounds.set(BTreeSet::new());
        collapsed_reviews.set(BTreeSet::new());
        if !code.is_empty() {
            load_detail(code.clone());
        }
        code
    });

    let save_round_review = move |round_id: String| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() || round_id.is_empty() {
            return;
        }
        let review = review_draft.get_untracked();
        review_saving.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::round_review(&ledger_id, &round_id, &review).await {
                Ok(data) => {
                    detail.set(Some(data));
                    review_editing.set(String::new());
                    review_draft.set(String::new());
                    Notifier::global().success("交易复盘已保存".to_string(), None);
                }
                Err(error) => notify_error("保存交易复盘失败", &error),
            }
            review_saving.set(false);
        });
    };

    let save_round_tag = move |(round_id, tag): (String, String)| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() || round_id.is_empty() {
            return;
        }
        tag_saving.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::round_tag(&ledger_id, &round_id, &tag).await {
                Ok(data) => {
                    detail.set(Some(data));
                    Notifier::global().success("交易标签已保存".to_string(), None);
                }
                Err(error) => notify_error("保存交易标签失败", &error),
            }
            tag_saving.set(false);
        });
    };

    let content = view! {
        <div class="stock-body">
        <div class="stock-history">
            <Show when=move || !histories.get().is_empty() || summary.get().stock_count.is_positive()>
                <div class="stock-summary-bar">
                    <div class="stock-summary-bar__cells">
                        <SummaryCell
                            label="已实现盈亏".to_string()
                            value=Signal::derive(move || format::signed_yuan(summary.get().total_pnl))
                            class=Signal::derive(move || {
                                format::pnl_class(summary.get().total_pnl).to_string()
                            })
                        />
                        <SummaryCell
                            label="累计盈亏率".to_string()
                            value=Signal::derive(move || {
                                format::signed_percent(summary.get().total_pnl_rate)
                            })
                            class=Signal::derive(move || {
                                format::pnl_class(summary.get().total_pnl).to_string()
                            })
                        />
                        <SummaryCell
                            label="胜负轮次".to_string()
                            value=Signal::derive(move || {
                                let data = summary.get();
                                format!("{} 胜 · {} 负", data.win_count, data.loss_count)
                            })
                            class=Signal::derive(String::new)
                        />
                        <SummaryCell
                            label="总轮次".to_string()
                            value=Signal::derive(move || {
                                format!("{} 轮", summary.get().round_count)
                            })
                            class=Signal::derive(String::new)
                        />
                    </div>
                </div>
            </Show>

            <div class="stock-history__grid">
                <div class="stock-panel stock-panel--list">
                    <Show
                        when=move || !histories.get().is_empty()
                        fallback=move || {
                            view! {
                                <div class="stock-empty">
                                    {move || {
                                        if histories_loading.get() {
                                            "正在加载…"
                                        } else {
                                            "暂无已清仓股票"
                                        }
                                    }}
                                </div>
                            }
                        }
                    >
                        <div class="stock-history__cards">
                            {move || {
                                histories
                                    .get()
                                    .into_iter()
                                    .map(|item| {
                                        let code = item.stock_code.clone();
                                        let is_active = code == selected_code.get();
                                        let code_for_click = code.clone();
                                        view! {
                                            <button
                                                type="button"
                                                class="stock-history-card"
                                                class:is-active=is_active
                                                title=format!(
                                                    "累计已实现盈亏 {}",
                                                    format::signed_yuan(item.total_pnl),
                                                )
                                                on:click=move |_| selected_code.set(code_for_click.clone())
                                            >
                                                <div class="stock-history-card__head">
                                                    <span class="stock-history-card__name">
                                                        {item.stock_name.clone()}
                                                    </span>
                                                    <span class="stock-mono stock-history-card__code">
                                                        {item.stock_code.clone()}
                                                    </span>
                                                </div>
                                                <div class="stock-history-card__quote">
                                                    <span class="stock-history-card__label">"现价"</span>
                                                    <span class="stock-history-card__value">
                                                        {format::quote_text(item.latest_price)}
                                                    </span>
                                                </div>
                                                <div class="stock-history-card__foot">
                                                    <span>
                                                        {format!(
                                                            "{} 轮 · 最近 {}",
                                                            item.round_count,
                                                            format_timestamp(item.last_closed_at, "YYYY-MM-DD"),
                                                        )}
                                                    </span>
                                                    <span class=format!(
                                                        "stock-amount {}",
                                                        format::pnl_class(item.total_pnl),
                                                    )>{format::pnl_text(item.total_pnl)}</span>
                                                </div>
                                            </button>
                                        }
                                    })
                                    .collect_view()
                            }}
                        </div>
                    </Show>
                </div>

                <div class="stock-panel stock-panel--detail">
                    {move || {
                        let Some(data) = detail.get() else {
                            return view! {
                                <div class="stock-empty">
                                    {move || {
                                        if detail_loading.get() {
                                            "正在加载…"
                                        } else {
                                            "选择左侧股票查看交易历史"
                                        }
                                    }}
                                </div>
                            }
                                .into_any();
                        };
                        view! {
                            <div class="stock-summary-bar">
                                <span class="stock-summary-bar__title">
                                    {format!("{} {}", data.stock_name, data.stock_code)}
                                </span>
                                <div class="stock-summary-bar__cells">
                                    <SummaryCell
                                        label="已实现盈亏".to_string()
                                        value=Signal::derive(move || format::signed_yuan(data.total_pnl))
                                        class=Signal::derive(move || {
                                            format::pnl_class(data.total_pnl).to_string()
                                        })
                                    />
                                    <SummaryCell
                                        label="累计盈亏率".to_string()
                                        value=Signal::derive(move || {
                                            format::signed_percent(data.total_pnl_rate)
                                        })
                                        class=Signal::derive(move || {
                                            format::pnl_class(data.total_pnl).to_string()
                                        })
                                    />
                                    <SummaryCell
                                        label="胜负轮次".to_string()
                                        value=Signal::derive(move || {
                                            format!("{} 胜 · {} 负", data.win_count, data.loss_count)
                                        })
                                        class=Signal::derive(String::new)
                                    />
                                    <SummaryCell
                                        label="已完成轮次".to_string()
                                        value=Signal::derive(move || {
                                            format!("{} 轮", data.round_count)
                                        })
                                        class=Signal::derive(String::new)
                                    />
                                </div>
                            </div>

                            <div class="stock-rounds">
                                {data
                                    .rounds
                                    .clone()
                                    .into_iter()
                                    .map(|round| {
                                        round_card(
                                            round,
                                            collapsed_rounds,
                                            collapsed_reviews,
                                            review_editing,
                                            review_draft,
                                            review_saving,
                                            tag_saving,
                                            tags,
                                            UnsyncCallback::new(move |id: String| {
                                                save_round_review(id)
                                            }),
                                            UnsyncCallback::new(move |(id, tag): (String, String)| {
                                                save_round_tag((id, tag))
                                            }),
                                        )
                                    })
                                    .collect_view()}
                            </div>
                        }
                            .into_any()
                    }}
                </div>
            </div>
        </div>
        </div>
    }
    .into_any();

    view! {
        <FeaturePage
            title=PAGE_TITLE
            class="stock-page"
            rail=view! { <StockSubRail sub=sub /> }.into_any()
            content=content
        />
    }
    .into_any()
}

/// 汇总条里的一格指标。
#[component]
fn SummaryCell(
    label: String,
    #[prop(into)] value: Signal<String>,
    #[prop(into)] class: Signal<String>,
) -> impl IntoView {
    view! {
        <div class="stock-summary-cell">
            <span class="stock-summary-cell__label">{label}</span>
            <span class=move || format!("stock-summary-cell__value {}", class.get())>
                {move || value.get()}
            </span>
        </div>
    }
}

/// 一轮交易卡片（轮头 + 成交表 + 本轮复盘）。
#[allow(clippy::too_many_arguments)]
fn round_card(
    round: StockTradeRoundDto,
    collapsed_rounds: RwSignal<BTreeSet<String>>,
    collapsed_reviews: RwSignal<BTreeSet<String>>,
    review_editing: RwSignal<String>,
    review_draft: RwSignal<String>,
    review_saving: RwSignal<bool>,
    tag_saving: RwSignal<bool>,
    tags: RwSignal<Vec<String>>,
    on_save_review: UnsyncCallback<String>,
    on_save_tag: UnsyncCallback<(String, String)>,
) -> AnyView {
    // 频繁复用的几个值放进 `StoredValue`（`Copy`）：
    // 它们在多个 `Show` 的 children（必须实现 `Fn`）里被读取，
    // 直接 move 捕获 `String` 会让那个 children 退化成 `FnOnce`。
    let id = StoredValue::new(round.id.clone());
    let review = StoredValue::new(round.review.clone());
    let has_review = StoredValue::new(!round.review.trim().is_empty());
    let collapsed = collapsed_rounds.get().contains(&round.id);
    let review_collapsed = collapsed_reviews.get().contains(&round.id);
    let editing = review_editing.get() == round.id;
    let result_class = format::result_class(round.pnl);
    let pnl_class = format::pnl_class(round.pnl);

    view! {
        <div class="stock-round">
            <div class="stock-round__head">
                <button
                    type="button"
                    class="stock-round__toggle"
                    aria-expanded=!collapsed
                    aria-label=if collapsed {
                        format!("第 {} 轮，点击展开", round.round_no)
                    } else {
                        format!("第 {} 轮，点击收起", round.round_no)
                    }
                    on:click=move |_| {
                        let key = id.get_value();
                        collapsed_rounds.update(|set| {
                            if !set.remove(&key) {
                                set.insert(key.clone());
                            }
                        });
                    }
                >
                    <span class="diary-tree__caret" class:is-open=!collapsed>
                        {icons::icon(Icon::CaretRight)}
                    </span>
                    <span class="stock-round__no">{format!("第 {} 轮", round.round_no)}</span>
                    <span class="stock-round__range stock-mono">
                        {format!(
                            "{} → {}",
                            format_timestamp(round.opened_at, "YYYY-MM-DD HH:mm"),
                            format_timestamp(round.closed_at, "YYYY-MM-DD HH:mm"),
                        )}
                    </span>
                </button>
                <div class="stock-round__meta">
                    <div class="stock-round__tag" on:click=move |event| event.stop_propagation()>
                        <Select
                            value=RwSignal::new(round.tag.clone())
                            options={
                                tags.get()
                                    .into_iter()
                                    .map(SelectOption::same)
                                    .collect::<Vec<_>>()
                            }
                            disabled=Signal::derive(move || tag_saving.get())
                            on_change=UnsyncCallback::new(move |tag: String| {
                                on_save_tag.run((id.get_value(), tag))
                            })
                        />
                    </div>
                    <span class=format!("stock-round__result {result_class}")>
                        {format::result_label(round.pnl)}
                    </span>
                    <span class=format!("stock-amount {pnl_class}")>
                        {format::signed_yuan(round.pnl)}
                    </span>
                    <span class=format!("stock-round__rate {pnl_class}")>
                        {format::rate_text(round.pnl_rate)}
                    </span>
                </div>
            </div>

            <Show when=move || !collapsed_rounds.get().contains(&id.get_value())>
                <div class="stock-table-wrap">
                    <table class="stock-table stock-table--trades">
                        <thead>
                            <tr>
                                <th class="is-center" style="width: 150px;">"时间"</th>
                                <th class="is-center" style="width: 90px;">"类型"</th>
                                <th class="is-right" style="width: 110px;">"成交价"</th>
                                <th class="is-right" style="width: 90px;">"手数"</th>
                                <th class="is-right" style="width: 120px;">"成交金额"</th>
                                <th class="is-right" style="width: 110px;">"费用"</th>
                            </tr>
                        </thead>
                        <tbody>
                            {round
                                .trades
                                .clone()
                                .into_iter()
                                .map(|trade| {
                                    let buy = format::is_buy(&trade.trade_type);
                                    view! {
                                        <tr>
                                            <td class="is-center stock-mono">
                                                {format_timestamp(trade.trade_time, "YYYY-MM-DD HH:mm")}
                                            </td>
                                            <td class="is-center">
                                                <span class=format!(
                                                    "stock-trade-type {}",
                                                    if buy { "type-buy" } else { "type-sell" },
                                                )>{format::trade_type_label(&trade.trade_type)}</span>
                                            </td>
                                            <td class="is-right stock-mono">{format::amount(trade.price)}</td>
                                            <td class="is-right">{format!("{}手", trade.lots)}</td>
                                            <td class="is-right stock-mono">{format::amount(trade.amount)}</td>
                                            <td class="is-right stock-mono stock-fee-cell">
                                                {format::amount(trade.fee)}
                                            </td>
                                        </tr>
                                    }
                                })
                                .collect_view()}
                        </tbody>
                    </table>
                </div>

                <div class="stock-review">
                    <div class="stock-review__head">
                        <button
                            type="button"
                            class="stock-review__toggle"
                            aria-expanded=!review_collapsed
                            disabled=!has_review.get_value()
                            on:click=move |_| {
                                let key = id.get_value();
                                collapsed_reviews.update(|set| {
                                    if !set.remove(&key) {
                                        set.insert(key.clone());
                                    }
                                });
                            }
                        >
                            <span class="diary-tree__caret" class:is-open=!review_collapsed>
                                {icons::icon(Icon::CaretRight)}
                            </span>
                            <span class="stock-review__title">"本轮复盘"</span>
                            <Show when=move || review_collapsed && !review.get_value().is_empty()>
                                <span class="stock-review__summary">{review.get_value()}</span>
                            </Show>
                        </button>
                        <Show when=move || !editing>
                            <button
                                type="button"
                                class="ui-btn ui-btn--text ui-btn--sm"
                                on:click=move |_| {
                                    let key = id.get_value();
                                    review_draft.set(if has_review.get_value() {
                                        review.get_value()
                                    } else {
                                        ROUND_REVIEW_TEMPLATE.to_string()
                                    });
                                    collapsed_reviews.update(|set| {
                                        set.remove(&key);
                                    });
                                    review_editing.set(key);
                                }
                            >
                                <span class="ui-btn__icon">{icons::icon(Icon::Edit)}</span>
                                {if has_review.get_value() { "编辑" } else { "写复盘" }}
                            </button>
                        </Show>
                    </div>
                    <Show when=move || !collapsed_reviews.get().contains(&id.get_value())>
                        <Show
                            when=move || review_editing.get() == id.get_value()
                            fallback=move || {
                                view! {
                                    {if has_review.get_value() {
                                        view! {
                                            <p class="stock-review__text">{review.get_value()}</p>
                                        }
                                            .into_any()
                                    } else {
                                        view! {
                                            <p class="stock-review__hint">
                                                "还没有写本轮复盘，可随时记录建仓理由与操作计划。"
                                            </p>
                                        }
                                            .into_any()
                                    }}
                                }
                            }
                        >
                            {review_editor(
                                review_draft,
                                review_saving,
                                UnsyncCallback::new(move |()| {
                                    review_editing.set(String::new());
                                    review_draft.set(String::new());
                                }),
                                UnsyncCallback::new(move |()| on_save_review.run(id.get_value())),
                            )}
                        </Show>
                    </Show>
                </div>
            </Show>
        </div>
    }
    .into_any()
}
// ==================================================================== 分栏四：交易统计

/// 统计曲线的指标定义（顺序与文案固定，改动即影响界面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Metric {
    TotalPnl,
    WinRate,
    AvgWin,
    AvgLoss,
    PnlRatio,
    Expectancy,
    MaxDrawdown,
}

impl Metric {
    const ALL: [Metric; 7] = [
        Metric::TotalPnl,
        Metric::WinRate,
        Metric::AvgWin,
        Metric::AvgLoss,
        Metric::PnlRatio,
        Metric::Expectancy,
        Metric::MaxDrawdown,
    ];

    fn label(self) -> &'static str {
        match self {
            Metric::TotalPnl => "累计盈亏",
            Metric::WinRate => "胜率",
            Metric::AvgWin => "平均盈利",
            Metric::AvgLoss => "平均亏损",
            Metric::PnlRatio => "实际盈亏比",
            Metric::Expectancy => "期望值",
            Metric::MaxDrawdown => "最大回撤",
        }
    }

    fn kind(self) -> ChartValueKind {
        match self {
            Metric::WinRate | Metric::MaxDrawdown => ChartValueKind::Percent,
            Metric::PnlRatio => ChartValueKind::Count,
            _ => ChartValueKind::Money,
        }
    }

    /// 是否画 y=0 虚线参考线（金额类且可能为负的指标才画）。
    fn has_reference(self) -> bool {
        matches!(self, Metric::TotalPnl | Metric::Expectancy)
    }

    /// 曲线取值（分 / 百分数 / 倍数）。
    fn value(self, point: &StockStatisticsPointDto) -> f64 {
        match self {
            Metric::TotalPnl => point.total_pnl as f64,
            Metric::WinRate => point.win_rate,
            Metric::AvgWin => point.avg_win as f64,
            // 平均亏损取负（曲线在 0 轴下方）
            Metric::AvgLoss => -(point.avg_loss as f64),
            Metric::PnlRatio => point.pnl_ratio.unwrap_or(0.0),
            Metric::Expectancy => point.expectancy as f64,
            Metric::MaxDrawdown => point.max_drawdown_pct,
        }
    }
}

/// 统计子功能的两个分栏（工具栏左侧 `Tabs` 的 key，固定文案，改动即影响界面）。
const STATS_TAB_SUMMARY: &str = "summary";
const STATS_TAB_DETAIL: &str = "detail";

/// 逐笔结算明细的默认每页条数（界面侧分页，理由见 [`statistics_view`]）。
const STATS_DETAIL_PAGE_SIZE: i32 = 20;

/// 统计子功能：工具栏左侧两个分栏（**统计** / **明细**），右侧是**两个分栏共用**的筛选组。
///
/// * **统计**：结算统计（已实现盈亏 + 6 个累计口径指标）+ 统计曲线。
/// * **明细**：逐笔结算明细表，按结算日期倒序、分页。
///
/// ## 两个分栏都填满内容区（这一页不再整页滚动）
///
/// 高度链是：`.page-content` → `.stock-body` → `.stock-statistics` → `.ui-tab-pane`
/// → `.stock-panel`，每一级都必须 `flex: 1; min-height: 0`（见 `stock.css` 的子功能四）。
/// 分栏内部再把"剩下的高度"给需要它的那一块：
/// 统计 → 曲线（图例之外全部给它，图表按画布实测尺寸重画 SVG）；
/// 明细 → 表格区（纵向/横向都在这块里滚）+ 页脚贴分栏底边。
///
/// ## 分页为什么在界面侧切
///
/// 每个统计点都是"截至该笔"的**累计口径**（累计盈亏 / 胜率 / 平均盈亏 / 最大回撤），
/// 必须由整条时序算出来 —— 服务端无论分不分页都得全量计算一遍。所以后端一次给全量
/// （`points`），界面只切展示：翻页是瞬时的，两个分栏也共用同一份数据（`STATS_DATA`），
/// 不会为翻页反复重算整条序列。数据量级是"清仓轮次数"，界面侧承载没有压力。
///
/// ## 缓存与复核（`STATS_DATA` / `STATS_TAGS`）
///
/// 这两份缓存跨视图重建保留，让人切走再切回来时**立刻**看到上一份结果。但统计的输入
/// 分布在别的子功能里（下单 / 改成交 / 减仓 / 清仓 / 重置在账户·持仓·设置，交易标签在设置），
/// 所以每次挂载都**复核一次**：缓存先渲染，请求回来若与缓存一致则**不写信号**
/// （避免图表重建、入场动画重播），不一致才换掉。去重键见 `stats_request_needed`。
///
/// ## 其余约定
///
/// 筛选组（区间 / 最近 N 笔 / 标签）是**页面级操作**，放工具栏右侧、两个分栏共用；
/// 工具栏在三种内容态（加载中 / 空态 / 有数据）下都渲染 —— 否则空态会把筛选入口一起藏掉。
fn statistics_view(sub: RwSignal<StockSub>) -> AnyView {
    let stores = AppStores::global();
    let stats = RwSignal::new(STATS_DATA.with(|data| data.borrow().clone()));
    let loading = RwSignal::new(STATS_LOADING.with(|loading| loading.get()));
    let tab = RwSignal::new(STATS_TAB_SUMMARY.to_string());
    let metric = RwSignal::new(Metric::TotalPnl.label().to_string());
    let filter_mode = RwSignal::new("all".to_string());
    let range_start = RwSignal::new(String::new());
    let range_end = RwSignal::new(String::new());
    let recent = RwSignal::new(0_i64);
    // 「最近 N 笔」下拉的绑定值：**必须建在组件体里**。
    // 之前写成 `value=RwSignal::new(recent.get().to_string())` —— 那是"把信号建在 props 里"，
    // 视图每重渲染一次就新建一个、值回到初始的 `0`，选中项立刻被弹回去（就是"刷新按钮异常"
    // 旁边那个下拉选不动的来源）。这里单独建一个、并在 `recent` 被重置时同步。
    let recent_signal = RwSignal::new(recent.get_untracked().to_string());
    let tag_filter = RwSignal::new(String::new());
    // 选项从**模块级缓存**起手：视图重建（切子功能再切回来）时下拉不会空掉，
    // 也就不必为了填它而重新拉一次标签（那次重新拉正是过去自激循环的一环）。
    let tags = RwSignal::new(STATS_TAGS.with(|tags| tags.borrow().clone()));
    // 明细分栏的分页状态（界面侧分页，见本函数文档注释）；每页条数走共享的
    // `Pagination` 下拉，默认值在 `STATS_DETAIL_PAGE_SIZE` 里（属于 `PAGE_SIZE_OPTIONS`）。
    let detail_page = RwSignal::new(1_i32);
    let detail_page_size = RwSignal::new(STATS_DETAIL_PAGE_SIZE);

    let apply_filter = move |force: bool| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            stats.set(None);
            return;
        }
        let mode = filter_mode.get_untracked();
        let (start_month, end_month) = if mode == "range" {
            (range_start.get_untracked(), range_end.get_untracked())
        } else {
            (String::new(), String::new())
        };
        let recent_value = if mode == "recent" && recent.get_untracked() > 0 {
            Some(recent.get_untracked())
        } else {
            None
        };
        let tag = tag_filter.get_untracked();
        let key = format!("{ledger_id}|{start_month}|{end_month}|{recent_value:?}|{tag}");
        // 去重状态在模块级（`STATS_DEDUPE`），因此**跨视图重建**也有效
        let needed =
            STATS_DEDUPE.with(|state| stats_request_needed(&mut state.borrow_mut(), &key, force));
        if !needed {
            // 同一份筛选已经请求过 / 正在飞行中 → 既不发请求，也**不要动 loading**
            // （这正是原来"卡在正在加载"的来源：视图被反复重建，每次都在飞行中又置 true）
            return;
        }
        loading.set(true);
        STATS_LOADING.with(|flag| flag.set(true));
        leptos::task::spawn_local(async move {
            match api::stock::statistics(&ledger_id, &start_month, &end_month, recent_value, &tag)
                .await
            {
                Ok(data) => {
                    // 数据落到**模块级缓存**：视图被重建后新挂载的那份也能拿到
                    STATS_DATA.with(|slot| *slot.borrow_mut() = Some(data.clone()));
                    // 信号**只在值真的变了时写**。`RwSignal::set` 同值也会通知，而统计页每次
                    // "从别的子功能切回来"都会复核一次缓存（见 ledger Effect 的 `!switched`
                    // 分支）—— 同值通知会重建整块图表、**入场动画重播**（正是过去
                    // "曲线一直在闪"的那个症状）。复核回来一模一样就什么都不做。
                    if stats.get_untracked().as_ref() != Some(&data) {
                        stats.set(Some(data));
                    }
                }
                Err(error) => {
                    notify_error("查询交易统计失败", &error);
                    // 失败时把去重键也清掉：没有「刷新」按钮了，失败后总得让下一次筛选
                    // （或重新进入本页）能再试一次，不能因为"已经请求过"就永远不再发。
                    STATS_DEDUPE.with(|state| state.borrow_mut().0.clear());
                }
            }
            // 飞行结束：把 in_flight 清掉（键保留，重复触发仍然不会再发）
            STATS_DEDUPE.with(|state| state.borrow_mut().1 = None);
            STATS_LOADING.with(|flag| flag.set(false));
            loading.set(false);
        });
    };

    let load_tags = move || {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            if let Ok(data) = api::stock::tag_settings_get(&ledger_id).await {
                // 落到模块级缓存（跨重建保留）+ **只在真的变了时写信号**：
                // `RwSignal::set` 同值也会通知，而 `tags` 被上层视图同步读着
                // （`Select.options` 要的是 `Vec`，调用方只能 `.get()`），
                // 于是"同值也通知"会变成一次上层重建 —— 那正是自激循环的燃料。
                let changed = STATS_TAGS.with(|slot| {
                    let mut slot = slot.borrow_mut();
                    if *slot == data.tags {
                        false
                    } else {
                        *slot = data.tags.clone();
                        true
                    }
                });
                if changed {
                    tags.set(data.tags);
                }
            }
        });
    };

    // 账本变化才重新初始化。判据是**模块级**的"已初始化账本"（不能用 Effect 自己的
    // `prev`：视图重建会重新创建这个 Effect，`prev` 每次都从 `None` 起步 —— 详见
    // `STATS_LEDGER` 的注释，那正是"曲线一直在闪"的根因）。
    Effect::new(move |_| {
        let ledger_id = stores.current_ledger_id.get();
        let switched = STATS_LEDGER.with(|slot| {
            let mut slot = slot.borrow_mut();
            if *slot == ledger_id {
                return false;
            }
            slot.clear();
            slot.push_str(&ledger_id);
            true
        });
        if !switched {
            // 视图重建（不是账本切换）：把**模块级缓存**同步回本次的信号。
            // 否则上一份 invocation 的飞行结果会落在已销毁的信号上，本次永远停在空态。
            stats.set(STATS_DATA.with(|data| data.borrow().clone()));
            loading.set(STATS_LOADING.with(|flag| flag.get()));
            tags.set(STATS_TAGS.with(|tags| tags.borrow().clone()));
            // 缓存**先显示、再复核**：`STATS_DATA` / `STATS_TAGS` 只在切账本与筛选变化时更新，
            // 而用户可能刚在别的子功能里下过单 / 改过成交 / 减过仓 / 清过仓 / 重置过股票数据 /
            // 改过交易标签 —— 手上这份就是那一轮之前的（真实缺陷：清仓后切到统计，刚清的那笔
            // 不在里面，得改一次筛选才刷出来）。
            // 复核一次的成本是一次只读聚合查询，比让人看到旧数字便宜得多。
            // 不用"改动点打脏标记"：那要求每个改动处都记得上报，漏一个就静默回到同一个坑里，
            // 而挂载时复核**在构造上不可能漏**（代价只是没改动时也查一次，见下面的去重）。
            load_tags();
            apply_filter(true);
            return;
        }
        // 账本切换（或首次进入）：清掉上一个账本的缓存
        reset_stats_cache();
        // 账本切换时重置筛选
        filter_mode.set("all".to_string());
        range_start.set(String::new());
        range_end.set(String::new());
        recent.set(0);
        recent_signal.set(String::new());
        tag_filter.set(String::new());
        if ledger_id.is_empty() {
            stats.set(None);
            return;
        }
        load_tags();
        apply_filter(false);
    });

    // 明细的页码：筛选条件或每页条数一变就回到第 1 页；数据变少时（切到笔数更少的筛选）
    // 把页码收敛到末页，避免停在一个不存在的页码上显示空表。
    // 页码一律用 `get_untracked` 读 —— 否则"本 Effect 写页码"会把自己再触发一次。
    Effect::new(move |prev: Option<(String, i32)>| {
        let key = format!(
            "{}|{}|{}|{}|{}",
            filter_mode.get(),
            range_start.get(),
            range_end.get(),
            recent.get(),
            tag_filter.get(),
        );
        let size = detail_page_size.get().max(1);
        let total = stats.get().map(|data| data.points.len()).unwrap_or(0) as i32;
        let pages = ((total + size - 1) / size).max(1);
        let reset = prev
            .as_ref()
            .map(|prev| prev.0 != key || prev.1 != size)
            .unwrap_or(true);
        if reset {
            detail_page.set(1);
        } else if detail_page.get_untracked() > pages {
            detail_page.set(pages);
        }
        (key, size)
    });

    let is_filtered = move || {
        (filter_mode.get() == "range" && !range_start.get().is_empty())
            || (filter_mode.get() == "recent" && recent.get() > 0)
            || !tag_filter.get().is_empty()
    };

    let latest_point = move || stats.get().and_then(|data| data.points.last().cloned());

    // 明细表的数据源：结算点按日期**倒序**（最近一笔在最上面），再按页切片。
    // "倒序"是为了让人一进明细分栏就先看到最近的结算 —— 曲线仍按**时序**画（正序）。
    let detail_rows = move || {
        let mut points = stats.get().map(|data| data.points).unwrap_or_default();
        points.reverse();
        points
    };
    let detail_total = move || detail_rows().len() as i32;
    let detail_total_pages = Signal::derive(move || {
        let size = detail_page_size.get().max(1);
        ((detail_total() + size - 1) / size).max(1)
    });
    let detail_page_rows = move || {
        let size = detail_page_size.get().max(1) as usize;
        let page = detail_page.get().max(1) as usize;
        detail_rows()
            .into_iter()
            .skip((page - 1) * size)
            .take(size)
            .collect::<Vec<_>>()
    };

    // 版心两块：工具栏（左侧分栏页签 + 右侧筛选组）/ 内容区（两个分栏各自填满内容区）
    //
    // ⚠ 筛选组**不能吃剩余宽度**：`stock-toolbar` 是 flex 行，给筛选组 `flex: 1` 时
    // 它会先把宽度占满，把左边的页签挤成一条。这里靠 `margin-left: auto` 把筛选组
    // 顶到右边，两边各按内容宽度排（见 stock.css）。
    let toolbar = view! {
        <div class="stock-toolbar">
            <Tabs
                active=tab
                items=vec![
                    TabItem::new(STATS_TAB_SUMMARY, "统计"),
                    TabItem::new(STATS_TAB_DETAIL, "明细"),
                ]
                class="stock-tabs"
            />
            <div class="stock-statistics__filters">
                <Segmented
                    value=filter_mode
                    options=vec![
                        SegmentedOption::new("all", "全部"),
                        SegmentedOption::new("range", "按时间"),
                        SegmentedOption::new("recent", "最近 N 笔"),
                    ]
                    on_change=UnsyncCallback::new(move |next: String| {
                        filter_mode.set(next.clone());
                        if next == "recent" && recent.get_untracked() == 0 {
                            recent.set(10);
                            recent_signal.set("10".to_string());
                        }
                        apply_filter(false);
                    })
                />
                <Show when=move || filter_mode.get() == "range">
                    <div class="stock-statistics__range">
                        <Input value=range_start placeholder="开始月份 YYYY-MM" />
                        <span class="stock-statistics__range-sep">"~"</span>
                        <Input value=range_end placeholder="结束月份 YYYY-MM" />
                    </div>
                </Show>
                <Show when=move || filter_mode.get() == "recent">
                    <div class="stock-statistics__recent">
                        <Select
                            value=recent_signal
                            options=vec![
                                SelectOption::new("10", "最近 10 笔"),
                                SelectOption::new("50", "最近 50 笔"),
                                SelectOption::new("100", "最近 100 笔"),
                            ]
                            on_change=UnsyncCallback::new(move |value: String| {
                                recent.set(value.parse().unwrap_or(10));
                                apply_filter(false);
                            })
                        />
                    </div>
                </Show>
                <div class="stock-statistics__tag">
                    // **选项必须在自己的动态块里读**：`Select.options` 是 `Vec`（不是信号），
                    // 调用方只能 `.get()` —— 若直接在工具栏里同步读 `tags`，就把它记成了
                    // **上层视图的依赖**，于是"标签列表一变 → 整个统计子视图重建 → 图表 SVG 重建
                    // → 入场动画重播"，而且重建会把飞行中的统计请求结果写进已销毁的信号。
                    // 放进这个 `move ||` 之后，`tags` 只由这一小块负责，重建范围仅限这个下拉。
                    {move || {
                        let mut options = vec![SelectOption::new("", "全部标签")];
                        options.extend(tags.get().into_iter().map(SelectOption::same));
                        view! {
                            <Select
                                value=tag_filter
                                options=options
                                on_change=UnsyncCallback::new(move |value: String| {
                                    tag_filter.set(value);
                                    apply_filter(false);
                                })
                            />
                        }
                    }}
                </div>
            </div>
        </div>
    }
    .into_any();

    let content = view! {
        <div class="stock-body">
        <div class="stock-statistics">
            <TabPane active=tab key=STATS_TAB_SUMMARY class="stock-statistics__pane">
            {move || {
                if stats.get().is_none() {
                    return view! {
                        <div class="stock-empty">
                            {move || if loading.get() { "正在加载…" } else { "暂无统计" }}
                        </div>
                    }
                        .into_any();
                }
                // 没有"最新一笔"就说明当前集合里一笔结算都没有：未筛选时是"还没清过仓"，
                // 筛选中则是筛选没命中 —— 两种文案分开说，别让人以为记录丢了。
                let Some(latest) = latest_point() else {
                    let filtered = is_filtered();
                    return view! {
                        <div class="stock-empty">
                            <Empty
                                title=if filtered { "当前筛选下暂无结算记录" } else { "还没有结算记录" }
                                description=if filtered {
                                    "换一个时间区间、笔数或标签再试。"
                                } else {
                                    "股票清仓后，这一轮从建仓到清仓会生成一笔结算；完成第一笔结算后即可看到统计与曲线。"
                                }
                            />
                        </div>
                    }
                        .into_any();
                };
                let filtered = is_filtered();
                view! {
                    <div class="stock-panel">
                        <div class="stock-panel__head">
                            <h4 class="stock-panel__title">"结算统计"</h4>
                        </div>

                        <div class="stock-stats-overview">
                            <div class="stock-stats-overview__lead">
                                <span class="stock-stats-overview__label">"已实现盈亏"</span>
                                <span class=format!(
                                    "stock-stats-overview__value {}",
                                    format::pnl_class(latest.total_pnl),
                                )>{format::signed_yuan(latest.total_pnl)}</span>
                                <span class="stock-stats-overview__sub">
                                    {format!(
                                        "截至 {} · 第 {} 笔结算{}",
                                        format_timestamp(latest.closed_at, "YYYY-MM-DD"),
                                        latest.sequence,
                                        if filtered { "（筛选内）" } else { "" },
                                    )}
                                </span>
                            </div>
                            <div class="stock-stats-overview__kpis">
                                <StatKpi
                                    label="胜率".to_string()
                                    value=Signal::derive(move || {
                                        format::rate_text(latest.win_rate)
                                    })
                                    class=Signal::derive(String::new)
                                    sub=Signal::derive(move || {
                                        format!(
                                            "{} 胜 · {} 负",
                                            latest.win_count, latest.loss_count,
                                        )
                                    })
                                />
                                <StatKpi
                                    label="实际盈亏比".to_string()
                                    value=Signal::derive(move || {
                                        format::ratio_text(latest.pnl_ratio)
                                    })
                                    class=Signal::derive(String::new)
                                    sub=Signal::derive(|| "盈利均值 ÷ 亏损均值".to_string())
                                />
                                <StatKpi
                                    label="期望值".to_string()
                                    value=Signal::derive(move || {
                                        format::signed_yuan(latest.expectancy)
                                    })
                                    class=Signal::derive(move || {
                                        format::pnl_class(latest.expectancy).to_string()
                                    })
                                    sub=Signal::derive(|| "每笔均值".to_string())
                                />
                                <StatKpi
                                    label="最大回撤".to_string()
                                    value=Signal::derive(move || {
                                        format::rate_text(latest.max_drawdown_pct)
                                    })
                                    class=Signal::derive(move || {
                                        if latest.max_drawdown_pct > 0.0 {
                                            "amount-expense".to_string()
                                        } else {
                                            String::new()
                                        }
                                    })
                                    sub=Signal::derive(move || {
                                        format!(
                                            "从高点回落 ¥{}",
                                            format::amount(latest.max_drawdown.max(0)),
                                        )
                                    })
                                />
                                <StatKpi
                                    label="平均盈利".to_string()
                                    value=Signal::derive(move || {
                                        if latest.avg_win > 0 {
                                            format!("¥{}", format::amount(latest.avg_win))
                                        } else {
                                            "—".to_string()
                                        }
                                    })
                                    class=Signal::derive(move || {
                                        if latest.avg_win > 0 {
                                            "amount-income".to_string()
                                        } else {
                                            String::new()
                                        }
                                    })
                                    sub=Signal::derive(move || {
                                        format!("{} 笔盈利", latest.win_count)
                                    })
                                />
                                <StatKpi
                                    label="平均亏损".to_string()
                                    value=Signal::derive(move || {
                                        if latest.avg_loss > 0 {
                                            format!("-¥{}", format::amount(latest.avg_loss))
                                        } else {
                                            "—".to_string()
                                        }
                                    })
                                    class=Signal::derive(move || {
                                        if latest.avg_loss > 0 {
                                            "amount-expense".to_string()
                                        } else {
                                            String::new()
                                        }
                                    })
                                    sub=Signal::derive(move || {
                                        format!("{} 笔亏损", latest.loss_count)
                                    })
                                />
                            </div>
                        </div>

                        <div class="stock-chart-block">
                            <div class="stock-panel__head">
                                <h4 class="stock-panel__title">"统计曲线"</h4>
                                <Segmented
                                    value=metric
                                    options={
                                        Metric::ALL
                                            .iter()
                                            .map(|item| SegmentedOption::same(item.label()))
                                            .collect::<Vec<_>>()
                                    }
                                    on_change=UnsyncCallback::new(move |label: String| {
                                        // `Segmented` 的值就是指标文案，这里反查回枚举
                                        if let Some(found) = Metric::ALL
                                            .iter()
                                            .find(|item| item.label() == label)
                                        {
                                            metric.set(found.label().to_string());
                                        }
                                    })
                                />
                            </div>
                            <div class="stock-chart-block__body">
                            {move || {
                                let Some(data) = stats.get() else {
                                    return ().into_any();
                                };
                                let selected = Metric::ALL
                                    .iter()
                                    .find(|item| item.label() == metric.get())
                                    .copied()
                                    .unwrap_or(Metric::TotalPnl);
                                let categories = data
                                    .points
                                    .iter()
                                    .map(|point| {
                                        format!(
                                            "第{}笔 {}",
                                            point.sequence,
                                            format_timestamp(point.closed_at, "MM-DD"),
                                        )
                                    })
                                    .collect::<Vec<_>>();
                                let values = data
                                    .points
                                    .iter()
                                    .map(|point| selected.value(point).round() as i64)
                                    .collect::<Vec<_>>();
                                let color = match selected {
                                    Metric::AvgWin | Metric::WinRate | Metric::PnlRatio => {
                                        "var(--transactions-color-expense)"
                                    }
                                    Metric::AvgLoss | Metric::MaxDrawdown => {
                                        "var(--transactions-color-income)"
                                    }
                                    _ => {
                                        let last = values.last().copied().unwrap_or(0);
                                        if last >= 0 {
                                            "var(--transactions-color-expense)"
                                        } else {
                                            "var(--transactions-color-income)"
                                        }
                                    }
                                };
                                let config = ChartConfig::default()
                                    .height(320)
                                    .value_kind(selected.kind())
                                    .y_title("金额（元）");
                                let config = if selected.has_reference() {
                                    config.reference(0)
                                } else {
                                    config
                                };
                                view! {
                                    <LineChart
                                        categories=Signal::derive(move || categories.clone())
                                        series=Signal::derive(move || {
                                            vec![ChartSeries::new(
                                                selected.label(),
                                                color,
                                                values.clone(),
                                            )]
                                        })
                                        config=config
                                    />
                                }
                                    .into_any()
                            }}
                            </div>
                        </div>
                    </div>
                }
                    .into_any()
            }}
            </TabPane>

            <TabPane active=tab key=STATS_TAB_DETAIL class="stock-statistics__pane">
            {move || {
                if stats.get().is_none() {
                    return view! {
                        <div class="stock-empty">
                            {move || if loading.get() { "正在加载…" } else { "暂无统计" }}
                        </div>
                    }
                        .into_any();
                }
                // 按结算日期**倒序**（最近一笔在最上面）；分页在界面侧切，
                // 理由见 `statistics_view` 的文档注释（累计口径必须整条算出来）。
                let rows = detail_page_rows();
                let empty = rows.is_empty();
                let latest_sequence = latest_point().map(|latest| latest.sequence);
                view! {
                    <div class="stock-panel">
                        <div class="stock-panel__head">
                            <h4 class="stock-panel__title">"逐笔结算明细"</h4>
                            <span class="stock-panel__hint">
                                "按结算日期倒序；每一行 = 结算到该笔时的累计结果"
                            </span>
                        </div>
                        <div class="stock-table-wrap" class:stock-table-wrap--empty=empty>
                            <table class="stock-table stock-table--stats">
                                <thead>
                                    <tr>
                                        <th class="is-center">"结算点"</th>
                                        <th class="is-center">"结算日期"</th>
                                        <th class="is-center">"标签"</th>
                                        <th class="is-center">"本笔盈亏"</th>
                                        <th class="is-center">"累计盈亏"</th>
                                        <th class="is-center">"胜负"</th>
                                        <th class="is-center">"胜率"</th>
                                        <th class="is-center">"平均盈利"</th>
                                        <th class="is-center">"平均亏损"</th>
                                        <th class="is-center">"实际盈亏比"</th>
                                        <th class="is-center">"期望值"</th>
                                        <th class="is-center">"最大回撤"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {if empty {
                                        view! {
                                            <tr>
                                                <td colspan="12">
                                                    <Empty
                                                        title="当前筛选下暂无结算记录"
                                                        description="放宽时间区间、笔数或标签即可看到逐笔明细。"
                                                    />
                                                </td>
                                            </tr>
                                        }
                                            .into_any()
                                    } else {
                                        rows
                                            .into_iter()
                                            .map(|point| {
                                                let is_latest = latest_sequence
                                                    == Some(point.sequence);
                                                statistics_detail_row(point, is_latest)
                                            })
                                            .collect_view()
                                            .into_any()
                                    }}
                                </tbody>
                            </table>
                        </div>
                        <div class="stock-panel__footer">
                            <span class="stock-panel__total">
                                {move || format!("共 {} 条", detail_total())}
                            </span>
                            // 没有数据时不渲染分页控件（只剩一个禁用态的「1」很吵）；
                            // 注意别在 `view!` 的属性里写 `>`，它会被当成标签结束符
                            <Show when=move || detail_total().is_positive()>
                                <Pagination
                                    page=detail_page
                                    total_pages=detail_total_pages
                                    page_size=detail_page_size
                                />
                            </Show>
                        </div>
                    </div>
                }
                    .into_any()
            }}
            </TabPane>
        </div>
        </div>
    }
    .into_any();

    view! {
        <FeaturePage
            title=PAGE_TITLE
            class="stock-page"
            rail=view! { <StockSubRail sub=sub /> }.into_any()
            toolbar=toolbar
            content=content
        />
    }
    .into_any()
}

/// 逐笔结算明细表的一行（统计 → 明细分栏）。
///
/// 一行展示的是**结算到该笔时的累计结果**（累计口径由服务端按整条时序算好，
/// 见 `tr-service/src/stock_statistics.rs`），界面只负责排版与语义色。
/// `is_latest` 标记当前集合的最后一笔（面板里那串大数的来源）。
fn statistics_detail_row(point: StockStatisticsPointDto, is_latest: bool) -> AnyView {
    view! {
        <tr class:is-latest=is_latest>
            <td class="is-center">
                <div class="stock-stats-point">
                    <span>{format!("第 {} 笔", point.sequence)}</span>
                    <span class="stock-stats-point__sub">
                        {format!("{} 第 {} 轮", point.stock_name, point.stock_round_no)}
                    </span>
                </div>
            </td>
            <td class="is-center stock-mono">
                {format_timestamp(point.closed_at, "YYYY-MM-DD")}
            </td>
            <td class="is-center">
                <span class="stock-cell-tag">{point.tag.clone()}</span>
            </td>
            <td class="is-right stock-mono">
                {format!(
                    "{} {}",
                    format::signed_yuan(point.pnl),
                    format::rate_text(point.pnl_rate),
                )}
            </td>
            <td class="is-right">
                <span class=format!(
                    "stock-amount {}",
                    format::pnl_class(point.total_pnl),
                )>{format::signed_yuan(point.total_pnl)}</span>
            </td>
            <td class="is-center">{format!("{} 胜 {} 负", point.win_count, point.loss_count)}</td>
            <td class="is-center">{format::rate_text(point.win_rate)}</td>
            <td class="is-right">
                {if point.avg_win > 0 {
                    format!("¥{}", format::amount(point.avg_win))
                } else {
                    "—".to_string()
                }}
            </td>
            <td class="is-right">
                {if point.avg_loss > 0 {
                    format!("-¥{}", format::amount(point.avg_loss))
                } else {
                    "—".to_string()
                }}
            </td>
            <td class="is-center">{format::ratio_text(point.pnl_ratio)}</td>
            <td class="is-right">
                <span class=format!(
                    "stock-amount {}",
                    format::pnl_class(point.expectancy),
                )>{format::signed_yuan(point.expectancy)}</span>
            </td>
            <td class="is-center">
                {format!(
                    "{} ¥{}",
                    format::rate_text(point.max_drawdown_pct),
                    format::amount(point.max_drawdown),
                )}
            </td>
        </tr>
    }
    .into_any()
}

/// 一个统计 KPI 格（标签 + 值 + 副标题）。
#[component]
fn StatKpi(
    label: String,
    #[prop(into)] value: Signal<String>,
    #[prop(into)] class: Signal<String>,
    #[prop(into)] sub: Signal<String>,
) -> impl IntoView {
    view! {
        <div class="stock-kpi">
            <span class="stock-kpi__label">{label}</span>
            <span class=move || format!("stock-kpi__value {}", class.get())>
                {move || value.get()}
            </span>
            <span class="stock-kpi__sub">{move || sub.get()}</span>
        </div>
    }
}

// ==================================================================== 子功能五：设置

/// 交易费用说明（「交易费用设置」标题旁的说明浮层；与下单弹窗的预估同口径）。
const FEE_TOOLTIP: &str = "佣金：委托成交总额 × 费率，不足最低佣金时按最低佣金收取（买卖双向）\n一笔委托分多笔成交时，费用按委托成交总额计算一次，再按各笔成交金额比例分摊\n买入实际成本 = 成交金额 + 佣金 + 过户费";
/// 印花税说明（固定文案）。
const STAMP_TOOLTIP: &str = "卖出时按成交金额 × 费率收取";
/// 过户费说明（固定文案）。
const TRANSFER_TOOLTIP: &str = "买卖双向收取，仅沪市（60/68 开头）适用";
/// 交易标签最多保存数量（上限 20）。
const MAX_STOCK_TAGS: usize = 20;

/// 设置子功能：交易标签 + 交易费用设置 + 操作记录（查看 / 回滚）+ 重置股票数据。
///
/// 这是原「应用设置 → 股票」整块迁进来的（它属于股票事务，不属于应用配置）：
/// 费用设置与标签都是**按账本**存的，放在这里与下单、复盘、统计同屏更顺手。
/// 「保存」是费用表单的提交键，放工具栏；标签的增删是即存即生效，不在工具栏里。
fn settings_view(sub: RwSignal<StockSub>) -> AnyView {
    let stores = AppStores::global();

    // ---- 费用设置 ----
    let fee_commission = RwSignal::new(String::new());
    let fee_min = RwSignal::new(String::new());
    let fee_stamp = RwSignal::new(String::new());
    let fee_transfer = RwSignal::new(String::new());
    let fee_saving = RwSignal::new(false);

    // ---- 交易标签 ----
    let tags = RwSignal::new(Vec::<String>::new());
    let default_tag = RwSignal::new(String::new());
    let tags_loading = RwSignal::new(false);
    let tags_saving = RwSignal::new(false);
    let new_tag = RwSignal::new(String::new());

    // ---- 操作记录 / 回滚 ----
    // 最多保留的条数（与后端 `STOCK_OPERATION_LIMIT` 同口径；界面只用于说明文案）
    const OPERATION_LIMIT_TEXT: &str = "最多保留最近 10 次操作";
    let operations = RwSignal::new(Vec::<StockOperationDto>::new());
    let operations_loading = RwSignal::new(false);
    // 「查看记录」弹窗
    let records_open = RwSignal::new(false);
    // 「回滚」确认弹窗：预演结果（要撤销哪一次 + 会失效的轮次）
    let rollback_preview = RwSignal::new(Option::<StockOperationRollbackPreviewDto>::None);
    let rolling_back = RwSignal::new(false);

    // ---- 重置 ----
    let confirm_open = RwSignal::new(false);
    let resetting = RwSignal::new(false);

    let no_ledger = move || stores.current_ledger_id.get().is_empty();

    // 回填：佣金 ×10000、最低佣金（分→元）、印花税/过户费 ×100
    let fill_fee_form = move |setting: &StockFeeSetting| {
        fee_commission.set(format_scaled(setting.commission_rate, 10_000.0, 4));
        fee_min.set(cents_to_yuan(setting.min_commission));
        fee_stamp.set(format_scaled(setting.stamp_duty_rate, 100.0, 3));
        fee_transfer.set(format_scaled(setting.transfer_fee_rate, 100.0, 3));
    };

    let load_fee = move |ledger_id: String| {
        if ledger_id.is_empty() {
            fee_commission.set(String::new());
            fee_min.set(String::new());
            fee_stamp.set(String::new());
            fee_transfer.set(String::new());
            return;
        }
        leptos::task::spawn_local(async move {
            match api::stock::fee_settings_get(&ledger_id).await {
                Ok(setting) => fill_fee_form(&setting),
                Err(error) => notify_error("读取费用设置失败", &error),
            }
        });
    };

    // ---- 操作记录 ----
    let load_operations = move |ledger_id: String| {
        if ledger_id.is_empty() {
            operations.set(Vec::new());
            return;
        }
        operations_loading.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::operation_list(&ledger_id).await {
                Ok(list) => operations.set(list),
                Err(error) => notify_error("读取操作记录失败", &error),
            }
            operations_loading.set(false);
        });
    };

    // 「查看记录」：先确保拿到最新的一份（本页挂载时已经拉过，这里只是刷新）
    let open_records = move || {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            Notifier::global().error("请先选择工作空间", None);
            return;
        }
        records_open.set(true);
        load_operations(ledger_id);
    };

    // 「回滚」：**先预演再确认**（回滚清仓会连带移除该轮次与它的复盘，不可恢复）
    let open_rollback = move || {
        if rolling_back.get_untracked() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            Notifier::global().error("请先选择工作空间", None);
            return;
        }
        rolling_back.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::operation_preview(&ledger_id).await {
                Ok(preview) => rollback_preview.set(Some(preview)),
                Err(error) => notify_error("回滚预演失败", &error),
            }
            rolling_back.set(false);
        });
    };

    let do_rollback = move || {
        if rolling_back.get_untracked() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            Notifier::global().error("请先选择工作空间", None);
            return;
        }
        rolling_back.set(true);
        leptos::task::spawn_local(async move {
            let reload = ledger_id.clone();
            match api::stock::operation_rollback(&ledger_id).await {
                Ok(result) => {
                    rollback_preview.set(None);
                    if result.skipped {
                        // 目标已被别的改动删掉：只把记录弹掉，没有实际回滚
                        Notifier::global()
                            .warning(format!("「{}」已不存在，已跳过", result.action), None);
                    } else {
                        Notifier::global().success(format!("已回滚「{}」", result.action), None);
                    }
                    load_operations(reload);
                }
                Err(error) => notify_error("回滚失败", &error),
            }
            rolling_back.set(false);
        });
    };

    let save_fee = move || {
        if fee_saving.get_untracked() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            Notifier::global().error("请先选择工作空间", None);
            return;
        }

        // 校验顺序与文案固定（改动即影响界面）
        let Some(commission) = parse_number(&fee_commission.get_untracked()) else {
            Notifier::global().error("请输入大于 0 的佣金费率", None);
            return;
        };
        if commission <= 0.0 {
            Notifier::global().error("请输入大于 0 的佣金费率", None);
            return;
        }
        let min_text = fee_min.get_untracked();
        let Some(min_commission_yuan) = parse_number(&min_text) else {
            Notifier::global().error("请输入不小于 0 的最低佣金", None);
            return;
        };
        if min_commission_yuan < 0.0 {
            Notifier::global().error("请输入不小于 0 的最低佣金", None);
            return;
        }
        let Some(stamp_duty) = parse_number(&fee_stamp.get_untracked()) else {
            Notifier::global().error("印花税与过户费需不小于 0", None);
            return;
        };
        let Some(transfer_fee) = parse_number(&fee_transfer.get_untracked()) else {
            Notifier::global().error("印花税与过户费需不小于 0", None);
            return;
        };
        if stamp_duty < 0.0 || transfer_fee < 0.0 {
            Notifier::global().error("印花税与过户费需不小于 0", None);
            return;
        }

        // 元 → 分必须走 `tr_domain::money`
        let min_commission = match yuan_to_cents(&min_text) {
            Ok(cents) => cents,
            Err(_) => {
                Notifier::global().error("请输入不小于 0 的最低佣金", None);
                return;
            }
        };

        fee_saving.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::fee_settings_put(
                &ledger_id,
                commission / 10_000.0,
                min_commission,
                stamp_duty / 100.0,
                transfer_fee / 100.0,
            )
            .await
            {
                Ok(setting) => {
                    // 保存后以后端返回为准回填
                    fill_fee_form(&setting);
                    Notifier::global().success("费用设置已保存", None);
                }
                Err(error) => notify_error("保存费用设置失败", &error),
            }
            fee_saving.set(false);
        });
    };

    // ---- 交易标签 ----
    let load_tags = move |ledger_id: String| {
        if ledger_id.is_empty() {
            tags.set(Vec::new());
            default_tag.set(String::new());
            tags_loading.set(false);
            return;
        }
        tags_loading.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::tag_settings_get(&ledger_id).await {
                Ok(setting) => {
                    tags.set(setting.tags);
                    default_tag.set(setting.default_tag);
                }
                Err(error) => notify_error("读取交易标签失败", &error),
            }
            tags_loading.set(false);
        });
    };

    let save_tags = move |next: Vec<String>, success: Option<String>| {
        if tags_saving.get_untracked() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            Notifier::global().error("请先选择工作空间", None);
            return;
        }
        tags_saving.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::tag_settings_put(&ledger_id, next).await {
                Ok(setting) => {
                    // 保存后以后端返回的 tags 为准
                    tags.set(setting.tags);
                    default_tag.set(setting.default_tag);
                    if let Some(text) = success {
                        Notifier::global().success(text, None);
                        new_tag.set(String::new());
                    }
                }
                Err(error) => notify_error("保存交易标签失败", &error),
            }
            tags_saving.set(false);
        });
    };

    let add_tag = move || {
        if tags_saving.get_untracked() {
            return;
        }
        if stores.current_ledger_id.get_untracked().is_empty() {
            Notifier::global().error("请先选择工作空间", None);
            return;
        }
        let next = new_tag.get_untracked().trim().to_string();
        if next.is_empty() {
            return;
        }
        if tags.with(|list| list.contains(&next)) {
            Notifier::global().error(format!("标签「{next}」已存在"), None);
            return;
        }
        if tags.with(Vec::len) >= MAX_STOCK_TAGS {
            Notifier::global().error("最多保存 20 个标签，请先删除不再需要的标签", None);
            return;
        }
        let mut list = tags.get_untracked();
        list.push(next.clone());
        save_tags(list, Some(format!("标签「{next}」已添加")));
    };

    let remove_tag = move |tag: String| {
        if tags_saving.get_untracked() || tag == default_tag.get_untracked() {
            return;
        }
        if stores.current_ledger_id.get_untracked().is_empty() {
            Notifier::global().error("请先选择工作空间", None);
            return;
        }
        let next: Vec<String> = tags
            .get_untracked()
            .into_iter()
            .filter(|item| item != &tag)
            .collect();
        save_tags(next, Some(format!("标签「{tag}」已删除")));
    };

    // ---- 重置 ----
    let do_reset = move || {
        if resetting.get_untracked() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            Notifier::global().error("重置股票数据失败", Some("请先选择工作空间".to_string()));
            return;
        }
        resetting.set(true);
        leptos::task::spawn_local(async move {
            match api::stock::reset(&ledger_id).await {
                Ok(_) => {
                    confirm_open.set(false);
                    // 重置会清掉费用设置与交易标签，重新拉一遍
                    load_fee(ledger_id.clone());
                    load_tags(ledger_id);
                    Notifier::global().success("股票数据已重置", None);
                }
                Err(error) => notify_error("重置股票数据失败", &error),
            }
            resetting.set(false);
        });
    };

    // 确认框正文（多行，用 `white-space: pre-wrap` 的 `.stock-impact__text` 渲染）
    let rollback_body = move || {
        let Some(preview) = rollback_preview.get() else {
            return String::new();
        };
        format!(
            "将撤销最新一次操作「{}」（{}）。\n{}\n资金记录、轮次与统计都会按新数据重算。\n回滚后不能撤销。",
            preview.action,
            preview.detail,
            // 与「编辑/删除成交」的影响预演同一份文案：没有失效轮次时明说「不会影响」，
            // 让人一眼知道这次回滚不会丢复盘。
            removed_rounds_text(&preview.removed_rounds),
        )
    };

    // 账本变化 → 重新加载费用设置、交易标签与操作记录
    Effect::new(move |_: Option<()>| {
        let ledger_id = stores.current_ledger_id.get();
        load_fee(ledger_id.clone());
        load_tags(ledger_id.clone());
        load_operations(ledger_id);
    });

    let tag_empty_text = move || {
        if tags_loading.get() {
            "正在加载…".to_string()
        } else if stores.current_ledger_id.get().is_empty() {
            "选择工作空间后即可配置交易标签".to_string()
        } else {
            "暂无标签".to_string()
        }
    };

    // 本子功能**没有页面级操作**：「保存」是费用表单的提交键，属于那张卡片，
    // 所以不给 `FeaturePage` 的 `toolbar`（空工具栏只会白占一条发丝线，
    // 而"保存"孤零零挂在页头、离它要保存的表单很远）。
    let content = view! {
        <div class="stock-body">
        <div class="stock-settings">
            <div class="st-list">
                // ---- 交易标签 ----
                <div class="st-card st-card--block">
                    <div class="st-tag-head">
                        <div class="st-card-info">
                            <span class="st-card-title">"交易标签"</span>
                            <span class="st-card-desc">
                                "每轮交易可选（最多 20 个、单个不超过 8 字）；删除不影响历史记录。"
                            </span>
                        </div>
                        <div class="st-tag-action">
                            <div class="st-tag-input">
                                <Input
                                    value=new_tag
                                    placeholder="如：低吸"
                                    maxlength=8
                                    allow_clear=true
                                    on_enter=move || add_tag()
                                />
                            </div>
                            <Button
                                variant=ButtonVariant::Primary
                                loading=tags_saving
                                disabled=Signal::derive(move || {
                                    new_tag.get().trim().is_empty()
                                        || tags.with(Vec::len) >= MAX_STOCK_TAGS
                                })
                                on_click=move || add_tag()
                            >
                                "添加"
                            </Button>
                        </div>
                    </div>

                    <div class="st-tag-list">
                        {move || {
                            let list = tags.get();
                            if list.is_empty() {
                                view! {
                                    <div class="st-tag-empty">{tag_empty_text()}</div>
                                }
                                    .into_any()
                            } else {
                                let default = default_tag.get();
                                list
                                    .into_iter()
                                    .map(|tag| {
                                        let is_default = tag == default;
                                        let for_title = tag.clone();
                                        let name = tag.clone();
                                        let aria = format!("删除标签 {tag}");
                                        // 回调先建好：`UnsyncCallback` 是 Copy，
                                        // `Show` 的 children 要求 `Fn`，不能把 String move 进去
                                        let remove_click =
                                            UnsyncCallback::new(move |()| remove_tag(tag.clone()));
                                        view! {
                                            <span
                                                class="st-tag-item"
                                                class:st-tag-item--default=is_default
                                                title=if is_default {
                                                    "默认标签，不可删除".to_string()
                                                } else {
                                                    for_title
                                                }
                                            >
                                                <span class="st-tag-item-name">{name}</span>
                                                <Show when=move || is_default>
                                                    <span class="st-tag-item-default">"默认"</span>
                                                </Show>
                                                <Show when=move || !is_default>
                                                    <button
                                                        type="button"
                                                        class="st-tag-item-remove"
                                                        disabled=move || tags_saving.get()
                                                        aria-label=aria.clone()
                                                        on:click=move |_| remove_click.run(())
                                                    >
                                                        {icons::icon(Icon::Close)}
                                                    </button>
                                                </Show>
                                            </span>
                                        }
                                    })
                                    .collect_view()
                                    .into_any()
                            }
                        }}
                    </div>
                </div>

                // ---- 交易费用设置 ----
                <div class="st-card st-card--block">
                    <div class="st-panel-head">
                        <div class="st-panel-title-row">
                            <h3 class="st-panel-title">"交易费用设置"</h3>
                            // 这个图标在面板**左半边**：气泡左对齐向右铺开（右对齐会往左伸出面板压到侧栏）
                            <Tooltip title=FEE_TOOLTIP class="st-fee-tip--start">
                                <span class="st-panel-tip" aria-label="查看交易费用说明">
                                    {icons::icon(Icon::InfoCircle)}
                                </span>
                            </Tooltip>
                        </div>
                        <Button
                            variant=ButtonVariant::Primary
                            loading=fee_saving
                            disabled=Signal::derive(no_ledger)
                            on_click=move || save_fee()
                        >
                            "保存"
                        </Button>
                    </div>

                    <Form layout=FormLayout::Vertical class="st-fee-form">
                        <FormItem label="佣金费率">
                            <div class="st-fee-field">
                                <Input
                                    value=fee_commission
                                    placeholder="如 2.354"
                                    disabled=Signal::derive(no_ledger)
                                />
                                <span class="st-fee-addon">"万分之"</span>
                            </div>
                        </FormItem>
                        <FormItem label="最低佣金">
                            <div class="st-fee-field">
                                <Input
                                    value=fee_min
                                    placeholder="如 5"
                                    disabled=Signal::derive(no_ledger)
                                />
                                <span class="st-fee-addon">"元/委托"</span>
                            </div>
                        </FormItem>
                        <FormItem label="印花税">
                            <div class="st-fee-field">
                                <Input
                                    value=fee_stamp
                                    placeholder="如 0.05"
                                    disabled=Signal::derive(no_ledger)
                                />
                                <span class="st-fee-addon">"%"</span>
                                <Tooltip title=STAMP_TOOLTIP class="st-fee-tip">
                                    <span class="st-panel-tip" aria-label="印花税说明">
                                        {icons::icon(Icon::InfoCircle)}
                                    </span>
                                </Tooltip>
                            </div>
                        </FormItem>
                        <FormItem label="过户费">
                            <div class="st-fee-field">
                                <Input
                                    value=fee_transfer
                                    placeholder="如 0.001"
                                    disabled=Signal::derive(no_ledger)
                                />
                                <span class="st-fee-addon">"%"</span>
                                <Tooltip title=TRANSFER_TOOLTIP class="st-fee-tip">
                                    <span class="st-panel-tip" aria-label="过户费说明">
                                        {icons::icon(Icon::InfoCircle)}
                                    </span>
                                </Tooltip>
                            </div>
                        </FormItem>
                    </Form>

                    <Show when=move || no_ledger()>
                        <p class="page-hint">"请先选择工作空间"</p>
                    </Show>
                </div>

                // ---- 操作记录 / 回滚 ----
                // 一行两个**同级**动作：左边看一眼记录，右边撤销最新一次。
                // 危险语义不放在按钮颜色上（两者是同一行的两种动作），而是放在确认框的确认键上。
                <div class="st-card">
                    <div class="st-card-info">
                        <span class="st-card-title">"操作记录"</span>
                        <span class="st-card-desc">
                            "追加本金 / 支取 / 利息归本 / 建仓 / 加仓 / 减仓 / 清仓 各记一条，最多保留最近 10 条；回滚撤销最新一次操作。"
                        </span>
                    </div>
                    <div class="st-card-action">
                        <Button
                            variant=ButtonVariant::Secondary
                            disabled=Signal::derive(move || {
                                no_ledger() || operations.get().is_empty()
                            })
                            on_click=move |_| open_records()
                        >
                            "查看记录"
                        </Button>
                        <Button
                            variant=ButtonVariant::Secondary
                            loading=rolling_back
                            disabled=Signal::derive(move || {
                                no_ledger() || operations.get().is_empty()
                            })
                            on_click=move |_| open_rollback()
                        >
                            "回滚"
                        </Button>
                    </div>
                </div>

                // ---- 重置 ----
                <div class="st-card">
                    <div class="st-card-info">
                        <span class="st-card-title">"重置"</span>
                        <span class="st-card-desc">
                            "清空当前账本的股票数据（账户本金、持仓、交易记录、资金记录、费用设置与交易标签），此操作不可恢复。"
                        </span>
                    </div>
                    <div class="st-card-action">
                        <Button
                            variant=ButtonVariant::PrimaryDanger
                            disabled=Signal::derive(no_ledger)
                            on_click=move || confirm_open.set(true)
                        >
                            "重置"
                        </Button>
                    </div>
                </div>
            </div>

            <Modal
                open=confirm_open
                title="重置股票数据"
                size=ModalSize::Small
                ok_text="确认重置"
                cancel_text="取消"
                ok_danger=true
                ok_loading=resetting
                on_close=move || confirm_open.set(false)
                on_ok=move || do_reset()
            >
                <p class="st-modal-text">
                    "将清空当前账本的账户本金、持仓、交易记录、资金记录、费用设置与交易标签。此操作不可恢复，确定继续吗？"
                </p>
            </Modal>

            // 「查看记录」：只读列表（`footer=false` —— 只读弹窗不需要确认键，
            // 关闭入口用底部的「关闭」按钮 + header 的 ×，不用「取消/确认」两个语义都不对的键）
            <Modal
                open=records_open
                title="操作记录"
                size=ModalSize::Medium
                footer=false
                on_close=move || records_open.set(false)
            >
                {move || {
                    let list = operations.get();
                    if list.is_empty() {
                        return view! {
                            <p class="page-hint">"还没有可回滚的操作。"</p>
                        }
                            .into_any();
                    }
                    view! {
                        <div class="stock-op-list">
                            {list
                                .into_iter()
                                .enumerate()
                                .map(|(index, operation)| {
                                    let is_latest = index == 0;
                                    view! {
                                        <div class="stock-op-item">
                                            <div class="stock-op-item__head">
                                                <span class="stock-op-item__action">
                                                    {operation.action.clone()}
                                                </span>
                                                <Show when=move || is_latest>
                                                    <span class="stock-op-item__badge">"最新"</span>
                                                </Show>
                                                <span class="stock-op-item__time">
                                                    {format_timestamp(
                                                        operation.created_at,
                                                        "MM-DD HH:mm",
                                                    )}
                                                </span>
                                            </div>
                                            <span class="stock-op-item__detail">
                                                {operation.detail.clone()}
                                            </span>
                                        </div>
                                    }
                                })
                                .collect_view()}
                        </div>
                    }
                        .into_any()
                }}
                <p class="page-hint">
                    {format!("{OPERATION_LIMIT_TEXT}；「回滚」只撤销最新那一条。")}
                </p>
                <div class="stock-op-actions">
                    <Button
                        variant=ButtonVariant::Secondary
                        on_click=move |_| records_open.set(false)
                    >
                        "关闭"
                    </Button>
                </div>
            </Modal>

            // 「回滚」确认框：复用编辑/删除成交那套「失效轮次」文案（回滚清仓会丢复盘）
            <Modal
                open=Signal::derive(move || rollback_preview.get().is_some())
                title="回滚操作"
                size=ModalSize::Small
                ok_text="确认回滚"
                cancel_text="取消"
                ok_danger=true
                ok_loading=rolling_back
                on_close=move || rollback_preview.set(None)
                on_ok=move || do_rollback()
            >
                <p class="stock-impact__text">{move || rollback_body()}</p>
            </Modal>
        </div>
        </div>
    }
    .into_any();

    view! {
        <FeaturePage
            title=PAGE_TITLE
            class="stock-page"
            rail=view! { <StockSubRail sub=sub /> }.into_any()
            content=content
        />
    }
    .into_any()
}

// ---------------------------------------------------------------- 工具函数

/// 按 `scale` 换算并格式化：
/// 先按 `scale` 换算，再四舍五入到 `digits` 位小数，最后去掉多余的 0。
fn format_scaled(value: f64, scale: f64, digits: i32) -> String {
    let factor = 10_f64.powi(digits);
    let rounded = (value * scale * factor).round() / factor;
    format!("{rounded}")
}

/// 解析用户输入的数值：非数字 / 非有限值（`NaN` / `inf`）视为非法。
fn parse_number(input: &str) -> Option<f64> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.parse::<f64>() {
        Ok(value) if value.is_finite() => Some(value),
        _ => None,
    }
}
