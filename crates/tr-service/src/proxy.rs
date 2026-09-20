//! 代理设置：**进程级槽位** + 系统代理探测（目前只支持 HTTP 代理）。
//!
//! ## 为什么是进程级槽位
//!
//! 配置（`~/.transactions.json` 的 `proxy` 键）由桌面外壳持有，而真正发请求的行情客户端在本层 ——
//! 两者之间隔着 `tr-ipc`。这里放一个**由外壳在启动与配置变更时写入**的槽位，
//! 于是 `quote.rs`（行情）与 `src-tauri/updater.rs`（更新检查/下载）读的是同一份设置，
//! 不会出现"更新走代理、行情不走"这种半生效。
//!
//! 纯逻辑（归一化 / 校验 / 解析系统值）都在 `tr_domain::proxy`，本模块只负责
//! **探测（I/O）**与**把设置变成 `ureq::Proxy`**。
//!
//! ## `auto` 的探测顺序（与 ureq 的环境变量顺序一致）
//!
//! ① `ALL_PROXY` / `HTTPS_PROXY` / `HTTP_PROXY`（含小写变体）→
//! ② Windows「Internet 选项」`HKCU` 的 `ProxyEnable`/`ProxyServer` → ③ 同路径的 `HKLM` → ④ 直连。
//!
//! **不支持**：PAC 自动配置脚本（`AutoConfigURL`，只作为提示上报，不解析）、
//! `ProxyOverride` 绕过列表（环境变量路径下由 ureq 的 `NO_PROXY` 处理）、SOCKS 与 HTTPS 代理。
//!
//! `auto` **每次请求都重新探测**：系统代理改了不必重启应用。

use std::sync::{OnceLock, RwLock};

use tr_domain::proxy::{
    parse_wininet_proxy_server, pick_env_proxy, ProxySetting, PROXY_MODE_MANUAL, PROXY_MODE_OFF,
};

/// 探测来源（给界面显示用）。
pub const SOURCE_ENV: &str = "env";
pub const SOURCE_SYSTEM: &str = "system";
pub const SOURCE_MANUAL: &str = "manual";
pub const SOURCE_NONE: &str = "none";

/// 一次解析的结果：最终地址 + 来源 + 给用户看的一句话。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProxy {
    /// 实际会用到的代理地址（`None` = 直连）。
    pub url: Option<String>,
    /// `env` / `system` / `manual` / `none`。
    pub source: &'static str,
    /// 系统里是否配置了 PAC 自动配置脚本（本版本不解析，只提示）。
    pub pac: bool,
    /// 直接展示给用户的说明（含"为什么没走代理"）。
    pub detail: String,
}

impl ResolvedProxy {
    /// 直连（带原因）。
    fn direct(source: &'static str, pac: bool, detail: impl Into<String>) -> Self {
        Self {
            url: None,
            source,
            pac,
            detail: detail.into(),
        }
    }
}

/// 进程级设置槽位（默认 `auto`，未写入时也按 `auto` 解析）。
static PROXY_SETTING: OnceLock<RwLock<ProxySetting>> = OnceLock::new();

fn slot() -> &'static RwLock<ProxySetting> {
    PROXY_SETTING.get_or_init(|| RwLock::new(ProxySetting::default()))
}

/// 写入当前设置（**由外壳在启动与配置变更时调用**）。
pub fn set(setting: ProxySetting) {
    let mut guard = slot().write().expect("代理设置锁中毒");
    *guard = setting;
}

/// 读一份当前设置的快照。
pub fn current() -> ProxySetting {
    slot().read().expect("代理设置锁中毒").clone()
}

/// 按当前设置解析出**可用**的代理地址（`auto` 会现场探测；非法值按未配置处理）。
pub fn resolved_url() -> Option<String> {
    resolve(&current()).url
}

/// 把（可能是空的/非法的）地址转成 `ureq::Proxy`；不合法时记一条 warning 并返回 `None`（退化为直连）。
pub fn proxy_from_url(url: Option<&str>) -> Option<ureq::Proxy> {
    let url = url?.trim();
    if url.is_empty() {
        return None;
    }
    match ureq::Proxy::new(url) {
        Ok(proxy) => Some(proxy),
        Err(error) => {
            tracing::warn!("代理地址不可用，已按直连处理: {url} ({error})");
            None
        }
    }
}

/// 一次请求要用的 `ureq::Proxy`（读当前设置；`off`/探测不到时是 `None`）。
pub fn agent_proxy() -> Option<ureq::Proxy> {
    proxy_from_url(resolved_url().as_deref())
}

/// 按给定设置解析（纯函数式入口：不读进程槽位，便于单测与界面"检测"按钮）。
pub fn resolve(setting: &ProxySetting) -> ResolvedProxy {
    match setting.effective_mode() {
        PROXY_MODE_OFF => ResolvedProxy::direct(SOURCE_NONE, false, "已关闭代理：所有网络请求直连"),
        PROXY_MODE_MANUAL => match setting.manual_url() {
            Some(url) => ResolvedProxy {
                url: Some(url.clone()),
                source: SOURCE_MANUAL,
                pac: false,
                detail: format!("手动代理：{url}"),
            },
            None => ResolvedProxy::direct(
                SOURCE_NONE,
                false,
                "手动代理地址无效，已按直连处理；请在「通用设置 → 代理」里修正",
            ),
        },
        _ => detect_system_proxy(),
    }
}

