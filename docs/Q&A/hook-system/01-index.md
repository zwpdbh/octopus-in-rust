# Hook System Tutorial — Index

This tutorial explains how the hook system works in `tmp/kimi-cli` (Python) and how it maps into `octopus-cli` (Rust). It is designed for anyone reimplementing or extending the permission / interception layer.

## Table of Contents

| # | Section | What You Will Learn |
|---|---------|---------------------|
| 1 | **Index** | This page. |
| 2 | **What Is a Hook?** | The conceptual foundation: why hooks exist, what problems they solve, and where they sit in a complex system. |
| 3 | **Architecture Overview** | The high-level components: `HookDef`, `HookEngine`, `HookRunner`, wire subscriptions, and event builders. |
| 4 | **Deep Dive: PreToolUse** | A complete walkthrough of the most important hook: how it is triggered, what payload it carries, how it blocks tool execution, and the exact code path. |
| 5 | **Hook Engine Internals** | Registration, matching by regex, deduplication, parallel execution, and the "fail-open" aggregation rule. |
| 6 | **Server-Side Runner** | How a matched hook becomes a subprocess: JSON on stdin, exit-code semantics, and the `permissionDecision` protocol. |
| 7 | **Wire-Side Hooks** | How hooks work over the wire: `HookRequest`, `HookResponse`, JSON-RPC, and the client-side subscription model. |
| 8 | **Mapping to octopus-cli (Rust)** | What changes when porting to Rust: typed enums vs. string literals, `serde` vs. `dict`, `tokio` subprocesses, and strong typing recommendations from `AGENTS.md`. |
| 9 | **Summary & Checklist** | A quick-reference checklist for implementing or debugging hooks. |

## How to Read This Tutorial

- **Start with section 2** if you are new to hook concepts.
- **Jump to section 4** if you want the exact `PreToolUse` code path immediately.
- **Read section 8** if you are actively porting the system to Rust and need architectural guidance.
