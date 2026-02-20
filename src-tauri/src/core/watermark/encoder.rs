use md5::{Md5, Digest};
use crate::models::{WatermarkData, BlindMarkError};

// ─── 原始文本水印编码常量 ────────────────────────────────────────────────────────

/// 魔数："WM"（用于识别是否嵌入了原始文本水印）
pub const TEXT_WATERMARK_MAGIC: [u8; 2] = [0x57, 0x4D];
/// 头部总位数：2字节魔数 + 2字节长度 = 32 位
pub const TEXT_WATERMARK_HEADER_BITS: usize = 32;
/// 固定总位数：头部 + 最大 64 字节 payload = 32 + 512 = 544 位
pub const TEXT_WATERMARK_TOTAL_BITS: usize = 544;
/// 文本 payload 最大字节数（UTF-8 编码后）
pub const TEXT_WATERMARK_MAX_BYTES: usize = 64;

/// Watermark encoder for converting text to MD5 hash and binary sequence
pub struct WatermarkEncoder;

impl WatermarkEncoder {
    /// Convert text to watermark binary sequence (MD5 mode, 128 bits)
    ///
    /// Process:
    /// 1. Calculate MD5 hash of input text (128 bits = 16 bytes)
    /// 2. Convert hash bytes to binary sequence (128 bits)
    /// 3. Return WatermarkData with both hex string and binary form
    pub fn encode(text: &str) -> WatermarkData {
        // Calculate MD5 hash (128 bits = 16 bytes)
        let mut hasher = Md5::new();
        hasher.update(text.as_bytes());
        let hash_bytes = hasher.finalize();

        // Convert to hex string for display/storage
        let md5_hash = format!("{:x}", hash_bytes);

        // Convert hash bytes to binary sequence (128 bits)
        let mut binary_sequence = Vec::with_capacity(128);
        for byte in hash_bytes.iter() {
            // Extract each bit from MSB to LSB
            for bit_pos in (0..8).rev() {
                binary_sequence.push((byte >> bit_pos) & 1);
            }
        }

        WatermarkData::new(md5_hash, binary_sequence)
    }

    /// Decode binary sequence back to MD5 hash string
    ///
    /// Takes a 128-bit binary sequence and converts it back to hex string format.
    /// This is used during watermark extraction to display the embedded data.
    pub fn decode(binary_sequence: &[u8]) -> Result<String, BlindMarkError> {
        if binary_sequence.len() != 128 {
            return Err(BlindMarkError::ExtractionFailed(format!(
                "Invalid binary sequence length: expected 128 bits, got {}",
                binary_sequence.len()
            )));
        }

        // Convert binary sequence back to bytes (16 bytes)
        let mut hash_bytes = Vec::with_capacity(16);
        for chunk in binary_sequence.chunks(8) {
            let mut byte = 0u8;
            for (i, bit) in chunk.iter().enumerate() {
                if *bit > 1 {
                    return Err(BlindMarkError::ExtractionFailed(format!(
                        "Invalid bit value: expected 0 or 1, got {}",
                        bit
                    )));
                }
                byte |= bit << (7 - i);
            }
            hash_bytes.push(byte);
        }

        // Format as hex string (32 characters)
        Ok(hash_bytes
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>())
    }

    // ─── 原始文本水印编码 ──────────────────────────────────────────────────────────

    /// 将原始文本编码为固定 544 位比特序列（用于图片盲水印）
    ///
    /// 格式：[魔数 2B: 0x57 0x4D][长度 2B u16 大端序][UTF-8文本][零填充]
    ///
    /// 最大文本长度：64 字节（UTF-8 编码后），约 64 个 ASCII 字符或 21 个汉字
    pub fn text_to_bits(text: &str) -> Result<Vec<u8>, BlindMarkError> {
        let bytes = text.as_bytes();
        if bytes.len() > TEXT_WATERMARK_MAX_BYTES {
            return Err(BlindMarkError::InvalidConfig(format!(
                "水印文本超出最大长度（{} 字节），当前 {} 字节（UTF-8 编码后）",
                TEXT_WATERMARK_MAX_BYTES, bytes.len()
            )));
        }
        let len = bytes.len() as u16;
        let mut bits = Vec::with_capacity(TEXT_WATERMARK_TOTAL_BITS);

        // 魔数（2 字节，MSB 优先）
        for &b in &TEXT_WATERMARK_MAGIC {
            for i in (0..8usize).rev() { bits.push((b >> i) & 1); }
        }
        // 文本长度（u16 大端序，16 位，MSB 优先）
        for i in (0..16usize).rev() { bits.push(((len >> i) & 1) as u8); }
        // 文本字节（MSB 优先）
        for &b in bytes {
            for i in (0..8usize).rev() { bits.push((b >> i) & 1); }
        }
        // 零填充至 544 位
        bits.resize(TEXT_WATERMARK_TOTAL_BITS, 0);
        Ok(bits)
    }

