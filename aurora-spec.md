# Aurora IDE — Product Specification

> **Compiled:** June 2, 2026
> **Based on:** Codebase analysis (all source files, Cargo.toml, ARCHITECTURE.md, ROADMAP.md, AGENTS.md) + user interviews
> **Status:** Living document — updated as decisions evolve

---

## 1. Product Vision

### 1.1 Elevator Pitch

Aurora is a Rust-native code editor that **surpasses Zed on every dimension**: faster startup (<80ms), lower memory (<120MB), richer AI integration (local-first + multi-provider routing), competitive visual polish, and stronger privacy guarantees (zero telemetry, encrypted keys). Built for developers who want a fast, private, AI-native editor without vendor lock-in or subscription fees.

### 1.2 Tagline

> *"The IDE that works the way you do. Fast, private, AI-native, completely yours."*

### 1.3 Core Differentiators

| Differentiator | Target | Zed (approx.) | Aurora Wins Because |
|---------------|--------|---------------|---------------------|
| **Startup Speed** | <80ms cold | ~400ms | Lazy init, stripped binary, `mimalloc` |
| **Memory Footprint** | <120MB idle | ~180MB | Zero-alloc hot paths, LRU caches, string interning |
| **Frame Budget** | 16ms (60fps) | 16ms | Render batching, egui response caching |
| **AI Integration** | Local-first + smart fallback | Zeta (edit prediction) + BYOK | Multi-provider routing, observable decisions, context-aware "auto" model |
| **Privacy** | Zero telemetry, encrypted keys | BYOK, no telemetry | Local models default, per-project cloud opt-in, AES-256-GCM keystore |
| **Vendor Independence** | No lock-in | Moderate | 6+ native Rust providers (Ollama, Groq, OpenAI, Anthropic, Gemini, Cerebras) |
| **Visual Polish** | Zed-level | Excellent | Custom egui styling, smooth animations, Aurora Dark/Light themes |
| **Distribution** | Single binary (~5MB) | ~20MB | Pure Rust, no Node.js or Electron |

### 1.4 Market Position

