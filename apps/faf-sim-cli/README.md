# faf-sim-cli

Launcher for the Bevy-powered FAF eco/build simulator.

## Native run

```sh
cargo run --bin faf-sim -- run
```

## Web / WASM run

The fastest way is through the workspace `xtask`:

```sh
cargo xtask web              # build + serve on port 8080
cargo xtask web --release    # use release builds
cargo xtask web serve --port 3000
```

Then open `http://localhost:8080` in a browser.

### Manual steps

If you prefer to run each step yourself:

```sh
rustup target add wasm32-unknown-unknown
cargo build --bin faf-sim --target wasm32-unknown-unknown --features web --release
wasm-bindgen \
  --out-dir apps/faf-sim-cli/web \
  --out-name faf_sim \
  --target web \
  target/wasm32-unknown-unknown/release/faf-sim.wasm
cargo run --bin faf-sim -- serve --port 8080
```

## How to play

- Click an empty tile to queue a T1 mass extractor.
- The ACU will build it, draining mass and energy from the shared economy pool.
- Watch the mass/energy income and storage in the top-left overlay.
