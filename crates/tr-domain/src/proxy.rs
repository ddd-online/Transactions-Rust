//! 代理设置（目前只支持 **HTTP 代理**）。
//!
//! 三态：
//! * `off` —— 不使用代理（**显式**禁用，连环境变量代理也不读）；
//! * `auto` —— 自动探测：环境变量 → Windows「Internet 选项」→ 直连（见 `tr-service` 的探测实现）；
//! * `manual` —— 手动 `http://host:port`（可带 `user:pass@`，ureq 会做 Basic 认证）。
//!
//! 本模块只放**纯逻辑**（归一化 / 校验 / 解析），探测与网络都不在这里：
//! 这样界面（wasm）能在发请求前先本地校验，后端也能复用同一套文案，两侧不会漂移。
//! 错误是 [`String`]，内容**直接展示给用户**，改动即破坏文案契约。

use serde::{Deserialize, Serialize};

/// 不使用代理。
pub const PROXY_MODE_OFF: &str = "off";
/// 自动探测（环境变量 → 系统代理 → 直连）。
pub const PROXY_MODE_AUTO: &str = "auto";
/// 手动指定 HTTP 代理。
pub const PROXY_MODE_MANUAL: &str = "manual";
/// 手动代理支持的协议：**目前只有它**（界面「协议」下拉框里就这一项）。
pub const PROXY_SCHEME_HTTP: &str = "http";

/// 代理设置（`~/.transactions.json` 的 `proxy` 键；字段名是配置契约，不可改）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProxySetting {
    pub mode: String,
    pub url: String,
}

impl Default for ProxySetting {
    /// 缺省 = **自动探测**（旧配置里没有 `proxy` 键时也是它）：本机有代理就跟着走。
    fn default() -> Self {
        Self {
            mode: PROXY_MODE_AUTO.to_string(),
            url: String::new(),
        }
    }
}

impl ProxySetting {
    /// 手动模式 + 已归一化的地址。
    pub fn manual(url: impl Into<String>) -> Self {
        Self {
            mode: PROXY_MODE_MANUAL.to_string(),
            url: url.into(),
        }
    }

    /// 模式是否是已知取值（未知取值一律按 `auto` 处理，见 [`Self::effective_mode`]）。
    pub fn is_known_mode(mode: &str) -> bool {
        matches!(mode, PROXY_MODE_OFF | PROXY_MODE_AUTO | PROXY_MODE_MANUAL)
    }

    /// 实际生效的模式：手改配置写进未知值时回退到 `auto`（不 panic、不报错）。
    pub fn effective_mode(&self) -> &'static str {
        match self.mode.as_str() {
            PROXY_MODE_OFF => PROXY_MODE_OFF,
            PROXY_MODE_MANUAL => PROXY_MODE_MANUAL,
            _ => PROXY_MODE_AUTO,
        }
    }

    /// 手动模式下**可用**的代理地址；其它模式、或值非法时都是 `None`（非法值不报错，按未配置处理）。
    pub fn manual_url(&self) -> Option<String> {
        if self.effective_mode() != PROXY_MODE_MANUAL {
            return None;
        }
        normalize_http_proxy(&self.url).ok()
    }

    /// 校验界面/配置传来的 (mode, url)，返回可落盘的设置。`Err` 是给用户看的文案。
    ///
    /// `off` / `auto` 下**原样保留** `url`：用户切回手动时地址还在，不必重打一遍。
    pub fn validated(mode: &str, url: &str) -> Result<Self, String> {
        if !Self::is_known_mode(mode) {
            return Err(format!("无效的代理模式：{mode}"));
        }
        if mode == PROXY_MODE_MANUAL {
            return Ok(Self::manual(normalize_http_proxy(url)?));
        }
        Ok(Self {
            mode: mode.to_string(),
            url: url.trim().to_string(),
        })
    }
}

