// Reproducible SP1 guest build (Spec §5.4).
//
// sp1-build's Docker mode would be the canonical reproducible path, but it
// only mounts the guest crate directory into the container. Our guest
// depends on `../../../crates/attestation-aws-nitro` via a workspace
// `path = ...` reference; that dependency lives outside the mounted area
// and the build fails inside the container. We therefore stay on the local
// toolchain and strip host-specific paths from the ELF via
// `--remap-path-prefix` instead.
//
// Without these remaps the ELF's `.debug_*` sections (and panic location
// strings) embed `/Users/<you>/...` on Mac, `/home/ec2-user/...` on EC2,
// etc., so the resulting vkey hash differs across hosts even when the
// source, Cargo.lock, and SP1 toolchain are byte-identical. The Rust
// stdlib paths (`/Users/runner/work/rust/rust/...`) come from the
// succinct toolchain's prebuilt sysroot and are already identical across
// hosts, so they need no remap.

use sp1_build::BuildArgs;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR must be set by cargo");
    // host/build.rs lives at `<repo>/sp1-guests/attestation-aws-nitro/host`.
    // Walk three levels up to reach the repo root.
    let repo_root = std::path::Path::new(&manifest)
        .ancestors()
        .nth(3)
        .expect("manifest dir should have at least 3 ancestors")
        .to_string_lossy()
        .into_owned();
    let home = std::env::var("HOME").unwrap_or_default();

    let args = BuildArgs {
        locked: true,
        rustflags: vec![
            format!("--remap-path-prefix={repo_root}=/repo"),
            format!("--remap-path-prefix={home}/.cargo=/cargo"),
            format!("--remap-path-prefix={home}/.rustup=/rustup"),
        ],
        ..Default::default()
    };
    sp1_build::build_program_with_args("../program", args);
}
