//! 共用辅助：当前时间、GORM 式错误语义。
//!
//! 各业务 Dao 随 P2–P4 阶段逐个补齐（账本、交易、分类、标签、图表、模板、
//! 关键事件、日记、股票）；本模块只放跨 Dao 的公共工具。

pub mod category;
pub mod chart;
pub mod diary;
pub mod key_event;
pub mod key_event_image;
pub mod ledger;
pub mod stock;
pub mod tag;
pub mod transaction_record;
pub mod transaction_record_tag;
pub mod transaction_template;

/// 当前 Unix 秒（与 GORM `autoCreateTime:unix` / `autoUpdateTime:unix` 一致）。
pub(crate) fn now_unix() -> i64 {
    crate::util::now_unix()
}

/// 等价 Go 的 `dao.IsNotFound(err)`（GORM 的 `ErrRecordNotFound` → rusqlite 的
/// `QueryReturnedNoRows`）。服务层用它在"不存在则创建"这类逻辑里区分"查无记录"与真实错误。
pub fn is_not_found(error: &rusqlite::Error) -> bool {
    matches!(error, rusqlite::Error::QueryReturnedNoRows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_not_found_only_matches_missing_rows() {
        assert!(is_not_found(&rusqlite::Error::QueryReturnedNoRows));
        assert!(!is_not_found(&rusqlite::Error::InvalidQuery));
    }

    #[test]
    fn now_unix_is_plausible_seconds() {
        // 2020-01-01 之后、2100 年之前
        let now = now_unix();
        assert!(now > 1_577_836_800, "now = {now}");
        assert!(now < 4_102_444_800, "now = {now}");
    }
}
