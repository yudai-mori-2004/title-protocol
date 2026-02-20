//! # Title Protocol Anchor Solanaプログラム
//!
//! 仕様書 §5.2 Step 1: Global Config PDAの管理。
//!
//! ## 命令
//! - `initialize`: Global Configの初期化
//! - `update_tee_nodes`: 信頼されたTEEノードリストの更新
//! - `update_wasm_modules`: 信頼されたWASMモジュールリストの更新
//! - `update_tsa_keys`: 信頼するTSA鍵リストの更新
//! - `delegate_collection_authority`: Collection AuthorityをTEEに委譲
//! - `revoke_collection_authority`: Collection Authority委譲の取り消し

use anchor_lang::prelude::*;

declare_id!("C2HryYkBKeoc4KE2RJ6au1oXc1jtKeKw3zrknQ455JQN");

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

    /// 信頼されたTEEノードリストを更新する。
    /// 仕様書 §5.2 Step 1
    pub fn update_tee_nodes(
        ctx: Context<UpdateConfig>,
        nodes: Vec<TrustedTeeNodeAccount>,
    ) -> Result<()> {
        let config = &mut ctx.accounts.global_config;
        config.trusted_tee_nodes = nodes;
        Ok(())
    }

    /// 信頼されたWASMモジュールリストを更新する。
    /// 仕様書 §5.2 Step 1
    pub fn update_wasm_modules(
        ctx: Context<UpdateConfig>,
        modules: Vec<TrustedWasmModuleAccount>,
    ) -> Result<()> {
        let config = &mut ctx.accounts.global_config;
        config.trusted_wasm_modules = modules;
        Ok(())
    }

    /// 信頼するTSA鍵リストを更新する。
    /// 仕様書 §5.2 Step 1
    pub fn update_tsa_keys(ctx: Context<UpdateConfig>, keys: Vec<[u8; 32]>) -> Result<()> {
        let config = &mut ctx.accounts.global_config;
        config.trusted_tsa_keys = keys;
        Ok(())
    }

    /// Collection AuthorityをTEEの署名鍵に委譲する。
    /// 仕様書 §8.2 TEEノードの追加時
    ///
    /// DAOの管理者（authority）がMPL CoreコレクションのUpdate Authority権限を
    /// TEEのsigning_pubkeyにDelegateする。これにより、TEEは公式コレクションに
    /// 属するcNFTをミントできるようになる。
    ///
    /// collection_typeで対象コレクションを指定する:
    /// - 0 = core_collection_mint
    /// - 1 = ext_collection_mint
    pub fn delegate_collection_authority(
        ctx: Context<CollectionAuthority>,
        tee_signing_pubkey: Pubkey,
        collection_type: u8,
    ) -> Result<()> {
        let config = &ctx.accounts.global_config;

        // 委譲対象のコレクションMintを特定
        let expected_collection = match collection_type {
            0 => config.core_collection_mint,
            1 => config.ext_collection_mint,
            _ => return Err(ErrorCode::InvalidCollectionType.into()),
        };

        // 渡されたコレクションアカウントが正しいか検証
        require_keys_eq!(
            ctx.accounts.collection.key(),
            expected_collection,
            ErrorCode::CollectionMismatch
        );

        // TEEのsigning_pubkeyがtrusted_tee_nodesに含まれるか検証
        let tee_bytes: [u8; 32] = tee_signing_pubkey.to_bytes();
        let is_trusted = config
            .trusted_tee_nodes
            .iter()
            .any(|n| n.signing_pubkey == tee_bytes && n.status == 1);
        require!(is_trusted, ErrorCode::UntrustedTeeNode);

        // MPL Core CPIでUpdateDelegateプラグインを追加する。
        // 実際のCPI呼び出しはMPL Core SDKの依存が必要なため、
        // ここではイベントを発行し、クライアントサイドで
        // MPL Core命令と合成するトランザクションを構築する設計とする。
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
    ///
    /// DAOの管理者（authority）がTEEへのCollection Authority委譲を取り消す。
    pub fn revoke_collection_authority(
        ctx: Context<CollectionAuthority>,
        tee_signing_pubkey: Pubkey,
        collection_type: u8,
    ) -> Result<()> {
        let config = &ctx.accounts.global_config;

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
#[account]
pub struct GlobalConfigAccount {
    /// DAO multi-sigのウォレットアドレス
    pub authority: Pubkey,
    /// Core cNFTの公式コレクションMintアドレス
    pub core_collection_mint: Pubkey,
    /// Extension cNFTの公式コレクションMintアドレス
    pub ext_collection_mint: Pubkey,
    /// 信頼されたTEEノードのリスト
    pub trusted_tee_nodes: Vec<TrustedTeeNodeAccount>,
    /// 信頼するTSA公開鍵ハッシュのリスト
    pub trusted_tsa_keys: Vec<[u8; 32]>,
    /// 信頼されたWASMモジュールのリスト
    pub trusted_wasm_modules: Vec<TrustedWasmModuleAccount>,
}

/// 信頼されたTEEノード情報（オンチェーン）。
/// 仕様書 §5.2 Step 1
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct TrustedTeeNodeAccount {
    /// Ed25519署名用公開鍵（32バイト）
    pub signing_pubkey: [u8; 32],
    /// X25519暗号化用公開鍵（32バイト）
    pub encryption_pubkey: [u8; 32],
    /// Gateway署名用Ed25519公開鍵
    pub gateway_pubkey: [u8; 32],
    /// ノードステータス (0=Inactive, 1=Active)
    pub status: u8,
    /// TEE種別 (0=aws_nitro, 1=amd_sev_snp, 2=intel_tdx)
    pub tee_type: u8,
}

/// 信頼されたWASMモジュール情報（オンチェーン）。
/// 仕様書 §5.2 Step 1
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct TrustedWasmModuleAccount {
    /// Extension識別子（最大32バイト）
    pub extension_id: [u8; 32],
    /// WASMバイナリのSHA-256ハッシュ
    pub wasm_hash: [u8; 32],
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
        space = 8 + 32 + 32 + 32 + 4 + 4 + 4 + 1024,
        seeds = [b"global-config"],
        bump
    )]
    pub global_config: Account<'info, GlobalConfigAccount>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
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
    pub authority: Signer<'info>,
    /// コレクションのMPL Coreアセットアカウント。
    /// CHECK: collection mintの一致はプログラム内で検証する。
    pub collection: UncheckedAccount<'info>,
}

// ---------------------------------------------------------------------------
// イベント
// ---------------------------------------------------------------------------

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
    /// TEEのsigning_pubkeyがtrusted_tee_nodesに含まれていない
    #[msg("TEEのsigning_pubkeyがtrusted_tee_nodesに含まれていません")]
    UntrustedTeeNode,
}
