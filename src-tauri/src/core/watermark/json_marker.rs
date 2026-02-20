use serde_json::Value;
use rand::Rng;
use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use sha2::{Sha256, Digest};
use crate::models::BlindMarkError;
use crate::core::watermark::encoder::WatermarkEncoder;

/// JSON 明文水印注入器
///
/// 在 JSON 文件的根对象中添加水印字段，
/// 不破坏原有结构和字段，提取时直接读取该字段。
///
/// 适用于 .json / .vaj / .vmi 等基于 JSON 的文件格式。
pub struct JsonWatermarker;

/// 默认水印字段名（未自定义时使用）
pub const DEFAULT_WATERMARK_KEY: &str = "_watermark";

// ─── 私有工具函数 ──────────────────────────────────────────────────────────────

/// 判断字符串是否符合 MD5 格式（32 位小写十六进制）
fn is_md5_like(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// 判断字符串是否是任意一种水印值格式
fn is_watermark_value(s: &str) -> bool {
    is_md5_like(s) || s.starts_with("txt:") || s.starts_with("aes:")
}

/// 字节数组转十六进制字符串
fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// 十六进制字符串转字节数组
fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, BlindMarkError> {
    if hex.len() % 2 != 0 {
        return Err(BlindMarkError::ImageProcessing("无效的十六进制字符串长度".to_string()));
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|_| BlindMarkError::ImageProcessing("十六进制解码失败".to_string()))
        })
        .collect()
}

/// 用 SHA-256 对用户密钥字符串求摘要，得到 32 字节 AES-256 密钥
fn derive_aes_key(user_key: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(user_key.as_bytes());
    hasher.finalize().into()
}

/// AES-256-GCM 加密：返回 `aes:<hex(12字节nonce || 密文含认证标签)>`
fn aes_encrypt(text: &str, key_bytes: &[u8; 32]) -> Result<String, BlindMarkError> {
    let key = Key::<Aes256Gcm>::from_slice(key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(&mut rand::rngs::OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, text.as_bytes())
        .map_err(|e| BlindMarkError::ImageProcessing(format!("AES 加密失败: {}", e)))?;
    let mut combined = nonce.to_vec();
    combined.extend_from_slice(&ciphertext);
    Ok(format!("aes:{}", bytes_to_hex(&combined)))
}

/// AES-256-GCM 解密：接受 `aes:<hex>` 格式的字符串
fn aes_decrypt(encoded: &str, key_bytes: &[u8; 32]) -> Result<String, BlindMarkError> {
    let hex_part = encoded
        .strip_prefix("aes:")
        .ok_or_else(|| BlindMarkError::ImageProcessing("不是有效的 AES 水印格式".to_string()))?;
    let combined = hex_to_bytes(hex_part)?;
    if combined.len() < 12 {
        return Err(BlindMarkError::ImageProcessing("AES 数据长度不足".to_string()));
    }
    let (nonce_bytes, ct) = combined.split_at(12);
    let key = Key::<Aes256Gcm>::from_slice(key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ct)
        .map_err(|_| BlindMarkError::ImageProcessing("AES 解密失败（密钥错误或数据损坏）".to_string()))?;
    String::from_utf8(plaintext)
        .map_err(|e| BlindMarkError::ImageProcessing(format!("解密结果不是有效 UTF-8: {}", e)))
}

/// 根据已有字段名随机生成伪装字段名，并返回用于定位插入位置的基础字段名。
///
/// 策略：随机选取某个已有字段的小写前缀，再随机拼接中性后缀（Hash/Id/Code 等），
/// 使其在视觉上融入原有字段风格。每次调用均独立随机，同一水印文本处理不同文件时结果各异。
fn make_disguised_key<'a>(existing_keys: &[&'a str]) -> (String, Option<&'a str>) {
    let mut rng = rand::thread_rng();
    let suffixes = ["Hash", "Id", "Code", "Key", "Sig", "Ref"];

    if !existing_keys.is_empty() {
        // 随机打乱顺序，依次尝试每个 key 作为基础
        let mut indices: Vec<usize> = (0..existing_keys.len()).collect();
        for i in (1..indices.len()).rev() {
            let j = rng.gen_range(0..=i);
            indices.swap(i, j);
        }

        for &base_idx in &indices {
            let base_key = existing_keys[base_idx];
            // 提取小写前缀（camelCase 第一个大写字母之前的部分）
            let prefix_owned: String = base_key.chars().take_while(|c| c.is_lowercase()).collect();
            let prefix: &str = if prefix_owned.len() >= 3 { &prefix_owned } else { base_key };

            // 随机尝试所有后缀（打乱顺序）
            let mut suf_indices: Vec<usize> = (0..suffixes.len()).collect();
            for i in (1..suf_indices.len()).rev() {
                let j = rng.gen_range(0..=i);
                suf_indices.swap(i, j);
            }
            for &si in &suf_indices {
                let candidate = format!("{}{}", prefix, suffixes[si]);
                if !existing_keys.contains(&candidate.as_str()) {
                    return (candidate, Some(base_key));
                }
            }
        }
    }

    // 备用通用池（同样随机选取）
    let pool = [
        "checksum", "contentHash", "packageId", "creatorId", "assetId",
        "buildVersion", "versionTag", "releaseId", "fileHash", "dataHash",
    ];
    let start = rng.gen_range(0..pool.len());
    for i in 0..pool.len() {
        let k = pool[(start + i) % pool.len()];
        if !existing_keys.contains(&k) {
            return (k.to_string(), None);
        }
    }

    (DEFAULT_WATERMARK_KEY.to_string(), None)
}

