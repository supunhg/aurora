# Aurora Architecture

> **Codename:** `aurora`
> **Language:** Rust (stable)
> **Tagline:** The IDE that works the way you do. Fast, private, AI-native, and completely yours.

---

## 1. Design Philosophy

### Why Rust

| Concern | Rust Advantage |
|---------|----------------|
| **Latency** | No GC pauses, zero-cost abstractions, direct GPU access via wgpu |
| **Memory** | Fine-grained control; <150MB RSS target vs 1.8GB+ for Electron editors |
| **Startup** | Native binary, lazy init, <80ms cold start vs 3-8s for VS Code/JetBrains |
| **Safety** | Memory safety without GC; prevents whole classes of editor crashes |
| **Distribution** | Single static binary, no runtime dependencies, ~5MB stripped |

### Design Principles

| Principle | Enforcement |
|-----------|-------------|
| **Frame Budget** | 16ms target. UI drops non-critical work if exceeded. `F12` overlay shows render/LSP/AI latency. |
| **Lazy by Default** | LSP, tree-sitter grammars, AI workers, plugins initialize on demand. Cold startup < 80ms. |
| **Local-First AI, Cloud-Fallback** | Local models run on-device. Cloud routed through unified endpoint with encrypted key storage, fallback chain, per-key rate tracking. Zero telemetry. |
| **Sandboxed Extensibility** | Plugins run in WASI with capability declarations. Native FFI only for audited, perf-critical modules. |
| **Observable AI** | Every AI decision is transparent: which model, why it was chosen, fallback count, rate headroom. |
| **Privacy by Default** | Zero telemetry shipped. Local models only unless explicitly opted in per-project. |

### Market Positioning

Aurora occupies a unique quadrant that no current editor serves well:

```
                    High Performance (Native)
                           │
         Zed               │               Aurora ★
         (Rust, GPU)       │               (Local-first AI +
                           │                Observable routing +
         Helix             │                Privacy by default)
         (Terminal,        │
         Modal)            │
───────────────────────────┼───────────────────────────
                           │     AI Integration
                           │
         VS Code           │               Cursor
         Neovim + plugins  │               Windsurf
         JetBrains         │               (Electron, cloud AI)
                           │
                    Low Performance (Electron/JVM)
```

**Aurora's wedge:** Local-first AI with transparent multi-provider fallback, zero telemetry, and observable AI decisions — in a natively fast Rust shell. No competitor combines these three axes.

---

## 2. Competitive Landscape (2026)

### Key Competitors

| Editor | Foundation | AI Approach | Performance | Privacy | Ecosystem |
|--------|-----------|-------------|-------------|---------|-----------|
| **VS Code** | Electron/TS | Copilot (cloud-focused) | ~650MB idle, 3s startup | Telemetry by default | 50k+ extensions |
| **Cursor** | VS Code fork | Cloud agents, Composer | ~700MB idle, 3s startup | Privacy mode (opt-in) | VS Code compatible |
| **Windsurf** | VS Code fork | Cascade agent (cloud) | ~700MB idle, 3s startup | Opt-in controls | VS Code (Open VSX) |
| **Zed** | Rust+GPUI | Zeta local + cloud BYOK | ~180MB idle, 0.4s startup | BYOK, no telemetry | ~800 extensions |
| **JetBrains** | JVM | AI Assistant (cloud) | 2-4GB heap, 10-30s startup | Telemetry | Rich per-language |
| **Helix** | Rust (terminal) | None (external tooling) | Minimal | Terminal-native | Plugin system TBD |
| **Lapce** | Rust+Floem | None | Native | Open source | WASI plugins |
| **Aurora** | Rust+egui/wgpu | Local-first + smart fallback | <150MB target, <80ms startup | Zero telemetry | WASI plugin system |

### Market Gaps Aurora Fills

1. **Local-first AI with transparent cloud fallback**
   Zed has Zeta (edit prediction only). Cursor/Windsurf are cloud-first. No editor makes local inference the default with smart routing to cloud when needed.

2. **Observable AI decisions**
   Users get `X-Routed-Via`, fallback count, rate headroom, latency breakdown. No competitor exposes this.

3. **Vendor-agnostic multi-provider routing**
   `model: "auto"` with configurable fallback chain across local + any OpenAI-compatible provider. Zero lock-in.

