//! Rebuild the binary whenever the build tag changes — `xtask fafcn
//! file-sync` sets a fresh `FAFCN_SYNC_BUILD_TAG` per build, and without this
//! directive cargo would reuse the cached binary with the old tag.

fn main() {
    println!("cargo:rerun-if-env-changed=FAFCN_SYNC_BUILD_TAG");
}
