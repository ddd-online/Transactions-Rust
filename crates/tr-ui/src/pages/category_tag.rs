//! 记账 · **标签**子功能（类型卡片 + 分类 / 标签两栏联动 + 拖拽排序）。
//!
//! 它是记账页（[`crate::pages::accounting`]）三个子功能之一，只提供版心里的
//! 「工具栏 + 内容区」，标题栏与左侧子功能图标条由 `FeaturePage` 统一渲染。
//!
//! 结构（固定）：工具栏（类型卡片）+ 左栏分类 / 右栏标签。
//!
//! 本实现的**设计取舍**（均有理由，不改变对外契约）：
//!
//! 1. **账本缺失时的引导**：各栏除了禁用按钮，还额外给出「请先选择工作空间」提示，
//!    避免出现"按钮全灰但不知道为什么"的死界面。
//! 2. **类型卡片不预取颜色**：卡片颜色由
//!    `--transactions-color-{expense,income,transfer}` 令牌 + `color-mix` 得出（令牌是设计系统
//!    的唯一取值来源，禁止裸 hex）。
//! 3. **分类重排的本地写回**：新顺序整体 `set` 回信号，并**只对 `sortOrder`
//!    确实变化的项**发请求。
//! 4. **删除确认用 `Modal`**：对应 `Modal` 的 `ok_danger=true`（已由组件补丁提供），
//!    不用 `Popconfirm`。
//! 5. **工具栏只有交易类型卡片**：当前账本由侧栏顶部的账本按钮显示，页面里不再重复一遍。
//!
//! 行为要点：
//! * 账本 id 或交易类型变化 → 清空选中分类与标签 → 重新取分类并检查账本内是否已有分类
//! * 取分类时会把**每个分类**的标签一次性查出来缓存；切换分类不再发请求
//! * 空名称静默返回，重名分别提示「该分类已存在」「该标签已存在」

use leptos::prelude::*;
use leptos::tachys::view::any_view::{AnyView, IntoAny};
use tr_domain::dto::{CategoryDto, TagDto};

use crate::api;
use crate::components::ui::{
    Button, ButtonSize, ButtonVariant, DragSortItem, DragSortState, FeaturePage, IconButton,
    IconButtonVariant, Input, Modal,
};
use crate::error_handler::{get_error_message, notify_error};
use crate::format;
use crate::icons::{self, Icon};
use crate::notify::Notifier;
use crate::store::AppStores;

/// 交易类型顺序（支出 / 收入 / 转账，顺序即渲染顺序）。
const TRANSACTION_TYPES: [(&str, &str); 3] = [
    ("expense", "支出"),
    ("income", "收入"),
    ("transfer", "转账"),
];

/// 名称最长 20 字（输入框 `maxlength`）。
const NAME_MAX_LENGTH: u32 = 20;

// ------------------------------------------------------------------ 文案常量
//
// 全部是固定文案，集中在此便于统一修改。

/// 「新增分类」
const TEXT_ADD_CATEGORY: &str = "新增分类";
/// 「新增标签」
const TEXT_ADD_TAG: &str = "新增标签";
/// 分类栏标题
const TEXT_COLUMN_CATEGORY: &str = "分类";
/// 标签栏兜底标题（未选中分类时才显示）
const TEXT_COLUMN_TAG: &str = "标签";

/// 新增分类弹窗标题
const TEXT_MODAL_ADD_CATEGORY: &str = "新增分类";
/// 新增标签弹窗标题
const TEXT_MODAL_ADD_TAG: &str = "新增标签";
/// 删掉分类弹窗标题
const TEXT_MODAL_DELETE_CATEGORY: &str = "删除分类";
/// 删掉标签弹窗标题
const TEXT_MODAL_DELETE_TAG: &str = "删除标签";

/// 初始化空态主文案
const TEXT_EMPTY_INIT: &str = "暂无分类标签";
/// 当前类型无分类、但账本里有分类
const TEXT_EMPTY_CATEGORY: &str = "暂无分类";
/// 选中了分类但该分类下没有标签
const TEXT_EMPTY_TAG: &str = "暂无标签";
/// 没有选中分类时的引导
const TEXT_EMPTY_TAG_GUIDE: &str = "选择分类查看标签";
/// 账本缺失引导（固定文案，见设计取舍 1）
const TEXT_EMPTY_LEDGER: &str = "请先选择工作空间";

