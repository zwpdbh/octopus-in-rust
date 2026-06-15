# docref

A small CLI tool that keeps markdown documentation in sync with code. It is
designed to be invoked from LLM agent hooks (e.g. `PostToolUse`) so that docs
are checked automatically whenever source files change.

## Install

```bash
cargo install --path docref
# or, once published:
cargo install docref
```

## Quick start

If you have Kimi CLI installed, run this once:

```bash
docref init --apply   # auto-detect and configure Kimi
```

Then scan your project docs:

```bash
docref scan
```

After that, every `WriteFile` / `StrReplaceFile` in Kimi will trigger a drift
check and warn you if any referenced docs have gone stale.

If you have multiple supported tools installed, `docref init --apply` will ask
you to pick one with `--tool <name>`. Run `docref init` (without `--apply`) to
see what's available without changing any config files.

## Convention

`docref` looks for source-location comments inside markdown code blocks:

```markdown
```rust
// src/hooks/engine.rs ~line 196 — HookEngine::trigger
pub async fn trigger(&self, event: HookEvent, matcher_value: &str) -> Vec<HookResult> {
    // ...
}
```
```

The comment format is:

```
// <relative-source-path> ~line <number> — <item-name>
```

Item names may be qualified (`HookEngine::trigger`), annotated
(`HookEngine (abbreviated)`), or descriptive labels. Descriptive labels are
reported as warnings rather than hard drift.

The comment prefix should match the code block's language:

| Prefix | Languages |
|---|---|
| `//` | Rust, C, C++, Java, JS, TS, Go, Swift, Kotlin |
| `#` | Python, Ruby, Bash, Perl, YAML, R, Makefile |
| `*` | Javadoc / Doxygen block comments |
| `--` | SQL, Lua, Haskell |
| `<!--` | HTML, XML |
| `;` | Lisp, Clojure, Assembly, Ini |
| `;;` | Scheme, Emacs Lisp |
| `(*` | OCaml, Pascal |
| `%` | Prolog, Erlang, LaTeX, Matlab |

### Demo / teaching / pseudo-code snippets

Not every code block corresponds to project source. For pure examples, teaching
illustrations, or pseudo-code, add a **demo marker** as the first line inside
the code block:

```rust
// (demo)
pub fn hypothetical_example() { ... }
```

```python
# (example)
def teaching_demo(): ...
```

Supported markers: `(demo)`, `(example)`, `(pseudo-code)`, `(teaching)`.

`docref` will skip these blocks during drift checks and migration.

## Commands

### `init`

Detect installed LLM CLI tools and configure docref hooks.

```bash
docref init                          # detection + instructions
docref init --tool kimi --apply      # auto-configure Kimi CLI
```

Supported tools:

| Tool | Detection | Hook support |
|---|---|---|
| Kimi CLI | `kimi` in PATH or `~/.kimi/config.toml` | ✅ PostToolUse |
| Claude Code | `claude` in PATH or `~/.claude/settings.json` | planned |
| OpenAI Codex CLI | `codex` in PATH | planned |
| Cursor | `cursor` in PATH | planned |

### `scan`

Index all markdown docs and source references:

```bash
docref scan                              # default: AGENTS.md, README.md, docs/
docref scan docs/Q&A/hook-system         # specific directory
docref scan docs/guide.md                # specific file
```

### `check`

Check whether recorded references still point to the right place:

```bash
# Check references to one source file (ideal for PostToolUse hooks)
docref check --source src/hooks/engine.rs

# Check every indexed reference
docref check --all

# JSON output for programmatic use
docref check --source src/hooks/engine.rs --format json
```

Exit code is `1` if drift is detected, `0` otherwise.

### `status`

Show index summary:

```bash
docref status
```

### `hook`

Run as an LLM agent hook. Currently supports the **Kimi CLI** event format.

```bash
# Reads a Kimi PostToolUse JSON event from stdin
docref hook kimi
```

## Kimi CLI integration

After running `docref init --tool kimi --apply`, your `~/.kimi/config.toml`
will contain:

```toml
[[hooks]]
event = "PostToolUse"
matcher = "WriteFile|StrReplaceFile"
command = "cd {cwd} && docref scan --format json >/dev/null 2>&1 && docref hook kimi"
timeout = 30
```

In the first session of a project, run `docref scan` manually so the index is
created. After that, every file edit will trigger a drift check.

The hook is **non-blocking** — it always exits 0 so the tool use completes.
Warnings are printed to stderr for you and the LLM to see.

## Storage

`docref` keeps its index in an SQLite database (`.docref.db` by default).
You should commit this file if you want CI to share the same baseline, or
regenerate it in CI with `docref scan`.

## Design notes

- Drift threshold is currently **5 lines**.
- Body-level drift detection is intentionally not the primary signal; line
  drift and item existence are.
- Descriptive labels (items that don't resolve to a code identifier) are
  reported as warnings, not errors.
