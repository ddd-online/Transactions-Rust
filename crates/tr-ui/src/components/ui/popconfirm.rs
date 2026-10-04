//! 气泡确认框。
//!
//! 行为：点击触发元素弹出小气泡（标题 + 可选描述 + 取消/确认），点面板外的透明遮罩关闭。
//! 浮层用 `--transactions-shadow-lg`，与 `ui-select` 的下拉面板同一套手法。
//!
//! ## 定位：`position: fixed` + 实测坐标，不是绝对定位
//!
//! 面板原来是触发器的绝对定位子元素 —— 只要中间有一层**滚动容器**（分析页的图表列表、
//! 事件页的关联交易栏…），气泡就会被那层的 `overflow` 裁掉：图表列表里点删除，
//! 气泡只画出小半截、确认键根本看不见（用户报的就是这个；UIA 照样能找到并点到它，
//! 所以这类"被裁掉"只有截图才看得见，护栏抓不住）。
//!
//! 改成 `position: fixed` 之后它不再受任何祖先裁剪，代价是坐标自己算：
//! 点开那一刻读触发器的 `getBoundingClientRect()`（本来就是**视口坐标**，与 fixed 同域），
//! 等面板渲染出来再量自己的高度，然后决定放下面还是翻到上面（下面装不下就翻上去），
//! 水平方向把面板夹进视口、留 8px。
//!
//! 为什么不上 portal：`fixed` 在这里已经把问题解掉了，面板仍留在原地的事件树里，
//! 组件、冒泡、无障碍关系都不变；portal 要另起挂载点，换来的还是同一件事。
//!
//! ```
//! <Popconfirm title="确定删除这条记录吗？" on_confirm=move || do_delete()>
//!     <Button variant=ButtonVariant::TextDanger>"删除"</Button>
//! </Popconfirm>
//! ```

use super::backdrop;
use super::with_class;
use leptos::prelude::*;

use crate::icons::{self, Icon};

/// 气泡与触发器之间的间距（px）。
const GAP: f64 = 4.0;

/// 气泡距视口边缘的最小留白（px）。
const EDGE: f64 = 8.0;

/// 视口尺寸（CSS px，与 `getBoundingClientRect` / `fixed` 同一坐标系）。
///
/// 取不到（理论上不会）时给一个保守值：气泡仍会落在左上角附近，而不是跑到屏幕外。
fn window_size() -> (f64, f64) {
    let Some(window) = web_sys::window() else {
        return (1024.0, 768.0);
    };
    let width = window
        .inner_width()
        .ok()
        .and_then(|value| value.as_f64())
        .unwrap_or(1024.0);
    let height = window
        .inner_height()
        .ok()
        .and_then(|value| value.as_f64())
        .unwrap_or(768.0);
    (width, height)
}

