# Run a Forked Python CLI Alongside the Official Installation

How to keep two versions of the same Python CLI tool on your system — the stable official release and your forked development version — without conflicts.

## Scenario

You want to contribute to a Python CLI tool (e.g., `kimi-cli`). You have:
- The **official release** installed system-wide (via `pip`, `uv`, or another package manager)
- Your **forked source code** cloned locally with changes you want to test

You need both available as separate commands:
- `kimi` → the stable official version
- `mykimi` → your forked development version

## Why This Pattern Matters

| Problem | Solution |
|---------|----------|
| You break your dev version while experimenting | Official `kimi` still works |
| You need to compare behavior between versions | Run both side-by-side |
| You want to test a PR without replacing stable | `mykimi` uses your fork |
| Official releases updates | Your fork stays independent |

## Prerequisites

- `uv` installed (or `pip` with virtualenv)
- The official CLI already installed and working
- Your fork cloned locally (e.g., `~/code/kimi-cli`)

## The Pattern

### 1. Verify the Official Installation

```bash
which kimi
# /home/user/.local/bin/kimi

kimi --version
# kimi, version 1.46.0
```

### 2. Verify Your Fork Has a Runnable Module

Most Python CLI tools use `python -m <package_name>`:

```bash
cd ~/code/kimi-cli
ls src/kimi_cli/__main__.py
# src/kimi_cli/__main__.py   ← this means `python -m kimi_cli` works
```

If the project uses `src/` layout (common for modern Python), the package lives under `src/<name>/`.

### 3. Let `uv` Create the Development Environment

`uv run` automatically creates a virtualenv, installs dependencies from `pyproject.toml`, and handles local path dependencies:

```bash
cd ~/code/kimi-cli

# This one command does everything:
# - Creates .venv/
# - Installs all dependencies from pyproject.toml
# - Installs local packages (e.g., packages/kosong) in editable mode
# - Runs the module
uv run python -m kimi_cli --version
```

You should see your fork's version (higher than official if upstream has moved):

```
kimi, version 1.47.0
```

### 4. Create the Wrapper Script

Create a wrapper that `cd`s into your project and uses `uv run`:

```bash
cat > ~/.local/bin/mykimi << 'EOF'
#!/bin/bash
# Run the forked kimi-cli from source.
# The official `kimi` command uses the uv-installed release.
# This wrapper uses `uv run` to execute the local source code.
set -e
cd /home/user/code/kimi-cli
exec uv run python -m kimi_cli "$@"
EOF

chmod +x ~/.local/bin/mykimi
```

> Replace `/home/user/code/kimi-cli` with your actual fork path.

### 5. Verify Both Commands Work

```bash
# Official (stable)
kimi --version
# kimi, version 1.46.0

# Forked (your development version)
mykimi --version
# kimi, version 1.47.0
```

Both use the same config directory (`~/.kimi/`), so sessions and settings carry over.

## How It Works

```
System PATH
│
├─► ~/.local/bin/kimi          ← wrapper from uv tool install
│      │
│      └──► ~/.local/share/uv/tools/kimi-cli/   (official release)
│            └── bin/python3 → kimi_cli package from PyPI
│
└─► ~/.local/bin/mykimi        ← your custom wrapper
       │
       └──► cd ~/code/kimi-cli && uv run python -m kimi_cli
             │
             ├── .venv/        (uv-managed virtualenv)
             ├── src/kimi_cli/  (your forked source code)
             └── packages/llm-provider/src/  (local dependencies)
```

| Aspect | Official `kimi` | Forked `mykimi` |
|--------|-----------------|-----------------|
| **Installation** | `uv tool install` or `pip install` | `git clone` + `uv run` |
| **Code source** | PyPI release tarball | Your local `src/` directory |
| **Virtualenv** | `~/.local/share/uv/tools/kimi-cli/` | `~/code/kimi-cli/.venv/` |
| **Dependencies** | Locked to release | From `pyproject.toml` + local packages |
| **Updates** | `uv tool upgrade kimi-cli` | `git pull upstream main` |

## Variant: Using `pip` + `venv` Instead of `uv`

If the project does not use `uv`:

```bash
cd ~/code/some-project
python3 -m venv .venv
source .venv/bin/activate
pip install -e .                    # install project in editable mode
pip install -e packages/some-lib    # install local deps
```

Wrapper script:

```bash
cat > ~/.local/bin/mysome << 'EOF'
#!/bin/bash
cd /home/user/code/some-project
source .venv/bin/activate
exec python -m some_project "$@"
EOF

chmod +x ~/.local/bin/mysome
```

## Variant: Projects Without `__main__.py`

Some Python CLI tools use a console script entry point instead of `python -m`. Check `pyproject.toml`:

```toml
[project.scripts]
kimi = "kimi_cli.__main__:main"
```

For these, `uv run` still works because it registers the console script in `.venv/bin/`:

```bash
cd ~/code/kimi-cli
uv run kimi --version    # uses the .venv's entry point
```

Wrapper:

```bash
cat > ~/.local/bin/mykimi << 'EOF'
#!/bin/bash
cd /home/user/code/kimi-cli
exec uv run kimi "$@"
EOF
```

## Keeping the Fork in Sync

```bash
cd ~/code/kimi-cli

# Fetch official updates
git fetch upstream

# Rebase your feature branch
git checkout feat/my-feature
git rebase upstream/main

# The .venv stays valid — uv automatically updates deps when needed
mykimi --version   # tests your rebased code
```

## Cleanup

If you want to remove the forked version:

```bash
rm ~/.local/bin/mykimi          # remove wrapper
cd ~/code/kimi-cli
rm -rf .venv                     # remove virtualenv (optional)
```

The official `kimi` remains untouched.

## Cheat Sheet

```bash
# One-time setup
git clone git@github.com:YOURNAME/kimi-cli.git ~/code/kimi-cli
cd ~/code/kimi-cli
uv run python -m kimi_cli --version   # verify it works

# Create wrapper
cat > ~/.local/bin/mykimi << 'EOF'
#!/bin/bash
cd /home/user/code/kimi-cli
exec uv run python -m kimi_cli "$@"
EOF
chmod +x ~/.local/bin/mykimi

# Daily use
kimi --version      # official
mykimi --version    # forked

# After rebasing on upstream
cd ~/code/kimi-cli
git fetch upstream
git rebase upstream/main
mykimi --version    # test updated fork
```