4. **Privacy by default**
   Zero telemetry shipped. Local-only mode. Keys encrypted at rest. Cloud only with explicit per-project opt-in.

5. **Rust-native performance ceiling**
   Electron editors literally cannot match native latency and memory targets.

---

## 3. User Persona & Workflow

### Primary Persona

Solo/small-team developer working in Rust, TypeScript, Python, Go. Frustrated with VS Code/Cursor memory bloat, privacy concerns with cloud AI, tired of subscription fatigue. Values speed, privacy, customization.

### Day-in-the-Life Flow

```
1. Open terminal -> `aurora .` -> editor opens in <80ms
2. File tree appears, last session restored (tabs, cursor position, undo history)
3. Start typing -> inline completions appear from local model (<50ms)
4. Cursor moves -> new auto-completion requested, previous cancelled
5. Complex refactor -> open chat panel -> `model: "auto"` selects Groq/llama3
6. Run build -> integrated terminal shows output with ANSI parsing
7. Test fails -> AI suggests fix based on test output context
8. Need deeper context -> Agent mode makes multi-file edit with diff review
9. Commit -> status bar shows AI status: "groq/llama3 (fallback: 0)"
10. End session -> zero telemetry sent, keys stay encrypted on disk
```

---

## 4. System Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           UI THREAD (egui/wgpu)                         │
│  ┌──────────┐  ┌──────────────┐  ┌──────────┐  ┌───────────────────┐  │
│  │ Input    │-> │ State        │-> │ egui     │-> │ Frame Pacer       │  │
│  │ Router   │   │ Snapshot     │   │ Render   │   │ & Budget Guard    │  │
│  │ (keybind)│   │ (copy-on-write)│ │ (layout) │   │ (16ms target)    │  │
│  └──────────┘   └──────────────┘  └──────────┘  └───────────────────┘  │
└────────────────────────────────┬────────────────────────────────────────┘
                                 │ tokio::sync::mpsc / oneshot
                                 │ Arc<RwLock<StateSnapshot>>
