//! 黄金对比的**归一化与差异报告**核心。
//!
//! 用途：把「Go 参考实现的响应/落库数据」与「Rust 实现的对应输出」放在同一把尺子上比较。
//! 两侧必然不同的只有**易变字段**（UUID、Unix 秒时间戳、并发下的并列排序），
//! 因此归一化的做法是**按显式白名单把易变字段替换成占位符**，而不是"忽略差异"——
//! 名单之外任何字段、任何层级不一致都必须报出来（含路径），避免用"模糊比较"掩盖真实偏差。
//!
//! 提供两个可直接使用的子命令（见 `main.rs`）：
//! * `parity normalize <in.json> --out <out.json>`：把易变字段替换为占位符（便于人工核对）
//! * `parity diff <a.json> <b.json>`：逐路径比较，输出前若干条差异

use serde_json::Value;

/// 易变字段名（全小写比较）。UUID、时间戳、以及"每次运行都不同"的派生值。
pub const VOLATILE_KEYS: &[&str] = &[
    // 主键 / 关联键：两侧各自生成 UUID，值必然不同
    "id",
    "transactionid",
    "transaction_id",
    "ledgerid",
    "ledger_id",
    "chartid",
    "chart_id",
    "templateid",
    "template_id",
    "roundid",
    "round_id",
    "historyid",
    "history_id",
    "orderid",
    "order_id",
    "imageid",
    "conversationid",
    // 时间戳：写入时刻不同（秒级）
    "createdat",
    "created_at",
    "updatedat",
    "updated_at",
    "applied_at",
    "quotetime",
    "quote_time",
    // 派生展示值：依赖当前时间/当日行情
    "latestprice",
    "latest_price",
    "prevclose",
    "prev_close",
    "quotefailedcount",
    "quote_failed_count",
];

/// 占位符：让归一化后的文件仍然可读，且能看出"这里被归一化掉了"。
pub const PLACEHOLDER: &str = "<volatile>";

/// 字段名是否属于易变字段（大小写不敏感）。
pub fn is_volatile_key(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    VOLATILE_KEYS.contains(&lowered.as_str())
}

/// 递归把易变字段替换为占位符；数组按**规范化后的序列化文本**排序，消除并列排序差异。
///
/// 排序只作用于对象数组（例如 `tr_query` 的分页结果）；标量数组（例如标签列表）保持原顺序，
/// 因为顺序本身是业务语义（标签顺序、轮次交易顺序都由字段显式表达）。
pub fn normalize(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, entry) in map.iter_mut() {
                if is_volatile_key(key) {
                    *entry = Value::String(PLACEHOLDER.to_string());
                } else {
                    normalize(entry);
                }
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                normalize(item);
            }
            // 对象数组排序：规范化后文本相同即视为并列，排序让两侧顺序可比
            let all_objects = items.iter().all(Value::is_object);
            if all_objects && items.len() > 1 {
                items.sort_by_key(|item| item.to_string());
            }
        }
        _ => {}
    }
}

/// 一处差异。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    /// JSON 路径，例如 `items[2].price`
    pub path: String,
    pub left: String,
    pub right: String,
}

/// 逐路径比较两个 JSON 值，返回全部差异（按路径顺序）。
pub fn diff(left: &Value, right: &Value) -> Vec<Difference> {
    let mut differences = Vec::new();
    diff_into(left, right, "$", &mut differences);
    differences
}

