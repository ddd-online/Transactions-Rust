//! 线上类型（wire）：界面与内核之间 IPC 的请求 / 响应 / 事件载荷。
//!
//! ## 为什么放在 tr-domain
//!
//! `tr-ipc` 依赖 tauri（编不到 wasm32），界面编不了它；而两侧**必须**就字段名达成一致。
//! 放在这里（两侧都依赖、且本 crate 没有任何 I/O 依赖）之后，界面与命令面共用同一份定义，
//! 字段漂移在编译期就不可能发生 —— `ProxySetting`、`CreateChartRequest` 一直就是这么做的，
//! 本模块只是把剩下那批"界面另抄一份"的类型收拢过来。
//!
//! 于是 `crates/tr-ui/src/api/*.rs` 里不再有请求结构体，
//! 逐字段比对两份手抄副本的 `fixtures/contract-audit.ps1` 也随之删除。
//!
//! ## 字段名是硬契约
//!
//! `#[serde(rename)]` / `alias` 逐字保留历史命名：camelCase（`ledgerId`、
//! `categoryTransactionType`）与 snake_case（`ledger_id`、`transaction_id`）混用是既成事实，
//! **不要"顺手统一"**。结构体上的 `#[serde(default)]` 同样原样保留：它决定"少传一个字段"
//! 是报错还是取零值（请求体侧的原样保留尤其重要，改了就换了错误文案与语义）。

use serde::{Deserialize, Serialize};

use crate::proxy::ProxySetting;

// ================================================================ 通用

/// 只带一个 `id` 的请求（账本查询 / 删除、消费记录删除、事件图片删除）。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct IdRequest {
    pub id: String,
}

/// 只带账本 id 的请求（线名 `ledger_id`；驼峰 `ledgerId` 的写法见各自的结构体）。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LedgerIdRequest {
    pub ledger_id: String,
}

// ================================================================ 账本

/// 账本列表请求：`id` 为 `all` 或账本 id（缺省 = 空串 = 未指定）。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LedgerListRequest {
    #[serde(default)]
    pub id: String,
}

/// 新建账本。`name` 为空由命令层报 `请输入账本名称`。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CreateLedgerRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// 修改账本名称与描述。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct UpdateLedgerRequest {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

// ================================================================ 分类 / 标签 / 模板 / 图表

/// 分类列表：参数 `type`（空串与 `all` 等价：不过滤），同时接受 `transactionType`。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CategoryListRequest {
    #[serde(rename = "type", alias = "transactionType")]
    pub transaction_type: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
}

/// 删除分类：`name` + `type`（同样接受 `transactionType`）+ `ledgerId`。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CategoryDeleteRequest {
    pub name: String,
    #[serde(rename = "type", alias = "transactionType")]
    pub transaction_type: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
}

/// 初始化（写入默认分类）：只带账本。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct InitializeCategoriesRequest {
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
}

/// 某分类下的标签列表。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TagListRequest {
    #[serde(rename = "categoryTransactionType")]
    pub category_transaction_type: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
}

/// 删除标签。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TagDeleteRequest {
    pub name: String,
    #[serde(rename = "categoryTransactionType")]
    pub category_transaction_type: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
}

/// 模板列表（按账本）。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TemplateListRequest {
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
}

/// 只带模板 id 的请求（同时接受 `templateId`，方便界面沿用 DTO 命名）。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TemplateIdRequest {
    #[serde(alias = "templateId")]
    pub id: String,
}

/// 模板拖拽排序后的落库参数。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TemplateSortRequest {
    /// 模板 ID（同时接受 `templateId`）
    #[serde(alias = "templateId")]
    pub id: String,
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
    #[serde(rename = "sortOrder")]
    pub sort_order: i32,
}

/// 图表 id 请求：DTO 里的字段名是 `chartId`，也接受 `id`。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChartIdRequest {
    #[serde(rename = "chartId", alias = "id")]
    pub chart_id: String,
}

