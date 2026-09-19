//! 消费记录页（P6-a 完整版：只读列表 + 全部写操作）。
//!
//! ## 文件结构
//!
//! | 组成部分 | 说明 |
//! |---|---|
//! | [`TransactionsPage`] | 页面编排：工具栏 / 时间范围 / 分页 / 空态三态 / 三个悬浮按钮 / 关联弹窗 |
//! | [`table_view`] / [`row_view`] | 表格：8 列（日期/类型/分类/标签/描述/金额/标记/操作）、列宽、行底色、行内「编辑 / 关联 / 同步 / 删除」 |
//! | [`record_modal`] | 记一笔 / 编辑（模板套用、类型→分类→标签联动、离群值） |
//! | [`sort_modal`] / [`sort_row_view`] | 排序弹窗及其每一行 |
//! | [`filter_modal`] | 筛选弹窗 |
//! | [`TrTimeRangePicker`] | 时间范围选择：预设「今天 / 本周 / 本月 / 上周 / 上月 / 今年」+ 日 / 月 / 年粒度 |
//! | [`empty_state_view`] | 空态三态（加载中 / 查询失败 / 无记录引导） |
//!
//! 查询参数：每页 `pageSize = 15`、默认排序 `transactionAt desc`；
//! 时间范围默认「今天」+ 粒度「日」。
//! [`month_bounds`] / [`add_month`] / [`week_monday`] 等是纯整数日期算术；
//! 数据访问统一走 `crate::api::tr::*` 封装。
//!
//! ## 设计取舍
//!
//! 1. **查询错误前缀**：[`QUERY_ERROR_PREFIX`] 定为「查询失败」且被别处引用，故保留该常量不动。
//! 2. **编辑 = 先建后删**：先创建新记录、成功后再删除旧记录，以避免旧记录被删除后新记录
//!    创建失败造成数据丢失；删除失败时回滚新建。顺序、回滚与提示文案都按此实现。
//! 3. **保存按钮文案**是「确认」。
//! 4. **保存为模板**：记一笔弹窗内另有「保存为模板」子弹窗，只填名称；
//!    模板本身的编辑与排序不在本任务范围内。
//! 5. **标签多选**：`ui::Select` 是单选（值 `String`），多选改用本文件的 [`TrCheckList`]
//!    （UI 上是一个勾选面板）。
//! 6. **时间范围面板**：`ui::DateRangePicker` 没有 presets 参数，故自绘一排链接按钮
//!    （今天/本周/本月/上周/上月/今年）。
//! 7. **date 粒度下的「周」**：没有独立「周」粒度，而是当区间跨 6 天时按整周翻页；
//!    判定见 [`shift_period`]。
//! 8. **同步到其他账本**：IPC 里没有 sync 命令，界面复制一份 DTO（换目标账本、清空 id
//!    让后端生成新 id），源记录保留。
//!
//! ## 关键纪律
//!
//! * 金额一律走 `tr_domain::money`（元 ⇄ 分），时间显示走 [`crate::time`]。
//! * 查询统一走 `api::tr::query`，参数由 `api::tr::default_condition` 打底再覆盖
//!   `ts_range` / `items` / `sort_fields`。
//! * 绝不 `unwrap()` 用户数据：所有解析失败都走 `Option`/`Result` 分支。

use std::collections::BTreeMap;

use leptos::prelude::*;
use leptos::tachys::view::any_view::{AnyView, IntoAny};
use tr_domain::dto::{
    CategoryDto, QueryConditionSortField, TagDto, TrQueryResult, TransactionRecordDto,
    TransactionTemplateDto,
};
use tr_domain::models::QueryConditionItem;
use tr_domain::money::yuan_to_cents;

use crate::api;
use crate::components::ui::{
    Button, ButtonSize, ButtonVariant, CheckboxGroup, DatePicker, DateRangePicker, Empty,
    FloatButton, Form, FormItem, FormLayout, Input, Modal, Pagination, Segmented, SegmentedOption,
    Select, SelectOption, Spin, Tag, TagKind,
};
use crate::error_handler::notify_error;
use crate::format;
use crate::icons::{self, Icon};
use crate::notify::Notifier;
use crate::store::AppStores;
use crate::time::{format_timestamp, today_ymd, ymd_to_seconds};

/// 页面标题（固定文案，改动即影响界面）
pub const PAGE_TITLE: &str = "消费记录";
/// 查询错误前缀（此常量由 P5 定稿并被别处引用，故保留）
pub const QUERY_ERROR_PREFIX: &str = "查询失败";

/// 默认每页条数（15 条 / 页）
const DEFAULT_PAGE_SIZE: i32 = 15;
/// 每页条数可选项：15 / 30 / 50 / 100
const PAGE_SIZE_OPTIONS: [i32; 4] = [15, 30, 50, 100];

/// 交易类型分段选项（收入 / 支出 / 转账）。
const TRANSACTION_TYPES: [(&str, &str); 3] = [
    ("income", "收入"),
    ("expense", "支出"),
    ("transfer", "转账"),
];

/// 时间范围粒度标签（日 / 月 / 年）。
const TIME_RANGE_MODES: [(&str, &str); 3] = [("date", "日"), ("month", "月"), ("year", "年")];

/// 排序字段选项（日期 / 金额 / 分类 / 类型）。
const SORT_FIELDS: [(&str, &str); 4] = [
    ("transactionAt", "日期"),
    ("price", "金额"),
    ("category", "分类"),
    ("transactionType", "类型"),
];

/// 标签匹配策略常量（`any` / `all`）。
const TAG_POLICY_ANY: &str = "any";

/// 一天的秒数（`date` 粒度翻页用）。
const DAY_SECONDS: i64 = 86_400;

/// 行内操作：编辑 / 关联 / 同步到其他账本 / 删除。
#[derive(Debug, Clone, PartialEq, Eq)]
enum RowAction {
    Edit,
    Link,
    /// 同步到其他账本（载荷是目标账本 id）
    Sync(String),
    Delete,
}

/// 排序项（字段 + 方向）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct SortItem {
    field: String,
    order: String,
}

/// 展开行内操作：`(动作, 记录)`。
type RowEvent = (RowAction, TransactionRecordDto);

// ==================================================================== 页面