```
                    High Performance (Native)
                           │
         Zed               │               Aurora ★
         (Rust, GPUI)      │               (Faster + Local AI +
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

Aurora's wedge: **local-first AI with transparent multi-provider routing + fastest performance + zero telemetry + full visual polish**. No competitor combines all four.

---

## 2. Interview Decisions (Key Takeaways)

These decisions were made during the user interview process and form the foundation of this specification:

| Decision | Choice | Rationale |
|----------|--------|-----------|
| **Vision priority** | All dimensions equally important | Aurora must beat Zed on ALL axes — speed, AI, polish, features |
| **Git workflow** | Atomic commits, developer's judgment | Clean history, each commit compiles + tests pass |
| **Open source** | MIT License, fully open | Build community from day one |
| **AI architecture** | Native Rust only (no Node.js sidecar) | Single binary distribution, fully controlled routing, better perf |
| **UI strategy** | Double down on egui | Push egui to its absolute limits with custom styling and animations |
| **Agent mode** | Propose then act | AI proposes changes in diff view, user reviews before applying |
| **Rust version** | Stable latest | Get newest features, no backward compat burden |
| **AI providers** | All major providers | Build Ollama, Groq, OpenAI, Anthropic, Gemini, Cerebras |
| **Tech debt** | Clean as we go | Fix issues when touching relevant code, no dedicated cleanup sprints |
| **Release cadence** | Semantic versioning | v0.1.0, v0.2.0, etc., tagged at meaningful milestones |

---

## 3. Target Audience

### 3.1 Primary Persona

**Polyglot developers** working across Rust, TypeScript, Python, and Go. Currently using VS Code (too slow, too much memory, telemetry concerns) or Zed (fast but AI story still maturing, GPUI learning curve). Frustrated with Electron editor bloat, privacy concerns around cloud AI, and subscription fatigue.

**Demographics:**
- Independent developers and small teams (< 20 people)
- Linux or macOS primary OS
- Open-source contributors and commercially employed engineers
- Ages 22–45
- Care deeply about tool quality, performance, and privacy
- Willing to try new tools that offer clear advantages

### 3.2 Day-in-the-Life Flow

```
1. Terminal → `aurora .` → editor opens in <80ms
2. File tree appears, last session restored (tabs, cursor positions, undo history)
3. Start typing → ghost text inline completions from local Ollama model (<50ms P95)
4. Cursor moves → previous completion cancelled, new one starts (debounced 200ms)
5. Complex refactor → open chat panel → native Rust AI router selects best model
6. Run build → integrated terminal with ANSI parsing
7. Test fails → AI suggests fix based on test output + file context
8. Need deeper context → Agent mode proposes multi-file edit → user reviews diff → approves
9. Commit → status bar shows AI status + provider info ("groq/llama3 (fallback: 0)")
10. End session → zero telemetry sent, keys stay encrypted on disk
```

---

## 4. Architecture Decisions

### 4.1 AI Routing: Native Rust (No Node.js Sidecar)

**Decision:** Replace the FreeLLMAPI Node.js sidecar with native Rust providers. This is a phased migration — the sidecar code exists and works, but new development focuses on native implementations.

**Rationale:**
- Single binary distribution (no Node.js runtime dependency)
- Fully controlled fallback chain logic in Rust
- Better performance (no HTTP hop to local sidecar)
- Cleaner architecture (all AI logic in one crate)

**Architecture:**
```rust
// ai/src/router — The native router
pub struct NativeRouter {
    provider_registry: Arc<ProviderRegistry>, // All registered providers
    rate_ledger: Arc<RateLimitLedger>,        // Per-key rate tracking
    health_monitor: Arc<HealthMonitor>,       // Background health checks
    key_store: Arc<KeyStore>,                 // Ephemeral or encrypted
    context_pruner: Arc<ContextPruner>,       // Token budget management
    sticky_sessions: StickySessionCache,      // Model affinity per conversation
    analytics: Arc<AIAnalytics>,              // Request logging
}
```

**Provider implementations (all native Rust, all OpenAI-compatible):**

| Provider | HTTP Endpoint | Auth | Status | Priority |
|----------|-------------|------|--------|----------|
| **Ollama** | `localhost:11434/v1/chat/completions` | None | ⬜ To build | Highest (local-first) |
| **Groq** | `api.groq.com/openai/v1/chat/completions` | Bearer token | ✅ Built (needs HTTP wiring) | High (free, fast) |
| **OpenAI** | `api.openai.com/v1/chat/completions` | Bearer token | ⬜ To build | Medium (paid) |
| **Anthropic** | `api.anthropic.com/v1/messages` | x-api-key | ⬜ To build | Medium (paid) |
| **Gemini** | `generativelanguage.googleapis.com/v1beta/models/...` | API key | ⬜ To build | Medium (free tier) |
| **Cerebras** | `api.cerebras.ai/v1/chat/completions` | Bearer token | ⬜ To build | Medium (free tier) |
| **Local (llama-cpp-rs)** | FFI (no HTTP) | None | ⬜ To build | Highest (fully offline) |

### 4.2 UI Framework: egui (Push to the Limit)

**Decision:** Double down on egui for the MVP and beyond. Push it to its visual limits with custom styling, smooth animations, and custom widget painting. No migration plan currently.

**Polishing strategy:**
- Custom `egui::Style` with Zed-like minimal aesthetic (already started in `ui/src/theme.rs`)
- Smooth scrolling via viewport fractional offset + `request_repaint_after()`
- Custom painted scrollbars (thin, semi-transparent, appear on hover)
- Tab bar with smooth transitions
- Minimap (post-MVP aspirational) rendered as a custom egui widget
- Aurora Dark/Light themes compiled from TOML config into `egui::Style` structs

**Why egui can work:**
- wgpu backend provides GPU-accelerated rendering
- Custom `Frame` painting gives fine-grained control
- The `egui::PaintCallback` mechanism allows custom GPU drawing
- Many successful editors use immediate-mode GUI patterns
- Community is active and improving performance continuously

### 4.3 Keybinding Model

**Decision:** Standard chord-based first (Ctrl+C/V/S/A/Z/Y/O, etc.). Vim/Emacs modes are future considerations, not MVP blockers. Keybindings configurable in `aurora.toml`.

### 4.4 Agent Mode: Propose Then Act

**Decision:** The AI agent proposes changes (shows a diff view), and the user reviews and approves before they are applied. This is the default mode. A "trusted mode" setting could reduce friction for power users later.

**Agent flow:**
```
User: "Refactor this function to use error handling"
  ↓
