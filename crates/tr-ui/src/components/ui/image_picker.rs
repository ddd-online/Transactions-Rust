//! 图片选择 / HEIC 转换 / 上传进度条。
//!
//! 对照原实现的三块：
//!
//! | 原文件 | 本模块 |
//! |---|---|
//! | `hooks/useImageUpload.ts` | [`read_as_data_url`] + [`HeicOutcome`] + [`UploadProgress`] 状态机 |
//! | `KeyEventImageGallery.vue` 的 `<input type="file">` | [`ImagePicker`]（隐藏 input + 触发按钮） |
//! | `UploadProgressBar.vue` | [`UploadProgressBar`]（顶部总进度 + 逐文件行 + 重试/跳过） |
//!
//! ## HEIC 的分工（与 AGENTS.md 一致）
//!
//! 后端只接受 JPEG/PNG/GIF/WebP，因此 **HEIC/HEIF 必须在界面层转成 JPEG**。
//! 原实现用 `heic-to`（libheif 的 wasm 版）；本仓库界面层不引入 JS 库，
//! 改为交给 **WebView2（Edge/Windows）自带的解码器 + canvas**：
//!
//! 1. 先把 HEIC 读成 `Blob`；
//! 2. 优先 `createImageBitmap`（对 HEIF 支持最好的路径），失败再退回
//!    `<img src=objectURL>`（部分 WebView2 版本只在这条路径上解 HEIF）；
//! 3. 画进 `<canvas>` → `toDataURL("image/jpeg", 0.92)` 直接得到 **JPEG data URI**；
//! 4. 把该 data URI 交给 `key_event_image_add`。
//!
//! 两条路径都失败时给出明确提示（[`HEIC_CONVERT_FAILED`]），
//! 与原实现 `'HEIC 转换失败: ' + message` 的语义一致。

use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::icons::{self, Icon};

/// HEIC/HEIF 的扩展名（照抄原 `useImageUpload.ts` 的 `HEIC_EXTENSIONS`）。
const HEIC_EXTENSIONS: [&str; 4] = [".heic", ".heif", ".HEIC", ".HEIF"];

/// JPEG 质量（原 `heicTo({ quality: 0.92 })`）。
const JPEG_QUALITY: f64 = 0.92;

/// HEIC 转换失败时的用户可见文案前缀（原实现同款）。
pub const HEIC_CONVERT_FAILED: &str = "HEIC 转换失败";

/// 文件名是否是 HEIC/HEIF。
pub fn is_heic(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    HEIC_EXTENSIONS
        .iter()
        .any(|extension| lower.ends_with(&extension.to_ascii_lowercase()))
}

/// 读取结果：`Ok(data URI)` / `Err(用户可见的错误文案)`。
pub type ReadOutcome = Result<String, String>;

/// 把一个文件读成可直接交给后端的 base64 data URI（HEIC 会先转 JPEG）。
///
/// `on_progress` 在关键节点回调（0 → 读取完成 30 → 转换完成 60），
/// 供上传进度条显示阶段；真实上传进度由后端/桥接层决定，这里只保底 60→100。
pub async fn read_as_data_url(file: &web_sys::File, on_progress: &dyn Fn(u8)) -> ReadOutcome {
    let name = file.name();
    on_progress(0);

    if !is_heic(&name) {
        let data = blob_to_data_url(&file.clone().into()).await?;
        on_progress(60);
        return Ok(data);
    }

    // HEIC：blob → 解码 → canvas → JPEG data URI
    let source: web_sys::Blob = file.clone().into();
    let data = convert_heic_to_jpeg(&source).await?;
    on_progress(60);
    Ok(data)
}