#[component]
pub fn TransactionsPage() -> impl IntoView {
    let stores = AppStores::global();

    // ---- 列表数据 ----
    let items = RwSignal::new(Vec::<TransactionRecordDto>::new());
    let total = RwSignal::new(0_i64);
    let page = RwSignal::new(1_i32);
    let page_size = RwSignal::new(DEFAULT_PAGE_SIZE);
    let total_pages = RwSignal::new(0_i32);
    let loading = RwSignal::new(false);
    // 首屏是否已经跑过一次查询（区分「正在加载」与「确实没有记录」）
    let loaded = RwSignal::new(false);
    let error_message = RwSignal::new(Option::<String>::None);

    // ---- 账本元信息（空态三态用）----
    let has_any_records = RwSignal::new(Option::<bool>::None);
    let has_any_categories = RwSignal::new(Option::<bool>::None);
    let init_loading = RwSignal::new(false);
    let init_confirm_open = RwSignal::new(false);

    // ---- 时间范围（默认「今天」+ 粒度「日」）----
    let range_mode = RwSignal::new("date".to_string());
    let range_start = RwSignal::new(today_ymd());
    let range_end = RwSignal::new(today_ymd());

    // ---- 筛选条件（项之间 OR）与排序 ----
    let condition_items = RwSignal::new(Vec::<QueryConditionItem>::new());
    let sort_items = RwSignal::new(vec![SortItem {
        field: "transactionAt".to_string(),
        order: "desc".to_string(),
    }]);

    // ---- 弹窗开关 ----
    let record_open = RwSignal::new(false);
    let editing = RwSignal::new(Option::<TransactionRecordDto>::None);
    let filter_open = RwSignal::new(false);
    let sort_open = RwSignal::new(false);
    let link_open = RwSignal::new(false);
    let link_target = RwSignal::new(Option::<TransactionRecordDto>::None);
    let link_date = RwSignal::new(String::new());
    // 正在同步的记录 id（同步期间禁用该行按钮并让图标旋转）
    let syncing_id = RwSignal::new(String::new());
    // 当前打开的「同步到其他账本」气泡所属记录 id（空串表示没有打开的面板）
    let sync_popover_id = RwSignal::new(String::new());

    // ---- 查询：`fetch_page` 是普通函数，输入全部显式传入（动作闭包因此都是 `Copy`）----
    // 事件回调里用 `_untracked`（不建立依赖）；`Effect` 里必须用**跟踪读**，
    // 否则账本/分页/筛选变化时不会重跑——启动时账本尚未拉到时会出现"页面永远空着"。
    let read_inputs = move || QueryInputs {
        ledger_id: stores.current_ledger_id.get_untracked(),
        page: page.get_untracked(),
        size: page_size.get_untracked(),
        start: range_start.get_untracked(),
        end: range_end.get_untracked(),
        filters: condition_items.get_untracked(),
        sorts: sort_items.get_untracked(),
    };

    let read_inputs_tracked = move || QueryInputs {
        ledger_id: stores.current_ledger_id.get(),
        page: page.get(),
        size: page_size.get(),
        start: range_start.get(),
        end: range_end.get(),
        filters: condition_items.get(),
        sorts: sort_items.get(),
    };

    let run_fetch = move |input: QueryInputs| {
        fetch_page(
            input,
            items,
            total,
            total_pages,
            loading,
            loaded,
            error_message,
            stores,
        )
    };

    // 手动刷新（保持当前页码）
    let do_refresh = move || {
        run_fetch(read_inputs());
    };

    // 账本 / 页码 / 每页条数 / 时间范围 / 筛选 / 排序 任一变化即重查。
    //
    // 回到第 1 页的规则：
    // * 账本 / 时间范围 / 筛选条件变化 → 只有当前不在第 1 页时才回退（避免打断用户翻页）；
    // * 每页条数变化 → 无条件回到第 1 页；
    // * 排序应用后也重查（页码不变）。
    Effect::new(move |prev: Option<QueryInputs>| {
        let current = read_inputs_tracked();

        match prev {
            None => {
                load_ledger_meta(
                    current.ledger_id.clone(),
                    has_any_records,
                    has_any_categories,
                );
            }
            Some(previous) => {
                if previous.ledger_id != current.ledger_id {
                    load_ledger_meta(
                        current.ledger_id.clone(),
                        has_any_records,
                        has_any_categories,
                    );
                }
                let size_changed = previous.size != current.size;
                let page_changed = previous.page != current.page;
                let view_changed = previous.start != current.start
                    || previous.end != current.end
                    || previous.filters != current.filters;
                if !page_changed && (size_changed || (view_changed && current.page != 1)) {
                    page.set(1);
                    return QueryInputs { page: 1, ..current };
                }
            }
        }

        run_fetch(current.clone());
        current
    });

    // ---- 记一笔：先检查分类是否存在 ----
    let request_create = move || {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            Notifier::global().warning("请先选择账本".to_string(), None);
            return;
        }
        if has_any_categories.get_untracked() == Some(false) {
            init_confirm_open.set(true);
            return;
        }
        editing.set(None);
        record_open.set(true);
    };

    // ---- 空态引导：初始化默认分类 ----
    let do_init_categories = move || {
        if init_loading.get_untracked() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            return;
        }
        init_loading.set(true);
        leptos::task::spawn_local(async move {
            match api::category::initialize(&ledger_id).await {
                Ok(result) => {
                    Notifier::global().success(
                        format!(
                            "已创建 {} 个默认分类和 {} 个标签",
                            result.categories, result.tags
                        ),
                        None,
                    );
                    init_confirm_open.set(false);
                    load_ledger_meta(ledger_id, has_any_records, has_any_categories);
                    editing.set(None);
                    record_open.set(true);
                }
                Err(error) => notify_error("初始化分类失败", &error),
            }
            init_loading.set(false);
        });
    };

    // ---- 空态引导：跳到上个月 / 今年 ----
    let go_last_month = move || {
        range_mode.set("month".to_string());
        set_range(&today_ymd(), "month", range_start, range_end, Some(-1));
    };
    let go_this_year = move || {
        range_mode.set("year".to_string());
        let today = today_ymd();
        if let Some((year, _)) = split_ymd(&today) {
            let start = format!("{year:04}-01-01");
            let end = format!("{year:04}-12-31");
            range_start.set(start);
            range_end.set(end);
        }
    };

    // ---- 行内操作 ----
    let handle_row = move |(action, record): RowEvent| match action {
        RowAction::Edit => {
            syncing_id.set(String::new());
            editing.set(Some(record));
            record_open.set(true);
        }
        RowAction::Link => {
            let fallback = format_timestamp(record.transaction_at, "YYYY-MM-DD");
            link_date.set(if record.key_event_date.is_empty() {
                fallback
            } else {
                record.key_event_date.clone()
            });
            link_target.set(Some(record));
            link_open.set(true);
        }
        RowAction::Sync(target_ledger_id) => {
            if record.transaction_id.is_empty() || target_ledger_id.is_empty() {
                return;
            }
            syncing_id.set(record.transaction_id.clone());
            // 复制一份 DTO，换上目标账本、清空 id 让后端生成新 id
            let mut copy = record.clone();
            copy.ledger_id = target_ledger_id;
            copy.transaction_id = String::new();
            leptos::task::spawn_local(async move {
                match api::tr::create(copy).await {
                    Ok(_) => Notifier::global().success("同步成功".to_string(), None),
                    Err(_) => Notifier::global().error("同步失败".to_string(), None),
                }
                syncing_id.set(String::new());
            });
        }
        RowAction::Delete => {
            let id = record.transaction_id;
            if id.is_empty() {
                return;
            }
            leptos::task::spawn_local(async move {
                match api::tr::delete(&id).await {
                    Ok(()) => do_refresh(),
                    Err(error) => notify_error("删除消费记录失败", &error),
                }
            });
        }
    };

    // ---- 关联关键事件 ----
    let confirm_link = move || {
        let Some(record) = link_target.get_untracked() else {
            return;
        };
        let date = link_date.get_untracked();
        if date.is_empty() || record.transaction_id.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            match api::tr::link(&record.transaction_id, &date).await {
                Ok(_) => {
                    Notifier::global().success("关联成功".to_string(), None);
                    link_open.set(false);
                    link_target.set(None);
                    do_refresh();
                }
                Err(error) => notify_error("关联失败", &error),
            }
        });
    };

    let unlink = move || {
        let Some(record) = link_target.get_untracked() else {
            return;
        };
        if record.transaction_id.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            match api::tr::unlink(&record.transaction_id).await {
                Ok(_) => {
                    Notifier::global().success("已解除关联".to_string(), None);
                    link_open.set(false);
                    link_target.set(None);
                    do_refresh();
                }
                Err(error) => notify_error("解除关联失败", &error),
            }
        });
    };

    // ---- 排序应用 ----
    let apply_sort = move |next: Vec<SortItem>| {
        sort_items.set(next);
    };

    let ledger_name = move || {
        let name = stores.current_ledger_name();
        if name.is_empty() {
            "未选择账本".to_string()
        } else {
            name
        }
    };

    let empty_state = move || {
        empty_state_view(
            loading,
            loaded,
            error_message,
            has_any_records,
            has_any_categories,
            range_start,
            range_end,
            init_loading,
            UnsyncCallback::new(move |()| do_init_categories()),
            UnsyncCallback::new(move |()| request_create()),
            UnsyncCallback::new(move |()| go_last_month()),
            UnsyncCallback::new(move |()| go_this_year()),
        )
    };

    let table_view = move || {
        let rows = items.get();
        if rows.is_empty() {
            return view! { <div></div> }.into_any();
        }
        view! {
            {table_view(
                rows,
                UnsyncCallback::new(move |event: RowEvent| handle_row(event)),
                syncing_id,
                sync_popover_id,
            )}
        }
        .into_any()
    };

    view! {
        <section class="page">
            <header class="page-header">
                <div class="page-header-text">
                    <h1 class="page-title">{PAGE_TITLE}</h1>
                    <p class="page-subtitle">{move || format!("账本：{}", ledger_name())}</p>
                </div>
                <div class="app-top-bar-spacer"></div>
            </header>

            <div class="page-body">
                <div class="tr-toolbar">
                    <div class="tr-toolbar-left">
                        <span class="tr-ledger-chip">
                            <span class="ledger-btn-icon">{icons::icon(Icon::Book)}</span>
                            {move || ledger_name()}
                        </span>
                        <TrTimeRangePicker
                            mode=range_mode
                            start=range_start
                            end=range_end
                        />
                    </div>
                    <div class="tr-toolbar-right">
                        <span class="tr-footer-total">
                            {move || format!("共 {} 条记录", total.get())}
                        </span>
                        <Button
                            variant=ButtonVariant::Secondary
                            size=ButtonSize::Small
                            on_click=move || do_refresh()
                        >
                            "刷新"
                        </Button>
                        <Button
                            variant=ButtonVariant::Primary
                            size=ButtonSize::Small
                            disabled=Signal::derive(move || {
                                stores.current_ledger_id.get().is_empty()
                            })
                            on_click=move || request_create()
                        >
                            "记一笔"
                        </Button>
                    </div>
                </div>

                <div class="tr-body">
                    <div class="tr-content">
                        <Show when=move || !items.get().is_empty() fallback=empty_state>
                            {table_view}
                        </Show>
                    </div>

                    <div class="tr-footer">
                        <span class="tr-footer-total">
                            {move || {
                                let page_now = page.get();
                                let pages = total_pages.get();
                                if pages <= 0 {
                                    format!("第 {page_now} 页")
                                } else {
                                    format!("第 {page_now} / {pages} 页")
                                }
                            }}
                        </span>
                        <Pagination
                            page=page
                            total_pages=Signal::derive(move || total_pages.get())
                            page_size=page_size
                            page_size_options=PAGE_SIZE_OPTIONS.to_vec()
                            disabled=Signal::derive(move || loading.get())
                        />
                    </div>
                </div>
            </div>

            // ---- 悬浮按钮组 ----
            <FloatButton
                class="tr-float-primary"
                title="记一笔"
                on_click=move || request_create()
            />
            <FloatButton
                class="tr-float-secondary"
                title="筛选条件"
                icon=Icon::Search
                on_click=move || filter_open.set(true)
            >
                <span class="tr-float-badge">{move || condition_items.get().len()}</span>
            </FloatButton>
            <FloatButton
                class="tr-float-sort"
                title="排序"
                icon=Icon::Down
                on_click=move || sort_open.set(true)
            >
                <span class="tr-float-sort-icon">
                    {move || {
                        let ascending = sort_items
                            .get()
                            .first()
                            .map(|item| item.order == "asc")
                            .unwrap_or(false);
                        if ascending {
                            svg_icon(&SORT_ASCENDING)
                        } else {
                            svg_icon(&SORT_DESCENDING)
                        }
                    }}
                </span>
            </FloatButton>

            // ---- 排序弹窗 ----
            {sort_modal(sort_open, sort_items, Callback::new(apply_sort))}

            // ---- 筛选弹窗 ----
            {filter_modal(filter_open, condition_items, Callback::new(move |next| {
                condition_items.set(next)
            }))}

            // ---- 关联关键事件弹窗 ----
            {link_modal(
                link_open,
                link_target,
                link_date,
                Callback::new(move |_| confirm_link()),
                Callback::new(move |_| unlink()),
            )}

            // ---- 记一笔 / 编辑弹窗 ----
            {record_modal(record_open, editing, Callback::new(move |_| do_refresh()))}

            // ---- 分类缺失确认框 ----
            <Modal
                open=Signal::derive(move || init_confirm_open.get())
                title="还没有分类"
                width=420
                ok_text="初始化分类"
                cancel_text="暂不记录"
                ok_loading=Signal::derive(move || init_loading.get())
                on_close=move || init_confirm_open.set(false)
                on_ok=move || do_init_categories()
            >
                <p class="workspace-picker-text">
                    "先创建默认分类与标签，记下的每一笔才能归档和复盘。"
                </p>
            </Modal>
        </section>
    }
}

// ==================================================================== 数据辅助

/// 一次查询的全部输入（快照式：异步块里只读这些值，不再读信号）。
#[derive(Clone, PartialEq)]
struct QueryInputs {
    ledger_id: String,
    page: i32,
    size: i32,
    start: String,
    end: String,
    filters: Vec<QueryConditionItem>,
    sorts: Vec<SortItem>,
}

/// 拉取一页并写入各信号。
///
/// 查询条件按固定顺序组装：
/// `ledgerId` / `offset` / `limit` 由 `api::tr::default_condition` 打底，
/// 再按需覆盖 `tsRange` / `items` / `sortFields`（`timeRange` 为空时不传 `tsRange`）。
#[allow(clippy::too_many_arguments)]
fn fetch_page(
    input: QueryInputs,
    items: RwSignal<Vec<TransactionRecordDto>>,
    total: RwSignal<i64>,
    total_pages: RwSignal<i32>,
    loading: RwSignal<bool>,
    loaded: RwSignal<bool>,
    error_message: RwSignal<Option<String>>,
    stores: AppStores,
) {
    // 当前时间范围快照：供「新增记录默认日期」读取
    RANGE_SNAPSHOT.with(|slot| {
        *slot.borrow_mut() = (input.start.clone(), input.end.clone());
    });

    if input.ledger_id.is_empty() {
        items.set(Vec::new());
        total.set(0);
        total_pages.set(0);
        stores.statistics.set(BTreeMap::new());
        error_message.set(None);
        loaded.set(true);
        return;
    }

    loading.set(true);
    leptos::task::spawn_local(async move {
        let mut condition = api::tr::default_condition(&input.ledger_id, input.page, input.size);
        if let Some(range) = range_to_seconds(&input.start, &input.end) {
            condition.ts_range = range;
        }
        condition.items = input.filters;
        condition.sort_fields = sort_fields(&input.sorts);

        match api::tr::query(condition).await {
            Ok(result) => {
                error_message.set(None);
                items.set(result.items);
                total.set(result.total);
                total_pages.set(result.total_pages);
                stores.statistics.set(result.tr_statistics);
            }
            Err(error) => {
                items.set(Vec::new());
                total.set(0);
                total_pages.set(0);
                stores.statistics.set(BTreeMap::new());
                error_message.set(Some(error.prefixed(QUERY_ERROR_PREFIX)));
                notify_error(QUERY_ERROR_PREFIX, &error);
            }
        }
        loading.set(false);
        loaded.set(true);
    });
}

/// 交易类型下拉选项（收入 / 支出 / 转账）。
fn transaction_type_options() -> Vec<SelectOption> {
    TRANSACTION_TYPES
        .iter()
        .map(|(value, label)| SelectOption::new(*value, *label))
        .collect()
}

/// 交易类型分段选项（`Segmented` 的 options 类型与 `Select` 不同）。
fn transaction_type_segments() -> Vec<SegmentedOption> {
    TRANSACTION_TYPES
        .iter()
        .map(|(value, label)| SegmentedOption::new(*value, *label))
        .collect()
}

/// 把时间范围（`YYYY-MM-DD`）转成**闭区间** Unix 秒
/// （`convertToUnixTimeRange`：起点当天 00:00:00、终点当天 23:59:59）。
fn range_to_seconds(start: &str, end: &str) -> Option<Vec<i64>> {
    let start_seconds = ymd_to_seconds(start)?;
    let end_seconds = end_of_day_seconds(end)?;
    Some(vec![start_seconds, end_seconds])
}

/// 某天 23:59:59（本地）的 Unix 秒。
fn end_of_day_seconds(ymd: &str) -> Option<i64> {
    Some(ymd_to_seconds(ymd)? + DAY_SECONDS - 1)
}

/// 排序项 → 查询 DTO 字段。
fn sort_fields(items: &[SortItem]) -> Vec<QueryConditionSortField> {
    items
        .iter()
        .map(|item| QueryConditionSortField {
            field: item.field.clone(),
            order: item.order.clone(),
        })
        .collect()
}

