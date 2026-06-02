# Aurora — workspace (scaffold)

This repository contains an initial scaffold for the Aurora project (architecture in ARCHITECTURE.md).

## Rust Version Requirements

- **Minimum for headless mode:** Rust 1.78.0
- **For GUI features:** Rust 1.80+ (stable) recommended

The egui ecosystem has moved beyond Rust 1.78.0 due to transitive dependency constraints. If you encounter `edition2024` errors when building the GUI, upgrade your Rust toolchain.

### Upgrade Rust

```bash
rustup update
rustup override set stable
```

Verify your Rust version:

```bash
rustc --version
```

## Build & Run

### Headless Mode (no GUI)

```bash
# Build the workspace
cargo build --workspace

# Run the self-test demonstrating provider fallback chain
cargo run -p aurora-bin -- --self-test
```

**Expected output:**

```
[self-test] Starting Aurora self-test...
[ui] Running in headless mode (GUI requires Rust 1.80+).
plugin host: WASI support not enabled (build with --features wasi to enable)
[self-test] Registered providers: MockCloud (5 req/min) → Groq → Local
[self-test] Routing 8 requests to test fallback when rate limit is hit...
[self-test] Request 1: routed to mock_cloud/test, fallbacks=0
[self-test] Request 2: routed to mock_cloud/test, fallbacks=0
[self-test] Request 3: routed to mock_cloud/test, fallbacks=0
[self-test] Request 4: routed to mock_cloud/test, fallbacks=0
[self-test] Request 5: routed to mock_cloud/test, fallbacks=0
[self-test] Request 6: routed to groq/llama-3.3-70b, fallbacks=1
[self-test] Request 7: routed to groq/llama-3.3-70b, fallbacks=1
[self-test] Request 8: routed to groq/llama-3.3-70b, fallbacks=1
[self-test] Complete.
```

The self-test demonstrates the **provider adapter pattern** with fallback chain logic:
1. Requests 1–5 route to `MockCloud` (succeeds, under rate limit)
2. Request 6+ fails on `MockCloud` (rate limited: 5 req/min)
3. Router automatically falls back to `Groq` for remaining requests
4. `LocalProvider` is available as a final fallback (never exceeded)

### GUI Mode (Rust 1.80+)

First, upgrade Rust:

```bash
rustup update stable
```

Then build and run with the GUI enabled:

```bash
cargo build -p aurora-bin --features gui

cargo run -p aurora-bin --features gui -- --self-test
```

The egui window opens showing:
- **Aurora** title and description
- **AI Provider** status panel (updated in real-time by the router)
- **Refresh Status** button to query the current provider

## Architecture

This scaffold provides:

- **`aurora-bin`**: Binary entry point and self-test harness
- **`aurora-core`**: Shared utilities and versioning
- **`ai`**: AI router with provider adapter pattern
  - `AIRouter`: Dynamic provider registry with fallback chain logic  
  - `ProviderAdapter` trait: Pluggable interface for AI providers (chat completion)
  - **Providers:**
    - `MockCloudProvider`: Rate-limited (5 req/min), simulates cloud API behavior
    - `GroqProvider`: Mock for Groq API (ready for real HTTP integration)
    - `LocalProvider`: Always-available fallback with simulated latency (~250ms)
  - `StatusHandle`: Thread-safe status updates for UI integration
- **`ui`**: UI layer with optional egui backend (feature-gated)
- **`plugin`**: WASI plugin host (feature-gated)

## Provider Adapter Pattern

Providers are registered with the router in priority order. When a request is made:

1. Router tries each provider in sequence
2. If a provider succeeds, routing stops (success recorded)
3. If a provider fails, router increments fallback counter and tries the next provider
4. If all providers fail, routing fails with error

**Example registration:**

```rust
let mut router = ai::AIRouter::new();
router.register_provider(Arc::new(ai::providers::MockCloudProvider::new("api/cloud")));
router.register_provider(Arc::new(ai::providers::GroqProvider::mock()));
router.register_provider(Arc::new(ai::providers::LocalProvider::new()));

let result = router.route(AIRequest { prompt: "Hello", conversation_id: None }).await;
// Returns: RoutingMetadata { routed_via: "mock_cloud/test" or "groq/llama-3.3-70b", fallback_attempts: 0..1 }
```

## WASI Plugin Support

To enable the WASI plugin host:

```bash
cargo build --features plugin/wasi
cargo run -p aurora-bin --features plugin/wasi -- --self-test
```

Output includes:
```
plugin host: WASI support enabled (wasmtime feature)
```

## Features

| Feature         | Dependencies      | Purpose                              |
|-----------------|-------------------|--------------------------------------|
| `gui`          | eframe 0.20, egui | Native UI window with egui rendering |
| `plugin/wasi`  | wasmtime 21       | WASI sandboxed plugin runtime       |

## Known Limitations

- **Groq, Cerebras, other cloud APIs**: Currently mocked (simulation mode)
  - Implementation ready; awaiting real API credentials and HTTP integration
  - Add `reqwest 0.11` to `ai/Cargo.toml` and implement HTTP requests when credentials available
  
- **GUI on Rust < 1.80**: Feature disabled on older Rust versions
  - Upgrade to Rust 1.80+ to enable the `gui` feature
  - See "Upgrade Rust" section above

## Troubleshooting

### `edition2024` error when building GUI

**Solution:** Upgrade Rust to 1.80+:

```bash
rustup update stable
```

The egui ecosystem on crates.io requires newer transitive dependencies that depend on the `edition2024` unstable feature. This is a known limitation with Rust 1.78.0 and pre-1.80 toolchains.

### Building in CI without GUI

If your CI environment is headless (no display), build without the GUI feature:

```bash
cargo build --workspace
cargo test --workspace
```

## Next Steps

1. **Real AI providers**: Replace mock providers with Groq, Cerebras, or local llama-cpp integrations.
2. **Tree-sitter integration**: Add syntax highlighting and symbol indexing.
3. **LSP client**: Connect to language servers for code intelligence.
4. **Terminal emulation**: Integrate vte parser and PTY spawning for workspace terminal support.
5. **WASM plugins**: Build example plugins and demonstrate capability-based sandboxing.

---

**Note:** Project codename: `aurora`. Architecture designed for Rust 1.78+ to 1.80+. GUI features optimized for 1.80+ (stable).

