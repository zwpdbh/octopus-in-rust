# LLM Project Bootstrap Template

> **Purpose**: Initialize a complex, evolving project for LLM-assisted development.  
> **How to use**: Copy this file into the root of a new project, then tell the LLM:  
> "Use `_template.md` to initialize the project structure and context."

---

## Instructions for the LLM

1. **Read this file fully** before doing anything else.
2. **Inspect the existing project** — read `Cargo.toml`, existing source files, and any existing docs to understand the codebase.
3. **Create the management backbone files** listed in the next section.
4. **Populate them accurately** with project-specific content. Do not invent status, decisions, or features you cannot verify.
5. **Run the verification steps** at the end of this file.
6. **Report completion** and list every file created or modified.

> ⚠️ Do not add features or refactor code during bootstrap unless explicitly requested. This pass is strictly about project structure and context.

---

## Files to Create

### 1. `AGENTS.md` — Project Constitution

Create a project constitution that every future LLM must read. Include:

- **Coding standards**: naming, formatting, error handling, forbidden patterns.
- **Architecture decisions**: language, framework, async runtime, storage, protocol, rewrite/port strategy.
- **Module/crate boundaries**: what belongs where, what dependencies are allowed.
- **Testing requirements**: minimum coverage, test gates, integration-test conventions.
- **Documentation rules**: when and how to update docs after code changes.

Keep it concise but enforceable. Mark decisions as **Locked** if they should not change without explicit approval.

---

### 2. `STATUS.md` — Operational Status

Create a single source of truth for current project state. Include:

- **Phase** and **active task** (use `TBD — pending next priority` if none).
- **Last completed task** (or `None` if bootstrap is the first work).
- **Compilation status** and **test status**.
- **Blockers** (or `None`).
- **Track/table overview** of major components with status symbols.
- **Recent decisions** table.
- Cross-references to `AGENTS.md`, `README.md`, `docs/plans/`, and trackers.

Update this file at the end of every session.

---

### 3. `README.md` — Overview + LLM Health Checklist

Create a README that serves both humans and LLMs. It must contain:

- **One-line pitch** and **current phase**.
- **What This Project Is**: original → replacement mapping table (if applicable).
- **LLM Agent Project Health Checklist** with four sections:
  - 🔴 **Pre-Flight Check**: read `AGENTS.md`, `STATUS.md`, active task spec, run `scripts/project-status.sh`.
  - 🟡 **Structural Compliance Check**: verify backbone files exist.
  - 🟢 **Ongoing Session Compliance**: task scope, compile gate, test gate, crate boundaries, git hygiene.
  - 🔵 **Session End Compliance**: update task spec, update `STATUS.md`, run `./scripts/project-status.sh`, verify `cargo check`/`cargo test`, review `git status`.
- **Project Status at a Glance**: ASCII tree or table of components.
- **Architecture Decisions (Locked)**: short table of locked choices.
- **Quick Start**: exact commands to build and test.
- **Workspace Layout**: file tree.
- **Reference Map**: where to find conventions, status, tasks, plans, deep-dives.
- **License**.

---

### 4. `tasks/_template.md` — Task Spec Template

Create a template for future tasks. Include sections for:

- Goal
- Background
- Scope (In Scope / Out of Scope)
- Acceptance Criteria
- Implementation Notes
- Completed Steps
- Decisions Made

---

### 5. `tasks/completed/` — Completed Task Archive

Create an empty directory. Future finished task specs move here.

---

### 6. `scripts/project-status.sh` — Automated State Reporter

Create an executable bash script that reports:

- Git branch and `git status --short`.
- Active task from `STATUS.md`.
- `cargo check --workspace` result.
- `cargo test` result (adapt command to the project's actual test command).
- List of active tasks (`tasks/*.md` excluding `_template.md`).
- List of completed tasks (`tasks/completed/*.md`).

Make it executable with `chmod +x scripts/project-status.sh`.

Adapt the script if the project does not use Cargo (e.g., use `npm test`, `pytest`, etc.).

---

### 7. `docs/plans/00-index.md` — Strategic Plans Index

Create an index for long-range planning documents. Include:

- Table of plans (start with `13-feature-checklist.md`).
- Links to existing trackers or roadmaps.
- Instructions for adding new plans.

---

### 8. `docs/plans/13-feature-checklist.md` — Feature Tracker

Create a P0/P1/P2 feature checklist. Include:

- **P0**: Required for parity / MVP.
- **P1**: Important quality-of-life or architectural improvements.
- **P2**: Stretch goals.

Use status symbols (✅ / 🔄 / ❌) and keep items aligned with the project's actual components.

---

## Verification Steps

Run these before declaring bootstrap complete:

- [ ] `AGENTS.md` exists and is non-empty.
- [ ] `STATUS.md` exists and reflects the current project state.
- [ ] `README.md` exists and contains the full LLM health checklist.
- [ ] `tasks/_template.md` exists.
- [ ] `tasks/completed/` directory exists.
- [ ] `scripts/project-status.sh` exists and is executable.
- [ ] `docs/plans/00-index.md` exists.
- [ ] `docs/plans/13-feature-checklist.md` exists.
- [ ] Project's build command passes (e.g., `cargo check --workspace`).
- [ ] Project's test command passes (e.g., `cargo test`).
- [ ] `./scripts/project-status.sh` runs without errors.

---

## Reporting Template

When bootstrap is complete, report using this format:

```
Bootstrap complete.

Files created:
- AGENTS.md
- STATUS.md
- README.md
- tasks/_template.md
- tasks/completed/
- scripts/project-status.sh
- docs/plans/00-index.md
- docs/plans/13-feature-checklist.md

Verification:
- cargo check: ✅
- cargo test: ✅
- project-status.sh: ✅

Current phase: <phase>
Active task: <TBD or name>
Blockers: <none or list>
```

---

## Notes

- This template assumes a Rust project using Cargo. For other ecosystems, replace `cargo check`/`cargo test` with the appropriate commands.
- Do not over-engineer initial content. Accuracy is more important than completeness.
- If a project already has some of these files, read them first and update them rather than overwriting, unless they are empty or obviously stale.
