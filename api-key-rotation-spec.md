# API Key Rotation Spec — Aurora IDE

> **Status:** Ready for implementation
> **Based on:** 3-round user interview + codebase analysis
> **Estimated effort:** ~40 hours across backend (Rust) + frontend (React/TS)

---

## 1. Executive Summary

Build a **multi-tier key rotation system** that lets Aurora seamlessly rotate through multiple API keys for the same provider when free-tier (or paid) usage limits are approached, then fall back to other providers only when all keys for a provider are exhausted. The system must be **fully transparent** to the developer, **proactively warn** before limits are hit, and **gracefully pause** AI features with clear messaging when every key across every provider is exhausted.

**Why this matters:** Free-tier providers (Groq, Gemini, Cerebras, etc.) have strict per-key RPM/RPD/TPM/TPD limits. A single key can be exhausted in minutes during an active coding session. By rotating through multiple keys for the same provider, a team can extend their free usage dramatically before ever needing to fall back to another (potentially slower or lower-quality) provider.

---

## 2. Core Decisions (From Interview)

| Decision | Choice |
|----------|--------|
| **Rotation depth** | Per-provider key pool first → cross-provider fallback second |
| **Provider scope** | Start with free-tier (Groq, Gemini, Cerebras, etc.), expand to paid (OpenAI, Anthropic) later |
| **Key management** | Hybrid: personal keys via env/config + shared pool via in-app Key Management panel |
| **Exhaustion policy** | Pause AI features, show clear "All keys exhausted" message |
| **Visibility** | Fully transparent — per-key usage, remaining quota, rotation events in status bar |
| **Monitoring style** | Proactive — warn at 80% of estimated quota, escalate to red at 95% |
| **Key config** | Dedicated Key Management UI panel with encrypted storage |
| **Team sync** | Each machine has local copy of shared pool (manually synced) |
| **Warning style** | Status bar indicator: yellow at 80%, red at 95% |

---

## 3. Architecture Overview

### 3.1 Multi-Tier Rotation Model

```
┌─────────────────────────────────────────────────────────────────┐
│                     AI Request (chat, inline, etc.)              │
└────────────────────────────────┬────────────────────────────────┘
                                 │
                    ┌────────────▼────────────┐
                    │   Provider Key Pool     │
                    │   Selection Engine      │
                    │   (per-provider)        │
                    └────────────┬────────────┘
                                 │
              ┌──────────────────┼──────────────────┐
              │                  │                  │
     ┌────────▼────────┐ ┌──────▼──────┐ ┌────────▼────────┐
     │  Groq Key Pool  │ │ Gemini Pool │ │ Cerebras Pool   │
     │  [key-1] ──┐    │ │ [key-1]     │ │ [key-1]         │
     │  [key-2]    │    │ │ [key-2]     │ │                 │
     │  [key-3]    │    │ │             │ │                 │
     │  [key-4]    │    │ │             │ │                 │
     │  [key-5] ◄──┘    │ │             │ │                 │
     └────────┬────────┘ └──────┬──────┘ └────────┬────────┘
              │                   │                 │
              └───────────────────┼─────────────────┘
                                  │
                    ┌─────────────▼─────────────┐
                    │   Cross-Provider Fallback │
                    │   (existing AIRouter)      │
                    │   Only when all keys in    │
                    │   a provider pool exhaust   │
                    └─────────────┬─────────────┘
                                  │
                    ┌─────────────▼─────────────┐
                    │   Final: All Exhausted     │
                    │   → Pause AI, show message │
                    └─────────────────────────────┘
```

**Key insight:** The existing `AIRouter` does cross-provider fallback. This feature adds a **new layer above it**: a `ProviderKeyPool` that manages multiple keys for a single provider and selects the best key before the router even sees the request.

### 3.2 Flow Diagram

```
1. AI Request arrives (chat panel, inline completion, agent mode)
2. Resolve target provider from router scoring (e.g., "groq")
3. Ask ProviderKeyPool for "groq" → get best available key
4. Attempt request with selected key
5. SUCCESS → record usage, update quota tracker, return response
6. FAILURE 429 → mark key as rate-limited, cooldown 60s, retry with next key in pool
7. FAILURE 401/403 → mark key as invalid, remove from rotation, retry with next key
8. All keys in pool exhausted → return "provider exhausted" to router
9. Router falls back to next provider (e.g., "gemini") → repeat from step 3
10. All providers exhausted → emit "AllKeysExhausted" event → UI pauses AI + shows message
```

---

## 4. Data Model Changes

### 4.1 New: `ProviderKeyPool` (Rust — `ai/src/keystore/`)