┌────────────────────────────────▼────────────────────────────────────────┐
│                        WORKER POOL (Tokio Multi)                        │
│                                                                         │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐  ┌──────────────────┐  │
│  │ LSP Client │  │ AI Engine  │  │ FS/Git     │  │ PTY/Terminal     │  │
│  │ Manager    │  │ Context+   │  │ Watcher    │  │ + Process Mgr    │  │
│  └────────────┘  └────────────┘  └────────────┘  └──────────────────┘  │
│  ┌────────────┐  ┌────────────┐  ┌──────────────────────────────────┐  │
│  │ WASI       │  │ DAP Client │  │ Index/Symbol/Tag Cache           │  │
│  │ Plugin VM  │  │            │  │ (Memory-mapped, LRU evicted)     │  │
│  └────────────┘  └────────────┘  └──────────────────────────────────┘  │
│                                                                         │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │                    AI ROUTER (FreeLLMAPI-inspired)                │  │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────────────┐    │  │
│  │  │ Router   │ │ Rate     │ │ Health   │ │ Encrypted        │    │  │
│  │  │ Service  │ │ Ledger   │ │ Monitor  │ │ Key Store        │    │  │
│  │  └──────────┘ └──────────┘ └──────────┘ └──────────────────┘    │  │
│  │  ┌──────────────────────────────────────────────────────────┐   │  │
│  │  │ Provider Adapters (Local -> Groq -> Cerebras -> Gemini   │   │  │
│  │  │                   -> OpenAI -> Anthropic)                 │   │  │
│  │  └──────────────────────────────────────────────────────────┘   │  │
│  │  ┌──────────────────────────────────────────────────────────┐   │  │
│  │  │ Context Pruner   │ Analytics Logger   │ Sticky Sessions  │   │  │
│  │  └──────────────────────────────────────────────────────────┘   │  │
│  └──────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
```

### Threading Model

- **UI Thread (single):** Handles input dispatch, state diffing, `egui` layout, `wgpu` frame submission. Blocks only on VSync.
- **Worker Pool (tokio multi-threaded):** Dedicated task queues for LSP, AI, FS, PTY, DAP, WASI. Workers publish versioned state snapshots.
- **State Sync:** `Arc<RwLock<StateSnapshot>>` with copy-on-write. UI reads lock-free snapshot. No locks held during render.
- **Cancellation:** Every async task tracked via `JoinHandle`. Cancelled on cursor move, navigation, or explicit user action (debounced).

---

## 5. Core Subsystems

### 5.1 Configuration System

```
config/
├── mod.rs            # Config trait, loader, watcher
├── schema.rs         # TOML schema validation with serde
├── migration.rs      # Breaking change migration (CLI-driven)
├── default.toml      # Shipped defaults
└── types.rs          # All config structs
```

- **Format:** TOML, hot-reloadable (watched via `notify`).
- **Scope:** `~/.config/aurora/aurora.toml` (global) + `.aurora.toml` (per-project, overrides global).
- **Validation:** Schema checked at load. Invalid keys warned, not fatal. Migration path for breaking changes.
- **Sections:** `[theme]`, `[keybindings]`, `[ai.routing]`, `[ai.providers]`, `[editor]`, `[terminal]`, `[lsp]`, `[plugins]`.

### 5.2 Keybinding System

- **Model:** Chord-based with optional Vim emulation mode.
- **Config:** User-defined remappings in `aurora.toml`. Layered (global + per-mode).
- **Dispatch:** Input events -> key sequence buffer -> match against active keymap -> emit `EditorAction`.
- **Multi-cursor aware:** Actions that support multi-cursor iterate over all cursors.

### 5.3 Editor Buffer & Text Model (Implemented)

```
editor/src/
├── buffer.rs        # Ropey-backed text buffer + undo/redo
├── cursor.rs        # Multi-cursor with selection (anchor/head)
├── viewport.rs      # Virtual scrolling, smooth scroll
├── syntax.rs        # Tree-sitter + rule-based highlighting
└── error.rs         # Typed editor errors
```

- **Data Structure:** `ropey`-backed Rope. O(log n) insert/delete at any position.
- **Hot Path Rules:**
  - Zero `String` allocations during typing. Use `&str`, `Arc<str>`, `bumpalo`.
  - Line/char indexing via cached offsets. O(1) viewport translation.
  - Undo stack stores `Delta` structs with memory pooling. Max depth configurable (default: 10k).
- **Virtual Scrolling:** Renders only visible + 2x viewport buffer. Line height cache + font metrics prefetch.
- **Multi-cursor:** Sorted cursor set. Operations process in reverse order to maintain positions. Each cursor supports independent selection (anchor/head model).

### 5.4 Syntax & Semantic Highlighting

- **Parser:** `tree-sitter` with incremental parsing. Grammars vendored, AOT-compiled.
- **Pipeline:** Parse -> AST injection queries -> Scope map -> Batched wgpu draw calls.
- **Latency:** <2ms for 10k LOC on modern CPU. Parsing off-UI-thread, snapshotted.
- **Fallback:** Rule-based `ScopeClassifier` when tree-sitter not available for a language.

### 5.5 LSP Client

```
lsp/
├── mod.rs           # LspClient, ConnectionPool
├── transport.rs     # JSON-RPC over stdio (tokio process)
├── router.rs        # Request routing (debounce, dedupe, cancel)
├── handlers.rs      # Response handlers -> state updates
└── capabilities.rs  # Server capabilities negotiation
```

- **Protocol:** LSP via `tower-lsp` or custom JSON-RPC over `tokio` child process stdio.
- **Connection Pool:** One server per language/workspace root. Restart on crash.
- **Request Routing:**
  - Debounced: Hover, completion, signature help (delay on keystroke).
  - Immediate: Diagnostics, didChange.
  - Cancellable: In-flight requests cancelled on new cursor position.
- **State Sync:** Diagnostics -> gutter icons + problem panel. Completions -> popup widget. Hover -> tooltip.

### 5.6 UI & Rendering

- **Framework:** `egui` (immediate mode) + `wgpu` (GPU backend).
- **Optimizations:**
  - Frame pacing: `wgpu` swap chain VSync + manual sleep if early.
  - Render batching: Merge identical glyph/text draws. Cache `egui::Mesh`.
  - Fallback: `wgpu` -> CPU rasterizer on init failure (configurable).
- **Theme System:** Hot-reloadable TOML. CSS-like variables compiled to `egui` style structs. Dark/light toggle.

### 5.7 Workspace & Project Management

```
workspace/
├── mod.rs           # Workspace (root dirs, open files, project state)
├── watcher.rs       # notify-based file watcher (debounced)
├── file_tree.rs     # Project panel model (lazy-loading)
├── buffer_tabs.rs   # Open buffer tab management
├── recent.rs        # Recent projects, session restore
└── settings.rs      # Per-project .aurora.toml loader
```

- **File Watcher:** `notify` (debounced, cross-platform). Triggers re-index, git status refresh, config reload.
- **Buffer Tabs:** LRU cache of open buffers. Persisted to disk for session restore.
- **Session Restore:** On open, restores last project's tabs, cursor positions, undo stacks from memory-mapped cache.

### 5.8 Git Integration

- **Library:** `gix` (pure Rust, no git binary required).
- **Status Gutter:** Color-coded indicators in editor gutter (green=added, yellow=modified, red=deleted).
- **AI Context:** Current diff (unstaged changes) included in AI context for relevant prompts.

### 5.9 Search & Indexing

- **Fuzzy Finder:** Fast fuzzy matching for files, symbols, commands. Bind to `ctrl+p` / `cmd+p`.
- **Project Search:** Async text search with regex support. Results streamed to panel.
- **Symbol Index:** Background extraction via tree-sitter (local) + LSP (semantic). Stored in sled or SQLite. <5% CPU during indexing.

### 5.10 Terminal & Process Management

- **Parser:** `vte` (ANSI escape codes). Supports full color, cursor positioning, bracketed paste.
- **Renderer:** GPU-textured glyph atlas. Zero allocation during output burst.
- **Process Manager:** Async PTY spawner per session. Job control (SIGINT/SIGTERM). Exit code capture.

### 5.11 Debug Adapter Protocol (DAP)

- **Protocol:** Async client over `debug-adapter-protocol` spec. Communicates over stdio to debug adapter.
- **UI Sync:** Breakpoint gutter (click to toggle), stack trace viewer, variable inspector (lazy tree), REPL console.
- **Process Isolation:** Each debug session runs in dedicated worker. Crash-safe.

### 5.12 Plugin System

- **Primary Runtime:** `wasmtime` (WASI) -- sandboxed, capability-based, cross-platform.
- **Manifest:** `plugin.toml` declares name, version, ABI, capabilities, entrypoint, resource limits.
- **AI Plugin Hooks:**
  - `register_provider`: Add new AI backends.
  - `context_hook`: Pre-process/post-process AI context.
  - `routing_hook`: Override auto-routing scoring.
- **Security:** Plugins cannot access encrypted key store directly. Must request via router service with scoped, user-approved permissions.

---

## 6. AI Architecture (FreeLLMAPI-Inspired)

### 6.1 Unified AI Endpoint

Single OpenAI-compatible interface that aggregates:
- **Local models** via `llama.cpp` FFI (`llama-cpp-rs` or `candle`)
- **Free-tier cloud** (Groq, Cerebras, Google Gemini, etc.) via encrypted key routing
- **Paid cloud** (OpenAI, Anthropic) with explicit opt-in per project

### 6.2 Core Concepts

| Concept | Implementation |
|---------|---------------|
| **Unified endpoint** | Internal `route()` accepts `model: "auto"` or explicit model ID. Router selects backend. |
| **Fallback chain** | Configurable priority list. Auto-skip on 429/5xx/timeout. Max 20 attempts. |
| **Per-key rate tracking** | In-memory DashMap + SQLite ledger. RPM/RPD/TPM/TPD per `(provider, model, key)`. Cooldown on rate limit. |
| **Sticky sessions** | Conversation ID -> model affinity for 30min. Prevents context-switch artifacts. |
| **Encrypted key storage** | AES-256-GCM via `ring`. Keys in `~/.aurora/keys.db`. Decrypted in-memory only, zeroized on drop. |
| **Health checks** | Background probes every 60s. Keys marked `Healthy | RateLimited | Invalid | Error`. |
| **"auto" virtual model** | Context-aware scoring: prefers local -> fast free-tier -> high-quality free-tier -> paid fallback. |
| **Analytics** | Per-request logging: latency, tokens, provider, fallback attempts. Exportable JSONL. |

### 6.3 Routing Architecture

```rust
// ai/src/router/service.rs
pub struct AIRouter {
    fallback_chain: Vec<ModelPriority>,
    rate_ledger: Arc<RateLimitLedger>,
    health_checker: Arc<HealthMonitor>,
    key_store: Arc<EncryptedKeyStore>,
    sticky_sessions: Arc<StickySessionCache>,
    analytics: Arc<AIAnalytics>,
    context_pruner: Arc<ContextPruner>,
}

