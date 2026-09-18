//! 进度条 —— 对应原 `a-progress`（`percent` / `:show-info`）。
//!
//! 用于「关于软件」的下载进度。百分比由外部传入（`update:download-progress` 事件的
//! `percent` 已经是 0..=100 的整数，直接 `as f64` 即可，不要再乘除）。

use leptos::prelude::*;

#[component]
pub fn Progress(
    /// 百分比（0..=100，越界会被夹紧）
    #[prop(into)]
    percent: Signal<f64>,
    /// 是否在右侧显示百分比文字
    #[prop(optional)]
    show_text: bool,
    /// 附加类名
    #[prop(optional, into)]
    class: Option<String>,
) -> impl IntoView {
    let clamped = move || percent.get().clamp(0.0, 100.0);

    let mut classes = String::from("ui-progress");
    if let Some(extra) = class.as_deref() {
        classes.push(' ');
        classes.push_str(extra);
    }

    view! {
        <div class=classes>
            <div class="ui-progress__track">
                <div
                    class="ui-progress__bar"
                    style=move || format!("width: {}%;", clamped())
                ></div>
            </div>
            <Show when=move || show_text>
                <span class="ui-progress__text">{move || format!("{}%", clamped().round())}</span>
            </Show>
        </div>
    }
}