```rust
/// A pool of API keys for a single provider.
pub struct ProviderKeyPool {
    provider_id: String,
    keys: Vec<PoolKeyEntry>,
    /// Strategy for selecting the next key.
    selection_strategy: KeySelectionStrategy,
}

/// An individual key in a provider pool.
pub struct PoolKeyEntry {
    key_id: KeyId,
    /// The actual API key (decrypted in-memory, zeroized on drop).
    api_key: DecryptedApiKey,
    /// User-defined label (e.g., "personal-work", "team-shared-1").
    label: String,
    /// Where this key came from (env var, config file, UI panel).
    source: KeySource,
    /// Current health of this specific key.
    health: KeyHealth,
    /// Usage tracking for this key.
    usage: KeyUsageSnapshot,
}

/// Health state of an individual key (more granular than provider-level).
pub enum KeyHealth {
    Healthy,
    ApproachingLimit { percent_used: f64 }, // 80%+
    RateLimited { cooldown_until: Instant },
    Invalid,
    Unknown,
}

/// Real-time usage snapshot for a key.
pub struct KeyUsageSnapshot {
    /// Requests this minute
    rpm_used: u32,
    rpm_limit: u32,
    /// Requests today
    rpd_used: u32,
    rpd_limit: u32,
    /// Tokens this minute
    tpm_used: u32,
    tpm_limit: u32,
    /// Tokens today
    tpd_used: u32,
    tpd_limit: u32,
    /// Last updated
    last_updated: Instant,
}

/// How to select the next key from the pool.
pub enum KeySelectionStrategy {
    /// Round-robin through all healthy keys.
    RoundRobin,
    /// Prefer the key with the most remaining quota.
    MostHeadroom,
    /// Prefer the key with the lowest recent latency.
    LowestLatency,
    /// Weighted random (spreads load unevenly to avoid synchronized exhaustion).
    WeightedRandom,
}
```

### 4.2 Enhanced: `RateLimitLedger` (Rust — `ai/src/ratelimit/`)

The existing `RateLimitLedger` tracks per-key counters but uses hardcoded defaults (`rpm: 60`, `rpd: 10_000`, etc.). We need **provider-specific quota definitions** for free-tier providers so proactive warnings can be accurate.

```rust
/// Provider-specific free tier quotas (documented limits as of June 2026).
pub struct ProviderQuota {
    pub rpm: u32,      // Requests per minute
    pub rpd: u32,      // Requests per day
    pub tpm: u32,      // Tokens per minute
    pub tpd: u32,      // Tokens per day
    pub tpm_burst: u32, // Burst allowance (some providers allow brief spikes)
}

impl ProviderQuota {
    /// Get documented free-tier quotas for known providers.
    pub fn free_tier(provider: &str) -> Option<Self> {
        match provider {
            "groq" => Some(Self {
                rpm: 20,
                rpd: 1_440,      // ~1 per minute over 24h (conservative)
                tpm: 25_000,
                tpd: 500_000,
                tpm_burst: 30_000,
            }),
            "gemini" => Some(Self {
                rpm: 60,
                rpd: 1_500,
                tpm: 1_000_000,
                tpd: 1_000_000,  // Gemini free tier is generous
                tpm_burst: 1_500_000,
            }),
            "cerebras" => Some(Self {
                rpm: 30,
                rpd: 1_000,
                tpm: 100_000,
                tpd: 1_000_000,
                tpm_burst: 150_000,
            }),
            "openai" => Some(Self {
                // OpenAI free tier is very limited (mostly trial credits)
                rpm: 3,
                rpd: 200,
                tpm: 150_000,
                tpd: 150_000,
                tpm_burst: 200_000,
            }),
            "anthropic" => Some(Self {
                // Anthropic free tier is essentially nonexistent
                rpm: 5,
                rpd: 100,
                tpm: 25_000,
                tpd: 100_000,
                tpm_burst: 30_000,
            }),
            _ => None,
        }
    }
}
```

> **Note:** These quotas are estimates based on provider documentation as of June 2026. They should be **configurable per-key** in case a user has a paid tier with different limits, or a provider changes their free tier.

### 4.3 Enhanced: `AIRouter` Integration

```rust
pub struct AIRouter {
    // ... existing fields ...
    
    /// NEW: Per-provider key pools.
    key_pools: DashMap<String, ProviderKeyPool>,
    
    /// NEW: Quota definitions (provider_id -> ProviderQuota).
    quotas: DashMap<String, ProviderQuota>,
    
    /// NEW: Event bus for UI notifications (key rotation, exhaustion, etc.).
    event_tx: tokio::sync::broadcast::Sender<KeyRotationEvent>,
}

/// Events emitted by the key rotation system for the UI to consume.
pub enum KeyRotationEvent {
    /// A key was rotated because the previous one hit a limit.
    KeyRotated {
        provider: String,
        from_key_id: String,
        to_key_id: String,
        reason: RotationReason,
    },
    /// A key is approaching its limit (80%).
    KeyApproachingLimit {
        provider: String,
        key_id: String,
        key_label: String,
        percent_used: f64,
        dimension: String, // "rpm", "rpd", "tpm", "tpd"
    },
    /// All keys for a provider are exhausted.
    ProviderExhausted {
        provider: String,
    },
    /// All keys across ALL providers are exhausted.
    AllKeysExhausted,
    /// A request succeeded — usage updated.
    UsageUpdated {
        provider: String,
        key_id: String,
        snapshot: KeyUsageSnapshot,
    },
}

pub enum RotationReason {
    RateLimited,
    InvalidKey,
    QuotaExceeded,
    HealthyKeyPreference, // proactively rotated to spread load
}
```