/// HEIC/HEIF blob → JPEG data URI。
///
/// 为什么直接用 `canvas.toDataURL("image/jpeg", 0.92)` 而不是 `toBlob` + `FileReader`：
/// 前者同步返回**同一个** data URI 结果，少一次 JS 回调与一次 blob 中转，
/// 而且 `to_blob_with_type_and_quality` 需要 `web-sys` 的 `BlobEvent` feature
/// （本仓库刻意不扩大 web-sys 的 feature 面）。
///
/// 失败时返回 `Err("HEIC 转换失败: ...")`（含具体原因，便于用户判断是不是
/// 当前 WebView2 缺少 HEIF 解码器）。
pub async fn convert_heic_to_jpeg(source: &web_sys::Blob) -> Result<String, String> {
    let (canvas, width, height) = match draw_source_to_canvas(source).await {
        Ok(value) => value,
        Err(reason) => return Err(format!("{HEIC_CONVERT_FAILED}: {reason}")),
    };
    if width == 0 || height == 0 {
        return Err(format!("{HEIC_CONVERT_FAILED}: 图片尺寸无效"));
    }
    // web-sys 暴露的是标准 `toDataURL(type, encoderOptions)` 的命名变体
    // （`_with_type_and_encoder_options`），质量参数走 JS 对象 `{quality: 0.92}`。
    let options = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &options,
        &JsValue::from_str("quality"),
        &JsValue::from_f64(JPEG_QUALITY),
    );
    canvas
        .to_data_url_with_type_and_encoder_options("image/jpeg", &options)
        .map_err(|_| format!("{HEIC_CONVERT_FAILED}: canvas.toDataURL 调用失败"))
}

/// 把源 blob 画进新建的 canvas，返回 `(canvas, 宽, 高)`。
///
/// 优先 `createImageBitmap`；失败退回 `<img>` + `objectURL`（见模块说明）。
async fn draw_source_to_canvas(
    source: &web_sys::Blob,
) -> Result<(web_sys::HtmlCanvasElement, u32, u32), String> {
    if let Ok((canvas, width, height)) = bitmap_path(source).await {
        return Ok((canvas, width, height));
    }
    image_element_path(source).await
}

/// 路径 1：`createImageBitmap(blob)` → canvas。
async fn bitmap_path(
    source: &web_sys::Blob,
) -> Result<(web_sys::HtmlCanvasElement, u32, u32), String> {
    let promise = create_image_bitmap(source)?;
    let value = JsFuture::from(promise)
        .await
        .map_err(|error| js_error_text(&error))?;
    let bitmap: web_sys::ImageBitmap = value
        .dyn_into()
        .map_err(|_| "浏览器未能解码该 HEIC 图片".to_string())?;
    let (width, height) = (bitmap.width(), bitmap.height());
    let canvas = create_canvas(width, height)?;
    let context = canvas_context(&canvas)?;
    context
        .draw_image_with_image_bitmap(&bitmap, 0.0, 0.0)
        .map_err(|_| "绘制图片到画布失败".to_string())?;
    Ok((canvas, width, height))
}

/// 路径 2：`<img src=objectURL>` → canvas。
async fn image_element_path(
    source: &web_sys::Blob,
) -> Result<(web_sys::HtmlCanvasElement, u32, u32), String> {
    let url = web_sys::Url::create_object_url_with_blob(source)
        .map_err(|_| "创建预览地址失败".to_string())?;

    let result = image_element_path_inner(&url, source).await;
    let _ = web_sys::Url::revoke_object_url(&url);
    result
}

async fn image_element_path_inner(
    url: &str,
    _source: &web_sys::Blob,
) -> Result<(web_sys::HtmlCanvasElement, u32, u32), String> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| "当前环境缺少 document".to_string())?;
    let image = document
        .create_element("img")
        .map_err(|_| "创建 img 元素失败".to_string())?
        .dyn_into::<web_sys::HtmlImageElement>()
        .map_err(|_| "创建 img 元素失败".to_string())?;

    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let image_for_load = image.clone();
        let loaded = Closure::<dyn FnMut()>::new(move || {
            let _ = image_for_load;
            let _ = resolve.call0(&JsValue::UNDEFINED);
        });
        let failed = Closure::<dyn FnMut()>::new(move || {
            let _ = reject.call1(&JsValue::UNDEFINED, &JsValue::from_str("图片解码失败"));
        });
        image.set_onload(Some(loaded.as_ref().unchecked_ref()));
        image.set_onerror(Some(failed.as_ref().unchecked_ref()));
        loaded.forget();
        failed.forget();
    });
    image.set_src(url);

    JsFuture::from(promise)
        .await
        .map_err(|error| js_error_text(&error))?;

    let width = image.natural_width();
    let height = image.natural_height();
    let canvas = create_canvas(width, height)?;
    let context = canvas_context(&canvas)?;
    context
        .draw_image_with_html_image_element(&image, 0.0, 0.0)
        .map_err(|_| "绘制图片到画布失败".to_string())?;
    Ok((canvas, width, height))
}

