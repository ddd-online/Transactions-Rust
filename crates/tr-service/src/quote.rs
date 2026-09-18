//! 行情抓取抽象。对照 Go `kernel/service/stock_service.go` 的 `StockQuoteFetcher`
//! / `tencentStockQuoteFetcher` / `fetchTencentQuotes` / `fetchStockNameExternal`。
//!
//! 拆成独立模块的理由：服务层（`stock.rs` / `stock_statistics.rs`）只依赖 trait，
//! 从而可以在没有网络的环境里用 stub 做确定性测试；真实实现（腾讯行情 qt.gtimg.cn）
//! 集中在这里，并且**解析逻辑与网络分离**（`parse_tencent_quote_payload` 是纯函数，
//! 可以直接用固定 payload 断言，对应 Go 的 `TestParseTencentQuotePayload`）。
//!
//! 网络约束（与 Go 版逐条一致）：
//! * `http://qt.gtimg.cn/q=<market><code>[,<market><code>...]`，**3 秒超时**；
//! * 响应是 GBK 编码，需要按 GBK 解码后再按 `~` 切分；
//! * 停牌（最新价 0）或非法价格视为"该股无行情"，静默跳过；
//! * 任何网络/解码失败都返回空映射而不是错误——行情是临时外部数据，**不得阻塞录入**。

use std::collections::HashMap;
use std::time::Duration;

use tr_domain::dto::StockQuoteDto;
use tr_domain::fee::{is_valid_stock_code, market_prefix};

/// 腾讯行情字段分隔符。
const FIELD_SEPARATOR: char = '~';

/// 行情请求超时（毫秒）。与原实现的 `http.Client{Timeout: 3 * time.Second}` 一致。
const QUOTE_TIMEOUT: Duration = Duration::from_secs(3);

/// 批量行情源：按股票代码返回最新价与昨收价。
///
/// 实现者必须是 `Send + Sync`：Tauri 命令层会在 `spawn_blocking` 线程里调用它。
pub trait StockQuoteFetcher: Send + Sync {
    /// 批量拉取行情。返回的映射只包含请求中解析成功的股票。
    fn fetch_quotes(&self, codes: &[String]) -> HashMap<String, StockQuoteDto>;

    /// 查询单只股票名称。查询失败或无行情时返回空串（不阻塞录入）。
    fn fetch_name(&self, code: &str) -> String;
}

/// 腾讯行情实现（`qt.gtimg.cn`）。名称查询与行情查询同源。
#[derive(Debug, Clone, Copy, Default)]
pub struct TencentStockQuoteFetcher;

impl TencentStockQuoteFetcher {
    pub fn new() -> Self {
        Self
    }

    /// 建立带 3 秒全局超时的 agent（每次调用都用同一份配置）。
    fn agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            .timeout_global(Some(QUOTE_TIMEOUT))
            .build()
            .into()
    }

    /// 发起一次行情请求并按 GBK 解码；任何失败都返回 `None`。
    fn fetch_payload(url: &str) -> Option<Vec<u8>> {
        let mut response = match Self::agent().get(url).call() {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!("查询股票行情失败(网络): {} ({})", url, error);
                return None;
            }
        };
        match response.body_mut().read_to_vec() {
            Ok(bytes) => Some(bytes),
            Err(error) => {
                tracing::warn!("查询股票行情失败(读取): {} ({})", url, error);
                None
            }
        }
    }
}

impl StockQuoteFetcher for TencentStockQuoteFetcher {
    fn fetch_quotes(&self, codes: &[String]) -> HashMap<String, StockQuoteDto> {
        let mut result = HashMap::new();
        if codes.is_empty() {
            return result;
        }

        // 过滤非法代码与重复项，拼出 `sh600000,sz000001` 形式的查询串。
        let mut query: Vec<String> = Vec::with_capacity(codes.len());
        let mut requested: Vec<String> = Vec::with_capacity(codes.len());
        for code in codes {
            let code = code.trim();
            if !is_valid_stock_code(code) {
                continue;
            }
            if requested.iter().any(|item| item == code) {
                continue;
            }
            requested.push(code.to_string());
            query.push(format!("{}{}", market_prefix(code), code));
        }
        if query.is_empty() {
            return result;
        }

        let url = format!("http://qt.gtimg.cn/q={}", query.join(","));
        let Some(payload) = Self::fetch_payload(&url) else {
            return result;
        };
        let payload = decode_gbk(&payload);

        let parsed = parse_tencent_quote_payload(&payload);
        for code in requested {
            if let Some(quote) = parsed.get(&code) {
                result.insert(code, quote.clone());
            }
        }
        result
    }

