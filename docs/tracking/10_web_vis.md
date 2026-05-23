# 10 — Web / Visualizer

## Status: ⬜ Not Started

## Python Source Files

| File | Description | LOC |
|------|-------------|-----|
| `kimi_cli/web/app.py` | FastAPI web app | ~300 |
| `kimi_cli/web/api/*.py` | Web API endpoints | ~400 |
| `kimi_cli/web/auth.py` | Web auth | ~100 |
| `kimi_cli/web/models.py` | Web data models | ~100 |
| `kimi_cli/web/runner/*.py` | Web runner process | ~200 |
| `kimi_cli/web/store/*.py` | Session store for web | ~150 |
| `kimi_cli/vis/app.py` | Vis FastAPI app | ~200 |
| `kimi_cli/vis/api/*.py` | Vis API endpoints | ~300 |
| `kimi_cli/cli/web.py` | `kimi web` command | ~100 |
| `kimi_cli/cli/vis.py` | `kimi vis` command | ~100 |

## Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/web/mod.rs` | Web module placeholder | ~? | ⬜ Empty / stub |
| `octopus-cli/src/vis/mod.rs` | Vis module placeholder | ~? | ⬜ Empty / stub |

## What's Missing

- [ ] FastAPI / Axum web server
- [ ] WebSocket for real-time updates
- [ ] Session API for web UI
- [ ] Visualizer data API
- [ ] OAuth callback handler for web
- [ ] Static file serving for web/vis frontends
