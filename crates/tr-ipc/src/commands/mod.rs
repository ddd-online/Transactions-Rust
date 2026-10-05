//! IPC 命令实现。
//!
//! 命令按业务域分模块：`ledger` / `tr` / `category` / `tag` / `template` / `chart` /
//! `key_event` / `diary` / `stock` / `todo`。
//!
//! 每个命令都用 `#[tauri::command]` 标注、接收 `tauri::State<'_, AppState>`，
//! 由 `src-tauri` 的 `generate_handler![]` 集中注册（命令清单因此只在一处）。
//!
//! ## 名字这条链由三处断言闭合
//!
//! 界面的命令名来自 `tr_domain::commands` 的清单；命令名在上线时有三处"必须一致"，
//! 每处都有一条能跑的断言，不靠人读注释：
//!
//! | 断言 | 位置 | 证明 |
//! |---|---|---|
//! | 实现 == 清单 | 本模块的 `implementations_match_the_catalog`（`-Unit ipc`） | 本 crate 里的 `#[tauri::command]` 函数名集合与清单逐名相同 |
//! | 注册表 == 清单 | `src-tauri/src/registry.rs` 的 `catalog_matches_registration`（`-Unit ipc`／`shell`） | 注册清单（`generate_handler![]` 与它同源）与清单逐名相同 |
//!
//! 传递起来就是"注册的每一条都真的存在、存在的每一条都真的注册了"。
//! 另外 `registration_name_is_the_function_name` 钉住这条链依赖的前提：
//! 命令**不能**用 `rename` / `rename_all` 改注册名（`#[tauri::command]` 默认拿函数名当名字）。

pub mod category;
pub mod chart;
pub mod diary;
pub mod key_event;
pub mod ledger;
pub mod stock;
pub mod tag;
pub mod template;
pub mod todo;
pub mod tr;

pub use category::*;
pub use chart::*;
pub use diary::*;
pub use key_event::*;
pub use ledger::*;
pub use stock::*;
pub use tag::*;
pub use template::*;
pub use todo::*;
pub use tr::*;

use tr_domain::error::AppError;

use crate::error::{ApiError, ApiResult};

/// 条件不满足时返回 400 与固定文案（`msg` 是用户可见契约，逐字不得改动）。
pub(crate) fn require(cond: bool, msg: &'static str) -> ApiResult<()> {
    if cond {
        return Ok(());
    }
    Err(ApiError::from(AppError::bad_request(msg)))
}

