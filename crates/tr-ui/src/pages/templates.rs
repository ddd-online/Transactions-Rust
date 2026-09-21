//! 记账 · **模板**子功能（消费模板：列表 + 新建 + 删除 + 拖拽排序）。
//!
//! 它原来是「应用设置 → 消费模板」的一个分栏，随"消费记录 / 分类标签 / 消费模板
//! 合并成记账"迁到这里（见 [`crate::pages::accounting`]）；只提供版心里的
//! 「工具栏 + 内容区」，标题栏与左侧子功能图标条由 `FeaturePage` 统一渲染。
//!
//! 表格样式沿用 `st-*` 类（`static/css/settings.css` 里那一组：
//! 它们现在**只被本子功能使用**，是迁移时保持不动的命名，日后可统一改成 `tpl-*`）。
//!
//! 行为要点：
//! * 数据随**当前账本**变化重新拉取（`template_list(ledgerId)`），失败回落空数组并提示；
//! * 新建前先在前端挡下后端 `validate()` 的同名错误（文案与后端一致）；
//! * 拖拽排序只对 `sort_order` 确实变化的项发请求，逐条互不中断。

use leptos::prelude::*;
use tr_domain::dto::TransactionTemplateDto;

use crate::api;
use crate::components::ui::{
    Button, ButtonSize, ButtonVariant, Checkbox, CheckboxGroup, CheckboxOption, DragSortItem,
    DragSortState, Empty, FeaturePage, Form, FormItem, FormLayout, Input, Modal, ModalSize,
    Popconfirm, Segmented, SegmentedOption, Select, SelectOption, Spin, SpinSize, Tag, TagKind,
};
use crate::error_handler::notify_error;
use crate::format;
use crate::icons::{self, Icon};
use crate::notify::Notifier;
use crate::store::AppStores;

use super::accounting::{SubFunction, SubFunctionRail};

