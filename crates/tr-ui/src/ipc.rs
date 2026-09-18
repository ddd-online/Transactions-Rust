//! Tauri 命令调用桥。
//!
//! 对照原 `app/src/backend/api/api-client.ts`：那里用 axios 调本机 HTTP API 并解析
//! `{code,msg,data}` 信封；这里改为调用 Tauri IPC（`window.__TAURI__.core.invoke`，
//! 由 `app.withGlobalTauri = true` 注入），对外语义保持一致：
//!
//! * 成功 → 直接返回数据
//! * 失败 → 抛出 [`IpcError`]；若 `msg == "未打开工作空间"`，额外派发 `workspace-required` 事件
//!   （等价原实现里 `extractErrorMessage` 的行为，外壳据此打开工作空间选择）
//!
//! 命令统一只收一个 `req` 参数对象，字段名与原 HTTP JSON body 逐字段一致。
//!
//! ## 错误前缀
//!
//! 原实现在 api-client 里把前缀拼进消息（`"{前缀}: {msg}"`），并在
//! [`crate::error_handler`] 的 `withErrorHandling` 里把同一前缀作为通知标题。
//! 这里把"前缀"保留为一条**只读**信息（[`IpcError::prefixed`]），由调用方决定
//! 是否展示，避免在桥接层里耦合通知。

use std::cell::RefCell;

use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use tr_domain::error::ERR_WORKSPACE_NOT_OPENED;

#[wasm_bindgen]
extern "C" {
    /// Tauri 注入的全局 IPC 入口（`withGlobalTauri: true`）。
    /// `catch` 让同步抛出（例如命令未注册）也变成 `Result`，不必依赖 JS 侧 try/catch。
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke, catch)]
    fn tauri_invoke(cmd: &str, args: JsValue) -> Result<js_sys::Promise, JsValue>;

    /// Tauri 注入的全局事件订阅（`window.__TAURI__.event.listen`）。
    /// 返回 `Promise<UnlistenFn>`；本层不保存 unlisten（界面生命周期 = 进程生命周期）。
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = listen, catch)]
    fn tauri_listen(event: &str, handler: &js_sys::Function) -> Result<js_sys::Promise, JsValue>;
}

/// 命令调用失败：携带错误信封里的 msg/status。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpcError {
    pub msg: String,
    pub status: u16,
}

impl IpcError {
    pub fn new(msg: impl Into<String>, status: u16) -> Self {
        Self {
            msg: msg.into(),
            status,
        }
    }

    /// 按原实现的文案规则拼出用户可见错误（`"{前缀}: {msg}"`）。
    ///
    /// 原实现见 `api-client.ts` 的 `extractErrorMessage`：
    /// 后端有 `msg` 时用 `"{前缀}: {msg}"`，否则退化为 `"{前缀}: {axios message}"`。
    pub fn prefixed(&self, prefix: &str) -> String {
        format!("{}: {}", prefix, self.msg)
    }

    /// 等价原 `getErrorMessage(error)`：取出可读信息（就是信封里的 `msg`）。
    pub fn message(&self) -> &str {
        &self.msg
    }