---

## 5. Key Management System

### 5.1 Key Sources (Hybrid Model)

Keys can come from multiple sources, prioritized as follows:

| Priority | Source | How | Use Case |
|----------|--------|-----|----------|
| 1 | Environment variables | `GROQ_API_KEY_1`, `GROQ_API_KEY_2`, etc. | Quick personal setup |
| 2 | Config file (`~/.config/aurora/aurora.toml`) | `[ai.providers.groq.keys]` array | Persistent personal keys |
| 3 | Key Management UI panel | In-app add/edit/delete with encrypted storage | Team-shared pools, easy management |
| 4 | Per-project `.aurora.toml` | `[ai.providers.groq.keys]` | Project-specific keys |

**Loading order:**
1. On startup, scan env vars for `PROVIDER_API_KEY_N` pattern
2. Load keys from global config file
3. Load keys from per-project `.aurora.toml` (overrides global)
4. Load keys from encrypted key store (populated via UI panel)
5. Deduplicate by key value (same key from multiple sources = one entry)
6. Group into per-provider `ProviderKeyPool`s

### 5.2 Encrypted Storage for UI-Added Keys

Keys added via the UI panel must be **encrypted at rest** using the existing `EncryptedKeyStore` (AES-256-GCM + SQLite) behind the `keychain` feature.

```rust
// Schema extension for the encrypted key store
const KEY_POOL_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS provider_key_pool (
    key_id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    encrypted_key BLOB NOT NULL,
    nonce BLOB NOT NULL,
    label TEXT,
    quota_rpm INTEGER,
    quota_rpd INTEGER,
    quota_tpm INTEGER,
    quota_tpd INTEGER,
    created_at TEXT DEFAULT (datetime('now')),
    last_used TEXT
);
"#;
```

### 5.3 Key Metadata (Label + Quota Override)

Each key can have:
- **Label**: Human-readable name (e.g., "team-groq-1", "personal-gmail")
- **Quota override**: If the user has a paid tier or a provider changed limits, they can override the default free-tier quotas per-key.

---

## 6. Proactive Quota Monitoring

### 6.1 How It Works

The system does NOT rely solely on 429 responses. Instead, it **tracks every request** and **compares against documented quotas** to predict when a key will hit its limit.

```
For each successful request:
  1. Increment RPM/RPD counters for the key
  2. Estimate tokens used (from response usage headers or local approximation)
  3. Increment TPM/TPD counters
  4. Calculate usage percentage for each dimension
  5. If any dimension >= 80%:
       - Emit KeyApproachingLimit event
       - Status bar turns yellow for that provider
  6. If any dimension >= 95%:
       - Status bar turns red
       - Start preferring other keys in the pool
  7. If any dimension == 100%:
       - Mark key as RateLimited
       - Emit KeyRotated event
       - Move to next key in pool
```

### 6.2 Status Bar Indicator Design

The existing `StatusBar.tsx` shows `AI Ready` / `Thinking...`. Extend it to show:

```
Normal state:     "AI Ready — groq/llama-3 (key: team-1, 45% RPM)"
Yellow warning:   "AI Ready — groq/llama-3 (key: team-1, 82% RPM ⚠️)"
Red warning:      "AI Ready — groq/llama-3 (key: team-2, 96% RPM 🔴)"
Rotated:          "AI Ready — groq/llama-3 (rotated: team-1 → team-2)"
Provider exhausted: "AI Ready — gemini/flash (groq exhausted)"
All exhausted:    "AI Paused — all keys exhausted"
```

**Status bar right section (existing layout):**
```
[Git branch] [Modified] [File name] [Language]                    [FS] [AI: groq/llama-3 45% ⚡] [UTF-8] [Spaces: 4] [Ln 1, Col 1]
```

### 6.3 Token Estimation

Since not all providers return usage in headers, we need a fallback:

```rust
/// Estimate tokens in a request/response.
/// 1. Try to parse from response `usage` field (OpenAI-compatible)
/// 2. Fallback: ~4 characters per token for English, ~2.5 for code
/// 3. Fallback: count words and multiply by 1.3 (rough tokenization)
pub fn estimate_tokens(text: &str) -> usize {
    if let Some(usage) = response.usage {
        return usage.total_tokens as usize;
    }
    // Rough but fast approximation
    text.len() / 4
}
```

---

## 7. Exhaustion Handling

### 7.1 Provider Exhaustion

When ALL keys in a provider's pool are rate-limited, invalid, or at 100% quota:
1. Mark the provider as `ProviderExhausted`
2. Emit `ProviderExhausted` event
3. Return control to `AIRouter` to try the next provider
4. UI updates status bar to show fallback provider

### 7.2 Total Exhaustion (All Providers)

