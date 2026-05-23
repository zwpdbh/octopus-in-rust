# 15 — Utils

## Status: 🔄 Partial

## Python Source Files

| File | Description | LOC |
|------|-------------|-----|
| `kimi_cli/utils/slashcmd.py` | Slash command registry | ~145 |
| `kimi_cli/utils/path.py` | Path utilities (KaosPath) | ~200 |
| `kimi_cli/utils/export.py` | Session export/import | ~300 |
| `kimi_cli/utils/editor.py` | Editor detection | ~100 |
| `kimi_cli/utils/diff.py` | Diff rendering | ~150 |
| `kimi_cli/utils/changelog.py` | Changelog parser | ~100 |
| `kimi_cli/utils/string.py` | String utilities | ~100 |
| `kimi_cli/utils/file_filter.py` | File filtering | ~100 |
| `kimi_cli/utils/sensitive.py` | Sensitive file detection | ~100 |
| `kimi_cli/utils/frontmatter.py` | Frontmatter parser | ~100 |
| `kimi_cli/utils/media_tags.py` | Media tag extraction | ~100 |
| `kimi_cli/utils/datetime.py` | Date/time utilities | ~50 |
| `kimi_cli/utils/envvar.py` | Environment variable helpers | ~50 |
| `kimi_cli/utils/io.py` | I/O helpers | ~100 |
| `kimi_cli/utils/clipboard.py` | Clipboard integration | ~50 |
| `kimi_cli/utils/proxy.py` | Proxy configuration | ~100 |
| `kimi_cli/utils/signals.py` | Signal handling | ~100 |
| `kimi_cli/utils/subprocess_env.py` | Subprocess environment | ~100 |
| `kimi_cli/utils/shell_quoting.py` | Shell quoting | ~100 |
| `kimi_cli/utils/term.py` | Terminal detection | ~100 |
| `kimi_cli/utils/windows_paths.py` | Windows path handling | ~50 |
| `kimi_cli/utils/aiohttp.py` | HTTP client wrapper | ~100 |
| `kimi_cli/utils/aioqueue.py` | Async queue helpers | ~100 |
| `kimi_cli/utils/broadcast.py` | Broadcast channel | ~100 |
| `kimi_cli/utils/logging.py` | Logging config | ~200 |
| `kimi_cli/utils/message.py` | Message utilities | ~100 |
| `kimi_cli/utils/server.py` | Server utilities | ~100 |
| `kimi_cli/utils/pyinstaller.py` | PyInstaller helpers | ~50 |
| `kimi_cli/utils/proctitle.py` | Process title | ~50 |
| `kimi_cli/utils/typing.py` | Typing utilities | ~50 |
| `kimi_cli/utils/rich/*.py` | Rich renderers | ~300 |

## Rust Target Files

| File | Description | LOC | Status |
|------|-------------|-----|--------|
| `octopus-cli/src/utils/mod.rs` | Utils module placeholder | ~? | ⬜ Mostly empty |

## What's Done

- [x] `slashcmd.py` → `soul::slash` (registry merged into slash module)
- [x] Basic path handling (raw `std::path::PathBuf`)
- [x] Basic diff rendering (inline in file tools)

## What's Missing

- [ ] KaosPath (canonical, expanduser, path validation)
- [ ] Export / import functionality
- [ ] Editor auto-detection ($VISUAL / $EDITOR)
- [ ] Sensitive file detection
- [ ] Frontmatter parsing
- [ ] Media tag extraction
- [ ] Clipboard integration
- [ ] Proxy configuration
- [ ] Signal handling (Ctrl-C graceful shutdown)
- [ ] HTTP client wrapper (reqwest config)
- [ ] Logging configuration (tracing setup)
- [ ] Rich renderers (markdown, syntax highlighting, columns)
