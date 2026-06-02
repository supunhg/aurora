use crate::types::AuroraConfig;

/// Validates a TOML string against the Aurora config schema.
/// Returns deserialized config on success, or a list of error messages on failure.
pub fn validate_toml(input: &str) -> Result<AuroraConfig, Vec<String>> {
    let mut errors = Vec::new();

    match toml::from_str::<AuroraConfig>(input) {
        Ok(config) => {
            // Semantic validation
            if config.theme.font_size <= 0.0 {
                errors.push("theme.font_size must be positive".into());
            }
            if config.editor.max_undo_depth == 0 {
                errors.push("editor.max_undo_depth must be > 0".into());
            }
            if config.ai.inline_debounce_ms < 50 {
                errors.push("ai.inline_debounce_ms must be >= 50ms".into());
            }
            if config.lsp.debounce_ms < 50 {
                errors.push("lsp.debounce_ms must be >= 50ms".into());
            }
            match config.theme.mode.as_str() {
                "dark" | "light" | "auto" => {}
                other => errors.push(format!(
                    "theme.mode must be 'dark', 'light', or 'auto', got '{}'",
                    other
                )),
            }
            match config.keybindings.mode.as_str() {
                "chord" | "vim" | "emacs" => {}
                other => errors.push(format!(
                    "keybindings.mode must be 'chord', 'vim', or 'emacs', got '{}'",
                    other
                )),
            }

            if errors.is_empty() {
                Ok(config)
            } else {
                Err(errors)
            }
        }
        Err(e) => {
            errors.push(format!("TOML parse error: {}", e));
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_default_config() {
        let toml = r#"
            [theme]
            mode = "dark"

            [editor]
            tab_width = 4

            [ai]
            default_model = "auto"
        "#;
        let result = validate_toml(toml);
        assert!(
            result.is_ok(),
            "Should accept valid partial config: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_invalid_font_size() {
        let toml = r#"
            [theme]
            font_size = -1
        "#;
        let result = validate_toml(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_theme_mode() {
        let toml = r#"
            [theme]
            mode = "solarized"
        "#;
        let result = validate_toml(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_config_uses_defaults() {
        let toml = "";
        let result = validate_toml(toml);
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.theme.mode, "dark");
        assert_eq!(config.editor.tab_width, 4);
        assert_eq!(config.ai.default_model, "auto");
    }
}