/// 查询账本元信息：该账本是否已有记录 / 是否已有分类。
///
/// 任一查询失败都保持 `None`（回退成通用空态），不让引导信息阻断页面。
fn load_ledger_meta(
    ledger_id: String,
    has_any_records: RwSignal<Option<bool>>,
    has_any_categories: RwSignal<Option<bool>>,
) {
    has_any_records.set(None);
    has_any_categories.set(None);
    if ledger_id.is_empty() {
        return;
    }
    leptos::task::spawn_local(async move {
        let mut condition = api::tr::default_condition(&ledger_id, 1, 1);
        condition.ts_range = Vec::new();
        condition.items = Vec::new();
        condition.sort_fields = Vec::new();

        let record_total = match api::tr::query(condition).await {
            Ok(result) => Some(result.total),
            Err(_) => None,
        };
        let categories = api::category::list(api::category::ALL, &ledger_id)
            .await
            .ok();

        // 账本在查询途中被切换时丢弃过期结果
        if AppStores::global().current_ledger_id.get_untracked() != ledger_id {
            return;
        }
        has_any_records.set(record_total.map(|total| total > 0));
        has_any_categories.set(categories.map(|list| !list.is_empty()));
    });
}

/// 把查询结果写进信号（供 `fetch` 之外的重用）。
#[allow(dead_code)]
fn apply_result(
    items: RwSignal<Vec<TransactionRecordDto>>,
    total: RwSignal<i64>,
    total_pages: RwSignal<i32>,
    stores: AppStores,
    result: TrQueryResult,
) {
    items.set(result.items);
    total.set(result.total);
    total_pages.set(result.total_pages);
    stores.statistics.set(result.tr_statistics);
}

// ==================================================================== 日期算术（纯整数，无新依赖）

/// 拆分 `YYYY-MM-DD` → `(年, 月)`。
fn split_ymd(input: &str) -> Option<(i32, u32)> {
    let trimmed = input.trim();
    let (year, rest) = trimmed.split_once('-')?;
    let (month, _) = rest.split_once('-')?;
    Some((year.parse().ok()?, month.parse().ok()?))
}

/// 拆分 `YYYY-MM-DD` → `(年, 月, 日)`。
fn split_ymd_full(input: &str) -> Option<(i32, u32, u32)> {
    let trimmed = input.trim();
    let mut parts = trimmed.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((year, month, day))
}

/// 月加减（`delta` 可负），返回 `(年, 月)`。
fn add_month(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let total = year * 12 + (month as i32 - 1) + delta;
    (total.div_euclid(12), total.rem_euclid(12) as u32 + 1)
}

/// 某年某月的天数（用 JS「下月第 0 天」技巧，自动处理闰年）。
fn days_in_month(year: i32, month: u32) -> u32 {
    let date = js_sys::Date::new_with_year_month_day(year as u32, month as i32, 0);
    date.get_date()
}

/// 某天所在周的周一。
///
/// 一周从周一算起：`get_day()` 的 0 是周日，用 `(day + 6) % 7` 折算成距周一的偏移；
fn week_monday(ymd: &str) -> Option<String> {
    let (year, month, day) = split_ymd_full(ymd)?;
    let date = js_sys::Date::new_with_year_month_day(year as u32, month as i32 - 1, day as i32);
    let offset = ((date.get_day() + 6) % 7) as i32; // 0 = 周一
    if offset == 0 {
        return Some(ymd.to_string());
    }
    let monday =
        js_sys::Date::new_with_year_month_day(year as u32, month as i32 - 1, day as i32 - offset);
    Some(format!(
        "{:04}-{:02}-{:02}",
        monday.get_full_year(),
        monday.get_month() + 1,
        monday.get_date()
    ))
}

/// 某天所在周的周日。
fn week_sunday(ymd: &str) -> Option<String> {
    let (year, month, day) = split_ymd_full(ymd)?;
    let date = js_sys::Date::new_with_year_month_day(year as u32, month as i32 - 1, day as i32 + 6);
    Some(format!(
        "{:04}-{:02}-{:02}",
        date.get_full_year(),
        date.get_month() + 1,
        date.get_date()
    ))
}

/// 区间是否落在同一天。
fn same_day(start: &str, end: &str) -> bool {
    start == end
}

/// 区间是否恰好 6 天（`end.diff(start, 'day') === 6`，即整周）。
fn is_six_days(start: &str, end: &str) -> bool {
    match (ymd_to_seconds(start), ymd_to_seconds(end)) {
        (Some(from), Some(to)) => (to - from) / DAY_SECONDS == 6,
        _ => false,
    }
}

/// 月份粒度对齐：起点 1 号、终点当月最后一天。
fn month_bounds(ymd: &str) -> Option<(String, String)> {
    let (year, month) = split_ymd(ymd)?;
    let last = days_in_month(year, month);
    Some((
        format!("{year:04}-{month:02}-01"),
        format!("{year:04}-{month:02}-{last:02}"),
    ))
}

/// 年份粒度对齐：1 月 1 日 ~ 12 月 31 日。
fn year_bounds(ymd: &str) -> Option<(String, String)> {
    let (year, _) = split_ymd(ymd)?;
    Some((format!("{year:04}-01-01"), format!("{year:04}-12-31")))
}

/// `normalizeTimeRange`：按粒度对齐后，起点取当天 00:00、终点取当天 23:59:59
/// （这里只需处理日期部分，秒数在 [`range_to_seconds`] 里补齐）。
fn normalize_range(start: &str, end: &str, mode: &str) -> (String, String) {
    let (from, to) = match mode {
        "month" => {
            let from = month_bounds(start)
                .map(|bounds| bounds.0)
                .unwrap_or_default();
            let to = month_bounds(end).map(|bounds| bounds.1).unwrap_or_default();
            (from, to)
        }
        "year" => {
            let from = year_bounds(start)
                .map(|bounds| bounds.0)
                .unwrap_or_default();
            let to = year_bounds(end).map(|bounds| bounds.1).unwrap_or_default();
            (from, to)
        }
        _ => (start.to_string(), end.to_string()),
    };
    if from.is_empty() || to.is_empty() || from > to {
        (start.to_string(), end.to_string())
    } else {
        (from, to)
    }
}

/// 设置区间（先按粒度对齐再落盘）；`shift` 为周期偏移（`None` 表示不偏移）。
fn set_range(
    anchor: &str,
    mode: &str,
    start: RwSignal<String>,
    end: RwSignal<String>,
    shift: Option<i32>,
) {
    let (current_start, current_end) = normalize_range(anchor, anchor, mode);
    let (next_start, next_end) = match shift {
        None | Some(0) => (current_start, current_end),
        Some(direction) => shift_period(&current_start, &current_end, mode, direction),
    };
    start.set(next_start);
    end.set(next_end);
}

/// `shiftPeriod`：按粒度前后翻一个周期。
///
/// * `date`：区间正好 6 天 → 整周（±7 天）；否则 ±1 天
/// * `month`：`start ± 1 月` 的月初 / `end ± 1 月` 的月末
/// * `year`：`start ± 1 年` 的年初 / `end ± 1 年` 的年末
fn shift_period(start: &str, end: &str, mode: &str, direction: i32) -> (String, String) {
    match mode {
        "month" => {
            let Some((start_year, start_month)) = split_ymd(start) else {
                return (start.to_string(), end.to_string());
            };
            let Some((end_year, end_month)) = split_ymd(end) else {
                return (start.to_string(), end.to_string());
            };
            let (next_start_year, next_start_month) = add_month(start_year, start_month, direction);
            let (next_end_year, next_end_month) = add_month(end_year, end_month, direction);
            let last = days_in_month(next_end_year, next_end_month);
            (
                format!("{next_start_year:04}-{next_start_month:02}-01"),
                format!("{next_end_year:04}-{next_end_month:02}-{last:02}"),
            )
        }
        "year" => {
            let Some((start_year, _)) = split_ymd(start) else {
                return (start.to_string(), end.to_string());
            };
            let Some((end_year, _)) = split_ymd(end) else {
                return (start.to_string(), end.to_string());
            };
            (
                format!("{:04}-01-01", start_year + direction),
                format!("{:04}-12-31", end_year + direction),
            )
        }
        _ => {
            let days = if is_six_days(start, end) { 7 } else { 1 };
            let delta = DAY_SECONDS * i64::from(days) * i64::from(direction);
            let next_start = ymd_to_seconds(start).map(|seconds| seconds + delta);
            let next_end = ymd_to_seconds(end).map(|seconds| seconds + delta);
            match (
                next_start.map(|seconds| format_timestamp(seconds, "YYYY-MM-DD")),
                next_end.map(|seconds| format_timestamp(seconds, "YYYY-MM-DD")),
            ) {
                (Some(from), Some(to)) => (from, to),
                _ => (start.to_string(), end.to_string()),
            }
        }
    }
}

/// 时间范围的展示文案（范围输入框里的内容）。
fn range_text(start: &str, end: &str) -> String {
    if start.is_empty() || end.is_empty() {
        return "请选择时间范围".to_string();
    }
    if same_day(start, end) {
        start.to_string()
    } else {
        format!("{start} ~ {end}")
    }
}

// ==================================================================== 时间范围选择器