// ─── 公开 API ──────────────────────────────────────────────────────────────────

impl JsonWatermarker {
    /// 将水印明文按照指定模式编码为存储字符串
    ///
    /// # 模式
    /// * `"plaintext"` → `txt:<text>`
    /// * `"aes"`       → `aes:<hex(nonce||ciphertext||tag)>`（需要 `aes_key`）
    /// * `"md5"` 或其他 → `<32位小写MD5哈希>`（默认）
    pub fn encode_watermark(
        text: &str,
        mode: &str,
        aes_key: Option<&str>,
    ) -> Result<String, BlindMarkError> {
        match mode {
            "plaintext" => Ok(format!("txt:{}", text)),
            "aes" => {
                let key_str = aes_key.ok_or_else(|| {
                    BlindMarkError::ImageProcessing("AES 模式需要提供密钥".to_string())
                })?;
                let key_bytes = derive_aes_key(key_str);
                aes_encrypt(text, &key_bytes)
            }
            _ => Ok(WatermarkEncoder::encode(text).md5_hash),
        }
    }

    /// 将存储字符串解码为 (显示值, 模式名称, 是否已成功解密/解码)
    ///
    /// * `"plaintext"` → (原文, "plaintext", true)
    /// * `"aes"` 且有正确密钥 → (解密原文, "aes", true)
    /// * `"aes"` 且无密钥或密钥错误 → (原始aes:...字符串, "aes", false)
    /// * MD5 格式 → (MD5哈希, "md5", true)
    /// * 其他 → (原值, "unknown", false)
    pub fn decode_watermark(raw: &str, aes_key: Option<&str>) -> (String, String, bool) {
        if let Some(text) = raw.strip_prefix("txt:") {
            (text.to_string(), "plaintext".to_string(), true)
        } else if raw.starts_with("aes:") {
            if let Some(key_str) = aes_key {
                let key_bytes = derive_aes_key(key_str);
                match aes_decrypt(raw, &key_bytes) {
                    Ok(decrypted) => (decrypted, "aes".to_string(), true),
                    Err(_) => (raw.to_string(), "aes".to_string(), false),
                }
            } else {
                (raw.to_string(), "aes".to_string(), false)
            }
        } else if is_md5_like(raw) {
            (raw.to_string(), "md5".to_string(), true)
        } else {
            (raw.to_string(), "unknown".to_string(), false)
        }
    }