/// 记账页的「模板」子功能。
#[component]
pub fn TemplateSub(sub: RwSignal<SubFunction>) -> impl IntoView {
    let stores = AppStores::global();

    let templates = RwSignal::new(Vec::<TransactionTemplateDto>::new());
    let loading = RwSignal::new(false);
    let drag = DragSortState::new();

    // 新建模板弹窗
    let create_open = RwSignal::new(false);
    let creating = RwSignal::new(false);
    let form_name = RwSignal::new(String::new());
    let form_type = RwSignal::new("expense".to_string());
    let form_category = RwSignal::new(String::new());
    let form_tags = RwSignal::new(Vec::<String>::new());
    let form_description = RwSignal::new(String::new());
    let form_outlier = RwSignal::new(false);
    let categories = RwSignal::new(Vec::<String>::new());
    let tag_names = RwSignal::new(Vec::<String>::new());

    let reset_form = move || {
        form_name.set(String::new());
        form_type.set("expense".to_string());
        form_category.set(String::new());
        form_tags.set(Vec::new());
        form_description.set(String::new());
        form_outlier.set(false);
    };

    // ---- 加载：`template_list(ledgerId)`（错误前缀「查询模板失败」，回落空数组） ----
    let load = move |ledger_id: String| {
        if ledger_id.is_empty() {
            templates.set(Vec::new());
            loading.set(false);
            return;
        }
        loading.set(true);
        leptos::task::spawn_local(async move {
            match api::template::list(&ledger_id).await {
                Ok(list) => templates.set(list),
                Err(error) => {
                    templates.set(Vec::new());
                    notify_error("查询模板失败", &error);
                }
            }
            loading.set(false);
        });
    };

    // 账本变化 → 重新加载
    Effect::new(move |_: Option<()>| load(stores.current_ledger_id.get()));

    // ---- 拖拽排序：只对 `sort_order` 变化的项发请求，且逐条互不中断 ----
    let reorder = move |from: usize, to: usize| {
        let mut list = templates.get_untracked();
        if from >= list.len() || to >= list.len() || from == to {
            return;
        }
        let previous: Vec<(String, i32)> = list
            .iter()
            .map(|item| (item.template_id.clone(), item.sort_order))
            .collect();

        let moved = list.remove(from);
        list.insert(to, moved);
        for (index, item) in list.iter_mut().enumerate() {
            item.sort_order = index as i32;
        }

        let ledger_id = stores.current_ledger_id.get_untracked();
        let pending: Vec<(String, i32)> = list
            .iter()
            .enumerate()
            .filter(|(index, item)| {
                previous
                    .iter()
                    .any(|(id, order)| id == &item.template_id && *order != *index as i32)
            })
            .map(|(index, item)| (item.template_id.clone(), index as i32))
            .collect();

        templates.set(list);

        if ledger_id.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            for (id, sort_order) in pending {
                if let Err(error) = api::template::update_sort(&id, &ledger_id, sort_order).await {
                    notify_error("更新模板排序失败", &error);
                }
            }
        });
    };

    // ---- 删除 ----
    let delete_template = move |template_id: String| {
        leptos::task::spawn_local(async move {
            match api::template::delete(&template_id).await {
                Ok(()) => {
                    Notifier::global().success("删除模板成功", None);
                    load(stores.current_ledger_id.get_untracked());
                }
                Err(error) => notify_error("删除模板失败", &error),
            }
        });
    };

    // ---- 新建模板（工具栏的「新建模板」按钮：重置表单 + 开弹窗） ----
    let open_create = move || {
        reset_form();
        create_open.set(true);
    };

    // 分类：随「弹窗打开 / 交易类型 / 账本」变化重新拉取
    Effect::new(move |_: Option<()>| {
        let open = create_open.get();
        let transaction_type = form_type.get();
        let ledger_id = stores.current_ledger_id.get();
        if !open {
            return;
        }
        form_category.set(String::new());
        form_tags.set(Vec::new());
        if ledger_id.is_empty() {
            categories.set(Vec::new());
            return;
        }
        leptos::task::spawn_local(async move {
            match api::category::list(&transaction_type, &ledger_id).await {
                Ok(list) => categories.set(list.into_iter().map(|item| item.name).collect()),
                Err(error) => {
                    categories.set(Vec::new());
                    notify_error("查询分类失败", &error);
                }
            }
        });
    });

    // 标签：`tag_list("{分类}:{类型}", ledgerId)`
    Effect::new(move |_: Option<()>| {
        let open = create_open.get();
        let category = form_category.get();
        let transaction_type = form_type.get();
        let ledger_id = stores.current_ledger_id.get();
        form_tags.set(Vec::new());
        if !open || category.is_empty() || ledger_id.is_empty() {
            tag_names.set(Vec::new());
            return;
        }
        let key = format!("{category}:{transaction_type}");
        leptos::task::spawn_local(async move {
            match api::tag::list(&key, &ledger_id).await {
                Ok(list) => tag_names.set(list.into_iter().map(|item| item.name).collect()),
                Err(error) => {
                    tag_names.set(Vec::new());
                    notify_error("查询标签失败", &error);
                }
            }
        });
    });

    let submit_create = move || {
        if creating.get_untracked() {
            return;
        }
        let ledger_id = stores.current_ledger_id.get_untracked();
        if ledger_id.is_empty() {
            Notifier::global().error("请先选择工作空间", None);
            return;
        }
        // 前端先挡一道后端 `validate()` 的三条错误（文案与后端一致，中文）
        let name = form_name.get_untracked().trim().to_string();
        if name.is_empty() {
            Notifier::global().error("模板名称不能为空", None);
            return;
        }
        let category = form_category.get_untracked();
        if category.is_empty() {
            Notifier::global().error("分类不能为空", None);
            return;
        }

        let dto = TransactionTemplateDto {
            template_id: String::new(),
            ledger_id: ledger_id.clone(),
            template_name: name,
            transaction_type: form_type.get_untracked(),
            category,
            tags: form_tags.get_untracked(),
            // 勾选离群值时写入标记名（逗号分隔的标记串）
            flags: if form_outlier.get_untracked() {
                "outlier".to_string()
            } else {
                String::new()
            },
            description: form_description.get_untracked(),
            sort_order: 0,
        };

        creating.set(true);
        leptos::task::spawn_local(async move {
            match api::template::create(dto).await {
                Ok(_id) => {
                    Notifier::global().success("保存模板成功", None);
                    create_open.set(false);
                    reset_form();
                    load(ledger_id);
                }
                Err(error) => notify_error("保存模板失败", &error),
            }
            creating.set(false);
        });
    };

    let toolbar = view! {
        <div class="toolbar-end">
            <Button variant=ButtonVariant::Primary on_click=move |_| open_create()>
                <span class="ui-btn__icon">{icons::icon(Icon::Plus)}</span>
                "新建模板"
            </Button>
        </div>
    }
    .into_any();

    let content = view! {
        <div class="page-pane tpl-pane">
            <div class="tpl-table">
                <div class="tpl-thead">
                    <div class="tpl-th tpl-th--drag"></div>
                    <div class="tpl-th">"模板名称"</div>
                    <div class="tpl-th tpl-th--center">"交易类型"</div>
                    <div class="tpl-th">"分类"</div>
                    <div class="tpl-th">"标签"</div>
                    <div class="tpl-th">"标记"</div>
                    <div class="tpl-th">"描述"</div>
                    <div class="tpl-th tpl-th--center">"操作"</div>
                </div>

                <div class="tpl-tbody">
                    {move || {
                        let list = templates.get();
                        if list.is_empty() {
                            if loading.get() {
                                view! {
                                    <div class="tpl-loading">
                                        <Spin spinning=true size=SpinSize::Small />
                                        <span>"正在加载…"</span>
                                    </div>
                                }
                                    .into_any()
                            } else {
                                view! { <Empty title="暂无模板" /> }.into_any()
                            }
                        } else {
                            list
                                .into_iter()
                                .enumerate()
                                .map(|(index, template)| {
                                    template_row(template, index, drag, reorder, delete_template)
                                })
                                .collect_view()
                                .into_any()
                        }
                    }}
                </div>
            </div>

            <Modal
                open=create_open
                title="新建模板"
                size=ModalSize::Medium
                ok_text="保存"
                cancel_text="取消"
                ok_loading=creating
                on_close=move || create_open.set(false)
                on_ok=move || submit_create()
            >
                <Form layout=FormLayout::Vertical>
                    <FormItem label="模板名称">
                        <Input value=form_name placeholder="请输入模板名称" maxlength=20 />
                    </FormItem>
                    <FormItem label="交易类型">
                        <Segmented
                            value=form_type
                            options=vec![
                                SegmentedOption::new("expense", "支出"),
                                SegmentedOption::new("income", "收入"),
                                SegmentedOption::new("transfer", "转账"),
                            ]
                        />
                    </FormItem>
                    <FormItem label="分类">
                        {move || {
                            let options = categories
                                .get()
                                .into_iter()
                                .map(SelectOption::same)
                                .collect::<Vec<SelectOption>>();
                            view! {
                                <Select
                                    value=form_category
                                    options=options
                                    placeholder="选择消费分类"
                                    searchable=true
                                />
                            }
                        }}
                    </FormItem>
                    <FormItem label="标签">
                        {move || {
                            let options = tag_names
                                .get()
                                .into_iter()
                                .map(CheckboxOption::same)
                                .collect::<Vec<CheckboxOption>>();
                            if options.is_empty() {
                                view! { <span class="page-hint">"该分类下暂无标签"</span> }.into_any()
                            } else {
                                view! { <CheckboxGroup values=form_tags options=options /> }.into_any()
                            }
                        }}
                    </FormItem>
                    <FormItem label="描述">
                        <Input value=form_description placeholder="请输入描述" maxlength=50 />
                    </FormItem>
                    <FormItem label="标记">
                        <Checkbox checked=form_outlier label="离群值" />
                    </FormItem>
                </Form>
            </Modal>
        </div>
    }
    .into_any();

    view! {
        <FeaturePage
            title=super::accounting::PAGE_TITLE
            rail=view! { <SubFunctionRail sub=sub /> }.into_any()
            toolbar=toolbar
            content=content
        />
    }
}

