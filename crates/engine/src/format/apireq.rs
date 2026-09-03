use super::blocks::{parse_blocks, parse_key_value_lines, render_key_value_block, render_raw_block, RawBlock};
use crate::error::EngineError;
use indexmap::IndexMap;

const HTTP_METHODS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "options"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    pub name: String,
    pub request_type: String, // "http" | "graphql" | "grpc" | "ws" | "sse"
    pub seq: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Json(String),
    Raw(String),
    Text(String),
    Xml(String),
    FormData(String),
    UrlEncoded(String),
    GraphQl(String),
    Binary(String),
}

impl Body {
    fn kind_suffix(&self) -> &'static str {
        match self {
            Body::Json(_) => "json",
            Body::Raw(_) => "raw",
            Body::Text(_) => "text",
            Body::Xml(_) => "xml",
            Body::FormData(_) => "form-data",
            Body::UrlEncoded(_) => "urlencoded",
            Body::GraphQl(_) => "graphql",
            Body::Binary(_) => "binary",
        }
    }

    pub fn content(&self) -> &str {
        match self {
            Body::Json(s)
            | Body::Raw(s)
            | Body::Text(s)
            | Body::Xml(s)
            | Body::FormData(s)
            | Body::UrlEncoded(s)
            | Body::GraphQl(s)
            | Body::Binary(s) => s,
        }
    }

    fn from_suffix(suffix: &str, content: String) -> Option<Body> {
        Some(match suffix {
            "json" => Body::Json(content),
            "raw" => Body::Raw(content),
            "text" => Body::Text(content),
            "xml" => Body::Xml(content),
            "form-data" => Body::FormData(content),
            "urlencoded" => Body::UrlEncoded(content),
            "graphql" => Body::GraphQl(content),
            "binary" => Body::Binary(content),
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assertion {
    pub path: String,       // e.g. "res.status"
    pub expression: String, // e.g. "eq 200" (grammar deferred; kept raw for MVP)
}

/// A single parsed `.apireq` file. Unrecognized blocks are preserved
/// verbatim in `extra_blocks` so round-tripping never silently drops
/// content the parser doesn't yet understand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiRequestFile {
    pub meta: Meta,
    pub method: String,
    pub url: String,
    pub params_query: IndexMap<String, String>,
    pub params_path: IndexMap<String, String>,
    pub headers: IndexMap<String, String>,
    pub body: Option<Body>,
    pub script_pre_request: Option<String>,
    pub script_post_response: Option<String>,
    pub asserts: Vec<Assertion>,
    pub extra_blocks: Vec<RawBlock>,
}

impl ApiRequestFile {
    pub fn parse(input: &str) -> Result<Self, EngineError> {
        let blocks = parse_blocks(input)?;

        let mut meta = None;
        let mut method = None;
        let mut url = None;
        let mut params_query = IndexMap::new();
        let mut params_path = IndexMap::new();
        let mut headers = IndexMap::new();
        let mut body = None;
        let mut script_pre_request = None;
        let mut script_post_response = None;
        let mut asserts = Vec::new();
        let mut extra_blocks = Vec::new();

        for block in blocks {
            match block.name.as_str() {
                "meta" => {
                    let kv = parse_key_value_lines(&block.content);
                    let seq = kv.get("seq").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
                    meta = Some(Meta {
                        name: kv.get("name").cloned().unwrap_or_default(),
                        request_type: kv.get("type").cloned().unwrap_or_else(|| "http".to_string()),
                        seq,
                    });
                }
                name if HTTP_METHODS.contains(&name) => {
                    let kv = parse_key_value_lines(&block.content);
                    method = Some(name.to_string());
                    url = kv.get("url").cloned();
                }
                "params:query" => params_query = parse_key_value_lines(&block.content),
                "params:path" => params_path = parse_key_value_lines(&block.content),
                "headers" => headers = parse_key_value_lines(&block.content),
                "assert" => {
                    for line in block.content.lines() {
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }
                        if let Some((path, expr)) = line.split_once(':') {
                            asserts.push(Assertion {
                                path: path.trim().to_string(),
                                expression: expr.trim().to_string(),
                            });
                        }
                    }
                }
                name if name.starts_with("body:") => {
                    let suffix = &name["body:".len()..];
                    body = Body::from_suffix(suffix, block.content.clone());
                    if body.is_none() {
                        extra_blocks.push(block);
                    }
                }
                "script:pre-request" => script_pre_request = Some(block.content.clone()),
                "script:post-response" => script_post_response = Some(block.content.clone()),
                _ => extra_blocks.push(block),
            }
        }

        let meta = meta.ok_or_else(|| EngineError::ParseFormat("missing 'meta' block".into()))?;
        let method = method.ok_or_else(|| EngineError::ParseFormat("missing HTTP method block (get/post/...)".into()))?;
        let url = url.ok_or_else(|| EngineError::ParseFormat(format!("'{method}' block missing 'url' field")))?;

        Ok(ApiRequestFile {
            meta,
            method,
            url,
            params_query,
            params_path,
            headers,
            body,
            script_pre_request,
            script_post_response,
            asserts,
            extra_blocks,
        })
    }

    pub fn to_string_pretty(&self) -> String {
        let mut sections = Vec::new();

        let mut meta_kv = IndexMap::new();
        meta_kv.insert("name".to_string(), self.meta.name.clone());
        meta_kv.insert("type".to_string(), self.meta.request_type.clone());
        meta_kv.insert("seq".to_string(), self.meta.seq.to_string());
        sections.push(render_key_value_block("meta", &meta_kv).unwrap());

        let mut method_kv = IndexMap::new();
        method_kv.insert("url".to_string(), self.url.clone());
        sections.push(render_key_value_block(&self.method, &method_kv).unwrap());

        if let Some(b) = render_key_value_block("params:path", &self.params_path) {
            sections.push(b);
        }
        if let Some(b) = render_key_value_block("params:query", &self.params_query) {
            sections.push(b);
        }
        if let Some(b) = render_key_value_block("headers", &self.headers) {
            sections.push(b);
        }
        if let Some(body) = &self.body {
            sections.push(render_raw_block(&format!("body:{}", body.kind_suffix()), body.content()));
        }
        if let Some(script) = &self.script_pre_request {
            sections.push(render_raw_block("script:pre-request", script));
        }
        if let Some(script) = &self.script_post_response {
            sections.push(render_raw_block("script:post-response", script));
        }
        if !self.asserts.is_empty() {
            let mut kv = IndexMap::new();
            for a in &self.asserts {
                kv.insert(a.path.clone(), a.expression.clone());
            }
            sections.push(render_key_value_block("assert", &kv).unwrap());
        }
        for extra in &self.extra_blocks {
            sections.push(render_raw_block(&extra.name, &extra.content));
        }

        sections.join("\n\n") + "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"meta {
  name: Get user by ID
  type: http
  seq: 3
}

get {
  url: {{base_url}}/users/:id
}

params:path {
  id: {{user_id}}
}

headers {
  Authorization: Bearer {{access_token}}
  Accept: application/json
}

body:json {
  {
    "include": ["profile", "roles"]
  }
}

script:pre-request {
  bru.setVar("request_time", Date.now());
}

script:post-response {
  if (res.status === 200) {
    bru.setVar("last_user_id", res.body.id);
  }
}

assert {
  res.status: eq 200
  res.body.id: isDefined
}
"#;

    #[test]
    fn parses_spec_example() {
        let parsed = ApiRequestFile::parse(EXAMPLE).unwrap();
        assert_eq!(parsed.meta.name, "Get user by ID");
        assert_eq!(parsed.meta.request_type, "http");
        assert_eq!(parsed.meta.seq, 3);
        assert_eq!(parsed.method, "get");
        assert_eq!(parsed.url, "{{base_url}}/users/:id");
        assert_eq!(parsed.params_path.get("id").unwrap(), "{{user_id}}");
        assert_eq!(parsed.headers.get("Authorization").unwrap(), "Bearer {{access_token}}");
        assert!(matches!(&parsed.body, Some(Body::Json(s)) if s.contains("\"include\"")));
        assert!(parsed.script_pre_request.as_ref().unwrap().contains("setVar"));
        assert!(parsed.script_post_response.as_ref().unwrap().contains("last_user_id"));
        assert_eq!(parsed.asserts.len(), 2);
        assert_eq!(parsed.asserts[0].path, "res.status");
        assert_eq!(parsed.asserts[0].expression, "eq 200");
    }

    #[test]
    fn round_trips_through_serialize_and_reparse() {
        let parsed = ApiRequestFile::parse(EXAMPLE).unwrap();
        let rendered = parsed.to_string_pretty();
        let reparsed = ApiRequestFile::parse(&rendered).unwrap();
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn missing_meta_block_errors() {
        let input = "get {\n  url: https://example.com\n}\n";
        assert!(ApiRequestFile::parse(input).is_err());
    }
}
