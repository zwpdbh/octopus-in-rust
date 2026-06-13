# faf-qq-bot Local Manual Test Checklist

Use this checklist to verify `faf-qq-bot` works end-to-end with NapCatQQ on your local machine. Fill in the **Actual Result** column and any notes, then share the updated file for review.

---

## 1. Prerequisites

| # | Step | Expected Result | Actual Result |
|---|------|-----------------|---------------|
| 1.1 | Docker and Docker Compose are installed (`docker compose version` works). | Command prints version. | |
| 1.2 | You have a spare QQ account for the bot (to reduce ban risk). | Account ready. | |
| 1.3 | You have a Kimi (Moonshot AI) API key from https://platform.moonshot.cn/. | Key copied. | |
| 1.4 | You have a test QQ group where the bot account is a member. | Group ID known: `__________`. | |

---

## 2. Build the Bot Image

| # | Step | Expected Result | Actual Result |
|---|------|-----------------|---------------|
| 2.1 | Run `cd /home/zw/code/rust_programming/octopus`. | Changed to project root. | |
| 2.2 | Run `docker build -f faf-qq-bot/Dockerfile -t faf-qq-bot:latest .`. | Image builds successfully with no errors. | |
| 2.3 | Run `docker run --rm faf-qq-bot:latest --help`. | CLI help is printed. | |

---

## 3. Prepare Directories and Config

| # | Step | Expected Result | Actual Result |
|---|------|-----------------|---------------|
| 3.1 | Run `mkdir -p napcat/app/.config/QQ napcat/app/napcat/config`. | Directories created. | |
| 3.2 | Copy `faf-qq-bot/config.example.toml` to `faf-qq-bot/config.toml`. | Config file created. | |
| 3.3 | Edit `faf-qq-bot/config.toml`: set `onebot.ws_url = "ws://napcat:3001"`. | Config saved. | |
| 3.4 | Set `bot.allowed_groups = [YOUR_GROUP_ID]`. | Group ID saved. | |
| 3.5 | Set `llm.api_key = "YOUR_KIMI_KEY"` and `llm.model = "moonshot-v1-8k"`. | API key and model saved. | |

---

## 4. Start NapCatQQ Container Only

| # | Step | Expected Result | Actual Result |
|---|------|-----------------|---------------|
| 4.1 | Run `docker compose up napcat -d`. | Container starts without errors. | |
| 4.2 | Run `docker compose logs -f napcat` and look for a WebUI URL + token. | WebUI URL and token appear in logs. | |
| 4.3 | Open `http://localhost:6099/webui` and enter the token from `napcat/app/napcat/config/webui.json`. | WebUI loads. | |
| 4.4 | In WebUI, log in the bot QQ account via QR code. | QQ account shows online. | |
| 4.5 | In WebUI → Network Config, add a **WebSocket Server** on `0.0.0.0:3001` and save. | WS server enabled and bound to 0.0.0.0. | |
| 4.6 | From host, run `curl -i http://localhost:3001` (or `nc -vz localhost 3001`). | Port 3001 is reachable. | |

---

## 5. Start the Bot Container

| # | Step | Expected Result | Actual Result |
|---|------|-----------------|---------------|
| 5.1 | Run `docker compose up faf-qq-bot -d`. | Container starts without errors. | |
| 5.2 | Run `docker compose logs -f faf-qq-bot`. | Log shows "connected to OneBot WebSocket" and "bot is running". | |
| 5.3 | In the test QQ group, send a normal text message. | Bot logs show the event (if logging is verbose enough). | |

---

## 6. Command Tests

Send each command in the test group and record the result.

| # | Command | Expected Result | Actual Result |
|---|---------|-----------------|---------------|
| 6.1 | `/help` | Bot replies with available commands. | |
| 6.2 | `/status` | Bot replies with buffered message count. | |
| 6.3 | Send 3–5 messages from different users, then `/summary`. | Bot replies with a concise summary of the recent conversation. | |
| 6.4 | `/s` (alias for `/summary`). | Same as 6.3. | |
| 6.5 | Send `/summary` immediately after starting with no messages. | Bot replies "No messages to summarize yet." | |

---

## 7. Edge Cases

| # | Scenario | Expected Result | Actual Result |
|---|----------|-----------------|---------------|
| 7.1 | Send a message in a group **not** in `allowed_groups`. | Bot ignores it. | |
| 7.2 | Restart the bot container (`docker compose restart faf-qq-bot`). | Bot reconnects and resumes working. | |
| 7.3 | Stop NapCatQQ (`docker compose stop napcat`), wait 10s, then start it again. | Bot reconnects automatically after NapCatQQ is back. | |
| 7.4 | Send a very long message (>500 chars) then `/summary`. | Bot handles it without crashing. | |

---

## 8. Stop and Clean Up

| # | Step | Expected Result | Actual Result |
|---|------|-----------------|---------------|
| 8.1 | Run `docker compose down`. | Both containers stop and are removed. | |
| 8.2 | Check no containers are running (`docker ps`). | No relevant containers running. | |

---

## 9. Issues and Notes

List any problems, error messages, or observations here:

```text
1.
2.
3.
```

---

## 10. Final Verdict

- [ ] All critical tests passed (4.x, 5.x, 6.x).
- [ ] Ready to build and push Docker image to Alibaba Cloud.
- [ ] Need fixes before deploying (explain in section 9).
