use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use fluxchunk_engine::collection::{self, resolve_inherited_auth, CollectionItem};
use fluxchunk_engine::format::{ApiKeyPlacement, ApiRequestFile, Auth, Body, EnvironmentFile, OAuth2Config, VaultFile};
use fluxchunk_engine::http::{HttpClient, Method, OutgoingBody, OutgoingRequest};
use fluxchunk_engine::vars::{interpolate, merge_scopes, resolve_vault};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// The single currently-loaded environment, if any. One at a time, same
/// as `CollectionState` -- this app has one sidebar, one open collection.
/// `vault` is never serialized back to the frontend; only `send_request`
/// reads it, and only at actual send time (spec section 9).
#[derive(Default)]
struct EnvironmentState {
    vars: IndexMap<String, String>,
    vault: IndexMap<String, String>,
}

/// The single currently-open collection, if any. `auth` backs `auth {
/// mode: inherit }` resolution (`resolve_inherited_auth`); `vars` merges
/// in below environment vars (global < collection < environment).
#[derive(Default)]
struct CollectionState {
    vars: IndexMap<String, String>,
    auth: Auth,
}

struct AppState {
    environment: Mutex<EnvironmentState>,
    collection: Mutex<CollectionState>,
}

#[derive(Debug, Serialize)]
struct EnvironmentSummary {
    name: String,
    vars: HashMap<String, String>,
}

/// Loads `<path>` as an `.apienv` plus its sibling `<path>.vault` (if
/// present), and makes both the active environment for subsequent
/// `send_request` calls. Returns only `vars` to the frontend -- vault
/// secrets never cross the IPC boundary.
#[tauri::command]
fn load_environment(state: tauri::State<AppState>, path: String) -> Result<EnvironmentSummary, String> {
    let path = PathBuf::from(path);
    let source = std::fs::read_to_string(&path).map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
    let env = EnvironmentFile::parse(&source).map_err(|e| e.to_string())?;

    let vault_path = PathBuf::from(format!("{}.vault", path.display()));
    let vault = match std::fs::read_to_string(&vault_path) {
        Ok(source) => VaultFile::parse(&source).secrets,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => IndexMap::new(),
        Err(e) => return Err(format!("couldn't read {}: {e}", vault_path.display())),
    };

    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("environment")
        .to_string();

    let summary = EnvironmentSummary {
        name: name.clone(),
        vars: env.vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
    };

    *state.environment.lock().unwrap() = EnvironmentState { vars: env.vars, vault };

    Ok(summary)
}

#[tauri::command]
fn clear_environment(state: tauri::State<AppState>) {
    *state.environment.lock().unwrap() = EnvironmentState::default();
}

/// Mirrors the Auth tab's mode selector -- only the fields relevant to
/// `mode` need to be set; the rest are `None`/ignored. Kept flat (rather
/// than a tagged enum) since it's just an IPC payload shape, not the
/// canonical `Auth` type. Used in both directions: `into_auth` builds a
/// real `Auth` from what the UI sent, `from_auth` builds the payload a
/// freshly-opened tab pre-fills its Auth section from.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthPayload {
    mode: String,
    username: Option<String>,
    password: Option<String>,
    token: Option<String>,
    key: Option<String>,
    value: Option<String>,
    placement: Option<String>,
    access_token: Option<String>,
}

impl AuthPayload {
    fn into_auth(self) -> Auth {
        match self.mode.as_str() {
            "basic" => Auth::Basic {
                username: self.username.unwrap_or_default(),
                password: self.password.unwrap_or_default(),
            },
            "bearer" => Auth::Bearer {
                token: self.token.unwrap_or_default(),
            },
            "apikey" => Auth::ApiKey {
                key: self.key.unwrap_or_default(),
                value: self.value.unwrap_or_default(),
                placement: ApiKeyPlacement::from_field(self.placement.as_deref()),
            },
            // Only the "I already have a token" case is wired up -- see
            // OAuth2Config::access_token's doc comment for why an actual
            // interactive grant flow isn't here yet.
            "oauth2" => Auth::OAuth2(OAuth2Config {
                grant_type: String::new(),
                auth_url: String::new(),
                access_token_url: String::new(),
                client_id: String::new(),
                client_secret: String::new(),
                scope: String::new(),
                redirect_uri: String::new(),
                access_token: self.access_token.unwrap_or_default(),
            }),
            "inherit" => Auth::Inherit,
            _ => Auth::None,
        }
    }

