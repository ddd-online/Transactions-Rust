//! IPC 业务命令的**名字清单** —— 界面与命令面共用的唯一来源。
//!
//! ## 两句话
//!
//! * **字段名靠共享类型**：请求 / 响应结构都在 [`crate::wire`] / [`crate::dto`] /
//!   [`crate::models`]，界面与内核引用同一份定义，字段名不可能漂移；
//! * **名字靠共享清单**：就是本模块。界面侧不再出现命令名字符串字面量
//!   （`ipc::call(commands::LEDGER_LIST, req)`），拼错名字**编译期**就报错，
//!   而不是等到运行时表现为"命令不存在"。
//!
//! ## 条目是什么
//!
//! 一条 [`Command`] 只声明三件事：**名字 + 请求类型 + 响应类型**，不放任何逻辑。
//! 于是界面侧的调用点可以写成
//!
//! ```ignore
//! ipc::call(commands::LEDGER_LIST, IdRequest { id: id.to_string() }).await
//! ```
//!
//! 名字、请求、响应三者从同一条目取，不可能出现"名字换了、类型没换"。
//!
//! ## 谁保证它没写错
//!
//! 清单自己编不过就编不过（类型写错即编译错误）；"清单里的每一条都真的被注册了"
//! 由应用外壳的注册守卫断言（`src-tauri/src/registry.rs` 的
//! `catalog_matches_registration`，跑在 `fixtures/test.ps1` 的 `test-src-tauri` 里）——
//! 命令注册表唯一存在于 `generate_handler![]` 的调用处，只有那儿能证明这件事。
//!
//! ## 边界
//!
//! 这里只有**业务命令**（`tr-ipc` 实现的 85 条）。桌面外壳与更新那 27 条命令的名字
//! 仍在各自的封装里以字符串出现，纳入本清单是另一票的事（#28）。

use std::marker::PhantomData;

use crate::dto::{
    CategoryDto, ChartDto, ChartQueryRequest, ChartQueryResponse, CreateCategoryRequest,
    CreateChartRequest, CreateTagRequest, DiaryExportRequest, DiaryExportResult, DiaryScanResponse,
    DiaryUpsertRequest, InitializeCategoriesResponse, LedgerDto, StockFundRecordPage, StockNameDto,
    StockOperationDto, StockOperationRollbackDto, StockOperationRollbackPreviewDto,
    StockOverviewDto, StockPositionDto, StockStatisticsDto, StockTradeDto,
    StockTradeHistoryDetailDto, StockTradeHistoryDto, StockTradeHistorySummaryDto,
    StockTradeImpactDto, StockTradeTagSettingDto, TagDto, TodoCardDto, TodoHistoryDto, TodoItemDto,
    TodoProgressDto, TrQueryCondition, TrQueryResult, TransactionRecordDto, TransactionTemplateDto,
    UpdateCategorySortRequest, UpdateChartRequest, UpdateTagSortRequest,
};
use crate::models::{DiaryDateItem, DiaryEntry, KeyEvent, KeyEventImage, StockFeeSetting};
use crate::wire::{
    CategoryDeleteRequest, CategoryListRequest, ChartIdRequest, ChartListRequest,
    CreateLedgerRequest, DiaryDateRequest, DiaryImportFileRequest, DiaryLedgerRequest,
    DiaryScanRequest, IdRequest, InitializeCategoriesRequest, KeyEventDateRequest,
    KeyEventImageAddRequest, KeyEventUpsertRequest, LedgerIdRequest, LedgerListRequest,
    LinkRequest, LinkedByDateRequest, StockAmountDateRequest, StockArchiveRequest,
    StockFeeSettingsRequest, StockFundRecordsRequest, StockNameRequest, StockPositionReviewRequest,
    StockRoundReviewRequest, StockRoundTagRequest, StockStatisticsRequest, StockTagSettingsRequest,
    StockTradeCreateRequest, StockTradeImpactRequest, StockTradeOrderDeleteRequest,
    StockTradeUpdateRequest, StockTradesRequest, TagDeleteRequest, TagListRequest,
    TemplateIdRequest, TemplateListRequest, TemplateSortRequest, TodoCardCreateRequest,
    TodoCardSortRequest, TodoItemCreateRequest, TodoItemStatusRequest, TodoItemUpdateRequest,
    TodoProgressCreateRequest, TodoProgressDoneRequest, UnlinkRequest, UpdateLedgerRequest,
    YearRequest,
};

