use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use fluxchunk_engine::format::{ApiKeyPlacement, Auth, EnvironmentFile, OAuth2Config, VaultFile};
use fluxchunk_engine::http::{HttpClient, Method, OutgoingBody, OutgoingRequest};
use fluxchunk_engine::vars::{interpolate, resolve_vault};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// The single currently-loaded environment, if any. One at a time for
/// now, matching the UI (no collection/workspace browser yet -- see
/// api-client-spec.md section 18 build order steps 5-6). `vault` is never
/// serialized back to the frontend; only `send_request` reads it, and
/// only at actual send time (spec section 9).
#[derive(Default)]
struct EnvironmentState {
    vars: IndexMap<String, String>,
    vault: IndexMap<String, String>,
}

struct AppState {
    environment: Mutex<EnvironmentState>,
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
/// `mode` need to be set; the rest are ignored. Kept flat (rather than a
/// tagged enum) since it's just an IPC payload shape, not the canonical
/// `Auth` type -- `into_auth` does the real conversion.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthInput {
    mode: String,
    username: Option<String>,
    password: Option<String>,
    token: Option<String>,
    key: Option<String>,
    value: Option<String>,
    placement: Option<String>,
    access_token: Option<String>,
}

impl AuthInput {
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
/// references against the currently loaded environment (if any) first.
/// The `.apireq` file round trip, collections, and scripting all layer on
/// top of this same path later.
#[tauri::command]
async fn send_request(
    state: tauri::State<'_, AppState>,
    method: String,
    url: String,
    headers: HashMap<String, String>,
    body: Option<String>,
    auth: AuthInput,
) -> Result<SendResponseResult, String> {
    let (vars, vault) = {
        let env = state.environment.lock().unwrap();
        (env.vars.clone(), env.vault.clone())
    };

    let method = Method::from_bytes(method.to_uppercase().as_bytes()).map_err(|e| e.to_string())?;

    let visible_url = interpolate(&url, &vars);
    let visible_headers: IndexMap<String, String> =
        headers.iter().map(|(k, v)| (k.clone(), interpolate(v, &vars))).collect();

    let send_url = resolve_vault(&visible_url, &vault);
    let send_headers: IndexMap<String, String> =
        visible_headers.iter().map(|(k, v)| (k.clone(), resolve_vault(v, &vault))).collect();
    let resolved_auth = auth.into_auth().resolve(&vars, &vault);
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            environment: Mutex::new(EnvironmentState::default()),
        })
        .invoke_handler(tauri::generate_handler![send_request, load_environment, clear_environment])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