/// 创建指定尺寸的 canvas（尺寸为 0 时给 1×1，避免 `toBlob` 直接失败）。
fn create_canvas(width: u32, height: u32) -> Result<web_sys::HtmlCanvasElement, String> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| "当前环境缺少 document".to_string())?;
    let canvas = document
        .create_element("canvas")
        .map_err(|_| "创建 canvas 失败".to_string())?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| "创建 canvas 失败".to_string())?;
    canvas.set_width(width.max(1));
    canvas.set_height(height.max(1));
    Ok(canvas)
}

fn canvas_context(
    canvas: &web_sys::HtmlCanvasElement,
) -> Result<web_sys::CanvasRenderingContext2d, String> {
    canvas
        .get_context("2d")
        .map_err(|_| "获取 2D 画布上下文失败".to_string())?
        .ok_or_else(|| "获取 2D 画布上下文失败".to_string())?
        .dyn_into::<web_sys::CanvasRenderingContext2d>()
        .map_err(|_| "获取 2D 画布上下文失败".to_string())
}

/// blob → base64 data URI（原实现用 `FileReader.readAsDataURL`）。
pub async fn blob_to_data_url(blob: &web_sys::Blob) -> ReadOutcome {
    let reader = web_sys::FileReader::new().map_err(|_| "读取文件失败".to_string())?;
    let promise = {
        let captured = reader.clone();
        js_sys::Promise::new(&mut |resolve, reject| {
            // 闭包是 `FnMut`，因此每次调用都再克隆一份（FileReader 很廉价）
            let reader_for_load = captured.clone();
            let loaded = Closure::<dyn FnMut()>::new(move || {
                let value = reader_for_load
                    .result()
                    .unwrap_or_else(|_| JsValue::from_str(""));
                let _ = resolve.call1(&JsValue::UNDEFINED, &value);
            });
            let failed = Closure::<dyn FnMut()>::new(move || {
                let _ = reject.call1(&JsValue::UNDEFINED, &JsValue::from_str("读取文件失败"));
            });
            reader.set_onload(Some(loaded.as_ref().unchecked_ref()));
            reader.set_onerror(Some(failed.as_ref().unchecked_ref()));
            loaded.forget();
            failed.forget();
        })
    };

    if reader.read_as_data_url(blob).is_err() {
        return Err("读取文件失败".to_string());
    }
    match JsFuture::from(promise).await {
        Ok(value) => value
            .as_string()
            .filter(|text| !text.is_empty())
            .ok_or_else(|| "读取文件失败".to_string()),
        Err(error) => Err(js_error_text(&error)),
    }
}

/// `createImageBitmap` 是全局函数（可能不存在），这里封装成 `Result<Promise>`。
fn create_image_bitmap(source: &web_sys::Blob) -> Result<js_sys::Promise, String> {
    let global = js_sys::global();
    let function = js_sys::Reflect::get(&global, &JsValue::from_str("createImageBitmap"))
        .ok()
        .and_then(|value| value.dyn_into::<js_sys::Function>().ok())
        .ok_or_else(|| "当前环境不支持 createImageBitmap".to_string())?;
    function
        .call1(&global, source)
        .map_err(|_| "当前环境不支持 createImageBitmap".to_string())?
        .dyn_into::<js_sys::Promise>()
        .map_err(|_| "当前环境不支持 createImageBitmap".to_string())
}

/// 任意 JS 错误 → 可读文案（不 panic）。
fn js_error_text(error: &JsValue) -> String {
    if let Some(text) = error.as_string() {
        return text;
    }
    if let Some(error) = error.dyn_ref::<js_sys::Error>() {
        return String::from(error.message());
    }
    format!("{error:?}")
}

// ---------------------------------------------------------------- 上传进度

/// 单个文件的进度状态（照抄原 `UploadFileProgress`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadFileProgress {
    pub name: String,
    pub percent: u8,
    pub status: FileStatus,
    pub error_message: String,
}

/// 文件级状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Pending,
    Uploading,
    Done,
    Error,
}

impl FileStatus {
    /// CSS 类后缀（`file-row--*` / `file-status--*`）。
    pub fn class(self) -> &'static str {
        match self {
            FileStatus::Pending => "pending",
            FileStatus::Uploading => "uploading",
            FileStatus::Done => "done",
            FileStatus::Error => "error",
        }
    }

    /// 状态文字（逐字照抄原模板）。
    pub fn label(self) -> &'static str {
        match self {
            FileStatus::Pending => "等待中",
            FileStatus::Uploading => "",
            FileStatus::Done => "已完成",
            FileStatus::Error => "失败",
        }
    }
}