/// 一条 IPC 命令的声明：**名字 + 请求类型 + 响应类型**（没有任何逻辑）。
///
/// `Req` / `Res` 只是类型参数，运行时不占空间；它们的作用是让界面侧的
/// `ipc::call(条目, req)` 从条目本身推出该发什么、该收什么 —— 名字与类型因此
/// 不可能各说各话。无请求形参的命令用 `()` 当 `Req`（业务命令里没有这种，
/// 桌面/更新命令才有，见模块注释的边界）。
pub struct Command<Req, Res> {
    name: &'static str,
    kinds: PhantomData<fn(Req) -> Res>,
}

impl<Req, Res> Command<Req, Res> {
    const fn new(name: &'static str) -> Self {
        Self {
            name,
            kinds: PhantomData,
        }
    }

    /// 线上命令名（既成契约，逐字不变）。
    pub const fn name(&self) -> &'static str {
        self.name
    }
}

// 手写而不是 derive：derive 会给 `Req` / `Res` 也加上 `Copy` 约束，
// 而这两个类型参数只是标记，与条目能不能拷贝无关。
impl<Req, Res> Clone for Command<Req, Res> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Req, Res> Copy for Command<Req, Res> {}

/// 声明清单：一行 = 一条命令（常量 = 命令名 + 请求类型 => 响应类型）。
///
/// 常量与 [`BUSINESS_COMMANDS`] 从**同一次声明**展开，所以不存在
/// "常量写了一个名字、清单里是另一个"这种半漂移。
macro_rules! command_catalog {
    ($($konst:ident = $name:literal : $req:ty => $res:ty),* $(,)?) => {
        $(
            #[doc = concat!("`", $name, "`")]
            pub const $konst: Command<$req, $res> = Command::new($name);
        )*

        /// 清单里的全部业务命令名（声明顺序）。
        ///
        /// 应用外壳的注册守卫拿它去核"清单 == 注册表"；顺序也在这里固定下来，
        /// 便于人读（与 `src-tauri/src/registry.rs` 的注册顺序一致）。
        pub const BUSINESS_COMMANDS: &[&str] = &[$($name),*];
    };
}

