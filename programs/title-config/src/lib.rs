//! # Title Protocol Anchor Solanaプログラム
//!
//! 仕様書 §5.2 Step 1: Global Config PDAの管理。
//!
//! ## 命令
//! - `initialize`: Global Configの初期化
//! - `update_tee_nodes`: 信頼されたTEEノードリストの更新
//! - `update_wasm_modules`: 信頼されたWASMモジュールリストの更新
//! - `update_tsa_keys`: 信頼するTSA鍵リストの更新

use anchor_lang::prelude::*;

declare_id!("TiTLECfg111111111111111111111111111111111111");

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
