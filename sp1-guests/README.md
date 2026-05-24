# sp1-guests/

SP1 zkVM guest programs and their host harnesses. Production code, intentionally
held outside the main Cargo workspace.

## Why this is not under `crates/`

Each subdirectory here is its own `[workspace]` because SP1 guests have build
requirements that conflict with the main workspace:

- **Guest crates** (`*/program/`) target the SP1 RISC-V zkVM and are built with
  `cargo prove build`, not `cargo build`. Including them in the main workspace
  would break `cargo build --workspace`.
- **Host crates** (`*/host/`) invoke `sp1_build::build_program(...)` from their
  own `build.rs`, which kicks off the SP1 toolchain at compile time. If a host
  crate sat in the main workspace, an unrelated developer running
  `cargo build --workspace` would trigger a long SP1 build (proving alone takes
  ~90 minutes on CPU).

The main workspace's `Cargo.toml` has `exclude = [..., "sp1-guests"]` for this
reason.

## Layout

```
sp1-guests/
└── attestation-aws-nitro/
    ├── program/   SP1 guest: verifies an AWS Nitro Attestation Document
    │              and commits (instance_id, timestamp, measurement,
    │              user_data_hash, public_key_hash) as public values.
    └── host/      CLI:
                     `cargo run --bin vkey`  — print the guest's vkey_hash
                                                (embed in title-whitelist).
                     `cargo run --bin prove` — generate a Groth16 proof from
                                                a captured Attestation Document.
```

New vendor support is added by creating a sibling `attestation-<vendor>/` with
the same `program/` + `host/` shape — no main-workspace changes required.

## Running

```bash
# Print the verifying-key hash to embed in the Solana program.
cd sp1-guests/attestation-aws-nitro/host
cargo run --release --bin vkey

# Generate a Groth16 proof (slow: ~90 min on CPU).
cargo run --release --bin prove -- /path/to/attestation.bin
```

> `prove` peaks at roughly 30 GiB resident memory during the Groth16 wrap.
> Use an instance with at least 64 GiB RAM (EC2 `r5.4xlarge` or larger).
> Run `cargo run --release --bin prove -- --help` for the full output-file
> layout and a recap of the resource requirements.

> Always build with `cargo build --locked` (or `cargo prove build --locked`).
> The committed `Cargo.lock` pins the SP1 SDK to the exact version that
> produced the on-chain `APPROVED_VKEYS` constant; an unlocked `cargo update`
> would silently change the vkey hash and invalidate every existing
> `register_key` on-chain.

See [docs/v0.1.2/OPERATIONS_JA.md](../docs/v0.1.2/OPERATIONS_JA.md) §2.4 / §4
for the full SP1 + on-chain lifecycle.