impl AIRouter {
    /// Route an AI request through the fallback chain.
    /// Returns the first successful response or aggregates all errors.
    pub async fn route(&self, req: AIRequest) -> Result<AIResponse, RouterError> {
        // 1. Resolve target model: explicit > sticky > auto-routing
        // 2. Filter candidates by health status + rate limits
        // 3. Score remaining candidates using auto-select heuristics
        // 4. Attempt each candidate in score order with fallback retry
        // 5. On success, update sticky session + rate ledger + analytics
        // 6. On all fail, return composite error
    }
}
```

### 6.4 Provider Adapter Pattern

```rust
// ai/src/providers/trait.rs
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    fn provider_id(&self) -> ProviderId;
    fn supports_streaming(&self) -> bool;
    fn supports_tool_calling(&self) -> bool;
    fn default_priority(&self) -> u8;

    async fn chat_completion(
        &self,
        req: OpenAICompatRequest,
        key: &DecryptedApiKey,
    ) -> Result<OpenAICompatResponse, ProviderError>;

    async fn stream_chat_completion(
        &self,
        req: OpenAICompatRequest,
        key: &DecryptedApiKey,
        tx: mpsc::Sender<StreamChunk>,
    ) -> Result<(), ProviderError>;
}
```

**Built-in providers:**
- `LocalLlamaProvider` -- `llama-cpp-rs` FFI, multi-quantization
- `GroqProvider` -- reqwest to `api.groq.com`
- `CerebrasProvider` -- reqwest to `api.cerebras.ai`
- `GeminiProvider` -- reqwest to `generativelanguage.googleapis.com`
- `OpenAIProvider` -- reqwest to `api.openai.com`
- `AnthropicProvider` -- reqwest to `api.anthropic.com`
- `OllamaProvider` -- local HTTP to `localhost:11434`

### 6.5 Context-Aware "Auto" Routing

When `model: "auto"` is specified:

```rust
pub fn score_candidate(ctx: &AIContext, candidate: &ModelCandidate, sticky: Option<&str>) -> i32 {
    let mut score = 0i32;

    // Local model bonus: <4k tokens AND latency-sensitive
    if candidate.is_local && ctx.token_count < 4096 && ctx.latency_budget_ms > 200 {
        score += 100;
    }

    // Fast provider bonus (Groq, Cerebras): inline completions
    if ctx.request_type == InlineCompletion && candidate.p95_latency_ms < 200 {
        score += 50;
    }

    // Quality bonus: chat, refactor, agent
    if matches!(ctx.request_type, Chat | Refactor | Agent) {
        score += candidate.quality_score() as i32;
    }

    // Rate limit penalty
    score -= candidate.rate_headroom_penalty();

    // Sticky session affinity
    if let Some(session) = sticky {
        if candidate.model_id == session {
            score += 200;
        }
    }

    score
}
```

### 6.6 AI Privacy Controls

| Feature | Implementation |
|---------|---------------|
| **Local-only toggle** | Disables all cloud routing. Only local models used. |
| **Cloud opt-in** | Per-project setting in `.aurora.toml`. Wiped on disable. |
| **Encrypted keys** | AES-256-GCM at rest. OS keyring (`keyring-rs`) as alternative. |
| **Audit log** | `~/.aurora/ai-audit.log` -- user-readable, never auto-uploaded. |
| **Request transparency** | Status bar: `AI: groq/llama3 (fallback: 2)`. `X-Routed-Via` in footer. |
| **Data retention** | Cloud providers instructed `'do not train'`. User can delete audit log. |
| **Zero telemetry** | No analytics, crash reports, or usage tracking shipped by default. |

---

## 7. Concurrency & Data Flow

### 7.1 Message Types

```rust
// Commands from UI to workers
pub enum EditorCommand {
    OpenFile(PathBuf),
    SaveFile,
    InsertText { text: String, position: usize },
    DeleteRange { start: usize, end: usize },
    CursorMove(CursorMove),
    Scroll(ScrollDirection),
    AIRequest(AIRequest),
    CancelAIRequest(RequestId),
    RunCommand(String),
    ToggleBreakpoint(usize),
}

