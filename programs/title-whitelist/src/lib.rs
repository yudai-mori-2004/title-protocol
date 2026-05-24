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

/// Admin authority pubkey: wrVwsTuRzbsDutybqqpf9tBE7JUqRPYzJ3iPUgcFmna.
///
/// Phase 1: single wallet. Future: multi-sig / DAO migration plan:
///   A) replace this with an on-chain `admin_authority` PDA owned by a
///      Squads-style multisig program, and
///   B) add `transfer_admin(new_admin)` ix gated by the current admin
///      signature so rotation no longer requires a program upgrade.
/// Until that lands, rotation requires `anchor upgrade` by the deploy key.
pub const ADMIN_AUTHORITY: Pubkey = Pubkey::new_from_array([
    14, 13, 85, 28, 133, 146, 12, 228, 183, 160, 156, 77, 30, 213, 163, 160,
    181, 106, 231, 149, 205, 50, 104, 222, 122, 121, 156, 214, 103, 125, 184, 3,
]);

#[program]
pub mod title_whitelist {
    use super::*;

    // -------- ApprovedVkeys (検証回路の指紋集合) --------

    /// Approved-vkeys レジストリを初期化する（プログラムごとに1回）。
    /// Spec §6.2 — 「Solanaプログラムは、許容するverifying_key_hash
    /// の集合をオンチェーンに保持する」
    pub fn initialize_approved_vkeys(ctx: Context<InitializeApprovedVkeys>) -> Result<()> {
        let registry = &mut ctx.accounts.approved_vkeys;
        registry.admin = ctx.accounts.admin.key();
        registry.vkeys = Vec::new();
        registry.bump = ctx.bumps.approved_vkeys;
        emit!(ApprovedVkeysInitialized {
            admin: registry.admin
        });
        Ok(())
    }

    /// 許容する verifying_key_hash を追加する（admin のみ）。
    /// 新しい SP1 guest をリリースした際に呼び出す。
    pub fn add_approved_vkey(
        ctx: Context<UpdateApprovedVkeys>,
        vkey_hash: [u8; 32],
    ) -> Result<()> {
        let registry = &mut ctx.accounts.approved_vkeys;
        require!(
            !registry.vkeys.contains(&vkey_hash),
            WhitelistError::VkeyAlreadyApproved
        );
        require!(
            registry.vkeys.len() < ApprovedVkeys::MAX_VKEYS,
            WhitelistError::VkeyRegistryFull
        );
        registry.vkeys.push(vkey_hash);
        emit!(VkeyApproved { vkey_hash });
        Ok(())
    }

    /// 許容 verifying_key_hash を取り除く（admin のみ）。
    pub fn remove_approved_vkey(
        ctx: Context<UpdateApprovedVkeys>,
        vkey_hash: [u8; 32],
    ) -> Result<()> {
        let registry = &mut ctx.accounts.approved_vkeys;
        let len_before = registry.vkeys.len();
        registry.vkeys.retain(|v| v != &vkey_hash);
        require!(
            registry.vkeys.len() < len_before,
            WhitelistError::VkeyNotApproved
        );
        emit!(VkeyRevoked { vkey_hash });
        Ok(())
    }

    // -------- ApprovedMeasurements (TEE バイナリの指紋集合) --------

    /// Approved-measurements レジストリを初期化する（プログラムごとに1回）。
    /// Spec §6.2 — 「許容する measurement の集合もオンチェーンに保持する」
    pub fn initialize_approved_measurements(
        ctx: Context<InitializeApprovedMeasurements>,
    ) -> Result<()> {
        let registry = &mut ctx.accounts.approved_measurements;
        registry.admin = ctx.accounts.admin.key();
        registry.entries = Vec::new();
        registry.bump = ctx.bumps.approved_measurements;
        emit!(ApprovedMeasurementsInitialized {
            admin: registry.admin
        });
        Ok(())
    }

