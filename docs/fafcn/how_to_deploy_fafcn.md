# How to Deploy the fafcn Web Stack (LLM-Guided Runbook)

This document is written to be **executed by an LLM agent** (Kimi Code CLI,
Claude Code, Cursor agent, etc.). It contains everything the agent needs:
context, decision rules, exact commands, expected outputs, and failure
actions.

## How to use this document

1. Fill in the **Variables** block below.
2. Paste this entire document into an LLM agent that has shell access to
   your local machine (and SSH access to the server), with a prompt like:

   > Follow the runbook below step by step. Execute each step, check its
   > expected output, and stop at each verification gate. Ask me before
   > making any choice not covered by the decision rules.

3. The agent executes the runbook; you answer questions and approve commands.

## Variables (fill these in first)

```bash
SERVER_SSH="ubuntu@<server-public-ip>"   # SSH login for the Ubuntu server
DOMAIN="<your-domain.example.com>"       # domain pointing at the server (REQUIRED for HTTPS; use "-" if none yet)
REPO_URL="<git-repo-clone-url>"          # e.g. your GitHub/Gitee mirror of octopus
LLM_API_KEY="<sk-...>"                   # OpenAI-compatible key for the Q&A page
UPLOAD_TOKEN="<run: openssl rand -hex 24>" # invent once, keep secret, share only with uploaders
```

Rules the agent must follow when variables are missing:

- If `DOMAIN` is `-`: deploy plain HTTP on port 80 and mark uploads as
  "dev-only" (warn the user that the upload token would cross the internet
  unencrypted). Do NOT run certbot.
- If the server has **< 4 GB RAM** (`free -h`): use **Option B** (build
  locally, ship artifacts). Otherwise prefer **Option A** (build on server).
  Check RAM first with `ssh $SERVER_SSH 'free -h'`.

## Architecture (context for the agent)

One binary, `fafcn-server`, serves everything: the fafcn-web SPA, the
JSON/WebSocket APIs, and the gamedata mirror (channels `gamedata`,
`map-generator`, `faf-client`). nginx terminates TLS and proxies to
`127.0.0.1:3000`. Two nginx details are REQUIRED:

- WebSocket upgrade headers (the `/ws/simulate` endpoint).
- `Host` + `X-Forwarded-Proto` headers — the server embeds
  `<scheme>://<host>` into every sync-client exe it serves; if these are
  wrong, every player's client gets a broken mirror address.

Runtime paths (all env-overridable; compile-time defaults are repo-relative,
so Option B must override every one of them):

| Env | Default (repo-relative) |
|---|---|
| `FAFCN_WEB_DIST` | `target/dx/fafcn-web/release/web/public` |
| `FAFCN_PORTRAITS_DIR` | `assets/icons/units` |
| `FAFCN_UNITS_FILE` | `plugins/faf-units/data/faf_units.json` (compile-time!) |
| `FAFCN_PLUGINS_DIR` | `data/qqbot-data/plugins` |
| `FAFCN_GAMEDATA_DIR` | `data/faf-gamedata` |
| `FAFCN_GAMEDATA_CLIENT_DIR` | `<gamedata>/client` |

## Phase 1 — Provision the server

```bash
ssh $SERVER_SSH 'sudo apt update && sudo apt install -y build-essential pkg-config libssl-dev curl git nginx ufw'
```

Expected: packages install without errors.

## Phase 2 — Build (choose by the RAM rule)

### Option A — build on the server (≥ 4 GB RAM)

```bash
ssh $SERVER_SSH
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env
rustup target add wasm32-unknown-unknown
curl -sSL https://dioxus.dev/install.sh | bash   # prebuilt dx; do NOT cargo install (OOM risk)

git clone $REPO_URL octopus && cd octopus
cargo build --release -p fafcn-server
cargo build --release -p faf-units-plugin --target wasm32-unknown-unknown
mkdir -p data/qqbot-data/plugins
cp target/wasm32-unknown-unknown/release/faf_units_plugin.wasm data/qqbot-data/plugins/
(cd apps/fafcn-web && dx build --release --platform web)
```

Expected: `target/release/fafcn-server` exists; the dx build prints
"Client build completed". Install dir for later phases:
`/home/ubuntu/octopus`. **The repo must stay in place** (compile-time
repo-relative defaults).

### Option B — build locally, ship artifacts (< 4 GB RAM)

Run on YOUR machine (not the server):

```bash
git clone $REPO_URL octopus 2>/dev/null || true; cd octopus
cargo build --release -p fafcn-server
cargo build --release -p faf-units-plugin --target wasm32-unknown-unknown
(cd apps/fafcn-web && dx build --release --platform web)
cargo xtask fafcn file-sync   # cross-compiles the Windows sync client for players

ssh $SERVER_SSH 'sudo mkdir -p /opt/fafcn && sudo chown $USER:$USER /opt/fafcn'
scp target/release/fafcn-server $SERVER_SSH:/opt/fafcn/
scp target/wasm32-unknown-unknown/release/faf_units_plugin.wasm $SERVER_SSH:/opt/fafcn/
rsync -az target/dx/fafcn-web/release/web/public/ $SERVER_SSH:/opt/fafcn/web/
rsync -az assets/icons/units/ $SERVER_SSH:/opt/fafcn/portraits/
scp plugins/faf-units/data/faf_units.json $SERVER_SSH:/opt/fafcn/
rsync -az data/faf-gamedata/ $SERVER_SSH:/opt/fafcn/gamedata/
```