/// 「今天 / 本周 / 本月 / 上周 / 上月 / 今年」六个预设。
fn preset_ranges() -> Vec<(&'static str, String, String)> {
    let today = today_ymd();
    let mut presets: Vec<(&'static str, String, String)> = Vec::new();

    presets.push(("今天", today.clone(), today.clone()));
    if let (Some(monday), Some(sunday)) = (week_monday(&today), week_sunday(&today)) {
        presets.push(("本周", monday, sunday));
    }
    if let Some((from, to)) = month_bounds(&today) {
        presets.push(("本月", from, to));
    }
    if let Some((year, month)) = split_ymd(&today) {
        let (last_year, last_month) = add_month(year, month, -1);
        let last = days_in_month(last_year, last_month);
        presets.push((
            "上月",
            format!("{last_year:04}-{last_month:02}-01"),
            format!("{last_year:04}-{last_month:02}-{last:02}"),
        ));
    }
    if let Some((monday, sunday)) = week_monday(&today).zip(week_sunday(&today)) {
        let shift = DAY_SECONDS * 7;
        if let (Some(from), Some(to)) = (ymd_to_seconds(&monday), ymd_to_seconds(&sunday)) {
            presets.push((
                "上周",
                format_timestamp(from - shift, "YYYY-MM-DD"),
                format_timestamp(to - shift, "YYYY-MM-DD"),
            ));
        }
    }
    if let Some((from, to)) = year_bounds(&today) {
        presets.push(("今年", from, to));
    }

    presets
}

/// 时间范围选择器：
/// 粒度分段（日/月/年）+ 区间选择器 + 前后翻页 + 一排预设。
#[component]
fn TrTimeRangePicker(
    /// 粒度：`date` / `month` / `year`
    mode: RwSignal<String>,
    /// 起点（`YYYY-MM-DD`）
    start: RwSignal<String>,
    /// 终点（`YYYY-MM-DD`）
    end: RwSignal<String>,
) -> impl IntoView {
    let picker_open = RwSignal::new(false);
    // 面板当前展示的 (年, 月)：整数二元组是 `Copy`，可以随便进闭包
    let visible = RwSignal::new(split_ymd(&start.get_untracked()).unwrap_or((1970, 1)));

    let shift = move |delta: i32| {
        let current_start = start.get_untracked();
        let current_end = end.get_untracked();
        let current_mode = mode.get_untracked();
        let (from, to) = shift_period(&current_start, &current_end, &current_mode, delta);
        start.set(from);
        end.set(to);
    };

    let open_picker = move |_| {
        let next = split_ymd(&start.get_untracked()).unwrap_or((1970, 1));
        visible.set(next);
        picker_open.update(|open| *open = !*open);
    };

    let change_mode = move |next: String| {
        let from = start.get_untracked();
        let to = end.get_untracked();
        let (normalized_from, normalized_to) = normalize_range(&from, &to, &next);
        start.set(normalized_from);
        end.set(normalized_to);
        mode.set(next);
    };

    let apply_preset = move |from: String, to: String| {
        start.set(from);
        end.set(to);
        picker_open.set(false);
    };

    let mode_options = TIME_RANGE_MODES
        .iter()
        .map(|(value, label)| SegmentedOption::new(*value, *label))
        .collect::<Vec<_>>();

    view! {
        <div class="tr-time">
            <Segmented
                value=mode
                options=mode_options
                on_change=move |next: String| change_mode(next)
            />
            <button
                type="button"
                class="tr-time__nav"
                title="上一周期"
                aria-label="上一个周期"
                on:click=move |_| shift(-1)
            >
                {icons::icon(Icon::Left)}
            </button>

            <div class="tr-time__field" class:is-open=move || picker_open.get()>
                <button
                    type="button"
                    class="tr-time__trigger"
                    on:click=open_picker
                >
                    <span class="ui-date-picker__icon">{icons::icon(Icon::ClockCircle)}</span>
                    <span class="tr-time__value">
                        {move || range_text(&start.get(), &end.get())}
                    </span>
                </button>

                <Show when=move || picker_open.get()>
                    <div class="ui-select__backdrop" on:click=move |_| picker_open.set(false)></div>
                    <div class="tr-time__panel">
                        {move || {
                            let current_mode = mode.get();
                            if current_mode == "date" {
                                view! {
                                    <DateRangePicker
                                        start=start
                                        end=end
                                        placeholder="请选择时间范围"
                                        on_change=move |(from, to): (String, String)| {
                                            if from.is_empty() || to.is_empty() {
                                                return;
                                            }
                                            let current_mode = mode.get_untracked();
                                            let (normalized_from, normalized_to) =
                                                normalize_range(&from, &to, &current_mode);
                                            start.set(normalized_from);
                                            end.set(normalized_to);
                                        }
                                    />
                                }
                                    .into_any()
                            } else if current_mode == "month" {
                                month_panel(visible, start, end)
                            } else {
                                year_panel(visible, start, end)
                            }
                        }}
                        <div class="tr-time__presets">
                            {preset_ranges()
                                .into_iter()
                                .map(|(label, from, to)| {
                                    let click_from = from.clone();
                                    let click_to = to.clone();
                                    view! {
                                        <button
                                            type="button"
                                            class="tr-time__preset"
                                            on:click=move |_| {
                                                apply_preset(click_from.clone(), click_to.clone())
                                            }
                                        >
                                            {label}
                                        </button>
                                    }
                                })
                                .collect_view()}
                        </div>
                    </div>
                </Show>
            </div>

            <button
                type="button"
                class="tr-time__nav"
                title="下一周期"
                aria-label="下一个周期"
                on:click=move |_| shift(1)
            >
                {icons::icon(Icon::Right)}
            </button>
        </div>
    }
}

/// 月份粒度面板：年份切换 + 12 个月。
fn month_panel(
    visible: RwSignal<(i32, u32)>,
    start: RwSignal<String>,
    end: RwSignal<String>,
) -> AnyView {
    let shift_year = move |delta: i32| {
        visible.update(|(year, _)| *year += delta);
    };

    view! {
        <div class="tr-time__panel-head">
            <button
                type="button"
                class="tr-time__nav"
                title="上一年"
                aria-label="上一年"
                on:click=move |_| shift_year(-1)
            >
                {icons::icon(Icon::Left)}
            </button>
            <span class="tr-time__panel-title">
                {move || format!("{} 年", visible.get().0)}
            </span>
            <button
                type="button"
                class="tr-time__nav"
                title="下一年"
                aria-label="下一年"
                on:click=move |_| shift_year(1)
            >
                {icons::icon(Icon::Right)}
            </button>
        </div>
        <div class="tr-time__grid">
            {move || {
                let (year, current_month) = visible.get();
                (1..=12u32)
                    .map(|month| {
                        let is_active = month == current_month;
                        view! {
                            <button
                                type="button"
                                class="tr-time__cell"
                                class:is-active=is_active
                                on:click=move |_| {
                                    visible.set((year, month));
                                    if let Some((from, to)) =
                                        month_bounds(&format!("{year:04}-{month:02}-01"))
                                    {
                                        start.set(from);
                                        end.set(to);
                                    }
                                }
                            >
                                {format!("{month} 月")}
                            </button>
                        }
                    })
                    .collect_view()
            }}
        </div>
    }
    .into_any()
}

/// 年份粒度面板：一段年份网格。
fn year_panel(
    visible: RwSignal<(i32, u32)>,
    start: RwSignal<String>,
    end: RwSignal<String>,
) -> AnyView {
    view! {
        <div class="tr-time__grid tr-time__grid--year">
            {move || {
                let current_year = visible.get().0;
                let decade = current_year.div_euclid(10) * 10;
                (decade - 10..decade + 20)
                    .map(|year| {
                        let is_active = year == current_year;
                        view! {
                            <button
                                type="button"
                                class="tr-time__cell"
                                class:is-active=is_active
                                on:click=move |_| {
                                    visible.set((year, 1));
                                    if let Some((from, to)) =
                                        year_bounds(&format!("{year:04}-01-01"))
                                    {
                                        start.set(from);
                                        end.set(to);
                                    }
                                }
                            >
                                {format!("{year} 年")}
                            </button>
                        }
                    })
                    .collect_view()
            }}
        </div>
    }
    .into_any()
}

// ==================================================================== 表格

/// 表格：8 列（日期/类型/分类/标签/描述/金额/标记/操作）。
fn table_view(
    rows: Vec<TransactionRecordDto>,
    on_action: UnsyncCallback<RowEvent>,
    syncing_id: RwSignal<String>,
    sync_popover_id: RwSignal<String>,
) -> AnyView {
    let views = rows
        .into_iter()
        .map(|record| row_view(record, on_action, syncing_id, sync_popover_id))
        .collect_view();

    view! {
        <div class="table-wrapper">
            <table class="tr-table">
                <thead>
                    <tr>
                        <th class="tr-cell--center" style="width: 100px;">"日期"</th>
                        <th class="tr-cell--center" style="width: 100px;">"类型"</th>
                        <th class="tr-cell--center" style="width: 100px;">"分类"</th>
                        <th style="width: 180px;">"标签"</th>
                        <th>"描述"</th>
                        <th class="tr-cell--right" style="width: 110px;">"金额"</th>
                        <th class="tr-cell--center" style="width: 100px;">"标记"</th>
                        <th class="tr-cell--center" style="width: 160px;">"操作"</th>
                    </tr>
                </thead>
                <tbody>{views}</tbody>
            </table>
        </div>
    }
    .into_any()
}

/// 渲染一行（列顺序与列宽见 [`table_view`] 的表头；操作列的四个动作见行内注释）。
fn row_view(
    record: TransactionRecordDto,
    on_action: UnsyncCallback<RowEvent>,
    syncing_id: RwSignal<String>,
    sync_popover_id: RwSignal<String>,
) -> AnyView {
    // 记录本体要留给操作列（每个动作各一份克隆），所以字段一律取克隆
    let transaction_type = record.transaction_type.clone();
    let category = record.category.clone();
    let description_raw = record.description.clone();
    let tags = record.tags.clone();
    let outlier = record.outlier;
    let key_event_date = record.key_event_date.clone();
    let price = record.price;
    let transaction_at = record.transaction_at;

    let date_text = format_timestamp(transaction_at, "MM-DD");
    let full_time = format_timestamp(transaction_at, "YYYY-MM-DD HH:mm:ss");
    let type_text = format::transaction_type_text(&transaction_type);
    let type_class = format::type_class(&transaction_type);
    let price_text = format::signed_amount(&transaction_type, price);
    let price_class = format::amount_class(&transaction_type);
    let row_class = format::row_class(&transaction_type);
    let description = if description_raw.is_empty() {
        "-".to_string()
    } else {
        description_raw.clone()
    };
    let tag_kind = TagKind::from_transaction_type(&transaction_type);
    let has_key_event = !key_event_date.is_empty();

    // 同一份字符串要进多个闭包时，每个闭包各持一份克隆
    let record_id = record.transaction_id.clone();
    let record_id_click = record.transaction_id.clone();
    let record_id_disabled = record.transaction_id.clone();
    let record_id_spinning = record.transaction_id.clone();

    // 同步气泡：只让"当前打开的那一行"显示，切换行时把上一行的面板收起来
    let panel_class = move || {
        if sync_popover_id.get() == record_id {
            "tr-sync-panel"
        } else {
            "tr-sync-panel is-hidden"
        }
    };

    // 操作列的四个动作各自需要一份记录：`view!` 的 children 会多次求值，
    // 每个 `move` 闭包必须拿到自己的克隆。
    let edit_record = record.clone();
    let link_record = record.clone();
    let sync_click_record = record.clone();
    let delete_record = record;
    let link_title = if has_key_event {
        format!("已关联至 {key_event_date}")
    } else {
        "关联".to_string()
    };

    view! {
        <tr class=row_class>
            <td class="tr-cell tr-cell--center">
                <span class="tr-cell-date" title=full_time>{date_text}</span>
            </td>
            <td class="tr-cell tr-cell--center">
                <span class=format!("tr-cell-type {type_class}")>{type_text}</span>
            </td>
            <td class="tr-cell tr-cell--center">
                <span class="tr-cell-category">{category}</span>
            </td>
            <td class="tr-cell">
                <div class="tr-cell-tags">
                    {tags
                        .into_iter()
                        .map(|tag| {
                            view! {
                                <Tag kind=tag_kind class="tag-item">
                                    {tag}
                                </Tag>
                            }
                        })
                        .collect_view()}
                </div>
            </td>
            <td class="tr-cell">
                <span class="tr-cell-description" title=description_raw>{description}</span>
            </td>
            <td class="tr-cell tr-cell--right">
                <span class=format!("tr-cell-price {price_class}")>{price_text}</span>
            </td>
            <td class="tr-cell tr-cell--center">
                <Show when=move || outlier>
                    <Tag kind=TagKind::Outlier>"离群值"</Tag>
                </Show>
                <Show when=move || has_key_event>
                    <span class="tr-cell-mark" title=key_event_date.clone()>
                        {icons::icon(Icon::Star)}
                    </span>
                </Show>
            </td>
            <td class="tr-cell tr-cell--center">
                <div class="tr-cell-actions">
                    // 1. 编辑
                    <crate::components::ui::Tooltip title="编辑">
                        <button
                            type="button"
                            class="tr-action"
                            aria-label="编辑记录"
                            on:click=move |_| on_action.run((RowAction::Edit, edit_record.clone()))
                        >
                            {svg_icon(&EDIT_PATHS)}
                        </button>
                    </crate::components::ui::Tooltip>

                    // 2. 关联（已关联时 tooltip 显示日期）
                    <crate::components::ui::Tooltip title=link_title>
                        <button
                            type="button"
                            class="tr-action"
                            aria-label=if has_key_event { "修改关联" } else { "关联到关键事件" }
                            on:click=move |_| on_action.run((RowAction::Link, link_record.clone()))
                        >
                            {svg_icon(&LINK_PATHS)}
                        </button>
                    </crate::components::ui::Tooltip>

                    // 3. 同步到其他账本（点击展开账本列表）
                    <crate::components::ui::Tooltip title="同步到其他账本">
                        <span
                            class="tr-sync"
                            on:click=move |_| {
                                sync_popover_id.update(|current| {
                                    if *current == record_id_click {
                                        current.clear();
                                    } else {
                                        *current = record_id_click.clone();
                                    }
                                });
                            }
                        >
                            <button
                                type="button"
                                class="tr-action"
                                aria-label="同步到其他账本"
                                disabled=move || syncing_id.get() == record_id_disabled
                            >
                                <span
                                    class="tr-sync-icon"
                                    class:is-spinning=move || syncing_id.get() == record_id_spinning
                                >
                                    {svg_icon(&SYNC_PATHS)}
                                </span>
                            </button>

                            <div class=panel_class>
                                // 面板内容整体由一个闭包渲染（`Show` 的 children/fallback 都必须是
                                // `Fn`，嵌套两层闭包会让 `record` 的所有权难以传递）。
                                {move || {
                                    let targets = sync_ledger_options();
                                    if targets.is_empty() {
                                        view! { <div class="tr-sync-empty">"无可用账本"</div> }
                                            .into_any()
                                    } else {
                                        targets
                                            .into_iter()
                                            .map(|(ledger_id, ledger_name)| {
                                                let click_target = ledger_id.clone();
                                                let click_record = sync_click_record.clone();
                                                view! {
                                                    <button
                                                        type="button"
                                                        class="tr-sync-item"
                                                        on:click=move |_| {
                                                            sync_popover_id.set(String::new());
                                                            on_action.run((
                                                                RowAction::Sync(click_target.clone()),
                                                                click_record.clone(),
                                                            ));
                                                        }
                                                    >
                                                        {ledger_name}
                                                    </button>
                                                }
                                            })
                                            .collect_view()
                                            .into_any()
                                    }
                                }}
                            </div>
                        </span>
                    </crate::components::ui::Tooltip>

                    // 4. 删除（不显示取消按钮）
                    <crate::components::ui::Popconfirm
                        title="删除这条消费记录？此操作不可恢复。"
                        ok_text="删除"
                        show_cancel=false
                        class="ui-popconfirm--end"
                        on_confirm=move || {
                            on_action.run((RowAction::Delete, delete_record.clone()))
                        }
                    >
                        <button
                            type="button"
                            class="tr-action tr-action--danger"
                            title="删除"
                            aria-label="删除记录"
                        >
                            {icons::icon(Icon::Trash)}
                        </button>
                    </crate::components::ui::Popconfirm>
                </div>
            </td>
        </tr>
    }
    .into_any()
}

/// 「同步到其他账本」的候选账本：`AppStores.ledgers` 里排除当前账本。
fn sync_ledger_options() -> Vec<(String, String)> {
    let stores = AppStores::global();
    let current = stores.current_ledger_id.get();
    stores.ledgers.with(|ledgers| {
        ledgers
            .iter()
            .filter(|ledger| ledger.id != current)
            .map(|ledger| (ledger.id.clone(), ledger.name.clone()))
            .collect()
    })
}

// ==================================================================== 空态

/// 空态三态（加载中 / 查询失败 / 无记录引导）。
#[allow(clippy::too_many_arguments)]
fn empty_state_view(
    loading: RwSignal<bool>,
    loaded: RwSignal<bool>,
    error_message: RwSignal<Option<String>>,
    has_any_records: RwSignal<Option<bool>>,
    has_any_categories: RwSignal<Option<bool>>,
    range_start: RwSignal<String>,
    range_end: RwSignal<String>,
    init_loading: RwSignal<bool>,
    on_init: UnsyncCallback<()>,
    on_create: UnsyncCallback<()>,
    on_last_month: UnsyncCallback<()>,
    on_this_year: UnsyncCallback<()>,
) -> AnyView {
    if loading.get() || !loaded.get() {
        return view! {
            <div class="empty-guide">
                <Spin spinning=true />
                <span class="empty-guide-loading">"正在加载记录…"</span>
            </div>
        }
        .into_any();
    }

    if let Some(message) = error_message.get() {
        return view! {
            <Empty title="查询失败" description=message icon=Icon::WarningCircle />
        }
        .into_any();
    }

    let any_records = has_any_records.get();
    if any_records == Some(false) {
        return view! {
            <div class="empty-guide">
                <span class="empty-guide-icon">{icons::icon(Icon::Transaction)}</span>
                <p class="empty-guide-title">"从第一笔开始"</p>
                <p class="empty-guide-text">
                    "记下的收入与支出会按月自动汇总，之后可在「数据分析」里看趋势。"
                </p>
                <div class="empty-guide-actions">
                    <Show when=move || has_any_categories.get() == Some(false)>
                        <Button
                            variant=ButtonVariant::Secondary
                            size=ButtonSize::Small
                            loading=Signal::derive(move || init_loading.get())
                            on_click=move || on_init.run(())
                        >
                            "初始化默认分类"
                        </Button>
                    </Show>
                    <Button
                        variant=ButtonVariant::Primary
                        size=ButtonSize::Small
                        on_click=move || on_create.run(())
                    >
                        "记一笔"
                    </Button>
                </div>
            </div>
        }
        .into_any();
    }

    // 该账本有记录，但当前时间范围内没有：文案按区间判定
    let today = today_ymd();
    let start = range_start.get();
    let end = range_end.get();
    let title = if same_day(&start, &end) {
        if start == today {
            "今天还没有记录"
        } else {
            "这一天还没有记录"
        }
    } else {
        "这段时间还没有记录"
    };

    view! {
        <div class="empty-guide">
            <span class="empty-guide-icon">{icons::icon(Icon::Transaction)}</span>
            <p class="empty-guide-title">{title}</p>
            <p class="empty-guide-text">"换个时间范围看看，或者直接记一笔。"</p>
            <div class="empty-guide-actions">
                <Button
                    variant=ButtonVariant::Secondary
                    size=ButtonSize::Small
                    on_click=move || on_last_month.run(())
                >
                    "看上个月"
                </Button>
                <Button
                    variant=ButtonVariant::Secondary
                    size=ButtonSize::Small
                    on_click=move || on_this_year.run(())
                >
                    "看今年"
                </Button>
                <Button
                    variant=ButtonVariant::Primary
                    size=ButtonSize::Small
                    on_click=move || on_create.run(())
                >
                    "记一笔"
                </Button>
            </div>
        </div>
    }
    .into_any()
}

// ==================================================================== 记一笔 / 编辑弹窗

/// 记一笔 / 编辑弹窗。
fn record_modal(
    open: RwSignal<bool>,
    editing: RwSignal<Option<TransactionRecordDto>>,
    on_saved: Callback<()>,
) -> AnyView {
    let stores = AppStores::global();

    let transaction_id = RwSignal::new(String::new());
    let transaction_type = RwSignal::new("expense".to_string());
    let price_text = RwSignal::new(String::new());
    let date_text = RwSignal::new(today_ymd());
    let category = RwSignal::new(String::new());
    let tags = RwSignal::new(Vec::<String>::new());
    let description = RwSignal::new(String::new());
    // `flags` 是一个字符串数组（多选），用 `Vec<String>`：
    // 唯一选项是 `outlier`（标签「离群值」）。
    let flags = RwSignal::new(Vec::<String>::new());
    let template_id = RwSignal::new(String::new());

    let categories = RwSignal::new(Vec::<CategoryDto>::new());
    let tag_list = RwSignal::new(Vec::<TagDto>::new());
    let templates = RwSignal::new(Vec::<TransactionTemplateDto>::new());
    let price_error = RwSignal::new(String::new());
    let saving = RwSignal::new(false);

    // 「保存为模板」子弹窗
    let save_template_open = RwSignal::new(false);
    let template_name = RwSignal::new(String::new());

    // 打开弹窗时回填表单
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        template_id.set(String::new());
        price_error.set(String::new());

        match editing.get_untracked() {
            Some(record) => {
                transaction_id.set(record.transaction_id.clone());
                transaction_type.set(record.transaction_type.clone());
                price_text.set(tr_domain::money::cents_to_yuan(record.price));
                date_text.set(format_timestamp(record.transaction_at, "YYYY-MM-DD"));
                category.set(record.category.clone());
                tags.set(record.tags.clone());
                description.set(record.description.clone());
                flags.set(if record.outlier {
                    vec!["outlier".to_string()]
                } else {
                    Vec::new()
                });
            }
            None => {
                transaction_id.set(String::new());
                transaction_type.set("expense".to_string());
                price_text.set(String::new());
                date_text.set(default_record_ymd());
                category.set(String::new());
                tags.set(Vec::new());
                description.set(String::new());
                flags.set(Vec::new());
            }
        }

        if ledger_id.is_empty() {
            categories.set(Vec::new());
            tag_list.set(Vec::new());
            templates.set(Vec::new());
            return;
        }
        leptos::task::spawn_local(async move {
            let category_list = api::category::list(&transaction_type.get_untracked(), &ledger_id)
                .await
                .unwrap_or_default();
            let template_list = api::template::list(&ledger_id).await.unwrap_or_default();
            if AppStores::global().current_ledger_id.get_untracked() != ledger_id {
                return;
            }
            // 新建时分类取该类型的第一个
            let current = category.get_untracked();
            let matched = category_list.iter().any(|item| item.name == current);
            if !matched {
                category.set(
                    category_list
                        .first()
                        .map(|item| item.name.clone())
                        .unwrap_or_default(),
                );
            }
            categories.set(category_list);
            templates.set(template_list);
        });
    });

    // 类型变化 → 重查分类；分类变化 → 重查标签
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get();
        let current_type = transaction_type.get();
        let current_category = category.get();
        if ledger_id.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            let category_list = api::category::list(&current_type, &ledger_id)
                .await
                .unwrap_or_default();
            if AppStores::global().current_ledger_id.get_untracked() != ledger_id {
                return;
            }
            // 当前分类不在新类型下时回落到第一个；
            // 列表为空则把分类置空。
            let matched = category_list
                .iter()
                .any(|item| item.name == category.get_untracked());
            if !matched {
                let fallback = category_list
                    .first()
                    .map(|item| item.name.clone())
                    .unwrap_or_default();
                category.set(fallback);
            }
            categories.set(category_list);

            if current_category.is_empty() {
                tag_list.set(Vec::new());
                return;
            }
            let key = format!("{current_category}:{current_type}");
            let available = api::tag::list(&key, &ledger_id).await.unwrap_or_default();
            if AppStores::global().current_ledger_id.get_untracked() != ledger_id {
                return;
            }
            let names: Vec<String> = available.iter().map(|item| item.name.clone()).collect();
            // 已选标签过滤成仍然存在的那些
            let kept: Vec<String> = tags
                .get_untracked()
                .into_iter()
                .filter(|tag| names.contains(tag))
                .collect();
            if kept != tags.get_untracked() {
                tags.set(kept);
            }
            tag_list.set(available);
        });
    });

    // 模板套用：
    // 注意：`flags` 是把整个模板 flags 串当作单个元素塞进数组。
    let apply_template = move |id: String| {
        if id.is_empty() {
            return;
        }
        let found = templates
            .get_untracked()
            .into_iter()
            .find(|template| template.template_id == id);
        let Some(template) = found else {
            return;
        };
        transaction_type.set(template.transaction_type.clone());
        category.set(template.category.clone());
        tags.set(template.tags.clone());
        flags.set(if template.flags.is_empty() {
            Vec::new()
        } else {
            vec![template.flags.clone()]
        });
        description.set(template.description.clone());
    };

    // 保存为模板
    let confirm_save_template = move || {
        let name = template_name.get_untracked().trim().to_string();
        if name.is_empty() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            return;
        }
        let template = TransactionTemplateDto {
            template_id: String::new(),
            ledger_id,
            template_name: name,
            transaction_type: transaction_type.get_untracked(),
            category: category.get_untracked(),
            tags: tags.get_untracked(),
            // `flags` 用逗号拼成字符串
            flags: flags.get_untracked().join(","),
            description: description.get_untracked(),
            sort_order: 0,
        };
        leptos::task::spawn_local(async move {
            match api::template::create(template).await {
                Ok(_) => {
                    Notifier::global().success("保存模板成功".to_string(), None);
                    save_template_open.set(false);
                    let ledger_id = stores.current_ledger_id.get_untracked();
                    if !ledger_id.is_empty() {
                        let list = api::template::list(&ledger_id).await.unwrap_or_default();
                        if AppStores::global().current_ledger_id.get_untracked() == ledger_id {
                            templates.set(list);
                        }
                    }
                }
                Err(error) => notify_error("保存模板失败", &error),
            }
        });
    };

    // 保存：新建 → 直接创建；编辑 → 先建后删，删除失败回滚新建
    let confirm = move || {
        if saving.get_untracked() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            Notifier::global().warning("请先选择账本".to_string(), None);
            return;
        }
        let current_type = transaction_type.get_untracked();
        if current_type.is_empty() {
            Notifier::global().error("请选择交易类型".to_string(), None);
            return;
        }
        let cents = match parse_price(&price_text.get_untracked()) {
            Ok(cents) => cents,
            Err(message) => {
                price_error.set(message);
                return;
            }
        };
        let current_category = category.get_untracked();
        if current_category.is_empty() {
            Notifier::global().error("请选择分类".to_string(), None);
            return;
        }
        let Some(transaction_at) = combine_date_and_time(&date_text.get_untracked()) else {
            Notifier::global().error("请选择日期".to_string(), None);
            return;
        };

        price_error.set(String::new());
        let mut record = TransactionRecordDto {
            ledger_id: ledger_id.clone(),
            transaction_id: transaction_id.get_untracked(),
            price: cents,
            transaction_type: current_type,
            category: current_category,
            description: description.get_untracked(),
            tags: tags.get_untracked(),
            transaction_at,
            outlier: flags.get_untracked().iter().any(|flag| flag == "outlier"),
            // key_event_date 一律留空（关联只能走 `tr_link`）
            key_event_date: String::new(),
        };
        if record.description.is_empty() {
            record.description = "-".to_string();
        }

        let old_id = record.transaction_id.clone();
        saving.set(true);
        leptos::task::spawn_local(async move {
            if old_id.is_empty() {
                match create_record(&record).await {
                    Ok(_) => {
                        open.set(false);
                        editing.set(None);
                        on_saved.run(());
                    }
                    Err(error) => notify_error("创建消费记录失败", &error),
                }
            } else {
                match create_record(&record).await {
                    Ok(new_id) => {
                        if let Err(error) = api::tr::delete(&old_id).await {
                            notify_error("删除旧记录失败", &error);
                            if api::tr::delete(&new_id).await.is_err() {
                                Notifier::global().error(
                                    "新记录已保存，但旧记录清理失败，请手动删除重复记录"
                                        .to_string(),
                                    None,
                                );
                            }
                        } else {
                            open.set(false);
                            editing.set(None);
                            on_saved.run(());
                        }
                    }
                    Err(error) => notify_error("保存修改失败", &error),
                }
            }
            saving.set(false);
        });
    };

    // 弹窗标题：
    // `Modal.title` 是普通 `String`（`#[prop(into)]`），不接受 `Signal` / 闭包，
    // 所以这里在构建视图时（此时 `editing` 已被打开弹窗的动作写好）算成静态串。
    let modal_title = if editing.get_untracked().is_some() {
        "编辑消费记录".to_string()
    } else {
        "新增消费记录".to_string()
    };

    view! {
        <Modal
            open=Signal::derive(move || open.get())
            title=modal_title
            width=800
            footer=false
            on_close=move || open.set(false)
        >
            <Form layout=FormLayout::Vertical class="tr-modal-form">
                // 字段顺序：模板 / 日期 / 类型 / 分类 / 标签 / 标记 / 描述 / 金额
                <FormItem label="模板">
                    <div class="tr-modal-template">
                        {move || {
                            let options = templates
                                .get()
                                .into_iter()
                                .map(|template| {
                                    SelectOption::new(
                                        template.template_id.clone(),
                                        template.template_name.clone(),
                                    )
                                })
                                .collect::<Vec<_>>();
                            view! {
                                <Select
                                    value=template_id
                                    options=options
                                    placeholder="选择模板后自动填充"
                                    allow_clear=true
                                    class="tr-modal-template-select"
                                    on_change=move |id: String| apply_template(id)
                                />
                            }
                        }}
                        <Button
                            variant=ButtonVariant::Secondary
                            size=ButtonSize::Small
                            disabled=Signal::derive(move || {
                                transaction_type.get().is_empty() || category.get().is_empty()
                            })
                            on_click=move || {
                                template_name.set(String::new());
                                save_template_open.set(true);
                            }
                        >
                            "保存为模板"
                        </Button>
                    </div>
                </FormItem>

                <FormItem label="日期">
                    <DatePicker value=date_text allow_clear=true class="tr-modal-full" />
                </FormItem>

                <FormItem label="类型">
                    <Segmented
                        value=transaction_type
                        options=transaction_type_segments()
                    />
                </FormItem>

                <FormItem label="分类">
                    {move || {
                        let options = categories
                            .get()
                            .into_iter()
                            .map(|item| SelectOption::same(item.name))
                            .collect::<Vec<_>>();
                        view! {
                            <Select
                                value=category
                                options=options
                                placeholder="请选择分类"
                                class="tr-modal-full"
                            />
                        }
                    }}
                    <Show when=move || categories.get().is_empty() && !transaction_type.get().is_empty()>
                        <p class="tr-modal-hint">
                            "该类型还没有分类，请先在「分类标签」页添加分类或初始化默认分类。"
                        </p>
                    </Show>
                </FormItem>

                <FormItem label="标签">
                    <TrCheckList
                        values=tags
                        options=Signal::derive(move || {
                            tag_list
                                .get()
                                .into_iter()
                                .map(|item| item.name)
                                .collect::<Vec<String>>()
                        })
                        placeholder="选择一个或多个标签"
                    />
                </FormItem>

                <FormItem label="标记">
                    <CheckboxGroup
                        values=flags
                        options=vec![crate::components::ui::CheckboxOption::new(
                            "outlier",
                            "离群值",
                        )]
                    />
                </FormItem>

                <FormItem label="描述">
                    <Input value=description placeholder="描述消费内容" allow_clear=true />
                </FormItem>

                <FormItem label="金额" error=Signal::derive(move || price_error.get())>
                    <div class="tr-modal-price">
                        <span class="tr-modal-price-prefix">"￥"</span>
                        <Input value=price_text placeholder="0.00" />
                    </div>
                </FormItem>
            </Form>

            <div class="tr-modal-footer">
                <Button
                    variant=ButtonVariant::Secondary
                    size=ButtonSize::Small
                    on_click=move || open.set(false)
                >
                    "取消"
                </Button>
                <Button
                    variant=ButtonVariant::Primary
                    size=ButtonSize::Small
                    loading=Signal::derive(move || saving.get())
                    on_click=move || confirm()
                >
                    "确认"
                </Button>
            </div>
        </Modal>

        // 保存为模板（子弹窗）
        <Modal
            open=Signal::derive(move || save_template_open.get())
            title="保存为模板"
            width=420
            ok_text="保存"
            cancel_text="取消"
            on_close=move || save_template_open.set(false)
            on_ok=move || confirm_save_template()
        >
            <Form layout=FormLayout::Vertical>
                <FormItem label="模板名称">
                    <Input value=template_name placeholder="请输入模板名称" />
                </FormItem>
            </Form>
        </Modal>
    }
    .into_any()
}