/// 归一化一个 HTTP 代理地址：接受 `host:port` 与 `http://[user:pass@]host:port`。
///
/// 只认 `http://`：`https://`（HTTPS 代理）与 `socks*://` 明确拒绝并说明原因，
/// 而不是静默当成 http —— 那样连不上时用户无从判断。
pub fn normalize_http_proxy(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("请填写代理地址（形如 http://127.0.0.1:7890）".to_string());
    }

    // scheme：有就校验，没有就按 http 处理（ureq 的默认行为一致）
    let authority = match raw.split_once("://") {
        Some((scheme, rest)) => {
            if !scheme.eq_ignore_ascii_case("http") {
                return Err(format!(
                    "仅支持 http:// 代理，暂不支持 {scheme}://（如需请手动填写本机转发端口）"
                ));
            }
            rest
        }
        None => raw,
    };

    // 路径 / 查询串没有意义，直接截掉（`http://host:8080/` 很常见）
    let authority = authority.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.trim();
    if authority.is_empty() {
        return Err("代理地址缺少主机名".to_string());
    }
    if authority.chars().any(char::is_whitespace) {
        return Err("代理地址含空格，请检查".to_string());
    }

    // 端口：从右往左找最后一个 `:`（IPv6 的 `[::1]:8080` 也能正确切分）
    let (host, port) = authority
        .rsplit_once(':')
        .ok_or_else(|| "代理地址缺少端口（形如 http://127.0.0.1:7890）".to_string())?;
    if host.is_empty() {
        return Err("代理地址缺少主机名".to_string());
    }
    let port: u16 = port
        .parse()
        .map_err(|_| "代理端口无效（应为 1-65535 的数字）".to_string())?;
    if port == 0 {
        return Err("代理端口无效（应为 1-65535 的数字）".to_string());
    }

    Ok(format!("http://{host}:{port}"))
}

/// 手动地址 → 表单的「域名 / 端口」两段（协议恒为 `http`，不在这里返回）。
///
/// 只做**形状**上的拆分，不做校验：拆不出来就返回空串（或整段），让表单留空/原样显示 ——
/// 唯一的判据仍是 [`normalize_http_proxy`]，它给出用户看到的文案。表单不是第二个校验入口。
/// 「域名」段保留 `user:pass@` 与 IPv6 的方括号，原样带回 [`join_http_proxy`]。
pub fn split_http_proxy(raw: &str) -> (String, String) {
    let raw = raw.trim();
    let authority = match raw.split_once("://") {
        Some((_, rest)) => rest,
        None => raw,
    };
    // 路径 / 查询串没有意义（老配置里可能是 `http://host:8080/`）
    let authority = authority.split(['/', '?', '#']).next().unwrap_or("").trim();
    if authority.is_empty() {
        return (String::new(), String::new());
    }
    match authority.rsplit_once(':') {
        // 从右往左找最后一个冒号（`[::1]:8080` 也切得对）；端口不像端口就整段当域名，
        // 别在这里猜 —— 用户还能看见自己填的东西，后端会说明哪里不对。
        Some((host, port))
            if !host.is_empty() && !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) =>
        {
            (host.to_string(), port.to_string())
        }
        _ => (authority.to_string(), String::new()),
    }
}

/// 表单的「协议 / 域名 / 端口」→ 一条 `scheme://host:port`（仍由 [`normalize_http_proxy`] 校验）。
///
/// 宽容之处只有一处：往「域名」框里**粘贴整条地址**是常见操作，所以丢掉它自带的
/// `scheme://` 前缀；它若自带端口而端口框空着，就原样带下去（拼出来还是 `host:port`，后端认得）。
/// 其余一律照抄：端口空着就**不加冒号**，让后端说"缺少端口"，而不是这里替他猜一个 80。
pub fn join_http_proxy(scheme: &str, host: &str, port: &str) -> String {
    let scheme = scheme.trim().trim_end_matches("://");
    let host = host.trim();
    let host = host
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(host)
        .trim();
    let port = port.trim();
    if host.is_empty() && port.is_empty() {
        // 表单空着：交给后端说「请填写代理地址（形如 http://127.0.0.1:7890）」
        return String::new();
    }
    if port.is_empty() {
        return format!("{scheme}://{host}");
    }
    format!("{scheme}://{host}:{port}")
}

/// 解析 Windows「Internet 选项」里的 `ProxyServer` 值。///
/// 该值有两种形态：裸 `host:port`，或 `http=host:port;https=host:port;ftp=…;socks=…`。
/// 只取 `http=` 段（退而取 `https=` 段）；`socks=` 段**不认**（本版本只支持 HTTP 代理）。
pub fn parse_wininet_proxy_server(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if !raw.contains('=') {
        return normalize_http_proxy(raw).ok();
    }

    let mut fallback = None;
    for part in raw.split(';') {
        let Some((scheme, value)) = part.split_once('=') else {
            continue;
        };
        let scheme = scheme.trim();
        let value = value.trim();
        if scheme.eq_ignore_ascii_case("http") {
            if let Ok(url) = normalize_http_proxy(value) {
                return Some(url);
            }
        } else if scheme.eq_ignore_ascii_case("https") && fallback.is_none() {
            fallback = normalize_http_proxy(value).ok();
        }
    }
    fallback
}

