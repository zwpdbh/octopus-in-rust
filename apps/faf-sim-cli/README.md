# faf-sim-cli

Launcher for the Bevy-powered FAF eco/build simulator.

## Native run

```sh
cargo run --bin faf-sim -- run
```

## Native run via xtask

```sh
cargo xtask faf-sim              # run the native simulator
cargo xtask faf-sim --release    # use release builds
```

## Web / WASM run

The fastest way is through the workspace `xtask`:

```sh
cargo xtask faf-sim web              # build + serve on port 8080
cargo xtask faf-sim web --release    # use release builds
cargo xtask faf-sim web serve --port 3000
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

- Click a unit to select it.
- If the unit can build, a build palette opens at the bottom; pick a target.
- Left-click the board to place the target (factories build mobile units on their own tile).
- Right-click or Esc cancels the active build target.
- Watch the economy, selected unit info, and category counts in the UI panels.
