// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol Anchor Solanaプログラム
//!
//! 仕様書 §5.2 Step 1: Global Config PDAの管理。
//! 仕様書 §8: ガバナンス命令。
//!
//! ## アカウント設計
//! - `GlobalConfigAccount` (PDA: seeds=[b"global-config"]):
//!   信頼の原点。コレクションMint、ノードIDリスト、TSA鍵、WASMモジュールを保持する。
//! - `TeeNodeAccount` (PDA: seeds=[b"tee-node", &signing_pubkey]):
//!   TEEノードごとの詳細情報。gateway_endpoint、expected_measurements含む。
//!
//! ## 命令
//! - `initialize`: Global Configの初期化
//! - `register_tee_node`: TEEノードの登録（PDA作成 + リスト追加）
//! - `update_tee_node`: TEEノード情報の更新
//! - `deactivate_tee_node`: TEEノードの無効化
//! - `update_collections`: コレクションMintの更新
//! - `add_wasm_module`: WASMモジュールの追加
//! - `remove_wasm_module`: WASMモジュールの削除
//! - `add_tsa_key`: TSA鍵の追加
//! - `remove_tsa_key`: TSA鍵の削除
//! - `delegate_collection_authority`: Collection AuthorityをTEEに委譲
//! - `revoke_collection_authority`: Collection Authority委譲の取り消し

use anchor_lang::prelude::*;

declare_id!("GXo7dQ4kW8oeSSSK2Lhaw1jakNps1fSeUHEfeb7dRsYP");

#[program]
pub mod title_config {
    use super::*;

    /// Global Configを初期化する。
    /// 仕様書 §5.2 Step 1
    pub fn initialize(
        ctx: Context<Initialize>,
        core_collection_mint: Pubkey,
        ext_collection_mint: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.global_config;
        config.authority = ctx.accounts.authority.key();
        config.core_collection_mint = core_collection_mint;
        config.ext_collection_mint = ext_collection_mint;
        Ok(())
    }

    /// TEEノードを登録する。
    /// 仕様書 §8.2 TEEノードの追加
    ///
    /// TeeNodeAccount PDAを作成し、GlobalConfigのtrusted_node_keysに追加する。
    pub fn register_tee_node(
        ctx: Context<RegisterTeeNode>,
        signing_pubkey: [u8; 32],
        encryption_pubkey: [u8; 32],
        gateway_pubkey: [u8; 32],
        gateway_endpoint: String,
        tee_type: u8,
        measurements: Vec<MeasurementEntry>,
    ) -> Result<()> {
        require!(
            gateway_endpoint.len() <= 256,
            ErrorCode::GatewayEndpointTooLong
        );
        require!(measurements.len() <= 8, ErrorCode::TooManyMeasurements);

        let node = &mut ctx.accounts.tee_node;
        node.signing_pubkey = signing_pubkey;
        node.encryption_pubkey = encryption_pubkey;
        node.gateway_pubkey = gateway_pubkey;
        node.gateway_endpoint = gateway_endpoint;
        node.status = 1; // Active
        node.tee_type = tee_type;
        node.measurements = measurements;
        node.bump = ctx.bumps.tee_node;

        let config = &mut ctx.accounts.global_config;
        config.trusted_node_keys.push(signing_pubkey);

        emit!(TeeNodeRegistered {
            signing_pubkey: Pubkey::new_from_array(signing_pubkey),
        });

        Ok(())
    }

    /// TEEノード情報を更新する。
    /// 仕様書 §8.2
    ///
    /// 更新対象フィールドのみSomeで渡す。Noneのフィールドは変更しない。
    pub fn update_tee_node(
        ctx: Context<UpdateTeeNode>,
        encryption_pubkey: Option<[u8; 32]>,
        gateway_pubkey: Option<[u8; 32]>,
        gateway_endpoint: Option<String>,
        status: Option<u8>,
        measurements: Option<Vec<MeasurementEntry>>,
    ) -> Result<()> {
        if let Some(ref ep) = gateway_endpoint {
            require!(ep.len() <= 256, ErrorCode::GatewayEndpointTooLong);
        }
        if let Some(ref m) = measurements {
            require!(m.len() <= 8, ErrorCode::TooManyMeasurements);
        }

        let node = &mut ctx.accounts.tee_node;
        if let Some(v) = encryption_pubkey {
            node.encryption_pubkey = v;
        }
        if let Some(v) = gateway_pubkey {
            node.gateway_pubkey = v;
        }
        if let Some(v) = gateway_endpoint {
            node.gateway_endpoint = v;
        }
        if let Some(v) = status {
            node.status = v;
        }
        if let Some(v) = measurements {
            node.measurements = v;
        }
        Ok(())
    }