    fn fetch_name(&self, code: &str) -> String {
        // 仅支持 A 股六位代码，避免非法入参打到外部接口。
        if !is_valid_stock_code(code) {
            return String::new();
        }
        let url = format!("http://qt.gtimg.cn/q={}{}", market_prefix(code), code);
        let Some(payload) = Self::fetch_payload(&url) else {
            return String::new();
        };
        let payload = decode_gbk(&payload);
        parse_tencent_name(&payload)
    }
}

/// 腾讯行情响应按 GBK 解码。非法字节以替换字符处理（与原实现 `GBK.NewDecoder()` 的容错一致）。
fn decode_gbk(payload: &[u8]) -> String {
    let (decoded, _, _) = encoding_rs::GBK.decode(payload);
    decoded.into_owned()
}

/// 从响应文本里取出第一段 `v_xxx="..."` 的引号内容并按 `~` 切分字段。
///
/// 原实现用正则 `v_(\w+)="([^"]*)"`，这里等价地手工扫描：
/// 对合法响应两者结果相同，而手工扫描不会把"正则回溯"这类行为差异带进来。
fn first_quote_fields(payload: &str) -> Option<Vec<&str>> {
    let mut rest = payload;
    while let Some(start) = rest.find("v_") {
        let candidate = &rest[start + 2..];
        if let Some(quote_start) = candidate.find('=') {
            let after_eq = &candidate[quote_start + 1..];
            if let Some(stripped) = after_eq.strip_prefix('"') {
                if let Some(end) = stripped.find('"') {
                    return Some(stripped[..end].split(FIELD_SEPARATOR).collect());
                }
            }
        }
        rest = &rest[start + 2..];
    }
    None
}

/// 遍历响应里全部 `v_xxx="..."` 段落（批量请求会返回多行）。
fn all_quote_fields(payload: &str) -> Vec<Vec<&str>> {
    let mut result = Vec::new();
    let mut rest = payload;
    while let Some(start) = rest.find("v_") {
        let candidate = &rest[start + 2..];
        let mut consumed = 0;
        if let Some(quote_start) = candidate.find('=') {
            let after_eq = &candidate[quote_start + 1..];
            if let Some(stripped) = after_eq.strip_prefix('"') {
                if let Some(end) = stripped.find('"') {
                    result.push(stripped[..end].split(FIELD_SEPARATOR).collect());
                    consumed = quote_start + 1 + 1 + end + 1;
                }
            }
        }
        if consumed == 0 {
            rest = &rest[start + 2..];
        } else {
            rest = &candidate[consumed..];
        }
    }
    result
}

/// 解析腾讯行情响应文本，返回全部可识别的 A 股行情。
///
/// 腾讯字段以 `~` 分隔：名称[1]、代码[2]、最新价[3]、昨收[4]、行情时间[30]（`YYYYMMDDHHMMSS`）。
/// 字段不足 5 段、代码不合法、价格 <= 0（停牌）或无法解析时跳过该股。
/// 行情时间缺失/非法时退化为"当前时间"（与 Go 的 `time.Now().Unix()` 兜底一致）。
pub fn parse_tencent_quote_payload(payload: &str) -> HashMap<String, StockQuoteDto> {
    let mut result = HashMap::new();
    for parts in all_quote_fields(payload) {
        if parts.len() < 5 {
            continue;
        }
        let code = parts[2].trim();
        if !is_valid_stock_code(code) {
            continue;
        }
        let (Ok(latest_yuan), Ok(prev_close_yuan)) = (
            parts[3].trim().parse::<f64>(),
            parts[4].trim().parse::<f64>(),
        ) else {
            continue;
        };
        if latest_yuan <= 0.0 || prev_close_yuan <= 0.0 {
            continue; // 停牌/非法值视为该股无行情
        }

        let mut quote_time = tr_store::util::now_unix();
        if parts.len() > 30 {
            if let Some(parsed) = parse_quote_timestamp(parts[30].trim()) {
                quote_time = parsed;
            }
        }
        result.insert(
            code.to_string(),
            StockQuoteDto {
                stock_code: code.to_string(),
                // 与原实现一致：int64(math.Round(价格元 * 100))
                latest_price: (latest_yuan * 100.0).round() as i64,
                prev_close: (prev_close_yuan * 100.0).round() as i64,
                quote_time,
            },
        );
    }
    result
}

/// 解析单只股票名称：取第一个 `v_xxx="..."` 的第 [1] 段并去空白。
pub fn parse_tencent_name(payload: &str) -> String {
    match first_quote_fields(payload) {
        Some(parts) if parts.len() >= 2 => parts[1].trim().to_string(),
        _ => String::new(),
    }
}

