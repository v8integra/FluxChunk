//! Collection/folder discovery (spec section 4's folder structure,
//! section 5's "Collections, folders, environments"): walks a directory
//! tree, finds `collection.apicol`, every `environments/*.apienv`, and
//! every `.apireq` file, building the tree a sidebar renders.
//!
//! This is read-only discovery -- creating/renaming/reordering
//! collection items from the UI is a later build-order step.

use std::path::{Path, PathBuf};

use crate::error::EngineError;
use crate::format::{ApiRequestFile, Auth, CollectionFile};

#[derive(Debug, Clone)]
pub enum CollectionItem {
    Folder { name: String, items: Vec<CollectionItem> },
    Request { name: String, path: PathBuf, seq: u32 },
}

#[derive(Debug, Clone)]
pub struct CollectionTree {
    pub root: PathBuf,
    pub collection: Option<CollectionFile>,
    pub environments: Vec<PathBuf>,
    pub items: Vec<CollectionItem>,
}

/// Resolves a request's `auth { mode: inherit }` against its collection's
/// auth. Anything other than `Auth::Inherit` is returned unchanged --
/// this is the only place that distinction matters, since
/// `http::apply_auth` just sends whatever `Auth` it's given.
pub fn resolve_inherited_auth(request_auth: &Auth, collection_auth: &Auth) -> Auth {
    match request_auth {
        Auth::Inherit => collection_auth.clone(),
        other => other.clone(),
    }
}

const RESERVED_DIR_NAMES: &[&str] = &["environments"];

pub fn discover(root: &Path) -> Result<CollectionTree, EngineError> {
    let collection = match std::fs::read_to_string(root.join("collection.apicol")) {
        Ok(source) => Some(CollectionFile::parse(&source)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(EngineError::Io(e)),
    };

    let environments = discover_environments(&root.join("environments"))?;
    let items = discover_items(root)?;

    Ok(CollectionTree {
        root: root.to_path_buf(),
        collection,
        environments,
        items,
    })
}

fn discover_environments(dir: &Path) -> Result<Vec<PathBuf>, EngineError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut envs = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("apienv") {
            envs.push(path);
        }
    }
    envs.sort();
    Ok(envs)
}