    /// TEEノードを無効化する。
    /// 仕様書 §8.2 TEEノードの削除時
    ///
    /// statusをInactiveに変更する。アカウントは維持される（過去のcNFT検証用）。
    pub fn deactivate_tee_node(ctx: Context<UpdateTeeNode>) -> Result<()> {
        let node = &mut ctx.accounts.tee_node;
        node.status = 0;

        emit!(TeeNodeDeactivated {
            signing_pubkey: Pubkey::new_from_array(node.signing_pubkey),
        });

        Ok(())
    }

    /// コレクションMintアドレスを更新する。
    /// 仕様書 §5.2 Step 1
    pub fn update_collections(
        ctx: Context<UpdateConfig>,
        core_collection_mint: Pubkey,
        ext_collection_mint: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.global_config;
        config.core_collection_mint = core_collection_mint;
        config.ext_collection_mint = ext_collection_mint;
        Ok(())
    }

    /// 信頼されたWASMモジュールを追加または更新する（upsert）。
    /// 仕様書 §7.3
    ///
    /// 同じextension_idが既に登録されている場合はwasm_hashとwasm_sourceを更新する。
    /// 存在しない場合は新規追加する。
    pub fn add_wasm_module(
        ctx: Context<UpdateConfig>,
        extension_id: [u8; 32],
        wasm_hash: [u8; 32],
        wasm_source: String,
    ) -> Result<()> {
        require!(wasm_source.len() <= 256, ErrorCode::WasmSourceTooLong);

        let config = &mut ctx.accounts.global_config;
        if let Some(existing) = config
            .trusted_wasm_modules
            .iter_mut()
            .find(|m| m.extension_id == extension_id)
        {
            existing.wasm_hash = wasm_hash;
            existing.wasm_source = wasm_source;
        } else {
            config.trusted_wasm_modules.push(WasmModuleEntry {
                extension_id,
                wasm_hash,
                wasm_source,
            });
        }
        Ok(())
    }

    /// 信頼されたWASMモジュールを削除する。
    /// 仕様書 §7.3
    pub fn remove_wasm_module(
        ctx: Context<UpdateConfig>,
        extension_id: [u8; 32],
    ) -> Result<()> {
        let config = &mut ctx.accounts.global_config;
        let pos = config
            .trusted_wasm_modules
            .iter()
            .position(|m| m.extension_id == extension_id)
            .ok_or(ErrorCode::WasmModuleNotFound)?;
        config.trusted_wasm_modules.remove(pos);
        Ok(())
    }

    /// 信頼するTSA鍵を追加する。
    /// 仕様書 §8.3
    pub fn add_tsa_key(ctx: Context<UpdateConfig>, key: [u8; 32]) -> Result<()> {
        let config = &mut ctx.accounts.global_config;
        let exists = config.trusted_tsa_keys.iter().any(|k| k == &key);
        require!(!exists, ErrorCode::DuplicateTsaKey);
        config.trusted_tsa_keys.push(key);
        Ok(())
    }

    /// 信頼するTSA鍵を削除する。
    /// 仕様書 §8.3
    pub fn remove_tsa_key(ctx: Context<UpdateConfig>, key: [u8; 32]) -> Result<()> {
        let config = &mut ctx.accounts.global_config;
        let pos = config
            .trusted_tsa_keys
            .iter()
            .position(|k| k == &key)
            .ok_or(ErrorCode::TsaKeyNotFound)?;
        config.trusted_tsa_keys.remove(pos);
        Ok(())
    }