    /// 是否为"未打开工作空间"（外壳据此弹出工作空间选择）。
    pub fn is_workspace_required(&self) -> bool {
        self.msg == ERR_WORKSPACE_NOT_OPENED
    }
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for IpcError {}

/// 组装 `{ req: ... }` 参数对象；序列化失败属于本地桥接错误。
fn build_args<Req: Serialize>(req: &Req) -> Result<js_sys::Object, IpcError> {
    let args = js_sys::Object::new();
    let req_value = serde_wasm_bindgen::to_value(req).map_err(json_error)?;
    js_sys::Reflect::set(&args, &JsValue::from_str("req"), &req_value).map_err(parse_error)?;
    Ok(args)
}

/// 处理 promise 拒绝：识别"未打开工作空间"并派发 `workspace-required`。
fn handle_rejection(error: JsValue) -> IpcError {
    let ipc_error = parse_error(error);
    if ipc_error.is_workspace_required() {
        dispatch_workspace_required();
    }
    ipc_error
}

/// 调用一个命令（带 `req` 参数），成功时把结果反序列化为 `Res`。
pub async fn call<Req, Res>(command: &str, req: Req) -> Result<Res, IpcError>
where
    Req: Serialize,
    Res: DeserializeOwned,
{
    let args = build_args(&req)?;
    let promise = tauri_invoke(command, args.into()).map_err(parse_error)?;
    match JsFuture::from(promise).await {
        Ok(value) => serde_wasm_bindgen::from_value(value).map_err(json_error),
        Err(error) => Err(handle_rejection(error)),
    }
}

/// 调用一个返回空值的命令（`ApiResult<()>`）。
///
/// 刻意**不**把 resolve 值反序列化成 `()`：Tauri 对 `()` 返回 `null`/`undefined`，
/// 直接忽略返回值可以避免任何序列化边界问题。
pub async fn call_void<Req>(command: &str, req: Req) -> Result<(), IpcError>
where
    Req: Serialize,
{
    let args = build_args(&req)?;
    let promise = tauri_invoke(command, args.into()).map_err(parse_error)?;
    match JsFuture::from(promise).await {
        Ok(_) => Ok(()),
        Err(error) => Err(handle_rejection(error)),
    }
}

/// 调用一个**没有 `req` 形参**的命令（`config_get` / `workspace_get` /
/// `devtools_get_state` / `config_file_path` 属于这一类）。
///
/// 为什么不复用 [`call`] 发一个空的 `req`：那些命令的签名里根本没有 `req`，
/// 传空参数虽然目前会被 Tauri 忽略，但那是实现细节；显式走"无参数"路径更稳。
pub async fn call_no_args<Res>(command: &str) -> Result<Res, IpcError>
where
    Res: DeserializeOwned,
{
    let args = js_sys::Object::new();
    let promise = tauri_invoke(command, args.into()).map_err(parse_error)?;
    match JsFuture::from(promise).await {
        Ok(value) => serde_wasm_bindgen::from_value(value).map_err(json_error),
        Err(error) => Err(handle_rejection(error)),
    }
}

/// 调用一个**没有 `req` 形参**、返回空值的命令（`update_cancel` 属于这一类）。
///
/// 与 [`call_void`] 的差别只在参数：那条路径会发出一个 `{}` 作为 `req`
/// （对没有该形参的命令无害，但那是实现细节），这里显式走"无参数"路径。
pub async fn call_void_no_args(command: &str) -> Result<(), IpcError> {
    let args = js_sys::Object::new();
    let promise = tauri_invoke(command, args.into()).map_err(parse_error)?;
    match JsFuture::from(promise).await {
        Ok(_) => Ok(()),
        Err(error) => Err(handle_rejection(error)),
    }
}

/// 批量调用：按给定顺序**串行**发出同构调用，返回值顺序与入参一致。
///
/// 用途：原实现里那些 `Promise.all([...])` 的页面首屏（例如同时取账本+分类+标签）。
/// 之所以不做并发：wasm 单线程下并发只省往返排队，却需要跨任务回传结果（自建 oneshot
/// 或 `Promise.all` 的 JS 胶水），复杂度与收益不成比例；串行版本语义更简单，
/// 且错误逐条可见（不会像 `Promise.all` 一样丢掉已完成项的结果）。
pub async fn call_batch<Req, Res>(calls: Vec<(&str, Req)>) -> Vec<Result<Res, IpcError>>
where
    Req: Serialize,
    Res: DeserializeOwned,
{
    let mut results = Vec::with_capacity(calls.len());
    for (command, req) in calls {
        results.push(call::<_, Res>(command, req).await);
    }
    results
}

/// 序列化/反序列化失败：属于本地桥接错误，与内核无关。
fn json_error(error: serde_wasm_bindgen::Error) -> IpcError {
    IpcError::new(format!("IPC 数据编码失败: {error}"), 500)
}

/// 把 IPC 拒绝载荷解析为 [`IpcError`]；载荷不是预期结构时退化为字符串。
fn parse_error(error: JsValue) -> IpcError {
    #[derive(serde::Deserialize)]
    struct Envelope {
        msg: String,
        #[serde(default = "default_status")]
        status: u16,
    }
    fn default_status() -> u16 {
        500
    }

    if let Ok(parsed) = serde_wasm_bindgen::from_value::<Envelope>(error.clone()) {
        return IpcError::new(parsed.msg, parsed.status);
    }

    IpcError::new(
        error
            .as_string()
            .unwrap_or_else(|| "内核调用失败".to_string()),
        500,
    )
}

/// 派发 `workspace-required` 事件（原实现由 `api-client.ts` 在识别到
/// `未打开工作空间` 时派发，`Layout.vue` 监听后弹出工作空间选择）。
fn dispatch_workspace_required() {
    let Some(window) = web_sys::window() else {
        return;
    };
    if let Ok(event) = web_sys::CustomEvent::new("workspace-required") {
        let _ = window.dispatch_event(&event);
    }
}

// ---------------------------------------------------------------- 事件订阅

thread_local! {
    /// 保活的 JS 闭包槽位：`listen` 注册的回调必须比 `listen` 调用活得久，
    /// 否则 wasm-bindgen 会在 `Closure` 析构时把 JS 侧的函数一起释放。
    /// 界面进程即应用进程，永不注销，因此这里只增不减。
    static LISTENERS: RefCell<Vec<Box<dyn std::any::Any>>> = const { RefCell::new(Vec::new()) };
}

/// 订阅一个 Tauri 事件，把 `event.payload` 反序列化为 `T` 后交给 `callback`。
///
/// 载荷形状与 Tauri 2 一致：`{ event: string, id: number, payload: T }`。
/// 反序列化失败时只打 `warn` 日志（事件是旁路信号，不应让界面崩掉）。
///
/// 与原实现对照：Electron 版的 `window.electronAPI.on('update:download-progress', ...)`
/// 是同一语义，只是把 `ipcRenderer.on` 换成 Tauri 的事件总线。
pub fn listen<T, F>(event: &str, callback: F)
where
    T: DeserializeOwned + 'static,
    F: Fn(T) + 'static,
{
    // 闭包必须 `'static`：把事件名复制一份进来，别借用入参
    let event_name = event.to_string();
    let closure = Closure::<dyn FnMut(JsValue)>::new(move |value: JsValue| {
        let payload = js_sys::Reflect::get(&value, &JsValue::from_str("payload"))
            .unwrap_or(JsValue::UNDEFINED);
        match serde_wasm_bindgen::from_value::<T>(payload) {
            Ok(parsed) => callback(parsed),
            Err(error) => {
                leptos::logging::warn!("事件 {} 载荷解析失败: {error}", event_name);
            }
        }
    });

    let result = tauri_listen(event, closure.as_ref().unchecked_ref::<js_sys::Function>());
    match result {
        Ok(promise) => {
            // 立即把 unlisten 函数丢掉：订阅的生命周期与界面一致，无需退订
            leptos::task::spawn_local(async move {
                let _ = JsFuture::from(promise).await;
            });
            LISTENERS.with(|slot| slot.borrow_mut().push(Box::new(closure)));
        }
        Err(error) => {
            leptos::logging::warn!("事件 {event} 订阅失败: {:?}", error.as_string());
        }
    }
}