Agent: Reads relevant files, understands context
  ↓
Agent: Proposes changes (shown in diff panel)
  ↓
User: Reviews diff, can accept/reject/modify
  ↓
Agent: Applies approved changes, runs tests, reports results
```

### 4.5 Distribution Model

**Decision:** Single static binary (~5MB stripped). MIT license. GitHub releases with semantic versioning.

**Platforms (MVP):** Linux (x86_64, aarch64) + macOS (x86_64, aarch64)
**Post-MVP:** Windows (x86_64)

---

## 5. Feature Roadmap (Prioritized)

The project already has a solid skeleton. Here's what needs building, organized by priority:

### 5.1 Phase 1: AI Native Routing + Real Providers (Immediate)

This is the highest-impact work. Replace mock/sidecar AI with real native providers.

| Task | Dependencies | Est. Effort |
|------|-------------|-------------|
| **OllamaProvider** — HTTP client to localhost:11434 | `ai/src/providers/` | 2-3 hours |
| **GroqProvider** — Wire real HTTP (already partially built) | `reqwest`, API key | 1-2 hours |
| **OpenAIProvider** — OpenAI-compatible HTTP client | `ai/src/providers/` | 2 hours |
| **AnthropicProvider** — Anthropic Messages API | `ai/src/providers/` | 3 hours |
| **GeminiProvider** — Google Gemini API | `ai/src/providers/` | 2 hours |
| **CerebrasProvider** — Cerebras API | `ai/src/providers/` | 2 hours |
| **NativeRouter** — Wrap providers, handle auto-routing | All providers | 4 hours |
| **Context-aware "auto" model selection** | NativeRouter | 3 hours |
| **Remove sidecar dependency** from self-test + UI | NativeRouter | 1 hour |
| **AnalyticsLogger** — JSONL request logging | NativeRouter | 2 hours |
| **HealthMonitor** — Wire actual provider health checks | Router | 1 hour |

**Total estimated effort: ~22 hours** (spread across sessions)

### 5.2 Phase 2: Visual Polish + Theme System

| Task | Dependencies | Est. Effort |
|------|-------------|-------------|
| **Smooth scrolling** in viewport | `editor/src/viewport.rs` | 3 hours |
| **Custom scrollbar** widget (thin, hover-reveal) | `ui/src/` | 3 hours |
| **Aurora Light theme** implementation | `ui/src/theme.rs` | 2 hours |
| **Theme hot-reload** from TOML config | `config/` + `ui/` | 4 hours |
| **Animations** (tab switch, panel reveal, cursor blink) | `ui/src/app.rs` | 4 hours |
| **Icon set** (file type icons, gutter indicators) | `ui/src/` | 3 hours |
| **Editor font rendering** (JetBrains Mono + Inter) | `ui/` | 2 hours |

**Total estimated effort: ~21 hours**

### 5.3 Phase 3: LSP Productionization

The LSP client is surprisingly complete already. Need to wire it to the UI.

| Task | Dependencies | Est. Effort |
|------|-------------|-------------|
| **Wire diagnostics** from LSP to editor gutter | `lsp/` + `ui/` | 4 hours |
| **Completion popup** widget | `ui/src/app.rs` | 5 hours |
| **Hover tooltip** display | `ui/src/app.rs` | 3 hours |
| **Go-to-definition** navigate | `editor/` + `ui/` | 3 hours |
| **Find references** panel | `lsp/` + `ui/` | 4 hours |
| **Signature help** popup | `lsp/` + `ui/` | 4 hours |
| **Code actions** (lightbulb) | `lsp/` + `ui/` | 5 hours |

**Total estimated effort: ~28 hours**

### 5.4 Phase 4: File Management + Sessions

| Task | Dependencies | Est. Effort |
|------|-------------|-------------|
| **File watcher** (notify → debounced events) | `config/` (already depends on notify) | 3 hours |
| **Git status gutter** (gix: modified/added/deleted indicators) | `editor/` + `ui/` | 5 hours |
| **Session restore** (persist open tabs, cursor positions) | `editor/` + `config/` | 6 hours |
| **Fuzzy file finder** (Ctrl+P, nucleo) | `ui/` | 5 hours |
| **Project-wide text search** (ripgrep integration) | `ui/` + `editor/` | 6 hours |
| **Integrated terminal** (PTY spawner + vte parser) | `terminal/` crate | 10 hours |

**Total estimated effort: ~35 hours**

### 5.5 Phase 5: AI UX + Agent Mode

| Task | Dependencies | Est. Effort |
|------|-------------|-------------|
| **Inline ghost text completions** from local model | Phase 1 + editor | 8 hours |
| **Chat panel improvements** (streaming, model selector, context preview) | Phase 1 + ui | 6 hours |
| **Agent trace panel** (thought/tool/diff display) | Phase 1 + `ai/src/agent/` | 6 hours |
| **Diff view** for proposed changes | `ui/` + `editor/` | 8 hours |
| **Agent approve/reject UI** | `ui/src/app.rs` | 4 hours |
| **F12 performance overlay** (FPS, AI latency, router decisions) | `ui/` | 5 hours |

**Total estimated effort: ~37 hours**

### 5.6 Phase 6: Hardening + Polish (Pre-v1.0)

| Task | Dependencies | Est. Effort |
|------|-------------|-------------|
| **Benchmark suite** (criterion for hot paths) | `benches/` | 6 hours |
| **Fuzz testing** (buffer ops, router decisions) | `fuzz/` | 8 hours |
| **Hot-reload config** (wire notify → reload handler) | `config/` | 4 hours |
| **Undo coalescing** (group rapid edits) | `editor/src/buffer.rs` | 3 hours |
| **Clipboard integration** (copy/paste) | `editor/` + `ui/` | 2 hours |
| **Auto-pair brackets/quotes** | `editor/` | 3 hours |
| **Bracket matching** | `editor/` | 2 hours |
| **Cross-platform testing** (Linux + macOS) | CI | 4 hours |
| **Binary size optimization** (strip, LTO, codegen-units=1) | Release profile | 2 hours |

**Total estimated effort: ~34 hours**

---

## 6. Project Structure (Current + Target)

```
aurora/
├── Cargo.toml                  # Workspace root
├── rust-toolchain.toml         # Stable Rust (latest), fmt + clippy
├── .gitignore
├── README.md
├── AGENTS.md                   # AI agent instructions (for Codebuff)
├── ARCHITECTURE.md             # Architectural documentation
├── aurora-spec.md              # ← THIS FILE (product spec)
├── ROADMAP.md                  # Strategic roadmap
│
├── aurora-bin/                 # Binary entry point
│   └── src/main.rs             # CLI args, self-test, editor-test, GUI launch
│
├── aurora-core/                # [VESTIGIAL] Shared types + versioning
│   └── src/lib.rs              # Only version() — consider merging into ai or removing
│
├── config/                     # Hot-reloadable TOML configuration
│   ├── Cargo.toml              # Depends on notify (unused), serde, toml
│   ├── src/lib.rs              # Re-exports
│   ├── src/types.rs            # All config structs (AuroraConfig, AiConfig, etc.)
│   ├── src/schema.rs           # TOML validation
│   ├── src/loader.rs           # ConfigLoader (no watcher yet)
│   └── src/default.toml        # Shipped defaults
│
├── editor/                     # Core text editing subsystem
│   ├── Cargo.toml              # ropey 1.6, tree-sitter 0.22 (feature-gated)
│   ├── src/lib.rs              # Editor struct (high-level API), file I/O, language detection
│   ├── src/buffer.rs           # Ropey-backed Buffer, Delta, undo/redo, CachedLineOffsets
│   ├── src/cursor.rs           # Cursor (anchor/head model), CursorSet (multi-cursor)
│   ├── src/viewport.rs         # Virtual scrolling, smooth scroll, page up/down
│   ├── src/syntax.rs           # ScopeClassifier (rule-based), HighlightEngine (tree-sitter)
│   ├── src/events.rs           # EditorEvent enum, EventCollector (for LSP/AI bridges)
│   └── src/error.rs            # EditorError enum (thiserror)
│
│   [NEEDS WORK:]
│   - Fix tree-sitter version in Cargo.toml (says 0.22, ARCHITECTURE says 0.23)
│   - Wire tree-sitter highlighting to UI
│
├── lsp/                        # Language Server Protocol client
│   ├── Cargo.toml              # lsp-types, tokio, serde, uuid, futures
│   ├── src/lib.rs              # Re-exports
│   ├── src/client.rs           # LspClient — initialize, shutdown, request dispatch
│   ├── src/transport.rs        # JSON-RPC over stdio (Content-Length framing)
│   ├── src/connection.rs       # LspConnection — process lifecycle, message I/O
│   ├── src/pool.rs             # ConnectionPool — per-language, per-workspace
│   └── src/bridge.rs           # LspBridge — EditorEvent → LSP messages
│
├── ui/                         # UI layer (egui-based)
│   ├── Cargo.toml              # eframe 0.20 (feature-gated), egui 0.20
│   ├── src/lib.rs              # start_ui(), library setup
│   ├── src/app.rs              # AuroraApp — main application state, all panel renderers
│   ├── src/window.rs           # eframe window setup (1200x800 default)
│   ├── src/theme.rs            # Color palette, frame builders, style setup, scope_to_color()
│   └── src/status.rs           # Status bar state
│
│   [NEEDS WORK:]
│   - Smooth scrolling
│   - Custom scrollbar widget
│   - Completion popup widget
│   - Hover tooltip widget
│   - Command palette (Ctrl+P)
│   - Agent trace panel
│   - Diff review component
│
├── ai/                         # AI routing & providers (NATIVE RUST, no sidecar)
│   ├── Cargo.toml              # reqwest, serde, dashmap, uuid, ring, rusqlite
│   ├── src/lib.rs              # Re-exports, FreeLlmClient (sidecar), EphemeralKeyStore
│   ├── src/error.rs            # AiError enum (thiserror)
│   │
│   ├── src/router/mod.rs       # AIRouter — fallback chain, provider registry, route()
│   │   [NEEDS WORK:]
│   │   - Sticky session cache
│   │   - Context-aware auto-scoring
│   │   - Streaming response support
│   │   - Analytics logging
│   │
│   ├── src/providers/mod.rs    # ProviderAdapter trait + LocalProvider + MockCloud + Groq (stub)
│   │   [NEEDS WORK:]
│   │   - OllamaProvider (LOCALHOST_11434) ⬜
│   │   - GroqProvider (real HTTP) 🟡 partially built
│   │   - OpenAIProvider ⬜
│   │   - AnthropicProvider ⬜
│   │   - GeminiProvider ⬜
│   │   - CerebrasProvider ⬜
│   │   - LocalLlamaProvider (llama-cpp-rs FFI) ⬜
│   │
│   ├── src/ratelimit/mod.rs    # RateLimitLedger — DashMap counters, sliding window
│   ├── src/health/mod.rs       # HealthMonitor — background probes, key health states
│   ├── src/keystore/mod.rs     # EphemeralKeyStore (in-memory) + EncryptedKeyStore (AES-256-GCM + SQLite)
│   ├── src/context/mod.rs      # ContextPruner — token budget management, relevance scoring
│   │
│   ├── src/agent/mod.rs        # AgentLoop — orchestrates LLM + tool calls
│   ├── src/agent/tools.rs      # Tool trait + built-in tools (read_file, write_file, etc.)
│   │
│   ├── src/sidecar.rs          # [TO BE DEPRECATED] FreeLLMAPI Node.js sidecar manager
│   └── src/freellm.rs          # [TO BE DEPRECATED] FreeLlmClient HTTP client
│
├── plugin/                     # WASI plugin host
│   └── src/lib.rs              # Skeleton (prints message, does nothing)
│
├── sidecar/                    # FreeLLMAPI sidecar (optional, to be deprecated)
│   ├── setup.sh
│   └── freellmapi/             # Cloned repo (gitignored)
│
├── .github/workflows/
│   └── ci.yml                  # CI pipeline: fmt, clippy, test, build
│
└── benches/                    # (future) criterion benchmarks
```

---

## 7. Performance Targets (vs. Zed)

These are concrete, measurable targets. All values are P95 unless specified.

| Metric | Aurora Target | Zed (approx.) | VS Code (approx.) | Priority |
|--------|--------------|---------------|-------------------|----------|
| Cold startup | **<80ms** | ~400ms | ~3,000ms | P0 |
| Warm startup (session restore) | **<30ms** | ~150ms | ~1,000ms | P0 |
| Frame budget | **16ms (60fps)** | 16ms | ~33ms | P0 |
| Memory RSS (idle) | **<120MB** | ~180MB | ~650MB | P0 |
| Memory RSS (@ 50k LOC) | **<150MB** | ~250MB | ~900MB | P1 |
| Edit latency (P95) | **<8ms** | ~5ms | ~20ms | P0 |
| LSP completion (P95) | **<50ms** | ~50ms | ~150ms | P1 |
| AI inline completion (P95) | **<50ms** | ~100ms (Zeta) | ~200ms | P0 |
| AI chat TTFT (P95) | **<500ms** | ~800ms | ~1,500ms | P0 |
| Router decision time | **<5ms** | N/A (Zed BYOK) | N/A | P1 |
| Tree-sitter parse (@10k LOC) | **<2ms** | ~2ms | ~15ms | P2 |
| File search (10k files) | **<100ms** | ~150ms | ~500ms | P2 |
| Binary size (stripped) | **<5MB** | ~20MB | ~200MB | P1 |

### Optimization Strategy

- **Memory:** `mimalloc` allocator, LRU caches, string interning, zero-alloc hot paths
- **Startup:** Lazy init for LSP, AI, plugins. Memory-mapped session cache for warm startup
- **Rendering:** Frame pacing, render batching, off-thread tree-sitter parsing
- **AI:** Context pruning (token budget), local-first routing, request cancellation on keystroke
- **LSP:** Debounced requests (150ms), cancellable in-flight, deduplicated
- **Distribution:** LTO, codegen-units=1, strip symbols in release

---

## 8. Known Technical Debt (Clean as We Go)

These issues should be fixed when touching the relevant code, not in dedicated cleanup sprints:

| Issue | File(s) | Severity | Notes |
|-------|---------|----------|-------|
| `notify` crate dependency unused | `config/Cargo.toml` | Low | Listed as dep, no watcher implemented |
| `aurora-core` is vestigial | `aurora-core/src/lib.rs` | Low | Only has `version()`, could merge into `ai` or remove |
| Config crate aliasing | `ai/Cargo.toml` | Medium | `package = "config"` imported as `aurora-config` |
| `plugin` crate does nothing | `plugin/src/lib.rs` | Low | Prints message, returns. No WASI init |
| Tree-sitter version mismatch | `editor/Cargo.toml` | Low | Uses 0.22, ARCHITECTURE specifies 0.23 |
| `SidecarManager` + `FreeLlmClient` | `ai/src/sidecar.rs`, `ai/src/freellm.rs` | Low | To be deprecated once native providers are built |
| No clipboard integration | Everywhere | Medium | `Ctrl+C/V` won't work with system clipboard |
| No keyboard shortcut dispatch | `ui/src/app.rs` | Medium | Shortcuts displayed in menus but not wired to actions |
| Buffer load_text creates new buffer | `editor/src/lib.rs` | Low | Destroys undo history on file switch |
| Viewport scroll buffer hardcoded | `editor/src/viewport.rs` | Low | `scroll_buffer_lines = 50` constant |
| EventCollector unused | `editor/src/events.rs` | Low | Events emitted but LspBridge not consuming them |
| Groq provider uses placeholder key | `ai/src/providers/mod.rs` | Low | Self-test uses "placeholder-key" |
| Sidecar referenced in UI chat | `ui/src/app.rs` | Medium | `send_chat_message` talks to FreeLLMAPI sidecar, needs native alternative |

---

## 9. Immediate Next Steps (First Session)

Based on the interview decisions and project analysis, here's the recommended first development session:

### Step 1: Real AI Providers (Native Rust)

**Goal:** Replace mock providers with real HTTP providers. The editor should be able to actually call AI models.

1. **OllamaProvider** — HTTP client to `localhost:11434/v1/chat/completions`
   - Zero config, no API key needed
   - Implement `ProviderAdapter` trait, streaming support
   - Auto-detect available models from `/api/tags`

2. **OpenAIProvider** — OpenAI-compatible HTTP client
   - Works with any OpenAI-compatible API (OpenAI, OpenRouter, etc.)
   - Streaming + tool calling support
   - Rate limit handling from `RateLimitLedger`

3. **Wire providers into router** so `--self-test` actually calls real models

4. **Auto-routing** — implement `model: "auto"` scoring heuristics
   - Inline completions → prefer local/Ollama
   - Chat → prefer quality (Groq, OpenAI)
   - Sticky sessions → 30min model affinity

### Step 2: Wire AI to UI

**Goal:** Chat panel and inline completions should work with real AI.

1. Replace `FreeLlmClient` call in `send_chat_message()` with native router call
2. Add streaming display to chat panel
3. Wire inline completions from Ollama into editor ghost text

### Step 3: First GitHub Commit

**Goal:** Clean commit with meaningful progress.

1. Commit atomic changes with clean history
2. Tag as `v0.1.0` after first real AI provider is working

---

## 10. Provider Adapter Template (For New Providers)

Each new provider follows this pattern:

```rust
// ai/src/providers/my_provider.rs