/// 图表列表（按账本）。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChartListRequest {
    #[serde(rename = "ledgerId")]
    pub ledger_id: String,
}

// ================================================================ 消费记录 / 关联事件

/// 只带记录 id 的请求（删除消费记录）。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TransactionIdRequest {
    pub id: String,
}

/// 关联一条记录到某天的事件（请求体里取 `transaction_id`，不是 `transactionId`）。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LinkRequest {
    pub transaction_id: String,
    pub date: String,
}

/// 解除关联。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct UnlinkRequest {
    pub transaction_id: String,
}

/// 某天已关联的记录。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LinkedByDateRequest {
    pub date: String,
    pub ledger_id: String,
}

// ================================================================ 日记

/// 日记按账本隔离，入参一律 snake_case。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DiaryLedgerRequest {
    pub ledger_id: String,
}

/// 某账本某天。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DiaryDateRequest {
    pub date: String,
    pub ledger_id: String,
}

/// 扫描目录：纯文件系统操作，**没有** `ledger_id`（保持原契约）。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DiaryScanRequest {
    pub directory: String,
}

/// 导入单个文件到指定账本。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DiaryImportFileRequest {
    pub path: String,
    pub date: String,
    pub ledger_id: String,
}

// ================================================================ 关键事件

/// 某年（事件按账本隔离）。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct YearRequest {
    pub year: String,
    pub ledger_id: String,
}

/// 某账本某天的事件。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct KeyEventDateRequest {
    pub date: String,
    pub ledger_id: String,
}

/// 写入事件（有则更新、无则插入）：`title` / `content` / `color` 可省略。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct KeyEventUpsertRequest {
    pub ledger_id: String,
    pub date: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

/// 上传一张图片（base64 data URI，可省略）。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct KeyEventImageAddRequest {
    pub date: String,
    pub ledger_id: String,
    #[serde(default)]
    pub data: Option<String>,
}

/// 删除一张图片。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct KeyEventImageIdRequest {
    pub id: String,
}

// ================================================================ 股票
// 请求体一律 snake_case（`ledger_id` / `stock_code`），响应是 camelCase（DTO 里已 rename）。

/// 追加本金 / 支取 / 利息归本：金额是**分**，日期可省略（空串表示今天）。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StockAmountDateRequest {
    pub ledger_id: String,
    pub amount: Option<i64>,
    pub date: String,
}

/// 保存费用设置。`commission_rate` 必填（缺省即 0 → 触发"必须大于 0"的错误）；
/// 其余三项缺省为 0。`min_commission` 的单位是**分**。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StockFeeSettingsRequest {
    pub ledger_id: String,
    /// 佣金费率（小数：万 2.354 → 0.0002354）
    pub commission_rate: Option<f64>,
    /// 最低佣金（**分**）
    pub min_commission: Option<f64>,
    pub stamp_duty_rate: Option<f64>,
    pub transfer_fee_rate: Option<f64>,
}

/// 保存可用交易标签（「分析」不可删除）。线名是 `ledger_id` / `tags`，
/// 同时接受驼峰 `ledgerId`，语义完全相同。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTagSettingsRequest {
    #[serde(alias = "ledgerId")]
    pub ledger_id: String,
    pub tags: Vec<String>,
}

/// 数值参数：界面可能传数字字符串（`"2"`）也可能直接传数字（`2`），两种形态都接受。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum QueryNumber {
    Text(String),
    Integer(i64),
    Float(f64),
}

impl From<i64> for QueryNumber {
    fn from(value: i64) -> Self {
        QueryNumber::Integer(value)
    }
}

impl From<String> for QueryNumber {
    fn from(value: String) -> Self {
        QueryNumber::Text(value)
    }
}

