//! Thin wrapper around `reqwest` that turns a parsed `.apireq` into an
//! actual request/response cycle. Scripting (pre-request/post-response) and
//! the vault secrets boundary are separate concerns layered on top later
//! (see spec sections 4 and 9) — this module only knows about the wire.

use std::collections::HashSet;
use std::time::Instant;

use indexmap::IndexMap;
pub use reqwest::{Method, StatusCode};

use crate::error::EngineError;
use crate::format::{ApiKeyPlacement, ApiRequestFile, Auth, Body};

#[derive(Debug, Clone)]
pub struct OutgoingRequest {
    pub method: Method,
    pub url: String,
    pub headers: IndexMap<String, String>,
    /// Already `Auth::resolve()`d — see that method's doc comment for the
    /// send-time-only rule this must follow.
    pub auth: Auth,
    pub body: Option<OutgoingBody>,
}

#[derive(Debug, Clone)]
pub enum OutgoingBody {
    Json(String),
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct ResponseSummary {
    pub status: StatusCode,
    pub headers: IndexMap<String, String>,
    pub body: Vec<u8>,
    pub elapsed_ms: u128,
}

impl ResponseSummary {
    pub fn body_as_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

pub struct HttpClient {
    client: reqwest::Client,
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    pub async fn send(&self, req: OutgoingRequest) -> Result<ResponseSummary, EngineError> {
        let started = Instant::now();

        // Auth first, explicit headers second: an explicit header of the
        // same name (e.g. the user typed their own `Authorization` line)
        // wins rather than the two silently stacking into two header
        // lines for the same field.
        let explicit_header_keys: HashSet<String> = req.headers.keys().map(|k| k.to_ascii_lowercase()).collect();
        let mut builder = apply_auth(
            self.client.request(req.method, &req.url),
            &req.auth,
            &explicit_header_keys,
        );

        for (key, value) in &req.headers {
            builder = builder.header(key, value);
        }
        builder = match req.body {
            Some(OutgoingBody::Json(s)) => builder
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(s),
            Some(OutgoingBody::Text(s)) => builder.body(s),
            Some(OutgoingBody::Bytes(b)) => builder.body(b),
            None => builder,
        };

        let response = builder.send().await?;
        let status = response.status();
        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
            .collect();
        let body = response.bytes().await?.to_vec();

        Ok(ResponseSummary {
            status,
            headers,
            body,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }
}

/// Applies a resolved `Auth` to the request builder. `Basic`/`Bearer` use
/// reqwest's own helpers (battle-tested header encoding, no hand-rolled
/// base64); `ApiKey` sets a header or query param directly; `OAuth2` sends
/// its cached token as a Bearer header when one is present. Skips setting
/// anything `explicit_header_keys` (from the request's own `headers {}`
/// block) already covers, so an explicit header always wins over an
/// auth-derived one instead of both being sent.
fn apply_auth(builder: reqwest::RequestBuilder, auth: &Auth, explicit_header_keys: &HashSet<String>) -> reqwest::RequestBuilder {
    let has_explicit_authorization = explicit_header_keys.contains("authorization");
    match auth {
        Auth::None | Auth::Inherit => builder,
        Auth::Basic { username, password } => {
            if has_explicit_authorization {
                builder
            } else {
                builder.basic_auth(username, Some(password.clone()))
            }
        }
        Auth::Bearer { token } => {
            if has_explicit_authorization {
                builder
            } else {
                builder.bearer_auth(token)
            }
        }
        Auth::ApiKey { key, value, placement } => match placement {
            ApiKeyPlacement::Header => {
                if explicit_header_keys.contains(&key.to_ascii_lowercase()) {
                    builder
                } else {
                    builder.header(key, value)
                }
            }
            ApiKeyPlacement::Query => builder.query(&[(key.as_str(), value.as_str())]),
        },
        Auth::OAuth2(cfg) => {
            if cfg.access_token.is_empty() || has_explicit_authorization {
                builder
            } else {
                builder.bearer_auth(&cfg.access_token)
            }
        }
    }
}

/// Builds an `OutgoingRequest` from a parsed `.apireq`, with `{{var}}`
/// interpolation already applied to the URL, headers, auth, and body by
/// the caller (see `crate::vars::resolve_for_send` and `Auth::resolve`).
/// `resolved_body` may be `None` when the request has no body. Query/path
/// params are merged into the final URL by the caller, not here.
pub fn build_outgoing_request(
    file: &ApiRequestFile,
    resolved_url: String,
    resolved_headers: IndexMap<String, String>,
    resolved_auth: Auth,
    resolved_body: Option<String>,
) -> Result<OutgoingRequest, EngineError> {
    let method = Method::from_bytes(file.method.to_uppercase().as_bytes())
        .map_err(|_| EngineError::ParseFormat(format!("unknown HTTP method '{}'", file.method)))?;

    // Every non-JSON body kind (raw/text/xml/form-data/urlencoded/graphql,
    // and binary until real file reads are wired up) is sent as opaque
    // text — only `body:json` gets the JSON content-type treatment.
    let body = resolved_body.map(|content| {
        if matches!(file.body, Some(Body::Json(_))) {
            OutgoingBody::Json(content)
        } else {
            OutgoingBody::Text(content)
        }
    });

    Ok(OutgoingRequest {
        method,
        url: resolved_url,
        headers: resolved_headers,
        auth: resolved_auth,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{ApiKeyPlacement, OAuth2Config};

    fn build(auth: &Auth, explicit_headers: &[&str]) -> reqwest::Request {
        let explicit_header_keys: HashSet<String> = explicit_headers.iter().map(|s| s.to_ascii_lowercase()).collect();
        let builder = apply_auth(
            reqwest::Client::new().get("https://example.com/x"),
            auth,
            &explicit_header_keys,
        );
        builder.build().unwrap()
    }

    #[test]
    fn bearer_sets_authorization_header() {
        let req = build(&Auth::Bearer { token: "abc123".into() }, &[]);
        assert_eq!(req.headers().get("authorization").unwrap(), "Bearer abc123");
    }

    #[test]
    fn explicit_authorization_header_wins_over_bearer() {
        // apply_auth alone can't add the explicit header (that's the
        // caller's job, done after apply_auth in `send`) -- this confirms
        // apply_auth *skips* setting one of its own when told one exists,
        // which is the half of the precedence rule that lives here.
        let req = build(&Auth::Bearer { token: "abc123".into() }, &["Authorization"]);
        assert!(req.headers().get("authorization").is_none());
    }

    #[test]
    fn api_key_header_placement_sets_named_header() {
        let req = build(
            &Auth::ApiKey {
                key: "X-API-Key".into(),
                value: "secret".into(),
                placement: ApiKeyPlacement::Header,
            },
            &[],
        );
        assert_eq!(req.headers().get("x-api-key").unwrap(), "secret");
    }

    #[test]
    fn api_key_query_placement_appends_to_url() {
        let req = build(
            &Auth::ApiKey {
                key: "api_key".into(),
                value: "secret".into(),
                placement: ApiKeyPlacement::Query,
            },
            &[],
        );
        assert_eq!(req.url().query(), Some("api_key=secret"));
        assert!(req.headers().get("x-api-key").is_none());
    }

    #[test]
    fn oauth2_with_empty_token_sends_no_authorization_header() {
        let req = build(
            &Auth::OAuth2(OAuth2Config {
                grant_type: "client_credentials".into(),
                auth_url: String::new(),
                access_token_url: String::new(),
                client_id: String::new(),
                client_secret: String::new(),
                scope: String::new(),
                redirect_uri: String::new(),
                access_token: String::new(),
            }),
            &[],
        );
        assert!(req.headers().get("authorization").is_none());
    }

    #[test]
    fn oauth2_with_cached_token_sends_bearer_header() {
        let req = build(
            &Auth::OAuth2(OAuth2Config {
                grant_type: "client_credentials".into(),
                auth_url: String::new(),
                access_token_url: String::new(),
                client_id: String::new(),
                client_secret: String::new(),
                scope: String::new(),
                redirect_uri: String::new(),
                access_token: "cached-token".into(),
            }),
            &[],
        );
        assert_eq!(req.headers().get("authorization").unwrap(), "Bearer cached-token");
    }

    #[test]
    fn none_and_inherit_set_no_authorization_header() {
        assert!(build(&Auth::None, &[]).headers().get("authorization").is_none());
        assert!(build(&Auth::Inherit, &[]).headers().get("authorization").is_none());
    }
}
