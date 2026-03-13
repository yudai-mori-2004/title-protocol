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
//! - `register_tee_node`: TEEノードの登録（PDA作成 + リスト追加 + Collection Authority委譲）
//! - `update_tee_node`: TEEノード情報の更新
//! - `deactivate_tee_node`: TEEノードの無効化
//! - `remove_tee_node`: TEEノードの削除（リスト除去 + Collection Authority取り消し + PDAクローズ）
//! - `update_collections`: コレクションMintの更新
//! - `add_wasm_module`: WASMモジュールの追加
//! - `remove_wasm_module`: WASMモジュールの削除
//! - `set_resource_limits`: リソース制限の設定
//! - `add_tsa_key`: TSA鍵の追加
//! - `remove_tsa_key`: TSA鍵の削除

use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::invoke,
};

declare_id!("9wodSEfsAzTGEJKMezCuDGpmrJGzb4wNM5TwvmphGoLn");

/// MPL Core プログラムID: CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d
fn mpl_core_program_id() -> Pubkey {
    "CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d"
        .parse()
        .unwrap()
}

// ---------------------------------------------------------------------------
// MPL Core CPI ヘルパー
// ---------------------------------------------------------------------------

/// MPL Core AddCollectionPluginV1 CPIを実行する。
/// UpdateDelegateプラグインを新規作成し、指定されたキーリストをadditional_delegatesに設定する。
/// コレクションに初めてUpdateDelegateプラグインを追加する場合に使用する。
///
/// MPL Core のアカウント順序:
///   0: collection (writable)
///   1: payer (writable, signer) — authority が兼任
///   2: authority (signer) — 同じキー
///   3: system_program
///   4: log_wrapper (optional → MPL Core ID をsentinel)
fn cpi_add_update_delegate<'a>(
    mpl_core_program: &AccountInfo<'a>,
    collection: &AccountInfo<'a>,
    authority: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
    delegates: &[[u8; 32]],
) -> Result<()> {
    let mut ix_data = Vec::with_capacity(6 + delegates.len() * 32);
    ix_data.push(3); // MplAssetInstruction::AddCollectionPluginV1
    ix_data.push(4); // Plugin::UpdateDelegate
    ix_data.extend_from_slice(&(delegates.len() as u32).to_le_bytes());
    for key in delegates {
        ix_data.extend_from_slice(key);
    }
    ix_data.push(0); // init_authority: Option<PluginAuthority>::None

    let ix = Instruction {
        program_id: mpl_core_program.key(),
        accounts: vec![
            AccountMeta::new(collection.key(), false),               // collection
            AccountMeta::new(authority.key(), true),                  // payer
            AccountMeta::new_readonly(authority.key(), true),         // authority (same key)
            AccountMeta::new_readonly(system_program.key(), false),   // system_program
            AccountMeta::new_readonly(mpl_core_program.key(), false), // log_wrapper (None sentinel)
        ],
        data: ix_data,
    };

    invoke(
        &ix,
        &[collection.clone(), authority.clone(), system_program.clone(), mpl_core_program.clone()],
    )?;

    Ok(())
}

/// MPL Core UpdateCollectionPluginV1 CPIを実行する。
/// 既存のUpdateDelegateプラグインのadditional_delegatesリストを全置換する。
/// 2つ目以降のTEEノード追加/削除時に使用する。
fn cpi_update_delegate<'a>(
    mpl_core_program: &AccountInfo<'a>,
    collection: &AccountInfo<'a>,
    authority: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
    delegates: &[[u8; 32]],
) -> Result<()> {
    let mut ix_data = Vec::with_capacity(6 + delegates.len() * 32);
    ix_data.push(7); // MplAssetInstruction::UpdateCollectionPluginV1
    ix_data.push(4); // Plugin::UpdateDelegate
    ix_data.extend_from_slice(&(delegates.len() as u32).to_le_bytes());
    for key in delegates {
        ix_data.extend_from_slice(key);
    }

    let ix = Instruction {
        program_id: mpl_core_program.key(),
        accounts: vec![
            AccountMeta::new(collection.key(), false),               // collection
            AccountMeta::new(authority.key(), true),                  // payer
            AccountMeta::new_readonly(authority.key(), true),         // authority (same key)
            AccountMeta::new_readonly(system_program.key(), false),   // system_program
            AccountMeta::new_readonly(mpl_core_program.key(), false), // log_wrapper (None sentinel)
        ],
        data: ix_data,
    };

    invoke(
        &ix,
        &[collection.clone(), authority.clone(), system_program.clone(), mpl_core_program.clone()],
    )?;

    Ok(())
}

