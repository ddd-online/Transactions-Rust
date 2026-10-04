//! 方形裁剪弹窗 —— 把一张图裁成正方的位图。
//!
//! 只有一个使用场景（工作空间图标），但裁剪本身是一段**纯几何**：
//! 拖拽改位移、按钮改缩放，最后按"视口里看到什么"反解出源图上的那块矩形。
//! 几何部分（[`cover_scale`] / [`max_offset`] / [`crop_rect`]）住在 `tr-draw::crop`
//! （native + wasm32 双可编，`cargo test -p tr-draw` 真跑），
//! 这里只负责把指针事件喂进去、把算出来的 [`CropRect`] 交给 canvas。
//!
//! ## 为什么不引第三方裁剪库
//!
//! 与全仓一致：仓库内无 Node 依赖、界面层不引 JS 库，全部能力都用 web-sys 现成的
//! `<img>` + `<canvas>`（与 `image_picker` 的 HEIC 转码同一条路）。
//!
//! ## 不变量
//!
//! **图像任何时候都必须盖满视口**（`cover`）：位移被 [`max_offset`] 夹住，
//! 缩放下限是 1.0，所以裁出来的永远是图里的真实像素，不会出现透明/白边。

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::components::ui::image_picker::{canvas_context, create_canvas, load_image};
use crate::components::ui::{IconButton, Modal, ModalSize};
use crate::icons::{self, Icon};
use tr_draw::crop::{cover_scale, crop_rect, max_offset, CropRect, CROP_VIEWPORT};

/// 输出位图的边长（px）。显示尺寸是 32px，256 足够 HiDPI 下的 8 倍缩放。
const CROP_OUTPUT: u32 = 256;
/// 缩放上限（相对"刚好铺满"的 1.0）。再大就只剩几个像素被拉成一张图了。
const CROP_MAX_ZOOM: f64 = 4.0;
/// 每按一次缩放按钮的步进倍率。
const CROP_ZOOM_STEP: f64 = 1.25;