/// 资金记录分页：`page` / `page_size` 同时接受数字与数字字符串
/// （`alias` 覆盖 `pageSize` 这种前端驼峰写法）；非法或缺失时回退默认值。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StockFundRecordsRequest {
    pub ledger_id: String,
    pub page: Option<QueryNumber>,
    #[serde(alias = "pageSize")]
    pub page_size: Option<QueryNumber>,
}

/// 保存持仓中的「本轮复盘」（500 字以内）。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StockPositionReviewRequest {
    pub ledger_id: String,
    /// 股票代码
    pub code: String,
    pub review: String,
}

/// 某股交易列表 / 历史详情。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTradesRequest {
    pub ledger_id: String,
    pub stock_code: String,
}

/// 一笔委托内的一笔成交明细（价格单位：**元**，后端负责 ×100）。
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct TradeFillRequest {
    pub price: f64,
    pub lots: f64,
}

impl TradeFillRequest {
    /// 界面的手数是整数。
    pub fn new(price: f64, lots: i64) -> Self {
        Self {
            price,
            lots: lots as f64,
        }
    }
}

/// 一笔委托（可含多笔成交明细），返回成交明细数组。
///
/// `fills` 为空时回退到单笔 `price` / `lots`（兼容旧调用）。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTradeCreateRequest {
    pub ledger_id: String,
    pub stock_code: String,
    pub stock_name: String,
    pub trade_type: String,
    pub trade_time: f64,
    pub remark: String,
    pub tag: String,
    pub fills: Vec<TradeFillRequest>,
    /// 兼容旧调用的单笔价格（**元**）
    pub price: f64,
    pub lots: f64,
}

/// 编辑一笔成交（按当前费用设置重算整笔委托）。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTradeUpdateRequest {
    pub ledger_id: String,
    /// 成交记录 ID
    pub id: String,
    /// 成交价（**元**）
    pub price: f64,
    pub lots: f64,
    pub trade_time: f64,
}

/// 删除整笔委托。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTradeOrderDeleteRequest {
    pub ledger_id: String,
    #[serde(alias = "order_id")]
    pub order_id: String,
}

/// 预演编辑 / 删除的影响（**不落库**）。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StockTradeImpactRequest {
    pub ledger_id: String,
    /// `update_trade` | `delete_order`
    pub action: String,
    pub trade_id: String,
    pub order_id: String,
    /// 成交价（**元**）
    pub price: f64,
    pub lots: f64,
    pub trade_time: f64,
}

/// 保存某轮次的交易复盘（500 字以内）。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StockRoundReviewRequest {
    pub ledger_id: String,
    /// 轮次 ID
    pub id: String,
    pub review: String,
}

/// 保存某轮次的交易标签。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StockRoundTagRequest {
    pub ledger_id: String,
    /// 轮次 ID
    pub id: String,
    pub tag: String,
}

/// 统计的筛选参数：`start_month` / `end_month` / `recent` / `tag`。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StockStatisticsRequest {
    pub ledger_id: String,
    pub start_month: String,
    pub end_month: String,
    /// 非法值报 `recent 必须为正整数`
    pub recent: Option<QueryNumber>,
    pub tag: String,
}

/// 查询股票名称（优先本地交易记录，未命中走外部行情；**不需要 ledger_id**）。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StockNameRequest {
    pub stock_code: String,
}

// ================================================================ 桌面外壳

/// 自绘标题栏的三个按钮：`minimize` | `maximize` | `close`。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct WindowControlRequest {
    pub action: String,
}

/// `app_info`：`name` / `version` / `isDev`。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AppInfoRequest {
    pub field: String,
}

/// 工作空间资产相对路径 → `trasset://` URL。
///
/// 界面发 camelCase `filePath`；只认 snake_case 曾导致过"关键事件图片全都显示不出来"
/// 的静默失效，因此这里保留别名。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AssetUrlRequest {
    #[serde(rename = "filePath", alias = "file_path")]
    pub file_path: String,
}

/// 关闭行为：`quit` | `tray` | 空串（首次询问）。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SetCloseBehaviorRequest {
    pub behavior: String,
}