/// 新建记录（错误已由调用方通知）。
async fn create_record(record: &TransactionRecordDto) -> Result<String, crate::ipc::IpcError> {
    api::tr::create(record.clone()).await
}

/// 新增记录的默认日期：今天在范围内 → 今天；否则用范围起点。
fn default_record_ymd() -> String {
    let today = today_ymd();
    let start = page_range_start();
    let end = page_range_end();
    if start.is_empty() || end.is_empty() {
        return today;
    }
    if today >= start && today <= end {
        today
    } else {
        start
    }
}

/// 当前时间范围起点的线程内快照（[`default_record_ymd`] 用，避免跨组件传信号）。
fn page_range_start() -> String {
    RANGE_SNAPSHOT.with(|slot| slot.borrow().0.clone())
}

/// 当前时间范围终点的线程内快照。
fn page_range_end() -> String {
    RANGE_SNAPSHOT.with(|slot| slot.borrow().1.clone())
}

thread_local! {
    /// 页面当前时间范围的快照（起点 / 终点），供「新增记录默认日期」读取。
    static RANGE_SNAPSHOT: std::cell::RefCell<(String, String)> =
        const { std::cell::RefCell::new((String::new(), String::new())) };
}

/// 把「所选日期」与「原记录的时分秒」合并成 Unix 秒。
///
/// **每次保存都把时间重置为所选日期的 12:00:00**（本地时区），
/// 既不是 00:00:00 也不是沿用原时分秒；新建与编辑两条路径都走这里。
///
/// 注：不保留原记录的时分秒（见上），一律取所选日期的 12:00:00。
fn combine_date_and_time(ymd: &str) -> Option<i64> {
    let day_start = ymd_to_seconds(ymd)?;
    Some(day_start + 12 * 3600)
}