/// 一行模板：`DragSortItem` 作整行容器（拖拽手柄是视觉元素）。
fn template_row(
    template: TransactionTemplateDto,
    index: usize,
    drag: DragSortState,
    on_drop: impl Fn(usize, usize) + Copy + 'static,
    on_delete: impl Fn(String) + Copy + 'static,
) -> impl IntoView {
    let template_id = template.template_id.clone();
    let name = template.template_name.clone();
    let name_title = name.clone();
    let type_text = format::transaction_type_text(&template.transaction_type);
    let tag_kind = TagKind::from_transaction_type(&template.transaction_type);
    let category = if template.category.is_empty() {
        "-".to_string()
    } else {
        template.category.clone()
    };
    let tags = template.tags.clone();
    let tags_empty = tags.is_empty();
    let has_flags = !template.flags.is_empty();
    let description_raw = template.description.clone();
    let description = if description_raw.is_empty() {
        "-".to_string()
    } else {
        description_raw.clone()
    };
    let delete_title = format!("删除模板「{}」？", template.template_name);
    let id_for_delete = template_id.clone();

    let drop_handler = UnsyncCallback::new(move |(from, to): (usize, usize)| on_drop(from, to));
    let delete_handler = UnsyncCallback::new(move |id: String| on_delete(id));

    view! {
        <DragSortItem index=index state=drag on_drop=drop_handler class="tpl-tr">
            <div class="tpl-td tpl-td--drag">
                <span class="ui-drag-handle" title="拖动排序">
                    {icons::icon(Icon::DragHandle)}
                </span>
            </div>

            <div class="tpl-td">
                <span class="tpl-cell-ellipsis" title=name_title>
                    {name}
                </span>
            </div>

            <div class="tpl-td tpl-td--center">
                <Tag kind=tag_kind>{type_text}</Tag>
            </div>

            <div class="tpl-td">{category}</div>

            <div class="tpl-td tpl-td--tags">
                {if tags_empty {
                    view! { <span class="tpl-dash">"-"</span> }.into_any()
                } else {
                    tags.into_iter()
                        .map(|tag| view! { <Tag>{tag}</Tag> })
                        .collect_view()
                        .into_any()
                }}
            </div>

            <div class="tpl-td">
                {if has_flags {
                    view! { <Tag kind=TagKind::Outlier>"离群值"</Tag> }.into_any()
                } else {
                    view! { <span class="tpl-dash">"-"</span> }.into_any()
                }}
            </div>

            <div class="tpl-td">
                <span class="tpl-cell-ellipsis" title=description_raw>
                    {description}
                </span>
            </div>

            <div class="tpl-td tpl-td--center">
                <Popconfirm
                    title=delete_title
                    ok_text="删除"
                    cancel_text="取消"
                    class="ui-popconfirm--end"
                    on_confirm=move || delete_handler.run(id_for_delete.clone())
                >
                    <Button
                        variant=ButtonVariant::TextDanger
                        size=ButtonSize::Small
                        icon_only=true
                        title="删除"
                    >
                        {icons::icon(Icon::Trash)}
                    </Button>
                </Popconfirm>
            </div>
        </DragSortItem>
    }
}