/// 公共约束：请求体里的 `ledger_id` 为空即报 `ledger_id is required`
/// （字段缺失时的取值口径由 `tr_domain::wire` 里各结构体的 `#[serde(default)]` 决定）。
pub(crate) fn require_ledger_id(ledger_id: &str) -> ApiResult<()> {
    require(!ledger_id.is_empty(), "ledger_id is required")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use tr_domain::commands::{catalog_mismatch, BUSINESS_COMMANDS};

    /// 命令源文件。
    ///
    /// `include_str!` 而不是运行时读盘：测试目标与源码一起被缓存，
    /// 改了命令源码就会触发重编（不会拿着一份过期的文本做断言）。
    /// **新增命令模块时要把它加进这张表**，否则这里的扫描看不见它
    /// （注册守卫仍然兜底：注册了却没进清单一样会红）。
    const SOURCES: &[(&str, &str)] = &[
        ("category.rs", include_str!("category.rs")),
        ("chart.rs", include_str!("chart.rs")),
        ("diary.rs", include_str!("diary.rs")),
        ("key_event.rs", include_str!("key_event.rs")),
        ("ledger.rs", include_str!("ledger.rs")),
        ("stock.rs", include_str!("stock.rs")),
        ("tag.rs", include_str!("tag.rs")),
        ("template.rs", include_str!("template.rs")),
        ("todo.rs", include_str!("todo.rs")),
        ("tr.rs", include_str!("tr.rs")),
    ];

    /// `#[tauri::command]` 属性的字面写法（扫描用，不引入正则依赖）。
    const ATTRIBUTE: &str = "#[tauri::command]";

    /// 扫出源文件里所有 `#[tauri::command]` 的函数名。
    ///
    /// 手写扫描而不是正则：本 crate 不为一条测试引入依赖。
    fn declared_commands() -> Vec<(&'static str, &'static str)> {
        let mut found = Vec::new();
        for (file, source) in SOURCES {
            let mut rest = *source;
            while let Some(index) = rest.find(ATTRIBUTE) {
                rest = &rest[index + ATTRIBUTE.len()..];
                // 签名以 `{` 结束（属性与签名之间可能还有别的属性，这里只取到函数体之前）
                let Some(end) = rest.find('{') else { break };
                let signature = &rest[..end];
                let name = signature
                    .split_whitespace()
                    .skip_while(|token| *token != "fn")
                    .nth(1)
                    // 名字后面紧跟 `(`（可能有别的形参），取前导标识符部分
                    .map(|token| {
                        let end = token
                            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                            .unwrap_or(token.len());
                        &token[..end]
                    });
                match name {
                    Some(name) => found.push((*file, name)),
                    // 扫不到函数名说明写法变了（例如 `pub(crate) fn`）——
                    // 宁可红，也不要静默漏掉一条命令
                    None => panic!("{file}: 在 `{ATTRIBUTE}` 之后没找到函数名"),
                }
            }
        }
        found
    }

    /// 命令实现与 `tr_domain::commands` 的清单逐名相同（不重不漏）。
    ///
    /// 这是"名字"这条链在本 crate 的那一半：**实现 == 清单**。
    /// 另一半（注册表 == 清单）在 `src-tauri/src/registry.rs` —— 那边能看到
    /// `generate_handler![]` 的调用处，本 crate 看不到。
    #[test]
    fn implementations_match_the_catalog() {
        let found = declared_commands();
        let implemented: Vec<&str> = found.iter().map(|(_, name)| *name).collect();

        assert_eq!(
            found.len(),
            implemented.iter().collect::<BTreeSet<_>>().len(),
            "有命令被实现了两次：{found:?}"
        );
        assert_eq!(
            catalog_mismatch(BUSINESS_COMMANDS, &implemented),
            None,
            "本 crate 的 `#[tauri::command]` 与 tr_domain::commands 清单不一致"
        );

        // 负向断言：拿**真实的扫描结果**去比一份少了一条的清单，必须红 ——
        // 否则上面那条可能只是在测自己（比较函数本身的敏感度另有一组单测，
        // 在 tr_domain::commands 里）。
        let without_one: Vec<&str> = BUSINESS_COMMANDS
            .iter()
            .copied()
            .filter(|name| *name != "ledger_list")
            .collect();
        assert_eq!(without_one.len(), BUSINESS_COMMANDS.len() - 1);
        assert!(
            catalog_mismatch(&without_one, &implemented).is_some(),
            "清单少一条没被发现"
        );
    }

    /// 注册名 = 函数名。
    ///
    /// `#[tauri::command]` 默认拿 `stringify!(fn)` 当线上命令名，而
    /// `src-tauri/src/registry.rs` 的守卫正是按"函数路径的最后一段"去核清单 ——
    /// 一旦有人给命令加上 `rename` / `rename_all`，那条守卫会**静默**失去准确性。
    /// 所以这里把这条前提钉死：属性必须光秃秃的。
    #[test]
    fn registration_name_is_the_function_name() {
        for (file, source) in SOURCES {
            for (index, line) in source.lines().enumerate() {
                let line = line.trim();
                let Some(rest) = line.strip_prefix("#[tauri::command") else {
                    continue;
                };
                assert!(
                    rest.starts_with(']'),
                    "{file}:{} 给命令加了参数（{line}）：注册名就不再等于函数名，\
                     请同步更新 src-tauri/src/registry.rs 的守卫",
                    index + 1
                );
            }
        }
    }

    /// 扫描本身要能扫到东西（否则上面两条断言会在"一条都没扫到"时静默成立）。
    #[test]
    fn the_scan_finds_commands() {
        assert!(
            !declared_commands().is_empty(),
            "一条命令都没扫到 —— 扫描逻辑或文件表坏了"
        );
    }
}
