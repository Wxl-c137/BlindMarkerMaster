use image::DynamicImage;
use ndarray::Array2;
use crate::models::BlindMarkError;
use crate::core::watermark::{
    dwt::DWTProcessor,
    dct::DCTProcessor,
    encoder::{WatermarkEncoder, TEXT_WATERMARK_TOTAL_BITS},
};

/// 完整的水印提取流水线
///
/// ## 算法（与 Python blind_watermark 完全一致）
///
/// 1. 对 R、G、B 三个通道分别做 **1 级 Haar DWT** → LL 子带
/// 2. 将 LL 子带划分为 **4×4 块**
/// 3. 对每块：2D DCT → 随机打乱 → SVD → **QIM 解码**
///    - 每块输出软判决值 = `(soft_s0 * 3 + soft_s1) / 4`，值域 [0, 1]
/// 4. 对所有块的循环副本取平均（与 Python extract_avg 一致）
/// 5. 三通道软判决值求和，阈值 1.5（= 0.5 × 3 通道）判决最终比特
pub struct WatermarkExtractor {
    dwt: DWTProcessor,
    dct: DCTProcessor,
}

impl WatermarkExtractor {
    pub fn new() -> Self {
        Self {
            dwt: DWTProcessor::new(),
            dct: DCTProcessor::new(),
        }
    }

    /// 从图片中提取 MD5 水印哈希字符串
    pub fn extract(&self, image: &DynamicImage) -> Result<String, BlindMarkError> {
        let soft_sum = self.extract_soft_sum(image, 128)?;
        let bits: Vec<u8> = soft_sum
            .iter()
            .map(|&v| if v > 1.5 { 1u8 } else { 0u8 })
            .collect();
        WatermarkEncoder::decode(&bits)
    }

    /// 提取 MD5 水印并返回置信度（保留接口，置信度固定为 1.0）
    pub fn extract_with_confidence(&self, image: &DynamicImage) -> Result<(String, f32), BlindMarkError> {
        let md5_hash = self.extract(image)?;
        Ok((md5_hash, 1.0))
    }

    /// 尝试从图片中提取原始文本盲水印
    ///
    /// ## 返回值
    /// * `Ok(Some(text))` — 图片有合法的原始文本水印
    /// * `Ok(None)` — 图片没有此格式水印（魔数不匹配、图片太小等）
    /// * `Err(...)` — 图片处理本身失败
    pub fn try_extract_text(&self, image: &DynamicImage) -> Result<Option<String>, BlindMarkError> {
        let soft_sum = match self.extract_soft_sum(image, TEXT_WATERMARK_TOTAL_BITS) {
            Ok(s) => s,
            Err(_) => return Ok(None),
        };

        // 三通道各贡献 [0,1]，总和在 [0,3]，阈值 1.5
        let bits: Vec<u8> = soft_sum
            .iter()
            .map(|&v| if v > 1.5 { 1u8 } else { 0u8 })
            .collect();

        Ok(WatermarkEncoder::bits_to_text(&bits))
    }

    /// 提取原始文本水印（若无则返回错误）
    pub fn extract_text(&self, image: &DynamicImage) -> Result<String, BlindMarkError> {
        self.try_extract_text(image)?.ok_or_else(|| {
            BlindMarkError::ExtractionFailed("图片中未找到原始文本盲水印".to_string())
        })
    }

    // ─── 核心提取逻辑 ─────────────────────────────────────────────────────────