command_catalog! {
    // ---- 账本 ----
    LEDGER_LIST = "ledger_list": LedgerListRequest => Vec<LedgerDto>,
    LEDGER_CREATE = "ledger_create": CreateLedgerRequest => String,
    LEDGER_GET = "ledger_get": IdRequest => LedgerDto,
    LEDGER_UPDATE = "ledger_update": UpdateLedgerRequest => (),
    LEDGER_DELETE = "ledger_delete": IdRequest => (),

    // ---- 记账记录 ----
    TR_QUERY = "tr_query": TrQueryCondition => TrQueryResult,
    TR_CHART_DATA = "tr_chart_data": ChartQueryRequest => ChartQueryResponse,
    TR_CREATE = "tr_create": TransactionRecordDto => String,
    // 请求形参就是数组本身（没有外层结构体），响应是"插入了几条"
    TR_BATCH_CREATE = "tr_batch_create": Vec<TransactionRecordDto> => i32,
    TR_DELETE = "tr_delete": IdRequest => (),
    TR_LINK = "tr_link": LinkRequest => String,
    TR_UNLINK = "tr_unlink": UnlinkRequest => String,
    TR_LINKED_BY_DATE = "tr_linked_by_date": LinkedByDateRequest => Vec<TransactionRecordDto>,

    // ---- 日记 ----
    DIARY_LIST_DATES = "diary_list_dates": DiaryLedgerRequest => Vec<DiaryDateItem>,
    DIARY_GET = "diary_get": DiaryDateRequest => DiaryEntry,
    DIARY_UPSERT = "diary_upsert": DiaryUpsertRequest => DiaryEntry,
    DIARY_DELETE = "diary_delete": DiaryDateRequest => (),
    DIARY_IMPORT_SCAN = "diary_import_scan": DiaryScanRequest => DiaryScanResponse,
    DIARY_IMPORT_FILE = "diary_import_file": DiaryImportFileRequest => DiaryEntry,
    DIARY_EXPORT = "diary_export": DiaryExportRequest => DiaryExportResult,

    // ---- 分类 ----
    CATEGORY_LIST = "category_list": CategoryListRequest => Vec<CategoryDto>,
    CATEGORY_CREATE = "category_create": CreateCategoryRequest => (),
    CATEGORY_DELETE = "category_delete": CategoryDeleteRequest => (),
    CATEGORY_UPDATE_SORT = "category_update_sort": UpdateCategorySortRequest => (),
    CATEGORY_INITIALIZE = "category_initialize": InitializeCategoriesRequest => InitializeCategoriesResponse,

    // ---- 标签 ----
    TAG_LIST = "tag_list": TagListRequest => Vec<TagDto>,
    TAG_CREATE = "tag_create": CreateTagRequest => (),
    TAG_DELETE = "tag_delete": TagDeleteRequest => (),
    TAG_UPDATE_SORT = "tag_update_sort": UpdateTagSortRequest => (),

    // ---- 模板 ----
    TEMPLATE_CREATE = "template_create": TransactionTemplateDto => String,
    TEMPLATE_LIST = "template_list": TemplateListRequest => Vec<TransactionTemplateDto>,
    TEMPLATE_DELETE = "template_delete": TemplateIdRequest => (),
    TEMPLATE_UPDATE_SORT = "template_update_sort": TemplateSortRequest => (),

    // ---- 图表 ----
    CHART_CREATE = "chart_create": CreateChartRequest => ChartDto,
    CHART_DELETE = "chart_delete": ChartIdRequest => (),
    CHART_LIST = "chart_list": ChartListRequest => Vec<ChartDto>,
    CHART_UPDATE = "chart_update": UpdateChartRequest => ChartDto,

    // ---- 关键事件 ----
    KEY_EVENT_LIST_BY_YEAR = "key_event_list_by_year": YearRequest => Vec<KeyEvent>,
    KEY_EVENT_DATES_BY_YEAR = "key_event_dates_by_year": YearRequest => Vec<String>,
    KEY_EVENT_GET = "key_event_get": KeyEventDateRequest => KeyEvent,
    KEY_EVENT_UPSERT = "key_event_upsert": KeyEventUpsertRequest => String,
    KEY_EVENT_DELETE = "key_event_delete": KeyEventDateRequest => (),
    KEY_EVENT_IMAGES_LIST = "key_event_images_list": KeyEventDateRequest => Vec<KeyEventImage>,
    KEY_EVENT_IMAGE_ADD = "key_event_image_add": KeyEventImageAddRequest => KeyEventImage,
    KEY_EVENT_IMAGE_DELETE = "key_event_image_delete": IdRequest => (),

    // ---- 股票 ----
    STOCK_OVERVIEW = "stock_overview": LedgerIdRequest => StockOverviewDto,
    STOCK_PRINCIPAL_ADD = "stock_principal_add": StockAmountDateRequest => StockOverviewDto,
    STOCK_INTEREST_ADD = "stock_interest_add": StockAmountDateRequest => StockOverviewDto,
    STOCK_WITHDRAW = "stock_withdraw": StockAmountDateRequest => StockOverviewDto,
    STOCK_FEE_SETTINGS_GET = "stock_fee_settings_get": LedgerIdRequest => StockFeeSetting,
    STOCK_FEE_SETTINGS_PUT = "stock_fee_settings_put": StockFeeSettingsRequest => StockFeeSetting,
    STOCK_TAG_SETTINGS_GET = "stock_tag_settings_get": LedgerIdRequest => StockTradeTagSettingDto,
    STOCK_TAG_SETTINGS_PUT = "stock_tag_settings_put": StockTagSettingsRequest => StockTradeTagSettingDto,
    STOCK_FUND_RECORDS = "stock_fund_records": StockFundRecordsRequest => StockFundRecordPage,
    STOCK_POSITIONS = "stock_positions": LedgerIdRequest => Vec<StockPositionDto>,
    STOCK_POSITION_REVIEW = "stock_position_review": StockPositionReviewRequest => StockPositionDto,
    STOCK_TRADES = "stock_trades": StockTradesRequest => Vec<StockTradeDto>,
    STOCK_TRADE_CREATE = "stock_trade_create": StockTradeCreateRequest => Vec<StockTradeDto>,
    STOCK_TRADE_UPDATE = "stock_trade_update": StockTradeUpdateRequest => StockTradeDto,
    STOCK_TRADE_ORDER_DELETE = "stock_trade_order_delete": StockTradeOrderDeleteRequest => bool,
    STOCK_TRADE_IMPACT = "stock_trade_impact": StockTradeImpactRequest => StockTradeImpactDto,
    STOCK_HISTORY = "stock_history": LedgerIdRequest => Vec<StockTradeHistoryDto>,
    STOCK_HISTORY_DETAIL = "stock_history_detail": StockTradesRequest => StockTradeHistoryDetailDto,
    STOCK_HISTORY_SUMMARY = "stock_history_summary": LedgerIdRequest => StockTradeHistorySummaryDto,
    STOCK_ROUND_REVIEW = "stock_round_review": StockRoundReviewRequest => StockTradeHistoryDetailDto,
    STOCK_ROUND_TAG = "stock_round_tag": StockRoundTagRequest => StockTradeHistoryDetailDto,
    STOCK_STATISTICS = "stock_statistics": StockStatisticsRequest => StockStatisticsDto,
    STOCK_NAME = "stock_name": StockNameRequest => StockNameDto,
    STOCK_RESET = "stock_reset": LedgerIdRequest => bool,
    STOCK_ARCHIVE = "stock_archive": StockArchiveRequest => String,
    STOCK_OPERATION_LIST = "stock_operation_list": LedgerIdRequest => Vec<StockOperationDto>,
    STOCK_OPERATION_PREVIEW = "stock_operation_preview": LedgerIdRequest => StockOperationRollbackPreviewDto,
    STOCK_OPERATION_ROLLBACK = "stock_operation_rollback": LedgerIdRequest => StockOperationRollbackDto,

    // ---- 待办 ----
    TODO_CARDS = "todo_cards": LedgerIdRequest => Vec<TodoCardDto>,
    TODO_HISTORY = "todo_history": LedgerIdRequest => Vec<TodoHistoryDto>,
    TODO_CARD_CREATE = "todo_card_create": TodoCardCreateRequest => TodoCardDto,
    TODO_CARD_DELETE = "todo_card_delete": IdRequest => (),
    TODO_CARD_SORT = "todo_card_sort": TodoCardSortRequest => (),
    TODO_ITEM_CREATE = "todo_item_create": TodoItemCreateRequest => TodoItemDto,
    TODO_ITEM_UPDATE = "todo_item_update": TodoItemUpdateRequest => TodoItemDto,
    TODO_ITEM_STATUS = "todo_item_status": TodoItemStatusRequest => TodoItemDto,
    TODO_ITEM_DELETE = "todo_item_delete": IdRequest => (),
    TODO_PROGRESS_ADD = "todo_progress_add": TodoProgressCreateRequest => TodoProgressDto,
    TODO_PROGRESS_DELETE = "todo_progress_delete": IdRequest => (),
    TODO_PROGRESS_DONE = "todo_progress_done": TodoProgressDoneRequest => (),
}

