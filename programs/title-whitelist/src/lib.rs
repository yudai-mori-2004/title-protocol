// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol Whitelist Program
//!
//! Spec §6.2 — ZK proof で検証された TEE 署名鍵のオンチェーンレジストリ。
//!
//! ホワイトリスト PDA は Solana プログラムが管理するオンチェーンアカウントである。
//! 更新権限はプログラムのみが持ち、ZK proof の検証に成功した場合にのみ
//! 新しい署名鍵を追加できる。人手による管理は介在しない。
//!
//! ## 信頼モデル
//!
//! cNFT の信頼判定は一点に帰着する:
//! 「発行トランザクションに、ホワイトリスト済みの署名鍵の署名が含まれているか」
//!
//! ホワイトリスト PDA は事後検証用の記録であり、mint トランザクションの
//! パス上には存在しない。mint のアクセス制御は Bubblegum の tree 設定
//! （tree_creator_or_delegate）で行い、信頼の担保は TEE コード自体
//! （PCR 値で証明済み）が Attestation 検証後にのみ署名する設計による。

use anchor_lang::prelude::*;
use sha2::{Digest, Sha256};

declare_id!("43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs");

/// Spec §6.2 — 署名鍵の有効期限（90日）
pub const KEY_EXPIRY_SECONDS: i64 = 90 * 24 * 60 * 60;

/// SP1 v6.2 Groth16 verification key (492 bytes).
/// Extracted from sp1-verifier 6.2.2 vk-artifacts/groth16_vk.bin.
pub const GROTH16_VK_BYTES: &[u8] = include_bytes!("../vk/groth16_vk_v6.2.bin");

/// Admin authority pubkey: wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna
/// Phase 1: single wallet. Future: multi-sig / DAO migration.
pub const ADMIN_AUTHORITY: [u8; 32] = [
    14, 13, 85, 28, 133, 146, 12, 228, 183, 160, 156, 77, 30, 213, 163, 160,
    181, 106, 231, 149, 205, 50, 104, 222, 122, 121, 156, 214, 103, 125, 184, 3,
];

#[program]
pub mod title_whitelist {
    use super::*;

    /// TEE 署名鍵を ZK proof 検証により登録する。
    /// Spec §6.2 — ZK proof を Solana プログラムに提出 → 検証に成功すれば登録
    ///
    /// # Flow
    /// 1. SP1 Groth16 proof を検証（attestation-program の実行結果）
    /// 2. 公開値から pcr0, user_data_hash を抽出
    /// 3. user_data_hash == SHA-256(SHA-256(signing_pubkey)) を確認
    ///    （Attestation Document の user_data = SHA-256(Solana公開鍵) であり、
    ///     guest は SHA-256(user_data) をコミットするため二重ハッシュ）
    /// 4. WhitelistEntry PDA を作成
    pub fn register_key(
        ctx: Context<RegisterKey>,
        signing_pubkey: [u8; 32],
        sp1_vkey_hash: [u8; 32],
        proof: Vec<u8>,
        public_values: Vec<u8>,
    ) -> Result<()> {
        // Step 1: SP1 Groth16 proof verification
        verify_sp1_groth16(&sp1_vkey_hash, &proof, &public_values)?;

        // Step 2: Parse and validate public values
        let parsed = parse_public_values(&public_values)?;

        // Step 3: Verify signing_pubkey ↔ user_data_hash binding
        // Attestation Document: user_data = SHA-256(signing_pubkey)
        // Guest commits: user_data_hash = SHA-256(user_data) = SHA-256(SHA-256(signing_pubkey))
        require!(parsed.has_user_data, WhitelistError::MissingUserData);

        let user_data = Sha256::digest(signing_pubkey);
        let expected_hash = Sha256::digest(user_data);
        require!(
            parsed.user_data_hash == expected_hash.as_slice(),
            WhitelistError::UserDataMismatch
        );

        // Step 4: Create PDA
        let clock = Clock::get()?;
        let entry = &mut ctx.accounts.whitelist_entry;
        entry.signing_pubkey = signing_pubkey;
        entry.registered_at = clock.unix_timestamp;
        entry.expires_at = clock.unix_timestamp + KEY_EXPIRY_SECONDS;
        entry.pcr0 = parsed.pcr0;
        entry.bump = ctx.bumps.whitelist_entry;

        emit!(KeyRegistered {
            signing_pubkey: Pubkey::new_from_array(signing_pubkey),
            pcr0: parsed.pcr0,
            expires_at: entry.expires_at,
        });

        Ok(())
    }

