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

use tr_domain::error::AppError;
use tr_store::Workspace;

use crate::ServiceError;

/// 资产根目录（`data/assets`）。
pub fn assets_root(workspace: &Workspace) -> PathBuf {
    workspace.assets_directory()
}

/// 工作空间图标在资产目录里的**固定基名**（不含扩展名）。
///
/// 图标与关键事件图片放在同一棵树里（`data/assets`），但只有一张、**不落库**：
/// 磁盘上有没有这个文件就是唯一事实来源，于是图标跟着工作空间目录走 ——
/// 换机器、拷目录、备份都不会丢。
pub const WORKSPACE_ICON_STEM: &str = "workspace-icon";

/// 允许的图标扩展名（顺序即查找顺序）。
///
/// 界面固定编码 PNG；其余三种是"万一将来换了编码方式"的兼容位 ——
/// 写入时会先清掉别的扩展名，所以同一时刻最多只有一个文件。
const WORKSPACE_ICON_EXTENSIONS: [&str; 4] = [".png", ".jpg", ".webp", ".gif"];

/// 工作空间图标相对 `data/assets` 的路径；**未设置时返回空串**。
pub fn workspace_icon_relative(workspace: &Workspace) -> String {
    let root = assets_root(workspace);
    for extension in WORKSPACE_ICON_EXTENSIONS {
        let name = format!("{WORKSPACE_ICON_STEM}{extension}");
        if root.join(&name).is_file() {
            return name;
        }
    }
    String::new()
}

/// 保存工作空间图标：解码 data URI → 校验确实是一张图片 → 落盘。
///
/// 校验放在写盘**之前**：坏数据（不是图片、或是被截断的 base64）不该在
/// 资产目录里留下一个打不开的图标 —— 那会让侧栏显示成一块裂图。
/// 返回写入后的相对路径（界面拿它拼 `trasset://` URL）。
pub fn save_workspace_icon(workspace: &Workspace, data_uri: &str) -> Result<String, ServiceError> {
    let (mime, bytes) = decode_base64_data(data_uri)?;
    let extension = match mime.as_str() {
        "image/png" => ".png",
        "image/jpeg" => ".jpg",
        "image/webp" => ".webp",
        "image/gif" => ".gif",
        _ => {
            return Err(ServiceError::from(AppError::bad_request(
                "图标只支持 PNG / JPEG / WebP / GIF",
            )))
        }
    };
    image::load_from_memory(&bytes).map_err(|error| {
        ServiceError::from(AppError::bad_request(format!(
            "图标不是可解码的图片: {error}"
        )))
    })?;

    let root = assets_root(workspace);
    std::fs::create_dir_all(&root)
        .map_err(|error| ServiceError::Internal(format!("create assets dir: {error}")))?;
    // 先清掉其它扩展名：同一时刻只留一个文件，"有没有图标"就只有一个判据
    remove_workspace_icon(workspace);

    let name = format!("{WORKSPACE_ICON_STEM}{extension}");
    let path = root.join(&name);
    std::fs::write(&path, &bytes)
        .map_err(|error| ServiceError::Internal(format!("write workspace icon: {error}")))?;
    Ok(name)
}

/// 删除工作空间图标（不存在不算失败：用户可能没设过，或手工清理过）。
pub fn remove_workspace_icon(workspace: &Workspace) {
    let root = assets_root(workspace);
    for extension in WORKSPACE_ICON_EXTENSIONS {
        let path = root.join(format!("{WORKSPACE_ICON_STEM}{extension}"));
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!("删除图标失败: {} err: {}", path.display(), error),
        }
    }
}

/// 把数据库中的相对路径解析为绝对路径。
pub fn resolve(workspace: &Workspace, relative_path: &str) -> PathBuf {
    assets_root(workspace).join(relative_path)
}

/// 删除一张图片的原图与缩略图。
///
/// 文件不存在不算失败（例如手工清理过），只记录告警。
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

/// 缩略图最大宽度：300（像素）。
const THUMB_MAX_WIDTH: u32 = 300;
/// 缩略图 JPEG 质量：75。
const THUMB_QUALITY: u8 = 75;

/// 保存图片：解码 data URI，写原图并生成缩略图，返回（原图相对路径, 缩略图相对路径）。
///
/// 错误文案与"缩略图失败时删掉已写原图"的回滚是硬契约。
///
/// **关于 HEIC**：后端只支持 JPEG/PNG/GIF/WebP，
/// HEIC 由前端转成 JPEG 后再上传。分工保持不变：
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
        // 缩略图失败视为整体失败并回滚已写原图
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

