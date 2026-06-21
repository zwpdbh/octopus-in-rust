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