fn diff_into(left: &Value, right: &Value, path: &str, out: &mut Vec<Difference>) {
    match (left, right) {
        (Value::Object(left_map), Value::Object(right_map)) => {
            // 键集合差异也算差异（多出/缺少字段是真实偏差）
            let mut keys: Vec<&String> = left_map.keys().chain(right_map.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                let child_path = format!("{path}.{key}");
                match (left_map.get(key), right_map.get(key)) {
                    (Some(left_value), Some(right_value)) => {
                        diff_into(left_value, right_value, &child_path, out)
                    }
                    (Some(left_value), None) => out.push(Difference {
                        path: child_path,
                        left: brief(left_value),
                        right: "<missing>".to_string(),
                    }),
                    (None, Some(right_value)) => out.push(Difference {
                        path: child_path,
                        left: "<missing>".to_string(),
                        right: brief(right_value),
                    }),
                    (None, None) => {}
                }
            }
        }
        (Value::Array(left_items), Value::Array(right_items)) => {
            if left_items.len() != right_items.len() {
                out.push(Difference {
                    path: format!("{path}.length"),
                    left: left_items.len().to_string(),
                    right: right_items.len().to_string(),
                });
            }
            for (index, (left_item, right_item)) in left_items.iter().zip(right_items).enumerate() {
                diff_into(left_item, right_item, &format!("{path}[{index}]"), out);
            }
        }
        _ => {
            if left != right {
                out.push(Difference {
                    path: path.to_string(),
                    left: brief(left),
                    right: brief(right),
                });
            }
        }
    }
}

/// 单值摘要（长字符串截断，便于在终端里读差异）。
fn brief(value: &Value) -> String {
    let text = value.to_string();
    if text.chars().count() > 120 {
        let truncated: String = text.chars().take(120).collect();
        format!("{truncated}…")
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn volatile_keys_are_masked_at_any_depth() {
        let mut value = json!({
            "id": "a",
            "items": [{"transactionId": "t1", "price": 100, "keyEventDate": "2026-01-01"}],
            "nested": {"createdAt": 123, "category": "餐饮美食"}
        });
        normalize(&mut value);

        assert_eq!(value["id"], PLACEHOLDER);
        assert_eq!(value["items"][0]["transactionId"], PLACEHOLDER);
        assert_eq!(value["nested"]["createdAt"], PLACEHOLDER);
        // 非易变字段原样保留
        assert_eq!(value["items"][0]["price"], 100);
        assert_eq!(value["items"][0]["keyEventDate"], "2026-01-01");
        assert_eq!(value["nested"]["category"], "餐饮美食");
    }

    #[test]
    fn object_arrays_are_sorted_but_scalar_arrays_are_not() {
        // 对象数组：顺序无关（并列排序差异由此消除）
        let mut left = json!([{"name": "b", "id": "1"}, {"name": "a", "id": "2"}]);
        let mut right = json!([{"name": "a", "id": "2"}, {"name": "b", "id": "1"}]);
        normalize(&mut left);
        normalize(&mut right);
        assert_eq!(left, right);
        assert!(diff(&left, &right).is_empty());

        // 标量数组（标签）顺序是业务语义，不得排序
        let mut left = json!(["三餐", "外卖"]);
        let mut right = json!(["外卖", "三餐"]);
        normalize(&mut left);
        normalize(&mut right);
        assert_ne!(left, right);
        assert_eq!(diff(&left, &right).len(), 2);
    }

    #[test]
    fn diff_reports_paths_and_missing_fields() {
        let left = json!({"total": 3, "items": [{"price": 100}], "extra": 1});
        let right = json!({"total": 4, "items": [{"price": 100}], "other": 2});
        let differences = diff(&left, &right);

        let paths: Vec<&str> = differences.iter().map(|d| d.path.as_str()).collect();
        assert!(paths.contains(&"$.total"));
        assert!(paths.contains(&"$.extra"));
        assert!(paths.contains(&"$.other"));
        assert_eq!(differences.len(), 3, "{differences:?}");
    }

    #[test]
    fn diff_detects_length_mismatch_and_index_differences() {
        let left = json!({"items": [1, 2, 3]});
        let right = json!({"items": [1, 9]});
        let differences = diff(&left, &right);
        let paths: Vec<&str> = differences.iter().map(|d| d.path.as_str()).collect();
        assert!(paths.contains(&"$.items.length"), "{paths:?}");
        assert!(paths.contains(&"$.items[1]"), "{paths:?}");
    }

    #[test]
    fn identical_values_have_no_differences() {
        let value = json!({"a": 1, "b": [1, 2], "c": {"d": "x"}});
        assert!(diff(&value, &value).is_empty());
    }
}