/// 外观：`light` | `dark` | `system`。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SetAppearanceRequest {
    pub appearance: String,
}

/// 功能开关的写入请求。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SetFeatureRequest {
    /// 功能名：`accounting` / `stock` / `keyEvent` / `diary`。
    ///
    /// 是**硬契约**（界面 `FeatureFlags` 与 `shell::Page::feature_key` 两边都按它写），
    /// 未知名一律拒绝而不是静默忽略 —— 静默忽略会表现成"开关点了没反应"。
    pub feature: String,
    pub enabled: bool,
}

/// 事件页右栏（关联交易）是否展开。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SetKeyEventLinkedOpenRequest {
    pub open: bool,
}

/// 功能开关：哪些顶级功能在侧边栏出现。
///
/// **默认全开**：老配置里没有 `features` 这个键、或外壳漏发该字段时，
/// 四个功能照常显示（缺少个别字段同理）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FeatureFlags {
    /// 记账（记录 / 分析 / 标签 / 模板）
    pub accounting: bool,
    /// 股票
    pub stock: bool,
    /// 事件
    #[serde(rename = "keyEvent")]
    pub key_event: bool,
    /// 日记
    pub diary: bool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self::defaults()
    }
}

impl FeatureFlags {
    /// 默认全开（新增功能时也按"开"处理）。
    pub const fn defaults() -> Self {
        Self {
            accounting: true,
            stock: true,
            key_event: true,
            diary: true,
        }
    }

    /// 按开关名读取；未知名返回 `None`（调用方据此拒绝请求，不做猜测）。
    pub fn get(&self, feature: &str) -> Option<bool> {
        Some(match feature {
            "accounting" => self.accounting,
            "stock" => self.stock,
            "keyEvent" => self.key_event,
            "diary" => self.diary,
            _ => return None,
        })
    }

    /// 按开关名写入；未知名**不写入**并返回 `false`。
    pub fn set(&mut self, feature: &str, enabled: bool) -> bool {
        match feature {
            "accounting" => self.accounting = enabled,
            "stock" => self.stock = enabled,
            "keyEvent" => self.key_event = enabled,
            "diary" => self.diary = enabled,
            _ => return false,
        }
        true
    }
}

/// `config_get` 的返回。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigSnapshot {
    #[serde(rename = "workspaceDir")]
    pub workspace_dir: String,
    #[serde(rename = "closeBehavior")]
    pub close_behavior: String,
    /// light / dark / system
    pub appearance: String,
    #[serde(rename = "configPath")]
    pub config_path: String,
    #[serde(rename = "isDev")]
    pub is_dev: bool,
    /// 代理设置（`mode` / `url`）；缺省 = 自动探测。
    pub proxy: ProxySetting,
    /// 功能开关（缺省 = 全开，见 [`FeatureFlags`]）。
    pub features: FeatureFlags,
    /// 事件页右栏（关联交易）是否展开（**缺省 = 展开**）。
    #[serde(rename = "keyEventLinkedOpen", default = "default_true")]
    pub key_event_linked_open: bool,
}

/// `bool` 字段的 serde 缺省值：**true**。
///
/// 容器上的 `#[serde(default)]` 会用 `bool::default()`（= false），
/// 对"缺省应当是开"的偏好是错的 —— 漏发字段时界面会静默变成"收起"。
pub fn default_true() -> bool {
    true
}

/// `proxy_detect` 的返回：探测到的地址 + 来源 + 一句给用户看的说明。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProxyDetectResponse {
    /// 会用到的代理地址（空串 = 直连）
    pub url: String,
    /// env / system / manual / none
    pub source: String,
    /// 系统是否配置了 PAC 自动配置脚本（本版本不解析）
    pub pac: bool,
    pub message: String,
}

/// `workspace_set` / `workspace_open` 的入参。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct WorkspaceDirRequest {
    #[serde(rename = "workspaceDir")]
    pub workspace_dir: String,
}

