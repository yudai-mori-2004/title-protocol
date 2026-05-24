// SPDX-License-Identifier: Apache-2.0

//! Devnet integration tests for the Title Protocol whitelist program.
//!
//! Run with: cargo test -p title-solana --test devnet_whitelist -- --ignored --nocapture
//!
//! Prerequisites:
//! - Whitelist program deployed to devnet at 43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs
//! - Authority key at legacy/v0.1.0/keys/authority.json with SOL balance

use sha2::{Digest, Sha256};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    system_program,
    transaction::Transaction,
};
use std::str::FromStr;

const DEVNET_URL: &str = "https://api.devnet.solana.com";
const WHITELIST_PROGRAM_ID: &str = "43y8EUMJFJPFVs65yK9KDTtSK7fMiJQBBnMnKpz9yVzs";

fn whitelist_program() -> Pubkey {
    Pubkey::from_str(WHITELIST_PROGRAM_ID).unwrap()
}

fn load_authority_keypair() -> Keypair {
    let key_path = format!(
        "{}/keys/admin.json",
        env!("CARGO_MANIFEST_DIR").replace("/crates/solana", "")
    );
    let key_data = std::fs::read_to_string(&key_path)
        .unwrap_or_else(|_| panic!("Admin key not found at {key_path}"));
    let bytes: Vec<u8> = serde_json::from_str(&key_data).unwrap();
    Keypair::from_bytes(&bytes).unwrap()
}

/// Compute Anchor instruction discriminator: SHA-256("global:<name>")[0..8]
fn anchor_discriminator(name: &str) -> [u8; 8] {
    let hash = Sha256::digest(format!("global:{name}").as_bytes());
    hash[..8].try_into().unwrap()
}

/// Build a register_key instruction.
/// Anchor borsh layout: discriminator(8) + signing_pubkey([u8;32]) +
///   sp1_vkey_hash([u8;32]) + proof(Vec<u8>) + public_values(Vec<u8>)
fn build_register_key_ix(
    payer: &Pubkey,
    signing_pubkey: &[u8; 32],
    sp1_vkey_hash: &[u8; 32],
    proof: &[u8],
    public_values: &[u8],
) -> Instruction {
    let program_id = whitelist_program();

    // Derive PDAs
    let (whitelist_pda, _bump) = Pubkey::find_program_address(
        &[b"whitelist", signing_pubkey.as_ref()],
        &program_id,
    );
    let (approved_vkeys_pda, _) =
        Pubkey::find_program_address(&[b"approved_vkeys"], &program_id);
    let (approved_measurements_pda, _) =
        Pubkey::find_program_address(&[b"approved_measurements"], &program_id);

    // Build instruction data
    let disc = anchor_discriminator("register_key");
    let mut data = Vec::with_capacity(8 + 32 + 32 + 4 + proof.len() + 4 + public_values.len());
    data.extend_from_slice(&disc);
    data.extend_from_slice(signing_pubkey);
    data.extend_from_slice(sp1_vkey_hash);
    data.extend_from_slice(&(proof.len() as u32).to_le_bytes());
    data.extend_from_slice(proof);
    data.extend_from_slice(&(public_values.len() as u32).to_le_bytes());
    data.extend_from_slice(public_values);

    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(whitelist_pda, false),
            AccountMeta::new_readonly(approved_vkeys_pda, false),
            AccountMeta::new_readonly(approved_measurements_pda, false),
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data,
    }
}

/// Build an `initialize_approved_vkeys` instruction.
fn build_initialize_approved_vkeys_ix(admin: &Pubkey) -> Instruction {
    let program_id = whitelist_program();
    let (pda, _) = Pubkey::find_program_address(&[b"approved_vkeys"], &program_id);
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(pda, false),
            AccountMeta::new(*admin, true),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: anchor_discriminator("initialize_approved_vkeys").to_vec(),
    }
}

/// Build an `initialize_approved_measurements` instruction.
fn build_initialize_approved_measurements_ix(admin: &Pubkey) -> Instruction {
    let program_id = whitelist_program();
    let (pda, _) = Pubkey::find_program_address(&[b"approved_measurements"], &program_id);
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(pda, false),
            AccountMeta::new(*admin, true),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: anchor_discriminator("initialize_approved_measurements").to_vec(),
    }
}