/// MIME → 扩展名：未知类型回退 `.jpg`。
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
    use image::GenericImageView; // `dimensions()` 来自该 trait

    /// 造一张纯色 PNG（宽高可控），再包成 data URI。
    fn png_data_uri(width: u32, height: u32) -> String {
        use base64::Engine;
        let mut bytes = Vec::new();
        image::DynamicImage::new_rgb8(width, height)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        )
    }

    fn temp_workspace(tag: &str) -> (PathBuf, Workspace) {
        let dir = std::env::temp_dir().join(format!("tr-assets-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let workspace = Workspace::open(&dir).unwrap();
        (dir, workspace)
    }

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

    /// 工作空间图标的完整生命周期：没有 → 保存 → 换扩展名时不留旧文件 → 删除。
    ///
    /// 关键不变量：**同一时刻最多一个 `workspace-icon.*` 文件**。
    /// 否则"有没有图标"就有两个来源，`trasset://` 也可能取到上一版。
    #[test]
    fn workspace_icon_roundtrips_and_keeps_a_single_file() {
        let (dir, workspace) = temp_workspace("icon");
        assert_eq!(workspace_icon_relative(&workspace), "");

        let saved = save_workspace_icon(&workspace, &png_data_uri(64, 64)).unwrap();
        assert_eq!(saved, "workspace-icon.png");
        assert_eq!(workspace_icon_relative(&workspace), "workspace-icon.png");

        // 换一种编码 → 旧文件必须消失
        let jpeg = {
            use base64::Engine;
            let image = image::DynamicImage::new_rgb8(32, 32);
            let mut bytes = Vec::new();
            image
                .write_to(
                    &mut std::io::Cursor::new(&mut bytes),
                    image::ImageFormat::Jpeg,
                )
                .unwrap();
            format!(
                "data:image/jpeg;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(&bytes)
            )
        };
        assert_eq!(
            save_workspace_icon(&workspace, &jpeg).unwrap(),
            "workspace-icon.jpg"
        );
        assert!(!assets_root(&workspace).join("workspace-icon.png").exists());
        assert_eq!(workspace_icon_relative(&workspace), "workspace-icon.jpg");

        remove_workspace_icon(&workspace);
        assert_eq!(workspace_icon_relative(&workspace), "");
        // 再删一次不该 panic
        remove_workspace_icon(&workspace);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 坏数据必须在**写盘之前**被拒：不然侧栏会显示成一块裂图。
    #[test]
    fn workspace_icon_rejects_non_images_and_unknown_mime() {
        use base64::Engine;

        let (dir, workspace) = temp_workspace("icon-bad");

        let not_an_image = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(b"not an image")
        );
        let error = save_workspace_icon(&workspace, &not_an_image).unwrap_err();
        assert_eq!(error.into_app_error().status, 400);

        let bitmap = format!(
            "data:image/bmp;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(b"BM")
        );
        assert!(save_workspace_icon(&workspace, &bitmap).is_err());

        assert_eq!(workspace_icon_relative(&workspace), "", "失败不得留下文件");

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

    /// 目录布局与命名是数据兼容的一部分：原图 `<uuid>.<ext>`、缩略图 `thumb_<uuid>.jpg`。
    #[test]
    fn save_image_writes_compatible_layout_and_scales_thumbnail_to_300() {
        let (dir, workspace) = temp_workspace("save");

        let (file_path, thumb_path) =
            save_image(&workspace, "2026-01-01", "abc-123", &png_data_uri(600, 400)).unwrap();

        assert_eq!(file_path, "key_events/2026-01-01/abc-123.png");
        assert_eq!(thumb_path, "key_events/2026-01-01/thumb_abc-123.jpg");

        // 原图按原字节落盘（不做任何转码）
        let original = resolve(&workspace, &file_path);
        assert!(original.exists());
        assert_eq!(
            image::load_from_memory(&std::fs::read(&original).unwrap())
                .unwrap()
                .dimensions(),
            (600, 400)
        );

        // 缩略图：宽度压到 300 且保持比例（600x400 → 300x200），编码为 JPEG
        let thumbnail = resolve(&workspace, &thumb_path);
        let decoded = image::load_from_memory(&std::fs::read(&thumbnail).unwrap()).unwrap();
        assert_eq!(decoded.dimensions(), (300, 200));
        assert_eq!(
            std::fs::read(&thumbnail).unwrap()[..2],
            [0xFF, 0xD8],
            "缩略图应是 JPEG"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_image_does_not_upscale_small_images() {
        let (dir, workspace) = temp_workspace("small");

        let (_, thumb_path) =
            save_image(&workspace, "2026-01-01", "small-1", &png_data_uri(100, 50)).unwrap();

        let decoded =
            image::load_from_memory(&std::fs::read(resolve(&workspace, &thumb_path)).unwrap())
                .unwrap();
        assert_eq!(decoded.dimensions(), (100, 50));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 缩略图失败时**回滚已写原图**，不能留下半个资产。
    #[test]
    fn save_image_rolls_back_original_when_thumbnail_fails() {
        use base64::Engine;
        let (dir, workspace) = temp_workspace("rollback");

        let not_an_image =
            base64::engine::general_purpose::STANDARD.encode(b"definitely not an image");
        let error = save_image(
            &workspace,
            "2026-01-01",
            "broken-1",
            &format!("data:image/png;base64,{not_an_image}"),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("generate thumbnail"),
            "错误文案应指向缩略图环节：{error}"
        );

        assert!(!resolve(&workspace, "key_events/2026-01-01/broken-1.png").exists());
        let leftovers = std::fs::read_dir(assets_root(&workspace).join("key_events/2026-01-01"))
            .unwrap()
            .count();
        assert_eq!(leftovers, 0, "回滚后目录应没有残留文件");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 未知 MIME 回退 `.jpg`。
    #[test]
    fn save_image_falls_back_to_jpg_extension_for_unknown_mime() {
        let (dir, workspace) = temp_workspace("mime");

        let payload = png_data_uri(20, 20);
        let payload = payload.split_once(',').unwrap().1;
        let (file_path, _) = save_image(
            &workspace,
            "2026-01-01",
            "unknown-1",
            &format!("data:application/octet-stream;base64,{payload}"),
        )
        .unwrap();
        assert_eq!(file_path, "key_events/2026-01-01/unknown-1.jpg");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_image_rejects_data_uri_without_comma() {
        let (dir, workspace) = temp_workspace("uri");

        let error =
            save_image(&workspace, "2026-01-01", "bad-1", "just-a-plain-string").unwrap_err();
        assert!(
            error.to_string().contains("no comma separator"),
            "错误文案应保持稳定：{error}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