When EVERY provider has exhausted ALL keys:
1. Emit `AllKeysExhausted` event
2. Set global AI state to `Paused`
3. **Block new AI requests** at the router level (return `AiError::AllKeysExhausted`)
4. **Status bar** shows: `"AI Paused — all keys exhausted. Add keys in Settings > AI Keys."`
5. **Chat panel** shows a persistent banner:
   ```
   ┌─────────────────────────────────────────────┐
   │ ⚠️ All AI provider keys are exhausted         │
   │                                               │
   │   Add more API keys in Settings > AI Keys     │
   │   to resume AI assistance.                    │
   │                                               │
   │   [Open Key Manager]                          │
   └─────────────────────────────────────────────┘
   ```
6. **Inline completions** stop being requested entirely
7. **Agent mode** pauses and shows the same banner

### 7.3 Recovery

When the user adds a new key via the UI panel:
1. Immediately add it to the pool
2. Reset global AI state to `Active`
3. Clear the exhaustion banner
4. Retry any queued requests (if we implement a small retry queue)
5. Emit `KeyAdded` event with usage at 0%

When a rate-limited key's cooldown expires:
1. Automatically re-mark it as `Healthy` (or `ApproachingLimit` if still near limits)
2. Re-include it in rotation

---

## 8. UI: Key Management Panel

### 8.1 Design

A new sidebar panel or settings modal titled **"AI Key Manager"** accessible from:
- ActivityBar (new key icon)
- Settings menu
- Status bar click (when AI is paused)

**Layout:**
```
┌─────────────────────────────────────────────────────────────┐
│  AI Key Manager                                    [+] [?]  │
├─────────────────────────────────────────────────────────────┤
│  Provider: [ Groq ▼ ]                                       │
│                                                             │
│  ┌───────────────────────────────────────────────────────┐   │
│  │ Key Label        Quota Used    Status    Actions     │   │
│  ├───────────────────────────────────────────────────────┤   │
│  │ personal-gmail   12/20 RPM   🟢 Healthy   [Edit][×]│   │
│  │ team-shared-1    18/20 RPM   🟡 90% RPM   [Edit][×]│   │
│  │ team-shared-2     3/20 RPM   🟢 Healthy   [Edit][×]│   │
│  │ old-key           —           🔴 Invalid  [Edit][×]│   │
│  └───────────────────────────────────────────────────────┘   │
│                                                             │
│  [+ Add Key]  [Import from Env]  [Test All Keys]           │
│                                                             │
│  ─────────────────────────────────────────────────────────  │
│  Provider Quota Settings (Free Tier)                        │
│  RPM: [20   ]  RPD: [1440  ]  TPM: [25000 ]  TPD: [500000]│
│  [ ] Use custom quotas for this provider                    │
│                                                             │
│  Key Selection Strategy: [Most Headroom ▼]                  │
│                                                             │
│  ─────────────────────────────────────────────────────────  │
│  Global Status                                               │
│  Active providers: 3/3  |  Keys: 5  |  Requests today: 47  │
└─────────────────────────────────────────────────────────────┘
```

### 8.2 Add Key Flow

1. Click `[+ Add Key]`
2. Dialog appears:
   ```
   Add API Key
   ───────────
   Provider: [ Groq ▼ ]
   API Key:  [sk-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx]
   Label:    [e.g., "personal-work"]
   Quota:    [ ] Use default free-tier quotas
              [ ] Override: RPM [___] RPD [___] TPM [___] TPD [___]
   
   [Cancel]  [Add & Test]
   ```
3. On `[Add & Test]`, encrypt and store the key, then immediately run a health check
4. Show result: `✅ Key valid — 20 RPM available` or `❌ Key invalid: 401 Unauthorized`

### 8.3 Frontend → Backend Commands

New Tauri commands to add:

```rust
// In src-tauri/src/lib.rs

/// Add a new API key to the encrypted store.
#[tauri::command]
async fn add_api_key(
    state: State<'_, AppState>,
    provider: String,
    api_key: String,
    label: String,
    custom_quota: Option<ProviderQuotaConfig>,
) -> Result<KeyInfo, String> { ... }

/// List all API keys (without exposing the actual key values).
#[tauri::command]
async fn list_api_keys(
    state: State<'_, AppState>,
) -> Result<Vec<KeyInfo>, String> { ... }

/// Delete an API key.
#[tauri::command]
async fn delete_api_key(
    state: State<'_, AppState>,
    key_id: String,
) -> Result<(), String> { ... }

/// Test an API key (health check + list models).
#[tauri::command]
async fn test_api_key(
    state: State<'_, AppState>,
    key_id: String,
) -> Result<KeyTestResult, String> { ... }

/// Get current usage snapshot for all keys.
#[tauri::command]
async fn get_key_usage(
    state: State<'_, AppState>,
) -> Result<Vec<KeyUsageInfo>, String> { ... }

/// Import keys from environment variables.
#[tauri::command]
async fn import_keys_from_env(
    state: State<'_, AppState>,
) -> Result<usize, String> { ... }
```

### 8.4 TypeScript Types