// Events from workers to UI
pub enum EditorEvent {
    BufferModified { version: u64, delta: Delta },
    HighlightsUpdated(Arc<Vec<HighlightRange>>),
    AISuggestion { request_id: RequestId, completion: InlineCompletion },
    AIStreamChunk { request_id: RequestId, chunk: StreamChunk },
    LSPDiagnostics { path: PathBuf, diagnostics: Vec<Diagnostic> },
    FileChanged(PathBuf),
    GitStatusChanged,
    TerminalOutput { session_id: u32, text: String },
}
```

### 7.2 Flow Control

- **UI -> Worker:** `tokio::mpsc` channels. One sender per command type. Capacity capped at 256. Backpressure: non-critical messages dropped if full.
- **Worker -> UI:** Versioned state snapshots via `Arc<RwLock<StateSnapshot>>`. UI reads snapshot atomically at start of frame.
- **Cancellation:** In-flight AI/LSP requests cancelled via `CancellationToken` on cursor move or navigation.
- **Debouncing:** LSP hover/diagnostics debounced at 150ms. AI completions debounced at 200ms after last keystroke.

---

## 8. Performance Targets

| Metric | Target | How |
|--------|--------|-----|
| Cold startup | <80ms | Lazy init, `mimalloc`, stripped binary |
| Warm startup (session restore) | <30ms | Memory-mapped state cache |
| Frame budget | 16ms (60fps) | egui response caching, render batching |
| Memory RSS | <150MB @ 50k LOC | LRU caches, zero-alloc hot paths |
| Edit latency | <8ms P95 | Ropey ops, pooled undo, off-thread tree-sitter |
| LSP completion | <50ms P95 | Debounced, cancelled on new keystroke |
| AI inline completion | <50ms P95 | Local-first routing, minimal context, cancellation |
| AI chat TTFT | <500ms P95 | Streaming, fast-provider priority |
| Router decision | <5ms | In-memory rate ledger, pre-filtered candidates |
| Tree-sitter parse | <2ms @ 10k LOC | Incremental parsing, off-thread |

### Optimization Checklist

- [ ] Replace `String` with `Arc<str>`/`Bytes`/`bumpalo` in hot paths
- [ ] String interning for LSP symbols, theme keys, file paths, provider IDs
- [ ] Frame pacing with `wgpu` swap chain + `std::thread::sleep` fallback
- [ ] `tracing` spans for LSP/AI/render/FS/router. Export to pprof/flamegraph
- [ ] `criterion` benchmarks for buffer ops, tree-sitter, context prune, router select
- [ ] Zeroize sensitive data: API keys, decrypted tokens, auth headers
- [ ] Memory pooling for undo deltas, terminal scrollback, highlight ranges

---

## 9. Security & Privacy

- **Zero Telemetry:** No analytics, crash reports, or usage tracking shipped by default. Explicit opt-in CLI flag only.
- **Encrypted Key Storage:** AES-256-GCM at rest. Keys decrypted in-memory during request, zeroized after. OS keyring support.
- **Sandboxed Plugins:** WASI caps enforce filesystem/network limits. Native FFI requires audit + signature verification.
- **AI Privacy:**
  - Local models: fully on-device.
  - Cloud: per-project opt-in. Keys encrypted. Audit log, no auto-upload.
  - `X-Routed-Via` header visible in UI.
- **Process Isolation:** PTY, DAP, LSP, AI router in separate workers. Worker crash doesn't take down UI.
- **Config Safety:** TOML validated at load. Invalid keys ignored. Migration CLI for breaking changes.

---

## 10. Testing Strategy

### 10.1 Test Levels

| Level | Scope | Tools |
|-------|-------|-------|
| **Unit** | Individual functions, edge cases | `cargo test` |
| **Integration** | Cross-crate workflows, router fallback chain | `cargo test -- --ignored` (integration) |
| **Fuzz** | Buffer ops, tree-sitter, WASI, router | `cargo fuzz` |
| **Benchmark** | Startup, edit latency, AI P95, router decision | `criterion` |
| **Smoke** | Self-test binary, startup -> edit -> AI -> exit | `aurora --self-test` |

### 10.2 Benchmark Targets

```rust
// benchmarks/ directory (criterion)
cargo bench -- buffer::insert_10k_lines
cargo bench -- treesitter::parse_50k_loc
cargo bench -- ai::context_prune_8k_tokens
cargo bench -- ai::router_select_auto_model
cargo bench -- ai::provider_adapter_groq_latency
cargo bench -- ui::frame_render_60fps
cargo bench -- startup::cold_vs_warm
```

### 10.3 Fuzz Targets

```rust
// fuzz/ directory (cargo-fuzz)
fuzz_targets/
├── buffer_ops.rs        // Random insert/delete/undo/redo sequences
├── treesitter_parse.rs  // Random byte sequences as source code
├── router_decision.rs   // Random context + candidate sets
└── wasi_plugin.rs       // Malformed WASM modules
```

---

## 11. Project Structure

```
aurora/
├── Cargo.toml                  # Workspace root
├── rust-toolchain.toml         # Stable Rust, fmt + clippy
├── ARCHITECTURE.md             # This file
├── README.md
├── .github/workflows/
│   ├── ci.yml                  # Lint, test, build, bench
│   └── release.yml             # Cross-platform binary builds
│
├── aurora-bin/                 # Binary entry point
├── aurora-core/                # Shared types, versioning, utilities
├── config/                     # Hot-reloadable configuration
├── editor/                     # Core text editing
├── ui/                         # Rendering + panels
├── lsp/                        # Language Server Protocol
├── terminal/                   # PTY terminal
├── debug/                      # Debug Adapter Protocol
├── workspace/                  # Project management
├── index/                      # Search & symbol indexing
├── ai/                         # AI routing & providers
│   ├── src/
│   │   ├── router/             # AIRouter, auto_select, fallback
│   │   ├── providers/          # ProviderAdapter + implementations
│   │   ├── ratelimit/          # RateLimitLedger
│   │   ├── keystore/           # EncryptedKeyStore
│   │   ├── health/             # HealthMonitor
│   │   ├── context/            # AIContext pruning
│   │   └── analytics/          # JSONL analytics logger
├── plugin/                     # WASI plugin system
├── profiler/                   # tracing, pprof, F12 overlay
├── utils/                      # interning, pool allocators, zeroize
├── benches/                    # criterion benchmarks
└── fuzz/                       # cargo-fuzz targets
```

---

## 12. Development Roadmap

### Phase 0 -- Foundation (Current state + polish)

| Task | Dependencies |
|------|-------------|
| Merge `aurora-core` into shared types crate | None |
| Create `config` crate with TOML loading + hot-reload | None |
| Wire dependency injection (Editor <-> AI <-> UI in main.rs) | config |
| Set up CLI argument parsing (clap) | None |
| Seed `criterion` benchmarks for buffer + startup | None |

### Phase 1 -- AI Plumbing (NOW)

| Task | Dependencies |
|------|-------------|
| `EncryptedKeyStore` (ring + rusqlite + zeroize) | config |
| `RateLimitLedger` (DashMap + SQLite sliding window) | config |
| `HealthMonitor` (background provider pings) | config |
| `GroqProvider` (reqwest -> api.groq.com) | keystore |
| `ContextPruner` (token budgeting, file chunking, git diff) | workspace/git |
| Context-aware `AutoRouter` (scoring heuristics) | rate ledger, health |
| `tracing` spans for AI pipeline | None |
| `OllamaProvider` (localhost:11434 for local inference) | None |

### Phase 2 -- Editor Completion

| Task | Dependencies |
|------|-------------|
| LSP client (connection pool, debounced routing) | config, editor |
| File watcher (`notify`, deferred debounced) | None |
| Git integration (`gix`: status gutter, blame, diff) | workspace |
| File tree UI panel | ui, workspace |
| Buffer tab management + session restore | workspace |
| Project-wide search (fuzzy files + text) | index |
| Theme system (TOML hot-reload -> egui styles) | config, ui |

### Phase 3 -- Terminal & Debug

| Task | Dependencies |
|------|-------------|
| PTY spawner + vte parser + GPU terminal renderer | ui |
| DAP client + debug UI | config, ui |

### Phase 4 -- AI UX

| Task | Dependencies |
|------|-------------|
| Inline ghost text completions | ai, editor |
| Chat panel (streaming, model selector) | ai, ui |
| Agent mode (autonomous multi-file edits) | ai, workspace, lsp |
| `F12` performance overlay | profiler, ui |

### Phase 5 -- Ecosystem & Hardening

| Task | Dependencies |
|------|-------------|
| WASI plugin runtime + manifest loader | plugin |
| AI plugin hooks | plugin, ai |
| Fuzz testing + cross-platform CI | All |

---

## 13. Dependencies

```toml
[dependencies]
# UI / Rendering
egui = "0.28"
wgpu = "0.20"