/// 从环境变量里挑代理：顺序与 ureq 的 `Proxy::try_from_env()` **完全一致**
/// （`ALL_PROXY` → `HTTPS_PROXY` → `HTTP_PROXY`，各含小写变体），
/// 这样"界面显示的代理"与"实际生效的代理"不会各说一套。
///
/// `lookup` 注入是为了单测不碰进程环境。
pub fn pick_env_proxy(lookup: impl Fn(&str) -> Option<String>) -> Option<String> {
    const KEYS: [&str; 6] = [
        "ALL_PROXY",
        "all_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ];
    for key in KEYS {
        let Some(value) = lookup(key) else { continue };
        if value.trim().is_empty() {
            continue;
        }
        if let Ok(url) = normalize_http_proxy(&value) {
            return Some(url);
        }
        // 非 http 的环境变量（例如 socks5://）跳过，继续看下一个
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_auto_and_unknown_mode_falls_back_to_auto() {
        assert_eq!(ProxySetting::default().mode, PROXY_MODE_AUTO);
        let broken = ProxySetting {
            mode: "whatever".to_string(),
            url: "http://127.0.0.1:7890".to_string(),
        };
        assert_eq!(broken.effective_mode(), PROXY_MODE_AUTO);
        assert_eq!(broken.manual_url(), None);
    }

    #[test]
    fn normalize_accepts_bare_host_port_and_http_url() {
        assert_eq!(
            normalize_http_proxy("127.0.0.1:7890").unwrap(),
            "http://127.0.0.1:7890"
        );
        assert_eq!(
            normalize_http_proxy("  http://127.0.0.1:7890/  ").unwrap(),
            "http://127.0.0.1:7890"
        );
        assert_eq!(
            normalize_http_proxy("http://user:pass@proxy.local:8080").unwrap(),
            "http://user:pass@proxy.local:8080"
        );
        assert_eq!(
            normalize_http_proxy("[::1]:1080").unwrap(),
            "http://[::1]:1080"
        );
    }

    #[test]
    fn normalize_rejects_non_http_scheme() {
        let err = normalize_http_proxy("socks5://127.0.0.1:1080").unwrap_err();
        assert!(err.contains("socks5"), "文案应点名不支持的协议: {err}");
        let err = normalize_http_proxy("https://127.0.0.1:8443").unwrap_err();
        assert!(err.contains("https"), "文案应点名不支持的协议: {err}");
    }

    #[test]
    fn normalize_rejects_broken_values() {
        assert!(normalize_http_proxy("").unwrap_err().contains("请填写"));
        assert!(normalize_http_proxy("127.0.0.1")
            .unwrap_err()
            .contains("端口"));
        assert!(normalize_http_proxy("127.0.0.1:0")
            .unwrap_err()
            .contains("端口"));
        assert!(normalize_http_proxy("127.0.0.1:70000")
            .unwrap_err()
            .contains("端口"));
        assert!(normalize_http_proxy(":7890")
            .unwrap_err()
            .contains("主机名"));
        assert!(normalize_http_proxy("http://127.0.0.1:78 90")
            .unwrap_err()
            .contains("空格"));
    }

    /// 表单三段 ↔ 一条地址：拆得干净、拼得回去，且拼出来的东西过得了后端那唯一一次校验。
    #[test]
    fn manual_form_parts_split_and_join_back_to_a_valid_url() {
        // 拆：归一化过的、裸 host:port（老配置里可能存的就是它）、带认证、IPv6
        assert_eq!(
            split_http_proxy("http://127.0.0.1:7890"),
            ("127.0.0.1".to_string(), "7890".to_string())
        );
        assert_eq!(
            split_http_proxy("127.0.0.1:7890"),
            ("127.0.0.1".to_string(), "7890".to_string())
        );
        assert_eq!(
            split_http_proxy("http://user:pass@proxy.local:8080"),
            ("user:pass@proxy.local".to_string(), "8080".to_string())
        );
        assert_eq!(
            split_http_proxy("http://[::1]:1080"),
            ("[::1]".to_string(), "1080".to_string())
        );
        // 缺端口 / 端口不像端口 / 空：都不猜，域名段原样留着让用户改
        assert_eq!(
            split_http_proxy("http://127.0.0.1"),
            ("127.0.0.1".to_string(), String::new())
        );
        assert_eq!(
            split_http_proxy("http://127.0.0.1:78a90"),
            ("127.0.0.1:78a90".to_string(), String::new())
        );
        assert_eq!(split_http_proxy("   "), (String::new(), String::new()));

        // 拼：正常、两端带空格、协议带 `://`、表单空着
        assert_eq!(
            join_http_proxy("http", "127.0.0.1", "7890"),
            "http://127.0.0.1:7890"
        );
        assert_eq!(
            join_http_proxy("http://", " 127.0.0.1 ", " 7890 "),
            "http://127.0.0.1:7890"
        );
        assert_eq!(join_http_proxy("http", "", ""), "");
        // 端口空着不加冒号（后端会说"缺少端口"），而不是替用户猜一个端口
        assert_eq!(join_http_proxy("http", "127.0.0.1", ""), "http://127.0.0.1");
        // 粘贴整条地址进「域名」框：丢掉自带 scheme，自带端口就留着
        assert_eq!(
            join_http_proxy("http", "http://127.0.0.1:7890", ""),
            "http://127.0.0.1:7890"
        );
        assert_eq!(
            join_http_proxy("http", "http://127.0.0.1", "7890"),
            "http://127.0.0.1:7890"
        );

        // 拼出来的一定过得了后端那唯一一次校验（表单不自己造文案）
        assert_eq!(
            normalize_http_proxy(&join_http_proxy("http", "127.0.0.1", "7890")).unwrap(),
            "http://127.0.0.1:7890"
        );
        assert_eq!(
            normalize_http_proxy(&join_http_proxy("http", "http://127.0.0.1:7890", "")).unwrap(),
            "http://127.0.0.1:7890"
        );
        assert!(
            normalize_http_proxy(&join_http_proxy("http", "user:pass@proxy.local", "8080")).is_ok()
        );
    }

    #[test]
    fn parsed_wininet_values_pick_the_http_entry() {
        assert_eq!(
            parse_wininet_proxy_server("127.0.0.1:7890").unwrap(),
            "http://127.0.0.1:7890"
        );
        assert_eq!(
            parse_wininet_proxy_server("ftp=1.2.3.4:21;http=127.0.0.1:7890;https=127.0.0.1:7891")
                .unwrap(),
            "http://127.0.0.1:7890"
        );
        // 没有 http= 时退而用 https=
        assert_eq!(
            parse_wininet_proxy_server("https=127.0.0.1:7891").unwrap(),
            "http://127.0.0.1:7891"
        );
        // 只有 socks= 段：不认（本版本只支持 HTTP 代理）
        assert_eq!(parse_wininet_proxy_server("socks=127.0.0.1:1080"), None);
        assert_eq!(parse_wininet_proxy_server("   "), None);
    }

    #[test]
    fn env_proxy_prefers_all_then_https_then_http() {
        let lookup = |key: &str| -> Option<String> {
            match key {
                "ALL_PROXY" => None,
                "HTTPS_PROXY" => Some("http://127.0.0.1:7891".to_string()),
                "HTTP_PROXY" => Some("127.0.0.1:7890".to_string()),
                _ => None,
            }
        };
        assert_eq!(
            pick_env_proxy(lookup).unwrap(),
            "http://127.0.0.1:7891",
            "HTTPS_PROXY 优先于 HTTP_PROXY（与 ureq 一致）"
        );

        // 非 http 的取值跳过，继续看下一个
        let lookup = |key: &str| -> Option<String> {
            match key {
                "ALL_PROXY" => Some("socks5://127.0.0.1:1080".to_string()),
                "HTTP_PROXY" => Some("127.0.0.1:7890".to_string()),
                _ => None,
            }
        };
        assert_eq!(pick_env_proxy(lookup).unwrap(), "http://127.0.0.1:7890");

        assert_eq!(pick_env_proxy(|_| None), None);
        assert_eq!(pick_env_proxy(|_| Some("   ".to_string())), None);
        assert_eq!(pick_env_proxy(|_| Some("socks5://x:1".to_string())), None);
    }

    #[test]
    fn validated_keeps_url_in_off_and_auto_but_normalizes_manual() {
        let manual = ProxySetting::validated("manual", " 127.0.0.1:7890 ").unwrap();
        assert_eq!(manual.mode, PROXY_MODE_MANUAL);
        assert_eq!(manual.url, "http://127.0.0.1:7890");
        assert_eq!(
            manual.manual_url().as_deref(),
            Some("http://127.0.0.1:7890")
        );

        let auto = ProxySetting::validated("auto", "127.0.0.1:7890").unwrap();
        assert_eq!(auto.mode, PROXY_MODE_AUTO);
        assert_eq!(auto.url, "127.0.0.1:7890", "auto 下地址原样保留");
        assert_eq!(auto.manual_url(), None);

        assert!(ProxySetting::validated("manual", "socks5://x:1")
            .unwrap_err()
            .contains("仅支持"));
        assert!(ProxySetting::validated("nope", "127.0.0.1:7890")
            .unwrap_err()
            .contains("无效的代理模式"));
    }

    #[test]
    fn setting_roundtrips_through_json() {
        let json = serde_json::to_string(&ProxySetting::manual("http://127.0.0.1:7890")).unwrap();
        assert_eq!(json, r#"{"mode":"manual","url":"http://127.0.0.1:7890"}"#);
        let back: ProxySetting = serde_json::from_str(&json).unwrap();
        assert_eq!(back.mode, PROXY_MODE_MANUAL);
        // 缺字段 / 空对象 → 默认（auto）
        let empty: ProxySetting = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, ProxySetting::default());
    }
}