```typescript
// src/types/index.ts (additions)

export interface ApiKeyInfo {
  key_id: string;
  provider: string;
  label: string;
  source: "env" | "config" | "ui";
  status: "healthy" | "approaching" | "rate_limited" | "invalid" | "unknown";
  percent_used: number;
  rpm_used: number;
  rpm_limit: number;
  last_used?: string;
}

export interface KeyUsageInfo {
  provider: string;
  key_id: string;
  label: string;
  rpm_used: number;
  rpm_limit: number;
  rpd_used: number;
  rpd_limit: number;
  tpm_used: number;
  tpm_limit: number;
  tpd_used: number;
  tpd_limit: number;
  percent_used: number;
  status: string;
}

export interface KeyRotationEvent {
  event_type: "key_rotated" | "key_approaching" | "provider_exhausted" | "all_exhausted" | "usage_updated";
  provider?: string;
  from_key_id?: string;
  to_key_id?: string;
  key_id?: string;
  key_label?: string;
  percent_used?: number;
  dimension?: string;
  reason?: string;
}
```

---

## 9. Backend Implementation Plan

### 9.1 New/Modified Files

| File | Action | Description |
|------|--------|-------------|
| `ai/src/keystore/pool.rs` | **New** | `ProviderKeyPool`, `PoolKeyEntry`, key selection strategies |
| `ai/src/keystore/quota.rs` | **New** | `ProviderQuota`, documented free-tier limits, per-key overrides |
| `ai/src/keystore/mod.rs` | **Modify** | Re-export new types, integrate `ProviderKeyPool` into existing `EphemeralKeyStore` / `EncryptedKeyStore` |
| `ai/src/ratelimit/mod.rs` | **Modify** | Add per-key `KeyUsageSnapshot`, `ProviderQuota` integration, `usage()` / `headroom()` methods |
| `ai/src/router/mod.rs` | **Modify** | Integrate `ProviderKeyPool` into `route()`: resolve provider → select key from pool → attempt request → handle rotation |
| `ai/src/router/events.rs` | **New** | `KeyRotationEvent` enum, broadcast channel for UI consumption |
| `ai/src/providers/mod.rs` | **Modify** | Extend `ProviderAdapter` to accept a specific `DecryptedApiKey` per-request instead of using the provider's single key |
| `ai/src/providers/groq.rs` | **Modify** | Update `GroqProvider` to accept key at request time (or use a key from the pool) |
| `ai/src/providers/openai.rs` | **Modify** | Same as Groq |
| `ai/src/providers/ollama.rs` | **No change** | Ollama has no API key, so it's outside the rotation system |
| `ai/src/lib.rs` | **Modify** | Wire up new modules, expose `ProviderKeyPool` and events |
| `src-tauri/src/lib.rs` | **Modify** | Add Tauri commands (`add_api_key`, `list_api_keys`, etc.), wire `AppState` to hold key pools |
| `src-tauri/src/lib.rs` | **Modify** | Emit `KeyRotationEvent` as Tauri events so React frontend can listen |
| `src/types/index.ts` | **Modify** | Add TypeScript types for keys, usage, rotation events |
| `src/components/StatusBar.tsx` | **Modify** | Show per-key usage, warning colors, rotation info |
| `src/components/KeyManagerPanel.tsx` | **New** | Full key management UI panel |
| `src/components/ActivityBar.tsx` | **Modify** | Add key manager icon |
| `config/src/types.rs` | **Modify** | Add `ProviderKeyConfig` to `AiConfig` for config-file-based keys |
| `config/src/default.toml` | **Modify** | Add example `[ai.providers.groq.keys]` section |

### 9.2 ProviderAdapter Trait Change

The current `ProviderAdapter` assumes each provider instance has a single baked-in API key:

```rust
// CURRENT (problematic)
pub struct GroqProvider {
    api_key: String, // set at construction time
}
```

This must change to allow **per-request key injection** from the pool:

```rust
// PROPOSED
pub struct GroqProvider {
    // No baked-in key — it's injected per-request
    client: Client,
    model: String,
    base_url: String,
}

impl GroqProvider {
    pub async fn chat_completion_with_key(
        &self,
        prompt: &str,
        api_key: &str,
    ) -> AiResult<String> { ... }
}
```

Alternatively, keep the current trait but have the router **construct a temporary provider instance** with the selected key for each request. This is simpler but slightly less efficient.

**Decision:** Add an optional `key` parameter to the existing `chat_completion` and `stream_chat_completion` methods with a default to the baked-in key for backward compatibility.

```rust
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    // ... existing methods ...
    
    /// Execute a chat completion with an explicit API key (for key rotation).
    /// If `key` is None, uses the provider's default/baked-in key.
    async fn chat_completion_with_key(
        &self,
        prompt: &str,
        key: Option<&str>,
    ) -> AiResult<String> {
        // Default: ignore the key param and use self.api_key
        self.chat_completion(prompt).await
    }
}
```

Then `GroqProvider` overrides this to actually use the injected key.

### 9.3 Router Integration Sequence

