//! Variable scoping and `{{variable}}` / `{{vault:...}}` resolution.
//!
//! Resolution is deliberately split into two stages that must never be
//! collapsed into one:
//!
//! 1. [`interpolate`] — ordinary `{{var}}` substitution from merged
//!    environment/collection/global scopes. Safe to run anywhere,
//!    including for a UI preview, since it leaves `{{vault:...}}`
//!    references untouched.
//! 2. [`resolve_vault`] — the *only* place a vault secret's real value is
//!    read. Per spec section 9's security boundary, this must run only
//!    inside the Rust engine, only at actual send time, after any
//!    pre-request script has already finished running. [`resolve_for_send`]
//!    chains both stages in the correct order for that call site.

use indexmap::IndexMap;

/// Merges variable scopes in increasing priority — e.g.
/// `merge_scopes(&[&global, &collection, &environment])` — so a later
/// layer overrides a key set by an earlier one, matching the
/// global/collection/environment scoping described in spec section 5.
pub fn merge_scopes(layers: &[&IndexMap<String, String>]) -> IndexMap<String, String> {
    let mut merged = IndexMap::new();
    for layer in layers {
        for (k, v) in layer.iter() {
            merged.insert(k.clone(), v.clone());
        }
    }
    merged
}

/// Scans `input` for `{{...}}` placeholders, replacing each with
/// `resolve(key)` when it returns `Some`, or leaving the placeholder
/// (`{{key}}`) untouched when it returns `None` — so an unresolved
/// reference stays visible rather than silently becoming an empty string.
fn replace_placeholders(input: &str, mut resolve: impl FnMut(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        let Some(end) = after_open.find("}}") else {
            // Unterminated `{{` — emit the rest verbatim and stop.
            out.push_str(&rest[start..]);
            return out;
        };
        let key = after_open[..end].trim();
        match resolve(key) {
            Some(value) => out.push_str(&value),
            None => {
                out.push_str("{{");
                out.push_str(key);
                out.push_str("}}");
            }
        }
        rest = &after_open[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Stage 1: substitutes `{{var}}` from `vars`. `{{vault:...}}` references
/// are always left untouched here, even if `vars` happens to contain a key
/// literally named `vault:something` — that prefix is reserved.
pub fn interpolate(input: &str, vars: &IndexMap<String, String>) -> String {
    replace_placeholders(input, |key| {
        if key.starts_with("vault:") {
            None
        } else {
            vars.get(key).cloned()
        }
    })
}

/// Stage 2: substitutes `{{vault:key}}` from `vault` (a parsed
/// `.apienv.vault`). Only call this at actual send time — see the module
/// docs and spec section 9.
pub fn resolve_vault(input: &str, vault: &IndexMap<String, String>) -> String {
    replace_placeholders(input, |key| {
        key.strip_prefix("vault:").and_then(|k| vault.get(k)).cloned()
    })
}

/// Runs both stages in order. This is what request-sending code should
/// call; nothing upstream of the actual send (UI previews, scripts) should
/// ever have `vault` in scope to pass here.
pub fn resolve_for_send(input: &str, vars: &IndexMap<String, String>, vault: &IndexMap<String, String>) -> String {
    resolve_vault(&interpolate(input, vars), vault)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_known_vars() {
        let mut vars = IndexMap::new();
        vars.insert("base_url".to_string(), "https://api.local.dev".to_string());
        vars.insert("user_id".to_string(), "42".to_string());
        let out = interpolate("{{base_url}}/users/{{user_id}}", &vars);
        assert_eq!(out, "https://api.local.dev/users/42");
    }

    #[test]
    fn leaves_vault_refs_untouched_during_interpolate() {
        let vars = IndexMap::new();
        let out = interpolate("Bearer {{vault:api_key}}", &vars);
        assert_eq!(out, "Bearer {{vault:api_key}}");
    }

    #[test]
    fn leaves_unresolved_vars_visible() {
        let vars = IndexMap::new();
        let out = interpolate("{{missing}}", &vars);
        assert_eq!(out, "{{missing}}");
    }

    #[test]
    fn resolve_vault_substitutes_only_vault_refs() {
        let mut vault = IndexMap::new();
        vault.insert("api_key".to_string(), "sk-live-abc123".to_string());
        let out = resolve_vault("Bearer {{vault:api_key}}, id {{user_id}}", &vault);
        assert_eq!(out, "Bearer sk-live-abc123, id {{user_id}}");
    }

    #[test]
    fn resolve_for_send_chains_both_stages() {
        let mut vars = IndexMap::new();
        vars.insert("api_key_ref".to_string(), "{{vault:api_key}}".to_string());
        let mut vault = IndexMap::new();
        vault.insert("api_key".to_string(), "sk-live-abc123".to_string());

        // A var whose value is itself a vault reference: interpolate()
        // substitutes the var, then resolve_vault() resolves what it
        // exposed. Confirms the two stages compose instead of only
        // handling `{{vault:...}}` written directly in the source text.
        let out = resolve_for_send("Bearer {{api_key_ref}}", &vars, &vault);
        assert_eq!(out, "Bearer sk-live-abc123");
    }

    #[test]
    fn merge_scopes_lets_later_layers_override_earlier() {
        let mut global = IndexMap::new();
        global.insert("base_url".to_string(), "https://global.example".to_string());
        global.insert("timeout".to_string(), "30".to_string());
        let mut env = IndexMap::new();
        env.insert("base_url".to_string(), "https://api.local.dev".to_string());

        let merged = merge_scopes(&[&global, &env]);
        assert_eq!(merged.get("base_url").unwrap(), "https://api.local.dev");
        assert_eq!(merged.get("timeout").unwrap(), "30");
    }
}