/// `dialog_open` 的入参：标题与初始目录都可省略。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DialogOpenRequest {
    pub title: Option<String>,
    /// 界面发的是 camelCase `defaultPath`。
    #[serde(default, rename = "defaultPath", alias = "default_path")]
    pub default_path: Option<String>,
}

/// `dialog_open` 的返回（固定契约）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DialogOpenResponse {
    pub canceled: bool,
    #[serde(rename = "filePaths")]
    pub file_paths: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl DialogOpenResponse {
    /// 取用户选中的第一个目录（取消或异常时为空）。
    pub fn first_path(&self) -> Option<&str> {
        self.file_paths.first().map(String::as_str)
    }
}

/// `file_save_image` 的入参。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FileSaveRequest {
    #[serde(rename = "relativePath")]
    pub relative_path: String,
}

/// `file_save_image` 的返回。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileSaveResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canceled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 开合 DevTools。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DevToolsToggleRequest {
    pub enabled: bool,
}

// ================================================================ 自动更新

/// `update_check` 的返回。
///
/// **不会 reject**：网络失败时它 resolve 出 `error` 字段，界面必须把
/// 「`hasUpdate == false` + 有 `error`」当成"检查失败"而不是"已是最新"。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateCheckResponse {
    #[serde(rename = "hasUpdate")]
    pub has_update: bool,
    #[serde(rename = "latestVersion")]
    pub latest_version: String,
    #[serde(rename = "downloadUrl")]
    pub download_url: String,
    /// 形如 `sha256:...`（GitHub release asset 的 digest）
    pub digest: String,
    /// release notes（Markdown；按纯文本渲染）
    pub body: String,
    /// 检查失败原因（网络 / 解析问题），成功时为 `None`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `update_download` / `update_install` 的返回：**不会 reject**，
/// 失败时 resolve 出 `{ success: false, error }`。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl UpdateResponse {
    pub fn ok() -> Self {
        Self {
            success: true,
            error: None,
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            success: false,
            error: Some(message.into()),
        }
    }

    /// 是否为"用户主动取消"（后端用固定文案 `cancelled` 表示）。
    pub fn is_cancelled(&self) -> bool {
        self.error.as_deref() == Some("cancelled")
    }
}

/// `update_download` 的入参。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct UpdateDownloadRequest {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

