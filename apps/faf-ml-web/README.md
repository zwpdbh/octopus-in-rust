# faf-ml-web

Dioxus 0.7 SPA for the **faf-ml** data platform (collect → review → generate
→ snapshot). Mirrors the `fafcn-web` structure: `main.rs` defines the routes,
`net.rs` resolves the API base URL (`http://localhost:3100` in debug builds,
same-origin in release), views live in `src/views/`.

## Views

| Route | View | Purpose |
|---|---|---|
| `/` | `Home` | what this is + quick links |
| `/gallery` | `Gallery` | thumbnail grid, multi-file PNG upload (gloo-net + `web_sys::FormData` multipart POST), delete |
| `/label/:id` | `Label` | image with SVG-rect overlay of the existing boxes (scaled via `viewBox` = natural size); click a box to select, re-assign its class (dropdown from `GET /api/classes`), delete it, save (`PUT` labels). Review-only: no drawing new boxes. |
| `/datagen` | `Datagen` | generation form (count/size/max-units/scale/seed → `POST /api/datagen`; the 400 "mark backgrounds first" error is surfaced) + jobs table polling `GET /api/datagen/jobs` every 2 s while any job is running; Done rows link to the Gallery's synthetic filter |
| `/datasets` | `Datasets` | list immutable snapshots (name, #images, #boxes, date); create one from all current screenshots |

## Dev loop

```sh
# terminal 1 — API + (release) static hosting on :3100
cargo run -p faf-ml-server

# terminal 2 — hot-reload dev server (its own port, proxies nothing;
# the app calls localhost:3100 directly in debug builds)
cd apps/faf-ml-web && dx serve
```

## Build

```sh
cd apps/faf-ml-web && dx build --web --release
# output: target/dx/faf-ml-web/release/web/public
# (= the server's FAF_ML_WEB_DIST default; serve via faf-ml-server)
```

Tailwind v4 is compiled automatically by `dx` from `tailwind.css` at the app
root into `assets/tailwind.css` — no npm step.

## Explicitly NOT built yet

- training/eval UI, live metrics (phases 2–3)
- draw-new-box interactions, tagging/filtering, auth
