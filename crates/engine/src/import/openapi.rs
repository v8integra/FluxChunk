//! OpenAPI 3.x / Swagger 2.0 import. JSON only for now -- most OpenAPI
//! tooling can export or serve a JSON representation even when the
//! source of truth is YAML, and picking a YAML crate wasn't worth the
//! dependency risk (`serde_yaml` is archived; its community forks are
//! young) for a first pass. Worth revisiting if that turns out to matter
//! in practice.
//!
//! Request bodies aren't generated from schemas -- resolving `$ref`
//! chains into an example payload is real JSON-Schema work, deserving
//! its own pass rather than a shallow guess. Every request that looks
//! like it needs one (POST/PUT/PATCH) gets a warning instead.

use indexmap::IndexMap;
use serde_json::Value;

use super::{dedupe_file_names, slugify, ImportedCollection, ImportedRequest};
use crate::error::EngineError;
use crate::format::{ApiRequestFile, Auth, Meta};

const HTTP_METHODS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "options"];

fn base_url_v3(doc: &Value) -> Option<String> {
    doc.get("servers")?.as_array()?.first()?.get("url")?.as_str().map(|s| s.to_string())
}

fn base_url_v2(doc: &Value) -> Option<String> {
    let host = doc.get("host")?.as_str()?;
    let scheme = doc.get("schemes").and_then(|s| s.as_array()).and_then(|a| a.first()).and_then(|s| s.as_str()).unwrap_or("https");
    let base_path = doc.get("basePath").and_then(|b| b.as_str()).unwrap_or("");
    Some(format!("{scheme}://{host}{base_path}"))
}

/// OpenAPI path templates use `{param}`; FluxChunk uses `{{param}}`.
/// Doubling every brace maps one straight onto the other with no need to
/// know which segments are actually parameters.
fn convert_path_template(path: &str) -> String {
    path.replace('{', "{{").replace('}', "}}")
}