    /// 許容する TEE measurement を追加する（admin のみ）。
    /// 新しい TEE バイナリをリリースした際に呼び出す。
    pub fn add_approved_measurement(
        ctx: Context<UpdateApprovedMeasurements>,
        measurement: Vec<u8>,
    ) -> Result<()> {
        require!(
            (1..=MAX_MEASUREMENT_LEN).contains(&measurement.len()),
            WhitelistError::InvalidMeasurementLen
        );
        let stored = StoredMeasurement::from_slice(&measurement);
        let registry = &mut ctx.accounts.approved_measurements;
        require!(
            !registry.entries.iter().any(|e| e == &stored),
            WhitelistError::MeasurementAlreadyApproved
        );
        require!(
            registry.entries.len() < ApprovedMeasurements::MAX_ENTRIES,
            WhitelistError::MeasurementRegistryFull
        );
        registry.entries.push(stored);
        emit!(MeasurementApproved { measurement });
        Ok(())
    }

    /// 許容 measurement を取り除く（admin のみ）。
    pub fn remove_approved_measurement(
        ctx: Context<UpdateApprovedMeasurements>,
        measurement: Vec<u8>,
    ) -> Result<()> {
        require!(
            (1..=MAX_MEASUREMENT_LEN).contains(&measurement.len()),
            WhitelistError::InvalidMeasurementLen
        );
        let stored = StoredMeasurement::from_slice(&measurement);
        let registry = &mut ctx.accounts.approved_measurements;
        let len_before = registry.entries.len();
        registry.entries.retain(|e| e != &stored);
        require!(
            registry.entries.len() < len_before,
            WhitelistError::MeasurementNotApproved
        );
        emit!(MeasurementRevoked { measurement });
        Ok(())
    }

    // -------- Signing key registration --------

    /// TEE 署名鍵を ZK proof 検証により登録する。
    /// Spec §6.2 — 二段の同一性確認 + proof 検証 + bind 確認
    ///
    /// # Flow
    /// 1. sp1_vkey_hash が ApprovedVkeys に含まれることを確認
    ///    （正規の検証回路で生成された proof のみ受理）
    /// 2. SP1 Groth16 proof を検証
    /// 3. 公開値から measurement, user_data_hash を抽出
    /// 4. measurement が ApprovedMeasurements に含まれることを確認
    ///    （正規の TEE バイナリで生成された Attestation のみ受理）
    /// 5. user_data_hash == SHA-256(SHA-256(signing_pubkey)) を確認
    ///    （Attestation の user_data = SHA-256(Solana公開鍵)、
    ///     guest は SHA-256(user_data) をコミットするので二重ハッシュ）
    /// 6. WhitelistEntry PDA を作成
    pub fn register_key(
        ctx: Context<RegisterKey>,
        signing_pubkey: [u8; 32],
        sp1_vkey_hash: [u8; 32],
        proof: Vec<u8>,
        public_values: Vec<u8>,
    ) -> Result<()> {
        // Order the checks so a malformed/spoofed input fails before we
        // burn the ~250K CU that the Groth16 pairing costs. Spec §6.2 lets
        // the four substantive checks run in any order; this just keeps
        // them DoS-resistant.

        // Step 1: vkey allowlist (cheap eq lookup).
        require!(
            ctx.accounts.approved_vkeys.vkeys.contains(&sp1_vkey_hash),
            WhitelistError::VkeyNotApproved
        );

        // Step 2: parse public values up front.
        let parsed = parse_public_values(&public_values)?;

        // Step 3: measurement allowlist (cheap eq).
        let candidate = StoredMeasurement::from_slice(parsed.measurement);
        require!(
            ctx.accounts
                .approved_measurements
                .entries
                .iter()
                .any(|e| e == &candidate),
            WhitelistError::MeasurementNotApproved
        );

        // Step 4: signing_pubkey ↔ user_data_hash binding (two SHA-256s).
        require!(parsed.has_user_data, WhitelistError::MissingUserData);
        let user_data = Sha256::digest(signing_pubkey);
        let expected_hash = Sha256::digest(user_data);
        require!(
            parsed.user_data_hash == expected_hash.as_slice(),
            WhitelistError::UserDataMismatch
        );

        // Step 5: Groth16 verify — the expensive step, last.
        verify_sp1_groth16(&sp1_vkey_hash, &proof, &public_values)?;

        // Step 6: Create PDA. The single `Vec` alloc this whole instruction
        // makes is the one for the `KeyRegistered` event — everything before
        // here stays in borrowed slices.
        let clock = Clock::get()?;
        let entry = &mut ctx.accounts.whitelist_entry;
        entry.signing_pubkey = signing_pubkey;
        entry.registered_at = clock.unix_timestamp;
        entry.expires_at = clock
            .unix_timestamp
            .checked_add(KEY_EXPIRY_SECONDS)
            .ok_or(error!(WhitelistError::TimestampOverflow))?;
        entry.measurement = candidate;
        entry.revoked = false;
        entry.bump = ctx.bumps.whitelist_entry;

        emit!(KeyRegistered {
            signing_pubkey: Pubkey::new_from_array(signing_pubkey),
            measurement: parsed.measurement.to_vec(),
            expires_at: entry.expires_at,
        });

        Ok(())
    }