/// MPL Core RemoveCollectionPluginV1 CPIを実行する。
/// UpdateDelegateプラグインを完全に削除する。
/// 最後のTEEノード削除時に使用する。
fn cpi_remove_delegate_plugin<'a>(
    mpl_core_program: &AccountInfo<'a>,
    collection: &AccountInfo<'a>,
    authority: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
) -> Result<()> {
    // instruction variant 5 (RemoveCollectionPluginV1) + PluginType::UpdateDelegate (4)
    let ix_data = vec![5, 4];

    let ix = Instruction {
        program_id: mpl_core_program.key(),
        accounts: vec![
            AccountMeta::new(collection.key(), false),               // collection
            AccountMeta::new(authority.key(), true),                  // payer
            AccountMeta::new_readonly(authority.key(), true),         // authority (same key)
            AccountMeta::new_readonly(system_program.key(), false),   // system_program
            AccountMeta::new_readonly(mpl_core_program.key(), false), // log_wrapper (None sentinel)
        ],
        data: ix_data,
    };

    invoke(
        &ix,
        &[collection.clone(), authority.clone(), system_program.clone(), mpl_core_program.clone()],
    )?;

    Ok(())
}

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
        config.resource_limits = ResourceLimitsOnChain::default();
        Ok(())
    }

    /// TEEノードを登録する。
    /// 仕様書 §8.2 TEEノードの追加
    ///
    /// TeeNodeAccount PDAを作成し、GlobalConfigのtrusted_node_keysに追加する。
    /// 同時にMPL Coreコレクション（Core + Extension）にUpdateDelegateプラグインCPIを実行し、
    /// TEEのsigning_pubkeyにコレクション操作権限を付与する。
    ///
    /// **不変条件**: GlobalConfigのtrusted_node_keys == コレクションのadditional_delegates。
    /// 登録と権限委譲は1トランザクションで不可分に実行される。
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

        // TeeNodeAccount 初期化
        let node = &mut ctx.accounts.tee_node;
        node.signing_pubkey = signing_pubkey;
        node.encryption_pubkey = encryption_pubkey;
        node.gateway_pubkey = gateway_pubkey;
        node.gateway_endpoint = gateway_endpoint;
        node.status = 1; // Active
        node.tee_type = tee_type;
        node.measurements = measurements;
        node.bump = ctx.bumps.tee_node;

        // GlobalConfig にノード追加
        let config = &mut ctx.accounts.global_config;
        config.trusted_node_keys.push(signing_pubkey);
        let is_first_node = config.trusted_node_keys.len() == 1;
        let all_keys = config.trusted_node_keys.clone();
        // NLL: config の mutable borrow はここで終了

        // MPL Core CPI: 両コレクションにUpdateDelegateプラグインを設定
        let mpl_info = ctx.accounts.mpl_core_program.to_account_info();
        let auth_info = ctx.accounts.authority.to_account_info();
        let sys_info = ctx.accounts.system_program.to_account_info();

        for collection_info in [
            ctx.accounts.core_collection.to_account_info(),
            ctx.accounts.ext_collection.to_account_info(),
        ] {
            if is_first_node {
                cpi_add_update_delegate(
                    &mpl_info,
                    &collection_info,
                    &auth_info,
                    &sys_info,
                    &all_keys,
                )?;
            } else {
                cpi_update_delegate(
                    &mpl_info,
                    &collection_info,
                    &auth_info,
                    &sys_info,
                    &all_keys,
                )?;
            }
        }

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

    /// TEEノードを完全削除する。
    /// 仕様書 §8.2 TEEノードの削除
    ///
    /// trusted_node_keysリストから削除し、TeeNodeAccountをクローズする。
    /// 同時にMPL CoreコレクションのUpdateDelegateプラグインを更新（残ノードあり）
    /// または削除（最後のノード）し、該当TEEのコレクション操作権限を取り消す。
    ///
    /// **不変条件**: GlobalConfigのtrusted_node_keys == コレクションのadditional_delegates。
    pub fn remove_tee_node(ctx: Context<RemoveTeeNode>) -> Result<()> {
        let signing_pubkey = ctx.accounts.tee_node.signing_pubkey;

        // GlobalConfig からノード削除
        let config = &mut ctx.accounts.global_config;
        config.trusted_node_keys.retain(|k| k != &signing_pubkey);
        let remaining_keys = config.trusted_node_keys.clone();
        // NLL: config の mutable borrow はここで終了

        // MPL Core CPI: 両コレクションのUpdateDelegateプラグインを更新/削除
        let mpl_info = ctx.accounts.mpl_core_program.to_account_info();
        let auth_info = ctx.accounts.authority.to_account_info();
        let sys_info = ctx.accounts.system_program.to_account_info();

        for collection_info in [
            ctx.accounts.core_collection.to_account_info(),
            ctx.accounts.ext_collection.to_account_info(),
        ] {
            if remaining_keys.is_empty() {
                cpi_remove_delegate_plugin(
                    &mpl_info,
                    &collection_info,
                    &auth_info,
                    &sys_info,
                )?;
            } else {
                cpi_update_delegate(
                    &mpl_info,
                    &collection_info,
                    &auth_info,
                    &sys_info,
                    &remaining_keys,
                )?;
            }
        }

        emit!(TeeNodeDeactivated {
            signing_pubkey: Pubkey::new_from_array(signing_pubkey),
        });

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

    /// リソース制限をオンチェーンに設定する。
    /// 仕様書 §6.2: Gatewayが読み取るResourceLimitsの上限値。
    pub fn set_resource_limits(
        ctx: Context<UpdateConfig>,
        limits: ResourceLimitsOnChain,
    ) -> Result<()> {
        let config = &mut ctx.accounts.global_config;
        config.resource_limits = limits;
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
}

// ---------------------------------------------------------------------------
// アカウント構造
// ---------------------------------------------------------------------------

/// Global Config PDA。信頼の原点。
/// 仕様書 §5.2 Step 1
///
/// プログラムはpermissionless: 誰でもデプロイしてGlobalConfigを作成できるが、
/// プロトコルとして正規なのはメインネット上のDAO multi-sigが管理するもののみ。
/// 正規GlobalConfigが指定する公式コレクションに属するcNFTだけが、
/// プロトコル検証者から正規のコンテンツ記録として認識される。
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
    /// リソース制限（オンチェーン上限）
    pub resource_limits: ResourceLimitsOnChain,
}