# Text / Syntax
ropey = "1.6"
tree-sitter = "0.23"
tree-sitter-highlight = "0.23"

# Async / Runtime
tokio = { version = "1", features = ["full", "tracing", "rt-multi-thread"] }
tokio-stream = "0.1"
async-trait = "0.1"
futures = "0.3"

# LSP
lsp-types = "0.95"
tower-lsp = "0.20"

# AI
reqwest = { version = "0.12", features = ["stream", "json", "rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4", "serde"] }

# Local Models
llama-cpp-rs = { version = "0.1", optional = true }

# Encryption / Security
ring = "0.17"
zeroize = { version = "1", features = ["derive"] }
keyring = "2.3"

# Storage
rusqlite = { version = "0.31", features = ["bundled"] }
sled = "0.34"

# Utilities
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["json", "env-filter"] }
mimalloc = "0.1"
bumpalo = "3.16"
notify = "6.0"
gix = "0.66"
vte = "0.14"
parking_lot = "0.12"
dashmap = "5.5"
slab = "0.4"
clap = { version = "4", features = ["derive"] }
thiserror = "1"
anyhow = "1"
chrono = "0.4"

# DAP
debug-adapter-protocol = "0.1"

# Terminal
portable-pty = "0.8"

# Plugin System
wasmtime = "24"
wasi-common = "24"