    /// ホワイトリスト鍵を取り消す（管理者の緊急操作のみ）。
    /// Spec §6.2 — TEE の侵害等、特別な事情が発生した場合にのみ
    ///
    /// PDA は **削除せず**、`revoked = true` を立てるだけにする。
    /// PDA を close すると `register_key` の `init` constraint を素通りして
    /// 同じ proof+public_values で再登録できてしまい、admin の取消操作が
    /// 巻き戻されてしまうため。
    pub fn revoke_key(ctx: Context<RevokeKey>) -> Result<()> {
        let entry = &mut ctx.accounts.whitelist_entry;
        require!(!entry.revoked, WhitelistError::AlreadyRevoked);
        entry.revoked = true;
        emit!(KeyRevoked {
            signing_pubkey: Pubkey::new_from_array(entry.signing_pubkey),
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
    require!(
        !public_values.is_empty(),
        WhitelistError::EmptyPublicValues
    );

    // SP1 prepends 4 bytes of SHA-256(groth16_vk) to the proof; the
    // remainder is the Groth16 proof (256 bytes: pi_a 64 + pi_b 128 +
    // pi_c 64). Verify the length up front so `verify_proof_raw` cannot
    // hit a bounded-array slice panic on a truncated input.
    require!(
        proof.len() == 4 + 256,
        WhitelistError::InvalidProofLength
    );
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

/// Decoded subset of the SP1 guest's public values that this program needs.
/// The guest's full layout is documented in
/// `sp1-guests/attestation-aws-nitro/program/src/main.rs`; here we only
/// retain the fields used for the four register_key checks.
struct ParsedPublicValues<'a> {
    measurement: &'a [u8],
    has_user_data: bool,
    user_data_hash: &'a [u8],
}

/// Parse the SP1 public values.
///
/// Layout (matches what every Title Protocol attestation guest emits):
///   instance_id_len  : u32 LE
///   instance_id      : instance_id_len bytes (UTF-8)
///   timestamp_ms     : u64 LE
///   measurement_len  : u32 LE
///   measurement      : measurement_len bytes
///   has_user_data    : u8
///   user_data_hash   : 32 bytes (only if has_user_data == 1)
///   has_public_key   : u8
///   public_key_hash  : 32 bytes (only if has_public_key == 1)
///
/// 長さは u32 LE で固定する。SP1 guest 側は `commit(&String)` を使うと
/// 内部の bincode-fixint シリアライザが u64 LE 長前置を出してしまい、
/// ここの u32 読み出しとずれる。guest では `commit(&len_u32)` +
/// `commit_slice(bytes)` の手動書き出しで揃えること
/// (`sp1-guests/attestation-aws-nitro/program/src/main.rs` を参照)。
fn parse_public_values(data: &[u8]) -> Result<ParsedPublicValues<'_>> {
    let mut offset = 0;

    // instance_id: u32 LE length + UTF-8 bytes — skipped, length-validated only.
    require!(data.len() >= offset + 4, WhitelistError::InvalidPublicValues);
    let id_len = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
    offset += 4;
    require!(
        data.len() >= offset + id_len,
        WhitelistError::InvalidPublicValues
    );
    offset += id_len;

    // timestamp_ms: u64
    require!(
        data.len() >= offset + 8,
        WhitelistError::InvalidPublicValues
    );
    offset += 8;

    // measurement_len: u32, then measurement bytes.
    require!(
        data.len() >= offset + 4,
        WhitelistError::InvalidPublicValues
    );
    let measurement_len =
        u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
    offset += 4;
    require!(
        (1..=MAX_MEASUREMENT_LEN).contains(&measurement_len),
        WhitelistError::InvalidMeasurementLen
    );
    require!(
        data.len() >= offset + measurement_len,
        WhitelistError::InvalidPublicValues
    );
    let measurement = &data[offset..offset + measurement_len];
    offset += measurement_len;

    // has_user_data: u8 — must be canonical 0/1. Treating any non-1 value
    // as `false` would let a SP1 guest with a subtly wrong commit layout
    // pass on-chain, so we reject anything that isn't a Borsh boolean.
    require!(
        data.len() >= offset + 1,
        WhitelistError::InvalidPublicValues
    );
    require!(data[offset] <= 1, WhitelistError::InvalidPublicValues);
    let has_user_data = data[offset] == 1;
    offset += 1;

    let user_data_hash: &[u8] = if has_user_data {
        require!(
            data.len() >= offset + 32,
            WhitelistError::InvalidPublicValues
        );
        let slice = &data[offset..offset + 32];
        offset += 32;
        slice
    } else {
        &[]
    };

    // has_public_key + (optional) public_key_hash. We don't use these
    // values, but parse them so trailing garbage in the public_values
    // buffer is detected.
    require!(
        data.len() >= offset + 1,
        WhitelistError::InvalidPublicValues
    );
    require!(data[offset] <= 1, WhitelistError::InvalidPublicValues);
    let has_public_key = data[offset] == 1;
    offset += 1;
    if has_public_key {
        require!(
            data.len() >= offset + 32,
            WhitelistError::InvalidPublicValues
        );
        offset += 32;
    }

    // The guest is supposed to commit exactly the public-values envelope
    // documented in `sp1-guests/.../program/src/main.rs`. Anything past
    // the end means the layout changed without the parser catching up.
    require!(data.len() == offset, WhitelistError::InvalidPublicValues);

    Ok(ParsedPublicValues {
        measurement,
        has_user_data,
        user_data_hash,
    })
}

// ---------------------------------------------------------------------------
// Account structures
// ---------------------------------------------------------------------------

/// Maximum allowed TEE measurement length, in bytes.
///
/// 64 bytes comfortably accommodates every TEE vendor we plan to support
/// (AWS Nitro PCR0 = 48B, AMD SEV-SNP report measurement = 48B, Intel TDX
/// MRTD = 48B; GCP Confidential Space may emit up to 64B in some modes).
/// A new vendor with a longer measurement requires bumping this constant
/// and migrating PDAs.
pub const MAX_MEASUREMENT_LEN: usize = 64;

/// Fixed-size on-chain representation of a vendor-neutral measurement.
///
/// Storing as `[u8; 64] + u8 len` (instead of `Vec<u8>`) keeps account sizes
/// stable and equality comparisons trivial. `bytes[len..]` is zero-padded
/// but never inspected; `bytes[..len]` carries the actual measurement.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoredMeasurement {
    pub bytes: [u8; MAX_MEASUREMENT_LEN],
    pub len: u8,
}

impl StoredMeasurement {
    /// Build from a slice. The caller is responsible for length validation
    /// (instructions enforce `1..=MAX_MEASUREMENT_LEN`); excessive input is
    /// truncated here as a defensive measure to keep equality well-defined.
    pub fn from_slice(input: &[u8]) -> Self {
        // Length should already be validated by the caller
        // (`parse_public_values` rejects > MAX_MEASUREMENT_LEN). The
        // `debug_assert` makes the invariant explicit while the runtime
        // `min` keeps us in-bounds even if a future caller forgets.
        debug_assert!(
            input.len() <= MAX_MEASUREMENT_LEN,
            "StoredMeasurement::from_slice received {} bytes (max {})",
            input.len(),
            MAX_MEASUREMENT_LEN
        );
        let mut bytes = [0u8; MAX_MEASUREMENT_LEN];
        let take = input.len().min(MAX_MEASUREMENT_LEN);
        bytes[..take].copy_from_slice(&input[..take]);
        Self {
            bytes,
            len: take as u8,
        }
    }

