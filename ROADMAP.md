# Aurora: From Scaffold to MVP

## Strategic Pillars

1. **Local-first AI with transparent cloud fallback** — no competitor does this well
2. **Vendor-agnostic multi-provider routing** — zero lock-in (via FreeLLMAPI sidecar)
3. **Privacy by default** — zero telemetry, encrypted keys, local models default
4. **Rust-native performance** — matches Zed, beats all Electron editors
5. **Observable AI** — transparent routing, fallback count, rate headroom in UI

## Architecture (After MVP)

```
┌───────────────────────────────────────────────────────┐
│                Aurora (Rust process)                    │
│                                                        │
│  ┌──────────┐  ┌──────────┐  ┌──────────────────────┐ │
│  │ Editor   │  │ LSP      │  │ AI Client (HTTP)     │ │
│  │ (Buffer, │  │ Client   │  │ → localhost:3999      │ │
│  │ cursors, │  │ (complet,│  └──────────────────────┘ │
│  │ viewport)│  │ diag,    │  ┌──────────────────────┐ │
│  │          │  │ hover)   │  │ Agent (tool exec,    │ │
│  └──────────┘  └──────────┘  │ diff review)         │ │
│                               └──────────────────────┘ │
└───────────────────────────┬────────────────────────────┘
                            │ HTTP
┌───────────────────────────▼────────────────────────────┐
│            FreeLLMAPI (Node.js sidecar)                 │
│  ┌────────┐  ┌────────┐  ┌────────┐  ┌────────┐       │
│  │ Router │  │ Rate   │  │ Key    │  │ Health │       │
│  │(fallbk)│  │ Ledger │  │ Store  │  │ Checks │       │
│  └────────┘  └────────┘  └────────┘  └────────┘       │
│        │          │           │           │             │
│        ▼          ▼           ▼           ▼             │
│  ┌─────────────────────────────────────────────────┐   │
│  │ 12+ Provider Adapters (Groq, Google, Cerebras)  │   │
│  └─────────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────────┘
```

## Phases

### Phase 0: Foundation — "It Compiles"
- Fix compilation errors in `main.rs`
- Set up CI (GitHub Actions)
- Verify config loads end-to-end
- Tag `v0.1.0`

### Phase 1: Editor Kernel — "Type Something"
- Wire editor to egui text rendering
- Keyboard input dispatch
- File I/O (open/save)
- Syntax highlighting on canvas
- Cut/copy/paste
- Undo/redo coalescing

### Phase 2: AI Backend — "Talk to AI"
- Bundle FreeLLMAPI as sidecar
- AI crate → HTTP client to sidecar
- Chat panel UI (streaming)
- Inline completions (Ollama local model)
- Drop Rust stub providers

### Phase 3: LSP Integration — "Code Intelligence"
- Wire LspClient to editor events
- Completions popup
- Diagnostics gutter
- Hover tooltip
- Go-to-definition

### Phase 4: Agent Mode — "The Killer Feature"
- Agent loop (AI calls tool → Aurora executes → feed back)
- Tool definitions (read/write/edit file, run command, search)
- Agent panel with thought/tool/diff display
- Workspace context in prompt
- Context pruning implementation

### Phase 5: Hardening — "Ship It"
- Hot-reload config
- Keybindings system
- Session restore
- Performance profiling (<80ms start, <16ms frame, <150MB RSS)
- WASI plugin skeleton
- Bug bash

## Key Decisions

- **FreeLLMAPI sidecar** over native Rust providers — 12 working providers, community-maintained
- **egui** for MVP UI — switch to wgpu custom rendering after Phase 3 if needed
- **Ollama** for local inference — via FreeLLMAPI OpenAI-compatible endpoint
- **AI before LSP** in phase order — AI is the differentiator, LSP is table stakes
