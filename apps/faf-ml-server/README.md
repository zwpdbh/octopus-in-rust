# faf-ml-server

Backend for the **faf-ml** data platform — phase 0 of the FAF unit-detection
ML project: collect screenshots → review bounding boxes → freeze immutable
dataset snapshots. No training here; models arrive in later phases
(`crates/faf-ml-model`, ported from the d2l workspace).

Serves the `faf-ml-web` Dioxus build as static files (SPA fallback to
`index.html`) and exposes the JSON API below.

## Run

```sh
cargo run -p faf-ml-server
# env (defaults shown):
#   FAF_ML_PORT=3100
#   FAF_ML_DATA_DIR=data/faf-ml                                (gitignored)
#   FAF_ML_WEB_DIST=target/dx/faf-ml-web/release/web/public
```

Logs: stdout + `data/logs/faf-ml-server.log`.

## Endpoints

| Route | Purpose |
|---|---|
| `GET /api/health` | service status |
| `POST /api/screenshots` | multipart PNG upload (one or more `files` fields) → stores + returns `ScreenshotMeta[]` |
| `GET /api/screenshots` | list all `ScreenshotMeta` (from `index.json`) |
| `GET /api/screenshots/{id}/image` | serve the PNG |
| `GET /api/screenshots/{id}/labels` | box list (JSON, `[]` when unlabeled) |
| `PUT /api/screenshots/{id}/labels` | replace the box list |
| `DELETE /api/screenshots/{id}` | remove image + labels + index entry |
| `GET /api/classes` | class names from `classes.txt` |
| `POST /api/import/datagen` | `{dir}` → import a `faf-datagen` output dir: copies `images/*.png` in as new screenshots, converts `labels/*.txt` (YOLO, normalized) to absolute-pixel JSON, merges `classes.txt` |
| `GET /api/datasets` | list dataset manifests |
| `POST /api/datasets` | `{name, image_ids}` → immutable snapshot embedding the current labels (409 if the name exists) |

Shared wire types live in `crates/faf-ml-core`.

## Data layout

```
data/faf-ml/
  classes.txt              one class per line (line no. = class id)
  screenshots/<uuid>.png   uploaded images
  screenshots/index.json   [ScreenshotMeta]
  labels/<uuid>.json       [LabeledBox]  (absolute pixels)
  datasets/<name>.json     DatasetManifest (labels embedded → immutable)
```

## Explicitly NOT in phase 0

- datagen-as-a-job, training runs + WS metrics, eval view (phases 1–3)
- draw-new-box interactions (review/edit only), tagging/filtering, auth
- Windows capture client (later — eframe if a GUI is needed)

## Dev loop

Terminal 1: `cargo run -p faf-ml-server`
Terminal 2: `cd apps/faf-ml-web && dx serve` (hot-reload UI on its own port,
talks to `localhost:3100` in debug builds — see `src/net.rs`).

Quick smoke test:

```sh
curl localhost:3100/api/health
curl -X POST localhost:3100/api/screenshots -F "files=@/path/to/shot.png"
curl -X POST localhost:3100/api/import/datagen \
  -H 'Content-Type: application/json' -d '{"dir":"data/faf-detect"}'
```
