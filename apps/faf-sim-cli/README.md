# faf-sim-cli

Launcher for the Bevy-powered FAF eco/build simulator.

## Native run

```sh
cargo run --bin faf-sim -- run
```

## Web / WASM run

Build the CLI as a WASM module:

```sh
rustup target add wasm32-unknown-unknown
cargo build --bin faf-sim --target wasm32-unknown-unknown --features web --release
```

Bind it for the web:

```sh
wasm-bindgen \
  --out-dir apps/faf-sim-cli/web \
  --out-name faf_sim \
  --target web \
  target/wasm32-unknown-unknown/release/faf-sim.wasm
```

Serve it with the embedded Axum server:

```sh
cargo run --bin faf-sim -- serve --port 8080
```

Then open `http://localhost:8080` in a browser.

## How to play

- Click an empty tile to queue a T1 mass extractor.
- The ACU will build it, draining mass and energy from the shared economy pool.
- Watch the mass/energy income and storage in the top-left overlay.
