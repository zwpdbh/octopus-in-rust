# Cookbook: Allow Unresolved Symbols in a macOS `cdylib` Build Script

## Problem

Extism plugins in this workspace are `cdylib` crates that import host functions such as `_alloc`, `_error_set`, `_input_length`, and `_output_set`. Those symbols are provided by the Extism runtime when the plugin runs as a `.wasm` module, but they do **not** exist on the native host.

On Linux, the linker allows unresolved symbols in a shared library, so a plain `cargo build` succeeds. On macOS, the linker treats them as a fatal error:

```text
Undefined symbols for architecture arm64:
  "_alloc", referenced from:
      extism_pdk::memory::Memory::new::... in libextism_pdk-....rlib
  "_error_set", referenced from:
      _execute in example_http....rcgu.o
  ...
ld: symbol(s) not found for architecture arm64
clang: error: linker command failed with exit code 1
```

You still want `cargo build` to work from the workspace root without excluding the plugin crates from the workspace.

## Solution

Add a `build.rs` next to each plugin's `Cargo.toml` that emits a macOS-only linker flag:

```rust
// plugins/example-http/build.rs ~line 1 — macOS linker workaround
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("apple-darwin") {
        // The Extism PDK imports host functions that only exist in the Wasm
        // runtime. When `cargo build` reaches this cdylib for the native macOS
        // target, tell the linker to leave those symbols unresolved. Real
        // plugin artifacts are still produced with
        // `--target wasm32-unknown-unknown`.
        println!("cargo:rustc-link-arg=-Wl,-undefined,dynamic_lookup");
    }
}
```

The same file is used for every plugin crate:

- `plugins/example-http/build.rs`
- `plugins/faf-units/build.rs`
- `plugins/faf-party/build.rs`

## Why This Works

Cargo runs `build.rs` before compiling the crate and captures lines printed to stdout that start with `cargo:`. Those lines are instructions to Cargo, not ordinary console output.

`cargo:rustc-link-arg=...` tells Cargo: "pass this exact flag to the linker when building this crate." The flag `-Wl,-undefined,dynamic_lookup` tells the Apple linker to leave undefined symbols as dynamically looked-up references instead of hard errors.

Because the check is based on the `TARGET` environment variable, the flag is only emitted when building for macOS. Linux and Windows builds are unaffected, and the real plugin artifact is still the `.wasm` file produced by:

```bash
cargo build -p faf-units-plugin --target wasm32-unknown-unknown
```

## Trade-off

The native `.dylib` produced by `cargo build` on macOS is **not a usable plugin**; it is only a build by-product. The flag relaxes the linker just enough to let the workspace compile. If a real missing symbol bug is introduced, the linker will no longer catch it for these plugin crates. Host crates still link normally and continue to catch missing symbols.

## When to Use

- A workspace mixes host binaries/libraries with Wasm plugin crates that use `extism-pdk` or any other PDK that imports host-only symbols.
- You want `cargo build`, `cargo check --workspace`, and `cargo test --workspace` to work on macOS without maintaining a `default-members` list or excluding plugins from the workspace.
- The real plugin output is still built explicitly with `--target wasm32-unknown-unknown`.

## When NOT to Use

- If you can use `workspace.default-members` to exclude plugins from the default build, that is a cleaner solution because it avoids producing a broken native `.dylib` at all.
- If the crate is meant to be a real native shared library (not a Wasm plugin), allowing unresolved symbols can hide real bugs.

## Real Example from the Codebase

**Files:** `plugins/example-http/build.rs`, `plugins/faf-units/build.rs`, `plugins/faf-party/build.rs`

These build scripts let `cargo build` succeed on macOS while the release workflow in `scripts/build-qqbot-release.sh` continues to build the actual plugins with `--target wasm32-unknown-unknown`.