/// Build a `revoke_key` instruction.
fn build_revoke_key_ix(
    admin: &Pubkey,
    signing_pubkey: &[u8; 32],
) -> Instruction {
    let program_id = whitelist_program();

    let (whitelist_pda, _bump) = Pubkey::find_program_address(
        &[b"whitelist", signing_pubkey.as_ref()],
        &program_id,
    );

    let disc = anchor_discriminator("revoke_key");

    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(whitelist_pda, false),
            AccountMeta::new_readonly(*admin, true),
        ],
        data: disc.to_vec(),
    }
}

#[test]
#[ignore]
fn program_is_deployed() {
    let client = RpcClient::new_with_commitment(DEVNET_URL, CommitmentConfig::confirmed());
    let program_id = whitelist_program();

    let account = client.get_account(&program_id).unwrap();
    assert!(account.executable, "Program should be executable");
    println!("Program deployed at {program_id}");
    println!("  Owner: {}", account.owner);
    println!("  Data length: {} bytes", account.data.len());
}

#[test]
#[ignore]
fn register_key_rejects_empty_proof() {
    let client = RpcClient::new_with_commitment(DEVNET_URL, CommitmentConfig::confirmed());
    let payer = load_authority_keypair();

    let signing_pubkey = [42u8; 32];
    let sp1_vkey_hash = [0u8; 32];

    let ix = build_register_key_ix(
        &payer.pubkey(),
        &signing_pubkey,
        &sp1_vkey_hash,
        &[],  // empty proof
        &[1], // non-empty public values
    );

    let blockhash = client.get_latest_blockhash().unwrap();
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &[&payer], blockhash);

    let result = client.send_and_confirm_transaction(&tx);
    assert!(result.is_err(), "Empty proof should be rejected");
    let err_msg = format!("{:?}", result.unwrap_err());
    println!("Expected error for empty proof: {err_msg}");
    assert!(
        err_msg.contains("custom program error")
            || err_msg.contains("InstructionError")
            || err_msg.contains("Error"),
        "Should contain error indication"
    );
}

#[test]
#[ignore]
fn register_key_rejects_invalid_proof() {
    let client = RpcClient::new_with_commitment(DEVNET_URL, CommitmentConfig::confirmed());
    let payer = load_authority_keypair();

    let signing_pubkey = [7u8; 32];
    let sp1_vkey_hash = [0u8; 32];

    // Fake proof (non-empty but invalid)
    let fake_proof = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04, 0x05];

    // Fake public values: minimum valid structure
    // module_id: empty string (4 bytes len = 0)
    // timestamp: 8 bytes
    // pcr0: 48 bytes
    // has_user_data: 1 byte (0)
    let mut fake_pv = Vec::new();
    fake_pv.extend_from_slice(&0u32.to_le_bytes()); // module_id len = 0
    fake_pv.extend_from_slice(&0u64.to_le_bytes()); // timestamp
    fake_pv.extend_from_slice(&[0u8; 48]);           // pcr0
    fake_pv.push(0);                                  // has_user_data = false

    let ix = build_register_key_ix(
        &payer.pubkey(),
        &signing_pubkey,
        &sp1_vkey_hash,
        &fake_proof,
        &fake_pv,
    );

    let blockhash = client.get_latest_blockhash().unwrap();
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &[&payer], blockhash);

    let result = client.send_and_confirm_transaction(&tx);
    assert!(result.is_err(), "Invalid proof should be rejected");
    let err_msg = format!("{:?}", result.unwrap_err());
    println!("Expected error for invalid proof: {err_msg}");
}

#[test]
#[ignore]
fn revoke_key_rejects_nonexistent_pda() {
    let client = RpcClient::new_with_commitment(DEVNET_URL, CommitmentConfig::confirmed());
    let admin = load_authority_keypair();

    // A key that was never registered
    let signing_pubkey = [99u8; 32];

    let ix = build_revoke_key_ix(&admin.pubkey(), &signing_pubkey);

    let blockhash = client.get_latest_blockhash().unwrap();
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&admin.pubkey()), &[&admin], blockhash);

    let result = client.send_and_confirm_transaction(&tx);
    assert!(result.is_err(), "Deleting nonexistent PDA should fail");
    let err_msg = format!("{:?}", result.unwrap_err());
    println!("Expected error for nonexistent PDA: {err_msg}");
}

