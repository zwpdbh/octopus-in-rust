# Strategic Plans Index

This directory holds long-range planning documents for the octopus workspace.  
For day-to-day operational status, see [`STATUS.md`](../../STATUS.md).

---

## Plans

| # | Document | Purpose |
|---|----------|---------|
| 13 | [`13-feature-checklist.md`](./13-feature-checklist.md) | P0/P1/P2 feature tracker across all tracks |
| 14 | [`14-brain-architecture.md`](./14-brain-architecture.md) | Extract a reusable Brain crate for octopus-cli and qqbot-core |
| 15 | [`15-qqbot-deployment.md`](./15-qqbot-deployment.md) | Deploy qqbot to AliCloud ECS via `cargo xtask` |

---

## Existing Trackers

The detailed 1:1 rewrite tracker lives outside this directory:

- [`docs/tracking/index.md`](../tracking/index.md) — module-by-module rewrite status with Python → Rust file mapping.

---

## How to Add a Plan

1. Create `docs/plans/NN-descriptive-name.md`.
2. Add a row to the table above.
3. Link it from `STATUS.md` if it becomes actively relevant.
