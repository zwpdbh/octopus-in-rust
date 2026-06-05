# 🏢 Welcome to Octopus-CLI — A Guided Tour

> *"Every building tells a story. This one speaks Rust."*

Welcome, visitor! You've arrived at **Octopus-CLI**, a faithful Rust rewrite of the Python `kimi-cli`. Think of this codebase not as files on a disk, but as a **sprawling, multi-story building** where each floor houses a different department, each corridor connects distinct systems, and every room contains machinery that keeps the AI assistant running.

This tour series will walk you through the building floor by floor, room by room. We'll pause at the interesting exhibits, open the machinery panels, and explain **how it works**, **where it differs from the Python original**, and **where Rust's architecture shines**.

---

## 🗺️ Tour Map

| Stop | Floor | What You'll See |
|------|-------|-----------------|
| [Tour 1: The Lobby](./01-lobby.md) | Ground | `main.rs`, `app.rs`, CLI parsing — where every visit begins |
| [Tour 2: The Control Room](./02-control-room.md) | 2F | `KimiSoul` — the beating heart of the agent loop |
| [Tour 3: The Tool Shed](./03-tool-shed.md) | 2F West | `Tool` trait, `KimiToolset`, and the tool execution pipeline |
| [Tour 4: The Security Desk](./04-security-desk.md) | 1F | Approval flow, OAuth, and the Y/N/A prompt system |
| [Tour 5: The Communication Hub](./05-communication-hub.md) | 3F | Wire protocol, notifications, and the broadcast channel |
| [Tour 6: The Workshop](./06-workshop.md) | Basement | Background tasks and subagent spawning |
| [Tour 7: The Front Desk](./07-front-desk.md) | Ground East | TUI shell, markdown rendering, and user interaction |
| [Tour 8: The Archives](./08-archives.md) | Sub-basement | Session persistence, context management, forking |
| [Tour 9: The Observatory](./09-observatory.md) | Rooftop | Telemetry, hooks, and the event tracking system |
| [Tour 10: The Hook System](./10-hook-system.md) | Security Annex | Deep dive into server-side and wire hooks |

---

## 🎭 Narrator's Note: Python vs. Rust

Before we enter, a word about our two protagonists:

**Python (the original architect)** built with **flexibility** — dynamic typing, runtime introspection, decorators, and the GIL. The Python codebase is ~47,000 lines of elegant, expressive code.

**Rust (the new architect)** builds with **contracts** — zero-cost abstractions, fearless concurrency, and the borrow checker. Our Rust codebase is ~15,000 lines (32% of Python's size) but packs the same punch.

> 🔑 **Key insight:** Rust's LOC reduction isn't from missing features. It's from **inlining** (what Python spreads across 5 files, Rust often keeps in 1), **eliminating runtime glue** (no `__init__.py` ceremony), and **type-system compression** (enums replace class hierarchies).

Throughout the tour, we'll call out three kinds of differences:
- 🐍 **Python's way** — how the original solved the problem
- 🦀 **Rust's way** — how we translated it (or reimagined it)
- ✨ **Where Rust shines** — architectural wins, performance gains, or safety guarantees

---

## 🚪 Let's Begin

The front doors are at [`01-lobby.md`](./01-lobby.md). Step inside!