/// 自动探测本机代理：环境变量 → Windows 系统设置 → 直连。
///
/// 环境变量优先（与 ureq 的 `Proxy::try_from_env()` 一致）；系统设置只读、绝不写入注册表。
pub fn detect_system_proxy() -> ResolvedProxy {
    if let Some(url) = pick_env_proxy(|key| std::env::var(key).ok()) {
        return ResolvedProxy {
            url: Some(url.clone()),
            source: SOURCE_ENV,
            pac: false,
            detail: format!("已从环境变量读取代理：{url}"),
        };
    }

    let (system_url, pac) = system_proxy_from_registry();
    match system_url {
        Some(url) => {
            ResolvedProxy {
                url: Some(url.clone()),
                source: SOURCE_SYSTEM,
                pac,
                detail: if pac {
                    format!("已从 Windows 系统设置读取代理：{url}（系统另配了 PAC 自动脚本，本版本不解析）")
                } else {
                    format!("已从 Windows 系统设置读取代理：{url}")
                },
            }
        }
        None if pac => ResolvedProxy::direct(
            SOURCE_NONE,
            true,
            "系统只配置了 PAC 自动配置脚本（本版本不支持解析），已按直连处理",
        ),
        None => ResolvedProxy::direct(SOURCE_NONE, false, "未检测到本机代理：直连"),
    }
}

/// 读 Windows「Internet 选项」的静态代理（`HKCU` → `HKLM`），并回报是否配了 PAC。
#[cfg(windows)]
fn system_proxy_from_registry() -> (Option<String>, bool) {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    const PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";
    let mut pac = false;
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let Ok(key) = RegKey::predef(hive).open_subkey_with_flags(PATH, KEY_READ) else {
            continue;
        };
        if key
            .get_value::<String, _>("AutoConfigURL")
            .is_ok_and(|url| !url.trim().is_empty())
        {
            pac = true;
        }
        // `ProxyEnable` 缺失或为 0 都按"没启用静态代理"处理
        let enabled: u32 = key.get_value("ProxyEnable").unwrap_or(0);
        if enabled == 0 {
            continue;
        }
        let Ok(server) = key.get_value::<String, _>("ProxyServer") else {
            continue;
        };
        if let Some(url) = parse_wininet_proxy_server(&server) {
            return (Some(url), pac);
        }
    }
    (None, pac)
}

/// 非 Windows：只看环境变量（`pick_env_proxy` 已经查过，这里必然是直连）。
#[cfg(not(windows))]
fn system_proxy_from_registry() -> (Option<String>, bool) {
    (None, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tr_domain::proxy::{PROXY_MODE_AUTO, PROXY_MODE_OFF};

    #[test]
    fn proxy_from_url_tolerates_broken_values() {
        assert!(proxy_from_url(Some("http://127.0.0.1:1")).is_some());
        assert!(proxy_from_url(None).is_none());
        assert!(proxy_from_url(Some("   ")).is_none());
        // 不是 URL：记 warning + 退化为直连，**不 panic**
        assert!(proxy_from_url(Some("这不是地址")).is_none());
    }

    #[test]
    fn off_and_manual_modes_resolve_predictably() {
        let off = resolve(&ProxySetting {
            mode: PROXY_MODE_OFF.to_string(),
            url: "http://127.0.0.1:7890".to_string(),
        });
        assert_eq!(off.url, None);
        assert_eq!(off.source, SOURCE_NONE);

        let manual = resolve(&ProxySetting::manual("http://127.0.0.1:7890"));
        assert_eq!(manual.url.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(manual.source, SOURCE_MANUAL);

        // 手动地址非法 → 直连 + 明确的提示文案（不报错）
        let broken = resolve(&ProxySetting {
            mode: PROXY_MODE_MANUAL.to_string(),
            url: "socks5://127.0.0.1:1080".to_string(),
        });
        assert_eq!(broken.url, None);
        assert!(broken.detail.contains("无效"), "文案: {}", broken.detail);
    }

    #[test]
    fn auto_mode_never_panics_and_reports_a_source() {
        // 探测结果取决于本机环境（可能没配代理），只断言"invariant"：
        // 要么给出地址与来源，要么是直连 + 一句说明。
        let resolved = resolve(&ProxySetting {
            mode: PROXY_MODE_AUTO.to_string(),
            url: String::new(),
        });
        assert!(!resolved.detail.is_empty());
        match resolved.url {
            Some(url) => {
                assert!(url.starts_with("http://"), "只应给出 http 代理: {url}");
                assert!(matches!(resolved.source, SOURCE_ENV | SOURCE_SYSTEM));
            }
            None => assert_eq!(resolved.source, SOURCE_NONE),
        }
    }

    #[test]
    fn process_slot_roundtrips_and_defaults_to_auto() {
        let previous = current();
        set(ProxySetting::manual("http://127.0.0.1:7890"));
        assert_eq!(current().mode, PROXY_MODE_MANUAL);
        assert_eq!(resolved_url().as_deref(), Some("http://127.0.0.1:7890"));

        set(ProxySetting {
            mode: PROXY_MODE_OFF.to_string(),
            url: String::new(),
        });
        assert_eq!(resolved_url(), None);

        // 还原：本进程内其它测试可能依赖默认值
        set(previous);
    }
}
