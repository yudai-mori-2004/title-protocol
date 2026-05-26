// SPDX-License-Identifier: Apache-2.0

//! Whitelist subcommand implementations.
//!
//! Wraps `title_solana::whitelist_ix::*` builders into a single-instruction
//! `Transaction` submission and surfaces RPC errors verbatim. All admin-gated
//! commands assume the `--admin <path>` flag points at the program's
//! `ADMIN_AUTHORITY` keypair (`programs/title-whitelist::ADMIN_AUTHORITY`).

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    signer::Signer,
    transaction::Transaction,
};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use title_solana::whitelist::{
    derive_approved_measurements_pda, derive_approved_vkeys_pda, derive_whitelist_pda,
    StoredMeasurement, WhitelistEntry, WHITELIST_PROGRAM_ID,
};
use title_solana::whitelist_ix::{
    build_add_approved_measurement_ix, build_add_approved_vkey_ix,
    build_initialize_approved_measurements_ix, build_initialize_approved_vkeys_ix,
    build_register_key_ix, build_remove_approved_measurement_ix, build_remove_approved_vkey_ix,
    build_revoke_key_ix, proof_bytes_for_program, ON_CHAIN_PROOF_LEN,
};

/// Compute-unit limit for `register_key`. The on-chain Groth16 verification
/// (`sp1_solana::verify_proof`) measures around 280K CU on devnet; the default
/// 200K is not enough. Mirrors `crates/solana/src/cnft.rs::CU_LIMIT_CREATE_TREE`
/// for a comfortable margin.
const CU_LIMIT_REGISTER_KEY: u32 = 400_000;

/// Load a Solana keypair (full 64-byte format, as produced by `solana-keygen`)
/// from a JSON file. Use for admin / payer signers in whitelist commands.
///
/// This is distinct from `crate::solana::load_keypair`, which returns an
/// `ed25519_dalek::SigningKey` used for cNFT mint flows. Whitelist commands
/// build legacy `Transaction`s that take `solana_sdk::Keypair` signers
/// directly.
pub fn load_solana_keypair(path: &str) -> Result<Keypair, String> {
    let data =
        std::fs::read_to_string(path).map_err(|e| format!("failed to read keypair {path}: {e}"))?;
    let bytes: Vec<u8> =
        serde_json::from_str(&data).map_err(|e| format!("failed to parse keypair JSON: {e}"))?;
    Keypair::try_from(bytes.as_slice())
        .map_err(|e| format!("invalid keypair bytes in {path}: {e}"))
}

/// Parse a 32-byte hex string (with or without `0x` prefix). Returns the
/// canonical byte array used by both `vkey_hash` and `signing_pubkey` args.
fn parse_hex32(s: &str) -> Result<[u8; 32], String> {
    let trimmed = s.trim().trim_start_matches("0x");
    let bytes = hex::decode(trimmed).map_err(|e| format!("hex decode failed: {e}"))?;
    bytes
        .try_into()
        .map_err(|v: Vec<u8>| format!("expected 32 bytes, got {}", v.len()))
}

/// Parse a 48-byte PCR0 measurement hex (AWS Nitro Sha384). Length is
/// validated against `MAX_MEASUREMENT_LEN = 64`.
fn parse_pcr0_hex(s: &str) -> Result<Vec<u8>, String> {
    let trimmed = s.trim().trim_start_matches("0x");
    let bytes = hex::decode(trimmed).map_err(|e| format!("hex decode failed: {e}"))?;
    if bytes.is_empty() || bytes.len() > 64 {
        return Err(format!(
            "measurement length {} out of range 1..=64",
            bytes.len()
        ));
    }
    Ok(bytes)
}

/// Parse a base58 Solana pubkey into raw 32-byte form.
fn parse_pubkey32(s: &str) -> Result<[u8; 32], String> {
    let pk = Pubkey::from_str(s.trim()).map_err(|e| format!("invalid pubkey: {e}"))?;
    Ok(pk.to_bytes())
}

fn rpc(url: &str) -> RpcClient {
    RpcClient::new_with_commitment(url.to_string(), CommitmentConfig::confirmed())
}