    /// Collection AuthorityをTEEの署名鍵に委譲する。
    /// 仕様書 §8.2 TEEノードの追加時
    ///
    /// DAOの管理者（authority）がMPL CoreコレクションのUpdate Authority権限を
    /// TEEのsigning_pubkeyにDelegateする。実際のCPI呼び出しはMPL Core SDKの
    /// 依存が必要なため、イベントを発行し、クライアントサイドでMPL Core命令と
    /// 合成するトランザクションを構築する設計とする。
    ///
    /// collection_typeで対象コレクションを指定する:
    /// - 0 = core_collection_mint
    /// - 1 = ext_collection_mint
    pub fn delegate_collection_authority(
        ctx: Context<CollectionAuthority>,
        collection_type: u8,
    ) -> Result<()> {
        let config = &ctx.accounts.global_config;
        let node = &ctx.accounts.tee_node;

        require!(node.status == 1, ErrorCode::UntrustedTeeNode);

        let expected_collection = match collection_type {
            0 => config.core_collection_mint,
            1 => config.ext_collection_mint,
            _ => return Err(ErrorCode::InvalidCollectionType.into()),
        };

        require_keys_eq!(
            ctx.accounts.collection.key(),
            expected_collection,
            ErrorCode::CollectionMismatch
        );

        let tee_signing_pubkey = Pubkey::new_from_array(node.signing_pubkey);

        emit!(CollectionAuthorityDelegated {
            collection: expected_collection,
            collection_type,
            tee_signing_pubkey,
        });

        msg!(
            "Collection Authority委譲を承認: collection={}, tee={}",
            expected_collection,
            tee_signing_pubkey
        );
        Ok(())
    }

    /// Collection Authority委譲を取り消す。
    /// 仕様書 §8.2 TEEノードの削除時（不正発覚時）
    pub fn revoke_collection_authority(
        ctx: Context<CollectionAuthority>,
        collection_type: u8,
    ) -> Result<()> {
        let config = &ctx.accounts.global_config;
        let node = &ctx.accounts.tee_node;

        let expected_collection = match collection_type {
            0 => config.core_collection_mint,
            1 => config.ext_collection_mint,
            _ => return Err(ErrorCode::InvalidCollectionType.into()),
        };

        require_keys_eq!(
            ctx.accounts.collection.key(),
            expected_collection,
            ErrorCode::CollectionMismatch
        );

        let tee_signing_pubkey = Pubkey::new_from_array(node.signing_pubkey);

        emit!(CollectionAuthorityRevoked {
            collection: expected_collection,
            collection_type,
            tee_signing_pubkey,
        });

        msg!(
            "Collection Authority委譲を取り消し: collection={}, tee={}",
            expected_collection,
            tee_signing_pubkey
        );
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// アカウント構造
// ---------------------------------------------------------------------------

/// Global Config PDA。信頼の原点。
/// 仕様書 §5.2 Step 1
///
/// TEEノードの詳細は個別のTeeNodeAccount PDAに格納される。
/// 本アカウントはノードのsigning_pubkeyリスト（フラット）のみを保持する。
#[account]
pub struct GlobalConfigAccount {
    /// DAO multi-sigのウォレットアドレス
    pub authority: Pubkey,
    /// Core cNFTの公式コレクションMintアドレス
    pub core_collection_mint: Pubkey,
    /// Extension cNFTの公式コレクションMintアドレス
    pub ext_collection_mint: Pubkey,
    /// 信頼されたTEEノードのsigning_pubkeyリスト
    pub trusted_node_keys: Vec<[u8; 32]>,
    /// 信頼するTSA公開鍵ハッシュのリスト
    pub trusted_tsa_keys: Vec<[u8; 32]>,
    /// 信頼されたWASMモジュールのリスト
    pub trusted_wasm_modules: Vec<WasmModuleEntry>,
}

impl GlobalConfigAccount {
    /// 固定フィールドのサイズ（discriminator + Pubkey×3 + Vec prefix×3）
    const BASE_SIZE: usize = 8 + 32 + 32 + 32 + 4 + 4 + 4;

    /// 初期割当サイズ。
    /// Solana CPI制限（MAX_PERMITTED_DATA_INCREASE = 10,240バイト）に収める。
    /// 可変領域 10,124B: ノードID(32B)×100 + TSA鍵(32B)×30 + WASMモジュール(≈98B)×30 ≈ 6.1KB。
    /// 将来的にrealloc命令追加で拡張可能。
    pub const INIT_SPACE: usize = 10240;
}

/// 信頼されたWASMモジュール情報。
/// 仕様書 §5.2 Step 1, §7.3
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct WasmModuleEntry {
    /// Extension識別子（最大32バイト、null-padded）
    pub extension_id: [u8; 32],
    /// WASMバイナリのSHA-256ハッシュ
    pub wasm_hash: [u8; 32],
    /// WASMバイナリの取得先URL（例: "ar://..."）
    pub wasm_source: String,
}

/// TEEノード情報。per-node PDA。
/// 仕様書 §5.2 Step 1
///
/// GlobalConfigのtrusted_node_keysにsigning_pubkeyが登録され、
/// 詳細情報は本PDA（seeds=[b"tee-node", &signing_pubkey]）に格納される。
#[account]
pub struct TeeNodeAccount {
    /// Ed25519署名用公開鍵（32バイト）
    pub signing_pubkey: [u8; 32],
    /// X25519暗号化用公開鍵（32バイト）
    pub encryption_pubkey: [u8; 32],
    /// Gateway署名用Ed25519公開鍵
    pub gateway_pubkey: [u8; 32],
    /// GatewayエンドポイントURL
    pub gateway_endpoint: String,
    /// ノードステータス (0=Inactive, 1=Active)
    pub status: u8,
    /// TEE種別 (0=aws_nitro, 1=amd_sev_snp, 2=intel_tdx)
    pub tee_type: u8,
    /// 期待される測定値。tee_typeに応じてキー名が異なる。
    /// 仕様書 §5.2 Step 4
    pub measurements: Vec<MeasurementEntry>,
    /// PDA bump seed
    pub bump: u8,
}

impl TeeNodeAccount {
    /// 固定フィールドサイズ
    const BASE_SIZE: usize = 8 // discriminator
        + 32  // signing_pubkey
        + 32  // encryption_pubkey
        + 32  // gateway_pubkey
        + 4   // gateway_endpoint String prefix
        + 1   // status
        + 1   // tee_type
        + 4   // measurements Vec prefix
        + 1;  // bump

    /// 最大スペース（gateway_endpoint 256文字 + measurements 8エントリ）
    pub const MAX_SPACE: usize = Self::BASE_SIZE + 256 + 8 * MeasurementEntry::SIZE;
}

/// TEE測定値エントリ。
/// 仕様書 §5.2 Step 4
///
/// キー名の例:
/// - AWS Nitro: "PCR0", "PCR1", "PCR2"
/// - AMD SEV-SNP: "MEASUREMENT"
/// - Intel TDX: "MRTD", "RTMR0"〜"RTMR3"
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct MeasurementEntry {
    /// 測定値キー名（null-padded、最大16バイト）
    pub key: [u8; 16],
    /// 測定値（生ハッシュバイト、SHA-384 = 48バイト）
    pub value: [u8; 48],
}

impl MeasurementEntry {
    pub const SIZE: usize = 16 + 48;
}

// ---------------------------------------------------------------------------
// Context構造体
// ---------------------------------------------------------------------------

/// initialize命令のアカウント。
#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = GlobalConfigAccount::INIT_SPACE,
        seeds = [b"global-config"],
        bump
    )]
    pub global_config: Account<'info, GlobalConfigAccount>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