    /// View as the actual measurement slice (excluding zero padding).
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

/// Whitelist PDA. Registration record for a TEE signing key.
/// Spec §6.2
///
/// Seeds: `[b"whitelist", signing_pubkey.as_ref()]`
///
/// Created only when all four register_key checks pass (vkey allowlist,
/// proof verification, measurement allowlist, user_data binding).
/// Expired signing keys cannot mint new cNFTs, but cNFTs minted within
/// the validity period remain permanently valid.
#[account]
pub struct WhitelistEntry {
    /// TEE Ed25519 signing public key (32 bytes)
    pub signing_pubkey: [u8; 32],
    /// Registration time (Unix timestamp)
    pub registered_at: i64,
    /// Mint validity expiry (Unix timestamp)
    pub expires_at: i64,
    /// TEE measurement at the moment of registration. Vendor-neutral
    /// fixed-size container so this struct's size is constant.
    pub measurement: StoredMeasurement,
    /// Set to `true` by `revoke_key` after a TEE compromise; never reset
    /// to `false`. Applications consulting the whitelist must treat a
    /// revoked entry as untrusted, even if `expires_at` has not yet passed.
    pub revoked: bool,
    /// PDA bump seed
    pub bump: u8,
}

impl WhitelistEntry {
    /// discriminator(8) + signing_pubkey(32) + registered_at(8)
    ///   + expires_at(8) + measurement(64 + 1) + revoked(1) + bump(1)
    pub const SIZE: usize = 8 + 32 + 8 + 8 + MAX_MEASUREMENT_LEN + 1 + 1 + 1;
}

/// Registry of SP1 verifying-key hashes that the program will accept proofs for.
/// Spec §6.2 — admin が SP1 guest コードの更新に合わせて vkey を追加・削除する。
///
/// Seeds: `[b"approved_vkeys"]` (singleton)
#[account]
pub struct ApprovedVkeys {
    /// 管理者公開鍵。`ADMIN_AUTHORITY` と同じ値を保持する。
    pub admin: Pubkey,
    /// 許容する vkey_hash のリスト。SP1 guest を1つしか持たない通常運用では
    /// 1 要素だが、guest のバージョン切替時には新旧 2 要素併存させる。
    pub vkeys: Vec<[u8; 32]>,
    /// PDA bump seed.
    pub bump: u8,
}

impl ApprovedVkeys {
    /// Upper bound for the registry. Each entry is 32 bytes; 16 entries are
    /// well within Solana's account-size limit and far above the number of
    /// SP1 guest versions we expect to track at once.
    pub const MAX_VKEYS: usize = 16;
    /// discriminator(8) + admin(32) + vec_len(4) + vkeys(32 * MAX) + bump(1)
    pub const SIZE: usize = 8 + 32 + 4 + 32 * Self::MAX_VKEYS + 1;
}

/// Registry of TEE measurements the program will accept attestations from.
/// Spec §6.2 — admin が TEE バイナリの更新に合わせて measurement を追加・削除する。
///
/// Seeds: `[b"approved_measurements"]` (singleton)
#[account]
pub struct ApprovedMeasurements {
    pub admin: Pubkey,
    pub entries: Vec<StoredMeasurement>,
    pub bump: u8,
}

impl ApprovedMeasurements {
    /// 16 entries is enough for normal operation (rolling 2-3 TEE versions
    /// concurrently) with plenty of headroom for migrations.
    pub const MAX_ENTRIES: usize = 16;
    /// Per-entry serialized size: MAX_MEASUREMENT_LEN bytes + 1 byte length.
    const ENTRY_SIZE: usize = MAX_MEASUREMENT_LEN + 1;
    /// discriminator(8) + admin(32) + vec_len(4) + entries(ENTRY_SIZE * MAX) + bump(1)
    pub const SIZE: usize = 8 + 32 + 4 + Self::ENTRY_SIZE * Self::MAX_ENTRIES + 1;
}

// ---------------------------------------------------------------------------
// Context structures
// ---------------------------------------------------------------------------

/// InitializeApprovedVkeys instruction accounts.
#[derive(Accounts)]
pub struct InitializeApprovedVkeys<'info> {
    #[account(
        init,
        payer = admin,
        space = ApprovedVkeys::SIZE,
        seeds = [b"approved_vkeys"],
        bump
    )]
    pub approved_vkeys: Account<'info, ApprovedVkeys>,
    #[account(
        mut,
        constraint = admin.key() == ADMIN_AUTHORITY @ WhitelistError::Unauthorized
    )]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

