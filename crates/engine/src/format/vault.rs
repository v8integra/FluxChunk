//! `.apienv.vault` — machine-local secrets, gitignored, never synced or
//! exported (spec section 4). Flat `key: value` lines, no block wrapper —
//! deliberately the simplest possible format since this file never needs
//! to diff cleanly (it's never committed) and never holds anything but
//! secrets.
//!
//! This type exists only so the engine can resolve `{{vault:key}}` at
//! actual send time (see `crate::vars::resolve_vault`). Nothing else in
//! the app — least of all the scripting sandbox — should ever get a
//! reference to a parsed `VaultFile`.

use super::blocks::parse_key_value_lines;
use indexmap::IndexMap;

#[derive(Clone, Default)]
pub struct VaultFile {
    pub secrets: IndexMap<String, String>,
}

impl VaultFile {
    pub fn parse(input: &str) -> Self {
        VaultFile {
            secrets: parse_key_value_lines(input),
        }
    }

    pub fn to_string_pretty(&self) -> String {
        let mut out = String::new();
        for (k, v) in &self.secrets {
            out.push_str(&format!("{k}: {v}\n"));
        }
        out
    }
}

// Deliberately no `Debug`/`Display` impl: printing a `VaultFile` (e.g. via
// `{:?}` in a stray `dbg!()` or log line) would put every secret in the
// process's stdout/log. Anything that legitimately needs to inspect a
// value must go through `secrets` explicitly, at the call site, on
// purpose.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flat_key_value_lines() {
        let vault = VaultFile::parse("api_key: sk-live-abc123\ndb_password: hunter2\n");
        assert_eq!(vault.secrets.get("api_key").unwrap(), "sk-live-abc123");
        assert_eq!(vault.secrets.get("db_password").unwrap(), "hunter2");
    }

    #[test]
    fn round_trips() {
        let vault = VaultFile::parse("api_key: sk-live-abc123\n");
        let rendered = vault.to_string_pretty();
        assert_eq!(VaultFile::parse(&rendered).secrets, vault.secrets);
    }
}
