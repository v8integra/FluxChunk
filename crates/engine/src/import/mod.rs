//! Postman/OpenAPI import (spec section 5: "Import: Postman collections,
//! OpenAPI specs" -- explicitly *without* the security scanning from
//! section 8, which is a separate, later build-order step).
//!
//! Both parsers below produce the same [`ImportedCollection`] shape,
//! which [`write_imported_collection`] turns into real `.apicol` +
//! `.apireq` files on disk using the exact same types and serializers as
//! everything else in this crate -- an imported request is not a special
//! case anywhere downstream of this module.

pub mod openapi;
pub mod postman;

use std::collections::HashMap;
use std::path::Path;

use indexmap::IndexMap;

use crate::error::EngineError;
use crate::format::{Auth, CollectionFile, CollectionMeta, ApiRequestFile};

#[derive(Debug, Clone)]
pub struct ImportedRequest {
    /// Folder path components relative to the collection root (empty =
    /// collection root). Already slugified.
    pub folder_path: Vec<String>,
    /// Includes the `.apireq` extension; unique within `folder_path` by
    /// the time [`import_from`]'s caller sees it (see [`dedupe_file_names`]).
    pub file_name: String,
    pub request: ApiRequestFile,
}

#[derive(Debug, Clone)]
pub struct ImportedCollection {
    pub name: String,
    /// Becomes `collection.apicol`'s `vars` block -- both Postman
    /// collection variables and an OpenAPI `servers[0].url` are
    /// collection-scoped concepts, matching what `.apicol`'s own `vars`
    /// block is for. No synthetic environment file is generated.
    pub vars: IndexMap<String, String>,
    pub collection_auth: Auth,
    pub requests: Vec<ImportedRequest>,
    /// Things skipped or simplified during import (an unsupported auth
    /// type, a request body left unset) -- surfaced to the user rather
    /// than silently dropped.
    pub warnings: Vec<String>,
}

/// Lowercase-alphanumeric-and-dashes, safe as a file or directory name.
/// Returns an empty string for input with no alphanumeric characters at
/// all -- callers apply their own contextual fallback (e.g. "request").
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut last_was_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !out.is_empty() && !last_was_dash {
            out.push('-');
            last_was_dash = true;
        }
    }
    if out.ends_with('-') {
        out.pop();
    }
    out
}

/// Resolves file-name collisions within the same folder (two Postman
/// requests named identically, or two OpenAPI operations that slugify to
/// the same string) by appending `-2`, `-3`, etc.
pub(crate) fn dedupe_file_names(requests: &mut [ImportedRequest]) {
    let mut seen: HashMap<(Vec<String>, String), u32> = HashMap::new();
    for req in requests.iter_mut() {
        let key = (req.folder_path.clone(), req.file_name.clone());
        let count = seen.entry(key).or_insert(0);
        *count += 1;
        if *count > 1 {
            let stem = req.file_name.strip_suffix(".apireq").unwrap_or(&req.file_name);
            req.file_name = format!("{stem}-{}.apireq", *count);
        }
    }
}

fn json_value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Writes an [`ImportedCollection`] to `dest` as `collection.apicol` plus
/// one `.apireq` per request, recreating each request's folder path.
/// Refuses to write into a `dest` that already exists and is non-empty,
/// rather than silently overwriting whatever's there.
pub fn write_imported_collection(imported: &ImportedCollection, dest: &Path) -> Result<(), EngineError> {
    if dest.exists() {
        let has_entries = std::fs::read_dir(dest)?.next().is_some();
        if has_entries {
            return Err(EngineError::ParseFormat(format!(
                "{} already exists and isn't empty -- choose a different location",
                dest.display()
            )));
        }
    }
    std::fs::create_dir_all(dest)?;

    let collection_file = CollectionFile {
        meta: CollectionMeta {
            name: imported.name.clone(),
            format_version: 1,
        },
        vars: imported.vars.clone(),
        auth: imported.collection_auth.clone(),
    };
    std::fs::write(dest.join("collection.apicol"), collection_file.to_string_pretty())?;

    for req in &imported.requests {
        let mut dir = dest.to_path_buf();
        for segment in &req.folder_path {
            dir.push(segment);
        }
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(&req.file_name), req.request.to_string_pretty())?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_lowercases_and_dashes() {
        assert_eq!(slugify("Get User By ID"), "get-user-by-id");
        assert_eq!(slugify("  leading/trailing  "), "leading-trailing");
        assert_eq!(slugify("a---b"), "a-b");
        assert_eq!(slugify("!!!"), "");
    }

    #[test]
    fn dedupe_appends_suffix_only_within_same_folder() {
        let mut reqs = vec![
            ImportedRequest {
                folder_path: vec![],
                file_name: "ping.apireq".to_string(),
                request: sample_request(),
            },
            ImportedRequest {
                folder_path: vec![],
                file_name: "ping.apireq".to_string(),
                request: sample_request(),
            },
            ImportedRequest {
                folder_path: vec!["users".to_string()],
                file_name: "ping.apireq".to_string(),
                request: sample_request(),
            },
        ];
        dedupe_file_names(&mut reqs);
        assert_eq!(reqs[0].file_name, "ping.apireq");
        assert_eq!(reqs[1].file_name, "ping-2.apireq");
        assert_eq!(reqs[2].file_name, "ping.apireq"); // different folder, no collision
    }

    fn sample_request() -> ApiRequestFile {
        ApiRequestFile::parse("meta {\n  name: x\n  type: http\n  seq: 1\n}\n\nget {\n  url: https://example.com\n}\n").unwrap()
    }

    #[test]
    fn writes_collection_and_nested_requests_to_disk() {
        let dir = std::env::temp_dir().join(format!("fluxchunk-import-write-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let imported = ImportedCollection {
            name: "Demo".to_string(),
            vars: IndexMap::from([("base_url".to_string(), "https://api.example.com".to_string())]),
            collection_auth: Auth::None,
            requests: vec![ImportedRequest {
                folder_path: vec!["users".to_string()],
                file_name: "get-user.apireq".to_string(),
                request: sample_request(),
            }],
            warnings: vec![],
        };

        write_imported_collection(&imported, &dir).unwrap();

        assert!(dir.join("collection.apicol").exists());
        assert!(dir.join("users").join("get-user.apireq").exists());
        let collection_source = std::fs::read_to_string(dir.join("collection.apicol")).unwrap();
        assert!(collection_source.contains("Demo"));
        assert!(collection_source.contains("base_url"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn refuses_to_write_into_a_nonempty_existing_directory() {
        let dir = std::env::temp_dir().join(format!("fluxchunk-import-nonempty-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("existing-file.txt"), "hello").unwrap();

        let imported = ImportedCollection {
            name: "Demo".to_string(),
            vars: IndexMap::new(),
            collection_auth: Auth::None,
            requests: vec![],
            warnings: vec![],
        };

        assert!(write_imported_collection(&imported, &dir).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
