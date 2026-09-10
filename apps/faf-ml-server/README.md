# faf-ml-server

Backend for the **faf-ml** data platform of the FAF unit-detection ML
project: collect screenshots → review bounding boxes → generate synthetic
training data → freeze immutable dataset snapshots. No training here; models
arrive in later phases (`crates/faf-ml-model`, ported from the d2l
workspace).

Serves the `faf-ml-web` Dioxus build as static files (SPA fallback to
`index.html`) and exposes the JSON API below.

## Run

```sh
cargo run -p faf-ml-server
# env (defaults shown):
#   FAF_ML_PORT=3100
#   FAF_ML_DATA_DIR=data/faf-ml                                (gitignored)
#   FAF_ML_WEB_DIST=target/dx/faf-ml-web/release/web/public
#   FAF_ML_ICONS_DIR=tmp/custom-strategic-icons                (datagen sprites)
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
| `DELETE /api/screenshots?kind={kind}` | bulk-remove every screenshot of one kind (image + labels + index entries); `kind` is mandatory. Clearing `synthetic` also drops finished datagen jobs from the registry |
| `GET /api/classes` | class names from `classes.txt` |
| `POST /api/datagen` | body = `DatagenConfig` → start a generation job: composites sprites onto `background`-kind screenshots (400 when none exist — triage in the Gallery first), streams each sample into the store as a `synthetic` screenshot + labels JSON, merges sprite class names into `classes.txt`. `exclude_classes` (default `[]`) skips icon classes; `classes.txt` still merges ALL sprite classes so class ids stay stable |
| `GET /api/datagen/sprites` | sorted class names of every sprite in the icons dir (the pool the web UI's icon picker excludes from) |
| `GET /api/datagen/sprites/{class}/image` | the sprite as PNG (the source DDS is not browser-displayable) |
| `GET /api/datagen/jobs` | all datagen jobs (newest first) |
| `GET /api/datagen/jobs/{id}` | one job (the web UI polls this while `running`) |
| `DELETE /api/datagen/jobs/{id}` | remove a finished job AND its generated sample set (400 while `running`); samples predate job tracking → use bulk delete instead |
| `GET /api/datasets` | list dataset manifests |
| `POST /api/datasets` | `{name, image_ids}` → immutable snapshot embedding the current labels (409 if the name exists) |

Generation logic lives in `crates/faf-ml-datagen` (the former `faf-datagen`
CLI, now a library); shared wire types (`DatagenConfig`, `DatagenJob`,
`DatagenStatus`, …) live in `crates/faf-ml-core`.

## Data layout

```
data/faf-ml/
  classes.txt              one class per line (line no. = class id)
  screenshots/<uuid>.png   uploaded images
  screenshots/index.json   [ScreenshotMeta]
  labels/<uuid>.json       [LabeledBox]  (absolute pixels)
  datasets/<name>.json     DatasetManifest (labels embedded → immutable)
```

## Explicitly NOT built yet

- training runs + WS metrics, eval view (phases 2–3)
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
# after marking a shot as background:
curl -X POST localhost:3100/api/datagen \
  -H 'Content-Type: application/json' -d '{"count":10}'
curl localhost:3100/api/datagen/jobs
```