#[test]
#[ignore]
fn revoke_key_rejects_non_admin() {
    let client = RpcClient::new_with_commitment(DEVNET_URL, CommitmentConfig::confirmed());

    // Use operator key instead of admin (authority)
    let key_path = format!(
        "{}/legacy/v0.1.0/keys/operator.json",
        env!("CARGO_MANIFEST_DIR").replace("/crates/solana", "")
    );
    let key_data = std::fs::read_to_string(&key_path)
        .unwrap_or_else(|_| panic!("Operator key not found at {key_path}"));
    let bytes: Vec<u8> = serde_json::from_str(&key_data).unwrap();
    let non_admin = Keypair::from_bytes(&bytes).unwrap();

    let signing_pubkey = [88u8; 32];

    let ix = build_revoke_key_ix(&non_admin.pubkey(), &signing_pubkey);

    let blockhash = client.get_latest_blockhash().unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&non_admin.pubkey()),
        &[&non_admin],
        blockhash,
    );

    let result = client.send_and_confirm_transaction(&tx);
    assert!(result.is_err(), "Non-admin should not be able to delete keys");
    let err_msg = format!("{:?}", result.unwrap_err());
    println!("Expected error for non-admin delete: {err_msg}");
}

#[test]
#[ignore]
fn cnft_mint_tx_construction() {
    use title_solana::cnft;
    use title_solana::signing_key::SolanaSigningKey;

    let key = SolanaSigningKey::generate(&mut rand::rngs::OsRng);
    let tree = Pubkey::new_unique();
    let owner = Pubkey::new_unique();

    let client = RpcClient::new_with_commitment(DEVNET_URL, CommitmentConfig::confirmed());
    let blockhash = client.get_latest_blockhash().unwrap();

    let tx = cnft::build_and_sign_mint_tx(
        &key,
        &tree,
        &owner,
        "sha256:devnet_test_hash",
        "https://example.com/devnet_test.json",
        None,
        &owner,
        &blockhash,
    )
    .unwrap();

    let tx_bytes = cnft::serialize_transaction(&tx).unwrap();
    println!("cNFT mint TX size: {} bytes (limit: 1232)", tx_bytes.len());
    assert!(tx_bytes.len() <= 1232, "TX must fit in Solana's 1232B limit");

    // Verify TEE signature is present
    let tee_pubkey = key.pubkey();
    let static_keys = tx.message.static_account_keys();
    let num_signers = tx.message.header().num_required_signatures as usize;

    let mut tee_signed = false;
    for i in 0..num_signers.min(static_keys.len()) {
        if static_keys[i] == tee_pubkey {
            assert_ne!(
                tx.signatures[i],
                solana_sdk::signature::Signature::default(),
            );
            tee_signed = true;
        }
    }
    assert!(tee_signed, "TEE key must have signed the transaction");
    println!("cNFT TX construction + TEE partial signing: OK");
}