use async_trait::async_trait;
use super::ProviderAdapter;
use crate::error::{AiError, AiResult};

pub struct MyProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl MyProvider {
    pub fn new(api_key: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.to_string(),
            model: "default-model".into(),
            base_url: "https://api.example.com".into(),
        }
    }
}

#[async_trait]
impl ProviderAdapter for MyProvider {
    fn provider_id(&self) -> &str { "my-provider" }
    fn model_name(&self) -> &str { "my-provider/model-name" }
    fn supports_streaming(&self) -> bool { true }
    fn supports_tool_calling(&self) -> bool { false }
    fn default_priority(&self) -> u8 { 50 }
    fn p95_latency_ms(&self) -> u64 { 300 }
    fn quality_score(&self) -> u8 { 7 }

    async fn chat_completion(&self, prompt: &str) -> AiResult<String> {
        // 1. Build request body (OpenAI-compatible format)
        // 2. POST to self.base_url + "/v1/chat/completions"
        // 3. Handle errors (429 → RateLimited, 4xx/5xx → ProviderError)
        // 4. Parse response and return content string
        todo!()
    }

    async fn stream_chat_completion(
        &self,
        prompt: &str,
        tx: tokio::sync::mpsc::Sender<String>,
    ) -> AiResult<()> {
        // 1. POST with stream: true
        // 2. Parse SSE data chunks
        // 3. Send each content delta through tx
        // 4. Handle cancellation via tx.send() error
        todo!()
    }
}
```

---

## 11. UI Component Inventory (Current State)

| Component | File | Status | Notes |
|-----------|------|--------|-------|
| Menu bar | `ui/src/app.rs:render_menu_bar` | ✅ Done | File, Edit, View, AI menus |
| Status bar | `ui/src/app.rs:render_status_bar` | ✅ Done | File name, lang, cursor pos, AI status |
| File tree | `ui/src/app.rs:render_file_tree` | ✅ Done | Expand/collapse dirs, file icons, click to open |
| Editor area | `ui/src/app.rs:render_editor_area` | ✅ Done | Tab bar, line numbers, syntax highlighting, cursor |
| Chat panel | `ui/src/app.rs:render_chat_panel` | ✅ Done | Messages, input, streaming indicator |
| Agent panel | `ui/src/app.rs:render_agent_panel` | ✅ Done | Trace display, empty state |
| Theme colors | `ui/src/theme.rs` | ✅ Done | Aurora Dark palette, scope mapping |
| **Missing:** | | | |
| Completion popup | — | ⬜ | Not implemented |
| Hover tooltip | — | ⬜ | Not implemented |
| Command palette | — | ⬜ | Not implemented |
| Custom scrollbar | — | ⬜ | Not implemented |
| Smooth scroll animation | `editor/src/viewport.rs` | ⬜ | Has smooth offset, not animated in UI |
| Diff review panel | — | ⬜ | Not implemented |
| F12 performance overlay | — | ⬜ | Not implemented |
| Aurora Light theme | — | ⬜ | Dark exists, Light doesn't |

---

## 12. Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| **egui visual limitations** prevent Zed-level polish | Medium | High | Invest in custom widget painting. If egui proves insufficient, benchmark wgpu custom rendering as alternative |
| **Native AI providers** more work than expected | Medium | Medium | Start with Ollama (simplest, zero-config). Groq is partially built. Add providers incrementally |
| **Performance targets** conflict with feature scope | Low | High | Aggressive lazy init. Profile startup early. Strip binary in release. Benchmark in CI |
| **Single developer** velocity limits competitive timeline | High | Medium | Focused MVP scope. Use AI-assisted development. Clean architecture enables future contributors |
| **Git history cleanup** conflicts with "frequent commits" | Low | Low | Use `git commit --amend` or squash before push. Follow "atomic commits" workflow |
| **FreeLLMAPI sidecar** deprecation breaks existing users | Low | Medium | Keep sidecar code in tree but unused. Document migration. Remove in v0.3.0+ |

---

## 13. Development Principles

1. **Atomic commits** — Each commit compiles, tests pass, tells a clear story
2. **Clean as we go** — Fix technical debt when touching related code
3. **Native Rust first** — No Node.js, Python, or other runtime dependencies
4. **Privacy by default** — Zero telemetry, local models default, cloud opt-in
5. **Observable AI** — Every AI decision visible in UI (provider, fallback count, latency)
6. **MIT license** — Open from day one, welcome contributions
7. **Stable Rust latest** — No nightly features, no unstable APIs
8. **Semantic versioning** — v0.1.0, v0.2.0, ... v1.0.0 at meaningful milestones

---

## 14. How to Use This Spec

1. **Review and approve** — Read through the spec, flag any issues, and confirm direction
2. **Start executing** — Use Section 9 (Immediate Next Steps) as the starting point for development
3. **Update as you go** — When decisions change or new context emerges, update this file
4. **Reference during commits** — Each commit should advance one of the phases listed above

## Appendix A: Estimated Timeline to v1.0

| Phase | Hours | Realistic Timeline (10-15 hrs/week) |
|-------|-------|-------------------------------------|
| Phase 1: AI Native Routing | ~22 hrs | 2 weeks |
| Phase 2: Visual Polish | ~21 hrs | 2 weeks |
| Phase 3: LSP Productionization | ~28 hrs | 2-3 weeks |
| Phase 4: File Management + Sessions | ~35 hrs | 3 weeks |
| Phase 5: AI UX + Agent Mode | ~37 hrs | 3 weeks |
| Phase 6: Hardening + Polish | ~34 hrs | 3 weeks |
| **Total** | **~177 hrs** | **~12-16 weeks (3-4 months)** |

Note: Phases can overlap. For example, visual polish (Phase 2) can proceed in parallel with LSP work (Phase 3). The AI routing (Phase 1) is the critical path — everything else depends on real AI working.

---

*This specification is a living document. All decisions are based on codebase analysis and user interviews as of June 2, 2026, and should be revisited as the project evolves.*