Expected: all copies succeed. Install dir: `/opt/fafcn`.

## Phase 3 — Configure `.env`

Option A: `apps/fafcn-server/.env` inside the repo (only the two REQUIRED
blocks; paths default correctly).
Option B: `/opt/fafcn/.env` (REQUIRED blocks PLUS all path overrides):

```bash
# REQUIRED — Q&A LLM
FAFCN_LLM_API_KEY=${LLM_API_KEY}
# REQUIRED — mirror upload token
FAFCN_GAMEDATA_UPLOAD_TOKEN=${UPLOAD_TOKEN}

# Option B only — path overrides:
FAFCN_WEB_DIST=/opt/fafcn/web
FAFCN_PORTRAITS_DIR=/opt/fafcn/portraits
FAFCN_UNITS_FILE=/opt/fafcn/faf_units.json
FAFCN_PLUGINS_DIR=/opt/fafcn
FAFCN_GAMEDATA_DIR=/opt/fafcn/gamedata

FAFCN_PORT=3000
```

## Phase 4 — systemd

Write `/etc/systemd/system/fafcn.service` (adjust paths per option):

```ini
[Unit]
Description=fafcn server
After=network.target

[Service]
Type=simple
User=ubuntu
WorkingDirectory=<INSTALL_DIR>
EnvironmentFile=<ENV_FILE_PATH>
ExecStart=<INSTALL_DIR>/fafcn-server        # Option A: <INSTALL_DIR>/target/release/fafcn-server
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload && sudo systemctl enable --now fafcn
systemctl status fafcn --no-pager
```

**Verification gate 1:** service is `active (running)` and
`curl -s http://127.0.0.1:3000/api/health/qa` returns `pong` on the server.
If it fails, read `journalctl -u fafcn -n 50 --no-pager` and fix the config
(most common: wrong paths in `.env`, missing LLM key).

## Phase 5 — nginx (+ TLS when DOMAIN is set)

Write `/etc/nginx/sites-available/fafcn`:

```nginx
server {
    listen 80;
    server_name ${DOMAIN:-_};

    location / {
        proxy_pass http://127.0.0.1:3000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_max_temp_file_size 0;
    }
}
```

```bash
sudo ln -sf /etc/nginx/sites-available/fafcn /etc/nginx/sites-enabled/fafcn
sudo nginx -t && sudo systemctl reload nginx
sudo ufw allow 'Nginx Full' && sudo ufw --force enable
# Only when DOMAIN is set:
sudo apt install -y certbot python3-certbot-nginx
sudo certbot --nginx -d $DOMAIN
```

**Verification gate 2:** from YOUR machine,
`curl -s https://$DOMAIN/api/gamedata/status` (or
`http://<server-ip>/api/gamedata/status` without a domain) returns JSON with
`channels` and `client_tag`. Open the home page in a browser — it renders
with the hero image.

## Phase 6 — Populate the mirror (uploader, needs VPN machine with FAF)

Do this from a machine that has the FAF install (usually your dev Windows
PC, using the GUI client downloaded from the site, or the CLI):

```bash
# 1. Sync client for players — Option B already shipped data/faf-gamedata/client/.
#    Option A: run on the server: cargo xtask fafcn file-sync

# 2. gamedata patch + map generator (auto-detects versions)
fafcn-sync upload --server https://$DOMAIN --token $UPLOAD_TOKEN --dir "C:\ProgramData\FAForever"

# 3. FAF client installer (optional; download it from GitHub via VPN first)
fafcn-sync upload-client --server https://$DOMAIN --token $UPLOAD_TOKEN --file dfc_windows_1_6_3.exe
```

Expected: each upload prints `Published <channel> <version>`.

**Verification gate 3 (acceptance):**

- `/api/gamedata/status` shows `gamedata` and `map-generator` (and
  `faf-client` if uploaded) with versions and your uploader name.
- The /sync page shows the same, plus a client build tag.
- Downloading the sync client from /sync and running it shows 镜像地址
  pre-filled with `https://$DOMAIN` and the FAForever folder auto-detected;
  开始同步 completes with "已是最新" or downloads the files.
- The /onboarding page step 1 shows the mirror download button.

If all gates pass, deployment is complete. Share the site URL and the QQ
group number (136430130) with the players; give `$UPLOAD_TOKEN` only to the
VPN-having uploaders.

## Ops notes (for the agent's future sessions)

- Logs: `journalctl -u fafcn -f` and `<INSTALL_DIR>/data/logs/fafcn-server.log`.
- Update: rebuild, replace the binary, `sudo systemctl restart fafcn`.
  Mirror content lives in `FAFCN_GAMEDATA_DIR` and survives restarts.
- One-command AliCloud provisioning pattern (if migrating to ECS later):
  `tasks/qqbot-aliyun-ecs-deployment.md`.