/// 金额校验（正则 `^(0|[1-9]\d*)(\.\d{1,2})?$` + [`yuan_to_cents`]）：
///
/// * 空串 → 「请输入金额」
/// * 不匹配正则 → 「请输入 ≥0 的有效金额，最多两位小数」
/// * 格式通过后再交给 [`yuan_to_cents`]（`MoneyError` 的 Display 是「无效的金额格式」，兜底用）
fn parse_price(input: &str) -> Result<i64, String> {
    let text = input.trim();
    if text.is_empty() {
        return Err("请输入金额".to_string());
    }
    let mut parts = text.splitn(2, '.');
    let integer = parts.next().unwrap_or_default();
    let decimals = parts.next();

    let integer_ok = integer == "0"
        || (!integer.is_empty()
            && !integer.starts_with('0')
            && integer.bytes().all(|byte| byte.is_ascii_digit()));
    let decimals_ok = match decimals {
        None => true,
        Some(part) => {
            !part.is_empty() && part.len() <= 2 && part.bytes().all(|byte| byte.is_ascii_digit())
        }
    };
    if !integer_ok || !decimals_ok {
        return Err("请输入 ≥0 的有效金额，最多两位小数".to_string());
    }
    yuan_to_cents(text).map_err(|error| error.to_string())
}

// ==================================================================== 标签多选

/// 标签多选面板（勾选式多选）。
#[component]
fn TrCheckList(
    /// 已选值集合
    values: RwSignal<Vec<String>>,
    /// 全部选项
    #[prop(into)]
    options: Signal<Vec<String>>,
    /// 空值占位文案
    #[prop(optional, into)]
    placeholder: Option<String>,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let placeholder = placeholder.unwrap_or_else(|| "请选择".to_string());

    let toggle = move |value: String| {
        values.update(|list| {
            if let Some(index) = list.iter().position(|item| item == &value) {
                list.remove(index);
            } else {
                list.push(value);
            }
        });
    };

    view! {
        <div class="tr-taglist" class:is-open=move || open.get()>
            <button
                type="button"
                class="tr-taglist__trigger"
                on:click=move |_| open.update(|value| *value = !*value)
            >
                <span class="tr-taglist__value">
                    {move || {
                        let selected = values.get();
                        if selected.is_empty() {
                            view! {
                                <span class="tr-taglist__placeholder">{placeholder.clone()}</span>
                            }
                                .into_any()
                        } else {
                            selected
                                .into_iter()
                                .map(|tag| {
                                    view! {
                                        <span class="tr-taglist__chip">
                                            <span class="tr-taglist__chip-text">{tag.clone()}</span>
                                            <span
                                                class="tr-taglist__chip-close"
                                                role="button"
                                                title="移除"
                                                on:click=move |ev| {
                                                    ev.stop_propagation();
                                                    values.update(|list| list.retain(|item| item != &tag));
                                                }
                                            >
                                                {icons::icon(Icon::Close)}
                                            </span>
                                        </span>
                                    }
                                })
                                .collect_view()
                                .into_any()
                        }
                    }}
                </span>
                <Show when=move || !values.get().is_empty()>
                    <span
                        class="ui-input__clear"
                        role="button"
                        title="清空"
                        on:click=move |ev| {
                            ev.stop_propagation();
                            values.set(Vec::new());
                        }
                    >
                        {icons::icon(Icon::CloseCircle)}
                    </span>
                </Show>
                <span class="ui-select__arrow">{icons::icon(Icon::Down)}</span>
            </button>

            <Show when=move || open.get()>
                <div class="ui-select__backdrop" on:click=move |_| open.set(false)></div>
                <div class="tr-taglist__panel">
                    {move || {
                        let list = options.get();
                        if list.is_empty() {
                            view! { <div class="ui-select__empty">"无匹配选项"</div> }.into_any()
                        } else {
                            list.into_iter()
                                .map(|option| {
                                    // 每个闭包各持一份克隆（`String` 不是 `Copy`）
                                    let selected_value = option.clone();
                                    let mark_value = option.clone();
                                    let click_value = option.clone();
                                    let label = option;
                                    view! {
                                        <button
                                            type="button"
                                            class="tr-taglist__option"
                                            class:is-selected=move || {
                                                values.with(|list| list.contains(&selected_value))
                                            }
                                            on:click=move |_| toggle(click_value.clone())
                                        >
                                            <span class="ui-checkbox__box">
                                                <Show when=move || {
                                                    values.with(|list| list.contains(&mark_value))
                                                }>
                                                    <span class="ui-checkbox__mark">
                                                        {icons::icon(Icon::Check)}
                                                    </span>
                                                </Show>
                                            </span>
                                            <span>{label}</span>
                                        </button>
                                    }
                                })
                                .collect_view()
                                .into_any()
                        }
                    }}
                </div>
            </Show>
        </div>
    }
}

