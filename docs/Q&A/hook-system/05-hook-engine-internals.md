# 5. Hook Engine Internals

The `HookEngine` is the brain of the hook system. It decides *which* hooks run, *when* they run, and *how* their results are combined. This section dissects the implementation in `src/kimi_cli/hooks/engine.py`.

## 5.1 Data Structures

```python
@dataclass
class HookResult:
    action: Literal["allow", "block"] = "allow"
    reason: str = ""

class HookEngine:
    def __init__(
        self,
        hooks: list[HookDef] | None = None,
        cwd: str | None = None,
        on_wire_hook: Callable[[WireHookHandle], Awaitable[None]] | None = None,
    ):
        self._hooks: list[HookDef] = list(hooks) if hooks else []
        self._wire_subs: list[WireHookSubscription] = []
        self._cwd = cwd
        self._on_wire_hook = on_wire_hook

        # Indexes for O(1) lookup by event name
        self._by_event: dict[str, list[HookDef]] = {}
        self._wire_by_event: dict[str, list[WireHookSubscription]] = {}
        self._pending_wire_hooks: dict[str, WireHookHandle] = {}

        self._rebuild_indexes()
```

The engine maintains **two indexes**:
- `_by_event`: maps `"PreToolUse"` → list of local `HookDef` objects.
- `_wire_by_event`: maps `"PreToolUse"` → list of remote `WireHookSubscription` objects.

These are rebuilt whenever hooks or subscriptions are added.

## 5.2 Index Rebuilding

```python
def _rebuild_indexes(self) -> None:
    self._by_event.clear()
    for h in self._hooks:
        self._by_event.setdefault(h.event, []).append(h)

    self._wire_by_event.clear()
    for sub in self._wire_subs:
        self._wire_by_event.setdefault(sub.event, []).append(sub)
```

This is simple but effective: it trades a small amount of startup time for O(1) event lookup at trigger time.

## 5.3 Registration API

### Adding Server-Side Hooks

```python
def add_hooks(self, hooks: list[HookDef]) -> None:
    self._hooks.extend(hooks)
    self._rebuild_indexes()
```

Called during startup after parsing `config.toml`.

### Adding Wire Subscriptions

```python
def add_wire_subscriptions(self, subs: list[WireHookSubscription]) -> None:
    self._wire_subs.extend(subs)
    self._rebuild_indexes()
```

Called when a wire client sends its subscription list during initialization.

## 5.4 The Trigger Method (Full Implementation)

```python
async def trigger(
    self,
    event: HookEventType,
    *,
    matcher_value: str = "",
    input_data: dict[str, Any],
) -> list[HookResult]:
    """
    Trigger all hooks matching the given event and matcher_value.
    Returns a list of HookResult, one per executed hook.
    """
    tasks: list[asyncio.Task[HookResult]] = []

    # --- Server-side hooks ---
    matched_hooks = self._match_hooks(event, matcher_value)
    for hook in matched_hooks:
        tasks.append(
            asyncio.create_task(
                run_hook(hook.command, input_data, timeout=hook.timeout, cwd=self._cwd),
                name=f"hook:{hook.command}",
            )
        )

    # --- Wire-side hooks ---
    matched_wire = self._match_wire(event, matcher_value)
    for sub in matched_wire:
        handle = WireHookHandle(
            id=str(uuid.uuid4()),
            subscription_id=sub.id,
            event=event,
            target=matcher_value,
            input_data=input_data,
            _future=asyncio.get_event_loop().create_future(),
        )
        self._pending_wire_hooks[handle.id] = handle
        if self._on_wire_hook:
            asyncio.create_task(self._on_wire_hook(handle))
        tasks.append(asyncio.create_task(handle.wait(), name=f"wire:{sub.id}"))

    # --- Run everything in parallel ---
    raw_results = await asyncio.gather(*tasks, return_exceptions=True)

    # --- Parse with fail-open ---
    results: list[HookResult] = []
    for r in raw_results:
        if isinstance(r, Exception):
            results.append(HookResult(action="allow", reason=f"Hook failed: {r}"))
        else:
            results.append(r)

    # --- Cleanup wire handles ---
    for sub in matched_wire:
        # Handles are removed from _pending_wire_hooks when resolved
        pass

    # --- Telemetry (fire-and-forget) ---
    self._emit_telemetry(event, matcher_value, len(tasks), results)

    return results
```

## 5.5 Matching Logic

```python
def _match_hooks(self, event: str, matcher_value: str) -> list[HookDef]:
    candidates = self._by_event.get(event, [])
    matched = []
    for h in candidates:
        if not h.matcher or re.search(h.matcher, matcher_value):
            matched.append(h)
    return matched

def _match_wire(self, event: str, matcher_value: str) -> list[WireHookSubscription]:
    candidates = self._wire_by_event.get(event, [])
    matched = []
    for sub in candidates:
        if not sub.matcher or re.search(sub.matcher, matcher_value):
            matched.append(sub)
    return matched
```

### Regex Behavior

- `matcher = ""` → matches everything (default).
- `matcher = "shell"` → matches exactly the string `shell` anywhere in `matcher_value`.
- `matcher = "shell|bash"` → matches either `shell` or `bash`.
- Invalid regex is handled gracefully (falls back to no-match).

## 5.6 Deduplication

Server-side hooks are deduplicated by **command string**:

```python
def _deduplicate_hooks(self, hooks: list[HookDef]) -> list[HookDef]:
    seen: set[str] = set()
    deduped: list[HookDef] = []
    for h in hooks:
        if h.command not in seen:
            seen.add(h.command)
            deduped.append(h)
    return deduped
```

Why? A user might accidentally register the same script twice in `config.toml`. Deduplication prevents double-execution.

Wire subscriptions are **not** deduplicated because each subscription comes from a different client and may return a different decision.

## 5.7 Fail-Open Guarantee

The engine guarantees that failures never accidentally block:

| Failure Mode | Result | Reasoning |
|--------------|--------|-----------|
| Subprocess crashes | `allow` | Don't block users because a script has a bug. |
| Timeout (default 30s) | `allow` | Don't freeze the CLI because a hook is slow. |
| Invalid JSON stdout | `allow` | Malformed output is treated as non-blocking. |
| Wire client disconnects | `allow` | Don't block if the remote UI is gone. |
| Telemetry crash | Ignored | Telemetry is outside the main try/except. |

This is critical for **availability**: a broken hook should not brick the entire tool.

## 5.8 Aggregation Semantics

```python
def _aggregate(self, results: list[HookResult]) -> HookResult:
    for r in results:
        if r.action == "block":
            return r  # First block wins; reason is preserved
    return HookResult(action="allow")
```

In practice, the caller (`toolset.py`) does its own loop, but the semantic is the same: **any block blocks**.

## 5.9 Thread Safety

The `HookEngine` is **not thread-safe** for mutation but is safe for concurrent triggers because:
- `add_hooks()` and `add_wire_subscriptions()` are only called during setup.
- `_pending_wire_hooks` is accessed only from the same event loop.
- `asyncio.gather` runs all hooks concurrently within the same loop.

If you need dynamic hook registration at runtime, you would need a lock around index rebuilding.