fn discover_items(dir: &Path) -> Result<Vec<CollectionItem>, EngineError> {
    let mut requests: Vec<(u32, CollectionItem)> = Vec::new();
    let mut folders: Vec<(String, CollectionItem)> = Vec::new();

    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();

        if name.starts_with('.') {
            continue; // .git, .apiworkspace, dotfiles
        }

        if path.is_dir() {
            if RESERVED_DIR_NAMES.contains(&name.as_str()) {
                continue;
            }
            let children = discover_items(&path)?;
            if !children.is_empty() {
                folders.push((name.clone(), CollectionItem::Folder { name, items: children }));
            }
            continue;
        }

        if path.extension().and_then(|e| e.to_str()) != Some("apireq") {
            continue;
        }

        let source = std::fs::read_to_string(&path)?;
        let parsed = ApiRequestFile::parse(&source)
            .map_err(|e| EngineError::ParseFormat(format!("{}: {e}", path.display())))?;
        let display_name = if parsed.meta.name.is_empty() {
            path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
        } else {
            parsed.meta.name.clone()
        };
        let seq = parsed.meta.seq;
        requests.push((
            seq,
            CollectionItem::Request { name: display_name, path, seq },
        ));
    }

    requests.sort_by_key(|(seq, _)| *seq);
    folders.sort_by(|(a, _), (b, _)| a.cmp(b));

    let mut items: Vec<CollectionItem> = folders.into_iter().map(|(_, item)| item).collect();
    items.extend(requests.into_iter().map(|(_, item)| item));
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("fluxchunk-collection-test-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn req(name: &str, seq: u32) -> String {
        format!("meta {{\n  name: {name}\n  type: http\n  seq: {seq}\n}}\n\nget {{\n  url: https://example.com\n}}\n")
    }

    #[test]
    fn discovers_flat_requests_sorted_by_seq() {
        let dir = TempDir::new();
        write(&dir.0, "b.apireq", &req("Second", 2));
        write(&dir.0, "a.apireq", &req("First", 1));

        let tree = discover(&dir.0).unwrap();
        let names: Vec<_> = tree
            .items
            .iter()
            .map(|i| match i {
                CollectionItem::Request { name, .. } => name.clone(),
                _ => panic!("expected request"),
            })
            .collect();
        assert_eq!(names, vec!["First", "Second"]);
    }

    #[test]
    fn discovers_nested_folders() {
        let dir = TempDir::new();
        write(&dir.0, "users/get-user.apireq", &req("Get user", 1));
        write(&dir.0, "auth/login.apireq", &req("Login", 1));

        let tree = discover(&dir.0).unwrap();
        let folder_names: Vec<_> = tree
            .items
            .iter()
            .map(|i| match i {
                CollectionItem::Folder { name, .. } => name.clone(),
                _ => panic!("expected folder"),
            })
            .collect();
        assert_eq!(folder_names, vec!["auth", "users"]); // sorted by name
    }

    #[test]
    fn skips_environments_dir_and_dotfiles_as_requests() {
        let dir = TempDir::new();
        write(&dir.0, "environments/local.apienv", "vars {\n  base_url: https://x.test\n}\n");
        write(&dir.0, ".apiworkspace", "relay: wss://example.com\n");
        write(&dir.0, "ping.apireq", &req("Ping", 1));

        let tree = discover(&dir.0).unwrap();
        assert_eq!(tree.items.len(), 1);
        assert_eq!(tree.environments.len(), 1);
        assert!(tree.environments[0].ends_with("local.apienv"));
    }

    #[test]
    fn empty_folders_are_omitted() {
        let dir = TempDir::new();
        std::fs::create_dir_all(dir.0.join("empty-folder")).unwrap();
        write(&dir.0, "ping.apireq", &req("Ping", 1));

        let tree = discover(&dir.0).unwrap();
        assert_eq!(tree.items.len(), 1);
    }

    #[test]
    fn loads_collection_manifest_when_present() {
        let dir = TempDir::new();
        write(
            &dir.0,
            "collection.apicol",
            "meta {\n  name: Demo\n  format_version: 1\n}\n\nauth {\n  mode: bearer\n}\n\nauth:bearer {\n  token: {{vault:t}}\n}\n",
        );

        let tree = discover(&dir.0).unwrap();
        let collection = tree.collection.unwrap();
        assert_eq!(collection.meta.name, "Demo");
        assert_eq!(collection.auth, Auth::Bearer { token: "{{vault:t}}".to_string() });
    }

    #[test]
    fn no_manifest_is_not_an_error() {
        let dir = TempDir::new();
        write(&dir.0, "ping.apireq", &req("Ping", 1));
        let tree = discover(&dir.0).unwrap();
        assert!(tree.collection.is_none());
    }

    #[test]
    fn malformed_request_file_fails_loudly() {
        let dir = TempDir::new();
        write(&dir.0, "broken.apireq", "not a valid apireq file\n");
        assert!(discover(&dir.0).is_err());
    }

    #[test]
    fn inherit_resolves_to_collection_auth() {
        let collection_auth = Auth::Bearer { token: "abc".to_string() };
        assert_eq!(resolve_inherited_auth(&Auth::Inherit, &collection_auth), collection_auth);
    }

    #[test]
    fn non_inherit_auth_is_unaffected_by_collection() {
        let request_auth = Auth::Basic {
            username: "u".to_string(),
            password: "p".to_string(),
        };
        let collection_auth = Auth::Bearer { token: "abc".to_string() };
        assert_eq!(resolve_inherited_auth(&request_auth, &collection_auth), request_auth);
    }
}