/// 「该分类已存在」
const TEXT_CATEGORY_EXISTS: &str = "该分类已存在";
/// 「该标签已存在」
const TEXT_TAG_EXISTS: &str = "该标签已存在";
/// 分类新增成功
const TEXT_CATEGORY_ADDED: &str = "分类已添加";
/// 标签新增成功
const TEXT_TAG_ADDED: &str = "标签已添加";
/// 分类删除成功
const TEXT_CATEGORY_DELETED: &str = "分类已删除";
/// 标签删除成功
const TEXT_TAG_DELETED: &str = "标签已删除";

/// 初始化按钮文案
const TEXT_INIT_BUTTON: &str = "初始化分类标签";
/// 初始化进行中
const TEXT_INIT_LOADING: &str = "初始化中…";
/// 初始化失败时 `getErrorMessage(error) || '初始化失败'` 的兜底
const TEXT_INIT_FAILED_FALLBACK: &str = "初始化失败";

/// 错误前缀：初始化
const ERR_INITIALIZE: &str = "初始化分类标签失败";
/// 错误前缀：创建分类
const ERR_CREATE_CATEGORY: &str = "创建分类失败";
/// 错误前缀：创建标签
const ERR_CREATE_TAG: &str = "创建标签失败";
/// 错误前缀：删除分类
const ERR_DELETE_CATEGORY: &str = "删除分类失败";
/// 错误前缀：删除标签
const ERR_DELETE_TAG: &str = "删除标签失败";
/// 错误前缀：分类排序
const ERR_SORT_CATEGORY: &str = "更新分类排序失败";
/// 错误前缀：标签排序
const ERR_SORT_TAG: &str = "更新标签排序失败";

/// 删除确认弹窗的目标类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CtrDeleteKind {
    Category,
    Tag,
}

