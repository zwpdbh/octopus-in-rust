# Task: faf-gamedata-sync-service

## Goal

Add a gamedata file distribution service to the fafcn stack (manifest + hash-verified sync client + token-gated upload) so Chinese FAF players can sync the hard-to-download `gamedata` files (< 700 MB total) from a community mirror instead of passing files through QQ.

## Background

Chinese FAF players often cannot download certain files in the FAF `gamedata` folder from the official servers. Today's workaround: a player with a VPN downloads the files, uploads them to a QQ group, and every other player manually figures out which files they need and downloads them from QQ. Two pain points:

1. Someone must download-then-re-upload every file through QQ.
2. Everyone else must manually identify which files they need.

This service replaces QQ with a deployed mirror: a VPN-having uploader pushes the files once to `fafcn-server`; every other player runs a small sync client that diffs their local `gamedata` folder against a server manifest and downloads exactly what's missing or changed — one command, no per-file decisions.

## Scope

### In Scope

- **Server (`apps/fafcn-server`)**: channel-aware `/api/gamedata/*` routes (channels: `gamedata`, `map-generator`):
  - `GET /api/gamedata/channels/<channel>/manifest.json` — anonymous read of a channel manifest.
  - `GET /api/gamedata/channels/<channel>/files/<path>` — static file download with HTTP range support.
  - `GET /api/gamedata/client/<file>` — sync client download; the server patches the binary per request with an embedded config block (`fafcn-gamedata::overlay`: JSON + length + magic appended as PE/ELF overlay data) containing the mirror's own origin (`X-Forwarded-Proto` + `Host`), so the player's client starts with 镜像地址 pre-filled — no manual setup.
  - `POST /api/gamedata/channels/<channel>/upload/check` — token-authed; uploader submits the full `{path, size, sha256}` list, server replies which files it still needs (dedup / cheap resume).
  - `POST /api/gamedata/channels/<channel>/upload/file` — token-authed; raw body + `x-gamedata-path` / `x-gamedata-sha256` headers; server hash-verifies before storing.
  - `POST /api/gamedata/channels/<channel>/upload/commit` — token-authed; server verifies every listed file is present with matching hash, rejects strictly-older versions (409), then atomically regenerates the channel manifest.
  - `GET /api/gamedata/status` — per-channel version/uploader/file count/size/last-updated + client build tag.
- **Channel definitions** (shared in `crates/fafcn-gamedata/src/channels.rs`):
  - `gamedata` — only `env.nx2`, `units.nx2`, `textures.nx2` (the files players actually struggle to download), version from `lua.nx2`.
  - `map-generator` — newest 3 `MapGenerator_*.jar` (semver sort), version = newest jar.
  - `faf-client` — mirror-only (NOT synced into FAForever): the downlords-faf-client installer from GitHub releases, uploaded as a single file by an uploader; players download it via a link on the /sync page. Version auto-detected from the installer filename (`dfc_windows_1_6_3.exe` → `1.6.3`).
- **Storage layout** (filesystem, under a configurable `FAFCN_GAMEDATA_DIR`, default `data/faf-gamedata/`):
  ```
  data/faf-gamedata/
    channels/<channel>/
      manifest.json          # generated, not hand-edited
      files/<relative path>  # content as served to clients
      incoming/              # temp dir for in-progress uploads (atomic rename on complete)
    client/                  # sync client binaries + VERSION build tag
  ```
- **Manifest format** (single JSON, regenerated on every accepted upload):
  ```json
  {
    "patch_version": "3825",
    "generated_at": "2026-08-18T00:00:00Z",
    "files": [
      { "path": "example.scd", "size": 12345678, "sha256": "<hex>" }
    ]
  }
  ```
- **Sync client (`apps/fafcn-sync`, ships as a single downloadable `.exe`)**:
  - **GUI (default, double-click)**: eframe-based, dark theme, for non-technical players — mirror address field, **FAForever root folder** field with native folder picker + auto-detect (`FAForever` dir containing `gamedata\*.nx2`), one big sync button syncing ALL channels, progress bar + localized log, Chinese/English toggle (Chinese default). Upload tab for VPN-having uploaders (token + player name; both channel versions auto-detected read-only; upload disabled with explanation when the server has a newer version). Windows release builds are GUI-subsystem (no console window).
  - **CLI (`fafcn-sync sync` / `upload`)**: same engines for terminal/automation use; on Windows release it re-attaches to the parent console.
  - Hash-diff against each channel manifest → download missing/mismatched files to a temp dir → verify sha256 → atomic rename into the channel subfolder.
  - gamedata: never delete local files not in the manifest (report them only). map-generator: prune local `MapGenerator_*.jar` beyond the newest 3 versions.
  - Remember server + FAForever dir + language + token/player name in a small config file; server defaults to the value embedded in the downloaded binary (remembered config wins over embedded).
  - Display the server manifest's version and last-updated so the user can judge freshness themselves; always tell the user how to fall back to the official channel.