/// TEEノード登録命令のアカウント。
/// 仕様書 §8.2
///
/// `payer`（TEE署名鍵の所有者）がrent費用を支払い、`authority`（DAO）が承認する。
/// `payer.key() == signing_pubkey` 制約により、TEEが自身の鍵で署名することを強制し、
/// スペック（encryption_pubkey, gateway_endpoint等）の偽造を防止する。
/// `/create-tree` と同様、TEEの内部鍵で支払いを行うフローに統一。
#[derive(Accounts)]
#[instruction(signing_pubkey: [u8; 32])]
pub struct RegisterTeeNode<'info> {
    #[account(
        mut,
        seeds = [b"global-config"],
        bump,
        has_one = authority
    )]
    pub global_config: Account<'info, GlobalConfigAccount>,
    #[account(
        init,
        payer = payer,
        space = TeeNodeAccount::MAX_SPACE,
        seeds = [b"tee-node", signing_pubkey.as_ref()],
        bump
    )]
    pub tee_node: Account<'info, TeeNodeAccount>,
    /// DAO承認（署名のみ、支払い不要）
    pub authority: Signer<'info>,
    /// TEEノードの署名鍵所有者（rent支払い + 鍵所有証明）
    #[account(
        mut,
        constraint = payer.key() == Pubkey::new_from_array(signing_pubkey) @ ErrorCode::PayerSigningKeyMismatch
    )]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

