//! Global/user settings (spec section 11, tier 1): theme, layout preset,
//! and panel visibility, persisted as a local TOML file. "Never synced" --
//! deliberately outside any Git-tracked collection folder, same as
//! response history. Doesn't (yet) cover the other tier-1 fields the spec
//! lists (update-check preference, AI model choice, keyboard shortcuts)
//! since none of those features exist yet either; `#[serde(default)]`
//! throughout means adding fields later won't break reading an
//! already-written config.toml.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelVisibility {
    pub headers: bool,
    pub auth: bool,
    pub body: bool,
}

impl Default for PanelVisibility {
    fn default() -> Self {
        PanelVisibility { headers: true, auth: true, body: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: String,
    pub layout_preset: String,
    pub panels: PanelVisibility,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: "light".to_string(),
            layout_preset: "stacked".to_string(),
            panels: PanelVisibility::default(),
        }
    }
}

/// A missing file (first run) isn't an error -- it just means defaults.
pub fn load(path: &Path) -> Result<Settings, String> {
    match std::fs::read_to_string(path) {
        Ok(contents) => toml::from_str(&contents).map_err(|e| format!("couldn't parse {}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(format!("couldn't read {}: {e}", path.display())),
    }
}

pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let toml_str = toml::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(path, toml_str).map_err(|e| format!("couldn't write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults() {
        let dir = std::env::temp_dir().join(format!("fluxchunk-settings-test-{}", std::process::id()));
        let path = dir.join("does-not-exist.toml");
        let settings = load(&path).unwrap();
        assert_eq!(settings.theme, "light");
        assert_eq!(settings.layout_preset, "stacked");
    }

    #[test]
    fn round_trips_through_save_and_load() {
        let dir = std::env::temp_dir().join(format!("fluxchunk-settings-test-rt-{}", std::process::id()));
        let path = dir.join("config.toml");
        let mut settings = Settings::default();
        settings.theme = "silver".to_string();
        settings.layout_preset = "split".to_string();
        settings.panels.body = false;

        save(&path, &settings).unwrap();
        let loaded = load(&path).unwrap();

        assert_eq!(loaded.theme, "silver");
        assert_eq!(loaded.layout_preset, "split");
        assert!(!loaded.panels.body);
        assert!(loaded.panels.headers);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
