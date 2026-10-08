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

/// 同 [`section_state`]，但**重拉时不把手上已有的内容换成加载态**。
///
/// 差别只有一条：**手上有内容且没有失败文案 ⇒ `Ready`**（哪怕 `loading` 是 true）。
///
/// 为什么需要它（真实缺陷）：页面在写入成功后习惯"整表重拉"，而重拉会让 `loading` 置位 ——
/// 若按 [`section_state`] 判成 `Loading`，整块列表会被换成"正在加载…"，列表所在的那一屏
/// **高度瞬间塌成一行**；浏览器此时会把滚动位置夹回顶部（而且不会自动恢复），展开着的
/// 面板/浮层也一起消失，用户看到的就是"页面像是刷新了"（待办页写进度即复现）。
/// 首帧（`!loaded`）与"手上没有内容"仍按原规则显示加载态；失败仍显示失败态 ——
/// **失败既不当成空，也不拿旧内容掩盖**。
pub fn section_state_keeping_content(
    items_empty: bool,
    loading: bool,
    loaded: bool,
    failed: Option<&str>,
) -> SectionState {
    if !items_empty && failed.is_none() {
        return SectionState::Ready;
    }
    section_state(items_empty, loading, loaded, failed)
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

    /// 「重拉不塌内容」：手上有内容时不显示加载态，其余三态与 [`section_state`] 完全一致。
    #[test]
    fn keeping_content_only_overrides_the_loading_branch() {
        // 重拉中（手上有内容）⇒ Ready：这是这条规则存在的唯一理由
        assert_eq!(
            section_state_keeping_content(false, true, true, None),
            SectionState::Ready
        );
        // 首帧（还没跑完一次、手上也没有内容）⇒ 仍然是 Loading
        assert_eq!(
            section_state_keeping_content(true, false, false, None),
            SectionState::Loading
        );
        // 手上**有**内容（`Query::keep` 的缓存那一类）⇒ 哪怕还没跑完一次也先显示它
        assert_eq!(
            section_state_keeping_content(false, false, false, None),
            SectionState::Ready
        );
        // 手上没有内容时与 section_state 逐字一致（重拉 / 跑完为空 / 失败）
        for items_empty in [true, false] {
            for loading in [false, true] {
                for failed in [None, Some("查询失败")] {
                    for loaded in [false, true] {
                        if !items_empty {
                            continue; // 有内容那一侧由上面两条断言覆盖
                        }
                        assert_eq!(
                            section_state_keeping_content(items_empty, loading, loaded, failed),
                            section_state(items_empty, loading, loaded, failed),
                            "items_empty={items_empty} loading={loading} loaded={loaded} failed={failed:?}"
                        );
                    }
                }
            }
        }
        // 失败优先：有内容但这次取数失败了 ⇒ Failed（不拿旧内容掩盖失败）
        assert_eq!(
            section_state_keeping_content(false, false, true, Some("查询待办失败")),
            SectionState::Failed
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
