//! Postman Collection v2.1 import. Postman's own `{{variable}}` syntax
//! happens to match FluxChunk's exactly, so variable references need no
//! translation at all -- they're copied straight through into the
//! generated `.apireq`/`.apicol` text.

use std::collections::HashMap;

use indexmap::IndexMap;
use serde::Deserialize;

use super::{dedupe_file_names, json_value_to_string, slugify, ImportedCollection, ImportedRequest};
use crate::error::EngineError;
use crate::format::{ApiKeyPlacement, ApiRequestFile, Auth, Body, Meta};

#[derive(Debug, Deserialize)]
struct PostmanCollection {
    info: PostmanInfo,
    #[serde(default)]
    item: Vec<PostmanItem>,
    #[serde(default)]
    variable: Vec<PostmanVariable>,
    #[serde(default)]
    auth: Option<PostmanAuth>,
}

#[derive(Debug, Deserialize)]
struct PostmanInfo {
    name: String,
}

#[derive(Debug, Deserialize)]
struct PostmanVariable {
    key: String,
    #[serde(default)]
    value: Option<serde_json::Value>,
}

/// Postman doesn't tag folders vs. requests explicitly -- a folder has an
/// `item` array, a request has a `request` object. `untagged` tries each
/// shape in turn.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PostmanItem {
    Folder(PostmanFolder),
    Request(PostmanRequestItem),
}

#[derive(Debug, Deserialize)]
struct PostmanFolder {
    name: String,
    // Deliberately *not* `#[serde(default)]`: this field is what
    // distinguishes a folder from a request under `#[serde(untagged)]`
    // below. If it were optional, a request item (no `item` array, but
    // one Serde would happily ignore in favor of matching Folder first)
    // would silently parse as an empty folder instead of falling through
    // to `PostmanRequestItem`, and the request would vanish.
    item: Vec<PostmanItem>,
}

#[derive(Debug, Deserialize)]
struct PostmanRequestItem {
    name: String,
    request: PostmanRequest,
}

#[derive(Debug, Deserialize)]
struct PostmanRequest {
    #[serde(default = "default_method")]
    method: String,
    #[serde(default)]
    header: Vec<PostmanHeader>,
    url: PostmanUrl,
    #[serde(default)]
    body: Option<PostmanBody>,
    #[serde(default)]
    auth: Option<PostmanAuth>,
}

fn default_method() -> String {
    "get".to_string()
}

#[derive(Debug, Deserialize)]
struct PostmanHeader {
    key: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    disabled: bool,
}

/// Postman's `url` field is a bare string in some exports, a detailed
/// object (`{raw, host, path, query, ...}`) in others -- only `raw` is
/// needed here since it already contains the full templated URL.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PostmanUrl {
    Raw(String),
    Detailed { raw: String },
}

impl PostmanUrl {
    fn raw(&self) -> &str {
        match self {
            PostmanUrl::Raw(s) => s,
            PostmanUrl::Detailed { raw } => raw,
        }
    }
}

#[derive(Debug, Deserialize)]
struct PostmanBody {
    mode: String,
    #[serde(default)]
    raw: Option<String>,
    #[serde(default)]
    options: Option<PostmanBodyOptions>,
    #[serde(default)]
    urlencoded: Vec<PostmanKv>,
    #[serde(default)]
    formdata: Vec<PostmanKv>,
}

#[derive(Debug, Deserialize)]
struct PostmanBodyOptions {
    raw: Option<PostmanRawOptions>,
}

