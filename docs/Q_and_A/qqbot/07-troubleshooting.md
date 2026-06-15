# 6. Troubleshooting

## 6.1 SnowLuma container exits or crashes

Run:

```bash
cargo run --bin qqbot -- logs snowluma -n 100
```

Common causes:

- **QQ native crash during hot update.** Usually transient. The supervisor will restart the container automatically.
- **Session corruption.** Run `cargo run --bin qqbot -- reset`, then `init` again and re-scan the QR code.
- **Out of memory.** SnowLuma needs at least 2 GB RAM and `--shm-size=1g`.

## 6.2 `qqbot-core` cannot connect to OneBot

Check the SnowLuma OneBot config:

```bash
cat data/snowluma-data/config/onebot.json
```

It should contain a `wsServers` entry listening on `0.0.0.0:3001` with role `Universal`.

Then check logs:

```bash
cargo run --bin qqbot -- logs core -n 50
cargo run --bin qqbot -- logs supervisor -n 50
```

## 6.3 The bot does not respond to `/summary`

1. Check that the `summary` plugin is enabled:

   ```bash
   cargo run --bin qqbot -- plugin list
   ```

2. Check that the bot is in the group and online:

   ```bash
   cargo run --bin qqbot -- health
   ```

3. Check core logs for plugin errors:

   ```bash
   cargo run --bin qqbot -- logs core -n 100
   ```

4. Make sure the message starts with the configured command prefix (default `/`).

## 6.4 Plugin changes are not picked up

After enabling/disabling a plugin, `qqbot` sends `SIGHUP` to `qqbot-core`. If you edited a plugin manually, run:

```bash
cargo run --bin qqbot -- plugin reload
```

If `qqbot-core` was started before the pid-file tracking was added, restart the daemon:

```bash
cargo run --bin qqbot -- restart
```

## 6.5 noVNC shows a black screen

The QQ client may still be starting. Wait 30–60 seconds and refresh. If the container keeps restarting, see [SnowLuma container exits or crashes](#61-snowluma-container-exits-or-crashes).

## 6.6 Where the QR code appears

Open `http://localhost:6081` in a browser. The password is `vncpasswd`. The QR code is inside the QQ login window in noVNC.

## 6.7 Reset everything

`reset` stops the daemon, removes the SnowLuma container, clears the QQ login
session, and also resets the SnowLuma WebUI admin password. Configs and enabled
plugins are preserved.

```bash
cargo run --bin qqbot -- stop
cargo run --bin qqbot -- reset
cargo run --bin qqbot -- init --account <QQ> --kimi-key <KEY> --group <GID>
```

After SnowLuma starts again, grab the new WebUI password from the container logs:

```bash
docker logs snowluma 2>&1 | grep -E "initial credentials|临时密码"
```

The default WebUI username is `admin`. The password is the one printed by the
line above.

Then open `http://localhost:6081`, scan the QR code, and add the bot to the
allowed group.
