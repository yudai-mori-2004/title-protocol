// SPDX-License-Identifier: Apache-2.0
//
// `vkey` — print the SP1 verifying-key hash of the attestation guest.
//
// Embed this constant in the Solana whitelist program (`APPROVED_VKEYS`) so the
// on-chain verifier accepts only proofs produced by this exact guest.

use title_sp1_attestation_aws_nitro_host::vkey_hash;

fn main() {
    let hash = vkey_hash();
    println!("0x{}", hex::encode(hash));
}