/// 记账页的「标签」子功能：分类与标签两栏联动。
///
/// 只负责工具栏 + 内容区，版心与左侧子功能图标条由 `FeaturePage` 提供。
#[component]
pub fn TagSub(sub: RwSignal<super::accounting::SubFunction>) -> impl IntoView {
    let stores = AppStores::global();

    // ---- 交易类型 / 分类 / 标签 ----
    let active_type = RwSignal::new(TRANSACTION_TYPES[0].0.to_string());
    let selected_category = RwSignal::new(String::new());
    let categories = RwSignal::new(Vec::<CategoryDto>::new());
    // 每个分类名 → 该分类下的标签（取分类时一次性缓存）
    let tag_cache = RwSignal::new(std::collections::BTreeMap::<String, Vec<TagDto>>::new());
    // 账本里是否存在任意类型的分类
    let has_any_categories = RwSignal::new(false);
    let init_loading = RwSignal::new(false);

    // ---- 新增分类 / 新增标签 ----
    let open_category_modal = RwSignal::new(false);
    let category_name = RwSignal::new(String::new());
    let open_tag_modal = RwSignal::new(false);
    let tag_name = RwSignal::new(String::new());

    // ---- 删除确认弹窗 ----
    let open_delete_modal = RwSignal::new(false);
    let delete_kind = RwSignal::new(CtrDeleteKind::Category);
    let delete_target_name = RwSignal::new(String::new());
    let delete_message = RwSignal::new(String::new());

    // ---- 拖拽状态：分类栏与标签栏各一份 ----
    let category_drag = DragSortState::new();
    let tag_drag = DragSortState::new();

    // ================================================================ 联动

    // 账本 id 或交易类型变化时（首次挂载也一样）：
    // 清空选中分类与标签 → 重新取分类并检查账本内是否已有分类。
    Effect::new(move |previous: Option<(String, String)>| {
        let ledger_id = stores.current_ledger_id.get();
        let transaction_type = active_type.get();

        let previous = previous.as_ref();
        if previous != Some(&(ledger_id.clone(), transaction_type.clone())) {
            selected_category.set(String::new());
            categories.set(Vec::new());
            tag_cache.set(std::collections::BTreeMap::new());
            if ledger_id.is_empty() {
                has_any_categories.set(false);
            }
        }
        (ledger_id, transaction_type)
    });

    // 先取分类，再为每个分类取标签并缓存。
    // 分类列表与标签缓存都从信号里现读，所以账本/类型一变就会重跑。
    Effect::new(move |_previous: Option<()>| {
        let ledger_id = stores.current_ledger_id.get();
        let transaction_type = active_type.get();
        if ledger_id.is_empty() {
            categories.set(Vec::new());
            tag_cache.set(std::collections::BTreeMap::new());
            return;
        }
        leptos::task::spawn_local(async move {
            let list = match api::category::list(&transaction_type, &ledger_id).await {
                Ok(list) => list,
                Err(error) => {
                    let prefix = format!(
                        "查询 {} 消费分类失败",
                        format::transaction_type_text(&transaction_type)
                    );
                    notify_error(&prefix, &error);
                    categories.set(Vec::new());
                    tag_cache.set(std::collections::BTreeMap::new());
                    return;
                }
            };

            let mut cache = std::collections::BTreeMap::<String, Vec<TagDto>>::new();
            for category in &list {
                let category_transaction_type = format!("{}:{}", category.name, transaction_type);
                match api::tag::list(&category_transaction_type, &ledger_id).await {
                    Ok(tags) => {
                        cache.insert(category.name.clone(), tags);
                    }
                    Err(error) => {
                        let prefix = format!("查询「{}」标签失败", category.name);
                        notify_error(&prefix, &error);
                        cache.insert(category.name.clone(), Vec::new());
                    }
                }
            }
            categories.set(list);
            tag_cache.set(cache);
        });
    });

    // checkHasAnyCategories()：逐类型探测，任一非空即为 true。
    Effect::new(move |_previous: Option<()>| {
        let ledger_id = stores.current_ledger_id.get();
        if ledger_id.is_empty() {
            has_any_categories.set(false);
            return;
        }
        leptos::task::spawn_local(async move {
            for (value, label) in TRANSACTION_TYPES {
                match api::category::list(value, &ledger_id).await {
                    Ok(list) if !list.is_empty() => {
                        has_any_categories.set(true);
                        return;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        let prefix = format!("查询{label}分类失败");
                        notify_error(&prefix, &error);
                    }
                }
            }
            has_any_categories.set(false);
        });
    });

    // ================================================================ 操作

    // 重新加载分类 + 标签缓存 + `hasAnyCategories`（成功变更后共用）。
    let reload_categories = move || {
        let ledger_id = stores.current_ledger_id.get_untracked();
        let transaction_type = active_type.get_untracked();
        if ledger_id.is_empty() {
            categories.set(Vec::new());
            tag_cache.set(std::collections::BTreeMap::new());
            has_any_categories.set(false);
            return;
        }
        leptos::task::spawn_local(async move {
            let list = match api::category::list(&transaction_type, &ledger_id).await {
                Ok(list) => list,
                Err(error) => {
                    let prefix = format!(
                        "查询 {} 消费分类失败",
                        format::transaction_type_text(&transaction_type)
                    );
                    notify_error(&prefix, &error);
                    categories.set(Vec::new());
                    tag_cache.set(std::collections::BTreeMap::new());
                    return;
                }
            };

            let mut cache = std::collections::BTreeMap::<String, Vec<TagDto>>::new();
            for category in &list {
                let category_transaction_type = format!("{}:{}", category.name, transaction_type);
                match api::tag::list(&category_transaction_type, &ledger_id).await {
                    Ok(tags) => {
                        cache.insert(category.name.clone(), tags);
                    }
                    Err(error) => {
                        let prefix = format!("查询「{}」标签失败", category.name);
                        notify_error(&prefix, &error);
                        cache.insert(category.name.clone(), Vec::new());
                    }
                }
            }
            categories.set(list);
            tag_cache.set(cache);

            // checkHasAnyCategories
            for (value, label) in TRANSACTION_TYPES {
                match api::category::list(value, &ledger_id).await {
                    Ok(list) if !list.is_empty() => {
                        has_any_categories.set(true);
                        return;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        let prefix = format!("查询{label}分类失败");
                        notify_error(&prefix, &error);
                    }
                }
            }
            has_any_categories.set(false);
        });
    };

    // 选中某个分类（标签列表由渲染层从 `tag_cache` 取）。
    // 选中态由 `selected_category` 驱动，标签列表由渲染层从 `tag_cache` 取。
    let select_category = move |name: String| {
        if selected_category.get_untracked() != name {
            selected_category.set(name);
        }
    };

    // 切换交易类型
    let switch_type = move |value: String| {
        if active_type.get_untracked() != value {
            active_type.set(value);
        }
    };

    // ---- 新增分类 ----
    let open_add_category = move || {
        category_name.set(String::new());
        open_category_modal.set(true);
    };

    let confirm_add_category = move || {
        let name = category_name.get_untracked().trim().to_string();
        // 空名称静默返回，不提示
        if name.is_empty() {
            return;
        }
        if categories
            .get_untracked()
            .iter()
            .any(|category| category.name == name)
        {
            notify_error_text(TEXT_CATEGORY_EXISTS);
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        let transaction_type = active_type.get_untracked();
        if ledger_id.is_empty() {
            return;
        }
        let selected = name.clone();
        leptos::task::spawn_local(async move {
            match api::category::create(&ledger_id, &name, &transaction_type).await {
                Ok(()) => {
                    Notifier::global().success(TEXT_CATEGORY_ADDED, None);
                    open_category_modal.set(false);
                    reload_categories();
                    // 自动选中刚建的分类
                    select_category(selected);
                }
                Err(error) => notify_error(ERR_CREATE_CATEGORY, &error),
            }
        });
    };

    // ---- 新增标签 ----
    let open_add_tag = move || {
        // 未选分类时按钮禁用
        if selected_category.get_untracked().is_empty() {
            return;
        }
        tag_name.set(String::new());
        open_tag_modal.set(true);
    };

    let confirm_add_tag = move || {
        let name = tag_name.get_untracked().trim().to_string();
        if name.is_empty() {
            return;
        }
        let category = selected_category.get_untracked();
        if category.is_empty() {
            return;
        }
        let duplicate = tag_cache
            .get_untracked()
            .get(&category)
            .map(|tags| tags.iter().any(|tag| tag.name == name))
            .unwrap_or(false);
        if duplicate {
            notify_error_text(TEXT_TAG_EXISTS);
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        let transaction_type = active_type.get_untracked();
        if ledger_id.is_empty() {
            return;
        }
        let category_transaction_type = format!("{category}:{transaction_type}");
        let selected = category.clone();
        leptos::task::spawn_local(async move {
            match api::tag::create(&ledger_id, &name, &category_transaction_type).await {
                Ok(()) => {
                    Notifier::global().success(TEXT_TAG_ADDED, None);
                    open_tag_modal.set(false);
                    reload_categories();
                    select_category(selected);
                }
                Err(error) => notify_error(ERR_CREATE_TAG, &error),
            }
        });
    };

    // ---- 删除 ----
    let confirm_delete_category = move |name: String| {
        delete_kind.set(CtrDeleteKind::Category);
        delete_message.set(format!("确定删除分类「{name}」？"));
        delete_target_name.set(name);
        open_delete_modal.set(true);
    };

    let confirm_delete_tag = move |name: String| {
        delete_kind.set(CtrDeleteKind::Tag);
        delete_message.set(format!("确定删除标签「{name}」？"));
        delete_target_name.set(name);
        open_delete_modal.set(true);
    };

    let execute_delete = move || {
        let target = delete_target_name.get_untracked();
        let kind = delete_kind.get_untracked();
        let ledger_id = stores.current_ledger_id.get_untracked();
        let transaction_type = active_type.get_untracked();
        if target.is_empty() || ledger_id.is_empty() {
            return;
        }
        match kind {
            CtrDeleteKind::Category => {
                let selected = selected_category.get_untracked();
                leptos::task::spawn_local(async move {
                    match api::category::delete(&target, &transaction_type, &ledger_id).await {
                        Ok(()) => {
                            Notifier::global().success(TEXT_CATEGORY_DELETED, None);
                            open_delete_modal.set(false);
                            let was_selected = selected == target;
                            if was_selected {
                                selected_category.set(String::new());
                            }
                            reload_categories();
                        }
                        Err(error) => notify_error(ERR_DELETE_CATEGORY, &error),
                    }
                });
            }
            CtrDeleteKind::Tag => {
                let category = selected_category.get_untracked();
                if category.is_empty() {
                    return;
                }
                let category_transaction_type = format!("{category}:{transaction_type}");
                leptos::task::spawn_local(async move {
                    match api::tag::delete(&target, &category_transaction_type, &ledger_id).await {
                        Ok(()) => {
                            Notifier::global().success(TEXT_TAG_DELETED, None);
                            open_delete_modal.set(false);
                            reload_categories();
                            select_category(category);
                        }
                        Err(error) => notify_error(ERR_DELETE_TAG, &error),
                    }
                });
            }
        }
    };

    // ---- 拖拽排序 ----
    // 拖拽结束后：重排数组 → 逐项比较新旧下标，只有下标变化的项才发 `*_update_sort`
    // → 本地写回新顺序。
    // 这里把新顺序整体写回信号，并把"需要更新"的
    // 项按新下标作为载荷发出。

    let reorder_categories = move |from: usize, to: usize| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            return;
        }
        let transaction_type = active_type.get_untracked();
        let current = categories.get_untracked();
        let Some((reordered, changed)) =
            reorder_with_changes(&current, from, to, |category| category.name.clone())
        else {
            return;
        };

        categories.set(reordered);
        leptos::task::spawn_local(async move {
            for (index, name) in changed {
                // 逐条 withErrorHandling：失败只提示，不中断后续项
                if let Err(error) =
                    api::category::update_sort(&ledger_id, &name, &transaction_type, index as i32)
                        .await
                {
                    notify_error(ERR_SORT_CATEGORY, &error);
                }
            }
        });
    };

    let reorder_tags = move |from: usize, to: usize| {
        let ledger_id = stores.current_ledger_id.get_untracked();
        let category = selected_category.get_untracked();
        if ledger_id.is_empty() || category.is_empty() {
            return;
        }
        let transaction_type = active_type.get_untracked();
        let category_transaction_type = format!("{category}:{transaction_type}");

        let current = tag_cache
            .get_untracked()
            .get(&category)
            .cloned()
            .unwrap_or_default();
        let Some((reordered, changed)) =
            reorder_with_changes(&current, from, to, |tag| tag.name.clone())
        else {
            return;
        };

        let cache_key = category.clone();
        tag_cache.update(|cache| {
            cache.insert(cache_key, reordered);
        });
        leptos::task::spawn_local(async move {
            for (index, name) in changed {
                if let Err(error) = api::tag::update_sort(
                    &ledger_id,
                    &name,
                    &category_transaction_type,
                    index as i32,
                )
                .await
                {
                    notify_error(ERR_SORT_TAG, &error);
                }
            }
        });
    };

    // ---- 初始化默认分类 ----
    //
    // 工具栏按钮与空态引导按钮共用同一份逻辑；闭包捕获的都是 `Copy` 信号，本身也可 `Copy`
    let initialize = move || {
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() || init_loading.get_untracked() {
            return;
        }
        init_loading.set(true);
        leptos::task::spawn_local(async move {
            let mut error_message: Option<String> = None;
            match api::category::initialize(&ledger_id).await {
                Ok(result) => {
                    Notifier::global().success(
                        format!(
                            "已添加 {} 个分类、{} 个标签",
                            result.categories, result.tags
                        ),
                        None,
                    );
                    has_any_categories.set(true);
                    reload_categories();
                }
                Err(error) => {
                    notify_error(ERR_INITIALIZE, &error);
                    error_message = Some(get_error_message(&error));
                }
            }
            if let Some(message) = error_message {
                // 失败时优先用错误消息，空则用兜底文案
                let text = if message.is_empty() {
                    TEXT_INIT_FAILED_FALLBACK.to_string()
                } else {
                    message
                };
                notify_error_text(text);
            }
            init_loading.set(false);
        });
    };

    // ================================================================ 渲染

    // ---- 工具栏：交易类型卡片 ----
    let type_nav_view = move || {
        let segments = TRANSACTION_TYPES.to_vec();
        view! {
            <nav class="ct-type-nav">
                {segments
                    .iter()
                    .map(|(value, label)| {
                        let value = *value;
                        let label = *label;
                        let target = value.to_string();
                        view! {
                            <button
                                type="button"
                                class=format!("ct-type-card ct-type-card--{value}")
                                class:is-active=move || active_type.get() == value
                                on:click=move |_| switch_type(target.clone())
                            >
                                <span class="ct-type-card-label">{label}</span>
                            </button>
                        }
                    })
                    .collect_view()}
            </nav>
        }
    };

    // ---- 左栏：分类 ----
    let category_column = move || {
        if stores.current_ledger_id.get().is_empty() {
            return empty_hint(TEXT_EMPTY_LEDGER);
        }

        let list = categories.get();
        if list.is_empty() {
            return if has_any_categories.get() {
                empty_hint(TEXT_EMPTY_CATEGORY)
            } else {
                init_panel(init_loading, initialize)
            };
        }

        let state = category_drag;
        let on_drop = move |(from, to): (usize, usize)| reorder_categories(from, to);

        view! {
            <div class="ct-column-body">
                {list
                    .into_iter()
                    .enumerate()
                    .map(|(index, category)| {
                        let name = category.name;
                        let record_count = category.record_count;

                        // 选中态 / 点击选中
                        let is_active_name = name.clone();
                        let select_name = name.clone();
                        // 删除按钮：title「删除」，点击弹出确认弹窗
                        let delete_click_name = name.clone();

                        view! {
                            <DragSortItem index=index state=state on_drop=on_drop class="ct-drag-item">
                                <div
                                    class="ct-list-item"
                                    class:is-active=move || {
                                        selected_category.get() == is_active_name
                                    }
                                    on:click=move |_| select_category(select_name.clone())
                                >
                                    <span class="ui-drag-handle" title="拖动排序">
                                        {icons::icon(Icon::DragHandle)}
                                    </span>
                                    <div class="ct-item-main">
                                        <span class="ct-item-name">{name}</span>
                                        <Show when=move || record_count != 0>
                                            <span class="ct-item-badge">{record_count}</span>
                                        </Show>
                                    </div>
                                    <div
                                        class="ct-item-actions"
                                        on:click=move |ev| ev.stop_propagation()
                                    >
                                        <IconButton
                                            variant=IconButtonVariant::Danger
                                            label="删除"
                                            on_click=move |_| {
                                                confirm_delete_category(delete_click_name.clone())
                                            }
                                        >
                                            {icons::icon(Icon::Trash)}
                                        </IconButton>
                                    </div>
                                </div>
                            </DragSortItem>
                        }
                    })
                    .collect_view()}
            </div>
        }
        .into_any()
    };

    // ---- 右栏：标签 ----
    let tag_column = move || {
        let category = selected_category.get();
        if category.is_empty() {
            return empty_hint(TEXT_EMPTY_TAG_GUIDE);
        }

        let list = tag_cache.get().get(&category).cloned().unwrap_or_default();
        if list.is_empty() {
            return empty_hint(TEXT_EMPTY_TAG);
        }

        let state = tag_drag;
        let on_drop = move |(from, to): (usize, usize)| reorder_tags(from, to);

        view! {
            <div class="ct-column-body">
                {list
                    .into_iter()
                    .enumerate()
                    .map(|(index, tag)| {
                        let name = tag.name;
                        let record_count = tag.record_count;
                        let delete_click_name = name.clone();

                        view! {
                            <DragSortItem index=index state=state on_drop=on_drop class="ct-drag-item">
                                <div class="ct-list-item">
                                    <span class="ui-drag-handle" title="拖动排序">
                                        {icons::icon(Icon::DragHandle)}
                                    </span>
                                    <div class="ct-item-main">
                                        <span class="ct-item-name">{name}</span>
                                        <Show when=move || record_count != 0>
                                            <span class="ct-item-badge">{record_count}</span>
                                        </Show>
                                    </div>
                                    <div class="ct-item-actions">
                                        <IconButton
                                            variant=IconButtonVariant::Danger
                                            label="删除"
                                            on_click=move |_| {
                                                confirm_delete_tag(delete_click_name.clone())
                                            }
                                        >
                                            {icons::icon(Icon::Trash)}
                                        </IconButton>
                                    </div>
                                </div>
                            </DragSortItem>
                        }
                    })
                    .collect_view()}
            </div>
        }
        .into_any()
    };

    let header_title = move || {
        let category = selected_category.get();
        if category.is_empty() {
            TEXT_COLUMN_TAG.to_string()
        } else {
            category
        }
    };

    view! {
        <FeaturePage
            title=super::accounting::PAGE_TITLE
            rail=view! { <super::accounting::SubFunctionRail sub=sub /> }.into_any()
            // 工具栏：只有交易类型卡片（当前账本由侧栏顶部的账本按钮显示，这里不再重复）
            toolbar=view! { {type_nav_view} }.into_any()
            content=view! {
                // 主体：240px 分类栏 + 1fr 标签栏
                <div class="ct-main">
                    <section class="ct-column ct-column--categories">
                        <div class="ct-column-header">
                            <span class="ct-column-title">{TEXT_COLUMN_CATEGORY}</span>
                            <span class="ct-column-count">{move || categories.get().len()}</span>
                            <Button
                                variant=ButtonVariant::Primary
                                size=ButtonSize::Small
                                class="ct-add-btn"
                                disabled=Signal::derive(move || {
                                    stores.current_ledger_id.get().is_empty()
                                })
                                on_click=move || open_add_category()
                            >
                                <span class="ct-add-btn-icon">{icons::icon(Icon::Plus)}</span>
                                <span>{TEXT_ADD_CATEGORY}</span>
                            </Button>
                        </div>
                        <div class="ct-column-body-wrap">{category_column}</div>
                    </section>

                    <section class="ct-column ct-column--tags">
                        <div class="ct-column-header">
                            <span class="ct-column-title">{header_title}</span>
                            <span class="ct-column-count">{move || {
                                tags_for(&tag_cache.get(), &selected_category.get()).len()
                            }}</span>
                            <Button
                                variant=ButtonVariant::Secondary
                                size=ButtonSize::Small
                                class="ct-add-btn"
                                disabled=Signal::derive(move || {
                                    selected_category.get().is_empty()
                                })
                                on_click=move || open_add_tag()
                            >
                                <span class="ct-add-btn-icon">{icons::icon(Icon::Plus)}</span>
                                <span>{TEXT_ADD_TAG}</span>
                            </Button>
                        </div>
                        <div class="ct-column-body-wrap">{tag_column}</div>
                    </section>
                </div>
            }.into_any()
        />

        // ---- 新增分类弹窗 ----
        <Modal
            open=Signal::derive(move || open_category_modal.get())
            title=TEXT_MODAL_ADD_CATEGORY
            width=360
            ok_text="确认"
            cancel_text="取消"
            on_close=move || open_category_modal.set(false)
            on_ok=move || confirm_add_category()
        >
            <div class="ct-modal-form">
                <Input
                    value=category_name
                    placeholder="输入分类名称"
                    maxlength=NAME_MAX_LENGTH
                    on_enter=move || confirm_add_category()
                />
            </div>
        </Modal>

        // ---- 新增标签弹窗 ----
        <Modal
            open=Signal::derive(move || open_tag_modal.get())
            title=TEXT_MODAL_ADD_TAG
            width=360
            ok_text="确认"
            cancel_text="取消"
            on_close=move || open_tag_modal.set(false)
            on_ok=move || confirm_add_tag()
        >
            <div class="ct-modal-form">
                <Input
                    value=tag_name
                    placeholder="输入标签名称"
                    maxlength=NAME_MAX_LENGTH
                    on_enter=move || confirm_add_tag()
                />
            </div>
        </Modal>

        // ---- 删除确认弹窗（标题随分类/标签切换，正文是待删除项的消息） ----
        <Modal
            open=Signal::derive(move || open_delete_modal.get())
            title=delete_modal_title(delete_kind.get())
            width=360
            ok_text="删除"
            ok_danger=true
            cancel_text="取消"
            on_close=move || open_delete_modal.set(false)
            on_ok=move || execute_delete()
        >
            <p class="ct-delete-message">{move || delete_message.get()}</p>
        </Modal>
    }
}

