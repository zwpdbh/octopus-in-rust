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

- **Server (`apps/fafcn-server`)**: new `/api/gamedata/*` route group:
  - `GET /api/gamedata/manifest.json` — anonymous read of the current manifest.
  - `GET /api/gamedata/files/<path>` — static file download with HTTP range support.
  - `POST /api/gamedata/upload` — token-authenticated upload (Bearer token from env config); client submits sha256 + size first so the server can skip files it already has.
  - `GET /api/gamedata/status` — patch version (as declared by the uploader), file count, total size, last-updated, uploader name.
- **Storage layout** (filesystem, under a configurable `FAFCN_GAMEDATA_DIR`, default `data/faf-gamedata/`):
  ```
  data/faf-gamedata/
    manifest.json          # generated, not hand-edited
    files/<relative path>  # content as served to clients
    incoming/              # temp dir for in-progress uploads (atomic rename on complete)
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
- **Sync client (`apps/fafcn-sync`, new small CLI binary, ships as a single downloadable `.exe`)**:
  - Locate the FAF `gamedata` dir (auto-detect common install paths, `--dir` override, remember in a small config file).
  - Fetch manifest → hash local files → download missing/mismatched files to a temp dir → verify sha256 → atomic rename into `gamedata`.
  - Never write in place; never delete local files not in the manifest (report them only).
  - Display the server manifest's `patch_version` and `last-updated` so the user can judge freshness themselves; always tell the user how to fall back to the official channel.
  - Exit code + human-readable summary of what changed.
- **Web page (`apps/fafcn-web`)**: one new `/sync` page — client download link + server status (patch version, last updated, file count/size, staleness indicator).
- **Upload helper**: a simple `fafcn-sync upload --token ... --dir ...` subcommand in the same client binary (no separate tool for uploaders).

### Out of Scope

- Maps/mods vault mirroring (later task if wanted).
- Delta/binary-diff sync, P2P distribution, chunked resumable upload (unnecessary at < 700 MB scale; range-supported *downloads* are included).
- Object storage (OSS/COS) offload (revisit only if bandwidth becomes a cost problem).
- GUI client via Dioxus desktop (CLI first; GUI is a later enhancement).
- Per-user accounts/RBAC (single shared upload token is enough for a friend group).
- Automatic mirroring of FAF's own patch-server protocol so the official client could use us directly (investigate separately).

## Acceptance Criteria

- [ ] `GET /api/gamedata/manifest.json` returns a valid manifest; regenerates after every upload.
- [ ] `GET /api/gamedata/files/<path>` serves files with range support and correct sizes.
- [ ] Upload without a valid token is rejected (401); upload with token stores files under `files/` via atomic rename and updates the manifest.
- [ ] `fafcn-sync sync` on a fresh machine downloads all manifest files, verifies hashes, and places them in the target `gamedata` dir.
- [ ] `fafcn-sync sync` on an up-to-date machine downloads nothing (hash diff is empty).
- [ ] A corrupted local file is detected by hash and re-downloaded; a failed download never leaves a partial file in `gamedata`.
- [ ] `/sync` page shows current status and a client download link.
- [ ] Tests added or updated (manifest generation, hash diff logic, upload auth).
- [ ] `cargo check --workspace` passes.
- [ ] `cargo test --workspace` passes.

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

- [ ] Shared manifest types crate/module.
- [ ] Server manifest/files/upload/status endpoints + storage layout.
- [ ] `fafcn-sync` CLI client (`sync` + `upload` subcommands).
- [ ] `/sync` page in `fafcn-web`.
- [ ] `.env.example` / config docs updated.
- [ ] End-to-end test on a real FAF install.

## Decisions Made

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-08-18 | Scope limited to `gamedata` folder files (< 700 MB), no maps/mods | Matches the actual pain point; keeps v1 weekend-scale. |
| 2026-08-18 | Manifest + sha256 diff sync, whole-file downloads only | At this size delta sync/P2P/chunking are unnecessary complexity; the manifest-diff is what eliminates the "which file do I need" problem. |
| 2026-08-18 | Single shared upload token instead of user accounts | Friend-group scale; uploaders are trusted because they effectively patch everyone's game. |
| 2026-08-18 | CLI client first, shipped as single `.exe` | Best effort/UX ratio; Dioxus desktop GUI deferred. |
| 2026-08-18 | Never delete local files not in manifest; download-to-temp + atomic rename | Client must never break a working game install. |
| 2026-08-18 | User upload is the only source of gamedata; no server-side fetching from official channels | The required patch files are not reliably downloadable from FAF's open-source GitHub repos; automated staleness checks are deferred until we investigate how the official client detects new patches (see Implementation Notes; source at `/home/zw/code/faf_related/official_faf_stack/downlords-faf-client`). |