- **Web page (`apps/fafcn-web`)**: one new `/sync` page — client download link + server status (patch version, last updated, file count/size, staleness indicator).
- **Upload helper**: an 上传 tab in the same GUI client (token + folder picker + patch version + name + progress), plus the `fafcn-sync upload` CLI subcommand — one exe for both roles.

### Out of Scope

- ~~Maps/mods vault mirroring~~ **Done (2026-08-20):** the `maps` channel mirrors FAF maps. Uploads merge into the manifest (newer `name.vNNNN` versions replace older ones); sync downloads them into the FAF Client's `maps_and_mods/maps` and prunes stale local map versions.
- Mods vault mirroring (later task if wanted).
- Delta/binary-diff sync, P2P distribution, chunked resumable upload (unnecessary at < 700 MB scale; range-supported *downloads* are included).
- Object storage (OSS/COS) offload (revisit only if bandwidth becomes a cost problem).
- GUI client via Dioxus desktop (CLI first; GUI is a later enhancement).
- Per-user accounts/RBAC (single shared upload token is enough for a friend group).
- Automatic mirroring of FAF's own patch-server protocol so the official client could use us directly (investigate separately).

## Acceptance Criteria

- [x] `GET /api/gamedata/manifest.json` returns a valid manifest; regenerates after every upload.
- [x] `GET /api/gamedata/files/<path>` serves files with range support and correct sizes.
- [x] Upload without a valid token is rejected (401); upload with token stores files under `files/` via atomic rename and updates the manifest.
- [x] `fafcn-sync sync` on a fresh machine downloads all manifest files, verifies hashes, and places them in the target `gamedata` dir.
- [x] `fafcn-sync sync` on an up-to-date machine downloads nothing (hash diff is empty).
- [x] A corrupted local file is detected by hash and re-downloaded; a failed download never leaves a partial file in `gamedata`.
- [x] `/sync` page shows current status and a client download link.
- [x] Tests added or updated (manifest generation, hash diff logic, upload auth).
- [x] `cargo check --workspace` passes.
- [x] `cargo test --workspace` passes.

## Implementation Notes