    /// ホワイトリストから署名鍵を削除する（管理者の緊急操作のみ）。
    /// Spec §6.2 — TEE の侵害等、特別な事情が発生した場合にのみ
    ///
    /// 通常運用では鍵の削除は行わない。削除された鍵で過去に発行された
    /// cNFT はブロックチェーン上に残り続ける。
    pub fn delete_key(ctx: Context<DeleteKey>) -> Result<()> {
        emit!(KeyDeleted {
            signing_pubkey: Pubkey::new_from_array(
                ctx.accounts.whitelist_entry.signing_pubkey,
            ),
        });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SP1 Groth16 proof verification
// ---------------------------------------------------------------------------

/// Verify an SP1 Groth16 proof using the embedded v6.2 verification key.
///
/// Uses sp1-solana's `verify_proof_raw` which calls Solana's alt_bn128
/// precompile syscalls for efficient on-chain BN254 pairing verification.
///
/// We avoid the higher-level `verify_proof` API because it requires a hex
/// string conversion (wasteful on-chain). Instead we reconstruct the
/// Groth16 public inputs directly from raw bytes.
fn verify_sp1_groth16(
    sp1_vkey_hash: &[u8; 32],
    proof: &[u8],
    public_values: &[u8],
) -> Result<()> {
    require!(!proof.is_empty(), WhitelistError::EmptyProof);
    require!(
        !public_values.is_empty(),
        WhitelistError::EmptyPublicValues
    );

    // SP1 prepends 4 bytes of SHA-256(groth16_vk) to the proof for VK binding.
    require!(proof.len() > 4, WhitelistError::EmptyProof);
    let groth16_vk_hash: [u8; 4] = Sha256::digest(GROTH16_VK_BYTES)[..4]
        .try_into()
        .unwrap();
    require!(
        proof[..4] == groth16_vk_hash,
        WhitelistError::ProofVerificationFailed
    );

    // Groth16 public inputs = vkey_hash[1..32] || committed_values_digest
    // committed_values_digest = SHA-256(public_values) with top 3 bits zeroed
    // (BN254 operates over a 254-bit field)
    let mut committed_values_digest = Sha256::digest(public_values);
    committed_values_digest[0] &= 0x1F;

    let mut groth16_inputs = [0u8; 63];
    groth16_inputs[..31].copy_from_slice(&sp1_vkey_hash[1..]);
    groth16_inputs[31..].copy_from_slice(&committed_values_digest);

    sp1_solana::verify_proof_raw(&proof[4..], &groth16_inputs, GROTH16_VK_BYTES)
        .map_err(|_| error!(WhitelistError::ProofVerificationFailed))
}

// ---------------------------------------------------------------------------
// Public values parser
// ---------------------------------------------------------------------------

/// Structure committed by the SP1 attestation-program guest.
/// See sandbox/03-sp1-attestation/program/src/main.rs.
struct ParsedPublicValues {
    #[allow(dead_code)]
    module_id_len: usize,
    pcr0: [u8; 48],
    has_user_data: bool,
    user_data_hash: Vec<u8>,
}

/// Parse the public values byte array.
///
/// Layout (Borsh-encoded by SP1):
///   module_id: String (len: u32 + utf8 bytes)
///   timestamp: u64
///   pcr0: [u8; 48]
///   has_user_data: u8
///   user_data_hash: [u8; 32] (if has_user_data == 1)
///   has_public_key: u8
///   public_key_hash: [u8; 32] (if has_public_key == 1)
fn parse_public_values(data: &[u8]) -> Result<ParsedPublicValues> {
    let mut offset = 0;

    // module_id: String (u32 len prefix + bytes)
    require!(data.len() >= offset + 4, WhitelistError::InvalidPublicValues);
    let module_id_len =
        u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
    offset += 4 + module_id_len;

    // timestamp: u64
    require!(
        data.len() >= offset + 8,
        WhitelistError::InvalidPublicValues
    );
    offset += 8;

    // pcr0: [u8; 48]
    require!(
        data.len() >= offset + 48,
        WhitelistError::InvalidPublicValues
    );
    let mut pcr0 = [0u8; 48];
    pcr0.copy_from_slice(&data[offset..offset + 48]);
    offset += 48;

    // has_user_data: u8
    require!(
        data.len() >= offset + 1,
        WhitelistError::InvalidPublicValues
    );
    let has_user_data = data[offset] == 1;
    offset += 1;

    let mut user_data_hash = Vec::new();
    if has_user_data {
        require!(
            data.len() >= offset + 32,
            WhitelistError::InvalidPublicValues
        );
        user_data_hash = data[offset..offset + 32].to_vec();
    }

    Ok(ParsedPublicValues {
        module_id_len,
        pcr0,
        has_user_data,
        user_data_hash,
    })
}

// ---------------------------------------------------------------------------
// Account structures
// ---------------------------------------------------------------------------

/// Whitelist PDA. Registration record for a TEE signing key.
/// Spec §6.2
///
/// Seeds: `[b"whitelist", signing_pubkey.as_ref()]`
///
/// Created only when ZK proof verification succeeds.
/// Expired signing keys cannot mint new cNFTs, but cNFTs
/// minted within the validity period remain permanently valid.
#[account]
pub struct WhitelistEntry {
    /// TEE Ed25519 signing public key (32 bytes)
    pub signing_pubkey: [u8; 32],
    /// Registration time (Unix timestamp)
    pub registered_at: i64,
    /// Mint validity expiry (Unix timestamp)
    pub expires_at: i64,
    /// Attestation Document PCR0 (48 bytes, SHA-384)
    /// Identifies the TEE enclave image
    pub pcr0: [u8; 48],
    /// PDA bump seed
    pub bump: u8,
}

impl WhitelistEntry {
    /// discriminator(8) + signing_pubkey(32) + registered_at(8) + expires_at(8) + pcr0(48) + bump(1)
    pub const SIZE: usize = 8 + 32 + 8 + 8 + 48 + 1;
}

// ---------------------------------------------------------------------------
// Context structures
// ---------------------------------------------------------------------------

/// RegisterKey instruction accounts.
#[derive(Accounts)]
#[instruction(signing_pubkey: [u8; 32])]
pub struct RegisterKey<'info> {
    #[account(
        init,
        payer = payer,
        space = WhitelistEntry::SIZE,
        seeds = [b"whitelist", signing_pubkey.as_ref()],
        bump
    )]
    pub whitelist_entry: Account<'info, WhitelistEntry>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