/// 方形裁剪弹窗。
///
/// `open` 控制显隐，`source` 是待裁剪的 data URL；确认时回调裁剪后的 **PNG data URL**
/// （带透明度，标志类图片常见）。几何计算与编码都在这里，调用方只管存取。
#[component]
pub fn ImageCropDialog(
    /// 是否打开
    #[prop(into)]
    open: Signal<bool>,
    /// 源图（data URL）。空串 = 还没有图，弹窗里什么都不画。
    #[prop(into)]
    source: Signal<String>,
    /// 确认：回调参数是裁剪后的 PNG data URL
    #[prop(into)]
    on_confirm: UnsyncCallback<String>,
    /// 关闭（遮罩 / × / 取消共用）
    #[prop(into)]
    on_close: UnsyncCallback<()>,
    /// 确认按钮加载态
    #[prop(optional, into)]
    ok_loading: Option<Signal<bool>>,
) -> impl IntoView {
    // 解码一次、留着复用：拖动与缩放只改两个数，确认时直接 drawImage。
    // `HtmlImageElement` 不是 Send + Sync（JS 句柄），所以走 `LocalStorage` 那一档
    // —— 与事件页的上传控制块同一个写法。
    let image: RwSignal<Option<web_sys::HtmlImageElement>, LocalStorage> =
        RwSignal::<Option<web_sys::HtmlImageElement>, LocalStorage>::new_local(None);
    // 源图自然尺寸（px）；`(0, 0)` = 还没解码完。
    let natural = RwSignal::new((0.0_f64, 0.0_f64));
    // 用户缩放（1.0 = 刚好铺满）
    let zoom = RwSignal::new(1.0_f64);
    // 图像中心相对视口中心的位移（CSS px）
    let offset = RwSignal::new((0.0_f64, 0.0_f64));
    // 拖拽的上一个指针位置；`None` = 没在拖
    let dragging = RwSignal::new(None::<(f64, f64)>);

    // 换图（或首次打开）时重置：重新解码 → 记自然尺寸 → 回到"铺满且居中"。
    // 不重置的话，上一张图的位移会带过来，在新图上停在某个想不到的角落。
    Effect::new(move |_| {
        let data = source.get();
        zoom.set(1.0);
        offset.set((0.0, 0.0));
        natural.set((0.0, 0.0));
        image.set(None);
        if data.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            match load_image(&data).await {
                Ok(element) => {
                    natural.set((
                        f64::from(element.natural_width()),
                        f64::from(element.natural_height()),
                    ));
                    image.set(Some(element));
                }
                // 解码不了就让弹窗停在没有图的状态：`ok` 会被 `image.is_some()` 拦住，
                // 用户只能取消 —— 比"确认后保存出一张空图标"好
                Err(error) => leptos::logging::error!("图标预览解码失败: {error}"),
            }
        });
    });

    // 当前比例（源图像素 → CSS px）。
    let scale_now = move || cover_scale(natural.get_untracked()) * zoom.get_untracked();

    // 把一个位移夹回合法范围（缩放变化后必须重夹，否则放大前的位置会越界）。
    let clamp = move |value: (f64, f64), scale: f64| {
        let (limit_x, limit_y) = max_offset(natural.get_untracked(), scale);
        (
            value.0.clamp(-limit_x, limit_x),
            value.1.clamp(-limit_y, limit_y),
        )
    };

    let zoom_by = move |factor: f64| {
        let next = (zoom.get_untracked() * factor).clamp(1.0, CROP_MAX_ZOOM);
        if (next - zoom.get_untracked()).abs() < f64::EPSILON {
            return;
        }
        zoom.set(next);
        let scale = cover_scale(natural.get_untracked()) * next;
        offset.update(|value| *value = clamp(*value, scale));
    };

    // 预览图的位置与大小：与 `crop_rect` 用的是同一组算式
    // （图左上角 = (视口 - 显示边长)/2 + 位移），只是这里要的是 CSS。
    // 还没解码完时交给 CSS 的 `object-fit: cover` 先顶上 —— 它与 `cover_scale` 是同一个语义，
    // 于是解码完成的瞬间画面**不跳**。
    let image_style = move || {
        let size = natural.get();
        if size.0 <= 0.0 || size.1 <= 0.0 {
            return "inset: 0; width: 100%; height: 100%; object-fit: cover;".to_string();
        }
        let scale = cover_scale(size) * zoom.get();
        let (width, height) = (size.0 * scale, size.1 * scale);
        let (dx, dy) = offset.get();
        format!(
            "left: {}px; top: {}px; width: {width}px; height: {height}px;",
            (CROP_VIEWPORT - width) / 2.0 + dx,
            (CROP_VIEWPORT - height) / 2.0 + dy,
        )
    };

    let confirm = move || {
        let Some(element) = image.get_untracked() else {
            return;
        };
        let size = natural.get_untracked();
        if size.0 <= 0.0 || size.1 <= 0.0 {
            return;
        }
        match crop_to_png(
            &element,
            crop_rect(size, offset.get_untracked(), scale_now()),
        ) {
            Ok(data_url) => on_confirm.run(data_url),
            Err(error) => leptos::logging::error!("图标裁剪失败: {error}"),
        }
    };

    let ready = move || image.get().is_some();

    view! {
        <Modal
            open=open
            title="方形裁剪"
            size=ModalSize::Small
            ok_text="应用"
            ok_loading=ok_loading.unwrap_or_else(|| Signal::derive(|| false))
            on_ok=move |_| confirm()
            on_close=on_close
        >
            <div class="icon-crop">
                <div
                    class="icon-crop__viewport"
                    class:is-dragging=move || dragging.get().is_some()
                    on:pointerdown=move |ev: leptos::ev::PointerEvent| {
                        // 拖拽期间不要触发文本选择 / 图片原生的"拖出去"行为
                        ev.prevent_default();
                        // 抓住指针：拖出视口再拖回来时事件仍然回到这里（否则一动就断了）
                        if let Some(element) = ev
                            .current_target()
                            .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
                        {
                            let _ = element.set_pointer_capture(ev.pointer_id());
                        }
                        dragging.set(Some((f64::from(ev.client_x()), f64::from(ev.client_y()))));
                    }
                    on:pointermove=move |ev: leptos::ev::PointerEvent| {
                        let Some((last_x, last_y)) = dragging.get_untracked() else {
                            return;
                        };
                        let (x, y) = (f64::from(ev.client_x()), f64::from(ev.client_y()));
                        dragging.set(Some((x, y)));
                        // 用"相对上一次的位置"而不是"相对按下点"：夹紧之后指针与图像的相对位置
                        // 会脱节，累积增量能让手感自然回来（拖到边界再往回拖立刻响应）。
                        let scale = scale_now();
                        offset.update(|value| {
                            let moved = (value.0 + x - last_x, value.1 + y - last_y);
                            *value = clamp(moved, scale);
                        });
                    }
                    on:pointerup=move |_| dragging.set(None)
                    on:pointercancel=move |_| dragging.set(None)
                    // 滚轮缩放：鼠标停在图上就能放大缩小，不必非去点按钮
                    on:wheel=move |ev: leptos::ev::WheelEvent| {
                        ev.prevent_default();
                        zoom_by(if ev.delta_y() < 0.0 {
                            CROP_ZOOM_STEP
                        } else {
                            1.0 / CROP_ZOOM_STEP
                        });
                    }
                >
                    <img
                        class="icon-crop__image"
                        alt=""
                        src=move || source.get()
                        style=image_style
                        draggable="false"
                    />
                    // 三分线：给"居中 / 对齐"一个参照，纯装饰（不吃指针事件）
                    <span class="icon-crop__grid"></span>
                </div>

                <div class="icon-crop__controls">
                    <IconButton
                        label="缩小"
                        disabled=Signal::derive(move || zoom.get() <= 1.0)
                        on_click=move |_| zoom_by(1.0 / CROP_ZOOM_STEP)
                    >
                        {icons::icon(Icon::ZoomOut)}
                    </IconButton>
                    <IconButton
                        label="放大"
                        disabled=Signal::derive(move || zoom.get() >= CROP_MAX_ZOOM)
                        on_click=move |_| zoom_by(CROP_ZOOM_STEP)
                    >
                        {icons::icon(Icon::ZoomIn)}
                    </IconButton>
                    <span class="icon-crop__hint">
                        {move || {
                            if ready() {
                                "拖动图片调整位置，滚轮或按钮缩放"
                            } else {
                                "正在读取图片…"
                            }
                        }}
                    </span>
                </div>
            </div>
        </Modal>
    }
}

/// 按裁剪框把源图画进一张 `CROP_OUTPUT` 见方的画布，编码成 PNG data URL。
fn crop_to_png(image: &web_sys::HtmlImageElement, rect: CropRect) -> Result<String, String> {
    let canvas = create_canvas(CROP_OUTPUT, CROP_OUTPUT)?;
    let context = canvas_context(&canvas)?;
    let side = f64::from(CROP_OUTPUT);
    context
        .draw_image_with_html_image_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
            image, rect.x, rect.y, rect.size, rect.size, 0.0, 0.0, side, side,
        )
        .map_err(|_| "绘制到画布失败".to_string())?;
    // PNG 而不是 JPEG：标志类图常有透明区，JPEG 会把它们填成黑色
    canvas
        .to_data_url_with_type("image/png")
        .map_err(|_| "编码 PNG 失败".to_string())
}
