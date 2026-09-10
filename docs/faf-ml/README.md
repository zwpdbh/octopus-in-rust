# FAF-ML: Unit Detection from Game Screenshots — Handover Doc

> **Read this first when you're back.** Goal of the project: point a model at
> a Forged Alliance (FAF) screenshot and get a per-player unit breakdown
> (the "analysis" view). All code lives in the **octopus** workspace; the
> separate `dive_into_deep_learning_in_rust` workspace is the d2l.ai learning
> track (chapters 2–10 ported; chapters 11+ not started).

## The one-paragraph version of the whole approach

Manual labeling doesn't scale, so we don't do it: `faf-ml-datagen` (driven
from the server's Datagen view) pastes the
game's own strategic-icon sprites onto crops of **empty-terrain screenshots**,
which yields perfectly labeled synthetic training data for free. A small SSD
detector (`crates/faf-ml-model`) trains on that. Real **battle** screenshots
are never trained on — they're the held-out exam. If the model fails on real
screenshots, fix the *generator* (colors/scale), not the labels. Later, the
model pre-labels real shots and you correct them in the web UI — that
correction set closes the domain gap.

## What's built and verified (as of 2026-09-09)

| Piece | Where | State |
|---|---|---|
| Synthetic data generator | `crates/faf-ml-datagen` (runs as a server job; the `faf-datagen` CLI is gone) | ✅ works; tint + scale + clustering; streams samples into the store as `synthetic` screenshots with JSON labels |
| Web platform | `apps/faf-ml-server` (:3100) + `apps/faf-ml-web` + `crates/faf-ml-core` | ✅ upload (drag&drop), triage badges, label view (edit boxes), dataset snapshots, datagen jobs (`POST /api/datagen` + polling) |
| Icon↔unit mapping | `crates/faf-unit-tools` (`icon-map` subcommand) | ✅ 114 classes ↔ 501 units; artifact at `data/faf-ml/icon-map.json` |
| SSD detector | `crates/faf-ml-model` + `apps/faf-ml-train` | ✅ implemented, 17/17 tests, smoke-trained; **never trained for real** |

Not built yet: training/eval inside the web UI (phases 2–3), Windows capture
client, the analysis view itself.

## ▶ Your next session, step by step

```bash
cd /home/zw/code/rust_programming/octopus

# 0. Start the platform
cargo xtask faf-ml backend          # → http://localhost:3100

# 1. COLLECT: in-game (Windows), take ~20 screenshots:
#    - EMPTY terrain (zoomed-out fog areas, several DIFFERENT maps) ← for backgrounds
#    - BUSY battles (several maps)                                  ← held-out exam
#    Upload ALL via the Gallery drop zone, then triage each card:
#    "background" = empty, "battle" = has units. (Default is "unclassified";
#    the "needs triage" filter shows what's left to mark.)

# 2. GENERATE synthetic data (reads ONLY background-marked shots):
#    open the Datagen view (http://localhost:3100/datagen), set count/size/
#    scale, Generate. Samples stream into the store while the job runs;
#    the jobs table polls until done.
#    Each sample is tagged with its job id, so a bad set is removed with the
#    job row's "Delete samples" button (deletes job + its samples). Older
#    untagged sets: Gallery → "synthetic" filter → "Clear N synthetic".
#    (Jobs live in server memory — a restart wipes the jobs table, but the
#    job→sample link is persisted in index.json.)

# 3. DOMAIN-GAP CHECK (5 min, do not skip): open a synthetic sample (Gallery
#    → "synthetic" filter) next to a REAL screenshot.
#    Icons must match the real render in SIZE + COLOR + sharpness.
#    If not: tune scale-min/scale-max and TEAM_COLORS in
#    crates/faf-ml-datagen/src/lib.rs, regenerate.

# 4. TRAIN on the platform store (release mode — debug conv is painfully slow):
cargo run -p faf-ml-train --release -- train --data data/faf-ml --epochs 50
# (the loader auto-detects the store layout: synthetic screenshots + labels/*.json)
# checkpoints land in data/faf-ml/runs/<timestamp>/ (model.mpk + config.json)

# 5. THE MOMENT OF TRUTH — predict on a HELD-OUT battle screenshot:
cargo run -p faf-ml-train --release -- predict \
  --model data/faf-ml/runs/<latest> --image <battle-shot.png> --out /tmp/pred.png
# open /tmp/pred.png: did it find the units?
```

### Reading the outcome of step 5

- **Works on real shots** → the synthetic approach is validated. Next: phase 2
  (training from the web UI) or the analysis view (detections → per-player
  table via `icon-map.json` + dominant-box-color for player attribution).
- **Works on synthetic, misses real** → domain gap. Iterate on datagen
  (colors/scale/AA), NOT on labeling by hand. Only if that stalls: mark some
  battle shots in the UI, let the model pre-label, correct, snapshot, mix into
  training.
- **Loss won't even go down on synthetic** → check anchor coverage /
  hyperparams; batch size is capped at 4 (see gotchas).

## Key decisions already made (don't re-litigate without reason)

1. **Classes = strategic icon names** (193 of them: units + markers +
   generics). Fine-grained on purpose — the analysis view aggregates via
   `faf-blueprints`. Markers/generics stay as classes so the model doesn't
   confuse them with units.
2. **Labeling UI is view+edit only** (no draw-from-scratch) — correction
   workflow, not a CVAT clone.
3. **Player attribution = dominant pixel color inside a detected box** — NOT
   an ML problem. But it means datagen's `TEAM_COLORS` must match real FAF
   player colors (currently approximate — calibrate from real screenshots).
4. **Learning workspace stays pure learning**; production model lives in
   `crates/faf-ml-model` (ch14 SSD concepts applied natively).
5. Icon-set orphans (`commander_*`, `experimental_*`, finer naval classes):
   the custom icon mod is finer than vanilla DB metadata; a curated alias
   table is a future task for the analysis view. 5 Nomads (`XNL*`) unit icons
   have no sprites — ignore unless you play with Nomads.

## Gotchas (hard-won, don't rediscover)

- **wgpu per-buffer cap ~128 MiB**: the detector's stem conv is stride-2 and
  default `--batch` is 4 because of this. Batch ≥6 panics with
  "can't allocate buffer". Fix = gradient accumulation (not implemented).
- **Train with `--release`.**
- Burn: `AdamConfig::with_grad_clipping` vs `SgdConfig::with_gradient_clipping`
  (inconsistent naming); BatchNorm/Dropout pick train/eval from
  `B::ad_enabled`, so eval needs `model.valid()` (the detector has no BN on
  purpose).
- Server on :3100; check for stale processes with `pgrep -f faf-ml-server`
  before assuming a new build is running ("Address already in use" bites).
- dx web builds go to `target/dx/faf-ml-web/{debug,release}/web/public`;
  the server serves the **release** build (`cargo xtask faf-ml build-web`).

## Reference material

- d2l.ai chapters that matter for this project: ch7 (convs), ch8 (esp. NiN's
  GAP, ResNet), ch14 (augmentation, anchors, SSD, NMS). Ports live in
  `~/code/rust_programming/dive_into_deep_learning_in_rust` with mdbook notes
  (`cargo xtask book` there).
- The icon↔blueprint reasoning: re-run
  `cargo run -p faf-unit-tools -- icon-map --out data/faf-ml/icon-map.json`.
- Platform READMEs: `apps/faf-ml-server/README.md`, `apps/faf-ml-web/README.md`.

## Phase roadmap (for orientation)

- **Phase 0 ✅** — platform: upload/triage/label/snapshot
- **Phase 1 ✅ (mostly)** — datagen is a server job driven from the web
  Datagen view; the `faf-datagen` CLI and the `/api/import/datagen` endpoint
  are gone (generation is internal, labels stream straight into the store).
  `faf-ml-train --data data/faf-ml` reads the platform store directly.
  Remaining: dataset compose view
- **Next real milestone** — detector trained on synthetic data detecting
  units on a held-out real screenshot (steps 1–5 above)
- **Phase 2** — training jobs + live metrics in the web UI
- **Phase 3** — eval/analysis view (per-player unit tables), correction loop,
  then the Windows capture client (eframe if GUI needed)
