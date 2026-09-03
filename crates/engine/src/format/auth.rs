//! Request-level auth (spec sections 5 and 10: Auth tab, modes
//! None/Inherit/Basic/Bearer/API Key/OAuth2), stored as `.apireq` blocks:
//!
//! ```text
//! auth {
//!   mode: bearer
//! }
//!
//! auth:bearer {
//!   token: {{vault:access_token}}
//! }
//! ```
//!
//! Field values are ordinary strings that may contain `{{var}}` /
//! `{{vault:...}}` references, resolved the same way as headers/body via
//! [`Auth::resolve`] — never earlier, per the send-time-only vault rule in
//! `crate::vars`.

use indexmap::IndexMap;

use super::blocks::render_key_value_block;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiKeyPlacement {
    Header,
    Query,
}

impl ApiKeyPlacement {
    fn as_str(&self) -> &'static str {
        match self {
            ApiKeyPlacement::Header => "header",
            ApiKeyPlacement::Query => "query",
        }
    }

    /// Defaults to `Header` for anything other than an explicit `query` —
    /// a missing/unrecognized placement shouldn't silently start sending
    /// the key in a URL (more likely to be logged/cached) instead of a
    /// header.
    pub fn from_field(s: Option<&str>) -> Self {
        match s {
            Some(s) if s.eq_ignore_ascii_case("query") => ApiKeyPlacement::Query,
            _ => ApiKeyPlacement::Header,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuth2Config {
    pub grant_type: String,
    pub auth_url: String,
    pub access_token_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub scope: String,
    pub redirect_uri: String,
    /// A previously obtained token — pasted manually today, or filled in
    /// by a cached/refreshed token once an interactive flow exists.
    /// Applied as a Bearer header at send time; empty means no
    /// Authorization header is added.
    ///
    /// Driving an actual grant flow (browser redirect, local callback
    /// listener, PKCE, refresh-on-expiry) needs a UI to host it and isn't
    /// implemented yet — this covers "I already have a token" today.
    pub access_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Auth {
    #[default]
    None,
    /// Use whatever auth the parent collection defines --
    /// `crate::collection::resolve_inherited_auth` does the actual lookup
    /// against a loaded `CollectionFile`'s auth before `.resolve()` /
    /// `http::apply_auth` ever see it. A caller that never resolves
    /// inheritance (or has no collection loaded) sends this as `None`,
    /// since `apply_auth` treats an unresolved `Inherit` that way. Only
    /// collection-level inheritance exists so far -- per-folder `.folder`
    /// overrides (spec section 4) aren't implemented yet.
    Inherit,
    Basic { username: String, password: String },
    Bearer { token: String },
    ApiKey { key: String, value: String, placement: ApiKeyPlacement },
    OAuth2(OAuth2Config),
}

impl Auth {
    pub fn mode_str(&self) -> &'static str {
        match self {
            Auth::None => "none",
            Auth::Inherit => "inherit",
            Auth::Basic { .. } => "basic",
            Auth::Bearer { .. } => "bearer",
            Auth::ApiKey { .. } => "apikey",
            Auth::OAuth2(_) => "oauth2",
        }
    }

    pub(super) fn from_parts(
        mode: Option<&str>,
        basic: Option<IndexMap<String, String>>,
        bearer: Option<IndexMap<String, String>>,
        apikey: Option<IndexMap<String, String>>,
        oauth2: Option<IndexMap<String, String>>,
    ) -> Result<Auth, String> {
        Ok(match mode {
            None | Some("none") => Auth::None,
            Some("inherit") => Auth::Inherit,
            Some("basic") => {
                let kv = basic.ok_or("mode: basic requires an 'auth:basic' block")?;
                Auth::Basic {
                    username: kv.get("username").cloned().unwrap_or_default(),
                    password: kv.get("password").cloned().unwrap_or_default(),
                }
            }
            Some("bearer") => {
                let kv = bearer.ok_or("mode: bearer requires an 'auth:bearer' block")?;
                Auth::Bearer {
                    token: kv.get("token").cloned().unwrap_or_default(),
                }
            }
            Some("apikey") => {
                let kv = apikey.ok_or("mode: apikey requires an 'auth:apikey' block")?;
                Auth::ApiKey {
                    key: kv.get("key").cloned().unwrap_or_default(),
                    value: kv.get("value").cloned().unwrap_or_default(),
                    placement: ApiKeyPlacement::from_field(kv.get("placement").map(|s| s.as_str())),
                }
            }
            Some("oauth2") => {
                let kv = oauth2.ok_or("mode: oauth2 requires an 'auth:oauth2' block")?;
                Auth::OAuth2(OAuth2Config {
                    grant_type: kv.get("grant_type").cloned().unwrap_or_default(),
                    auth_url: kv.get("auth_url").cloned().unwrap_or_default(),
                    access_token_url: kv.get("access_token_url").cloned().unwrap_or_default(),
                    client_id: kv.get("client_id").cloned().unwrap_or_default(),
                    client_secret: kv.get("client_secret").cloned().unwrap_or_default(),
                    scope: kv.get("scope").cloned().unwrap_or_default(),
                    redirect_uri: kv.get("redirect_uri").cloned().unwrap_or_default(),
                    access_token: kv.get("access_token").cloned().unwrap_or_default(),
                })
            }
            Some(other) => return Err(format!("unknown auth mode '{other}'")),
        })
    }

    /// Renders this auth config back into `.apireq` blocks (the `auth {}`
    /// mode selector plus its typed `auth:<mode>` detail block), or
    /// nothing for `None`/`Inherit`.
    pub(super) fn render_blocks(&self) -> Vec<String> {
        if matches!(self, Auth::None) {
            return Vec::new();
        }

        let mut mode_kv = IndexMap::new();
        mode_kv.insert("mode".to_string(), self.mode_str().to_string());
        let mut out = vec![render_key_value_block("auth", &mode_kv).unwrap()];

        let detail_kv: Option<IndexMap<String, String>> = match self {
            Auth::None | Auth::Inherit => None,
            Auth::Basic { username, password } => {
                let mut kv = IndexMap::new();
                kv.insert("username".to_string(), username.clone());
                kv.insert("password".to_string(), password.clone());
                Some(kv)
            }
            Auth::Bearer { token } => {
                let mut kv = IndexMap::new();
                kv.insert("token".to_string(), token.clone());
                Some(kv)
            }
            Auth::ApiKey { key, value, placement } => {
                let mut kv = IndexMap::new();
                kv.insert("key".to_string(), key.clone());
                kv.insert("value".to_string(), value.clone());
                kv.insert("placement".to_string(), placement.as_str().to_string());
                Some(kv)
            }
            Auth::OAuth2(cfg) => {
                let mut kv = IndexMap::new();
                kv.insert("grant_type".to_string(), cfg.grant_type.clone());
                kv.insert("auth_url".to_string(), cfg.auth_url.clone());
                kv.insert("access_token_url".to_string(), cfg.access_token_url.clone());
                kv.insert("client_id".to_string(), cfg.client_id.clone());
                kv.insert("client_secret".to_string(), cfg.client_secret.clone());
                kv.insert("scope".to_string(), cfg.scope.clone());
                kv.insert("redirect_uri".to_string(), cfg.redirect_uri.clone());
                kv.insert("access_token".to_string(), cfg.access_token.clone());
                Some(kv)
            }
        };

        if let Some(kv) = detail_kv {
            if let Some(block) = render_key_value_block(&format!("auth:{}", self.mode_str()), &kv) {
                out.push(block);
            }
        }

        out
    }

    /// Resolves every `{{var}}` / `{{vault:...}}` reference across this
    /// auth config's fields. Call only at actual send time — same rule as
    /// `crate::vars::resolve_for_send`, which this delegates to.
    pub fn resolve(&self, vars: &IndexMap<String, String>, vault: &IndexMap<String, String>) -> Auth {
        let r = |s: &str| crate::vars::resolve_for_send(s, vars, vault);
        match self {
            Auth::None => Auth::None,
            Auth::Inherit => Auth::Inherit,
            Auth::Basic { username, password } => Auth::Basic {
                username: r(username),
                password: r(password),
            },
            Auth::Bearer { token } => Auth::Bearer { token: r(token) },
            Auth::ApiKey { key, value, placement } => Auth::ApiKey {
                key: r(key),
                value: r(value),
                placement: placement.clone(),
            },
            Auth::OAuth2(cfg) => Auth::OAuth2(OAuth2Config {
                grant_type: cfg.grant_type.clone(),
                auth_url: r(&cfg.auth_url),
                access_token_url: r(&cfg.access_token_url),
                client_id: r(&cfg.client_id),
                client_secret: r(&cfg.client_secret),
                scope: cfg.scope.clone(),
                redirect_uri: r(&cfg.redirect_uri),
                access_token: r(&cfg.access_token),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_placement_defaults_to_header() {
        assert_eq!(ApiKeyPlacement::from_field(None), ApiKeyPlacement::Header);
        assert_eq!(ApiKeyPlacement::from_field(Some("bogus")), ApiKeyPlacement::Header);
        assert_eq!(ApiKeyPlacement::from_field(Some("QUERY")), ApiKeyPlacement::Query);
    }

    #[test]
    fn resolve_substitutes_vars_and_vault_in_basic_auth() {
        let mut vars = IndexMap::new();
        vars.insert("username".to_string(), "alice".to_string());
        let mut vault = IndexMap::new();
        vault.insert("password".to_string(), "hunter2".to_string());

        let auth = Auth::Basic {
            username: "{{username}}".to_string(),
            password: "{{vault:password}}".to_string(),
        };
        let resolved = auth.resolve(&vars, &vault);
        assert_eq!(
            resolved,
            Auth::Basic {
                username: "alice".to_string(),
                password: "hunter2".to_string()
            }
        );
    }

    #[test]
    fn resolve_leaves_none_and_inherit_untouched() {
        let empty = IndexMap::new();
        assert_eq!(Auth::None.resolve(&empty, &empty), Auth::None);
        assert_eq!(Auth::Inherit.resolve(&empty, &empty), Auth::Inherit);
    }

    #[test]
    fn from_parts_rejects_unknown_mode() {
        assert!(Auth::from_parts(Some("carrier-pigeon"), None, None, None, None).is_err());
    }
}
