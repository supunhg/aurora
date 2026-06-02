use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Root Aurora configuration.
/// Loaded from ~/.config/aurora/aurora.toml + .aurora.toml (per-project, overrides).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuroraConfig {
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default)]
    pub editor: EditorConfig,
    #[serde(default)]
    pub keybindings: KeybindingsConfig,
    #[serde(default)]
    pub ai: AiConfig,
    #[serde(default)]
    pub lsp: LspConfig,
    #[serde(default)]
    pub terminal: TerminalConfig,
    #[serde(default)]
    pub plugins: PluginConfig,
}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeConfig {
    /// "dark" | "light" | "auto"
    #[serde(default = "default_theme_mode")]
    pub mode: String,
    /// Path to custom theme .toml file
    #[serde(default)]
    pub custom_theme_path: Option<String>,
    /// Font size in points
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    /// Font family
    #[serde(default = "default_font_family")]
    pub font_family: String,
    /// Line height ratio (1.0 = single-spaced)
    #[serde(default = "default_line_height")]
    pub line_height: f32,
}

fn default_theme_mode() -> String {
    "dark".into()
}
fn default_font_size() -> f32 {
    14.0
}
fn default_font_family() -> String {
    "JetBrains Mono".into()
}
fn default_line_height() -> f32 {
    1.5
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            mode: default_theme_mode(),
            custom_theme_path: None,
            font_size: default_font_size(),
            font_family: default_font_family(),
            line_height: default_line_height(),
        }
    }
}

// ---------------------------------------------------------------------------
// Editor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorConfig {
    /// Tab width in spaces
    #[serde(default = "default_tab_width")]
    pub tab_width: usize,
    /// Use spaces instead of tabs
    #[serde(default = "default_true")]
    pub use_spaces: bool,
    /// Show line numbers gutter
    #[serde(default = "default_true")]
    pub line_numbers: bool,
    /// Show git status gutter
    #[serde(default = "default_true")]
    pub git_gutter: bool,
    /// Enable multi-cursor editing
    #[serde(default = "default_true")]
    pub multi_cursor: bool,
    /// Max undo depth
    #[serde(default = "default_undo_depth")]
    pub max_undo_depth: usize,
    /// Wrap lines at viewport width
    #[serde(default)]
    pub word_wrap: bool,
    /// Enable bracket matching
    #[serde(default = "default_true")]
    pub bracket_matching: bool,
    /// Enable auto-pair brackets/quotes
    #[serde(default = "default_true")]
    pub auto_pairs: bool,
}

fn default_tab_width() -> usize {
    4
}
fn default_true() -> bool {
    true
}
fn default_undo_depth() -> usize {
    10_000
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            tab_width: default_tab_width(),
            use_spaces: true,
            line_numbers: true,
            git_gutter: true,
            multi_cursor: true,
            max_undo_depth: default_undo_depth(),
            word_wrap: false,
            bracket_matching: true,
            auto_pairs: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Keybindings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindingsConfig {
    /// "chord" | "vim" | "emacs"
    #[serde(default = "default_keybind_mode")]
    pub mode: String,
    /// User overrides: action -> key sequence
    #[serde(default)]
    pub overrides: HashMap<String, String>,
}

fn default_keybind_mode() -> String {
    "chord".into()
}

impl Default for KeybindingsConfig {
    fn default() -> Self {
        Self {
            mode: default_keybind_mode(),
            overrides: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// AI
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    /// Default model hint: "auto" or explicit provider/model
    #[serde(default = "default_auto")]
    pub default_model: String,
    /// Fallback chain priority list
    #[serde(default = "default_fallback_chain")]
    pub fallback_chain: Vec<String>,
    /// Enable local models
    #[serde(default = "default_true")]
    pub local_enabled: bool,
    /// Enable cloud providers (requires opt-in)
    #[serde(default)]
    pub cloud_enabled: bool,
    /// Max tokens for AI context
    #[serde(default = "default_max_context")]
    pub max_context_tokens: usize,
    /// Inline completion debounce ms
    #[serde(default = "default_inline_debounce")]
    pub inline_debounce_ms: u64,
    /// Sticky session TTL in minutes
    #[serde(default = "default_sticky_ttl")]
    pub sticky_session_ttl_minutes: u64,
    /// FreeLLMAPI sidecar configuration
    #[serde(default)]
    pub sidecar: SidecarConfig,
    /// Per-project AI settings
    #[serde(default)]
    pub per_project: HashMap<String, PerProjectAiConfig>,
}

/// Configuration for the FreeLLMAPI sidecar process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarConfig {
    /// Enable/disable the sidecar (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Auto-start the sidecar on editor launch (default: true)
    #[serde(default = "default_true")]
    pub auto_start: bool,
    /// Path to the FreeLLMAPI repo (relative to workspace root)
    #[serde(default = "default_freellmapi_path")]
    pub freellmapi_path: String,
    /// Preferred port for the sidecar (0 = auto-assign)
    #[serde(default)]
    pub port: u16,
    /// FreeLLMAPI model to use (default: "auto" for router selection)
    #[serde(default = "default_auto")]
    pub model: String,
}

fn default_freellmapi_path() -> String {
    "sidecar/freellmapi".into()
}

impl Default for SidecarConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_start: true,
            freellmapi_path: default_freellmapi_path(),
            port: 3001,
            model: default_auto(),
        }
    }
}

fn default_auto() -> String {
    "auto".into()
}
fn default_fallback_chain() -> Vec<String> {
    vec![
        "local/llama-3.2".into(),
        "groq/llama-3.3-70b".into(),
        "cerebras/qwen3".into(),
        "gemini/gemini-2.5-flash".into(),
    ]
}
fn default_max_context() -> usize {
    8_192
}
fn default_inline_debounce() -> u64 {
    200
}
fn default_sticky_ttl() -> u64 {
    30
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            default_model: default_auto(),
            fallback_chain: default_fallback_chain(),
            local_enabled: true,
            cloud_enabled: false,
            max_context_tokens: default_max_context(),
            inline_debounce_ms: default_inline_debounce(),
            sticky_session_ttl_minutes: default_sticky_ttl(),
            sidecar: SidecarConfig::default(),
            per_project: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerProjectAiConfig {
    pub cloud_enabled: Option<bool>,
    pub default_model: Option<String>,
    pub fallback_chain: Option<Vec<String>>,
    pub allowed_providers: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// LSP
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspConfig {
    #[serde(default = "default_lsp_debounce")]
    pub debounce_ms: u64,
    #[serde(default)]
    pub server_paths: HashMap<String, String>,
}

fn default_lsp_debounce() -> u64 {
    150
}

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            debounce_ms: default_lsp_debounce(),
            server_paths: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Terminal
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalConfig {
    #[serde(default = "default_terminal_shell")]
    pub shell: String,
    #[serde(default = "default_scrollback_lines")]
    pub scrollback_lines: usize,
}

fn default_terminal_shell() -> String {
    if cfg!(target_os = "windows") {
        "cmd".into()
    } else {
        "bash".into()
    }
}
fn default_scrollback_lines() -> usize {
    10_000
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            shell: default_terminal_shell(),
            scrollback_lines: default_scrollback_lines(),
        }
    }
}

// ---------------------------------------------------------------------------
// Plugins
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    #[serde(default = "default_plugin_dir")]
    pub plugin_dir: String,
    #[serde(default)]
    pub enabled_plugins: Vec<String>,
}

fn default_plugin_dir() -> String {
    "~/.config/aurora/plugins".into()
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            plugin_dir: default_plugin_dir(),
            enabled_plugins: Vec::new(),
        }
    }
}