#[derive(Debug, Deserialize)]
struct PostmanRawOptions {
    language: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PostmanKv {
    key: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    disabled: bool,
}

/// Postman's auth shape is `{"type": "bearer", "bearer": [{"key":
/// "token", "value": "..."}]}` -- the type-named field holding the
/// params is captured by the flatten below.
#[derive(Debug, Deserialize)]
struct PostmanAuth {
    #[serde(rename = "type")]
    kind: String,
    #[serde(flatten)]
    params_by_type: HashMap<String, Vec<PostmanAuthParam>>,
}

#[derive(Debug, Deserialize)]
struct PostmanAuthParam {
    key: String,
    #[serde(default)]
    value: Option<serde_json::Value>,
}

impl PostmanAuth {
    fn param(&self, param_key: &str) -> String {
        self.params_by_type
            .get(&self.kind)
            .and_then(|params| params.iter().find(|p| p.key == param_key))
            .and_then(|p| p.value.as_ref())
            .map(json_value_to_string)
            .unwrap_or_default()
    }
}

/// Converts Postman auth to our `Auth`. Anything not in this list (OAuth2,
/// AWS Sig4, digest, hawk, ...) comes back as `Auth::None` with a warning
/// rather than a guess at a mapping that doesn't exist.
fn convert_auth(auth: Option<&PostmanAuth>) -> (Auth, Option<String>) {
    let Some(auth) = auth else { return (Auth::None, None) };
    match auth.kind.as_str() {
        "noauth" => (Auth::None, None),
        "bearer" => (Auth::Bearer { token: auth.param("token") }, None),
        "basic" => (
            Auth::Basic {
                username: auth.param("username"),
                password: auth.param("password"),
            },
            None,
        ),
        "apikey" => (
            Auth::ApiKey {
                key: auth.param("key"),
                value: auth.param("value"),
                placement: ApiKeyPlacement::from_field(Some(auth.param("in").as_str())),
            },
            None,
        ),
        other => (Auth::None, Some(format!("auth type '{other}' isn't supported yet; imported as no auth"))),
    }
}

fn convert_body(body: Option<&PostmanBody>) -> Option<Body> {
    let body = body?;
    match body.mode.as_str() {
        "raw" => {
            let content = body.raw.clone().unwrap_or_default();
            if content.trim().is_empty() {
                return None;
            }
            let language = body.options.as_ref().and_then(|o| o.raw.as_ref()).and_then(|r| r.language.as_deref());
            Some(match language {
                Some("json") => Body::Json(content),
                Some("xml") => Body::Xml(content),
                _ => Body::Text(content),
            })
        }
        "urlencoded" => {
            let content = body
                .urlencoded
                .iter()
                .filter(|kv| !kv.disabled)
                .map(|kv| format!("{}={}", kv.key, kv.value))
                .collect::<Vec<_>>()
                .join("&");
            (!content.is_empty()).then_some(Body::UrlEncoded(content))
        }
        "formdata" => {
            // Body doesn't model structured multipart parts; best effort
            // as readable key: value lines rather than dropping it.
            let content = body
                .formdata
                .iter()
                .filter(|kv| !kv.disabled)
                .map(|kv| format!("{}: {}", kv.key, kv.value))
                .collect::<Vec<_>>()
                .join("\n");
            (!content.is_empty()).then_some(Body::FormData(content))
        }
        _ => None,
    }
}

fn walk_items(items: &[PostmanItem], folder_path: &[String], seq: &mut u32, out: &mut Vec<ImportedRequest>, warnings: &mut Vec<String>) {
    for item in items {
        match item {
            PostmanItem::Folder(folder) => {
                let mut path = folder_path.to_vec();
                let slug = slugify(&folder.name);
                path.push(if slug.is_empty() { "folder".to_string() } else { slug });
                walk_items(&folder.item, &path, seq, out, warnings);
            }
            PostmanItem::Request(item) => {
                *seq += 1;
                let (auth, auth_warning) = convert_auth(item.request.auth.as_ref());
                if let Some(w) = auth_warning {
                    warnings.push(format!("{}: {w}", item.name));
                }

                let headers: IndexMap<String, String> =
                    item.request.header.iter().filter(|h| !h.disabled).map(|h| (h.key.clone(), h.value.clone())).collect();

                let request = ApiRequestFile {
                    meta: Meta {
                        name: item.name.clone(),
                        request_type: "http".to_string(),
                        seq: *seq,
                    },
                    method: item.request.method.to_lowercase(),
                    url: item.request.url.raw().to_string(),
                    params_query: IndexMap::new(),
                    params_path: IndexMap::new(),
                    headers,
                    auth,
                    body: convert_body(item.request.body.as_ref()),
                    script_pre_request: None,
                    script_post_response: None,
                    asserts: Vec::new(),
                    extra_blocks: Vec::new(),
                };

                let slug = slugify(&item.name);
                let file_name = format!("{}.apireq", if slug.is_empty() { "request".to_string() } else { slug });
                out.push(ImportedRequest {
                    folder_path: folder_path.to_vec(),
                    file_name,
                    request,
                });
            }
        }
    }
}

pub fn import_postman_collection(json: &str) -> Result<ImportedCollection, EngineError> {
    let parsed: PostmanCollection =
        serde_json::from_str(json).map_err(|e| EngineError::ParseFormat(format!("invalid Postman collection: {e}")))?;

    let mut requests = Vec::new();
    let mut warnings = Vec::new();
    let mut seq = 0u32;
    walk_items(&parsed.item, &[], &mut seq, &mut requests, &mut warnings);
    dedupe_file_names(&mut requests);

    let vars: IndexMap<String, String> = parsed
        .variable
        .iter()
        .filter_map(|v| v.value.as_ref().map(|val| (v.key.clone(), json_value_to_string(val))))
        .collect();

    let (collection_auth, auth_warning) = convert_auth(parsed.auth.as_ref());
    if let Some(w) = auth_warning {
        warnings.push(format!("collection auth: {w}"));
    }

    Ok(ImportedCollection {
        name: parsed.info.name,
        vars,
        collection_auth,
        requests,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "info": { "name": "Demo API" },
      "variable": [
        { "key": "base_url", "value": "https://api.example.com" },
        { "key": "retries", "value": 3 }
      ],
      "auth": { "type": "bearer", "bearer": [{ "key": "token", "value": "{{vault:collection_token}}" }] },
      "item": [
        {
          "name": "Users",
          "item": [
            {
              "name": "Get user by ID",
              "request": {
                "method": "GET",
                "header": [{ "key": "Accept", "value": "application/json" }],
                "url": { "raw": "{{base_url}}/users/{{user_id}}" }
              }
            },
            {
              "name": "Create user",
              "request": {
                "method": "POST",
                "header": [],
                "url": "{{base_url}}/users",
                "body": {
                  "mode": "raw",
                  "raw": "{\"name\": \"Ada\"}",
                  "options": { "raw": { "language": "json" } }
                },
                "auth": { "type": "noauth" }
              }
            }
          ]
        },
        {
          "name": "Ping",
          "request": { "method": "GET", "url": "{{base_url}}/ping" }
        }
      ]
    }"#;

    #[test]
    fn parses_name_and_vars() {
        let imported = import_postman_collection(SAMPLE).unwrap();
        assert_eq!(imported.name, "Demo API");
        assert_eq!(imported.vars.get("base_url").unwrap(), "https://api.example.com");
        assert_eq!(imported.vars.get("retries").unwrap(), "3"); // non-string coerced
    }

    #[test]
    fn maps_collection_auth() {
        let imported = import_postman_collection(SAMPLE).unwrap();
        assert_eq!(
            imported.collection_auth,
            Auth::Bearer {
                token: "{{vault:collection_token}}".to_string()
            }
        );
    }

    #[test]
    fn nests_requests_under_slugified_folder_names() {
        let imported = import_postman_collection(SAMPLE).unwrap();
        let get_user = imported.requests.iter().find(|r| r.file_name == "get-user-by-id.apireq").unwrap();
        assert_eq!(get_user.folder_path, vec!["users".to_string()]);
        assert_eq!(get_user.request.url, "{{base_url}}/users/{{user_id}}");
        assert_eq!(get_user.request.headers.get("Accept").unwrap(), "application/json");

        let ping = imported.requests.iter().find(|r| r.file_name == "ping.apireq").unwrap();
        assert!(ping.folder_path.is_empty());
    }

    #[test]
    fn maps_json_body_and_per_request_noauth_override() {
        let imported = import_postman_collection(SAMPLE).unwrap();
        let create_user = imported.requests.iter().find(|r| r.file_name == "create-user.apireq").unwrap();
        assert_eq!(create_user.request.method, "post");
        assert!(matches!(&create_user.request.body, Some(Body::Json(s)) if s.contains("Ada")));
        assert_eq!(create_user.request.auth, Auth::None);
    }

    #[test]
    fn unsupported_auth_type_warns_and_falls_back_to_none() {
        let json = r#"{
          "info": { "name": "X" },
          "item": [{ "name": "R", "request": { "method": "GET", "url": "https://x.test", "auth": { "type": "oauth2" } } }]
        }"#;
        let imported = import_postman_collection(json).unwrap();
        assert_eq!(imported.requests[0].request.auth, Auth::None);
        assert!(imported.warnings.iter().any(|w| w.contains("oauth2")));
    }

    #[test]
    fn invalid_json_errors_cleanly() {
        assert!(import_postman_collection("not json").is_err());
    }
}
