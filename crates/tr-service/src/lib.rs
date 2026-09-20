//! tr-service —— 业务服务层。
//!
//! 调用链：api（命令面）→ service（业务规则）→ dao（SQL）→ models。
//! 本层不依赖 tauri，因此可以用普通 `cargo test` 对全部业务规则做无界面验证。
//!
//! 各业务服务随阶段补齐：
//! * P2：账本 / 消费记录（含筛选、统计、图表分桶）/ 分类 / 标签 / 模板 / 图表
//! * P3：关键事件 / 资产与缩略图 / 日记
//! * P4：股票（账户 / 委托与费用 / 持仓重放 / 轮次历史 / 统计 / 行情）
//!
//! 另外：`proxy` 是**进程级代理设置**（行情与外壳的更新器共用；探测系统代理）。

pub mod assets;
pub mod category;
pub mod chart;
pub mod diary;
pub mod error;
pub mod key_event;
pub mod ledger;
pub mod proxy;
pub mod quote;
pub mod stock;
pub mod stock_statistics;
pub mod tag;
pub mod transaction_record;
pub mod transaction_template;

pub use error::{ServiceError, ServiceResult};

/// 各模块测试共用的夹具（只在测试编译时存在）。
#[cfg(test)]
pub(crate) mod test_support {
    use tr_store::Workspace;

    /// 开一个全新的临时工作空间，返回 `(workspace, 目录)`；
    /// 目录名带模块名、进程号与纳秒时间戳，并发跑的测试互不干扰。
    /// 调用方测试结束时自行 `std::fs::remove_dir_all(&dir)`。
    pub fn workspace(tag: &str) -> (Workspace, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "tr-service-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        (Workspace::open(&dir).unwrap(), dir)
    }
}
