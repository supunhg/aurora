# AGENTS.md — Aurora Editor

## What This Is

Rust workspace for a native AI-native code editor. Early scaffold phase — most subsystems are stubs or partial implementations.

## Build & Test Commands

```bash
cargo build --workspace              # headless (no GUI)
cargo test --workspace               # run all tests
cargo test -p editor                 # single crate
cargo test -p ai                     # single crate
cargo run -p aurora-bin -- --self-test        # headless self-test
cargo run -p aurora-bin -- --editor-test      # editor core test

# GUI (requires Rust 1.80+)
cargo build -p aurora-bin --features gui
cargo run -p aurora-bin --features gui -- --self-test
```

CI pipeline: `cargo fmt --check && cargo clippy --workspace -- -D warnings && cargo test --workspace && cargo build --workspace`

## Toolchain

- `rust-toolchain.toml`: stable channel, with `rustfmt` + `clippy` components
- Edition 2021 across all crates

## Workspace Crates

| Crate | Role | Notes |
|-------|------|-------|
| `aurora-bin` | Binary entry point | Dispatches to `--self-test`, `--editor-test`, or GUI |
| `aurora-core` | Shared types/versioning | Minimal, just serde + version string |
| `ai` | AI router + providers | Provider adapter trait, fallback chain, agent loop, FreeLLMAPI client |
| `config` | Hot-reloadable TOML config | Watcher via `notify` (not yet wired), schema validation |
| `editor` | Core text editing | Ropey-backed buffer, multi-cursor, viewport, syntax, events |
| `ui` | UI layer | Headless by default, egui behind `gui` feature |
| `plugin` | WASI plugin host | Feature-gated on `wasi` (skeleton only) |
| `lsp` | Language Server Protocol | JSON-RPC over stdio, connection pooling, debouncing, bridge |

## Feature Flags

- `ai`: `cloud-ai` (default), `local-ai`, `keychain` (encryption + SQLite)
- `editor`: `treesitter` (optional tree-sitter highlighting)
- `ui`: `gui` (enables eframe/egui window)
- `plugin`: `wasi` (enables wasmtime runtime)
- `aurora-bin`: `gui` (forwards to `ui/gui`)

## Critical Bug Fixed: Buffer UTF-8

**Every ropey mutation method** (`insert`, `remove`, `slice`) expects **character indices**, but the `Buffer` API exposes **byte offsets** everywhere. This works for ASCII (byte == char) but panics for multi-byte UTF-8.

**Fix pattern:** Convert byte→char before every ropey call:
```rust
let char_idx = self.rope.byte_to_char(byte_pos);
self.rope.insert(char_idx, text);
```

The `get_line` method also needed fixing: `rope.line_to_byte()` returns byte offsets, so convert to char indices before passing to `rope.slice()`.

## Key Gotchas

1. **`notify` crate is unused**: `config/Cargo.toml` depends on `notify` but `ConfigLoader` has no filesystem watcher. Hot-reload is not implemented.

2. **`GroqProvider` is real HTTP**: Under `cloud-ai` feature, it makes actual requests to `api.groq.com`. The self-test uses it with a placeholder key.

3. **`plugin` crate is empty**: Prints a message only. No actual WASI runtime initialization.

4. **`aurora-core` is vestigial**: Contains only a hardcoded `version()` function.

5. **`config` crate name aliasing**: Package name is `config` but imported as `aurora-config = { path = "../config", package = "config" }` in `ai/Cargo.toml`.

## Architecture (High-Signal)

- **Provider adapter pattern**: `ProviderAdapter` trait in `ai/src/providers/mod.rs`. Implement `chat_completion` and optionally `stream_chat_completion`.
- **Fallback chain**: Router tries providers in registration order, skips unhealthy/rate-limited, tracks fallback count.
- **Editor core**: `Editor` struct in `editor/src/lib.rs` bundles `Buffer`, `CursorSet`, `Viewport`, `HighlightSnapshot`, `EventCollector`.
- **LSP**: Full client with transport, connection pool, debouncing, document sync in `lsp/`. `LspBridge` connects editor events to LSP.
- **Config**: TOML-based, global at `~/.config/aurora/aurora.toml`, per-project at `.aurora.toml`.
- **Agent system**: `AgentLoop` orchestrates LLM + tool calls. Tools: read_file, write_file, search_files, list_directory, run_command, grep. Proposed changes require approval.
- **FreeLLMAPI sidecar**: Cloned at `sidecar/freellmapi`. `SidecarManager` handles lifecycle. `FreeLlmClient` talks to it via HTTP. 12+ cloud providers available.

## Conventions

- Tests are inline `#[cfg(test)] mod tests` in each crate
- Error types use `thiserror` with crate-specific error enums (`AiError`, `EditorError`, `ConnectionError`)
- Async runtime: `tokio` with `rt-multi-thread`
- Dependencies: `parking_lot` over `std::sync`, `dashmap` for concurrent maps
- All code must pass `cargo fmt --check` and `cargo clippy --workspace -- -D warnings`