    /// 从比特序列中尝试解析原始文本水印
    ///
    /// 若魔数不匹配或 UTF-8 无效则返回 `None`（表示图片中无此格式水印）
    pub fn bits_to_text(bits: &[u8]) -> Option<String> {
        if bits.len() < TEXT_WATERMARK_HEADER_BITS { return None; }

        // 验证魔数（前 16 位）
        let mut magic = [0u8; 2];
        for (mi, m) in magic.iter_mut().enumerate() {
            for j in 0..8 {
                *m = (*m << 1) | bits[mi * 8 + j];
            }
        }
        if magic != TEXT_WATERMARK_MAGIC { return None; }

        // 读取长度（位 16-31，u16 大端序）
        let mut len = 0u16;
        for j in 0..16 { len = (len << 1) | (bits[16 + j] as u16); }
        let len = len as usize;
        if len > TEXT_WATERMARK_MAX_BYTES { return None; }

        // 读取文本字节
        let needed = TEXT_WATERMARK_HEADER_BITS + len * 8;
        if bits.len() < needed { return None; }
        let mut bytes = Vec::with_capacity(len);
        for i in 0..len {
            let mut byte = 0u8;
            for j in 0..8 {
                byte = (byte << 1) | bits[TEXT_WATERMARK_HEADER_BITS + i * 8 + j];
            }
            bytes.push(byte);
        }

        String::from_utf8(bytes).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_known_text() {
        let text = "Hello, World!";
        let watermark = WatermarkEncoder::encode(text);

        // MD5 of "Hello, World!" is 65a8e27d8879283831b664bd8b7f0ad4
        assert_eq!(watermark.md5_hash, "65a8e27d8879283831b664bd8b7f0ad4");
        assert_eq!(watermark.binary_sequence.len(), 128);
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let text = "Test watermark 123";
        let watermark = WatermarkEncoder::encode(text);
        let decoded = WatermarkEncoder::decode(&watermark.binary_sequence).unwrap();

        assert_eq!(watermark.md5_hash, decoded);
    }

    #[test]
    fn test_decode_invalid_length() {
        let invalid_sequence = vec![0u8; 100];  // Wrong length
        let result = WatermarkEncoder::decode(&invalid_sequence);

        assert!(result.is_err());
        if let Err(BlindMarkError::ExtractionFailed(msg)) = result {
            assert!(msg.contains("Invalid binary sequence length"));
        }
    }

    #[test]
    fn test_decode_invalid_bit_values() {
        let mut invalid_sequence = vec![0u8; 128];
        invalid_sequence[0] = 2;  // Invalid bit value
        let result = WatermarkEncoder::decode(&invalid_sequence);

        assert!(result.is_err());
    }

    #[test]
    fn test_binary_sequence_all_bits() {
        let watermark = WatermarkEncoder::encode("test");

        // Verify all bits are either 0 or 1
        for bit in &watermark.binary_sequence {
            assert!(*bit == 0 || *bit == 1);
        }
    }

    // ─── 原始文本编码测试 ──────────────────────────────────────────────────────────

    #[test]
    fn test_text_to_bits_length() {
        let bits = WatermarkEncoder::text_to_bits("Hello").unwrap();
        assert_eq!(bits.len(), TEXT_WATERMARK_TOTAL_BITS);
    }

    #[test]
    fn test_text_roundtrip_ascii() {
        let text = "购买者:张三";
        let bits = WatermarkEncoder::text_to_bits(text).unwrap();
        let decoded = WatermarkEncoder::bits_to_text(&bits).unwrap();
        assert_eq!(decoded, text);
    }

    #[test]
    fn test_text_roundtrip_chinese() {
        let text = "购买者:张三李四 ID:12345";
        let bits = WatermarkEncoder::text_to_bits(text).unwrap();
        let decoded = WatermarkEncoder::bits_to_text(&bits).unwrap();
        assert_eq!(decoded, text);
    }

    #[test]
    fn test_text_too_long() {
        let long_text = "a".repeat(65);
        let result = WatermarkEncoder::text_to_bits(&long_text);
        assert!(result.is_err());
    }

    #[test]
    fn test_bits_to_text_invalid_magic() {
        let mut bits = vec![0u8; TEXT_WATERMARK_TOTAL_BITS];
        // Wrong magic → should return None
        assert!(WatermarkEncoder::bits_to_text(&bits).is_none());
        // All zeros has magic 0x00 0x00 ≠ 0x57 0x4D
        bits[0] = 1;
        assert!(WatermarkEncoder::bits_to_text(&bits).is_none());
    }
}