/// DeleteKey instruction accounts (admin emergency operation).
#[derive(Accounts)]
pub struct DeleteKey<'info> {
    #[account(
        mut,
        close = admin,
        seeds = [b"whitelist", whitelist_entry.signing_pubkey.as_ref()],
        bump = whitelist_entry.bump
    )]
    pub whitelist_entry: Account<'info, WhitelistEntry>,
    #[account(
        mut,
        constraint = admin.key() == admin_authority() @ WhitelistError::Unauthorized
    )]
    pub admin: Signer<'info>,
}

fn admin_authority() -> Pubkey {
    Pubkey::new_from_array(ADMIN_AUTHORITY)
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[event]
pub struct KeyRegistered {
    pub signing_pubkey: Pubkey,
    pub pcr0: [u8; 48],
    pub expires_at: i64,
}

#[event]
pub struct KeyDeleted {
    pub signing_pubkey: Pubkey,
}

// ---------------------------------------------------------------------------
// Error codes
// ---------------------------------------------------------------------------

#[error_code]
pub enum WhitelistError {
    #[msg("SP1 proof is empty")]
    EmptyProof,
    #[msg("Public values are empty")]
    EmptyPublicValues,
    #[msg("SP1 Groth16 proof verification failed")]
    ProofVerificationFailed,
    #[msg("Public values format is invalid")]
    InvalidPublicValues,
    #[msg("Attestation Document has no user_data")]
    MissingUserData,
    #[msg("user_data hash does not match signing_pubkey")]
    UserDataMismatch,
    #[msg("Unauthorized: not the admin authority")]
    Unauthorized,
}
