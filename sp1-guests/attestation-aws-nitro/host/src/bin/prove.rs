// SPDX-License-Identifier: Apache-2.0
//
// `prove` — run the SP1 prover over an AWS Nitro Attestation Document and emit
// the Groth16 proof bytes + public values bytes for on-chain `register_key`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::Parser;

use title_sp1_attestation_aws_nitro_host::generate_groth16_proof;

/// Hard cap on the input document size — well above any real AWS Nitro
/// document; refuses to spend 90 minutes on obvious garbage.
const MAX_DOC_BYTES: usize = 16 * 1024;

#[derive(Parser, Debug)]
#[command(
    about = "Generate a Groth16 proof from an AWS Nitro Attestation Document.",
    long_about = "Output files (placed in the same directory as the input):
  <input-filename>.proof.bin            Groth16 proof bytes
  <input-filename>.public_values.bin    Public values bytes
  <input-filename>.vkey_hash.hex        Verifying-key hash (sanity check)

Proving on CPU takes roughly 90 minutes and peaks at ~30 GiB resident
memory during the Groth16 wrap. Use an instance with at least 64 GiB of
RAM (e.g. EC2 r5.4xlarge or larger). Set RUST_LOG=info to see the SP1
progress bar."
)]
struct Args {
    /// Path to the Attestation Document file (raw COSE_Sign1 bytes).
    attestation: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // SP1 emits a tqdm-style progress bar via `tracing`; without a
    // subscriber the ~90 minute prove loop is silent — impossible to tell
    // "working" from "wedged".
    sp1_sdk::utils::setup_logger();

    let args = Args::parse();

    let doc = fs::read(&args.attestation)?;
    anyhow::ensure!(
        doc.len() <= MAX_DOC_BYTES,
        "attestation document too large ({} > {} bytes); aborting before SP1 setup",
        doc.len(),
        MAX_DOC_BYTES,
    );

    eprintln!(
        "Loaded {} bytes from {}",
        doc.len(),
        args.attestation.display()
    );

    let started = Instant::now();
    let artifacts = generate_groth16_proof(&doc).await?;
    let elapsed = started.elapsed();

    // Build sibling filenames by appending suffixes to the *complete*
    // filename. `Path::with_extension` would strip only the last
    // dot-segment and lose meaningful parts of names like
    // `nitro.v1.bin`, so use explicit `format!` instead.
    let stem = args
        .attestation
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid input filename"))?;
    let dir = args
        .attestation
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let proof_path = dir.join(format!("{stem}.proof.bin"));
    let pv_path = dir.join(format!("{stem}.public_values.bin"));
    let vkey_path = dir.join(format!("{stem}.vkey_hash.hex"));

    fs::write(&proof_path, &artifacts.proof)?;
    fs::write(&pv_path, &artifacts.public_values)?;
    fs::write(&vkey_path, format!("0x{}\n", hex::encode(artifacts.vkey_hash)))?;

    eprintln!("Proof generated in {elapsed:.1?}");
    eprintln!("Wrote:");
    eprintln!("  {}", proof_path.display());
    eprintln!("  {}", pv_path.display());
    eprintln!("  {}", vkey_path.display());
    Ok(())
}