// ------------------------------------------------------------------ 辅助

/// 需要落库的一项：`(新下标, 名字)`。
type SortChange = (usize, String);

/// 重排列表并算出"需要落库的项"。
///
/// 做法：先把被拖动的项从 `from` 取出、插到 `to`，随后**全量重排**
/// `sortOrder`，但只把 `sortOrder` 与新下标不一致的项放进结果（调用方据此发请求）。
///
/// 返回 `(新顺序, 需要落库的项)`；下标越界或原位返回 `None`。
fn reorder_with_changes<T, F>(
    list: &[T],
    from: usize,
    to: usize,
    key: F,
) -> Option<(Vec<T>, Vec<SortChange>)>
where
    T: Clone + SortOrder,
    F: Fn(&T) -> String,
{
    if from == to || from >= list.len() || to >= list.len() {
        return None;
    }

    let mut reordered = list.to_vec();
    let moved = reordered.remove(from);
    reordered.insert(to, moved);

    let mut changed: Vec<SortChange> = Vec::new();
    for (index, item) in reordered.iter().enumerate() {
        if item.sort_order() != index as i32 {
            changed.push((index, key(item)));
        }
    }
    Some((reordered, changed))
}

/// 列表项携带排序号。
trait SortOrder {
    fn sort_order(&self) -> i32;
}

