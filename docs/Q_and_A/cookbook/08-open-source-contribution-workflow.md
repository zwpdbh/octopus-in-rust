# Open Source Contribution Workflow

A step-by-step cookbook for contributing to open source projects using Git and GitHub.

## Scenario

You want to add a feature to an open source project (e.g., `MoonshotAI/kimi-cli`). You do not have write access to the official repository, so you work through a **fork** and submit changes via a **Pull Request (PR)**.

## Prerequisites

- A GitHub account
- Git installed locally
- SSH key or HTTPS authentication configured for GitHub

## The Complete Workflow

### 1. Fork the Upstream Repository (on GitHub)

Navigate to the official repository (e.g., `https://github.com/MoonshotAI/kimi-cli`) and click the **Fork** button. This creates your own copy under your account (e.g., `zwpdbh/kimi-cli`).

### 2. Clone Your Fork Locally

```bash
git clone git@github.com:zwpdbh/kimi-cli.git
cd kimi-cli
```

Your fork is now the `origin` remote:

```bash
git remote -v
# origin  git@github.com:zwpdbh/kimi-cli.git (fetch)
# origin  git@github.com:zwpdbh/kimi-cli.git (push)
```

### 3. Add the Upstream Remote (one-time setup)

Your fork does not automatically sync with the official repo. Add `upstream` to track it:

```bash
git remote add upstream https://github.com/MoonshotAI/kimi-cli.git
git remote -v
# origin    git@github.com:zwpdbh/kimi-cli.git (fetch/push)
# upstream  https://github.com/MoonshotAI/kimi-cli.git (fetch/push)
```

#### What is the difference between `origin` and `upstream`?

```
GitHub (cloud)
│
├─► MoonshotAI/kimi-cli    ←── the "official" repo  (= upstream)
│      │                          (you cannot push here)
│      │ ◄── fork button
│      │
├─► zwpdbh/kimi-cli        ←── your copy           (= origin)
           │                      (you CAN push here)
           │ ◄── git clone
           │
Your laptop
    │
    ├── .git/config says:
    │     origin = git@github.com:zwpdbh/kimi-cli.git      (your fork)
    │     upstream = https://github.com/MoonshotAI/kimi-cli.git  (official)
    │
    └── your code
```

| Remote | Points to | Who owns it | What you do with it |
|--------|-----------|-------------|---------------------|
| **`origin`** | Your fork (`zwpdbh/kimi-cli`) | **You** | Push your branches here; open PRs from here |
| **`upstream`** | Official repo (`MoonshotAI/kimi-cli`) | **The project maintainers** | Pull updates from here; never push |

**Why you need both:** Without `upstream`, your fork becomes a snapshot. When the official repo releases v1.48.0, your fork is still at v1.47.0. Your PR branch diverges and becomes stale.

### 4. Create a Feature Branch

Never work directly on `main`. Create a focused branch for your change:

```bash
git checkout -b feat/posttooluse-hook-stderr-to-llm
```

Branch naming conventions:
- `feat/` — new feature
- `fix/` — bug fix
- `docs/` — documentation
- `refactor/` — code refactoring

### 5. Make Changes and Commit

Edit files, then commit with a clear message:

```bash
git add src/kimi_cli/soul/toolset.py
git commit -m "feat(hooks): surface PostToolUse hook stderr to LLM context

PostToolUse hooks were fire-and-forget: their stdout/stderr were captured
but discarded. This made it impossible for hooks like docref to report
drift back to the LLM so it could act on the information.

Now PostToolUse hooks are awaited and any stderr they produce is appended
to the tool result's .message field, which the LLM sees in the next turn's
context."
```

Good commit messages explain **what** and **why**, not just "update file".

### 6. Push Your Branch to Your Fork

```bash
git push -u origin feat/posttooluse-hook-stderr-to-llm
```

The `-u` flag links your local branch to the remote branch for future `git push`/`git pull`.

### 7. Open a Pull Request (on GitHub)

Go to your fork on GitHub. You will see a yellow banner:

> **Your recently pushed branches:** `feat/posttooluse-hook-stderr-to-llm` — **Compare & pull request**