fn submit(rpc_url: &str, ixs: &[solana_sdk::instruction::Instruction], signers: &[&Keypair]) -> Result<Signature, String> {
    let client = rpc(rpc_url);
    let blockhash = client
        .get_latest_blockhash()
        .map_err(|e| format!("failed to fetch blockhash: {e}"))?;
    let payer_pk = signers
        .first()
        .ok_or("no signer provided")?
        .pubkey();
    let tx = Transaction::new_signed_with_payer(ixs, Some(&payer_pk), signers, blockhash);
    client
        .send_and_confirm_transaction(&tx)
        .map_err(|e| format!("submit failed: {e}"))
}

fn explorer_url(sig: &Signature, cluster: &str) -> String {
    format!("https://explorer.solana.com/tx/{sig}?cluster={cluster}")
}

fn cluster_label(rpc_url: &str) -> &'static str {
    if rpc_url.contains("devnet") {
        "devnet"
    } else if rpc_url.contains("testnet") {
        "testnet"
    } else {
        "mainnet-beta"
    }
}

/// `init-registries` — create `ApprovedVkeys` and `ApprovedMeasurements`
/// PDAs in one transaction. Idempotent: re-runs report "already initialized"
/// without error.
pub fn cmd_init_registries(admin_path: &str, rpc_url: &str) -> Result<(), String> {
    let admin = load_solana_keypair(admin_path)?;
    let client = rpc(rpc_url);

    let (vkeys_pda, _) = derive_approved_vkeys_pda();
    let (meas_pda, _) = derive_approved_measurements_pda();

    let vkeys_exists = client
        .get_account_with_commitment(&vkeys_pda, CommitmentConfig::confirmed())
        .map(|r| r.value.is_some())
        .unwrap_or(false);
    let meas_exists = client
        .get_account_with_commitment(&meas_pda, CommitmentConfig::confirmed())
        .map(|r| r.value.is_some())
        .unwrap_or(false);

    println!("admin:                  {}", admin.pubkey());
    println!("rpc:                    {rpc_url}");
    println!("approved_vkeys PDA:        {vkeys_pda} ({})", if vkeys_exists { "exists" } else { "missing" });
    println!("approved_measurements PDA: {meas_pda} ({})", if meas_exists { "exists" } else { "missing" });

    let mut ixs = Vec::new();
    if !vkeys_exists {
        ixs.push(build_initialize_approved_vkeys_ix(&admin.pubkey()));
    }
    if !meas_exists {
        ixs.push(build_initialize_approved_measurements_ix(&admin.pubkey()));
    }

    if ixs.is_empty() {
        println!("both registries already initialized, nothing to do");
        return Ok(());
    }

    let sig = submit(rpc_url, &ixs, &[&admin])?;
    println!("tx:                     {sig}");
    println!("explorer:               {}", explorer_url(&sig, cluster_label(rpc_url)));
    Ok(())
}

/// `add-vkey --hex 0x...` — append a SP1 verifying-key hash to the allowlist.
pub fn cmd_add_vkey(admin_path: &str, vkey_hex: &str, rpc_url: &str) -> Result<(), String> {
    let admin = load_solana_keypair(admin_path)?;
    let vkey = parse_hex32(vkey_hex)?;

    println!("admin:    {}", admin.pubkey());
    println!("vkey:     0x{}", hex::encode(vkey));
    println!("rpc:      {rpc_url}");

    let ix = build_add_approved_vkey_ix(&admin.pubkey(), &vkey);
    let sig = submit(rpc_url, &[ix], &[&admin])?;
    println!("tx:       {sig}");
    println!("explorer: {}", explorer_url(&sig, cluster_label(rpc_url)));
    Ok(())
}

/// `remove-vkey --hex 0x...` — remove a SP1 verifying-key hash from the allowlist.
pub fn cmd_remove_vkey(admin_path: &str, vkey_hex: &str, rpc_url: &str) -> Result<(), String> {
    let admin = load_solana_keypair(admin_path)?;
    let vkey = parse_hex32(vkey_hex)?;

    println!("admin:    {}", admin.pubkey());
    println!("vkey:     0x{}", hex::encode(vkey));

    let ix = build_remove_approved_vkey_ix(&admin.pubkey(), &vkey);
    let sig = submit(rpc_url, &[ix], &[&admin])?;
    println!("tx:       {sig}");
    println!("explorer: {}", explorer_url(&sig, cluster_label(rpc_url)));
    Ok(())
}