/// Full end-to-end cNFT flow on devnet:
/// 1. Create Merkle tree (spl-account-compression V2)
/// 2. Mint cNFT with TEE partial signature
/// 3. Add payer signature and submit
#[test]
#[ignore]
fn cnft_full_flow_devnet() {
    use title_solana::cnft;
    use title_solana::signing_key::SolanaSigningKey;

    let client = RpcClient::new_with_commitment(DEVNET_URL, CommitmentConfig::confirmed());
    let payer = load_authority_keypair();
    let tee_key = SolanaSigningKey::generate(&mut rand::rngs::OsRng);
    let tree_keypair = Keypair::new();

    println!("=== cNFT Full Flow Devnet Test ===");
    println!("Payer: {}", payer.pubkey());
    println!("TEE signing key: {}", tee_key.pubkey_base58());
    println!("Tree keypair: {}", tree_keypair.pubkey());

    // --- Step 1: Create Merkle tree ---
    // depth=3, buffer=8 — minimal tree for testing (8 leaves)
    let max_depth = 3u32;
    let max_buffer_size = 8u32;

    let blockhash = client.get_latest_blockhash().unwrap();
    let mut create_tree_tx = cnft::build_create_tree_tx(
        &payer.pubkey(),
        &tree_keypair.pubkey(),
        &tee_key.pubkey(),
        max_depth,
        max_buffer_size,
        &blockhash,
    )
    .expect("build_create_tree_tx failed");

    // Sign with all 3 signers: payer, tree_keypair, tree_creator (TEE)
    // First pass: SDK keypairs. Second pass: TEE key (needs &mut).
    {
        let static_keys = create_tree_tx.message.static_account_keys().to_vec();
        let num_signers = create_tree_tx.message.header().num_required_signatures as usize;
        let message_bytes = create_tree_tx.message.serialize();
        for i in 0..num_signers.min(static_keys.len()) {
            if static_keys[i] == payer.pubkey() {
                create_tree_tx.signatures[i] = payer.sign_message(&message_bytes);
            } else if static_keys[i] == tree_keypair.pubkey() {
                create_tree_tx.signatures[i] = tree_keypair.sign_message(&message_bytes);
            }
        }
    }
    tee_key.sign_transaction(&mut create_tree_tx).unwrap();

    println!("\nStep 1: Creating Merkle tree (depth={max_depth}, buffer={max_buffer_size})...");
    let tree_sig = client
        .send_and_confirm_transaction(&create_tree_tx)
        .expect("create_tree TX failed");
    println!("  Tree created! TX: {tree_sig}");
    println!("  Tree address: {}", tree_keypair.pubkey());

    let (tree_config, _) = cnft::derive_tree_config(&tree_keypair.pubkey());
    println!("  Tree config PDA: {tree_config}");

    // --- Step 2: Mint cNFT ---
    println!("\nStep 2: Minting cNFT...");
    let blockhash = client.get_latest_blockhash().unwrap();
    let mut mint_tx = cnft::build_and_sign_mint_tx(
        &tee_key,
        &tree_keypair.pubkey(),
        &payer.pubkey(),
        "sha256:devnet_e2e_test_1234",
        "https://example.com/title/devnet_test.json",
        None,
        &payer.pubkey(),
        &blockhash,
    )
    .expect("build_and_sign_mint_tx failed");

    // Add payer signature
    {
        let mint_message_bytes = mint_tx.message.serialize();
        let mint_static_keys = mint_tx.message.static_account_keys().to_vec();
        let mint_num_signers = mint_tx.message.header().num_required_signatures as usize;
        for i in 0..mint_num_signers.min(mint_static_keys.len()) {
            if mint_static_keys[i] == payer.pubkey() {
                mint_tx.signatures[i] = payer.sign_message(&mint_message_bytes);
            }
        }
    }

    let mint_bytes = cnft::serialize_transaction(&mint_tx).unwrap();
    println!("  Mint TX size: {} bytes", mint_bytes.len());

    let mint_sig = client
        .send_and_confirm_transaction(&mint_tx)
        .expect("Mint cNFT TX failed");
    println!("  cNFT minted! TX: {mint_sig}");

    // --- Step 3: Verify ---
    println!("\nStep 3: Verifying...");
    let tree_account = client.get_account(&tree_keypair.pubkey()).unwrap();
    println!(
        "  Tree account: {} bytes, owner={}",
        tree_account.data.len(),
        tree_account.owner
    );
    assert_eq!(
        tree_account.owner,
        cnft::spl_account_compression_v2_id(),
        "Tree should be owned by spl-account-compression V2"
    );

    println!("\n=== cNFT Full Flow: SUCCESS ===");
    println!("Explorer: https://explorer.solana.com/tx/{mint_sig}?cluster=devnet");
}

// ===========================================================================
// Registry initialization (one-shot operational tests)
// ===========================================================================
//
// These tests perform admin-only setup of the on-chain registries. Run each
// exactly once per deployed program version, in this order:
//
//   1. `initialize_registries_devnet`     — create both PDAs
//   2. `add_placeholder_vkey_devnet`      — populate ApprovedVkeys with a
//                                           placeholder for test purposes;
//                                           replace with the real SP1 guest
//                                           vkey_hash once available
//   3. `add_placeholder_measurement_devnet` — same for ApprovedMeasurements
//
// Re-running #1 after success will fail with `already in use` — that's
// expected. Re-running #2/#3 after success will fail with
// `VkeyAlreadyApproved` / `MeasurementAlreadyApproved`.