- Follow root `AGENTS.md`: typed enums for sync outcomes (e.g. `enum FileSyncAction { Download, Skip, Remove? }` — decide; never delete by default), typed serde structs for the manifest (shared between server and client — consider putting manifest types in a small shared crate or in `crates/` so both sides use one definition).
- Server: reuse existing Axum setup; `tower-http` `ServeDir` (or manual `Range` handling) for downloads; token check via a small middleware reading `FAFCN_GAMEDATA_UPLOAD_TOKEN` from `.env` (add to `.env.example`).
- Upload flow: client sends `HEAD`-like pre-check (`POST /api/gamedata/upload/check` with list of `{path, sha256, size}`), server replies which files it needs; client uploads only those. Upload to `incoming/`, verify sha256 server-side, then rename into `files/` and regenerate the manifest in one step (write `manifest.json.tmp` + rename).
- Client defaults: Windows-first target (most players), but keep it cross-platform; distribute the built `.exe` through the server itself as a static download.
- Freshness is human-judged, not automated: the server has no access to official patch data (the needed files are not reliably available from FAF's open-source GitHub repos), so user upload is the *only* source. The uploader declares `patch_version` at upload time; status surfaces `patch_version` + `last-updated` prominently so the group can see at a glance that someone needs to upload a newer patch.
- **Future investigation (marked for later):** the official FAF client *does* detect when new patch data is available — e.g. the client starts normally, but when the user creates a game it begins downloading the latest patch data if a new balance patch was released. So there must be a machine-readable version/update check somewhere in the client. Investigate the official client source at `/home/zw/code/faf_related/official_faf_stack/downlords-faf-client` to find: (a) which endpoint it queries for the current patch/mod version, (b) whether that check is reachable without downloading the files themselves. If found, add an automated staleness flag (server periodically queries the version endpoint and compares with the uploaded manifest's `patch_version`) — no manifest format change needed.
- Keep the distribution private to the group (no public listing/indexing) since these are redistributed game files.
- Bandwidth estimate: ~700 MB per full sync; tens of GB/month for a small group — fine on the existing VPS; monitor before considering OSS.

## Completed Steps

- [x] Shared manifest types crate/module (`crates/fafcn-gamedata`).
- [x] Server manifest/files/upload/status endpoints + storage layout (`apps/fafcn-server/src/handlers/gamedata/`).
- [x] `fafcn-sync` client (`apps/fafcn-sync`): GUI (eframe, double-click) + CLI (`sync` + `upload` subcommands).
- [x] `/sync` page in `fafcn-web`.
- [x] `.env.example` / config docs updated.
- [x] End-to-end smoke test (local): token auth 401, upload + commit, sync to empty dir byte-identical, no-op re-sync, corrupt file re-downloaded, extra files untouched, dedup re-upload, HTTP 206 range downloads, downgrade rejection (409), build tag in exe + status, channel E2E (gamedata filtered to 3 files, map-generator newest-3 jars, jar pruning), faf-client installer E2E (auto version detect, byte-identical download, 409 on older).
- [ ] End-to-end test on a real FAF install (with a real downlords-faf-client installer).
- [x] Windows release build of `fafcn-sync` published under `/api/gamedata/client/` (via `cargo xtask fafcn file-sync`, cross-compiles to `x86_64-pc-windows-gnu`; use `--release` for distribution).

## Decisions Made

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-08-18 | Scope limited to `gamedata` folder files (< 700 MB), no maps/mods | Matches the actual pain point; keeps v1 weekend-scale. |
| 2026-08-18 | Manifest + sha256 diff sync, whole-file downloads only | At this size delta sync/P2P/chunking are unnecessary complexity; the manifest-diff is what eliminates the "which file do I need" problem. |
| 2026-08-18 | Single shared upload token instead of user accounts | Friend-group scale; uploaders are trusted because they effectively patch everyone's game. |
| 2026-08-18 | CLI client first, shipped as single `.exe` | Best effort/UX ratio; Dioxus desktop GUI deferred. |
| 2026-08-18 | GUI (eframe) is the default client UX; CLI kept as subcommands | Most FAF players are non-technical: double-click → auto-detected folder → one sync button. Dioxus desktop was rejected because wry/WebView2 cannot cross-compile from Linux; eframe (winit+glow) cross-compiles to `x86_64-pc-windows-gnu` cleanly. Windows release uses the GUI subsystem (no console window); CLI mode re-attaches to the parent console. |
| 2026-08-18 | Mirror address embedded into the client binary per download (PE/ELF overlay) | Non-technical players must never type a URL. Alternatives rejected: zip-with-config (extraction is error-prone for the audience), custom protocol handler (requires registry writes). Appended overlay data is ignored by both loaders; remembered config takes precedence so power users can still switch mirrors. |
| 2026-08-18 | Upload lives in the client (GUI tab + CLI), not as web drag-drop | Uploads are rare and done by the technical, VPN-having player; browser folder-upload in WASM (traversal, 700MB hashing, no fetch upload progress) is high-complexity for the wrong path. The /sync page shows uploader instructions instead. |
| 2026-08-18 | Patch version auto-detected from `lua.nx2` (`lua/version.lua` — it's a ZIP); server rejects strictly older uploads (409) | The version is ground truth from the game data, so users never type it (manual entry is fallback only). The GUI also pre-checks the server manifest and disables upload with an explanation when the server is newer; the commit-time server guard is the authoritative enforcement. |
| 2026-08-18 | Two sync channels rooted at the FAForever folder: gamedata filtered to env/units/textures.nx2; map-generator keeps newest 3 jars (semver sort, pruned locally beyond that) | Player feedback: these are the only files they actually struggle to download. Version compare generalized to dotted-numeric (`1.22.10` > `1.22.1`). gamedata still never deletes extras; jar pruning is scoped strictly to the `MapGenerator_*.jar` pattern. |
| 2026-08-18 | FAF client installer is a mirror-only channel, not part of folder sync | Most players don't need it, and an installer is downloaded-and-run, not synced into FAForever. Reuses the full channel machinery (dedup, downgrade guard); upload via GUI installer section or `fafcn-sync upload-client`; download via a plain link on the /sync page. |
| 2026-08-18 | Never delete local files not in manifest; download-to-temp + atomic rename | Client must never break a working game install. |
| 2026-08-18 | User upload is the only source of gamedata; no server-side fetching from official channels | The required patch files are not reliably downloadable from FAF's open-source GitHub repos; automated staleness checks are deferred until we investigate how the official client detects new patches (see Implementation Notes; source at `/home/zw/code/faf_related/official_faf_stack/downlords-faf-client`). |
