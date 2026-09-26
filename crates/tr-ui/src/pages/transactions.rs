//! 记账 · **记录**子功能（P6-a 完整版：只读列表 + 全部写操作）。
//!
//! 它是记账页（[`crate::pages::accounting`]）三个子功能之一，只提供版心里的
//! 「工具栏 + 内容区 + 底栏」，标题栏与左侧子功能图标条由 `FeaturePage` 统一渲染。
//!
//! ## 文件结构
//!
//! | 组成部分 | 说明 |
//! |---|---|
//! | [`RecordSub`] | 子功能编排：工具栏 / 时间范围 / 分页 / 空态三态 / 三个悬浮按钮 / 关联弹窗 |
//! | [`table_view`] / [`row_view`] | 表格：8 列（日期/类型/分类/标签/描述/金额/标记/操作）、列宽、行底色、行内「编辑 / 关联 / 同步 / 删除」 |
//! | [`record_modal`] | 记一笔 / 编辑（模板套用、类型→分类→标签联动、离群值） |
//! | [`sort_modal`] / [`sort_row_view`] | 排序弹窗及其每一行 |
//! | [`filter_modal`] | 筛选弹窗 |
//! | [`TimeRangePicker`]（共享组件）| 时间范围选择：日 / 月 / 年三档区间 + 按粒度的快捷项 |
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
//! 9. **同步气泡的收起**：开关挂在图标按钮本身、不是外层 `<span>` —— 挂外层会让面板内的每次
//!    点击冒泡上来把面板**再打开一次**；选中目标账本与点面板以外的任何地方都立即收起。
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
use tr_domain::consts::TAG_POLICY_ANY;
use tr_domain::dto::{
    CategoryDto, QueryConditionSortField, TagDto, TransactionRecordDto, TransactionTemplateDto,
};
use tr_domain::models::QueryConditionItem;
use tr_domain::money::yuan_to_cents;

use crate::api;
// 时间范围选择器是共享组件（与分析子功能共用）；其中三个日期算术 helper 本页也要用
use crate::components::ui::time_range_picker::{normalize_range, shift_period, split_ymd};
use crate::components::ui::{
    Button, ButtonSize, ButtonVariant, CheckboxGroup, DatePicker, Empty, FeaturePage, Form,
    FormItem, FormLayout, Input, Modal, ModalSize, Pagination, Segmented, SegmentedOption, Select,
    SelectOption, Spin, Tag, TagKind, TimeRangePicker,
};
use crate::error_handler::notify_error;
use crate::format;
use crate::icons::{self, Icon};
use crate::notify::Notifier;
use crate::store::AppStores;
use crate::time::{format_timestamp, range_to_seconds, today_ymd, ymd_to_seconds};

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

/// 排序字段选项（日期 / 金额 / 分类 / 类型）。
const SORT_FIELDS: [(&str, &str); 4] = [
    ("transactionAt", "日期"),
    ("price", "金额"),
    ("category", "分类"),
    ("transactionType", "类型"),
];

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

