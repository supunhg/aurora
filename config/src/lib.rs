pub mod loader;
pub mod schema;
pub mod types;

pub use loader::ConfigLoader;
pub use schema::validate_toml;
pub use types::{
    AiConfig, AuroraConfig, EditorConfig, KeybindingsConfig, SidecarConfig, ThemeConfig,
};