/// 整体状态（照抄原 `UploadProgress.status`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadStatus {
    Idle,
    Uploading,
    Done,
    Error,
}

/// 上传进度快照。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UploadProgress {
    pub files: Vec<UploadFileProgress>,
    pub total: usize,
    pub completed: usize,
    pub status: Option<UploadStatus>,
    pub error_message: String,
}

impl UploadProgress {
    /// 是否处于空闲（不渲染进度条）。
    pub fn is_idle(&self) -> bool {
        matches!(self.status, None | Some(UploadStatus::Idle))
    }

    /// 总进度百分比（`completed / total`，与原文一致）。
    pub fn overall_percent(&self) -> u8 {
        if self.total == 0 {
            return 0;
        }
        (((self.completed as f64 / self.total as f64) * 100.0).round() as i64).clamp(0, 100) as u8
    }

    /// 总进度条配色类名。
    pub fn bar_class(&self) -> &'static str {
        match self.status {
            Some(UploadStatus::Done) => "is-done",
            Some(UploadStatus::Error) => "is-error",
            _ => "",
        }
    }
}

/// 上传进度条（原 `UploadProgressBar.vue`）。
///
/// `on_retry` / `on_skip` 与原文的两个 emit 对应；`progress` 为空闲时渲染空视图。
#[component]
pub fn UploadProgressBar(
    /// 进度快照
    #[prop(into)]
    progress: Signal<UploadProgress>,
    /// 「重试」回调
    #[prop(optional, into)]
    on_retry: Option<UnsyncCallback<()>>,
    /// 「跳过」回调
    #[prop(optional, into)]
    on_skip: Option<UnsyncCallback<()>>,
) -> impl IntoView {
    view! {
        <Show when=move || !progress.get().is_idle()>
            <div class="upload-progress-bar">
                <div class="progress-summary">
                    <div class="summary-row">
                        <span class="summary-text">
                            {move || {
                                let snapshot = progress.get();
                                match snapshot.status {
                                    Some(UploadStatus::Uploading) => {
                                        format!(
                                            "上传中 {}/{}",
                                            snapshot.completed, snapshot.total,
                                        )
                                    }
                                    Some(UploadStatus::Done) => {
                                        format!("{} 张上传完成", snapshot.total)
                                    }
                                    Some(UploadStatus::Error) => {
                                        format!(
                                            "上传中断，已完成 {}/{}",
                                            snapshot.completed, snapshot.total,
                                        )
                                    }
                                    _ => String::new(),
                                }
                            }}
                        </span>
                        <span class="summary-percent">
                            {move || format!("{}%", progress.get().overall_percent())}
                        </span>
                    </div>
                    <div class="summary-bar-track">
                        <div
                            class=move || {
                                format!("summary-bar-fill {}", progress.get().bar_class())
                            }
                            style=move || {
                                format!(
                                    "transform: scaleX({})",
                                    f64::from(progress.get().overall_percent()) / 100.0,
                                )
                            }
                        ></div>
                    </div>
                </div>

                <div class="file-list">
                    {move || {
                        progress
                            .get()
                            .files
                            .into_iter()
                            .map(|file| {
                                let status = file.status;
                                view! {
                                    <div class=format!("file-row file-row--{}", status.class())>
                                        <span class="file-dot">
                                            {match status {
                                                FileStatus::Done => icons::icon(Icon::CheckCircle),
                                                FileStatus::Uploading => icons::icon(Icon::Loading),
                                                FileStatus::Error => icons::icon(Icon::CloseCircle),
                                                FileStatus::Pending => {
                                                    view! { <span class="dot-pending"></span> }
                                                        .into_any()
                                                }
                                            }}
                                        </span>
                                        <div class="file-body">
                                            <span class="file-name" title=file.name.clone()>
                                                {file.name.clone()}
                                            </span>
                                            <Show when=move || status == FileStatus::Uploading>
                                                <div class="file-bar-track">
                                                    <div
                                                        class="file-bar-fill"
                                                        style=format!(
                                                            "transform: scaleX({})",
                                                            f64::from(file.percent) / 100.0,
                                                        )
                                                    ></div>
                                                </div>
                                            </Show>
                                        </div>
                                        <span class=format!(
                                            "file-status file-status--{}",
                                            status.class(),
                                        )>
                                            {if status == FileStatus::Uploading {
                                                format!("{}%", file.percent)
                                            } else {
                                                status.label().to_string()
                                            }}
                                        </span>
                                    </div>
                                }
                            })
                            .collect_view()
                    }}

                    <Show when=move || {
                        progress.get().status == Some(UploadStatus::Error)
                    }>
                        <div class="error-actions">
                            <span class="error-msg">{move || progress.get().error_message}</span>
                            <div class="error-btns">
                                <button
                                    type="button"
                                    class="ui-btn ui-btn--sm"
                                    on:click=move |_| {
                                        if let Some(callback) = on_retry {
                                            callback.run(());
                                        }
                                    }
                                >
                                    "重试"
                                </button>
                                <button
                                    type="button"
                                    class="ui-btn ui-btn--sm"
                                    on:click=move |_| {
                                        if let Some(callback) = on_skip {
                                            callback.run(());
                                        }
                                    }
                                >
                                    "跳过"
                                </button>
                            </div>
                        </div>
                    </Show>
                </div>
            </div>
        </Show>
    }
}