/// 比较"清单"与"实际观测到的名字"：一一对应返回 `None`，否则给出两个方向的差集。
///
/// 两处守卫共用它（`tr-ipc` 的 `implementations_match_the_catalog` 比的是
/// "本 crate 里的命令实现"，应用外壳的 `catalog_matches_registration` 比的是
/// "注册表"），所以"什么算不一致"只有一份口径：**不重不漏**。
///
/// 比集合而不是比顺序：清单的顺序只是可读性；但条数与重复都算数，所以排序后逐个比
/// （重复项会在排序结果里露出来）。顺序有意不参与 —— 注册顺序与声明顺序对齐是约定，
/// 不是契约，不该让守卫因为"把新命令加在末尾"而红。
pub fn catalog_mismatch(declared: &[&str], observed: &[&str]) -> Option<String> {
    let mut declared: Vec<&str> = declared.to_vec();
    let mut observed: Vec<&str> = observed.to_vec();
    declared.sort_unstable();
    observed.sort_unstable();
    if declared == observed {
        return None;
    }
    let missing: Vec<&&str> = declared
        .iter()
        .filter(|name| !observed.contains(name))
        .collect();
    let extra: Vec<&&str> = observed
        .iter()
        .filter(|name| !declared.contains(name))
        .collect();
    Some(format!(
        "清单里有、实际没有：{missing:?}；实际有、清单里没有：{extra:?}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 命令名以域前缀开头（`<域>_<动作>`，见 `tr-ipc` 的模块注释）。
    const DOMAINS: &[&str] = &[
        "ledger",
        "tr",
        "diary",
        "category",
        "tag",
        "template",
        "chart",
        "key_event",
        "stock",
        "todo",
    ];

    #[test]
    fn names_are_unique_and_snake_case() {
        let mut seen = std::collections::BTreeSet::new();
        for name in BUSINESS_COMMANDS {
            assert!(seen.insert(*name), "命令名重复：{name}");
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "命令名必须是 snake_case：{name}"
            );
            assert!(
                name.starts_with(|c: char| c.is_ascii_lowercase()),
                "命令名不能以 '_' 或数字开头：{name}"
            );
        }
        assert_eq!(seen.len(), BUSINESS_COMMANDS.len());
    }

    #[test]
    fn every_name_is_prefixed_by_a_known_domain() {
        for name in BUSINESS_COMMANDS {
            assert!(
                DOMAINS
                    .iter()
                    .any(|domain| name.starts_with(&format!("{domain}_"))),
                "命令名不在任何已知域前缀下：{name}"
            );
        }
    }

    /// 清单的 10 个域都要有命令 —— 少一个域说明整块漏了（不是少一条）。
    #[test]
    fn every_domain_contributes_at_least_one_command() {
        for domain in DOMAINS {
            let prefix = format!("{domain}_");
            assert!(
                BUSINESS_COMMANDS
                    .iter()
                    .any(|name| name.starts_with(&prefix)),
                "域 {domain} 一条命令都没有"
            );
        }
    }

    /// 条目自己报的名字要与清单里的名字一致（宏把两者从同一行展开，
    /// 这条断言防的是"宏被改坏"这类退化）。
    #[test]
    fn entries_report_the_declared_name() {
        assert_eq!(LEDGER_LIST.name(), "ledger_list");
        assert_eq!(TR_BATCH_CREATE.name(), "tr_batch_create");
        assert_eq!(TODO_PROGRESS_DONE.name(), "todo_progress_done");
        for entry in [LEDGER_LIST.name(), CHART_UPDATE.name(), STOCK_NAME.name()] {
            assert!(BUSINESS_COMMANDS.contains(&entry), "{entry} 不在清单里");
        }
    }

    /// 类型参数真的接上了：这几条要么编译不过，要么就证明条目带着请求与响应类型。
    #[test]
    fn entries_carry_request_and_response_types() {
        fn assert_command<Req, Res>(entry: Command<Req, Res>) -> &'static str {
            entry.name()
        }
        assert_eq!(assert_command(LEDGER_GET), "ledger_get");
        assert_eq!(assert_command(CATEGORY_CREATE), "category_create");
        assert_eq!(
            assert_command(STOCK_TRADE_ORDER_DELETE),
            "stock_trade_order_delete"
        );

        // 值的类型就是声明里的类型（反证：换一个类型就编译不过）
        let request: IdRequest = IdRequest {
            id: "x".to_string(),
        };
        let entry: Command<IdRequest, LedgerDto> = LEDGER_GET;
        assert_eq!(entry.name(), "ledger_get");
        assert_eq!(request.id, "x");
    }

    #[test]
    fn catalog_is_not_empty() {
        assert!(BUSINESS_COMMANDS.len() > 80);
    }

    /// 比较函数本身要敏感：两处守卫都靠它，它要是"恒等"那两条守卫就都在测自己。
    ///
    /// 这是**负向断言**的所在层 —— 守卫各自的测试用真实输入（真清单 / 真注册表 /
    /// 真扫描结果）跑一遍"一致时必须是 `None`"，而"不一致时必须是 `Some`"由这里
    /// 逐种破坏方式钉住。
    #[test]
    fn catalog_mismatch_catches_every_kind_of_drift() {
        let declared = ["ledger_list", "ledger_create", "tr_query"];
        assert_eq!(catalog_mismatch(&declared, &declared), None);
        // 顺序不算不一致（清单顺序只是可读性）
        assert_eq!(
            catalog_mismatch(&declared, &["tr_query", "ledger_list", "ledger_create"]),
            None
        );

        // 少一条
        assert!(catalog_mismatch(&declared, &["ledger_list", "ledger_create"]).is_some());
        // 多一条
        assert!(catalog_mismatch(
            &declared,
            &["ledger_list", "ledger_create", "tr_query", "ghost"]
        )
        .is_some());
        // 改名（两边各差一条）
        assert!(catalog_mismatch(
            &declared,
            &["ledger_list_renamed", "ledger_create", "tr_query"]
        )
        .is_some());
        // 重复（条数没变，但少了一条真命令）
        assert!(catalog_mismatch(&declared, &["ledger_list", "ledger_list", "tr_query"]).is_some());
        // 空
        assert!(catalog_mismatch(&declared, &[]).is_some());
        assert!(catalog_mismatch(&[], &["ledger_list"]).is_some());
        assert_eq!(catalog_mismatch(&[], &[]), None);

        // 差集要说清是哪一边（排查时靠这句话）
        let message =
            catalog_mismatch(&declared, &["ledger_list", "ledger_create"]).expect("应当不一致");
        assert!(
            message.contains("tr_query"),
            "差集里应当点名缺的那条：{message}"
        );
    }
}