# Tools
nucleo = "0.4"              # Fuzzy matching
grep = "0.3"                # Fast project search

[dev-dependencies]
criterion = "0.5"
tempfile = "3"
proptest = "1"

[features]
default = ["local-ai"]
local-ai = ["llama-cpp-rs"]
gui = ["egui", "wgpu"]

[profile.release]
lto = "fat"
codegen-units = 1
strip = "symbols"
opt-level = 3
```

---

## Appendix A: FreeLLMAPI Compatibility

- [ ] Implement OpenAI-compatible `/v1/chat/completions` endpoint
- [ ] Support `model: "auto"` as virtual model for router-selected backend
- [ ] Return `X-Routed-Via` and `X-Fallback-Attempts` headers
- [ ] Implement rate limit headers
- [ ] AES-256-GCM for key encryption
- [ ] Export analytics in JSONL format
- [ ] Document fallback chain configuration

---

## Appendix B: Key Data Structures

```rust
// Versioned state snapshot for UI thread
pub struct StateSnapshot {
    pub version: u64,
    pub buffer: Arc<ropey::Rope>,
    pub cursors: CursorSet,
    pub highlights: Arc<Vec<HighlightRange>>,
    pub ai_completions: Vec<InlineCompletion>,
    pub ai_status: AIStatus,
    pub terminal_sessions: Arc<Vec<TerminalSession>>,
    pub file_tree: Arc<FileTree>,
    pub problems: Arc<Vec<Problem>>,
    pub debug_state: Option<DebugSessionStatus>,
    pub workspace_status: WorkspaceStatus,
}