impl SortOrder for CategoryDto {
    fn sort_order(&self) -> i32 {
        self.sort_order
    }
}

impl SortOrder for TagDto {
    fn sort_order(&self) -> i32 {
        self.sort_order
    }
}

/// 删除弹窗标题（分类 / 标签两个固定分支）。
fn delete_modal_title(kind: CtrDeleteKind) -> String {
    match kind {
        CtrDeleteKind::Category => TEXT_MODAL_DELETE_CATEGORY.to_string(),
        CtrDeleteKind::Tag => TEXT_MODAL_DELETE_TAG.to_string(),
    }
}

/// 取某个分类缓存的标签（未缓存即空数组）。
fn tags_for(
    cache: &std::collections::BTreeMap<String, Vec<TagDto>>,
    category: &str,
) -> Vec<TagDto> {
    if category.is_empty() {
        return Vec::new();
    }
    cache.get(category).cloned().unwrap_or_default()
}

/// 纯文字空态（三处空态都是单行文案）。
fn empty_hint(text: &'static str) -> AnyView {
    view! { <div class="ct-column-empty"><span>{text}</span></div> }.into_any()
}

/// 初始化引导面板。
fn init_panel(init_loading: RwSignal<bool>, initialize: impl Fn() + Copy + 'static) -> AnyView {
    view! {
        <div class="ct-column-empty">
            <div class="ct-empty-init">
                <div class="ct-empty-init-icon">{icons::icon(Icon::Inbox)}</div>
                <span class="ct-empty-init-text">{TEXT_EMPTY_INIT}</span>
                <Button
                    variant=ButtonVariant::Primary
                    loading=Signal::derive(move || init_loading.get())
                    on_click=initialize
                >
                    {move || {
                        if init_loading.get() {
                            TEXT_INIT_LOADING
                        } else {
                            TEXT_INIT_BUTTON
                        }
                    }}
                </Button>
            </div>
        </div>
    }
    .into_any()
}

/// 只有一句话、无描述的错误提示（走底部 message 通道）。
fn notify_error_text(text: impl Into<String>) {
    Notifier::global().error(text, None);
}
