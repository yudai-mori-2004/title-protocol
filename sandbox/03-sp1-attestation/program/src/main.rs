//! # SP1 Guest: AWS Nitro Attestation Document 検証
//!
//! 仕様書 §6.2 — Solana Extension 準備（TEEインスタンスごとに一度）
//!
//! このプログラムは SP1 zkVM 内で実行され、以下を証明する:
//! 1. Attestation Document の COSE_Sign1 署名が正当である
//! 2. 証明書チェーンが AWS Nitro PKI ルートまで検証可能である
//! 3. 抽出された PCR 値・user_data・public_key が改ざんされていない
//!
//! 公開値（proof の一部として公開）:
//! - module_id: エンクレーブモジュール識別子
//! - timestamp: Attestation 生成時刻（Unix ms）
//! - pcr0: エンクレーブイメージハッシュ（48バイト）
//! - user_data_hash: user_data の SHA-256 ハッシュ（存在する場合）
//! - public_key_hash: public_key の SHA-256 ハッシュ（存在する場合）

#![no_main]
sp1_zkvm::entrypoint!(main);

use attestation_verify::AttestationReport;
use sha2::{Digest, Sha256};

pub fn main() {
    // ── 入力読み取り ──
    // 入力1: Attestation Document バイト列（COSE_Sign1 エンコード）
    let doc_bytes: Vec<u8> = sp1_zkvm::io::read_vec();
    // 入力2: 信頼済み証明書プレフィックス長（0 = チェーン全体を検証）
    let trusted_certs_prefix_len: u32 = sp1_zkvm::io::read();

    // ── Phase 1: COSE_Sign1 パース ──
    let report =
        AttestationReport::parse(&doc_bytes).expect("COSE_Sign1 パースに失敗");
    let doc = report.doc();

    // ── Phase 2: 証明書チェーン検証 + COSE 署名検証 ──
    // authenticate() は以下を実行:
    //   1. cabundle + certificate から証明書チェーンを構築
    //   2. 各証明書の署名を親証明書の公開鍵で検証（ECDSA P-384）
    //   3. 証明書の有効期限を timestamp で検証
    //   4. リーフ証明書の公開鍵で COSE_Sign1 署名を検証（ES384）
    let _cert_chain = report
        .authenticate(
            trusted_certs_prefix_len as usize,
            doc.timestamp / 1000, // ms → s
        )
        .expect("Attestation Document の検証に失敗");

    // ── Phase 3: 検証済みデータの抽出とコミット ──

    // module_id: エンクレーブインスタンス識別子
    sp1_zkvm::io::commit(&doc.module_id);

    // timestamp: Attestation 生成時刻
    sp1_zkvm::io::commit(&doc.timestamp);

    // PCR0: エンクレーブイメージハッシュ（48バイト SHA-384）
    let pcr0 = doc.pcrs.get(&0).expect("PCR0 が存在しない");
    sp1_zkvm::io::commit_slice(pcr0.as_ref());

    // user_data: SHA-256 ハッシュをコミット（存在フラグ + ハッシュ）
    let has_user_data: u8 = if doc.user_data.is_some() { 1 } else { 0 };
    sp1_zkvm::io::commit(&has_user_data);
    if let Some(ref ud) = doc.user_data {
        let hash = Sha256::digest(ud.as_ref());
        sp1_zkvm::io::commit_slice(&hash);
    }

    // public_key: SHA-256 ハッシュをコミット（存在フラグ + ハッシュ）
    let has_public_key: u8 = if doc.public_key.is_some() { 1 } else { 0 };
    sp1_zkvm::io::commit(&has_public_key);
    if let Some(ref pk) = doc.public_key {
        let hash = Sha256::digest(pk.as_ref());
        sp1_zkvm::io::commit_slice(&hash);
    }
}