    fn from_auth(auth: &Auth) -> Self {
        let mut payload = AuthPayload {
            mode: auth.mode_str().to_string(),
            ..Default::default()
        };
        match auth {
            Auth::None | Auth::Inherit => {}
            Auth::Basic { username, password } => {
                payload.username = Some(username.clone());
                payload.password = Some(password.clone());
            }
            Auth::Bearer { token } => payload.token = Some(token.clone()),
            Auth::ApiKey { key, value, placement } => {
                payload.key = Some(key.clone());
                payload.value = Some(value.clone());
                payload.placement = Some(
                    match placement {
                        ApiKeyPlacement::Header => "header",
                        ApiKeyPlacement::Query => "query",
                    }
                    .to_string(),
                );
            }
            Auth::OAuth2(cfg) => payload.access_token = Some(cfg.access_token.clone()),
        }
        payload
    }
}

#[derive(Debug, Serialize)]
struct SendResponseResult {
    status: u16,
    status_text: String,
    headers: HashMap<String, String>,
    body: String,
    elapsed_ms: u128,
    /// The URL actually requested, with `{{var}}`s resolved but
    /// `{{vault:...}}` refs deliberately left alone -- safe to show in
    /// the UI. Same rule `apicli` follows: a resolved secret is never
    /// echoed anywhere outside the actual outgoing request.
    resolved_url: String,
}

/// Sends a single request, resolving `{{var}}` / `{{vault:...}}`
/// references against the currently loaded environment and collection
/// (if any) first, and resolving `auth { mode: inherit }` against the
/// collection's auth. The `.apireq` file round trip and scripting layer
/// on top of this same path later.
#[tauri::command]
async fn send_request(
    state: tauri::State<'_, AppState>,
    method: String,
    url: String,
    headers: HashMap<String, String>,
    body: Option<String>,
    auth: AuthPayload,
) -> Result<SendResponseResult, String> {
    let (env_vars, vault) = {
        let env = state.environment.lock().unwrap();
        (env.vars.clone(), env.vault.clone())
    };
    let (collection_vars, collection_auth) = {
        let col = state.collection.lock().unwrap();
        (col.vars.clone(), col.auth.clone())
    };
    let vars = merge_scopes(&[&collection_vars, &env_vars]);

    let method = Method::from_bytes(method.to_uppercase().as_bytes()).map_err(|e| e.to_string())?;

    let visible_url = interpolate(&url, &vars);
    let visible_headers: IndexMap<String, String> =
        headers.iter().map(|(k, v)| (k.clone(), interpolate(v, &vars))).collect();

    let send_url = resolve_vault(&visible_url, &vault);
    let send_headers: IndexMap<String, String> =
        visible_headers.iter().map(|(k, v)| (k.clone(), resolve_vault(v, &vault))).collect();
    let auth = resolve_inherited_auth(&auth.into_auth(), &collection_auth);
    let resolved_auth = auth.resolve(&vars, &vault);
    let resolved_body = body.map(|b| resolve_vault(&interpolate(&b, &vars), &vault));

    let outgoing = OutgoingRequest {
        method,
        url: send_url,
        headers: send_headers,
        auth: resolved_auth,
        body: resolved_body.map(OutgoingBody::Text),
    };

    let client = HttpClient::new();
    let resp = client.send(outgoing).await.map_err(|e| e.to_string())?;

    let status = resp.status.as_u16();
    let status_text = resp.status.canonical_reason().unwrap_or("").to_string();
    let body = resp.body_as_text();

    Ok(SendResponseResult {
        status,
        status_text,
        headers: resp.headers.into_iter().collect(),
        body,
        elapsed_ms: resp.elapsed_ms,
        resolved_url: visible_url,
    })
}

/// Mirrors `fluxchunk_engine::collection::CollectionItem`, but with paths
/// as strings (IPC-friendly) and tagged for the frontend to discriminate.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum TreeNode {
    Folder { name: String, items: Vec<TreeNode> },
    Request { name: String, path: String },
}

