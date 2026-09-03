//! `collection.apicol` — the collection-level manifest (spec section 4:
//! "base URL, default auth, format version"). Reuses the same block
//! primitives and `Auth` type as `.apireq` so a request's `auth { mode:
//! inherit }` has something real to inherit from (see
//! `crate::collection::resolve_inherited_auth`).

use super::auth::Auth;
use super::blocks::{parse_blocks, parse_key_value_lines, render_key_value_block};
use crate::error::EngineError;
use indexmap::IndexMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionMeta {
    pub name: String,
    pub format_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionFile {
    pub meta: CollectionMeta,
    pub vars: IndexMap<String, String>,
    pub auth: Auth,
}

impl CollectionFile {
    pub fn parse(input: &str) -> Result<Self, EngineError> {
        let blocks = parse_blocks(input)?;

        let mut meta = None;
        let mut vars = IndexMap::new();
        let mut auth_mode = None;
        let mut auth_basic = None;
        let mut auth_bearer = None;
        let mut auth_apikey = None;
        let mut auth_oauth2 = None;

        for block in blocks {
            match block.name.as_str() {
                "meta" => {
                    let kv = parse_key_value_lines(&block.content);
                    let format_version = kv.get("format_version").and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
                    meta = Some(CollectionMeta {
                        name: kv.get("name").cloned().unwrap_or_default(),
                        format_version,
                    });
                }
                "vars" => vars = parse_key_value_lines(&block.content),
                "auth" => auth_mode = parse_key_value_lines(&block.content).get("mode").cloned(),
                "auth:basic" => auth_basic = Some(parse_key_value_lines(&block.content)),
                "auth:bearer" => auth_bearer = Some(parse_key_value_lines(&block.content)),
                "auth:apikey" => auth_apikey = Some(parse_key_value_lines(&block.content)),
                "auth:oauth2" => auth_oauth2 = Some(parse_key_value_lines(&block.content)),
                _ => {} // forward-compatible: unknown blocks are ignored rather than erroring
            }
        }

        let meta = meta.unwrap_or(CollectionMeta {
            name: String::new(),
            format_version: 1,
        });
        // A collection's own auth can't itself be `inherit` (there's
        // nothing above it to inherit from) -- fall back to `none` rather
        // than erroring, since this is a much less surprising failure
        // mode than rejecting an otherwise-valid collection file.
        let auth = match auth_mode.as_deref() {
            Some("inherit") => Auth::None,
            _ => Auth::from_parts(auth_mode.as_deref(), auth_basic, auth_bearer, auth_apikey, auth_oauth2)
                .map_err(EngineError::ParseFormat)?,
        };

        Ok(CollectionFile { meta, vars, auth })
    }

    pub fn to_string_pretty(&self) -> String {
        let mut sections = Vec::new();

        let mut meta_kv = IndexMap::new();
        meta_kv.insert("name".to_string(), self.meta.name.clone());
        meta_kv.insert("format_version".to_string(), self.meta.format_version.to_string());
        sections.push(render_key_value_block("meta", &meta_kv).unwrap());

        if let Some(b) = render_key_value_block("vars", &self.vars) {
            sections.push(b);
        }
        sections.extend(self.auth.render_blocks());

        sections.join("\n\n") + "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"meta {
  name: My API Collection
  format_version: 1
}

vars {
  base_url: https://api.example.com
}

auth {
  mode: bearer
}

auth:bearer {
  token: {{vault:default_token}}
}
"#;

    #[test]
    fn parses_example() {
        let col = CollectionFile::parse(EXAMPLE).unwrap();
        assert_eq!(col.meta.name, "My API Collection");
        assert_eq!(col.meta.format_version, 1);
        assert_eq!(col.vars.get("base_url").unwrap(), "https://api.example.com");
        assert_eq!(col.auth, Auth::Bearer { token: "{{vault:default_token}}".to_string() });
    }

    #[test]
    fn round_trips() {
        let col = CollectionFile::parse(EXAMPLE).unwrap();
        let rendered = col.to_string_pretty();
        assert_eq!(CollectionFile::parse(&rendered).unwrap(), col);
    }

    #[test]
    fn missing_meta_defaults_rather_than_errors() {
        let col = CollectionFile::parse("vars {\n  base_url: https://x.test\n}\n").unwrap();
        assert_eq!(col.meta.name, "");
        assert_eq!(col.meta.format_version, 1);
    }

    #[test]
    fn collection_level_inherit_falls_back_to_none() {
        let col = CollectionFile::parse("meta {\n  name: x\n  format_version: 1\n}\n\nauth {\n  mode: inherit\n}\n").unwrap();
        assert_eq!(col.auth, Auth::None);
    }
}