/// 记账页的「记录」子功能：消费记录列表。
///
/// 只负责工具栏 + 内容区（+ 底栏），版心与左侧子功能图标条由 `FeaturePage` 提供。
#[component]
pub fn RecordSub(sub: RwSignal<super::accounting::SubFunction>) -> impl IntoView {
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

        // 区间**只点了起点**（终点待选）时不发查询：空终点会被下游当成"不设上界"，
        // 列表会突然变成全部数据（实测：共 729 条 → 7312 条）。
        // 保持上一次已提交的区间，等第二次点击落定再查。
        if !current.start.is_empty() && current.end.is_empty() {
            return prev.unwrap_or(QueryInputs { page: 1, ..current });
        }

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
            Notifier::global().warning("尚未选择账本".to_string(), None);
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

    // ---- 关联事件 ----
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

    // 版心三块：工具栏 / 内容区各自建好视图再交给 `FeaturePage`（骨架见 components/ui/feature_page.rs）
    let toolbar = view! {
        <div class="tr-toolbar">
            <div class="tr-toolbar-left">
                <TimeRangePicker
                    mode=range_mode
                    start=range_start
                    end=range_end
                />
            </div>
        <div class="tr-toolbar-right">
            <Button
                variant=ButtonVariant::Secondary
                class="tr-tool-btn"
                aria_label="排序"
                title=Signal::derive(move || {
                    let fields = sort_items.get();
                    let first = fields
                        .first()
                        .map(|item| {
                            SORT_FIELDS
                                .iter()
                                .find(|(key, _)| *key == item.field)
                                .map(|(_, label)| *label)
                                .unwrap_or(item.field.as_str())
                        })
                        .unwrap_or("日期");
                    let direction = if fields.first().map(|item| item.order == "asc").unwrap_or(false) {
                        "升序"
                    } else {
                        "降序"
                    };
                    format!("排序：{first} {direction}")
                })
                on_click=move || sort_open.set(true)
            >
                <span class="tr-btn-icon">
                    {move || {
                        let ascending = sort_items
                            .get()
                            .first()
                            .map(|item| item.order == "asc")
                            .unwrap_or(false);
                        if ascending {
                            icons::icon(Icon::SortAsc)
                        } else {
                            icons::icon(Icon::SortDesc)
                        }
                    }}
                </span>
                "排序"
            </Button>
            <Button
                variant=ButtonVariant::Secondary
                class="tr-tool-btn"
                aria_label="筛选"
                title="筛选条件"
                on_click=move || filter_open.set(true)
            >
                <span class="tr-btn-icon">{icons::icon(Icon::Search)}</span>
                "筛选"
                <Show when=move || !condition_items.get().is_empty()>
                    <span class="tr-btn-badge">
                        {move || condition_items.get().len()}
                    </span>
                </Show>
            </Button>
            <Button
                variant=ButtonVariant::Primary
                disabled=Signal::derive(move || {
                    stores.current_ledger_id.get().is_empty()
                })
                on_click=move || request_create()
            >
                "记一笔"
            </Button>
            </div>
        </div>
    }.into_any();

    let content = view! {
        <div class="tr-body">
            <div class="tr-content">
                <Show when=move || !items.get().is_empty() fallback=empty_state>
                    {table_view}
                </Show>
            </div>

            <div class="tr-footer">
                // 结果条数就放在分页组件的**左侧**（底栏只留收支合计）
                <span class="tr-footer-total">{move || format!("共 {} 条", total.get())}</span>
                <Pagination
                    page=page
                    total_pages=Signal::derive(move || total_pages.get())
                    page_size=page_size
                    page_size_options=PAGE_SIZE_OPTIONS.to_vec()
                    disabled=Signal::derive(move || loading.get())
                />
            </div>
        </div>
    }
    .into_any();

    view! {
        <FeaturePage
            title=super::accounting::PAGE_TITLE
            rail=view! { <super::accounting::SubFunctionRail sub=sub /> }.into_any()
            toolbar=toolbar
            content=content
            // 底栏只放收支合计，且**贴右**（结果条数在分页行里，见上面的 `.tr-footer`）
            footer=statistics_bar()
            footer_end=true
        />

        // ---- 排序弹窗 ----
        {sort_modal(sort_open, sort_items, Callback::new(apply_sort))}

        // ---- 筛选弹窗 ----
        {filter_modal(filter_open, condition_items, Callback::new(move |next| {
            condition_items.set(next)
        }))}

        // ---- 关联事件弹窗 ----
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
            title="暂无分类"
            size=ModalSize::Small
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
        if let Some((from, to)) = range_to_seconds(&input.start, &input.end) {
            condition.ts_range = vec![from, to];
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

/// 区间是否落在同一天。
fn same_day(start: &str, end: &str) -> bool {
    start == end
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
                        <th class="tr-cell--center" style="width: 180px;">"标签"</th>
                        <th>"描述"</th>
                        <th class="tr-cell--center" style="width: 110px;">"金额"</th>
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
    let record_id_backdrop = record.transaction_id.clone();
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
            <td class="tr-cell tr-cell--center">
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
            <td class="tr-cell tr-cell--center">
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
                    <crate::components::ui::Tooltip title="编辑记录">
                        <crate::components::ui::IconButton
                            label="编辑记录"
                            on_click=move |_| on_action.run((RowAction::Edit, edit_record.clone()))
                        >
                            {icons::icon(Icon::Edit)}
                        </crate::components::ui::IconButton>
                    </crate::components::ui::Tooltip>

                    // 2. 关联（已关联时 tooltip 显示日期）
                    <crate::components::ui::Tooltip title=link_title>
                        <crate::components::ui::IconButton
                            label=if has_key_event { "修改关联" } else { "关联事件" }
                            on_click=move |_| on_action.run((RowAction::Link, link_record.clone()))
                        >
                            {icons::icon(Icon::Link)}
                        </crate::components::ui::IconButton>
                    </crate::components::ui::Tooltip>

                    // 3. 同步到其他账本（点击展开账本列表）
                    <crate::components::ui::Tooltip title="同步到其他账本">
                        <span class="tr-sync">
                            <crate::components::ui::IconButton
                                label="同步到其他账本"
                                disabled=Signal::derive(move || {
                                    syncing_id.get() == record_id_disabled
                                })
                                // 开关必须挂在按钮自己身上：面板就在同一个 `<span>` 里，把开关挂在外层
                                // 会让面板内的每一次点击都冒泡上去**再把面板打开一次** —— 这正是
                                // "点了账本名却关不掉、得再点一次图标"的原因。
                                on_click=move |_| {
                                    sync_popover_id.update(|current| {
                                        if *current == record_id_click {
                                            current.clear();
                                        } else {
                                            *current = record_id_click.clone();
                                        }
                                    });
                                }
                            >
                                <span
                                    class="tr-sync-icon"
                                    class:is-spinning=move || syncing_id.get() == record_id_spinning
                                >
                                    // 原来用 1024 格的 `Sync`（套错 viewBox 被裁，看着就是"图标异常"）；
                                    // 换成 896 格、笔画更简的 `Reload`，16px 下更清楚
                                    {icons::icon(Icon::Reload)}
                                </span>
                            </crate::components::ui::IconButton>

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

                    // 3b. 点面板以外的任何地方都收起（与下拉 / 日期选择同一套 `.ui-select__backdrop`：
                    // fixed 铺满视口、z-index 低于面板，所以不会挡住面板自身的点击）。
                    // ⚠ 必须挂在 `Tooltip` **外面**：`Tooltip` 的气泡是纯 CSS `:hover` 驱动，
                    // 铺满视口的遮罩留在它里面会让面板一打开就"顺带"把提示气泡也悬停出来。
                    <Show when=move || sync_popover_id.get() == record_id_backdrop>
                        {crate::components::ui::backdrop(UnsyncCallback::new(move |()| {
                            sync_popover_id.set(String::new())
                        }))}
                    </Show>

                    // 4. 删除（不显示取消按钮）
                    <crate::components::ui::Popconfirm
                        title="删除这条消费记录？"
                        ok_text="删除"
                        show_cancel=false
                        class="ui-popconfirm--end"
                        on_confirm=move || {
                            on_action.run((RowAction::Delete, delete_record.clone()))
                        }
                    >
                        <crate::components::ui::IconButton
                            variant=crate::components::ui::IconButtonVariant::Danger
                            label="删除记录"
                        >
                            {icons::icon(Icon::Trash)}
                        </crate::components::ui::IconButton>
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

/// 版心底栏里的**收支合计**（内容由 `FeaturePage` 的 `footer` 插槽包在 `.page-footer-bar` 里，
/// 这里只给"里面的东西"；结果条数「共 N 条」在分页行里，见 [`record_footer_row`]）。
///
/// 这一页**有**底栏而其它子功能没有，判断依据是"这个功能用不用得上"：
/// 收支合计是列表页的产物，其余子功能（分析 / 标签 / 模板）与其它顶级功能
/// （股票、事件、日记、设置）不需要，于是它们的版心直接触达窗口底边。
fn statistics_bar() -> AnyView {
    let stores = AppStores::global();
    let value = move |key: &'static str| {
        stores
            .statistics
            .with(|map| map.get(key).copied().unwrap_or(0))
    };

    view! {
        <div class="statistics-footer">
            <div class="statistics-footer-item">
                <span class="statistics-footer-item-label">"收入"</span>
                <span class="statistics-footer-item-value income">
                    {move || format::amount(value("income"))}
                </span>
            </div>
            <div class="statistics-footer-divider"></div>
            <div class="statistics-footer-item">
                <span class="statistics-footer-item-label">"支出"</span>
                <span class="statistics-footer-item-value expense">
                    {move || format::amount(value("expense"))}
                </span>
            </div>
            <div class="statistics-footer-divider"></div>
            <div class="statistics-footer-item">
                <span class="statistics-footer-item-label">"转账"</span>
                <span class="statistics-footer-item-value transfer">
                    {move || format::amount(value("transfer"))}
                </span>
            </div>
        </div>
    }
    .into_any()
}

/// 空态（未选择账本 / 加载中 / 查询失败 / 无记录引导）。
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
    // **一个账本都没有**：先说这件事 —— 没有账本时"时间范围 / 记一笔 / 筛选"都无从谈起。
    //
    // 这一支必须放在**最前面**：账本被删空之后，`items` / `loaded` / `has_any_records`
    // 可能还留着上一次查询的旧值，于是会错显示成"这段时间还没有记录"（实测就是这个现象）。
    // 账本列表非空时 `AppStores::set_ledgers` 一定会选中第一个，所以"未选中"就等于"一个都没有"。
    if AppStores::global().current_ledger_id.get().is_empty() {
        return view! {
            <div class="empty-guide">
                <span class="empty-guide-icon">{icons::icon(Icon::Book)}</span>
                <p class="empty-guide-title">"未选择账本"</p>
                <p class="empty-guide-text">
                    "请在左上角「选择账本」里新建或选择一个账本；记录、统计与图表都按账本分开。"
                </p>
            </div>
        }
        .into_any();
    }

    if loading.get() || !loaded.get() {
        return view! {
            <div class="empty-guide">
                <Spin spinning=true />
                <span class="empty-guide-loading">"正在加载…"</span>
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
                    "收入与支出会自动按月汇总，可在「分析」查看趋势。"
                </p>
                <div class="empty-guide-actions">
                    <Show when=move || has_any_categories.get() == Some(false)>
                        <Button
                            variant=ButtonVariant::Secondary
                            loading=Signal::derive(move || init_loading.get())
                            on_click=move || on_init.run(())
                        >
                            "初始化默认分类"
                        </Button>
                    </Show>
                    <Button
                        variant=ButtonVariant::Primary
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
            <p class="empty-guide-text">"换个时间范围，或直接记一笔。"</p>
            <div class="empty-guide-actions">
                <Button
                    variant=ButtonVariant::Secondary
                    on_click=move || on_last_month.run(())
                >
                    "上月"
                </Button>
                <Button
                    variant=ButtonVariant::Secondary
                    on_click=move || on_this_year.run(())
                >
                    "今年"
                </Button>
                <Button
                    variant=ButtonVariant::Primary
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
            Notifier::global().warning("尚未选择账本".to_string(), None);
            return;
        }
        let current_type = transaction_type.get_untracked();
        if current_type.is_empty() {
            Notifier::global().error("请选择消费类型".to_string(), None);
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
            Notifier::global().error("请选择消费分类".to_string(), None);
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
        "编辑记录".to_string()
    } else {
        "新增记录".to_string()
    };

    view! {
        <Modal
            open=Signal::derive(move || open.get())
            title=modal_title
            size=ModalSize::Large
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
                                    placeholder="选择模板"
                                    allow_clear=true
                                    class="tr-modal-template-select"
                                    on_change=move |id: String| apply_template(id)
                                />
                            }
                        }}
                        <Button
                            variant=ButtonVariant::Secondary
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
                                placeholder="选择消费分类"
                                class="tr-modal-full"
                            />
                        }
                    }}
                    <Show when=move || categories.get().is_empty() && !transaction_type.get().is_empty()>
                        <p class="tr-modal-hint">
                            "该类型还没有消费分类，请先在「分类标签」页创建分类或初始化默认分类。"
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
                    <Input value=price_text placeholder="0.00" />
                </FormItem>
            </Form>

            <div class="tr-modal-footer">
                <Button
                    variant=ButtonVariant::Secondary
                    on_click=move || open.set(false)
                >
                    "取消"
                </Button>
                <Button
                    variant=ButtonVariant::Primary
                    loading=Signal::derive(move || saving.get())
                    on_click=move || confirm()
                >
                    "保存"
                </Button>
            </div>
        </Modal>

        // 保存为模板（子弹窗）
        <Modal
            open=Signal::derive(move || save_template_open.get())
            title="保存为模板"
            size=ModalSize::Small
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
        return Err("请输入不小于 0 的金额，最多两位小数".to_string());
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
    let placeholder = placeholder.unwrap_or_else(|| "选择".to_string());

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
            title="筛选条件"
            size=ModalSize::Large
            footer=false
            on_close=move || open.set(false)
        >
            <div class="tr-filter">
                <div class="tr-filter__field">
                    <div class="tr-filter__label">"消费类型"</div>
                    <Select
                        value=temp_type
                        options=transaction_type_options()
                        placeholder="选择消费类型"
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
                                placeholder="选择消费分类"
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
                        placeholder="选择标签"
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
                                SelectOption::new("no", "包含"),
                                SelectOption::new("yes", "排除"),
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
                    "+ 添加条件"
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
                                                <Tag kind=TagKind::Expense>"标签取反"</Tag>
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