/// TEEノード更新命令のアカウント。
/// 仕様書 §8.2
#[derive(Accounts)]
pub struct UpdateTeeNode<'info> {
    #[account(
        seeds = [b"global-config"],
        bump,
        has_one = authority
    )]
    pub global_config: Account<'info, GlobalConfigAccount>,
    #[account(
        mut,
        seeds = [b"tee-node", tee_node.signing_pubkey.as_ref()],
        bump = tee_node.bump
    )]
    pub tee_node: Account<'info, TeeNodeAccount>,
    pub authority: Signer<'info>,
}

/// 設定更新命令のアカウント。
#[derive(Accounts)]
pub struct UpdateConfig<'info> {
    #[account(
        mut,
        seeds = [b"global-config"],
        bump,
        has_one = authority
    )]
    pub global_config: Account<'info, GlobalConfigAccount>,
    pub authority: Signer<'info>,
}

/// Collection Authority委譲/取り消し命令のアカウント。
/// 仕様書 §8.2
#[derive(Accounts)]
pub struct CollectionAuthority<'info> {
    #[account(
        seeds = [b"global-config"],
        bump,
        has_one = authority
    )]
    pub global_config: Account<'info, GlobalConfigAccount>,
    #[account(
        seeds = [b"tee-node", tee_node.signing_pubkey.as_ref()],
        bump = tee_node.bump
    )]
    pub tee_node: Account<'info, TeeNodeAccount>,
    pub authority: Signer<'info>,
    /// CHECK: collection mintの一致はプログラム内で検証する。
    pub collection: UncheckedAccount<'info>,
}

// ---------------------------------------------------------------------------
// イベント
// ---------------------------------------------------------------------------

/// TEEノード登録イベント。
/// 仕様書 §8.2
#[event]
pub struct TeeNodeRegistered {
    pub signing_pubkey: Pubkey,
}

/// TEEノード無効化イベント。
/// 仕様書 §8.2
#[event]
pub struct TeeNodeDeactivated {
    pub signing_pubkey: Pubkey,
}

/// Collection Authority委譲イベント。
/// 仕様書 §8.2
#[event]
pub struct CollectionAuthorityDelegated {
    pub collection: Pubkey,
    pub collection_type: u8,
    pub tee_signing_pubkey: Pubkey,
}

/// Collection Authority取り消しイベント。
/// 仕様書 §8.2
#[event]
pub struct CollectionAuthorityRevoked {
    pub collection: Pubkey,
    pub collection_type: u8,
    pub tee_signing_pubkey: Pubkey,
}

// ---------------------------------------------------------------------------
// エラーコード
// ---------------------------------------------------------------------------

/// プログラム固有のエラーコード。
#[error_code]
pub enum ErrorCode {
    /// 不正なcollection_type値（0または1のみ有効）
    #[msg("collection_typeは0(Core)または1(Extension)のみ有効です")]
    InvalidCollectionType,
    /// コレクションアカウントがGlobal Configの値と一致しない
    #[msg("コレクションアカウントがGlobal Configの値と一致しません")]
    CollectionMismatch,
    /// TEEノードがActiveでない
    #[msg("TEEノードがActiveではありません")]
    UntrustedTeeNode,
    /// WASMモジュールが既に登録されている
    #[msg("同じextension_idのWASMモジュールが既に登録されています")]
    DuplicateWasmModule,
    /// WASMモジュールが見つからない
    #[msg("指定されたextension_idのWASMモジュールが見つかりません")]
    WasmModuleNotFound,
    /// TSA鍵が既に登録されている
    #[msg("同じTSA鍵が既に登録されています")]
    DuplicateTsaKey,
    /// TSA鍵が見つからない
    #[msg("指定されたTSA鍵が見つかりません")]
    TsaKeyNotFound,
    /// gateway_endpointが長すぎる
    #[msg("gateway_endpointは256文字以内である必要があります")]
    GatewayEndpointTooLong,
    /// 測定値エントリが多すぎる
    #[msg("measurementsは8エントリ以内である必要があります")]
    TooManyMeasurements,
    /// wasm_sourceが長すぎる
    #[msg("wasm_sourceは256文字以内である必要があります")]
    WasmSourceTooLong,
    /// payerのアドレスがsigning_pubkeyと一致しない
    #[msg("payerのアドレスがsigning_pubkeyと一致しません")]
    PayerSigningKeyMismatch,
}
