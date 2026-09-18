//! 工作空间资产：`<workspace>/data/assets/**`。
//!
//! 目录布局与文件命名是**数据兼容的一部分**（用户已有的图片按此布局存放）：
//! ```text
//! <workspace>/data/assets/key_events/<YYYY-MM-DD>/<uuid>.<ext>      原图
//! <workspace>/data/assets/key_events/<YYYY-MM-DD>/thumb_<uuid>.jpg  缩略图
//! ```
//! 数据库里存的是**相对 `data/assets` 的路径**（用 `/` 分隔）。
//!
//! 本文件先提供查询/删除所需的最小能力；图片写入（base64 解码、HEIC、缩略图生成）
//! 在 P3 阶段随关键事件移植一并补齐。

use std::path::{Path, PathBuf};

use tr_store::Workspace;

use crate::ServiceError;

/// 资产根目录（`data/assets`）。
pub fn assets_root(workspace: &Workspace) -> PathBuf {
    workspace.assets_directory()
}

/// 把数据库中的相对路径解析为绝对路径。
pub fn resolve(workspace: &Workspace, relative_path: &str) -> PathBuf {
    assets_root(workspace).join(relative_path)
}

/// 删除一张图片的原图与缩略图。
///
/// 与原实现一致：文件不存在不算失败（例如手工清理过），只记录告警。
/// 调用时机也必须一致——**在事务提交之后**，避免"记录已删但文件删除失败"造成状态不可恢复。
pub fn remove_image_files(workspace: &Workspace, file_path: &str, thumb_path: &str) {
    for relative in [file_path, thumb_path] {
        if relative.is_empty() {
            continue;
        }
        let path = resolve(workspace, relative);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!("删除图片文件失败: {} err: {}", path.display(), error),
        }
    }
}

/// 缩略图最大宽度（与原实现 `thumbMaxWidth` 一致）。
const THUMB_MAX_WIDTH: u32 = 300;
/// 缩略图 JPEG 质量（与原实现 `thumbQuality` 一致）。
const THUMB_QUALITY: u8 = 75;

/// 保存图片：解码 data URI，写原图并生成缩略图，返回（原图相对路径, 缩略图相对路径）。
///
/// 与 Go `util.SaveImage` 等价，包括错误文案与"缩略图失败时删掉已写原图"的回滚。
///
/// **关于 HEIC**：原实现的后端也只支持 JPEG/PNG/GIF/WebP（`mimeToExt` + `image.Decode`），
/// HEIC 是前端用 JS 库转成 JPEG 后再上传的。Rust 版沿用同一分工：
/// HEIC 转换留在界面层（web-sys canvas 交给 WebView2/系统解码器），后端不必接入 libheif/WIC。
pub fn save_image(
    workspace: &Workspace,
    event_date: &str,
    image_id: &str,
    data_uri: &str,
) -> Result<(String, String), ServiceError> {
    let (mime, bytes) = decode_base64_data(data_uri)?;
    let extension = mime_to_extension(&mime);

    let directory = assets_root(workspace).join("key_events").join(event_date);
    std::fs::create_dir_all(&directory).map_err(|error| {
        ServiceError::Internal(format!("create dir {}: {error}", directory.display()))
    })?;

    let relative_base = format!("key_events/{event_date}");
    let original_name = format!("{image_id}{extension}");
    let thumb_name = format!("thumb_{image_id}.jpg");

    let original_path = directory.join(&original_name);
    std::fs::write(&original_path, &bytes)
        .map_err(|error| ServiceError::Internal(format!("write original: {error}")))?;

    let thumb_path = directory.join(&thumb_name);
    if let Err(error) = generate_thumbnail(&bytes, &thumb_path) {
        // 与原实现一致：缩略图失败视为整体失败并回滚已写原图
        let _ = std::fs::remove_file(&original_path);
        return Err(ServiceError::Internal(format!(
            "generate thumbnail: {error}"
        )));
    }

    Ok((
        format!("{relative_base}/{original_name}"),
        format!("{relative_base}/{thumb_name}"),
    ))
}

/// 解析 data URI（`data:image/png;base64,....`）。
fn decode_base64_data(raw: &str) -> Result<(String, Vec<u8>), ServiceError> {
    use base64::Engine;

    let index = raw.find(',').ok_or_else(|| {
        ServiceError::Internal("decode base64: invalid data URI: no comma separator".to_string())
    })?;
    let header = &raw[..index];
    let payload = &raw[index + 1..];

    let mut mime = header
        .find(':')
        .map(|position| &header[position + 1..])
        .unwrap_or("");
    if let Some(position) = mime.find(';') {
        mime = &mime[..position];
    }

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|error| ServiceError::Internal(format!("decode base64: {error}")))?;
    Ok((mime.to_string(), bytes))
}

/// MIME → 扩展名。与原实现一致：未知类型回退 `.jpg`。
fn mime_to_extension(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => ".jpg",
        "image/png" => ".png",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        _ => ".jpg",
    }
}

/// 生成缩略图：宽度超过 300 时按比例缩放到 300（CatmullRom），编码为 JPEG(q75)。
fn generate_thumbnail(data: &[u8], out_path: &Path) -> Result<(), String> {
    let source = image::load_from_memory(data).map_err(|error| format!("decode image: {error}"))?;

    let (width, height) = (source.width(), source.height());
    let (target_width, target_height) = if width > THUMB_MAX_WIDTH {
        let scaled = (f64::from(height) * f64::from(THUMB_MAX_WIDTH) / f64::from(width)) as u32;
        (THUMB_MAX_WIDTH, scaled.max(1))
    } else {
        (width, height)
    };

    let resized = image::imageops::resize(
        &source,
        target_width,
        target_height,
        image::imageops::FilterType::CatmullRom,
    );

    let mut file = std::fs::File::create(out_path)
        .map_err(|error| format!("create thumbnail file: {error}"))?;
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, THUMB_QUALITY)
        .encode_image(&resized)
        .map_err(|error| format!("encode thumbnail: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_joins_under_assets_root() {
        let dir = std::env::temp_dir().join(format!("tr-assets-test-{}", std::process::id()));
        let workspace = Workspace::open(&dir).unwrap();
        let path = resolve(&workspace, "key_events/2026-01-01/a.jpg");
        let expected = dir
            .join("data")
            .join("assets")
            .join("key_events/2026-01-01/a.jpg");
        assert_eq!(path, expected);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remove_image_files_is_tolerant_of_missing_files() {
        let dir = std::env::temp_dir().join(format!("tr-assets-rm-{}", std::process::id()));
        let workspace = Workspace::open(&dir).unwrap();
        let target = resolve(&workspace, "key_events/2026-01-01/a.jpg");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"x").unwrap();

        remove_image_files(
            &workspace,
            "key_events/2026-01-01/a.jpg",
            "key_events/2026-01-01/thumb_a.jpg",
        );
        assert!(!target.exists());
        // 再次调用（文件已不存在）不应 panic
        remove_image_files(&workspace, "key_events/2026-01-01/a.jpg", "");

        std::fs::remove_dir_all(&dir).ok();
    }
}
