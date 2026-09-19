//! `trasset://` 自定义协议：把工作空间的 `data/assets` 只读暴露给界面。
//!
//! 界面没有本地 HTTP 服务可用，因此由自定义协议承担读取图片的职责
//! （`<img src>` 可以直接使用）：
//!
//! * Windows / WebView2：`http://trasset.localhost/<相对路径>`
//! * 其它平台：`trasset://localhost/<相对路径>`
//!
//! 安全校验：规范化路径、拒绝 `..`、解析符号链接后必须仍位于 `data/assets` 之内
//! （额外做真实路径解析，比只比较字符串更严格）。

use std::borrow::Cow;
use std::path::Path;
use std::sync::Arc;

use tauri::http::{header, Request, Response, StatusCode};
use tr_store::Workspace;

/// 自定义协议名。
pub const SCHEME: &str = "trasset";

/// 构造界面可直接放进 `<img src>` 的 URL。
pub fn asset_url(relative_path: &str) -> String {
    let encoded = percent_encode_path(relative_path);
    if cfg!(windows) {
        format!("http://{SCHEME}.localhost/{encoded}")
    } else {
        format!("{SCHEME}://localhost/{encoded}")
    }
}

/// 处理一次资产请求。`workspace` 为 `None` 表示尚未打开工作空间。
pub fn handle(
    request: Request<Vec<u8>>,
    workspace: Option<Arc<Workspace>>,
) -> Response<Cow<'static, [u8]>> {
    let Some(workspace) = workspace else {
        return plain(StatusCode::INTERNAL_SERVER_ERROR, "workspace not opened");
    };

    let relative = percent_decode(request.uri().path().trim_start_matches('/'));
    if relative.is_empty() || relative.contains("..") {
        return plain(StatusCode::FORBIDDEN, "invalid file path");
    }

    let assets_root = workspace.assets_directory();
    let Ok(root) = assets_root.canonicalize() else {
        // 工作空间里还没有 assets 目录：视为文件不存在
        return plain(StatusCode::NOT_FOUND, "file not found");
    };

    let Ok(resolved) = assets_root.join(&relative).canonicalize() else {
        return plain(StatusCode::NOT_FOUND, "file not found");
    };
    if !resolved.starts_with(&root) {
        return plain(StatusCode::FORBIDDEN, "invalid file path");
    }

    match std::fs::read(&resolved) {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type_of(&resolved))
            .body(Cow::Owned(bytes))
            .unwrap_or_else(|_| plain(StatusCode::INTERNAL_SERVER_ERROR, "response failed")),
        Err(_) => plain(StatusCode::NOT_FOUND, "file not found"),
    }
}

fn plain(status: StatusCode, message: &'static str) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Cow::Borrowed(message.as_bytes()))
        .expect("静态响应构造不会失败")
}

fn content_type_of(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// 路径用的百分号解码（`+` 不视为空格，符合路径语义）。
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                out.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 路径用的百分号编码（保留 `/` 与 URL 安全字符，其余按 UTF-8 字节转义）。
fn percent_encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encoding_roundtrips_non_ascii_and_spaces() {
        let path = "key_events/2026-01-01/图片 1.jpg";
        let encoded = percent_encode_path(path);
        assert_eq!(encoded, "key_events/2026-01-01/%E5%9B%BE%E7%89%87%201.jpg");
        assert_eq!(percent_decode(&encoded), path);
    }

    #[test]
    fn percent_decode_keeps_plus_as_plus() {
        assert_eq!(percent_decode("a+b"), "a+b");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn content_types_follow_extension() {
        assert_eq!(content_type_of(Path::new("a.JPG")), "image/jpeg");
        assert_eq!(content_type_of(Path::new("a.png")), "image/png");
        assert_eq!(content_type_of(Path::new("a.webp")), "image/webp");
        assert_eq!(
            content_type_of(Path::new("a.txt")),
            "application/octet-stream"
        );
    }

    #[test]
    fn asset_url_uses_platform_specific_form() {
        let url = asset_url("key_events/2026-01-01/a.jpg");
        if cfg!(windows) {
            assert_eq!(url, "http://trasset.localhost/key_events/2026-01-01/a.jpg");
        } else {
            assert_eq!(url, "trasset://localhost/key_events/2026-01-01/a.jpg");
        }
    }

    #[test]
    fn handle_without_workspace_is_500() {
        let request = Request::builder().uri("/x.jpg").body(Vec::new()).unwrap();
        let response = handle(request, None);
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn handle_rejects_path_traversal() {
        let dir = std::env::temp_dir().join(format!("tr-asset-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let workspace = Arc::new(Workspace::open(&dir).unwrap());

        let request = Request::builder()
            .uri("/../../secrets.txt")
            .body(Vec::new())
            .unwrap();
        let response = handle(request, Some(workspace.clone()));
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 百分号编码的穿越：解码**必须**发生在 `..` 检查之前。
    /// 如果哪天有人把顺序调成"先检查再解码"，`%2e%2e%2f` 就会绕过第一道闸门，
    /// 这条测试会立刻变红（第二道 canonicalize + starts_with 只是兜底）。
    #[test]
    fn handle_rejects_percent_encoded_traversal() {
        let dir = std::env::temp_dir().join(format!("tr-asset-enc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let workspace = Arc::new(Workspace::open(&dir).unwrap());

        for uri in [
            "/%2e%2e%2fsecrets.txt",
            "/..%2fsecrets.txt",
            "/%2e%2e/secrets.txt",
        ] {
            let request = Request::builder().uri(uri).body(Vec::new()).unwrap();
            let response = handle(request, Some(workspace.clone()));
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri} 没有被拒绝");
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 绝对路径逃逸：Windows 上 `join` 一个带盘符的绝对路径会**替换**基路径，
    /// 因此必须靠 canonicalize 之后的 `starts_with(root)` 拦住。
    #[test]
    fn handle_rejects_absolute_path_escape() {
        let dir = std::env::temp_dir().join(format!("tr-asset-abs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let workspace = Arc::new(Workspace::open(&dir).unwrap());

        let request = Request::builder()
            .uri("/C:/Windows/win.ini")
            .body(Vec::new())
            .unwrap();
        let response = handle(request, Some(workspace));
        assert_ne!(
            response.status(),
            StatusCode::OK,
            "绝对路径不得被当作工作空间内的资产读出"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn handle_serves_file_inside_assets() {
        let dir = std::env::temp_dir().join(format!("tr-asset-serve-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let workspace = Arc::new(Workspace::open(&dir).unwrap());
        let target_dir = dir
            .join("data")
            .join("assets")
            .join("key_events")
            .join("2026-01-01");
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::write(target_dir.join("a.jpg"), b"jpeg-bytes").unwrap();

        let request = Request::builder()
            .uri("/key_events/2026-01-01/a.jpg")
            .body(Vec::new())
            .unwrap();
        let response = handle(request, Some(workspace));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body().as_ref(), b"jpeg-bytes");

        std::fs::remove_dir_all(&dir).ok();
    }
}
