# HEIC 测试图片（合成，非真实照片）

`fixtures/` 的纪律是"只放合成数据"，但 HEIC（HEVC 编码的静态图）没法在这台机器上
现造 —— 没有编码器：Windows 的 WIC 解不了（本机没装 HEIF 图像扩展），
纯 Rust 编码器 `still265` 是 GPL-2.0 且要 rustc 1.97。所以这三张图是**从
[`heic-rs`](https://github.com/tbraun96/heic-rs) 的测试套件里拿来的**。

来源与许可（两者都与该 crate 相同）：

* 上游文件：`tests/fixtures/{flat-64.heic, flat-64.ref.png, gradient-512.heic}`
* 许可：**MIT OR Apache-2.0**（`heic-rs` 的 `tests/fixtures/README.md` 明说这些图
  由 `scripts/gen-pngs.py` 逐像素合成、再用 macOS `sips` 编码成 HEIC，
  **不含任何第三方图片数据**，可自由再分发）
* 因此这里也没有任何真实个人数据 —— 与 `fixtures/README.md` 的纪律一致。

| 文件 | 尺寸 | 内容 | 用途 |
|---|---|---|---|
| `flat-64.heic` | 64×64 | 纯色 RGB(200,30,60) | `tr-draw` 单测：断言解码结果与 `flat-64.ref.png` 逐像素接近 |
| `flat-64.ref.png` | 64×64 | 上面那张的 **Apple `sips` 解码结果**（8-bit RGB PNG） | 单测的参照物（不是我们自己的输出） |
| `gradient-512.heic` | 512×512 | 红向右、绿向下渐变，蓝恒为 128 | `ui-upload.ps1`：真跑一次 HEIC 上传（512 > 300，可验证缩略图缩放） |

`flat-64.ref.png` 是**别人的解码器**（Apple）对同一份 HEIC 的解码结果，
所以拿它当参照才有意义 —— 用我们自己的输出当基线是循环论证。
HEVC 有损、且是 4:2:0，允许几个码值的偏差（实测 mean abs ≈ 0.7）。
