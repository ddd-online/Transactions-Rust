//! 列表区的**四态判定**：这一段该显示"加载中 / 失败 / 空 / 就绪"里的哪一个。
//!
//! 从前这条优先级在每个页面各判一遍（界面里 11 处 `if loading { "正在加载…" } else { "暂无××" }`），
//! 于是有两个真实后果：
//!
//! * **失败被渲染成业务空态** —— 用户被告知「暂无已清仓股票」，而其实是查询失败了；
//! * **首帧闪一下空态** —— `loading` 是在取数 module 的 effect 里才置位的，第一次渲染时它是 `false`、
//!   结果还是默认值，于是"还没回来"被判成了"确实是空"。
//!
//! 这里把优先级钉成一条规则（见 [`section_state`]），页面只负责把文案填进对应的槽位。

/// 列表区的四种状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionState {
    /// 还没跑完一次（含正在重拉）：显示加载态。
    Loading,
    /// 上一次取数失败了（失败文案在调用方手上）：显示失败态，**不要**显示成空。
    Failed,
    /// 跑完了、成功了，但确实没有数据。
    Empty,
    /// 有数据，正常渲染。
    Ready,
}

/// 四态的判据。
///
/// 优先级（写成断言在下面的测试里，逐条钉住）：
///
/// 1. **没跑完过（或正在跑）一律 `Loading`** —— 包括"失败了正在重试"这一段：手上那份错已经过期，
///    显示加载比显示旧错误更贴近事实；
/// 2. 跑完了且有失败文案 → `Failed`（**失败不是空**）；
/// 3. 跑完了、没失败、但没有数据 → `Empty`；
/// 4. 其余 → `Ready`。
///
/// `items_empty` 只看"手上这份结果是不是空的"，不区分它是默认值还是真结果 —— 那件事靠 `loaded` 表达。
pub fn section_state(
    items_empty: bool,
    loading: bool,
    loaded: bool,
    failed: Option<&str>,
) -> SectionState {
    if loading || !loaded {
        return SectionState::Loading;
    }
    if failed.is_some() {
        return SectionState::Failed;
    }
    if items_empty {
        return SectionState::Empty;
    }
    SectionState::Ready
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 首帧：还没跑完（`loading` 也还是 false）→ 加载中，**不能**判成空。
    #[test]
    fn the_first_frame_is_loading_not_empty() {
        assert_eq!(
            section_state(true, false, false, None),
            SectionState::Loading
        );
        assert_eq!(
            section_state(true, false, false, Some("炸了")),
            SectionState::Loading
        );
    }

    /// 失败不是空：跑完了、手上没数据、有失败文案 → `Failed`。
    #[test]
    fn a_failure_is_never_rendered_as_an_empty_list() {
        assert_eq!(
            section_state(true, false, true, Some("查询已清仓股票失败")),
            SectionState::Failed
        );
        // 同上，但手上还留着上一次的结果（keep_stale 那类页面）：仍然是失败态
        assert_eq!(
            section_state(false, false, true, Some("查询失败")),
            SectionState::Failed
        );
    }

    /// 重试期间显示加载：手上那份错已经过期。
    #[test]
    fn a_retry_in_flight_shows_loading_rather_than_the_stale_error() {
        assert_eq!(
            section_state(true, true, true, Some("上一次失败了")),
            SectionState::Loading
        );
    }

    /// 跑完了、没有失败、没有数据 → 空态。
    #[test]
    fn a_loaded_empty_list_is_the_empty_state() {
        assert_eq!(section_state(true, false, true, None), SectionState::Empty);
        // 失败被清掉之后（重试成功但确实没数据）也回到空态
        assert_eq!(section_state(true, false, true, None), SectionState::Empty);
    }

    #[test]
    fn a_loaded_list_with_items_is_ready() {
        assert_eq!(section_state(false, false, true, None), SectionState::Ready);
        // 重拉中仍显示加载（手上有数据也不闪回内容）
        assert_eq!(
            section_state(false, true, true, None),
            SectionState::Loading
        );
    }

    /// 16 种输入组合里，四态各自都该出现，且优先级不随 `items_empty` 变。
    #[test]
    fn the_precedence_holds_for_every_combination() {
        for items_empty in [true, false] {
            for loading in [true, false] {
                for loaded in [true, false] {
                    for failed in [None, Some("失败")] {
                        let state = section_state(items_empty, loading, loaded, failed);
                        let expected = if loading || !loaded {
                            SectionState::Loading
                        } else if failed.is_some() {
                            SectionState::Failed
                        } else if items_empty {
                            SectionState::Empty
                        } else {
                            SectionState::Ready
                        };
                        assert_eq!(
                            state, expected,
                            "items_empty={items_empty} loading={loading} loaded={loaded} failed={failed:?}"
                        );
                    }
                }
            }
        }
        // 四态都被覆盖到
        let mut seen = vec![
            section_state(true, false, false, None),
            section_state(true, false, true, Some("失败")),
            section_state(true, false, true, None),
            section_state(false, false, true, None),
        ];
        seen.sort_by_key(|state| format!("{state:?}"));
        seen.dedup();
        assert_eq!(seen.len(), 4);
    }
}