```rust
impl AIRouter {
    pub async fn route(&self, req: AIRequest) -> AiResult<RoutingMetadata> {
        // 1. Determine target provider using existing scoring logic
        let candidates = self.score_candidates(&req);
        
        for candidate in candidates {
            let provider_id = candidate.adapter.provider_id();
            
            // 2. NEW: Get the key pool for this provider
            if let Some(pool) = self.key_pools.get(provider_id) {
                // 3. Select the best key from the pool
                if let Some(key_entry) = pool.select_key() {
                    let api_key = &key_entry.api_key.value;
                    
                    // 4. Attempt the request with this specific key
                    match candidate.adapter.chat_completion_with_key(&req.prompt, Some(api_key)).await {
                        Ok(response) => {
                            // 5. Record success, update usage
                            self.record_success(&key_entry, &response);
                            return Ok(RoutingMetadata { ... });
                        }
                        Err(AiError::RateLimited(_)) => {
                            // 6. Mark key as rate-limited, cooldown, try next key
                            pool.mark_rate_limited(&key_entry.key_id, Duration::from_secs(60));
                            continue; // try next key in pool
                        }
                        Err(AiError::ProviderError(_, msg)) if msg.contains("401") || msg.contains("403") => {
                            // 7. Mark key as invalid
                            pool.mark_invalid(&key_entry.key_id);
                            continue;
                        }
                        Err(e) => {
                            // 8. Other error — try next key
                            continue;
                        }
                    }
                }
                
                // 9. All keys in pool exhausted for this provider
                self.emit_event(KeyRotationEvent::ProviderExhausted {
                    provider: provider_id.to_string(),
                });
                continue; // fall back to next provider
            } else {
                // 10. No key pool for this provider — use existing behavior (single key)
                match candidate.adapter.chat_completion(&req.prompt).await { ... }
            }
        }
        
        // 11. ALL providers exhausted
        self.emit_event(KeyRotationEvent::AllKeysExhausted);
        Err(AiError::AllKeysExhausted)
    }
}
```

---

## 10. Frontend Implementation Plan

### 10.1 StatusBar Enhancement

Modify `src/components/StatusBar.tsx`:

```tsx
// Add to Props interface
interface Props {
  // ... existing props ...
  aiKeyStatus?: AiKeyStatus;
}

interface AiKeyStatus {
  provider: string;
  model: string;
  key_label: string;
  percent_used: number;
  dimension: string; // "rpm", "rpd", etc.
  state: "healthy" | "approaching" | "critical" | "exhausted";
}

// In the right section of the status bar:
<span
  className="status-bar-item"
  style={{
    color: aiKeyStatus?.state === "exhausted" ? "var(--status-error)"
      : aiKeyStatus?.state === "critical" ? "var(--status-critical)"
      : aiKeyStatus?.state === "approaching" ? "var(--status-warning)"
      : "var(--status-ai-ready)",
  }}
  title={`${aiKeyStatus.provider}/${aiKeyStatus.model} — ${aiKeyStatus.key_label} — ${aiKeyStatus.percent_used}% ${aiKeyStatus.dimension}`}
>
  <Icon icon={aiKeyStatus?.state === "exhausted" ? "material-symbols:pause-circle" : "material-symbols:bolt"} size={12} />
  {aiKeyStatus?.state === "exhausted"
    ? "AI Paused"
    : `${aiKeyStatus.provider} ${aiKeyStatus.percent_used}%`}
</span>
```

### 10.2 KeyManagerPanel Component

New file: `src/components/KeyManagerPanel.tsx`

- Uses `invoke` to call Tauri commands
- Listens for `KeyRotationEvent` via Tauri's event system
- Table of keys with status indicators
- Add/Edit/Delete key dialogs
- Test key button (runs health check)
- Provider quota override form
- Selection strategy dropdown

### 10.3 Event Listeners

In `src/App.tsx`, add:

```tsx
useEffect(() => {
  const unlisten = listen<KeyRotationEvent>("key-rotation-event", (event) => {
    const payload = event.payload;
    
    switch (payload.event_type) {
      case "key_rotated":
        // Show toast: "Rotated from team-1 to team-2 on Groq"
        break;
      case "key_approaching":
        setAiKeyStatus({
          provider: payload.provider!,
          key_label: payload.key_label!,
          percent_used: payload.percent_used!,
          state: payload.percent_used! > 95 ? "critical" : "approaching",
          dimension: payload.dimension!,
        });
        break;
      case "all_exhausted":
        setIsAiThinking(false);
        setAiPaused(true);
        break;
      case "usage_updated":
        // Update status bar in real-time
        break;
    }
  });
  
  return () => { unlisten.then((fn) => fn()); };
}, []);
```

---

## 11. Security Considerations

| Concern | Mitigation |
|---------|-----------|
| **Keys in memory** | Use `DecryptedApiKey` with `ZeroizeOnDrop`. Keys only decrypted during request, zeroized after. |
| **Keys on disk** | AES-256-GCM encryption via `EncryptedKeyStore`. SQLite database at `~/.aurora/keys.db`. |
| **Keys in logs** | Never log key values. Log only `key_id` and `provider`. |
| **Keys in crash dumps** | `ZeroizeOnDrop` helps, but consider `mlock` for extreme cases (future enhancement). |
| **Team key sharing** | Manually synced means no network exposure. Keys travel via private channels the team already trusts. |
| **Config file keys** | If keys are in `aurora.toml`, warn the user that TOML is plaintext — recommend migrating to encrypted store. |

