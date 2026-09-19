//! 日志：程序目录 `logs/app.log`（5MB 轮转、保留 5 份）+ 工作空间 `transactions.log`。
//!
//! 契约里固定的部分：日志文件位置、`transactions.log` 的文件名（写入工作空间，
//! 便于用户随数据一起带走排障信息）、app.log 的轮转阈值与备份份数。
//! 日志行时间戳格式由本实现自行决定（这里由 tracing 输出，级别为大写），
//! 日志格式不属于数据兼容契约。

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;

/// 单文件上限 5MB。
const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024;
/// 备份份数。
const MAX_LOG_BACKUPS: u32 = 5;
const LOG_DIR: &str = "logs";
const LOG_FILE: &str = "app.log";

/// 日志落点集合：app.log 恒写；工作空间日志在打开工作空间后才写。
#[derive(Clone)]
pub struct LogSinks {
    inner: Arc<Inner>,
}

struct Inner {
    app_log: PathBuf,
    workspace_log: Mutex<Option<PathBuf>>,
}

impl LogSinks {
    /// 在程序目录下建立 `logs/`。目录不可写时退化为仅标准输出，不影响启动。
    pub fn new(app_dir: &Path) -> Self {
        let dir = app_dir.join(LOG_DIR);
        let _ = std::fs::create_dir_all(&dir);
        Self {
            inner: Arc::new(Inner {
                app_log: dir.join(LOG_FILE),
                workspace_log: Mutex::new(None),
            }),
        }
    }

    pub fn app_log_path(&self) -> &Path {
        &self.inner.app_log
    }

    /// 工作空间打开/切换时调用；`None` 表示关闭工作空间日志。
    pub fn set_workspace(&self, dir: Option<&Path>) {
        let mut guard = self.inner.workspace_log.lock().expect("日志锁中毒");
        *guard = dir.map(|dir| dir.join(tr_domain::consts::LOG_NAME));
    }
}

/// 每条日志同时写入 app.log 与（若已打开）工作空间 transactions.log。
pub struct SinkWriter {
    sinks: LogSinks,
}

impl Write for SinkWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let app_log = &self.sinks.inner.app_log;
        rotate_if_needed(app_log);
        let _ = append(app_log, buf);

        let workspace_log = self
            .sinks
            .inner
            .workspace_log
            .lock()
            .expect("日志锁中毒")
            .clone();
        if let Some(path) = workspace_log {
            let _ = append(&path, buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for LogSinks {
    type Writer = SinkWriter;

    fn make_writer(&'a self) -> Self::Writer {
        SinkWriter {
            sinks: self.clone(),
        }
    }
}

/// 初始化全局 tracing 订阅者（只允许调用一次）。
pub fn init_tracing(sinks: LogSinks) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(sinks)
        .with_ansi(false)
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::ChronoLocal::new(
            "%Y-%m-%d %H:%M:%S".to_string(),
        ))
        .try_init();
}

/// 追加一行（失败静默：日志不可写不应影响业务流程）。
fn append(path: &Path, buf: &[u8]) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(buf)?;
    file.flush()
}

/// 超过上限时轮转：删除最旧备份，`.log.1..N-1` 顺序后移，当前文件变 `.log.1`。
fn rotate_if_needed(path: &Path) {
    let size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    if size < MAX_LOG_SIZE {
        return;
    }
    let dir = path.parent().unwrap_or(Path::new("."));

    let _ = std::fs::remove_file(dir.join(format!("{LOG_FILE}.{MAX_LOG_BACKUPS}")));
    for index in (1..MAX_LOG_BACKUPS).rev() {
        let from = dir.join(format!("{LOG_FILE}.{index}"));
        if from.exists() {
            let _ = std::fs::rename(&from, dir.join(format!("{LOG_FILE}.{}", index + 1)));
        }
    }
    let _ = std::fs::rename(path, dir.join(format!("{LOG_FILE}.1")));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tr-log-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_to_app_log_and_workspace_log() {
        let app_dir = temp_dir("app");
        let workspace = temp_dir("ws");
        let sinks = LogSinks::new(&app_dir);
        sinks.set_workspace(Some(&workspace));

        let mut writer = sinks.make_writer();
        writer.write_all(b"hello\n").unwrap();

        let app_log = std::fs::read_to_string(sinks.app_log_path()).unwrap();
        assert_eq!(app_log, "hello\n");
        let workspace_log =
            std::fs::read_to_string(workspace.join(tr_domain::consts::LOG_NAME)).unwrap();
        assert_eq!(workspace_log, "hello\n");

        // 关闭工作空间日志后不再写入
        sinks.set_workspace(None);
        let mut writer = sinks.make_writer();
        writer.write_all(b"second\n").unwrap();
        let workspace_log =
            std::fs::read_to_string(workspace.join(tr_domain::consts::LOG_NAME)).unwrap();
        assert_eq!(workspace_log, "hello\n");

        std::fs::remove_dir_all(&app_dir).ok();
        std::fs::remove_dir_all(&workspace).ok();
    }

    #[test]
    fn rotates_when_over_size_limit() {
        let app_dir = temp_dir("rotate");
        let sinks = LogSinks::new(&app_dir);
        let app_log = sinks.app_log_path().to_path_buf();

        std::fs::write(&app_log, vec![b'x'; MAX_LOG_SIZE as usize]).unwrap();
        let mut writer = sinks.make_writer();
        writer.write_all(b"after-rotation\n").unwrap();

        assert!(app_log.exists(), "新日志文件应被重建");
        let dir = app_dir.join(LOG_DIR);
        assert!(dir.join("app.log.1").exists(), "旧文件应轮转为 app.log.1");
        assert_eq!(
            std::fs::read_to_string(&app_log).unwrap(),
            "after-rotation\n"
        );

        std::fs::remove_dir_all(&app_dir).ok();
    }
}
