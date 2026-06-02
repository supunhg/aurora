use crate::schema::validate_toml;
use crate::types::AuroraConfig;
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;

/// Loads and watches Aurora configuration from disk.
/// Supports global config + per-project override.
pub struct ConfigLoader {
    global_path: PathBuf,
    project_path: Option<PathBuf>,
    cached: Arc<RwLock<AuroraConfig>>,
}

impl ConfigLoader {
    /// Create a new config loader for the given project root.
    /// If `project_root` is None, only global config is loaded.
    pub fn new(project_root: Option<PathBuf>) -> Self {
        let global_path = Self::global_config_path();

        let project_path = project_root.map(|root| root.join(".aurora.toml"));

        let merged = Self::load_and_merge(&global_path, &project_path).unwrap_or_else(|e| {
            eprintln!("Config warning: {:?}, using defaults", e);
            AuroraConfig::default()
        });
        let cached = Arc::new(RwLock::new(merged));

        ConfigLoader {
            global_path,
            project_path,
            cached,
        }
    }

    /// Get the current merged configuration.
    pub fn get(&self) -> AuroraConfig {
        self.cached.read().clone()
    }

    /// Reload configuration from disk, merging global + project settings.
    pub fn reload(&self) -> Result<(), Vec<String>> {
        let merged = Self::load_and_merge(&self.global_path, &self.project_path)?;
        *self.cached.write() = merged;
        Ok(())
    }

    /// Create a shared handle for passing around the application.
    pub fn handle(&self) -> ConfigHandle {
        ConfigHandle {
            inner: self.cached.clone(),
        }
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    fn global_config_path() -> PathBuf {
        let home = dirs_or_default();
        let mut path = PathBuf::from(home);
        path.push(".config");
        path.push("aurora");
        path.push("aurora.toml");
        path
    }

    fn load_and_merge(
        global_path: &PathBuf,
        project_path: &Option<PathBuf>,
    ) -> Result<AuroraConfig, Vec<String>> {
        let global = Self::load_file(global_path).unwrap_or_default();
        let project = project_path
            .as_ref()
            .and_then(|p| Self::load_file(p).ok())
            .unwrap_or_default();

        Ok(merge_configs(global, project))
    }

    fn load_file(path: &PathBuf) -> Result<AuroraConfig, Vec<String>> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Err(vec![format!("Cannot read config file: {:?}", path)]),
        };
        validate_toml(&content)
    }
}

/// Thread-safe handle to the config for passing to subsystems.
#[derive(Clone)]
pub struct ConfigHandle {
    inner: Arc<RwLock<AuroraConfig>>,
}

impl ConfigHandle {
    pub fn get(&self) -> AuroraConfig {
        self.inner.read().clone()
    }
}

/// Merge two configs. Project-level values override global-level values.
fn merge_configs(global: AuroraConfig, project: AuroraConfig) -> AuroraConfig {
    // For now: project wins on all fields that are set (non-default).
    // A more sophisticated merge would use Option<T> to detect "not set".
    let theme = if project.theme.mode != "dark" || project.theme.font_size != 14.0 {
        project.theme.clone()
    } else {
        global.theme.clone()
    };

    AuroraConfig {
        theme,
        editor: project.editor.clone(),
        keybindings: project.keybindings.clone(),
        ai: project.ai.clone(),
        lsp: project.lsp.clone(),
        terminal: project.terminal.clone(),
        plugins: project.plugins.clone(),
    }
}

fn dirs_or_default() -> String {
    if let Ok(home) = std::env::var("HOME") {
        home
    } else if let Ok(home) = std::env::var("USERPROFILE") {
        home
    } else {
        "/tmp".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loader_creates_with_defaults() {
        let loader = ConfigLoader::new(None);
        let config = loader.get();
        assert_eq!(config.theme.mode, "dark");
        assert_eq!(config.editor.tab_width, 4);
    }

    #[test]
    fn test_merge_global_and_project() {
        let global = AuroraConfig::default();
        let mut project = AuroraConfig::default();
        project.editor.tab_width = 8;
        project.ai.default_model = "groq/llama3".into();

        let merged = merge_configs(global, project);
        assert_eq!(merged.editor.tab_width, 8);
        assert_eq!(merged.ai.default_model, "groq/llama3");
    }
}