---

## 12. Testing Plan

### 12.1 Unit Tests (Rust)

| Test | File | What it verifies |
|------|------|-----------------|
| `test_round_robin_selection` | `pool.rs` | Keys are selected in round-robin order |
| `test_most_headroom_selection` | `pool.rs` | Key with most remaining quota is selected |
| `test_mark_rate_limited` | `pool.rs` | Rate-limited keys are skipped until cooldown |
| `test_mark_invalid` | `pool.rs` | Invalid keys are permanently removed |
| `test_all_keys_exhausted` | `pool.rs` | Returns `None` when no usable keys remain |
| `test_quota_80_percent_warning` | `ratelimit.rs` | Emits warning at 80% usage |
| `test_quota_100_percent_rotation` | `ratelimit.rs` | Triggers rotation at 100% |
| `test_groq_with_injected_key` | `groq.rs` | Provider uses injected key, not baked-in key |
| `test_router_key_pool_integration` | `router/mod.rs` | Router correctly uses key pools before cross-provider fallback |

### 12.2 Integration Tests (Rust + Tauri)

| Test | What it verifies |
|------|-----------------|
| `test_add_key_via_command` | `add_api_key` Tauri command encrypts and stores correctly |
| `test_list_keys_no_exposure` | `list_api_keys` never returns actual key values |
| `test_key_rotation_event_emission` | Events are emitted to the frontend via Tauri |
| `test_full_rotation_chain` | Mock providers + mock keys simulate full exhaustion and recovery |

### 12.3 Frontend Tests (Manual/Playwright)

| Test | What it verifies |
|------|-----------------|
| Add key via UI panel, see it in list | Key manager CRUD works |
| Trigger 80% usage, see yellow status bar | Proactive warning displays correctly |
| Exhaust all mock keys, see "AI Paused" banner | Exhaustion UI works |
| Add key while paused, see AI resume | Recovery flow works |

---

## 13. Configuration Reference

### 13.1 Global Config (`~/.config/aurora/aurora.toml`)

```toml
[ai]
default_model = "auto"

[ai.providers.groq]
# Multiple keys for Groq — loaded at startup
keys = [
    { label = "personal", key = "gsk_xxxxxxxxxxxxxxxxxxxxxxxx", quota = "free" },
    { label = "team-1", key = "gsk_yyyyyyyyyyyyyyyyyyyyyyyy", quota = "free" },
]
selection_strategy = "most_headroom"

[ai.providers.gemini]
keys = [
    { label = "personal", key = "AIzaSyxxxxxxxxxxxxxxxxxxxxxxxx", quota = "free" },
]

[ai.providers.cerebras]
keys = [
    { label = "team", key = "csk_xxxxxxxxxxxxxxxxxxxxxxxx", quota = "free" },
]

# Quota overrides (if you have paid tiers or providers changed limits)
[ai.quotas.groq]
rpm = 30       # Paid tier might be higher
rpd = 144_000  # 100x free tier
tpm = 100_000
tpd = 5_000_000
```

### 13.2 Environment Variables

```bash
# Auto-detected on startup and added to the key pool
export GROQ_API_KEY_1="gsk_xxx"
export GROQ_API_KEY_2="gsk_yyy"
export GEMINI_API_KEY_1="AIzaSy..."
export OPENAI_API_KEY_1="sk-..."
```

> **Pattern:** `{PROVIDER}_API_KEY_{N}` where N is any number. Provider name must match the provider_id (e.g., `groq`, `gemini`, `openai`).

---

## 14. Free-Tier Provider Quotas (Reference)

| Provider | RPM | RPD | TPM | TPD | Notes |
|----------|-----|-----|-----|-----|-------|
| Groq | 20 | ~1,440 | 25,000 | 500,000 | Very fast, free tier generous |
| Google Gemini | 60 | ~1,500 | 1,000,000 | 1,000,000 | Most generous free tier |
| Cerebras | 30 | ~1,000 | 100,000 | 1,000,000 | Fast, good for code |
| Together AI | 60 | ~1,440 | 100,000 | 200,000 | Many models |
| OpenAI | 3 | ~200 | 150,000 | 150,000 | Very limited free tier |
| Anthropic | 5 | ~100 | 25,000 | 100,000 | Essentially no free tier |
| Ollama | ∞ | ∞ | ∞ | ∞ | Local, no limits |

> **Important:** These are **default estimates**. Each key can override its quota. The system should also learn from actual rate limit responses — if a provider returns `x-ratelimit-remaining` headers, use those values instead of the documented defaults.

---

## 15. Performance Considerations