/// AddApprovedVkey / RemoveApprovedVkey instruction accounts.
///
/// Two admin checks in series: `has_one = admin` proves the signer matches
/// the PDA-recorded admin, plus the explicit `constraint = ADMIN_AUTHORITY`
/// keeps the program-level invariant alive even if a future migration ever
/// reassigns `approved_vkeys.admin`.
#[derive(Accounts)]
pub struct UpdateApprovedVkeys<'info> {
    #[account(
        mut,
        seeds = [b"approved_vkeys"],
        bump = approved_vkeys.bump,
        has_one = admin @ WhitelistError::Unauthorized
    )]
    pub approved_vkeys: Account<'info, ApprovedVkeys>,
    #[account(
        constraint = admin.key() == ADMIN_AUTHORITY @ WhitelistError::Unauthorized
    )]
    pub admin: Signer<'info>,
}

/// InitializeApprovedMeasurements instruction accounts.
#[derive(Accounts)]
pub struct InitializeApprovedMeasurements<'info> {
    #[account(
        init,
        payer = admin,
        space = ApprovedMeasurements::SIZE,
        seeds = [b"approved_measurements"],
        bump
    )]
    pub approved_measurements: Account<'info, ApprovedMeasurements>,
    #[account(
        mut,
        constraint = admin.key() == ADMIN_AUTHORITY @ WhitelistError::Unauthorized
    )]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

