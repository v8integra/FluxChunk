//! Persists the local peer identity (spec section 6) to
//! `%APPDATA%\FluxChunk\identity.toml` -- outside any Git-tracked
//! collection folder, same as `settings.rs` and response history,
//! since this is a per-install identity, not something to sync or
//! commit. All the actual crypto lives in
//! `fluxchunk_engine::identity`; this module only adds the "generate
//! once, persist, reload on every later launch" policy spec section 6's
//! "generated on first install" describes.

use std::path::Path;

use fluxchunk_engine::identity::LocalIdentity;

/// Loads the existing identity, or generates and persists a fresh one
/// if this is the first launch. Deliberately does *not* regenerate on
/// every call -- "generated on first install" means once, ever, for
/// this install; anything else would silently invalidate this
/// identity's standing as an `.apiworkspace` approver every time it
/// happened.
pub fn load_or_create(path: &Path) -> Result<LocalIdentity, String> {
    match std::fs::read_to_string(path) {
        Ok(contents) => toml::from_str(&contents).map_err(|e| format!("couldn't parse {}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let identity = LocalIdentity::generate(default_display_name());
            save(path, &identity)?;
            Ok(identity)
        }
        Err(e) => Err(format!("couldn't read {}: {e}", path.display())),
    }
}

pub fn save(path: &Path, identity: &LocalIdentity) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let toml_str = toml::to_string_pretty(identity).map_err(|e| e.to_string())?;
    std::fs::write(path, toml_str).map_err(|e| format!("couldn't write {}: {e}", path.display()))
}

fn default_display_name() -> String {
    std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "Anonymous".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_generates_and_persists_a_fresh_identity() {
        let dir = std::env::temp_dir().join(format!("fluxchunk-identity-test-new-{}", std::process::id()));
        let path = dir.join("identity.toml");

        let generated = load_or_create(&path).unwrap();
        assert!(!generated.public_key.is_empty());
        assert!(!generated.secret_key.is_empty());
        assert!(path.exists());

        // Loading again must return the *same* identity, not a new one.
        let reloaded = load_or_create(&path).unwrap();
        assert_eq!(generated, reloaded);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_and_load_round_trips() {
        let dir = std::env::temp_dir().join(format!("fluxchunk-identity-test-rt-{}", std::process::id()));
        let path = dir.join("identity.toml");

        let identity = LocalIdentity::generate("Alice");
        save(&path, &identity).unwrap();
        let loaded = load_or_create(&path).unwrap();
        assert_eq!(identity, loaded);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn display_name_can_be_changed_without_touching_the_keypair() {
        let dir = std::env::temp_dir().join(format!("fluxchunk-identity-test-rename-{}", std::process::id()));
        let path = dir.join("identity.toml");

        let mut identity = load_or_create(&path).unwrap();
        let original_public_key = identity.public_key.clone();
        identity.display_name = "New Name".to_string();
        save(&path, &identity).unwrap();

        let reloaded = load_or_create(&path).unwrap();
        assert_eq!(reloaded.display_name, "New Name");
        assert_eq!(reloaded.public_key, original_public_key);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
