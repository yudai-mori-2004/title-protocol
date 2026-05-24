// SPDX-License-Identifier: Apache-2.0
//
// `vkey` — print the SP1 verifying-key hash of the attestation guest.
//
// Embed this constant in the Solana whitelist program (`APPROVED_VKEYS`) so the
// on-chain verifier accepts only proofs produced by this exact guest.

use std::time::{SystemTime, UNIX_EPOCH};

use title_sp1_attestation_aws_nitro_host::vkey_hash;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let hash = vkey_hash().await?;
    // Metadata on stderr so the stdout line stays machine-friendly
    // (`vkey > vkey_hash.hex` keeps a single hex value).
    let captured = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    eprintln!(
        "# guest: title-sp1-attestation-aws-nitro-program {}",
        env!("CARGO_PKG_VERSION")
    );
    eprintln!("# captured: {captured} (unix seconds)");
    println!("0x{}", hex::encode(hash));
    Ok(())
}