/// 排序条件最多 4 条。
const MAX_SORT_ROWS: usize = 4;

/// 排序弹窗。
fn sort_modal(
    open: RwSignal<bool>,
    applied: RwSignal<Vec<SortItem>>,
    on_apply: Callback<Vec<SortItem>>,
) -> AnyView {
    // 行信号的**池**：最多 4 条条件，**在页面自己的 owner 下一次建好**，
    // 打开时只回填值、增删只改 `count`。
    //
    // ⚠ 绝不能像原来那样在 `Effect` 里 `SortRow::new(...)`：Effect 每重跑一次就会
    // dispose 上一次创建的值，而界面那时还在通过 `Select` 读这些信号 ——
    // 现象是**关掉排序弹窗再打开必 panic**（控制台：`you tried to access a reactive value
    // ... but it has already been disposed`，报在 `select.rs` 读 `SortRow.field`）。
    // 与"关于软件面板切走再切回变空白"是同一类坑（见 AGENTS.md）。
    let rows: [SortRow; MAX_SORT_ROWS] =
        std::array::from_fn(|_| SortRow::new("transactionAt", "desc"));
    let count = RwSignal::new(1_usize);

    // 打开时回填当前排序：已应用的写进前 N 行，其余行预填"还没用过的字段"
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        let items = applied.get_untracked();
        let used = items.len().clamp(1, MAX_SORT_ROWS);
        let mut taken: Vec<String> = items.iter().map(|item| item.field.clone()).collect();
        for (index, row) in rows.iter().enumerate() {
            match items.get(index) {
                Some(item) => {
                    row.field.set(item.field.clone());
                    row.order.set(item.order.clone());
                }
                None => {
                    // 追加行时默认取第一个没用过的字段，方向「降序」
                    let next = SORT_FIELDS
                        .iter()
                        .find(|(field, _)| !taken.contains(&(*field).to_string()))
                        .map(|(field, _)| (*field).to_string())
                        .unwrap_or_else(|| "transactionAt".to_string());
                    taken.push(next.clone());
                    row.field.set(next);
                    row.order.set("desc".to_string());
                }
            }
        }
        count.set(used);
    });

    let add_item = move || {
        let used = count.get_untracked();
        if used >= MAX_SORT_ROWS {
            return;
        }
        // 新行的字段取"前几行还没用过"的第一个
        let taken: Vec<String> = rows
            .iter()
            .take(used)
            .map(|row| row.field.get_untracked())
            .collect();
        if let Some((field, _)) = SORT_FIELDS
            .iter()
            .find(|(field, _)| !taken.contains(&(*field).to_string()))
        {
            rows[used].field.set((*field).to_string());
            rows[used].order.set("desc".to_string());
        }
        count.set(used + 1);
    };

    view! {
        <Modal
            open=Signal::derive(move || open.get())
            title="排序"
            size=ModalSize::Medium
            footer=false
            on_close=move || open.set(false)
        >
            <div class="tr-sort">
                <div class="tr-sort__list">
                    {move || {
                        let len = count.get().min(MAX_SORT_ROWS);
                        rows
                            .iter()
                            .take(len)
                            .enumerate()
                            .map(|(index, row)| sort_row_view(index, *row, count, rows))
                            .collect_view()
                    }}
                </div>
                <div class="tr-sort__actions">
                    // 「添加排序条件」从上面的链接改成按钮，放在「重置」**左侧**：
                    // 三个动作同一行、同一套按钮样式，不再是一行链接 + 一行按钮
                    <Button
                        variant=ButtonVariant::Secondary
                        disabled=Signal::derive(move || count.get() >= MAX_SORT_ROWS)
                        on_click=move || add_item()
                    >
                        <span class="tr-sort__add-icon">{icons::icon(Icon::Plus)}</span>
                        "添加排序条件"
                    </Button>
                    <Button
                        variant=ButtonVariant::Secondary
                        on_click=move || {
                            // 回到「日期 + 降序」单条（只改池里的值，不重建信号）
                            rows[0].field.set("transactionAt".to_string());
                            rows[0].order.set("desc".to_string());
                            count.set(1);
                        }
                    >
                        "重置"
                    </Button>
                    <Button
                        variant=ButtonVariant::Primary
                        on_click=move || {
                            // 只取前 count 行（信号池里后面的行不参与）
                            let len = count.get_untracked().min(MAX_SORT_ROWS);
                            let items: Vec<SortItem> =
                                rows.iter().take(len).map(SortRow::snapshot).collect();
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
///
/// `rows` / `count` 是弹窗里的**行信号池**（见 [`sort_modal`]）：删除只是把后面的值往前挪、
/// 再把 `count` 减一，**不重建信号**（重建就会再次踩上"已 dispose"那个坑）。
fn sort_row_view(
    index: usize,
    row: SortRow,
    count: RwSignal<usize>,
    rows: [SortRow; MAX_SORT_ROWS],
) -> AnyView {
    // 同一行之前的字段不重复出现
    let field_options = move || {
        let used: Vec<String> = rows
            .iter()
            .take(index)
            .map(|entry| entry.field.get_untracked())
            .collect();
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
                disabled=Signal::derive(move || count.get() <= 1)
                on_click=move || {
                    // 至少保留一行：把后面的值往前挪一格，然后少显示一行
                    let len = count.get_untracked();
                    if len <= 1 || index >= len {
                        return;
                    }
                    for i in index..len - 1 {
                        rows[i].field.set(rows[i + 1].field.get_untracked());
                        rows[i].order.set(rows[i + 1].order.get_untracked());
                    }
                    count.set(len - 1);
                }
            >
                {icons::icon(Icon::Trash)}
            </Button>
        </div>
    }
    .into_any()
}

// ==================================================================== 关联事件弹窗

/// 关联事件弹窗。
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
            title="关联事件"
            size=ModalSize::Medium
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