    /// 对三个 RGB 通道提取软判决值并求和
    ///
    /// 返回长度为 `wm_size` 的向量，每个元素为三通道软判决值之和，值域 [0, 3]。
    fn extract_soft_sum(
        &self,
        image: &DynamicImage,
        wm_size: usize,
    ) -> Result<Vec<f64>, BlindMarkError> {
        let rgb_image = image.to_rgb8();
        let (width, height) = rgb_image.dimensions();
        let (w, h) = (width as usize, height as usize);

        // 奇数尺寸无法做 DWT
        if width % 2 != 0 || height % 2 != 0 {
            return Err(BlindMarkError::ImageProcessing(
                format!("图片尺寸必须为偶数：{}×{}", width, height)
            ));
        }

        let mut soft_sum = vec![0.0f64; wm_size];

        for ch in 0..3usize {
            let mut ch_data = Array2::zeros((h, w));
            for y in 0..h {
                for x in 0..w {
                    let p = rgb_image.get_pixel(x as u32, y as u32);
                    ch_data[[y, x]] = p[ch] as f64;
                }
            }

            let (ll, _, _, _) = match self.dwt.decompose_1level(ch_data.view()) {
                Ok(c) => c,
                Err(_) => return Err(BlindMarkError::ImageProcessing(
                    "DWT 分解失败".to_string()
                )),
            };

            let soft = self.dct.extract_watermark_blocks_soft(&ll, wm_size)?;

            for (i, &v) in soft.iter().enumerate() {
                soft_sum[i] += v;
            }
        }

        Ok(soft_sum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::watermark::embedder::WatermarkEmbedder;
    use image::{Rgb, ImageBuffer};

    /// PNG 存取 roundtrip（模拟真实文件读写场景）
    fn png_roundtrip(img: &DynamicImage) -> DynamicImage {
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png).unwrap();
        image::load_from_memory(&buf).unwrap()
    }

    fn create_test_image(width: u32, height: u32) -> DynamicImage {
        let mut img = ImageBuffer::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let r = ((x * 255) / width) as u8;
                let g = ((y * 255) / height) as u8;
                let b = 128u8;
                img.put_pixel(x, y, Rgb([r, g, b]));
            }
        }
        DynamicImage::ImageRgb8(img)
    }

    #[test]
    fn test_extract_basic() {
        let embedder = WatermarkEmbedder::new();
        let extractor = WatermarkExtractor::new();

        // MD5 128 位，256×256 → LL=128×128，1024 块 >> 128 位，冗余充足
        let original = create_test_image(256, 256);
        let watermark_text = "Test watermark";

        let watermarked = embedder.embed(&original, watermark_text, 0.5).unwrap();
        let extracted = extractor.extract(&watermarked);

        assert!(extracted.is_ok(), "提取应成功");

        let expected_hash = WatermarkEncoder::encode(watermark_text).md5_hash;
        assert_eq!(extracted.unwrap(), expected_hash);
    }

    #[test]
    fn test_extract_invalid_dimensions() {
        let extractor = WatermarkExtractor::new();
        let image = create_test_image(63, 63); // 奇数尺寸

        let result = extractor.extract(&image);
        assert!(result.is_err(), "奇数尺寸应失败");
    }

    #[test]
    fn test_extract_different_strengths() {
        let embedder = WatermarkEmbedder::new();
        let extractor = WatermarkExtractor::new();

        let original = create_test_image(256, 256);
        let watermark_text = "Strength test";
        let expected_hash = WatermarkEncoder::encode(watermark_text).md5_hash;

        let weak = embedder.embed(&original, watermark_text, 0.1).unwrap();
        let extracted_weak = extractor.extract(&weak).unwrap();
        assert_eq!(extracted_weak, expected_hash, "弱水印应能提取");

        let strong = embedder.embed(&original, watermark_text, 1.0).unwrap();
        let extracted_strong = extractor.extract(&strong).unwrap();
        assert_eq!(extracted_strong, expected_hash, "强水印应能提取");
    }

    #[test]
    fn test_raw_text_roundtrip_multichannel() {
        let embedder = WatermarkEmbedder::new();
        let extractor = WatermarkExtractor::new();

        // 544 位水印需要 LL ≥ 128×128 = 1024 块，原图 ≥ 256×256
        let original = create_test_image(256, 256);
        let test_text = "Hello, VAM!";

        let watermarked = embedder.embed_raw_text(&original, test_text, 0.5, false).unwrap();
        let extracted = extractor.try_extract_text(&watermarked).unwrap();

        assert_eq!(
            extracted.as_deref(),
            Some(test_text),
            "三通道嵌入/提取应完整还原原始文本"
        );
    }

    #[test]
    fn test_raw_text_roundtrip_various_sizes() {
        let embedder = WatermarkEmbedder::new();
        let extractor = WatermarkExtractor::new();
        let test_text = "BlindMark";

        // 544 位水印最小图片为 256×256（LL=128×128，1024 块 > 544）
        for size in [256u32, 512] {
            let original = create_test_image(size, size);
            let watermarked = embedder.embed_raw_text(&original, test_text, 0.5, false).unwrap();
            let extracted = extractor.try_extract_text(&watermarked).unwrap();

            assert_eq!(
                extracted.as_deref(),
                Some(test_text),
                "{}×{} 图片应能提取水印",
                size,
                size
            );
        }
    }

    #[test]
    fn test_try_extract_none_on_unwatermarked_image() {
        let extractor = WatermarkExtractor::new();
        let image = create_test_image(256, 256);
        let result = extractor.try_extract_text(&image).unwrap();
        assert!(result.is_none(), "未嵌入水印的图片应返回 None");
    }

    #[test]
    fn test_try_extract_none_on_small_image() {
        let extractor = WatermarkExtractor::new();
        // 128×128 → LL=64×64 → 256 块 < 544 位，应返回 None（不报错）
        let image = create_test_image(128, 128);
        let result = extractor.try_extract_text(&image);
        assert!(result.is_ok(), "小图片不应报错: {:?}", result.err());
        assert!(result.unwrap().is_none(), "小图片应返回 None");
    }

    #[test]
    fn test_extract_with_confidence() {
        let embedder = WatermarkEmbedder::new();
        let extractor = WatermarkExtractor::new();

        let original = create_test_image(256, 256);
        let watermark_text = "Confidence test";

        let watermarked = embedder.embed(&original, watermark_text, 0.5).unwrap();
        let result = extractor.extract_with_confidence(&watermarked);

        assert!(result.is_ok(), "带置信度提取应成功");
        let (hash, confidence) = result.unwrap();
        let expected_hash = WatermarkEncoder::encode(watermark_text).md5_hash;
        assert_eq!(hash, expected_hash);
        assert!(confidence > 0.0 && confidence <= 1.0);
    }

    #[test]
    fn test_roundtrip_preserves_hash() {
        let embedder = WatermarkEmbedder::new();
        let extractor = WatermarkExtractor::new();

        let original = create_test_image(256, 256);
        let watermark_text = "Roundtrip test with longer text to ensure proper encoding";

        let watermarked = embedder.embed(&original, watermark_text, 0.3).unwrap();
        let extracted_hash = extractor.extract(&watermarked).unwrap();

        let expected_hash = WatermarkEncoder::encode(watermark_text).md5_hash;
        assert_eq!(extracted_hash, expected_hash);
    }

    #[test]
    fn test_extract_from_different_image_sizes() {
        let embedder = WatermarkEmbedder::new();
        let extractor = WatermarkExtractor::new();

        let watermark_text = "Size test";
        let expected_hash = WatermarkEncoder::encode(watermark_text).md5_hash;

        for size in [256u32, 512] {
            let original = create_test_image(size, size);
            let watermarked = embedder.embed(&original, watermark_text, 0.5).unwrap();
            let extracted = extractor.extract(&watermarked).unwrap();

            assert_eq!(
                extracted, expected_hash,
                "{}×{} 图片提取失败",
                size, size
            );
        }
    }

    /// 关键测试：PNG save/load（u8 量化）后仍能提取文本水印
    #[test]
    fn test_raw_text_roundtrip_with_png_quantization() {
        let embedder = WatermarkEmbedder::new();
        let extractor = WatermarkExtractor::new();

        let test_cases = [
            (256u32, 256u32, "Hello"),
            (512, 512, "TestWatermark"),
        ];

        for (w, h, text) in &test_cases {
            let original = create_test_image(*w, *h);
            let watermarked = embedder.embed_raw_text(&original, text, 0.5, false)
                .unwrap_or_else(|e| panic!("embed 失败 {}×{}: {}", w, h, e));

            let after_png = png_roundtrip(&watermarked);
            let extracted = extractor.try_extract_text(&after_png)
                .unwrap_or_else(|e| panic!("extract 错误 {}×{}: {}", w, h, e));

            assert_eq!(
                extracted.as_deref(),
                Some(text.as_ref()),
                "PNG roundtrip 失败 {}×{} 文本 {:?}",
                w, h, text
            );
        }
    }

    /// 核心测试：高频噪声图片（模拟真实照片）经 PNG roundtrip 后应能提取水印
    ///
    /// 旧算法（LH2+全局DCT+符号编码）在此测试上失败，
    /// 新算法（LL1+4×4块DCT+SVD+QIM）应通过。
    #[test]
    fn test_raw_text_roundtrip_noisy_image() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let embedder = WatermarkEmbedder::new();
        let extractor = WatermarkExtractor::new();

        let (w, h) = (256u32, 256u32);
        let mut img = ImageBuffer::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let mut s = DefaultHasher::new();
                (x, y).hash(&mut s);
                let v = s.finish();
                img.put_pixel(x, y, Rgb([
                    (v & 0xFF) as u8,
                    ((v >> 8) & 0xFF) as u8,
                    ((v >> 16) & 0xFF) as u8,
                ]));
            }
        }
        let original = DynamicImage::ImageRgb8(img);

        let text = "RealImageTest";
        let watermarked = embedder.embed_raw_text(&original, text, 0.5, false)
            .expect("embed 应在 256×256 噪声图上成功");

        let after_png = png_roundtrip(&watermarked);

        let extracted = extractor.try_extract_text(&after_png)
            .expect("extract_text 不应报错");

        assert_eq!(
            extracted.as_deref(),
            Some(text),
            "噪声图片经 PNG roundtrip 后应能提取水印（新 QIM 算法应通过此测试）"
        );
    }
}