// ==================================================================== 筛选弹窗

/// 筛选弹窗。
fn filter_modal(
    open: RwSignal<bool>,
    conditions: RwSignal<Vec<QueryConditionItem>>,
    on_apply: Callback<Vec<QueryConditionItem>>,
) -> AnyView {
    let stores = AppStores::global();
    let draft = RwSignal::new(Vec::<QueryConditionItem>::new());

    let temp_type = RwSignal::new(String::new());
    let temp_category = RwSignal::new(String::new());
    let temp_tags = RwSignal::new(Vec::<String>::new());
    let temp_policy = RwSignal::new(TAG_POLICY_ANY.to_string());
    // 「标签取反」下拉的值（no / yes），与 `temp_not` 保持同步
    let temp_not_value = RwSignal::new("no".to_string());
    let temp_not = RwSignal::new(false);
    let temp_description = RwSignal::new(String::new());

    let categories = RwSignal::new(Vec::<CategoryDto>::new());
    let tag_list = RwSignal::new(Vec::<TagDto>::new());

    // 重置临时输入
    let reset_filter_inputs = move || {
        temp_type.set(String::new());
        temp_category.set(String::new());
        temp_tags.set(Vec::new());
        temp_policy.set(TAG_POLICY_ANY.to_string());
        temp_not.set(false);
        temp_not_value.set("no".to_string());
        temp_description.set(String::new());
    };

    // 打开时回填已确认条件并重置临时输入
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        draft.set(conditions.get_untracked());
        reset_filter_inputs();
        categories.set(Vec::new());
        tag_list.set(Vec::new());
    });

    // 交易类型 → 分类；分类 → 标签
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get();
        let current_type = temp_type.get();
        if ledger_id.is_empty() {
            return;
        }
        if current_type.is_empty() {
            categories.set(Vec::new());
            return;
        }
        leptos::task::spawn_local(async move {
            let list = api::category::list(&current_type, &ledger_id)
                .await
                .unwrap_or_default();
            if AppStores::global().current_ledger_id.get_untracked() != ledger_id {
                return;
            }
            categories.set(list);
        });
    });

    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get();
        let current_type = temp_type.get();
        let current_category = temp_category.get();
        if ledger_id.is_empty() || current_type.is_empty() || current_category.is_empty() {
            tag_list.set(Vec::new());
            return;
        }
        leptos::task::spawn_local(async move {
            let key = format!("{current_category}:{current_type}");
            let list = api::tag::list(&key, &ledger_id).await.unwrap_or_default();
            if AppStores::global().current_ledger_id.get_untracked() != ledger_id {
                return;
            }
            tag_list.set(list);
        });
    });

    let add_condition = move || {
        let condition_type = temp_type.get_untracked();
        let condition_category = temp_category.get_untracked();
        let condition_tags = temp_tags.get_untracked();
        let condition_description = temp_description.get_untracked().trim().to_string();
        // 四个字段全空 → 直接 return 不加
        if condition_type.is_empty()
            && condition_category.is_empty()
            && condition_tags.is_empty()
            && condition_description.is_empty()
        {
            return;
        }
        let item = QueryConditionItem {
            transaction_type: condition_type,
            category: condition_category,
            tags: condition_tags,
            tag_policy: temp_policy.get_untracked(),
            tag_not: temp_not.get_untracked(),
            description: condition_description,
        };
        draft.update(|list| list.push(item));
        temp_type.set(String::new());
        temp_category.set(String::new());
        temp_tags.set(Vec::new());
        temp_policy.set(TAG_POLICY_ANY.to_string());
        temp_not.set(false);
        temp_description.set(String::new());
        categories.set(Vec::new());
        tag_list.set(Vec::new());
    };

    view! {
        <Modal
            open=Signal::derive(move || open.get())
            title="筛选消费记录"
            width=600
            footer=false
            on_close=move || open.set(false)
        >
            <div class="tr-filter">
                <div class="tr-filter__field">
                    <div class="tr-filter__label">"交易类型"</div>
                    <Select
                        value=temp_type
                        options=transaction_type_options()
                        placeholder="请选择交易类型"
                        allow_clear=true
                    />
                </div>

                <div class="tr-filter__field">
                    <div class="tr-filter__label">"分类"</div>
                    {move || {
                        let options = categories
                            .get()
                            .into_iter()
                            .map(|item| SelectOption::same(item.name))
                            .collect::<Vec<_>>();
                        view! {
                            <Select
                                value=temp_category
                                options=options
                                placeholder="请选择分类"
                                allow_clear=true
                                on_change=move |_| temp_tags.set(Vec::new())
                            />
                        }
                    }}
                </div>

                <div class="tr-filter__field">
                    <div class="tr-filter__label">"标签"</div>
                    <TrCheckList
                        values=temp_tags
                        options=Signal::derive(move || {
                            tag_list
                                .get()
                                .into_iter()
                                .map(|item| item.name)
                                .collect::<Vec<String>>()
                        })
                        placeholder="请选择标签"
                    />
                </div>

                <div class="tr-filter__row">
                    <div class="tr-filter__half">
                        <div class="tr-filter__label">"标签匹配"</div>
                        <Select
                            value=temp_policy
                            options=vec![
                                SelectOption::new(TAG_POLICY_ANY, "任意"),
                                SelectOption::new("all", "全部"),
                            ]
                            class="tr-filter__select"
                        />
                    </div>
                    <div class="tr-filter__half">
                        <div class="tr-filter__label">"标签取反"</div>
                        {move || {
                            // `Select` 的 `value` 必须是可写信号，且选项是普通 `Vec`；
                            // 用信号直接承载 "yes"/"no"，并在变化时同步成 bool。
                            let options = vec![
                                SelectOption::new("no", "否"),
                                SelectOption::new("yes", "是"),
                            ];
                            view! {
                                <Select
                                    value=temp_not_value
                                    options=options
                                    class="tr-filter__select"
                                    on_change=move |value: String| {
                                        temp_not.set(value == "yes")
                                    }
                                />
                            }
                        }}
                    </div>
                </div>

                <div class="tr-filter__field">
                    <div class="tr-filter__label">"描述包含"</div>
                    <Input value=temp_description placeholder="输入关键词" />
                </div>

                <Button
                    variant=ButtonVariant::Dashed
                    block=true
                    class="tr-filter__add"
                    on_click=move || add_condition()
                >
                    "+ 添加筛选条件"
                </Button>

                <Show when=move || !draft.get().is_empty()>
                    <div class="tr-filter__list">
                        <For each=move || draft.get() key=|item| condition_key(item) children=move |item: QueryConditionItem| {
                            let index = draft.with(|list| list.iter().position(|entry| *entry == item));
                            let type_class = format!("condition-type-tag condition-type-tag--{}", item.transaction_type);
                            view! {
                                <div class="tr-filter__item">
                                    <div class="tr-filter__item-body">
                                        <span class=type_class.clone()>
                                            {format::transaction_type_text(&item.transaction_type)}
                                        </span>
                                        {(!item.category.is_empty()).then(|| {
                                            view! {
                                                <span class="tr-filter__separator">"/"</span>
                                                <span>{item.category.clone()}</span>
                                            }
                                        })}
                                        {(!item.tags.is_empty()).then(|| {
                                            view! {
                                                <span class="tr-filter__separator">"/"</span>
                                                <span class="tr-filter__tags">{item.tags.join(", ")}</span>
                                            }
                                        })}
                                        {(!item.description.is_empty()).then(|| {
                                            let text = format!("\"{}\"", item.description);
                                            view! {
                                                <span class="tr-filter__separator">"/"</span>
                                                <span class="tr-filter__desc">{text}</span>
                                            }
                                        })}
                                        {item.tag_not.then(|| {
                                            view! {
                                                <span class="tr-filter__separator">"/"</span>
                                                <Tag kind=TagKind::Expense>"取反"</Tag>
                                            }
                                        })}
                                    </div>
                                    <Button
                                        variant=ButtonVariant::TextDanger
                                        size=ButtonSize::Small
                                        on_click=move || {
                                            if let Some(index) = index {
                                                draft.update(|list| {
                                                    if index < list.len() {
                                                        list.remove(index);
                                                    }
                                                });
                                            }
                                        }
                                    >
                                        "删除"
                                    </Button>
                                </div>
                            }
                        } />
                    </div>
                </Show>

                <div class="tr-filter__footer">
                    <Button
                        variant=ButtonVariant::Secondary
                        size=ButtonSize::Small
                        on_click=move || {
                            // 清空草稿里的全部条件
                            draft.set(Vec::new());
                            reset_filter_inputs();
                        }
                    >
                        "清除条件"
                    </Button>
                    <Button
                        variant=ButtonVariant::Secondary
                        size=ButtonSize::Small
                        on_click=move || {
                            // 丢弃本次未确认的编辑并关闭
                            draft.set(Vec::new());
                            reset_filter_inputs();
                            open.set(false);
                        }
                    >
                        "取消"
                    </Button>
                    <Button
                        variant=ButtonVariant::Primary
                        size=ButtonSize::Small
                        on_click=move || {
                            on_apply.run(draft.get_untracked());
                            open.set(false);
                        }
                    >
                        "确认"
                    </Button>
                </div>
            </div>
        </Modal>
    }
    .into_any()
}

/// 条件项的 `For` key（按条件各字段拼出的签名）。
fn condition_key(item: &QueryConditionItem) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        item.transaction_type,
        item.category,
        item.tags.join(","),
        item.tag_policy,
        item.tag_not,
        item.description
    )
}

// ==================================================================== 排序弹窗

/// 排序弹窗里的一行（每个字段各持一个信号）。
#[derive(Clone, Copy)]
struct SortRow {
    field: RwSignal<String>,
    order: RwSignal<String>,
}

impl SortRow {
    fn new(field: &str, order: &str) -> Self {
        Self {
            field: RwSignal::new(field.to_string()),
            order: RwSignal::new(order.to_string()),
        }
    }

    /// 取当前值的快照（方法名避开 `to_*`：`SortRow` 是 `Copy`，
    /// `to_*` 约定要求按值接收 `self`，而这里只需要读信号）。
    fn snapshot(&self) -> SortItem {
        SortItem {
            field: self.field.get_untracked(),
            order: self.order.get_untracked(),
        }
    }
}