Click it. Set:
- **Base repository**: `MoonshotAI/kimi-cli` (upstream)
- **Base branch**: `main`
- **Head repository**: `zwpdbh/kimi-cli`
- **Compare branch**: `feat/posttooluse-hook-stderr-to-llm`

Click **Create pull request**.

### 8. Write a Good PR Description

A well-written PR helps maintainers review quickly:

```markdown
## Summary
Surface PostToolUse hook stderr to the LLM so hooks can act as reporters
rather than being forced to silently auto-fix or warn-only.

## Problem
Currently PostToolUse hooks are fire-and-forget. Their stderr is captured
but never shown to the LLM. This makes it impossible for a hook like
docref to say "docs are drifted, please fix them" — the LLM never sees it.

## Solution
Await PostToolUse hooks and append non-empty stderr to the tool result's
`.message` field, which becomes part of the LLM context.

## Trade-offs
- Tool calls now block until PostToolUse hooks complete
- For fast hooks (<1s) this is negligible
- Existing hooks that don't print stderr are unaffected

## Testing
- [ ] Manual: run `kimi` with docref hook, modify source file, verify
  LLM sees drift report in next turn
```

### 9. Address Review Feedback

Maintainers will leave comments. To update your PR:

```bash
# Edit files
git add .
git commit -m "address review: add timeout guard for hook await"
git push origin feat/posttooluse-hook-stderr-to-llm
```

The PR updates automatically. No need to close and reopen it.

**If you want to amend the last commit** instead of creating a new one:

```bash
git add .
git commit --amend --no-edit
git push --force-with-lease origin feat/posttooluse-hook-stderr-to-llm
```

> ⚠️ Only force-push on feature branches, never on `main`.

### 10. Keep Your Branch in Sync with Upstream

While your PR is open, upstream `main` may receive new commits. Rebase to stay current:

```bash
# Fetch upstream changes without merging
 git fetch upstream

# Rebase your branch on the latest upstream main
git checkout feat/posttooluse-hook-stderr-to-llm
git rebase upstream/main

# Force-push to update the PR
git push --force-with-lease origin feat/posttooluse-hook-stderr-to-llm
```

**Why rebase instead of merge?** Rebasing replays your commits on top of the latest `main`, producing a clean linear history. Merging creates extra "merge commits" that clutter the PR.

### 11. PR Gets Merged

Once approved, a maintainer merges your PR into upstream `main`. Your code is now official.

### 12. Clean Up (after merge)

```bash
# Sync your local main with upstream
git checkout main
git pull upstream main

# Sync your fork's main with upstream
git push origin main

# Delete the feature branch locally
git branch -d feat/posttooluse-hook-stderr-to-llm

# Delete the feature branch on your fork
git push origin --delete feat/posttooluse-hook-stderr-to-llm
```

## Cheat Sheet

```bash
# One-time setup
git clone git@github.com:YOURNAME/REPO.git
cd REPO
git remote add upstream https://github.com/UPSTREAM/REPO.git

# Daily workflow
git checkout main
git pull upstream main
git checkout -b feat/my-feature
# ... edit, commit ...
git push -u origin feat/my-feature

# Addressing review feedback
git add .
git commit --amend --no-edit
git push --force-with-lease origin feat/my-feature

# Sync with upstream while PR is open
git fetch upstream
git rebase upstream/main
git push --force-with-lease origin feat/my-feature

# Clean up after merge
git checkout main
git pull upstream main
git push origin main
git branch -d feat/my-feature
git push origin --delete feat/my-feature
```

## Common Mistakes to Avoid

| Mistake | Why it hurts | Fix |
|---|---|---|
| Working on `main` | Hard to sync with upstream; messy history | Always create a feature branch |
| `git merge upstream/main` on feature branch | Creates merge commits in PR | Use `git rebase upstream/main` |
| `git push --force` without `-with-lease` | Can overwrite someone else's push | Always use `--force-with-lease` |
| Vague commit messages | Reviewers don't understand intent | Explain what and why |
| Not adding `upstream` remote | Your fork falls behind; PRs get stale | `git remote add upstream <url>` |

## Further Reading

- [GitHub Docs: Contributing to projects](https://docs.github.com/en/get-started/quickstart/contributing-to-projects)
- [Git Rebasing](https://git-scm.com/book/en/v2/Git-Branching-Rebasing)