/// AddApprovedMeasurement / RemoveApprovedMeasurement instruction accounts.
/// See `UpdateApprovedVkeys` for the two-layer admin check rationale.
#[derive(Accounts)]
pub struct UpdateApprovedMeasurements<'info> {
    #[account(
        mut,
        seeds = [b"approved_measurements"],
        bump = approved_measurements.bump,
        has_one = admin @ WhitelistError::Unauthorized
    )]
    pub approved_measurements: Account<'info, ApprovedMeasurements>,
    #[account(
        constraint = admin.key() == ADMIN_AUTHORITY @ WhitelistError::Unauthorized
    )]
    pub admin: Signer<'info>,
}

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
    #[account(
        seeds = [b"approved_vkeys"],
        bump = approved_vkeys.bump
    )]
    pub approved_vkeys: Account<'info, ApprovedVkeys>,
    #[account(
        seeds = [b"approved_measurements"],
        bump = approved_measurements.bump
    )]
    pub approved_measurements: Account<'info, ApprovedMeasurements>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

/// RevokeKey instruction accounts (admin emergency operation).
///
/// The PDA is updated in-place rather than closed, so the seeds remain
/// occupied and `register_key` cannot re-create the same entry later by
/// replaying the original proof.
#[derive(Accounts)]
pub struct RevokeKey<'info> {
    #[account(
        mut,
        seeds = [b"whitelist", whitelist_entry.signing_pubkey.as_ref()],
        bump = whitelist_entry.bump
    )]
    pub whitelist_entry: Account<'info, WhitelistEntry>,
    /// Borrow `approved_vkeys` (read-only) just to cross-check `has_one =
    /// admin` against its PDA-recorded admin. Combined with the explicit
    /// `constraint = ADMIN_AUTHORITY` below, this mirrors the two-layer
    /// admin check on `UpdateApprovedVkeys` / `UpdateApprovedMeasurements`
    /// — a future `transfer_admin` ix that rotates the PDA admin would
    /// then also flow through revoke_key without a code change here.
    #[account(
        seeds = [b"approved_vkeys"],
        bump = approved_vkeys.bump,
        has_one = admin @ WhitelistError::Unauthorized
    )]
    pub approved_vkeys: Account<'info, ApprovedVkeys>,
    #[account(
        constraint = admin.key() == ADMIN_AUTHORITY @ WhitelistError::Unauthorized
    )]
    pub admin: Signer<'info>,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[event]
