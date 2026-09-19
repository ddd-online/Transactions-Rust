//! 桌面配置：`~/.transactions.json`（开发期 `~/.transactions-dev.json`）。
//!
//! **这是用户数据的一部分**：窗口尺寸/位置、上次工作空间目录、
//! 关闭行为、外观都写在这里。本实现必须读写同一文件、同一组键，并且——这一点很关键——
//! **保留未知键**：`extra` 用 `serde(flatten)` 捕获未识别的字段并原样写回，
//! 避免升级/降级时把别的版本写入的配置项抹掉。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// 生产配置文件名（用户数据契约，不可更改）。
pub const CONFIG_FILE: &str = ".transactions.json";
/// 开发配置文件名（用户数据契约，不可更改）。
pub const CONFIG_FILE_DEV: &str = ".transactions-dev.json";

/// 关闭行为取值。
pub const CLOSE_BEHAVIOR_QUIT: &str = "quit";
pub const CLOSE_BEHAVIOR_TRAY: &str = "tray";

/// 外观取值。
pub const APPEARANCE_SYSTEM: &str = "system";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    #[serde(rename = "width")]
    pub width: u32,
    #[serde(rename = "height")]
    pub height: u32,
    #[serde(rename = "x")]
    pub x: Option<i32>,
    #[serde(rename = "y")]
    pub y: Option<i32>,
    /// 上次使用的工作空间目录
    #[serde(rename = "workspaceDir")]
    pub workspace_dir: String,
    /// 关闭行为：quit / tray / 空（首次询问）
    #[serde(rename = "closeBehavior")]
    pub close_behavior: String,
    /// 外观：light / dark / system
    #[serde(rename = "appearance")]
    pub appearance: String,
    /// 未识别字段（其它版本写入的配置项）原样保留
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            // 默认窗口尺寸
            width: 1400,
            height: 1000,
            x: None,
            y: None,
            workspace_dir: String::new(),
            close_behavior: String::new(),
            appearance: APPEARANCE_SYSTEM.to_string(),
            extra: serde_json::Map::new(),
        }
    }
}

impl AppConfig {
    /// 配置文件路径（dev 与生产分离；文件名属于用户数据契约，不可更改）。
    pub fn path(dev: bool) -> PathBuf {
        let file = if dev { CONFIG_FILE_DEV } else { CONFIG_FILE };
        home_dir().join(file)
    }

    /// 读取配置；文件不存在或解析失败时回退默认值（只记日志、不中断启动）。
    pub fn load(path: &Path) -> Self {
        let Ok(content) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match serde_json::from_str(&content) {
            Ok(config) => config,
            Err(error) => {
                eprintln!("读取配置文件失败: {error}（已回退默认配置）");
                Self::default()
            }
        }
    }

    /// 写回配置（缩进 2 空格，保持既有的文件格式）。
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        std::fs::write(path, content)
    }
}

/// 线程安全的配置持有者：外壳命令与窗口事件都通过它读写。
pub struct ConfigStore {
    path: PathBuf,
    config: Mutex<AppConfig>,
}

impl ConfigStore {
    /// 从磁盘加载（dev 决定用哪个文件名）。
    pub fn load(dev: bool) -> Self {
        let path = AppConfig::path(dev);
        let config = AppConfig::load(&path);
        Self {
            path,
            config: Mutex::new(config),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 读取一份快照。
    pub fn snapshot(&self) -> AppConfig {
        self.config.lock().expect("配置锁中毒").clone()
    }

    /// 修改配置并立即落盘。
    pub fn update<T>(&self, edit: impl FnOnce(&mut AppConfig) -> T) -> T {
        let mut guard = self.config.lock().expect("配置锁中毒");
        let value = edit(&mut guard);
        if let Err(error) = guard.save(&self.path) {
            eprintln!("保存配置失败: {error}");
        }
        value
    }
}

/// 用户主目录。Windows 用 `USERPROFILE`，其它平台用 `HOME`。
fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tr-config-test-{tag}-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn defaults_match_documented_initial_values() {
        let config = AppConfig::default();
        assert_eq!(config.width, 1400);
        assert_eq!(config.height, 1000);
        assert_eq!(config.appearance, "system");
        assert!(config.workspace_dir.is_empty());
        assert!(config.close_behavior.is_empty());
    }

    #[test]
    fn reads_existing_config_file() {
        // 与真实 ~/.transactions.json 内容同构
        let json = r#"{
            "width": 1815,
            "height": 1316,
            "x": 364,
            "y": 43,
            "workspaceDir": "E:\\ljwfile\\transactions",
            "closeBehavior": "quit",
            "appearance": "system"
        }"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.width, 1815);
        assert_eq!(config.height, 1316);
        assert_eq!(config.x, Some(364));
        assert_eq!(config.workspace_dir, "E:\\ljwfile\\transactions");
        assert_eq!(config.close_behavior, "quit");
    }

    #[test]
    fn unknown_keys_survive_a_save_roundtrip() {
        let path = temp_path("unknown-keys");
        let json = r#"{
            "width": 800,
            "workspaceDir": "D:\\ws",
            "futureFeature": {"enabled": true},
            "legacyNumber": 7
        }"#;
        std::fs::write(&path, json).unwrap();

        let mut config = AppConfig::load(&path);
        config.close_behavior = CLOSE_BEHAVIOR_TRAY.to_string();
        config.save(&path).unwrap();

        let reloaded: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(reloaded["futureFeature"]["enabled"], true);
        assert_eq!(reloaded["legacyNumber"], 7);
        assert_eq!(reloaded["closeBehavior"], "tray");
        assert_eq!(reloaded["width"], 800);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn store_persists_updates() {
        let path = temp_path("store");
        std::fs::write(&path, "{\"width\": 1024}").unwrap();
        let config = AppConfig::load(&path);
        let store = ConfigStore {
            path: path.clone(),
            config: Mutex::new(config),
        };

        store.update(|cfg| cfg.workspace_dir = "D:\\ws".to_string());
        assert_eq!(store.snapshot().workspace_dir, "D:\\ws");
        assert_eq!(store.snapshot().width, 1024);

        let reloaded: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(reloaded["workspaceDir"], "D:\\ws");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn missing_or_broken_file_falls_back_to_defaults() {
        let missing = temp_path("missing");
        assert_eq!(AppConfig::load(&missing).width, 1400);

        let broken = temp_path("broken");
        std::fs::write(&broken, "{ not json").unwrap();
        assert_eq!(AppConfig::load(&broken).width, 1400);
        std::fs::remove_file(&broken).ok();
    }
}
