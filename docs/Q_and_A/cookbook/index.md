# Cookbook Index

Practical Rust patterns used across the Octopus codebase.

| # | Pattern | File |
|---|---------|------|
| 01 | Move a field out of a struct with `Option::take()` | [01-option-take-move-out.md](./01-option-take-move-out.md) |
| 02 | Hold a RAII guard with `let _guard` | [02-raii-guard-pattern.md](./02-raii-guard-pattern.md) |
| 03 | Enforce "setup before use" with a newtype adapter | [03-newtype-adapter-enforce-construction-invariant.md](./03-newtype-adapter-enforce-construction-invariant.md) |
| 04 | Eliminate schema/parser drift with `schemars` + associated types | [04-typed-tool-schemars-associated-type.md](./04-typed-tool-schemars-associated-type.md) |
| 05 | Carry ambient async context with `tokio::task_local!` | [05-task-local-async-context.md](./05-task-local-async-context.md) |
| 06 | Mutate state through `&self` with interior mutability | [06-interior-mutability-mutate-through-shared-ref.md](./06-interior-mutability-mutate-through-shared-ref.md) |
| 07 | Never hold a `std::sync::Mutex` across `.await` | [07-never-hold-std-sync-mutex-across-await.md](./07-never-hold-std-sync-mutex-across-await.md) |
| 08 | Contribute to open source via fork + PR | [08-open-source-contribution-workflow.md](./08-open-source-contribution-workflow.md) |
| 09 | Run a forked Python CLI alongside the official install | [09-run-forked-python-cli-alongside-official.md](./09-run-forked-python-cli-alongside-official.md) |
| 10 | Switch LLM providers with a config-driven factory | [10-switch-llm-providers-with-config-driven-factory.md](./10-switch-llm-providers-with-config-driven-factory.md) |
