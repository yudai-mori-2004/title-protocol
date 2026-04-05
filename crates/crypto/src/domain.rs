// SPDX-License-Identifier: Apache-2.0

//! # ドメインタグ
//!
//! cross-protocol攻撃防止のための署名対象プレフィクス。
//! Ed25519 (RFC 8032 §5.1.6) はドメイン分離を提供しないため、
//! アプリケーション層で明示的にタグを付与する。

/// 署名対象メッセージにドメインプレフィクスを付与する。
///
/// フォーマット: `[4B: domain_len (BE u32)][domain bytes][message bytes]`
///
/// # 用途
/// - Protocol署名: `domain_tagged("title-protocol-v1", &sign_bytes)`
/// - Solana TX署名: タグ不可（Solanaフォーマット固定）。Protocol側にタグがあれば
///   Solana TX署名→Protocol署名への流用は防止される。
pub fn domain_tagged(domain: &str, message: &[u8]) -> Vec<u8> {
    let mut tagged = Vec::with_capacity(4 + domain.len() + message.len());
    tagged.extend_from_slice(&(domain.len() as u32).to_be_bytes());
    tagged.extend_from_slice(domain.as_bytes());
    tagged.extend_from_slice(message);
    tagged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn different_domains_produce_different_tags() {
        let msg = b"same message";
        let t1 = domain_tagged("domain-a", msg);
        let t2 = domain_tagged("domain-b", msg);
        assert_ne!(t1, t2);
    }

    #[test]
    fn format_is_correct() {
        let tagged = domain_tagged("abc", b"xyz");
        // 4 bytes length (0,0,0,3) + "abc" (3) + "xyz" (3) = 10 bytes
        assert_eq!(tagged.len(), 10);
        assert_eq!(&tagged[0..4], &[0, 0, 0, 3]);
        assert_eq!(&tagged[4..7], b"abc");
        assert_eq!(&tagged[7..10], b"xyz");
    }
}