// ---------------------------------------------------------------- 文件选择

/// 隐藏的 `<input type="file">` + 触发按钮（原 `KeyEventImageGallery.vue` 的
/// `<input ref="fileInput" type="file" :accept="accept" multiple hidden>`）。
///
/// 选中文件后回调 `(文件名, File)` 列表；`reset` 由调用方在每次选择后清空
/// input 的 `value`，否则同一个文件第二次选择不会触发 `change`。
#[component]
pub fn ImagePicker(
    /// `accept` 属性（默认 `image/*`）
    #[prop(optional, into)]
    accept: Option<String>,
    /// 允许一次选多张（默认 true）
    #[prop(optional)]
    multiple: bool,
    /// 按钮文案
    #[prop(optional, into)]
    label: Option<String>,
    /// 是否禁用
    #[prop(optional, into)]
    disabled: Option<Signal<bool>>,
    /// 选中文件回调
    #[prop(into)]
    on_files: UnsyncCallback<Vec<web_sys::File>>,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
) -> impl IntoView {
    let accept = accept.unwrap_or_else(|| "image/*".to_string());
    let label = label.unwrap_or_else(|| "上传图片".to_string());
    let class = class.unwrap_or_default();

    view! {
        <label class=format!("image-picker {class}")>
            <input
                type="file"
                class="image-picker__input"
                accept=accept
                multiple=multiple
                disabled=move || disabled.map(|signal| signal.get()).unwrap_or(false)
                on:change=move |event: leptos::ev::Event| {
                    let Some(input) = event
                        .target()
                        .and_then(|target| {
                            use wasm_bindgen::JsCast;
                            target.dyn_into::<web_sys::HtmlInputElement>().ok()
                        })
                    else {
                        return;
                    };
                    let Some(files) = input.files() else {
                        return;
                    };
                    let selected: Vec<web_sys::File> = (0..files.length())
                        .filter_map(|index| files.get(index))
                        .collect();
                    // 立即清空 value：同一文件二次选择也能触发 change
                    input.set_value("");
                    if !selected.is_empty() {
                        on_files.run(selected);
                    }
                }
            />
            <span class="image-picker__button">
                <span class="image-picker__icon">{icons::icon(Icon::Upload)}</span>
                {label}
            </span>
        </label>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heic_detection_is_case_insensitive() {
        assert!(is_heic("IMG_0001.HEIC"));
        assert!(is_heic("photo.heif"));
        assert!(is_heic("a.HeIc"));
        assert!(!is_heic("photo.jpg"));
        assert!(!is_heic("heic"));
    }

    #[test]
    fn overall_percent_guards_zero_total() {
        let progress = UploadProgress {
            total: 0,
            completed: 0,
            ..UploadProgress::default()
        };
        assert_eq!(progress.overall_percent(), 0);

        let progress = UploadProgress {
            total: 3,
            completed: 1,
            status: Some(UploadStatus::Uploading),
            ..UploadProgress::default()
        };
        assert_eq!(progress.overall_percent(), 33);
        assert!(!progress.is_idle());
    }

    #[test]
    fn file_status_labels_match_reference() {
        assert_eq!(FileStatus::Pending.label(), "等待中");
        assert_eq!(FileStatus::Done.label(), "已完成");
        assert_eq!(FileStatus::Error.label(), "失败");
    }
}