/// Initialize both registry PDAs. Run once per deployed program.
#[test]
#[ignore]
fn initialize_registries_devnet() {
    let client = RpcClient::new_with_commitment(DEVNET_URL, CommitmentConfig::confirmed());
    let admin = load_authority_keypair();

    let init_vkeys = build_initialize_approved_vkeys_ix(&admin.pubkey());
    let init_meas = build_initialize_approved_measurements_ix(&admin.pubkey());

    let blockhash = client.get_latest_blockhash().unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[init_vkeys, init_meas],
        Some(&admin.pubkey()),
        &[&admin],
        blockhash,
    );

    match client.send_and_confirm_transaction(&tx) {
        Ok(sig) => {
            println!("Initialized both registry PDAs in {sig}");
            println!(
                "Explorer: https://explorer.solana.com/tx/{sig}?cluster=devnet"
            );
        }
        Err(e) => {
            let msg = format!("{e:?}");
            // Both PDAs init in a single tx; partial-init isn't possible here,
            // so the only legitimate failure mode is "already initialized".
            assert!(
                msg.contains("already in use") || msg.contains("0x0"),
                "unexpected init error: {msg}"
            );
            println!("Registries already initialized (idempotent skip)");
        }
    }
}

/// Add an SP1 verifying-key hash to the approved set. Until the real SP1
/// guest is built and its vkey captured, this registers a placeholder so the
/// devnet pipeline can be exercised. Replace the placeholder before production.
#[test]
#[ignore]
fn add_placeholder_vkey_devnet() {
    let client = RpcClient::new_with_commitment(DEVNET_URL, CommitmentConfig::confirmed());
    let admin = load_authority_keypair();
    let program_id = whitelist_program();
    let (registry_pda, _) =
        Pubkey::find_program_address(&[b"approved_vkeys"], &program_id);

    // Placeholder: deterministic non-zero bytes so it's recognizable.
    // Real value comes from `sp1-guests/attestation-aws-nitro/host: cargo run --bin vkey`.
    let placeholder: [u8; 32] = [0xAA; 32];

    // `vkey_hash: [u8; 32]` is a Borsh fixed-length array — emit the 32
    // bytes raw without a length prefix (Vec<u8> would prefix with u32).
    let mut data = anchor_discriminator("add_approved_vkey").to_vec();
    data.extend_from_slice(&placeholder);

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(registry_pda, false),
            AccountMeta::new_readonly(admin.pubkey(), true),
        ],
        data,
    };

    let blockhash = client.get_latest_blockhash().unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&admin.pubkey()),
        &[&admin],
        blockhash,
    );

    match client.send_and_confirm_transaction(&tx) {
        Ok(sig) => println!("Added placeholder vkey in {sig}"),
        Err(e) => {
            let msg = format!("{e:?}");
            // VkeyAlreadyApproved = 6009 = 0x1779 (post-InvalidProofLength).
            assert!(
                msg.contains("0x1779") || msg.contains("VkeyAlreadyApproved"),
                "unexpected error: {msg}"
            );
            println!("Placeholder vkey already approved (idempotent skip)");
        }
    }
}

/// Add a TEE measurement to the approved set. Placeholder for now —
/// replace with the real Nitro PCR0 once the EIF is built.
#[test]
#[ignore]
fn add_placeholder_measurement_devnet() {
    let client = RpcClient::new_with_commitment(DEVNET_URL, CommitmentConfig::confirmed());
    let admin = load_authority_keypair();
    let program_id = whitelist_program();
    let (registry_pda, _) =
        Pubkey::find_program_address(&[b"approved_measurements"], &program_id);

    // Placeholder 48-byte measurement, matching AWS Nitro PCR0 size.
    let placeholder: Vec<u8> = vec![0xBB; 48];

    let mut data = anchor_discriminator("add_approved_measurement").to_vec();
    data.extend_from_slice(&(placeholder.len() as u32).to_le_bytes());
    data.extend_from_slice(&placeholder);

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(registry_pda, false),
            AccountMeta::new_readonly(admin.pubkey(), true),
        ],
        data,
    };

    let blockhash = client.get_latest_blockhash().unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&admin.pubkey()),
        &[&admin],
        blockhash,
    );

    match client.send_and_confirm_transaction(&tx) {
        Ok(sig) => println!("Added placeholder measurement in {sig}"),
        Err(e) => {
            let msg = format!("{e:?}");
            // MeasurementAlreadyApproved = 6012 = 0x177c (post-InvalidProofLength).
            assert!(
                msg.contains("0x177c") || msg.contains("MeasurementAlreadyApproved"),
                "unexpected error: {msg}"
            );
            println!("Placeholder measurement already approved (idempotent skip)");
        }
    }
}