    /// 向 JSON 内容中注入水印
    ///
    /// # 参数
    /// * `content`        - 原始 JSON 字符串（UTF-8）
    /// * `watermark_text` - 要嵌入的明文
    /// * `key`            - 水印字段名
    /// * `mode`           - 编码模式（"md5" / "plaintext" / "aes"）
    /// * `aes_key`        - AES 模式下的用户密钥
    pub fn embed(
        content: &str,
        watermark_text: &str,
        key: &str,
        mode: &str,
        aes_key: Option<&str>,
    ) -> Result<String, BlindMarkError> {
        let mut json: Value = serde_json::from_str(content).map_err(|e| {
            BlindMarkError::ImageProcessing(format!("JSON 解析失败: {}", e))
        })?;

        let encoded = Self::encode_watermark(watermark_text, mode, aes_key)?;

        if let Some(obj) = json.as_object_mut() {
            obj.shift_remove(key);
            obj.insert(key.to_string(), Value::String(encoded));
        }

        serde_json::to_string_pretty(&json).map_err(|e| {
            BlindMarkError::ImageProcessing(format!("JSON 序列化失败: {}", e))
        })
    }

    /// 从 JSON 内容中提取水印（按指定字段名）
    pub fn extract(content: &str, key: &str) -> Result<String, BlindMarkError> {
        let json: Value = serde_json::from_str(content).map_err(|e| {
            BlindMarkError::ImageProcessing(format!("JSON 解析失败: {}", e))
        })?;

        json.get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                BlindMarkError::ExtractionFailed(
                    format!("未在 JSON 中找到水印字段 {}", key),
                )
            })
    }

    /// 检查 JSON 内容是否已包含指定水印字段
    pub fn has_watermark(content: &str, key: &str) -> bool {
        serde_json::from_str::<Value>(content)
            .ok()
            .and_then(|v| v.get(key).cloned())
            .is_some()
    }

    /// 嵌入水印到 JSON 字节（自动处理 UTF-8 解码/编码）
    pub fn embed_bytes(
        bytes: &[u8],
        watermark_text: &str,
        key: &str,
        mode: &str,
        aes_key: Option<&str>,
    ) -> Result<Vec<u8>, BlindMarkError> {
        let content = std::str::from_utf8(bytes).map_err(|e| {
            BlindMarkError::ImageProcessing(format!("JSON 文件不是有效 UTF-8: {}", e))
        })?;
        let result = Self::embed(content, watermark_text, key, mode, aes_key)?;
        Ok(result.into_bytes())
    }

    /// 从 JSON 字节中提取水印（按字段名）
    pub fn extract_bytes(bytes: &[u8], key: &str) -> Result<String, BlindMarkError> {
        let content = std::str::from_utf8(bytes).map_err(|e| {
            BlindMarkError::ImageProcessing(format!("JSON 文件不是有效 UTF-8: {}", e))
        })?;
        Self::extract(content, key)
    }

    /// 混淆模式嵌入：
    /// 1. 遍历已有字段名，生成与之风格一致的伪装字段名
    /// 2. 将水印插入到基础字段附近而非末尾
    /// 3. 自动移除已有的所有格式旧水印，保证每个文件只有一个水印
    pub fn embed_obfuscated(
        content: &str,
        watermark_text: &str,
        mode: &str,
        aes_key: Option<&str>,
    ) -> Result<String, BlindMarkError> {
        let json: Value = serde_json::from_str(content).map_err(|e| {
            BlindMarkError::ImageProcessing(format!("JSON 解析失败: {}", e))
        })?;

        // 非 Object 根节点（如纯数组）原样返回
        let Value::Object(map) = json else {
            return serde_json::to_string_pretty(&json).map_err(|e| {
                BlindMarkError::ImageProcessing(format!("JSON 序列化失败: {}", e))
            });
        };

        let encoded = Self::encode_watermark(watermark_text, mode, aes_key)?;

        // 过滤掉所有值为水印格式的旧水印字段（兼容三种格式）
        let clean_entries: Vec<(String, Value)> = map
            .into_iter()
            .filter(|(_, v)| !v.as_str().map(is_watermark_value).unwrap_or(false))
            .collect();

        let existing_key_refs: Vec<&str> = clean_entries.iter().map(|(k, _)| k.as_str()).collect();
        let (disguised_key, base_key) = make_disguised_key(&existing_key_refs);

        // 插入位置：紧靠基础字段之后；否则在中段随机选位（避免放在末尾）
        let n = clean_entries.len();
        let insert_pos = base_key
            .and_then(|bk| clean_entries.iter().position(|(k, _)| k.as_str() == bk))
            .map(|p| p + 1)
            .unwrap_or_else(|| {
                if n <= 2 { n.saturating_sub(1) }
                else { rand::thread_rng().gen_range(1..n) }
            });

        let mut new_map = serde_json::Map::new();
        let mut inserted = false;
        for (i, (k, v)) in clean_entries.into_iter().enumerate() {
            if i == insert_pos {
                new_map.insert(disguised_key.clone(), Value::String(encoded.clone()));
                inserted = true;
            }
            new_map.insert(k, v);
        }
        if !inserted {
            new_map.insert(disguised_key, Value::String(encoded));
        }

        serde_json::to_string_pretty(&Value::Object(new_map)).map_err(|e| {
            BlindMarkError::ImageProcessing(format!("JSON 序列化失败: {}", e))
        })
    }

    /// 扫描 JSON 内容，提取所有水印值（兼容明文、MD5、AES 三种格式）
    ///
    /// # 返回
    /// 每个元素为 `(显示值, 模式名称, 是否已成功解码)`
    pub fn scan_watermark_values(
        content: &str,
        aes_key: Option<&str>,
    ) -> Vec<(String, String, bool)> {
        let Ok(json) = serde_json::from_str::<Value>(content) else {
            return vec![];
        };
        let Some(obj) = json.as_object() else {
            return vec![];
        };
        obj.values()
            .filter_map(|v| v.as_str())
            .filter(|s| is_watermark_value(s))
            .map(|s| Self::decode_watermark(s, aes_key))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embed_md5_mode() {
        let json = r#"{"name": "test", "version": "1.0"}"#;
        let result = JsonWatermarker::embed(json, "hello world", DEFAULT_WATERMARK_KEY, "md5", None).unwrap();

        let parsed: Value = serde_json::from_str(&result).unwrap();
        let wm = parsed["_watermark"].as_str().unwrap();
        assert!(is_md5_like(wm), "MD5 模式应存储32位哈希");
        assert!(parsed.get("name").is_some());
        assert!(parsed.get("version").is_some());
    }

    #[test]
    fn test_embed_plaintext_mode() {
        let json = r#"{"name": "test"}"#;
        let result = JsonWatermarker::embed(json, "张三", DEFAULT_WATERMARK_KEY, "plaintext", None).unwrap();

        let parsed: Value = serde_json::from_str(&result).unwrap();
        let wm = parsed["_watermark"].as_str().unwrap();
        assert_eq!(wm, "txt:张三");
    }

    #[test]
    fn test_embed_aes_mode() {
        let json = r#"{"name": "test"}"#;
        let result = JsonWatermarker::embed(json, "张三", DEFAULT_WATERMARK_KEY, "aes", Some("mykey")).unwrap();

        let parsed: Value = serde_json::from_str(&result).unwrap();
        let wm = parsed["_watermark"].as_str().unwrap();
        assert!(wm.starts_with("aes:"), "AES 模式应以 aes: 开头");
    }

    #[test]
    fn test_aes_roundtrip() {
        let json = r#"{"name": "test"}"#;
        let watermarked = JsonWatermarker::embed(json, "购买者:李四", DEFAULT_WATERMARK_KEY, "aes", Some("secret")).unwrap();

        // 扫描，提供正确密钥
        let findings = JsonWatermarker::scan_watermark_values(&watermarked, Some("secret"));
        assert_eq!(findings.len(), 1);
        let (value, mode, decrypted) = &findings[0];
        assert_eq!(value, "购买者:李四");
        assert_eq!(mode, "aes");
        assert!(decrypted);
    }

    #[test]
    fn test_aes_wrong_key() {
        let json = r#"{"name": "test"}"#;
        let watermarked = JsonWatermarker::embed(json, "秘密", DEFAULT_WATERMARK_KEY, "aes", Some("correct")).unwrap();

        // 提供错误密钥
        let findings = JsonWatermarker::scan_watermark_values(&watermarked, Some("wrong"));
        assert_eq!(findings.len(), 1);
        let (_, mode, decrypted) = &findings[0];
        assert_eq!(mode, "aes");
        assert!(!decrypted, "错误密钥应导致解密失败");
    }

    #[test]
    fn test_decode_watermark_plaintext() {
        let (val, mode, ok) = JsonWatermarker::decode_watermark("txt:hello", None);
        assert_eq!(val, "hello");
        assert_eq!(mode, "plaintext");
        assert!(ok);
    }

    #[test]
    fn test_decode_watermark_md5() {
        let (val, mode, ok) = JsonWatermarker::decode_watermark("5d41402abc4b2a76b9719d911017c592", None);
        assert_eq!(mode, "md5");
        assert!(ok);
        assert_eq!(val.len(), 32);
    }

    #[test]
    fn test_extract_roundtrip() {
        let json = r#"{"licenseType": "CC BY-NC-SA", "packageName": "test"}"#;
        let watermark_text = "Dnaddr.Mica_v2";

        let watermarked = JsonWatermarker::embed(json, watermark_text, DEFAULT_WATERMARK_KEY, "md5", None).unwrap();
        let extracted = JsonWatermarker::extract(&watermarked, DEFAULT_WATERMARK_KEY).unwrap();

        let expected = crate::core::watermark::encoder::WatermarkEncoder::encode(watermark_text).md5_hash;
        assert_eq!(extracted, expected);
    }

    #[test]
    fn test_overwrite_existing_watermark() {
        let json = r#"{"key": "value", "_watermark": "old_hash_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"}"#;
        let result = JsonWatermarker::embed(json, "new text", DEFAULT_WATERMARK_KEY, "md5", None).unwrap();
        let extracted = JsonWatermarker::extract(&result, DEFAULT_WATERMARK_KEY).unwrap();

        let new_expected = crate::core::watermark::encoder::WatermarkEncoder::encode("new text").md5_hash;
        assert_eq!(extracted, new_expected);
    }

    #[test]
    fn test_non_object_json() {
        let json = r#"[1, 2, 3]"#;
        let result = JsonWatermarker::embed(json, "test", DEFAULT_WATERMARK_KEY, "md5", None).unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.is_array());
    }

    #[test]
    fn test_obfuscated_three_modes() {
        let meta = r#"{"licenseType": "CC BY-NC-SA", "creatorName": "Dnaddr"}"#;

        // 明文模式
        let wm1 = JsonWatermarker::embed_obfuscated(meta, "张三", "plaintext", None).unwrap();
        let findings1 = JsonWatermarker::scan_watermark_values(&wm1, None);
        assert!(!findings1.is_empty());
        assert_eq!(findings1[0].1, "plaintext");
        assert_eq!(findings1[0].0, "张三");

        // MD5 模式
        let wm2 = JsonWatermarker::embed_obfuscated(meta, "张三", "md5", None).unwrap();
        let findings2 = JsonWatermarker::scan_watermark_values(&wm2, None);
        assert!(!findings2.is_empty());
        assert_eq!(findings2[0].1, "md5");

        // AES 模式
        let wm3 = JsonWatermarker::embed_obfuscated(meta, "张三", "aes", Some("key123")).unwrap();
        let findings3 = JsonWatermarker::scan_watermark_values(&wm3, Some("key123"));
        assert!(!findings3.is_empty());
        assert_eq!(findings3[0].1, "aes");
        assert_eq!(findings3[0].0, "张三");
        assert!(findings3[0].2);
    }

    #[test]
    fn test_meta_json_simulation() {
        let meta = r#"{
  "licenseType": "CC BY-NC-SA",
  "creatorName": "Dnaddr",
  "packageName": "Mica_v2_P",
  "programVersion": "1.22.0.3",
  "contentList": ["Saves/scene/scene.json"],
  "dependencies": {}
}"#;
        let watermarked = JsonWatermarker::embed(meta, "购买者:张三", DEFAULT_WATERMARK_KEY, "md5", None).unwrap();
        let extracted = JsonWatermarker::extract(&watermarked, DEFAULT_WATERMARK_KEY).unwrap();

        let parsed: Value = serde_json::from_str(&watermarked).unwrap();
        assert_eq!(parsed["licenseType"], "CC BY-NC-SA");
        assert_eq!(parsed["creatorName"], "Dnaddr");
        assert!(parsed["contentList"].is_array());

        let expected = WatermarkEncoder::encode("购买者:张三").md5_hash;
        assert_eq!(extracted, expected);
    }
}
