//! 稳定的内容指纹。
//!
//! 只生成 `sha256:` 前缀标签，不读文件、不碰凭据。
//! 由隔离配置投影和启动准备共用。

use sha2::{Digest, Sha256};

/// 把字节写成 `sha256:` 加小写十六进制。
pub fn sha256_label(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    format!("sha256:{encoded}")
}