#[component]
pub fn Popconfirm(
    /// 标题（必填）
    #[prop(into)]
    title: String,
    /// 补充说明
    #[prop(optional, into)]
    description: Option<String>,
    /// 确认按钮文案，默认「确认」；删除类请传「删除」
    #[prop(optional, into)]
    ok_text: Option<String>,
    /// 取消按钮文案，默认「取消」
    #[prop(optional, into)]
    cancel_text: Option<String>,
    /// 是否显示取消按钮（关掉后只留一个确认按钮）
    #[prop(default = true)]
    show_cancel: bool,
    /// 确认回调（气泡会自动关闭）
    #[prop(optional, into)]
    on_confirm: Option<UnsyncCallback<()>>,
    /// 触发元素里的定位修正类名（例如右对齐的 `ui-popconfirm--end`）
    #[prop(optional, into)]
    class: Option<String>,
    children: Children,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let ok_text = ok_text.unwrap_or_else(|| "确认".to_string());
    let cancel_text = cancel_text.unwrap_or_else(|| "取消".to_string());

    let classes = with_class("ui-popconfirm", class.as_deref());
    // `--end`（贴右）由调用方按类名给：面板的**边**与触发器的哪条边对齐。
    // 定位现在由组件算，但这个意图仍然要认 —— 触发器贴着表格右缘时，
    // 面板的右边缘该跟它齐平，而不是从它的左边缘往右铺出去。
    let align_end = classes.contains("ui-popconfirm--end");

    // 触发器的视口矩形（x, y, w, h）与面板的落点（left, top）。都是视口坐标。
    let anchor = RwSignal::new((0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64));
    let placed = RwSignal::new((0.0_f64, 0.0_f64));
    let panel = NodeRef::<leptos::html::Div>::new();

    // 面板渲染出来之后量一次自己的尺寸，再定最终落点。
    // 放在下面装不下就翻到上面；两边都装不下时以上面为准（夹进视口），
    // 免得气泡出现在半个屏幕之外。
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        let Some(element) = panel.get() else {
            return;
        };
        let size = element.get_bounding_client_rect();
        let (ax, ay, aw, ah) = anchor.get_untracked();
        let (viewport_w, viewport_h) = window_size();

        let below = ay + ah + GAP;
        let above = ay - GAP - size.height();
        let top = if below + size.height() <= viewport_h - EDGE {
            below
        } else {
            above.max(EDGE)
        };

        let desired_left = if align_end {
            ax + aw - size.width()
        } else {
            ax
        };
        let left = desired_left.clamp(EDGE, (viewport_w - size.width() - EDGE).max(EDGE));
        placed.set((left, top));
    });

    view! {
        <div class=classes class:is-open=move || open.get()>
            // 触发挂**捕获阶段**：调用方常在子元素上写 `stop_propagation()`（例如列表项里的删除按钮，
            // 为了不触发整行的"选中"）。如果这里挂在冒泡阶段，那个 stop_propagation 会把点击吃掉，
            // 气泡永远弹不出来 —— 删除图表/删除事件/删除关联交易三处都因此失效过（点删除毫无反应）。
            <span
                class="ui-popconfirm__trigger"
                on:click:capture=move |ev: leptos::ev::MouseEvent| {
                    // 先量触发器再翻开关：面板要用这份坐标定位，而它下一秒就渲染出来了。
                    if let Some(element) = ev
                        .current_target()
                        .and_then(|target| {
                            use wasm_bindgen::JsCast;
                            target.dyn_into::<web_sys::Element>().ok()
                        })
                    {
                        let rect = element.get_bounding_client_rect();
                        anchor.set((
                            rect.left(),
                            rect.top(),
                            rect.width(),
                            rect.height(),
                        ));
                    }
                    open.update(|v| *v = !*v);
                }
            >
                {children()}
            </span>

            <Show when=move || open.get()>
                {backdrop(UnsyncCallback::new(move |()| open.set(false)))}
                <div
                    class="ui-popconfirm__panel"
                    role="dialog"
                    node_ref=panel
                    style=move || {
                        let (left, top) = placed.get();
                        format!("left: {left}px; top: {top}px;")
                    }
                >
                    <div class="ui-popconfirm__header">
                        {icons::icon(Icon::WarningCircle)}
                        <span class="ui-popconfirm__title">{title.clone()}</span>
                    </div>
                    {description
                        .clone()
                        .map(|text| view! { <p class="ui-popconfirm__description">{text}</p> })}
                    <div class="ui-popconfirm__actions">
                        // 用 `is-hidden` 而不是嵌套 `Show`：`Show` 的 children 必须是 `Fn`，
                        // 而 `cancel_text` 一旦被闭包 move 进去就会让整段退化成 `FnOnce`。
                        <button
                            type="button"
                            class="ui-btn ui-btn--secondary"
                            class:is-hidden=move || !show_cancel
                            on:click=move |_| open.set(false)
                        >
                            {cancel_text.clone()}
                        </button>
                        <button
                            type="button"
                            class="ui-btn ui-btn--primary"
                            on:click=move |_| {
                                open.set(false);
                                if let Some(callback) = on_confirm {
                                    callback.run(());
                                }
                            }
                        >
                            {ok_text.clone()}
                        </button>
                    </div>
                </div>
            </Show>
        </div>
    }
}
