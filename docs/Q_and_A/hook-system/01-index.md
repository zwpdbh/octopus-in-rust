# Hook System Tutorial — Index

This tutorial explains how the hook system is designed and implemented in **octopus-cli** (Rust), using the original Python `kimi-cli` as a reference point. It is written for anyone who wants to understand how to build a permission / interception / extension layer in a complex system, and how a Rust rewrite can improve on a Python predecessor.

> **Context:** We are reimplementing `tmp/kimi-cli` (Python, ~47k LOC) as `octopus-cli` (Rust, ~15k LOC). The hook system is one of the most instructive parts of this rewrite because it touches type design, concurrency, subprocess management, and cross-process communication.

## Table of Contents

| # | Section | What You Will Learn |
|---|---------|---------------------|
| 1 | **Index** | This page. |
| 2 | **What Is a Hook?** | The conceptual foundation, independent of language. |
| 3 | **Architecture Overview** | The Rust design: `HookEvent` enum, `HookEngine`, `HookRunner`, wire types, and how they compare to the Python original. |
| 4 | **Deep Dive: PreToolUse** | The full Rust code path — from `Toolset::call()` through `HookEngine::trigger()` to `run_hook()` — with Python references. |
| 5 | **Hook Engine Internals** | Registration, compiled-regex matching, `Arc<HookEvent>` dispatch, parallel execution, and the "fail-open" rule. |
| 6 | **Server-Side Runner** | How `run_hook()` turns a `HookDef` into a `tokio::process::Command`, feeds JSON on stdin, and parses stdout with typed structs. |
| 7 | **Wire-Side Hooks** | How `HookRequest` / `HookResponse` travel over JSON-RPC, and how the Rust `WireEvent` enum prevents the trial-and-error deserialization the Python version used. |
| 8 | **Lessons from the Rewrite** | Concrete improvements the Rust port made over Python: strong enums, typed deserialization, compiled regexes, and leak-free wire handles. |
| 9 | **Summary & Checklist** | A quick-reference checklist for implementing or debugging hooks. |

## How to Read This Tutorial

- **Start with section 2** if you are new to hook concepts.
- **Jump to section 4** if you want the exact `PreToolUse` code path immediately.
- **Read section 8** if you want to see the before/after comparison between the Python original and the Rust rewrite.