impl GlobalConfigAccount {
    /// 固定フィールドのサイズ（discriminator + Pubkey×3 + Vec prefix×3 + ResourceLimitsOnChain）
    const BASE_SIZE: usize = 8 + 32 + 32 + 32 + 4 + 4 + 4 + 63;

    /// 初期割当サイズ。
    /// Solana CPI制限（MAX_PERMITTED_DATA_INCREASE = 10,240バイト）に収める。
    /// 可変領域 10,124B: ノードID(32B)×100 + TSA鍵(32B)×30 + WASMモジュール(≈98B)×30 ≈ 6.1KB。
    /// 将来的にrealloc命令追加で拡張可能。
    pub const INIT_SPACE: usize = 10240;
}

/// オンチェーン リソース制限。
/// 仕様書 §6.2: GatewayのResourceLimitsの上限をオンチェーンで管理する。
///
/// 各フィールドはOption<u64>で、Noneの場合はGatewayのデフォルト値を使用する。
/// Someの場合はGatewayのデフォルト値とのminを取る（オンチェーン値が上限となる）。
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct ResourceLimitsOnChain {
    /// 1コンテンツの最大バイト数
    pub max_single_content_bytes: Option<u64>,
    /// 同時処理中の合計最大バイト数
    pub max_concurrent_bytes: Option<u64>,
    /// 最低アップロード速度（バイト/秒）
    pub min_upload_speed_bytes: Option<u64>,
    /// 基本処理時間（秒）
    pub base_processing_time_sec: Option<u64>,
    /// グローバルタイムアウト上限（秒）
    pub max_global_timeout_sec: Option<u64>,
    /// チャンク読み取りタイムアウト（秒）
    pub chunk_read_timeout_sec: Option<u64>,
    /// C2PAグラフノード数上限
    pub c2pa_max_graph_size: Option<u64>,
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
///
/// 同時にMPL Core CPIでコレクションにUpdateDelegateプラグインを設定し、
/// TEEにcNFTミント権限を付与する。authorityはコレクションのupdate_authorityとして
/// MPL Core CPIのpayerも兼ねる（プラグイン追加/更新のrent費用）。
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
    /// DAO承認（署名 + MPL Core CPI authority/payer）
    #[account(mut)]
    pub authority: Signer<'info>,
    /// TEEノードの署名鍵所有者（rent支払い + 鍵所有証明）
    #[account(
        mut,
        constraint = payer.key() == Pubkey::new_from_array(signing_pubkey) @ ErrorCode::PayerSigningKeyMismatch
    )]
    pub payer: Signer<'info>,
    /// CHECK: Core cNFTコレクション。GlobalConfigの値と一致を検証する。
    #[account(
        mut,
        constraint = core_collection.key() == global_config.core_collection_mint @ ErrorCode::CollectionMismatch
    )]
    pub core_collection: UncheckedAccount<'info>,
    /// CHECK: Extension cNFTコレクション。GlobalConfigの値と一致を検証する。
    #[account(
        mut,
        constraint = ext_collection.key() == global_config.ext_collection_mint @ ErrorCode::CollectionMismatch
    )]
    pub ext_collection: UncheckedAccount<'info>,
    /// CHECK: MPL Core プログラム。アドレスで検証する。
    #[account(address = mpl_core_program_id())]
    pub mpl_core_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