/// `add-measurement --pcr0 <hex>` — append a TEE measurement (e.g. AWS Nitro
/// PCR0, 48 bytes) to the allowlist. Accepts hex with or without `0x` prefix.
pub fn cmd_add_measurement(admin_path: &str, pcr0_hex: &str, rpc_url: &str) -> Result<(), String> {
    let admin = load_solana_keypair(admin_path)?;
    let measurement = parse_pcr0_hex(pcr0_hex)?;

    println!("admin:       {}", admin.pubkey());
    println!("measurement: 0x{} ({} bytes)", hex::encode(&measurement), measurement.len());

    let ix = build_add_approved_measurement_ix(&admin.pubkey(), &measurement);
    let sig = submit(rpc_url, &[ix], &[&admin])?;
    println!("tx:          {sig}");
    println!("explorer:    {}", explorer_url(&sig, cluster_label(rpc_url)));
    Ok(())
}

/// `remove-measurement --pcr0 <hex>` — remove a TEE measurement from the allowlist.
pub fn cmd_remove_measurement(admin_path: &str, pcr0_hex: &str, rpc_url: &str) -> Result<(), String> {
    let admin = load_solana_keypair(admin_path)?;
    let measurement = parse_pcr0_hex(pcr0_hex)?;

    println!("admin:       {}", admin.pubkey());
    println!("measurement: 0x{}", hex::encode(&measurement));

    let ix = build_remove_approved_measurement_ix(&admin.pubkey(), &measurement);
    let sig = submit(rpc_url, &[ix], &[&admin])?;
    println!("tx:          {sig}");
    println!("explorer:    {}", explorer_url(&sig, cluster_label(rpc_url)));
    Ok(())
}

/// `register-key --bundle <dir>` — submit a `register_key` transaction using
/// the standard 4-file bundle produced by `fetch-registration-bundle.sh` +
/// `attestation-aws-nitro/host/prove`:
///
/// ```text
/// <dir>/
///   solana_pubkey.txt              # base58 TEE Ed25519 pubkey
///   attestation.bin.proof.bin      # 260-byte SP1 Groth16 proof
///   attestation.bin.public_values.bin # ZKP public values envelope
///   attestation.bin.vkey_hash.hex  # 0x... SP1 verifying-key hash (hex)
/// ```
pub fn cmd_register_key(payer_path: &str, bundle_dir: &str, rpc_url: &str) -> Result<(), String> {
    let payer = load_solana_keypair(payer_path)?;
    let bundle = Path::new(bundle_dir);

    let signing_pubkey = read_signing_pubkey(bundle)?;
    let vkey_hash = read_vkey_hash(bundle)?;
    let proof_raw = read_bundle_file(bundle, "attestation.bin.proof.bin")?;
    let public_values = read_bundle_file(bundle, "attestation.bin.public_values.bin")?;

    let proof = proof_bytes_for_program(&proof_raw)?;
    if proof.len() != ON_CHAIN_PROOF_LEN {
        return Err(format!(
            "proof normalization failed: got {} bytes, expected {ON_CHAIN_PROOF_LEN}",
            proof.len()
        ));
    }

    let (entry_pda, _) = derive_whitelist_pda(&signing_pubkey);

    println!("payer:           {}", payer.pubkey());
    println!("signing_pubkey:  {}", Pubkey::new_from_array(signing_pubkey));
    println!("vkey_hash:       0x{}", hex::encode(vkey_hash));
    println!("proof bytes:     {}", proof.len());
    println!("public_values:   {} bytes", public_values.len());
    println!("entry PDA:       {entry_pda}");
    println!("rpc:             {rpc_url}");

    let cu_ix = ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT_REGISTER_KEY);
    let ix = build_register_key_ix(
        &payer.pubkey(),
        &signing_pubkey,
        &vkey_hash,
        &proof,
        &public_values,
    );
    let sig = submit(rpc_url, &[cu_ix, ix], &[&payer])?;
    println!("tx:              {sig}");
    println!("explorer:        {}", explorer_url(&sig, cluster_label(rpc_url)));
    println!("WhitelistEntry PDA created and valid for 90 days.");
    Ok(())
}