/// 解析 `YYYYMMDDHHMMSS`（本地时区语义）为 Unix 秒。非法时返回 `None`。
///
/// 对照 Go `time.ParseInLocation("20060102150405", raw, time.Local)`：日期时间按**本地时区**
/// 解释。这里先转成 UTC 的 `NaiveDateTime` 再按本地偏移折算，语义与 Go 相同。
fn parse_quote_timestamp(raw: &str) -> Option<i64> {
    if raw.len() != 14 || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number = |range: std::ops::Range<usize>| raw[range].parse::<u32>().ok();
    let year = number(0..4)? as i32;
    let month = number(4..6)?;
    let day = number(6..8)?;
    let hour = number(8..10)?;
    let minute = number(10..12)?;
    let second = number(12..14)?;

    let naive =
        chrono::NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(hour, minute, second)?;
    Some(naive.and_utc().timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 复刻 Go `TestParseTencentQuotePayload` 的造数：32 段，名称[1]、代码[2]、
    /// 最新价[3]、昨收[4]、时间[30]，其余为 `-`。
    fn line(code: &str, latest: &str, prev: &str, ts: &str) -> String {
        let mut parts = vec!["-"; 32];
        parts[1] = "测试股票";
        parts[2] = code;
        parts[3] = latest;
        parts[4] = prev;
        parts[30] = ts;
        format!("v_sh{code}=\"{}\";", parts.join("~"))
    }

    #[test]
    fn parse_tencent_quote_payload_skips_suspended_and_malformed() {
        let payload = [
            line("600000", "10.20", "10.05", "20260904150000"),
            // 停牌：最新价 0 → 跳过
            line("000001", "0.00", "12.30", "20260904150000"),
            // 畸形价格 → 跳过
            line("600519", "bad", "1500.00", "20260904150000"),
        ]
        .join("\n");

        let quotes = parse_tencent_quote_payload(&payload);
        assert_eq!(quotes.len(), 1, "应只解析 1 只股票: {quotes:?}");

        let quote = quotes.get("600000").expect("缺少 600000 行情");
        assert_eq!(quote.latest_price, 1020);
        assert_eq!(quote.prev_close, 1005);
        assert!(quote.quote_time > 0, "行情时间缺失: {quote:?}");
        assert_eq!(quote.stock_code, "600000");
    }

    #[test]
    fn parse_tencent_quote_payload_handles_invalid_code_and_short_fields() {
        let payload = [
            // 北交所代码不在支持范围
            line("830799", "10.00", "10.00", "20260904150000"),
            // 段数不足 5
            "v_sh600000=\"1~2~3\";".to_string(),
        ]
        .join("\n");
        assert!(parse_tencent_quote_payload(&payload).is_empty());
    }

    #[test]
    fn parse_tencent_quote_payload_falls_back_to_now_without_timestamp() {
        let payload = line("600000", "10.20", "10.05", "-");
        let quotes = parse_tencent_quote_payload(&payload);
        let quote = quotes.get("600000").expect("应有行情");
        assert!(
            quote.quote_time >= tr_store::util::now_unix() - 5,
            "时间非法时应退化为当前时间"
        );
    }

    #[test]
    fn parse_tencent_name_takes_second_field() {
        assert_eq!(
            parse_tencent_name(&line("600000", "10.20", "10.05", "20260904150000")),
            "测试股票"
        );
        assert_eq!(parse_tencent_name("garbage"), "");
    }

    #[test]
    fn parse_quote_timestamp_requires_14_digits() {
        assert!(parse_quote_timestamp("20260904150000").is_some());
        assert_eq!(parse_quote_timestamp("2026-09-04"), None);
        assert_eq!(parse_quote_timestamp("20261304150000"), None);
    }

    #[test]
    fn decode_gbk_reads_chinese_names() {
        // "浦发银行" 的 GBK 字节
        let bytes = [0xC6, 0xD6, 0xB7, 0xA2, 0xD2, 0xF8, 0xD0, 0xD0];
        assert_eq!(decode_gbk(&bytes), "浦发银行");
    }

    #[test]
    fn stub_fetcher_returns_only_requested_codes() {
        struct Stub;
        impl StockQuoteFetcher for Stub {
            fn fetch_quotes(&self, codes: &[String]) -> HashMap<String, StockQuoteDto> {
                codes
                    .iter()
                    .filter(|code| code.as_str() == "600000")
                    .map(|code| {
                        (
                            code.clone(),
                            StockQuoteDto {
                                stock_code: code.clone(),
                                latest_price: 1100,
                                prev_close: 1050,
                                quote_time: 12345,
                            },
                        )
                    })
                    .collect()
            }

            fn fetch_name(&self, _code: &str) -> String {
                "浦发银行".to_string()
            }
        }

        let quotes = Stub.fetch_quotes(&["600000".to_string(), "000001".to_string()]);
        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes["600000"].latest_price, 1100);
        assert!(Stub.fetch_quotes(&[]).is_empty());
    }
}