/// `update_download_status` 的返回：界面（重新）进入「关于软件」时恢复下载状态。
///
/// 下载是**单例**（一次只有一笔，跑在外壳的线程里），界面进来时先问一次当前状态，
/// 这样切换页面回来、甚至重开界面，都能接着显示进度而不是回到"未下载"。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateDownloadStatus {
    /// 是否有下载正在跑
    pub active: bool,
    /// 已经下载好、正在等待安装（`%TEMP%` 里那份文件还在）
    pub downloaded: bool,
    pub percent: u32,
    pub speed: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(value: &impl Serialize) -> String {
        serde_json::to_string(value).unwrap()
    }

    /// 界面发出去的字段名与"缺省字段"就是契约：改名或改默认等于静默失效。
    #[test]
    fn requests_keep_the_documented_wire_shape() {
        assert_eq!(
            json(&ChartIdRequest {
                chart_id: "c1".into()
            }),
            r#"{"chartId":"c1"}"#
        );
        assert_eq!(
            json(&CategoryListRequest {
                transaction_type: "expense".into(),
                ledger_id: "l1".into()
            }),
            r#"{"type":"expense","ledgerId":"l1"}"#
        );
        assert_eq!(
            json(&TemplateSortRequest {
                id: "t1".into(),
                ledger_id: "l1".into(),
                sort_order: 2
            }),
            r#"{"id":"t1","ledgerId":"l1","sortOrder":2}"#
        );
        assert_eq!(
            json(&LedgerIdRequest {
                ledger_id: "l1".into()
            }),
            r#"{"ledger_id":"l1"}"#
        );
        assert_eq!(
            json(&KeyEventUpsertRequest {
                ledger_id: "l1".into(),
                date: "2026-02-10".into(),
                title: Some("标题".into()),
                content: Some("正文".into()),
                color: Some("outlier".into()),
            }),
            r#"{"ledger_id":"l1","date":"2026-02-10","title":"标题","content":"正文","color":"outlier"}"#
        );
        // 金额：`Some(12345)` 就是 `12345`，不能变成字符串或 `null`
        assert_eq!(
            json(&StockAmountDateRequest {
                ledger_id: "l1".into(),
                amount: Some(12_345),
                date: String::new(),
            }),
            r#"{"ledger_id":"l1","amount":12345,"date":""}"#
        );
        // `QueryNumber` 是 untagged：`Integer` 必须发成**裸数字**（发成 `{"Integer":2}` 后端不认），
        // 字符串形态则原样发字符串
        assert_eq!(
            json(&StockFundRecordsRequest {
                ledger_id: "l1".into(),
                page: Some(QueryNumber::Integer(2)),
                page_size: Some(QueryNumber::Text("50".into())),
            }),
            r#"{"ledger_id":"l1","page":2,"page_size":"50"}"#
        );
        assert_eq!(
            json(&StockStatisticsRequest {
                ledger_id: "l1".into(),
                start_month: "2026-01".into(),
                end_month: "2026-03".into(),
                recent: Some(20_i64.into()),
                tag: "分析".into(),
            }),
            r#"{"ledger_id":"l1","start_month":"2026-01","end_month":"2026-03","recent":20,"tag":"分析"}"#
        );
        // 成交明细与旧调用：`fills` 与单笔 `price`/`lots` 同时发（与手抄副本逐字一致）
        assert_eq!(
            json(&StockTradeCreateRequest {
                ledger_id: "l1".into(),
                stock_code: "600519".into(),
                stock_name: "贵州茅台".into(),
                trade_type: "open".into(),
                trade_time: 1_767_657_600.0,
                remark: String::new(),
                tag: "分析".into(),
                fills: vec![TradeFillRequest::new(1500.0, 1)],
                ..Default::default()
            }),
            r#"{"ledger_id":"l1","stock_code":"600519","stock_name":"贵州茅台","trade_type":"open","trade_time":1767657600.0,"remark":"","tag":"分析","fills":[{"price":1500.0,"lots":1.0}],"price":0.0,"lots":0.0}"#
        );
    }

    /// 反序列化一侧：别名、数字/字符串、缺省字段的口径（旧调用方仍能解析）。
    #[test]
    fn requests_deserialize_aliases_and_missing_fields() {
        let by_alias: CategoryListRequest =
            serde_json::from_str(r#"{"transactionType":"income","ledgerId":"l1"}"#).unwrap();
        assert_eq!(by_alias.transaction_type, "income");

        let by_id: ChartIdRequest = serde_json::from_str(r#"{"id":"c1"}"#).unwrap();
        assert_eq!(by_id.chart_id, "c1");

        let page: StockFundRecordsRequest =
            serde_json::from_str(r#"{"ledger_id":"l1","page":"2","pageSize":10}"#).unwrap();
        assert!(matches!(page.page, Some(QueryNumber::Text(_))));
        assert!(matches!(page.page_size, Some(QueryNumber::Integer(10))));

        let upsert: KeyEventUpsertRequest =
            serde_json::from_str(r#"{"ledger_id":"l1","date":"2026-02-10"}"#).unwrap();
        assert!(upsert.title.is_none() && upsert.color.is_none());

        // 响应侧：缺字段走默认（外壳漏发时界面不能炸）
        let snapshot: ConfigSnapshot = serde_json::from_str("{}").unwrap();
        assert!(snapshot.key_event_linked_open);
        assert_eq!(snapshot.features, FeatureFlags::defaults());
    }
}