/// `revoke-key --pubkey <base58>` — admin sets `revoked = true` on the
/// WhitelistEntry PDA. The PDA is NOT closed; replays of the same proof
/// would re-register a revoked key otherwise.
pub fn cmd_revoke_key(admin_path: &str, signing_pubkey_b58: &str, rpc_url: &str) -> Result<(), String> {
    let admin = load_solana_keypair(admin_path)?;
    let signing_pubkey = parse_pubkey32(signing_pubkey_b58)?;
    let (entry_pda, _) = derive_whitelist_pda(&signing_pubkey);

    println!("admin:           {}", admin.pubkey());
    println!("signing_pubkey:  {signing_pubkey_b58}");
    println!("entry PDA:       {entry_pda}");

    let ix = build_revoke_key_ix(&admin.pubkey(), &signing_pubkey);
    let sig = submit(rpc_url, &[ix], &[&admin])?;
    println!("tx:              {sig}");
    println!("explorer:        {}", explorer_url(&sig, cluster_label(rpc_url)));
    Ok(())
}

/// `describe-whitelist` — read and pretty-print on-chain state:
///   - approved vkeys + admin
///   - approved measurements + admin
///   - optional WhitelistEntry lookup by --signing-pubkey
pub fn cmd_describe(rpc_url: &str, lookup_pubkey: Option<&str>) -> Result<(), String> {
    let client = rpc(rpc_url);

    println!("program:                {}", WHITELIST_PROGRAM_ID);
    println!("rpc:                    {rpc_url}");
    println!();

    let (vkeys_pda, _) = derive_approved_vkeys_pda();
    println!("== ApprovedVkeys ({}) ==", vkeys_pda);
    match client.get_account(&vkeys_pda) {
        Ok(account) => {
            let parsed = parse_approved_vkeys(&account.data)?;
            println!("  admin:    {}", parsed.admin);
            println!("  count:    {}", parsed.vkeys.len());
            for (i, vk) in parsed.vkeys.iter().enumerate() {
                println!("  [{i:>2}]      0x{}", hex::encode(vk));
            }
        }
        Err(e) => println!("  (not initialized: {e})"),
    }
    println!();

    let (meas_pda, _) = derive_approved_measurements_pda();
    println!("== ApprovedMeasurements ({}) ==", meas_pda);
    match client.get_account(&meas_pda) {
        Ok(account) => {
            let parsed = parse_approved_measurements(&account.data)?;
            println!("  admin:    {}", parsed.admin);
            println!("  count:    {}", parsed.entries.len());
            for (i, m) in parsed.entries.iter().enumerate() {
                println!("  [{i:>2}]      0x{} ({} bytes)", hex::encode(m.as_bytes()), m.len);
            }
        }
        Err(e) => println!("  (not initialized: {e})"),
    }

    if let Some(pubkey_b58) = lookup_pubkey {
        let pk = parse_pubkey32(pubkey_b58)?;
        let (entry_pda, _) = derive_whitelist_pda(&pk);
        println!();
        println!("== WhitelistEntry for {pubkey_b58} ({entry_pda}) ==");
        match client.get_account(&entry_pda) {
            Ok(account) => {
                let parsed = parse_whitelist_entry(&account.data)?;
                println!("  signing_pubkey: {}", Pubkey::new_from_array(parsed.signing_pubkey));
                println!("  registered_at:  {} ({})", parsed.registered_at, unix_to_str(parsed.registered_at));
                println!("  expires_at:     {} ({})", parsed.expires_at, unix_to_str(parsed.expires_at));
                println!("  measurement:    0x{} ({} bytes)",
                    hex::encode(parsed.measurement.as_bytes()),
                    parsed.measurement.len);
                println!("  revoked:        {}", parsed.revoked);
                println!("  bump:           {}", parsed.bump);
            }
            Err(e) => println!("  (no entry: {e})"),
        }
    }

    Ok(())
}

// --- bundle file readers ---

fn bundle_path(bundle: &Path, name: &str) -> PathBuf {
    bundle.join(name)
}

fn read_bundle_file(bundle: &Path, name: &str) -> Result<Vec<u8>, String> {
    let path = bundle_path(bundle, name);
    std::fs::read(&path).map_err(|e| format!("failed to read {}: {e}", path.display()))
}

fn read_bundle_string(bundle: &Path, name: &str) -> Result<String, String> {
    let path = bundle_path(bundle, name);
    std::fs::read_to_string(&path).map_err(|e| format!("failed to read {}: {e}", path.display()))
}

