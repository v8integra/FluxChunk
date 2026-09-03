//! `{{variable}}` interpolation.
//!
//! `{{vault:...}}` references are deliberately left untouched here — per
//! spec section 9, vault secrets resolve only inside the Rust engine at
//! actual send time, in a dedicated step scripts cannot observe. This
//! function is used ahead of that step (e.g. to preview a resolved URL in
//! the UI), so it must never be the thing that resolves vault values.

use indexmap::IndexMap;

pub fn interpolate(input: &str, vars: &IndexMap<String, String>) -> String {
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
        if key.starts_with("vault:") {
            out.push_str("{{");
            out.push_str(key);
            out.push_str("}}");
        } else if let Some(value) = vars.get(key) {
            out.push_str(value);
        } else {
            // Unresolved variable: leave the placeholder as-is so it's
            // visible in the UI rather than silently becoming "".
            out.push_str("{{");
            out.push_str(key);
            out.push_str("}}");
        }
        rest = &after_open[end + 2..];
    }
    out.push_str(rest);
    out
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
    fn leaves_vault_refs_untouched() {
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
}