/// 排序弹窗。
fn sort_modal(
    open: RwSignal<bool>,
    applied: RwSignal<Vec<SortItem>>,
    on_apply: Callback<Vec<SortItem>>,
) -> AnyView {
    let draft = RwSignal::new(Vec::<SortRow>::new());

    // 打开时回填当前排序
    Effect::new(move |_| {
        if open.get() {
            draft.set(
                applied
                    .get_untracked()
                    .into_iter()
                    .map(|item| SortRow::new(&item.field, &item.order))
                    .collect(),
            );
        }
    });

    let add_item = move || {
        let used: Vec<String> =
            draft.with(|list| list.iter().map(|row| row.field.get_untracked()).collect());
        if used.len() >= 4 {
            return;
        }
        // 追加第一个还没用过的字段，方向默认「降序」
        if let Some((field, _)) = SORT_FIELDS
            .iter()
            .find(|(field, _)| !used.contains(&(*field).to_string()))
        {
            draft.update(|list| list.push(SortRow::new(field, "desc")));
        }
    };

    view! {
        <Modal
            open=Signal::derive(move || open.get())
            title="排序"
            width=500
            footer=false
            on_close=move || open.set(false)
        >
            <div class="tr-sort">
                <div class="tr-sort__list">
                    {move || {
                        draft
                            .get()
                            .into_iter()
                            .enumerate()
                            .map(|(index, row)| sort_row_view(index, row, draft))
                            .collect_view()
                    }}
                </div>
                <div>
                    <Button
                        variant=ButtonVariant::Link
                        size=ButtonSize::Small
                        disabled=Signal::derive(move || draft.with(|list| list.len()) >= 4)
                        on_click=move || add_item()
                    >
                        <span class="tr-sort__add-icon">{icons::icon(Icon::Plus)}</span>
                        "添加排序条件"
                    </Button>
                </div>
                <div class="tr-sort__actions">
                    <Button
                        variant=ButtonVariant::Secondary
                        size=ButtonSize::Small
                        on_click=move || {
                            // 回到「日期 + 降序」单条
                            draft.set(vec![SortRow::new("transactionAt", "desc")]);
                        }
                    >
                        "重置"
                    </Button>
                    <Button
                        variant=ButtonVariant::Primary
                        size=ButtonSize::Small
                        on_click=move || {
                            let items: Vec<SortItem> = draft.with(|list| {
                                list.iter().map(SortRow::snapshot).collect()
                            });
                            on_apply.run(items);
                            open.set(false);
                        }
                    >
                        "应用"
                    </Button>
                </div>
            </div>
        </Modal>
    }
    .into_any()
}

/// 排序弹窗的一行（优先级序号 + 字段 + 方向 + 删除）。
fn sort_row_view(index: usize, row: SortRow, draft: RwSignal<Vec<SortRow>>) -> AnyView {
    // 同一行之前的字段不重复出现
    let field_options = move || {
        let used: Vec<String> = draft.with(|list| {
            list.iter()
                .take(index)
                .map(|entry| entry.field.get_untracked())
                .collect()
        });
        SORT_FIELDS
            .iter()
            .filter(|(value, _)| !used.contains(&(*value).to_string()))
            .map(|(value, label)| SelectOption::new(*value, *label))
            .collect::<Vec<_>>()
    };

    view! {
        <div class="tr-sort__item">
            <span class="tr-sort__priority">{index + 1}</span>
            {move || {
                let options = field_options();
                view! {
                    <Select
                        value=row.field
                        options=options
                        placeholder="选择字段"
                        class="tr-sort__select tr-sort__select--field"
                    />
                }
            }}
            <Select
                value=row.order
                options=vec![
                    SelectOption::new("asc", "升序"),
                    SelectOption::new("desc", "降序"),
                ]
                class="tr-sort__select tr-sort__select--order"
            />
            <Button
                variant=ButtonVariant::TextDanger
                size=ButtonSize::Small
                icon_only=true
                title="删除该排序条件"
                disabled=Signal::derive(move || draft.with(|list| list.len()) <= 1)
                on_click=move || {
                    // 至少保留一行；删完重建为全新的行信号
                    draft.update(|list| {
                        if list.len() > 1 && index < list.len() {
                            list.remove(index);
                            let rebuilt: Vec<SortRow> = list
                                .drain(..)
                                .map(|row| {
                                    SortRow::new(
                                        &row.field.get_untracked(),
                                        &row.order.get_untracked(),
                                    )
                                })
                                .collect();
                            list.extend(rebuilt);
                        }
                    });
                }
            >
                {icons::icon(Icon::Trash)}
            </Button>
        </div>
    }
    .into_any()
}

// ==================================================================== 关联关键事件弹窗

/// 关联关键事件弹窗。
fn link_modal(
    open: RwSignal<bool>,
    target: RwSignal<Option<TransactionRecordDto>>,
    link_date: RwSignal<String>,
    on_confirm: Callback<()>,
    on_unlink: Callback<()>,
) -> AnyView {
    let has_link = move || {
        target
            .get()
            .map(|record| !record.key_event_date.is_empty())
            .unwrap_or(false)
    };

    view! {
        <Modal
            open=Signal::derive(move || open.get())
            title="关联关键事件"
            width=480
            ok_text="确认关联"
            cancel_text="取消"
            on_close=move || {
                open.set(false);
                target.set(None);
            }
            on_ok=move || on_confirm.run(())
        >
            <Form layout=FormLayout::Vertical>
                <FormItem label="选择日期">
                    <DatePicker value=link_date placeholder="选择要关联的日期" />
                </FormItem>
            </Form>
            <Show when=has_link>
                <div class="tr-link__footer">
                    <Button
                        variant=ButtonVariant::PrimaryDanger
                        size=ButtonSize::Small
                        on_click=move || on_unlink.run(())
                    >
                        "解除关联"
                    </Button>
                </div>
            </Show>
        </Modal>
    }
    .into_any()
}

// ==================================================================== 内联 SVG

/// 编辑图标的 path。
const EDIT_PATHS: [&str; 1] = [
    "M257.7 752c2 0 4-.2 6-.5L431.9 722c2-.4 3.9-1.3 5.3-2.8l423.9-423.9a9.96 9.96 0 000-14.1L694.9 114.9c-1.9-1.9-4.4-2.9-7.1-2.9s-5.2 1-7.1 2.9L256.8 538.8c-1.5 1.5-2.4 3.3-2.8 5.3l-29.5 168.2a33.5 33.5 0 009.4 29.8c6.6 6.4 14.9 9.9 23.8 9.9zm67.4-174.4L687.8 215l73.3 73.3-362.7 362.6-88.9 15.7 15.6-89zM880 836H144c-17.7 0-32 14.3-32 32v36c0 4.4 3.6 8 8 8h784c4.4 0 8-3.6 8-8v-36c0-17.7-14.3-32-32-32z",
];

/// 关联（链接）图标的 path。
const LINK_PATHS: [&str; 1] = [
    "M574 665.4a8.03 8.03 0 00-11.3 0L446.5 781.6c-53.8 53.8-144.8 53.9-198.7 0C221 755 208 721.5 208 686s13-69 39.8-95.7l115.1-115.2c3.1-3.1 3.1-8.2 0-11.3l-28.3-28.3a8.03 8.03 0 00-11.3 0L208 550.6c-37.4 37.4-58 87.2-58 140.1s20.6 102.7 58 140.1c38.7 38.7 89.5 58 140.1 58s101.4-19.3 140.1-58l116.2-116.2c3.1-3.1 3.1-8.2 0-11.3L574 665.4zM816 182.5c-38.7-38.7-89.5-58-140.1-58s-101.5 19.3-140.1 58L419.6 298.7c-3.1 3.1-3.1 8.2 0 11.3l28.3 28.3c3.1 3.1 8.2 3.1 11.3 0l116.2-116.2c53.8-53.8 144.8-53.9 198.7 0 26.8 26.7 39.8 60.2 39.8 95.7s-13 69-39.8 95.7L659.6 528.7c-3.1 3.1-3.1 8.2 0 11.3l28.3 28.3c3.1 3.1 8.2 3.1 11.3 0L816 451.3c37.4-37.4 58-87.2 58-140.1s-20.6-101.5-58-138.7z",
];

/// 升序图标的 path。
const SORT_ASCENDING: [&str; 1] = [
    "M839.6 433.8L749 150.5a9.24 9.24 0 00-8.9-6.5h-77.4c-4.1 0-7.6 2.6-8.9 6.5l-91.3 283.3c-.3.9-.5 1.9-.5 2.9 0 5.1 4.1 9.3 9.3 9.3h56.4c4.2 0 7.8-2.8 9.2-6.8l17.5-61.6h89l17.3 61.5c1.3 4 4.8 6.8 9.1 6.8h61.2c1 0 1.9-.1 2.8-.4 2.8-.8 4.8-3.4 4.8-6.3-.1-1-.3-2.1-.8-3.1zm-191.1-95.8l32.9-115.8h1.3l33.5 115.8h-67.7zM533 793h-229V291c0-4.4-3.6-8-8-8h-56c-4.4 0-8 3.6-8 8v502H3c-6.2 0-9.4 7.4-5.1 11.8l265 264.5c2.9 3 7.7 3 10.6 0l265-264.5c4.3-4.4 1.1-11.8-5.5-11.8z",
];

/// 降序图标的 path。
const SORT_DESCENDING: [&str; 1] = [
    "M839.6 433.8L749 150.5a9.24 9.24 0 00-8.9-6.5h-77.4c-4.1 0-7.6 2.6-8.9 6.5l-91.3 283.3c-.3.9-.5 1.9-.5 2.9 0 5.1 4.1 9.3 9.3 9.3h56.4c4.2 0 7.8-2.8 9.2-6.8l17.5-61.6h89l17.3 61.5c1.3 4 4.8 6.8 9.1 6.8h61.2c1 0 1.9-.1 2.8-.4 2.8-.8 4.8-3.4 4.8-6.3-.1-1-.3-2.1-.8-3.1zm-191.1-95.8l32.9-115.8h1.3l33.5 115.8h-67.7zM3 795.7l265 264.5c2.9 3 7.7 3 10.6 0l265-264.5c4.3-4.4 1.1-11.8-5.5-11.8H304V291c0-4.4-3.6-8-8-8h-56c-4.4 0-8 3.6-8 8v502.7H8.5c-6.6 0-9.8 7.4-5.5 11.8z",
];

/// 同步图标的 path。
const SYNC_PATHS: [&str; 1] = [
    "M925.7 381.8l-59.3-10.4a8 8 0 00-9.1 6.1l-6.8 31.6a353.3 353.3 0 00-114.3-144.4 352.8 352.8 0 00-112.4-75.9c-43.6-18.4-89.9-27.8-137.6-27.8-89.6 0-174.1 32.7-240.2 92.6l-46.5-36.4c-5-3.9-12.3-.3-12.3 6.1l-1.1 148.8c0 5.1 4.9 8.8 9.8 7.6l155.3-38a8 8 0 002.9-14l-49.9-39a277.5 277.5 0 01133.2-72.9 289.6 289.6 0 01112.4 0 289.6 289.6 0 01112.4 45.9 277.5 277.5 0 0189.9 111.6 276.7 276.7 0 0127.7 70.3l-17.3 61.4c-1.4 4 2.2 7.9 6.3 7.9h73.3c4.3 0 7.9-2.8 9.2-6.9l20.4-72.3 1.9-7.1c1.1-4-2-7.8-6.1-8.5zM512 754a277.5 277.5 0 01-133.2-72.9 277.5 277.5 0 01-89.9-111.6 276.7 276.7 0 01-27.7-70.3l17.3-61.4c1.4-4-2.2-7.9-6.3-7.9h-73.3c-4.3 0-7.9 2.8-9.2 6.9l-22.3 79.4c-1.1 4 2 7.8 6.1 8.5l59.3 10.4a8 8 0 009.1-6.1l6.8-31.6a353.3 353.3 0 00114.3 144.4 352.8 352.8 0 00112.4 75.9c43.6 18.4 89.9 27.8 137.6 27.8 89.6 0 174.1-32.7 240.2-92.6l46.5 36.4c5 3.9 12.3.3 12.3-6.1l1.1-148.8c0-5.1-4.9-8.8-9.8-7.6l-155.3 38a8 8 0 00-2.9 14l49.9 39a277.5 277.5 0 01-133.2 72.9 289.6 289.6 0 01-112.4 0z",
];

/// 渲染一枚内联 SVG（`viewBox` 固定为 `64 64 896 896`）。
fn svg_icon(paths: &[&'static str]) -> AnyView {
    let first = paths.first().copied().unwrap_or_default();
    view! {
        <svg
            viewBox="64 64 896 896"
            focusable="false"
            aria-hidden="true"
            fill="currentColor"
            class="icon"
        >
            <path d=first></path>
        </svg>
    }
    .into_any()
}