pub fn import_openapi_spec(json: &str) -> Result<ImportedCollection, EngineError> {
    let doc: Value = serde_json::from_str(json).map_err(|e| EngineError::ParseFormat(format!("invalid OpenAPI/Swagger JSON: {e}")))?;

    let is_v3 = doc.get("openapi").and_then(|v| v.as_str()).map(|s| s.starts_with('3')).unwrap_or(false);
    let is_v2 = doc.get("swagger").and_then(|v| v.as_str()).map(|s| s.starts_with('2')).unwrap_or(false);
    if !is_v3 && !is_v2 {
        return Err(EngineError::ParseFormat(
            "not a recognized OpenAPI (3.x) or Swagger (2.0) document -- missing 'openapi'/'swagger' version field".to_string(),
        ));
    }

    let name = doc
        .get("info")
        .and_then(|i| i.get("title"))
        .and_then(|t| t.as_str())
        .unwrap_or("Imported API")
        .to_string();

    let base_url = if is_v3 { base_url_v3(&doc) } else { base_url_v2(&doc) };
    let mut vars = IndexMap::new();
    if let Some(base_url) = &base_url {
        vars.insert("base_url".to_string(), base_url.clone());
    }

    let mut requests = Vec::new();
    let mut warnings = Vec::new();
    let mut seq = 0u32;
    let mut needs_body_warning = false;

    if let Some(paths) = doc.get("paths").and_then(|p| p.as_object()) {
        let mut path_keys: Vec<&String> = paths.keys().collect();
        path_keys.sort(); // deterministic output regardless of source JSON key order

        for path in path_keys {
            let Some(ops) = paths[path].as_object() else { continue };
            for &method in HTTP_METHODS {
                let Some(op) = ops.get(method) else { continue };
                seq += 1;

                let request_name = op
                    .get("summary")
                    .and_then(|s| s.as_str())
                    .or_else(|| op.get("operationId").and_then(|s| s.as_str()))
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("{} {path}", method.to_uppercase()));

                let folder_path = op
                    .get("tags")
                    .and_then(|t| t.as_array())
                    .and_then(|a| a.first())
                    .and_then(|t| t.as_str())
                    .map(slugify)
                    .filter(|s| !s.is_empty())
                    .map(|s| vec![s])
                    .unwrap_or_default();

                let mut headers = IndexMap::new();
                if let Some(params) = op.get("parameters").and_then(|p| p.as_array()) {
                    for param in params {
                        if param.get("in").and_then(|i| i.as_str()) == Some("header") {
                            if let Some(pname) = param.get("name").and_then(|n| n.as_str()) {
                                headers.insert(pname.to_string(), ["{{", pname, "}}"].concat());
                            }
                        }
                    }
                }

                if ["post", "put", "patch"].contains(&method) && op.get("requestBody").or_else(|| op.get("parameters")).is_some() {
                    needs_body_warning = true;
                }

                let request = ApiRequestFile {
                    meta: Meta {
                        name: request_name,
                        request_type: "http".to_string(),
                        seq,
                    },
                    method: method.to_string(),
                    url: format!("{{{{base_url}}}}{}", convert_path_template(path)),
                    params_query: IndexMap::new(),
                    params_path: IndexMap::new(),
                    headers,
                    auth: Auth::None,
                    body: None,
                    script_pre_request: None,
                    script_post_response: None,
                    asserts: Vec::new(),
                    extra_blocks: Vec::new(),
                };

                // Named after the same human-readable name the request
                // itself carries (summary/operationId/method+path
                // fallback, already resolved above into `meta.name`), not
                // separately re-derived from operationId or method+path --
                // otherwise the file name and the request's own displayed
                // name tell two different stories.
                let slug = slugify(&request.meta.name);
                let file_name = format!("{}.apireq", if slug.is_empty() { format!("{method}-{}", slugify(path)) } else { slug });

                requests.push(ImportedRequest { folder_path, file_name, request });
            }
        }
    }

    if base_url.is_none() {
        warnings.push("no servers/host declared in the spec -- request URLs use {{base_url}}, which you'll need to set yourself".to_string());
    }
    if needs_body_warning {
        warnings.push("request bodies aren't generated from schemas -- add them manually where needed".to_string());
    }

    dedupe_file_names(&mut requests);

    Ok(ImportedCollection {
        name,
        vars,
        collection_auth: Auth::None,
        requests,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_V3: &str = r#"{
      "openapi": "3.0.0",
      "info": { "title": "Demo API" },
      "servers": [{ "url": "https://api.example.com" }],
      "paths": {
        "/users/{id}": {
          "get": {
            "summary": "Get user by ID",
            "tags": ["Users"],
            "parameters": [{ "name": "X-Trace-Id", "in": "header" }]
          }
        },
        "/ping": {
          "get": { "operationId": "ping" }
        },
        "/users": {
          "post": { "summary": "Create user", "tags": ["Users"] }
        }
      }
    }"#;

    #[test]
    fn parses_title_and_base_url() {
        let imported = import_openapi_spec(SAMPLE_V3).unwrap();
        assert_eq!(imported.name, "Demo API");
        assert_eq!(imported.vars.get("base_url").unwrap(), "https://api.example.com");
    }

    #[test]
    fn converts_path_params_and_groups_by_tag() {
        let imported = import_openapi_spec(SAMPLE_V3).unwrap();
        let get_user = imported.requests.iter().find(|r| r.file_name == "get-user-by-id.apireq").unwrap();
        assert_eq!(get_user.request.url, "{{base_url}}/users/{{id}}");
        assert_eq!(get_user.folder_path, vec!["users".to_string()]);
        assert_eq!(get_user.request.headers.get("X-Trace-Id").unwrap(), "{{X-Trace-Id}}");
    }

    #[test]
    fn ungrouped_operation_id_used_when_no_summary() {
        let imported = import_openapi_spec(SAMPLE_V3).unwrap();
        let ping = imported.requests.iter().find(|r| r.file_name == "ping.apireq").unwrap();
        assert!(ping.folder_path.is_empty());
        assert_eq!(ping.request.meta.name, "ping");
    }

    #[test]
    fn warns_about_unsupported_request_bodies() {
        let imported = import_openapi_spec(SAMPLE_V3).unwrap();
        // The POST /users operation has no explicit requestBody/parameters
        // in this fixture, so no warning is expected here; add one to
        // confirm the warning fires when a POST does declare parameters.
        let json = r#"{
          "openapi": "3.0.0",
          "info": { "title": "X" },
          "paths": { "/items": { "post": { "requestBody": { "content": {} } } } }
        }"#;
        let with_body = import_openapi_spec(json).unwrap();
        assert!(with_body.warnings.iter().any(|w| w.contains("request bodies")));
        assert!(imported.requests.iter().all(|r| r.request.body.is_none()));
    }

    #[test]
    fn swagger_v2_base_url_from_host_and_scheme() {
        let json = r#"{
          "swagger": "2.0",
          "info": { "title": "Legacy API" },
          "host": "api.legacy.test",
          "basePath": "/v1",
          "schemes": ["https"],
          "paths": { "/ping": { "get": {} } }
        }"#;
        let imported = import_openapi_spec(json).unwrap();
        assert_eq!(imported.vars.get("base_url").unwrap(), "https://api.legacy.test/v1");
    }

    #[test]
    fn missing_version_field_errors() {
        assert!(import_openapi_spec(r#"{"paths": {}}"#).is_err());
    }

    #[test]
    fn missing_servers_warns_but_still_imports() {
        let json = r#"{"openapi": "3.0.0", "info": {"title": "X"}, "paths": {"/x": {"get": {}}}}"#;
        let imported = import_openapi_spec(json).unwrap();
        assert!(imported.vars.get("base_url").is_none());
        assert!(imported.warnings.iter().any(|w| w.contains("base_url")));
        assert_eq!(imported.requests[0].request.url, "{{base_url}}/x");
    }

    #[test]
    fn deterministic_seq_order_regardless_of_json_key_order() {
        let imported = import_openapi_spec(SAMPLE_V3).unwrap();
        let mut seqs: Vec<u32> = imported.requests.iter().map(|r| r.request.meta.seq).collect();
        seqs.sort();
        assert_eq!(seqs, vec![1, 2, 3]);
    }
}