pub struct KeyRegistered {
    pub signing_pubkey: Pubkey,
    pub measurement: Vec<u8>,
    pub expires_at: i64,
}

#[event]
pub struct KeyRevoked {
    pub signing_pubkey: Pubkey,
}

#[event]
pub struct ApprovedVkeysInitialized {
    pub admin: Pubkey,
}

#[event]
pub struct VkeyApproved {
    pub vkey_hash: [u8; 32],
}

#[event]
pub struct VkeyRevoked {
    pub vkey_hash: [u8; 32],
}

#[event]
pub struct ApprovedMeasurementsInitialized {
    pub admin: Pubkey,
}

#[event]
pub struct MeasurementApproved {
    pub measurement: Vec<u8>,
}

#[event]
pub struct MeasurementRevoked {
    pub measurement: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Error codes
// ---------------------------------------------------------------------------

// Anchor numbers error variants from 6000 in declaration order, and those
// codes are part of the program's external ABI: clients (including the
// integration tests in `crates/solana/tests/devnet_whitelist.rs`) match on
// the resulting hex codes. **Only append new variants at the end.** Reordering
// or inserting in the middle silently breaks every consumer that compares
// against a hex code without recompiling against this crate.
#[error_code]
pub enum WhitelistError {
    #[msg("SP1 proof is empty")]
    EmptyProof,
    #[msg("SP1 proof has unexpected length (expected 4 + 256 bytes)")]
    InvalidProofLength,
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
    #[msg("Provided vkey_hash is not in the approved set")]
    VkeyNotApproved,
    #[msg("vkey_hash is already approved")]
    VkeyAlreadyApproved,
    #[msg("Approved vkey registry is at capacity")]
    VkeyRegistryFull,
    #[msg("Provided measurement is not in the approved set")]
    MeasurementNotApproved,
    #[msg("Measurement is already approved")]
    MeasurementAlreadyApproved,
    #[msg("Approved measurement registry is at capacity")]
    MeasurementRegistryFull,
    #[msg("Measurement length is out of range")]
    InvalidMeasurementLen,
    #[msg("Timestamp arithmetic overflow")]
    TimestampOverflow,
    #[msg("Key has already been revoked")]
    AlreadyRevoked,
}
