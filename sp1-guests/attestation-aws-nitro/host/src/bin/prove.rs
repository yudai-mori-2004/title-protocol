// SPDX-License-Identifier: Apache-2.0
//
// `prove` — run the SP1 prover over an AWS Nitro Attestation Document and emit
// the Groth16 proof bytes + public values bytes for on-chain `register_key`.

use std::fs;
use std::path::PathBuf;

use clap::Parser;

use title_sp1_attestation_aws_nitro_host::generate_groth16_proof;

#[derive(Parser, Debug)]
#[command(
    about = "Generate a Groth16 proof from an AWS Nitro Attestation Document.",
    long_about = "Output files (placed in the same directory as the input):
  <input>.proof.bin            Groth16 proof bytes
  <input>.public_values.bin    Public values bytes
  <input>.vkey_hash.hex        Verifying-key hash (sanity check)

Proving an Attestation Document on CPU takes ~90 minutes. Use a host with
sufficient memory; the SP1 prover allocates several GB of working set."
)]
struct Args {
    /// Path to the Attestation Document file (raw COSE_Sign1 bytes).
    attestation: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let doc = fs::read(&args.attestation)?;
    eprintln!(
        "Loaded {} bytes from {}",
        doc.len(),
        args.attestation.display()
    );
    eprintln!("Generating proof (this takes ~90 minutes on CPU)...");

    let artifacts = generate_groth16_proof(&doc).await?;

    let base = args.attestation.with_extension("");
    let proof_path = base.with_extension("proof.bin");
    let pv_path = base.with_extension("public_values.bin");
    let vkey_path = base.with_extension("vkey_hash.hex");

    fs::write(&proof_path, &artifacts.proof)?;
    fs::write(&pv_path, &artifacts.public_values)?;
    fs::write(&vkey_path, format!("0x{}\n", hex::encode(artifacts.vkey_hash)))?;

    eprintln!("Wrote:");
    eprintln!("  {}", proof_path.display());
    eprintln!("  {}", pv_path.display());
    eprintln!("  {}", vkey_path.display());
    Ok(())
}