// AI context payload (pruned for token budget)
pub struct AIContext {
    pub file_chunks: Vec<ContextChunk>,
    pub symbols: Vec<SymbolRef>,
    pub git_diff: DiffPatch,
    pub cursor_history: Vec<CursorPos>,
    pub terminal_output: Option<String>,
    pub problems: Vec<Problem>,
    pub token_budget: usize,
    pub request_type: RequestType,
    pub latency_budget_ms: u64,
    pub conversation_id: Option<Uuid>,
    pub cancellation_token: CancellationToken,
}

// Router decision metadata
pub struct RoutingMetadata {
    pub routed_via: String,
    pub fallback_attempts: u8,
    pub decision_reason: String,
    pub latency_ms: u64,
    pub tokens_in: usize,
    pub tokens_out: usize,
    pub rate_headroom: f64,
}

// Plugin manifest
pub struct PluginManifest {
    pub name: String,
    pub version: semver::Version,
    pub abi_version: u32,
    pub capabilities: PluginCaps,
    pub entrypoint: String,
    pub resources: ResourceLimits,
}
```

---

*This document is the single source of truth for Aurora's engineering decisions, performance targets, and system boundaries. The local-first AI routing fabric is Aurora's core differentiator: it enables privacy, vendor independence, and observability that no other editor provides.*