/// TEEノード削除命令のアカウント。
/// 仕様書 §8.2
///
/// GlobalConfigからノードを除去し、TeeNodeAccount PDAをクローズすると同時に、
/// MPL Core CPIでコレクションのUpdateDelegateプラグインを更新/削除する。
#[derive(Accounts)]
pub struct RemoveTeeNode<'info> {
    #[account(
        mut,
        seeds = [b"global-config"],
        bump,
        has_one = authority
    )]
    pub global_config: Account<'info, GlobalConfigAccount>,
    #[account(
        mut,
        close = rent_recipient,
        seeds = [b"tee-node", tee_node.signing_pubkey.as_ref()],
        bump = tee_node.bump
    )]
    pub tee_node: Account<'info, TeeNodeAccount>,
    /// DAO承認（署名 + MPL Core CPI authority/payer）
    #[account(mut)]
    pub authority: Signer<'info>,
    /// rent返還先
    /// CHECK: rent lamportsの受取先。任意のアカウント。
    #[account(mut)]
    pub rent_recipient: UncheckedAccount<'info>,
    /// CHECK: Core cNFTコレクション。GlobalConfigの値と一致を検証する。
    #[account(
        mut,
        constraint = core_collection.key() == global_config.core_collection_mint @ ErrorCode::CollectionMismatch
    )]
    pub core_collection: UncheckedAccount<'info>,
    /// CHECK: Extension cNFTコレクション。GlobalConfigの値と一致を検証する。
    #[account(
        mut,
        constraint = ext_collection.key() == global_config.ext_collection_mint @ ErrorCode::CollectionMismatch
    )]
    pub ext_collection: UncheckedAccount<'info>,
    /// CHECK: MPL Core プログラム。アドレスで検証する。
    #[account(address = mpl_core_program_id())]
    pub mpl_core_program: UncheckedAccount<'info>,
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

// ---------------------------------------------------------------------------
// イベント
// ---------------------------------------------------------------------------

/// TEEノード登録イベント。
/// 仕様書 §8.2
#[event]
pub struct TeeNodeRegistered {
    pub signing_pubkey: Pubkey,
}

/// TEEノード無効化/削除イベント。
/// 仕様書 §8.2
#[event]
pub struct TeeNodeDeactivated {
    pub signing_pubkey: Pubkey,
}

// ---------------------------------------------------------------------------
// エラーコード
// ---------------------------------------------------------------------------

/// プログラム固有のエラーコード。
#[error_code]
pub enum ErrorCode {
    /// コレクションアカウントがGlobal Configの値と一致しない
    #[msg("コレクションアカウントがGlobal Configの値と一致しません")]
    CollectionMismatch,
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