| Concern | Mitigation |
|---------|-----------|
| Key selection overhead | Key pools are in-memory `DashMap`. Selection is O(n) where n = keys per provider (typically < 10). Negligible vs HTTP latency. |
| Encryption overhead | Keys are decrypted once at startup and held in memory with `ZeroizeOnDrop`. No per-request decryption. |
| Event emission overhead | Events are fire-and-forget broadcast. Frontend subscribes only to what it needs. |
| Status bar updates | Throttle UI updates to 1Hz to avoid React re-render storms during rapid requests. |
| Memory growth | Rate counter sliding windows auto-evict old entries. Cooldown keys are re-checked periodically and re-included. |

---

## 16. Future Enhancements (Post-MVP)

| Feature | Description | Priority |
|---------|-------------|----------|
| **x-ratelimit header parsing** | Parse actual rate limit headers from providers (Groq, OpenAI, etc. return these) instead of relying on documented quotas. | P0 |
| **Key usage analytics** | Per-key, per-day, per-week usage charts in the key manager. Export to CSV. | P1 |
| **Auto-key discovery** | Scan common env var patterns (`GROQ_API_KEY`, `GROQ_API_KEY_1`, etc.) and prompt to import. | P1 |
| **Team key server** | Lightweight internal HTTP endpoint for fetching shared keys. | P2 |
| **Key rotation strategies** | Least-recently-used, weighted by cost (for paid tiers), time-based (rotate every N minutes). | P2 |
| **Provider-specific model pools** | Different models within the same provider have different quotas. Track per-model, not just per-provider. | P2 |
| **Predictive exhaustion** | Use request history to predict when a key will exhaust and pre-emptively rotate. | P3 |

---

## 17. Implementation Order (Recommended)

| Step | Task | Backend | Frontend | Est. Hours |
|------|------|---------|----------|-----------|
| 1 | Define `ProviderKeyPool`, `PoolKeyEntry`, quota types | ✅ | | 2 |
| 2 | Add provider-specific `ProviderQuota` defaults | ✅ | | 1 |
| 3 | Extend `RateLimitLedger` with per-key snapshots and 80% warning logic | ✅ | | 3 |
| 4 | Modify `ProviderAdapter` trait to accept per-request keys | ✅ | | 2 |
| 5 | Update `GroqProvider`, `OpenAIProvider` to use injected keys | ✅ | | 2 |
| 6 | Integrate key pools into `AIRouter::route()` | ✅ | | 4 |
| 7 | Add `KeyRotationEvent` broadcast system | ✅ | | 2 |
| 8 | Add Tauri commands for key CRUD + testing | ✅ | | 3 |
| 9 | Extend encrypted key store schema for pool keys | ✅ | | 2 |
| 10 | Load keys from env vars + config file on startup | ✅ | | 2 |
| 11 | Write backend unit tests | ✅ | | 3 |
| 12 | Add TypeScript types for key management | | ✅ | 1 |
| 13 | Create `KeyManagerPanel` React component | | ✅ | 5 |
| 14 | Enhance `StatusBar` with key usage indicators | | ✅ | 2 |
| 15 | Wire `App.tsx` to listen for rotation events | | ✅ | 2 |
| 16 | Add exhaustion banner to `ChatPanel` | | ✅ | 2 |
| 17 | Add ActivityBar icon for key manager | | ✅ | 1 |
| 18 | End-to-end test + polish | ✅ | ✅ | 4 |
| | **Total** | | | **43 hours** |

---

## 18. Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Provider quota docs are wrong | High | Medium | Make quotas **configurable per-key**. Parse actual rate limit headers when available. |
| Key encryption adds complexity | Medium | Low | Use existing `EncryptedKeyStore`. Fallback to `EphemeralKeyStore` if `keychain` feature is off. |
| UI panel feels cluttered | Medium | Low | Start simple (table + add/delete). Iterate based on usage. |
| Team key sync is manual | Low | Medium | Document the workflow. Future: add lightweight key server. |
| Too many keys → selection overhead | Low | Low | n is small (< 10 per provider). O(n) is trivial. |
| Providers change rate limit behavior | Medium | Medium | Reactive cooldown on 429 is the ultimate safety net. Proactive is best-effort. |

---

## 19. Acceptance Criteria

- [ ] A developer can add 3 Groq API keys via the Key Manager UI panel
- [ ] When key #1 hits 80% of its RPM quota, the status bar turns yellow and shows the warning
- [ ] When key #1 hits 100% RPM, the system automatically rotates to key #2 with no user interruption
- [ ] The rotation event is visible in the status bar ("rotated: team-1 → team-2")
- [ ] When all Groq keys are exhausted, the system falls back to Gemini keys automatically
- [ ] When ALL keys across ALL providers are exhausted, AI features pause and a clear banner appears
- [ ] Adding a new key while paused immediately resumes AI features
- [ ] Keys added via the UI are encrypted at rest (verified by inspecting `~/.aurora/keys.db`)
- [ ] Keys from environment variables (`GROQ_API_KEY_1`) are automatically loaded on startup
- [ ] The `list_api_keys` command never returns the actual key value (only metadata)
- [ ] All new code passes `cargo fmt --check` and `cargo clippy --workspace -- -D warnings`
- [ ] All new code has inline unit tests with > 80% coverage of the rotation logic

---

*This specification is a living document. Update it as implementation reveals new constraints or better approaches.*