fn read_signing_pubkey(bundle: &Path) -> Result<[u8; 32], String> {
    let raw = read_bundle_string(bundle, "solana_pubkey.txt")?;
    parse_pubkey32(raw.trim())
}

fn read_vkey_hash(bundle: &Path) -> Result<[u8; 32], String> {
    let raw = read_bundle_string(bundle, "attestation.bin.vkey_hash.hex")?;
    parse_hex32(raw.trim())
}

// --- on-chain account deserializers ---
//
// Layout matches `programs/title-whitelist/src/lib.rs` exactly. We avoid an
// Anchor dep here so the CLI doesn't pull in solana-program; instead we
// hand-parse the well-known field order.

struct ApprovedVkeysAccount {
    admin: Pubkey,
    vkeys: Vec<[u8; 32]>,
}

fn parse_approved_vkeys(data: &[u8]) -> Result<ApprovedVkeysAccount, String> {
    // discriminator(8) + admin(32) + vkeys_len(4) + vkeys(32*N) + bump(1)
    if data.len() < 8 + 32 + 4 + 1 {
        return Err(format!("ApprovedVkeys account too short: {} bytes", data.len()));
    }
    let admin = Pubkey::new_from_array(data[8..40].try_into().unwrap());
    let len = u32::from_le_bytes(data[40..44].try_into().unwrap()) as usize;
    let body_end = 44 + len * 32;
    if data.len() < body_end + 1 {
        return Err("ApprovedVkeys account length mismatch".into());
    }
    let mut vkeys = Vec::with_capacity(len);
    for i in 0..len {
        let off = 44 + i * 32;
        vkeys.push(data[off..off + 32].try_into().unwrap());
    }
    Ok(ApprovedVkeysAccount { admin, vkeys })
}

struct ApprovedMeasurementsAccount {
    admin: Pubkey,
    entries: Vec<StoredMeasurement>,
}

fn parse_approved_measurements(data: &[u8]) -> Result<ApprovedMeasurementsAccount, String> {
    // discriminator(8) + admin(32) + entries_len(4) + entries(65*N) + bump(1)
    if data.len() < 8 + 32 + 4 + 1 {
        return Err(format!("ApprovedMeasurements account too short: {} bytes", data.len()));
    }
    let admin = Pubkey::new_from_array(data[8..40].try_into().unwrap());
    let len = u32::from_le_bytes(data[40..44].try_into().unwrap()) as usize;
    let body_end = 44 + len * 65;
    if data.len() < body_end + 1 {
        return Err("ApprovedMeasurements account length mismatch".into());
    }
    let mut entries = Vec::with_capacity(len);
    for i in 0..len {
        let off = 44 + i * 65;
        let mut bytes = [0u8; 64];
        bytes.copy_from_slice(&data[off..off + 64]);
        let m_len = data[off + 64];
        entries.push(StoredMeasurement { bytes, len: m_len });
    }
    Ok(ApprovedMeasurementsAccount { admin, entries })
}

fn parse_whitelist_entry(data: &[u8]) -> Result<WhitelistEntry, String> {
    // discriminator(8) + signing_pubkey(32) + registered_at(8) + expires_at(8)
    //   + measurement.bytes(64) + measurement.len(1) + revoked(1) + bump(1)
    if data.len() < WhitelistEntry::SIZE {
        return Err(format!("WhitelistEntry too short: {} bytes", data.len()));
    }
    let signing_pubkey: [u8; 32] = data[8..40].try_into().unwrap();
    let registered_at = i64::from_le_bytes(data[40..48].try_into().unwrap());
    let expires_at = i64::from_le_bytes(data[48..56].try_into().unwrap());
    let mut bytes = [0u8; 64];
    bytes.copy_from_slice(&data[56..120]);
    let m_len = data[120];
    let revoked = data[121] != 0;
    let bump = data[122];
    Ok(WhitelistEntry {
        signing_pubkey,
        registered_at,
        expires_at,
        measurement: StoredMeasurement { bytes, len: m_len },
        revoked,
        bump,
    })
}

fn unix_to_str(unix_seconds: i64) -> String {
    use std::time::{Duration, UNIX_EPOCH};
    if unix_seconds < 0 {
        return "(invalid)".into();
    }
    let dt = UNIX_EPOCH + Duration::from_secs(unix_seconds as u64);
    format!("{:?}", dt)
}