impl TreeNode {
    fn from_item(item: &CollectionItem) -> TreeNode {
        match item {
            CollectionItem::Folder { name, items } => TreeNode::Folder {
                name: name.clone(),
                items: items.iter().map(TreeNode::from_item).collect(),
            },
            CollectionItem::Request { name, path, .. } => TreeNode::Request {
                name: name.clone(),
                path: path.to_string_lossy().to_string(),
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct EnvironmentEntry {
    name: String,
    path: String,
}

#[derive(Debug, Serialize)]
struct CollectionSummary {
    name: String,
    root: String,
    items: Vec<TreeNode>,
    environments: Vec<EnvironmentEntry>,
}

/// Scans `path` for `collection.apicol`, `environments/*.apienv`, and
/// every `.apireq` file (`fluxchunk_engine::collection::discover`), and
/// makes the collection's own vars/auth active for subsequent
/// `send_request` calls (`auth { mode: inherit }` resolves against this).
#[tauri::command]
fn open_collection(state: tauri::State<AppState>, path: String) -> Result<CollectionSummary, String> {
    let root = PathBuf::from(path);
    let tree = collection::discover(&root).map_err(|e| e.to_string())?;

    let name = tree
        .collection
        .as_ref()
        .map(|c| c.meta.name.clone())
        .filter(|n| !n.is_empty())
        .or_else(|| root.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "Collection".to_string());

    let environments = tree
        .environments
        .iter()
        .map(|p| EnvironmentEntry {
            name: p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
            path: p.to_string_lossy().to_string(),
        })
        .collect();

    let items = tree.items.iter().map(TreeNode::from_item).collect();

    let (vars, auth) = match &tree.collection {
        Some(c) => (c.vars.clone(), c.auth.clone()),
        None => (IndexMap::new(), Auth::None),
    };
    *state.collection.lock().unwrap() = CollectionState { vars, auth };

    Ok(CollectionSummary {
        name,
        root: root.to_string_lossy().to_string(),
        items,
        environments,
    })
}

#[tauri::command]
fn close_collection(state: tauri::State<AppState>) {
    *state.collection.lock().unwrap() = CollectionState::default();
}

#[derive(Debug, Serialize)]
struct RequestSummary {
    name: String,
    method: String,
    url: String,
    headers: HashMap<String, String>,
    auth: AuthPayload,
    body: Option<String>,
}

/// Parses a `.apireq` file for opening in a tab. Everything comes back
/// *unresolved* (raw `{{var}}`/`{{vault:...}}` text) -- this is for
/// editing the template, not for sending; `send_request` does its own
/// resolution when the tab's Send button is used.
#[tauri::command]
fn read_request(path: String) -> Result<RequestSummary, String> {
    let path = PathBuf::from(path);
    let source = std::fs::read_to_string(&path).map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
    let file = ApiRequestFile::parse(&source).map_err(|e| e.to_string())?;

    Ok(RequestSummary {
        name: file.meta.name.clone(),
        method: file.method.to_uppercase(),
        url: file.url.clone(),
        headers: file.headers.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        auth: AuthPayload::from_auth(&file.auth),
        body: file.body.as_ref().map(|b| b.content().to_string()),
    })
}

/// Writes the tab's edited method/url/headers/auth/body back into the
/// original `.apireq` file. Re-reads that file fresh first and only
/// overwrites those fields, so anything the UI doesn't (yet) expose --
/// meta, params, scripts, asserts, unrecognized blocks -- survives
/// untouched rather than being silently dropped.
#[tauri::command]
fn save_request(
    path: String,
    method: String,
    url: String,
    headers: HashMap<String, String>,
    auth: AuthPayload,
    body: Option<String>,
) -> Result<(), String> {
    let path = PathBuf::from(path);
    let source = std::fs::read_to_string(&path).map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
    let mut file = ApiRequestFile::parse(&source).map_err(|e| e.to_string())?;

    file.method = method.to_lowercase();
    file.url = url;
    file.headers = headers.into_iter().collect();
    file.auth = auth.into_auth();
    file.body = match body.filter(|b| !b.is_empty()) {
        Some(content) => Some(match &file.body {
            Some(existing) => existing.with_content(content),
            // No body-type selector in the UI yet -- JSON is the
            // reasonable default for a first body on a request that
            // didn't have one (matches the spec's own .apireq example).
            None => Body::Json(content),
        }),
        None => None,
    };

    std::fs::write(&path, file.to_string_pretty()).map_err(|e| format!("couldn't write {}: {e}", path.display()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            environment: Mutex::new(EnvironmentState::default()),
            collection: Mutex::new(CollectionState::default()),
        })
        .invoke_handler(tauri::generate_handler![
            send_request,
            load_environment,
            clear_environment,
            open_collection,
            close_collection,
            read_request,
            save_request
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
