mod crash;
mod identity;
mod logging;
mod settings;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use fluxchunk_engine::collection::{self, resolve_inherited_auth, CollectionItem};
use fluxchunk_engine::diff::{self, DiffNode};
use fluxchunk_engine::error::{categorize_request_error, EngineError};
use fluxchunk_engine::format::{ApiKeyPlacement, ApiRequestFile, Auth, Body, EnvironmentFile, OAuth2Config, VaultFile};
use fluxchunk_engine::history::{self, HistoryEntrySummary, HistoryStore};
use fluxchunk_engine::http::{HttpClient, Method, OutgoingBody, OutgoingRequest};
use fluxchunk_engine::identity::LocalIdentity;
use fluxchunk_engine::import;
use fluxchunk_engine::response::{self, BodyPreview, Cookie};
use fluxchunk_engine::script::{self, ConsoleLevel, ScriptLimits, ScriptRequest, ScriptResponse};
use fluxchunk_engine::vars::{interpolate, merge_scopes, resolve_vault};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use settings::Settings;
use tauri::Manager;

/// Spec section 10's own default ("last N runs per request, default ~20,
/// configurable"). Not actually configurable yet -- that's a settings-tier
/// UI (section 11) that doesn't exist for anything else either.
const DEFAULT_HISTORY_RETENTION: u32 = 20;

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
    history: HistoryStore,
    settings_path: PathBuf,
    identity_path: PathBuf,
    /// This install's local keypair (spec section 6). Loaded once at
    /// startup (generated on first launch if missing); `set_display_name`
    /// is the only thing that ever mutates it afterward.
    identity: Mutex<LocalIdentity>,
    /// Where the panic hook writes redacted crash reports (spec section
    /// 16) -- `check_pending_crash`/`read_crash_report` both read from
    /// here, and `read_crash_report` refuses any path outside it.
    crash_dir: PathBuf,
    /// The `Update` handle from the most recent `check_for_updates` call,
    /// if one found something newer -- `download_update` and
    /// `install_and_restart` operate on it. `tokio::sync::Mutex` (not
    /// `std::sync::Mutex`) because it's held across `.await` points.
    pending_update: tokio::sync::Mutex<Option<tauri_plugin_updater::Update>>,
    /// The bytes `download_update` fetched, handed to `install_and_restart`
    /// -- spec section 13's "separate 'Install and Restart' action" means
    /// these two are genuinely different IPC calls, potentially with the
    /// user doing other things in between.
    pending_update_bytes: tokio::sync::Mutex<Option<Vec<u8>>>,
}

#[tauri::command]
fn load_settings(state: tauri::State<AppState>) -> Result<Settings, String> {
    settings::load(&state.settings_path)
}

#[tauri::command]
fn save_settings(state: tauri::State<AppState>, settings: Settings) -> Result<(), String> {
    logging::set_verbose(settings.verbose_logging);
    settings::save(&state.settings_path, &settings)
}

/// The local identity's public half only (spec section 6) -- what an
/// `.apiworkspace` approver block references. `secret_key` never
/// crosses IPC, same rule as vault secrets never reaching the UI.
#[derive(Debug, Serialize)]
struct LocalIdentityInfo {
    public_key: String,
    display_name: String,
}

impl From<&LocalIdentity> for LocalIdentityInfo {
    fn from(identity: &LocalIdentity) -> Self {
        LocalIdentityInfo { public_key: identity.public_key.clone(), display_name: identity.display_name.clone() }
    }
}

#[tauri::command]
fn get_identity(state: tauri::State<AppState>) -> LocalIdentityInfo {
    LocalIdentityInfo::from(&*state.identity.lock().unwrap())
}

/// Renames this identity without touching its keypair -- the public key
/// is what actually identifies an approver, the display name is just a
/// label for humans reading an `.apiworkspace` diff.
#[tauri::command]
fn set_display_name(state: tauri::State<AppState>, display_name: String) -> Result<LocalIdentityInfo, String> {
    let mut identity = state.identity.lock().unwrap();
    identity.display_name = display_name;
    identity::save(&state.identity_path, &identity)?;
    Ok(LocalIdentityInfo::from(&*identity))
}

/// Called once at launch (spec section 16, "next launch: calm 'app
/// closed unexpectedly' notice"). Returns `None` on an ordinary launch;
/// reading this clears the pending marker, so it only ever fires once
/// per crash, not on every subsequent launch.
#[tauri::command]
fn check_pending_crash(state: tauri::State<AppState>) -> Option<crash::CrashInfo> {
    crash::take_pending_crash(&state.crash_dir)
}

/// Backs "View details" on the crash notice. Refuses anything outside
/// our own crash directory -- the path always originates from our own
/// `check_pending_crash` response, but there's no reason to accept an
/// arbitrary-file-read path from the frontend regardless.
#[tauri::command]
fn read_crash_report(state: tauri::State<AppState>, path: String) -> Result<String, String> {
    let requested = PathBuf::from(&path);
    if requested.parent() != Some(state.crash_dir.as_path()) {
        return Err("refused: not a crash report path".to_string());
    }
    crash::read_report(&requested)
}

#[derive(Debug, Serialize)]
struct UpdateInfo {
    version: String,
    notes: Option<String>,
    pub_date: Option<String>,
}

/// Checks for an update against the configured endpoint (the enterprise
/// override in settings, if set, else the default baked into
/// tauri.conf.json). Returns `None` when already up to date. Every call
/// replaces any previously pending update/downloaded bytes -- "can check
/// again anytime" (spec section 13) always starts a fresh cycle rather
/// than risking a stale download getting installed against a check
/// result it doesn't match.
#[tauri::command]
async fn check_for_updates(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<Option<UpdateInfo>, String> {
    use tauri_plugin_updater::UpdaterExt;

    let current_settings = settings::load(&state.settings_path)?;
    let mut builder = app.updater_builder();
    if !current_settings.update_check_url.trim().is_empty() {
        let endpoint = url::Url::parse(current_settings.update_check_url.trim()).map_err(|e| format!("invalid update check URL: {e}"))?;
        builder = builder.endpoints(vec![endpoint]).map_err(|e| e.to_string())?;
    }
    let updater = builder.build().map_err(|e| e.to_string())?;
    let found = updater.check().await.map_err(|e| e.to_string())?;

    let info = found.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        notes: u.body.clone(),
        pub_date: u.date.map(|d| d.to_string()),
    });

    *state.pending_update.lock().await = found;
    *state.pending_update_bytes.lock().await = None;

    Ok(info)
}

/// Spec section 13's "Approve & Download" -- fetches the update `check_for_updates`
/// found, but doesn't install it yet.
#[tauri::command]
async fn download_update(state: tauri::State<'_, AppState>) -> Result<(), String> {
    crash::set_context("downloading update");
    let guard = state.pending_update.lock().await;
    let Some(update) = guard.as_ref() else {
        return Err("no update to download -- check for updates first".to_string());
    };
    let bytes = update.download(|_chunk_len, _total_len| {}, || {}).await.map_err(|e| e.to_string())?;
    drop(guard);
    *state.pending_update_bytes.lock().await = Some(bytes);
    Ok(())
}

/// Spec section 13's separate "Install and Restart" action -- applies the
/// already-downloaded bytes and relaunches. Requires `download_update` to
/// have completed first; won't silently re-download.
#[tauri::command]
async fn install_and_restart(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    crash::set_context("installing update");
    let update_guard = state.pending_update.lock().await;
    let Some(update) = update_guard.as_ref() else {
        return Err("no pending update -- check for updates first".to_string());
    };
    let bytes_guard = state.pending_update_bytes.lock().await;
    let Some(bytes) = bytes_guard.as_ref() else {
        return Err("update hasn't been downloaded yet".to_string());
    };
    update.install(bytes).map_err(|e| e.to_string())?;
    app.restart();
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
    crash::set_context("loading environment");
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

/// One `console.*` call from a pre-request or post-response script
/// (spec section 9's `console.*` -> Console panel), in call order --
/// pre-request's entries first, then post-response's.
#[derive(Debug, Clone, Serialize)]
struct ConsoleEntryDto {
    level: String,
    message: String,
}

fn map_console(entries: &[script::ConsoleEntry]) -> Vec<ConsoleEntryDto> {
    entries
        .iter()
        .map(|e| ConsoleEntryDto {
            level: match e.level {
                ConsoleLevel::Log => "log",
                ConsoleLevel::Info => "info",
                ConsoleLevel::Warn => "warn",
                ConsoleLevel::Error => "error",
            }
            .to_string(),
            message: e.message.clone(),
        })
        .collect()
}

/// Merges script-set vars into the active environment's in-memory vars
/// -- never written back to the `.apienv` file -- so a later send in the
/// same session sees them via `{{var}}` (spec section 4's own example:
/// a post-response script sets `last_user_id` for a later request to
/// use). Only touches keys a script actually changed, so a collection-
/// scoped var a script never mutated doesn't get silently promoted into
/// environment scope.
fn write_back_script_vars(environment: &Mutex<EnvironmentState>, vars: &IndexMap<String, String>) {
    let mut env = environment.lock().unwrap();
    for (k, v) in vars {
        if env.vars.get(k) != Some(v) {
            env.vars.insert(k.clone(), v.clone());
        }
    }
}

#[derive(Debug, Serialize)]
struct SendResponseResult {
    status: u16,
    status_text: String,
    headers: HashMap<String, String>,
    /// Classified/gated per spec section 10 -- large bodies come back
    /// without `json`/`text` populated; see `BodyPreview::exceeds_threshold`
    /// and `load_full_response_body`.
    body: BodyPreview,
    cookies: Vec<Cookie>,
    elapsed_ms: u128,
    /// The URL actually requested, with `{{var}}`s resolved but
    /// `{{vault:...}}` refs deliberately left alone -- safe to show in
    /// the UI. Same rule `apicli` follows: a resolved secret is never
    /// echoed anywhere outside the actual outgoing request.
    resolved_url: String,
    /// This send's row in response history -- already recorded by the
    /// time this returns, so "compare against a past run" is just
    /// diffing two history ids (see `diff_history`), this one included.
    history_id: i64,
    /// Combined `console.*` output from both script phases. A
    /// post-response script *error* also lands here as one more "error"
    /// entry rather than failing the whole call -- the response already
    /// succeeded and matters more than the test script that ran after it.
    console: Vec<ConsoleEntryDto>,
}

/// A failed `send_request` call, categorized (spec section 16: "DNS
/// failure, timeout, TLS error, etc. -- categorized, not a generic error
/// badge") rather than a flat error string, so the frontend can show an
/// explanation specific to what actually went wrong.
#[derive(Debug, Serialize)]
struct RequestFailure {
    /// One of the `RequestErrorKind` labels ("dns"/"timeout"/"tls"/
    /// "connection_refused"/"other"), "script" for a pre-request script
    /// that threw (spec section 16: surfaced in the Console panel, with
    /// line numbers, not the same DNS/TLS-style card), or "internal" for
    /// anything else before/after the network call (bad method, local
    /// history-store error).
    kind: String,
    message: String,
    /// Whatever console output happened before the failure -- e.g. a
    /// pre-request script that logged something and then threw.
    console: Vec<ConsoleEntryDto>,
}

impl RequestFailure {
    fn internal(message: impl Into<String>, console: Vec<ConsoleEntryDto>) -> Self {
        let message = message.into();
        logging::error(format!("send_request failed before/after the network call: {message}"));
        RequestFailure { kind: "internal".to_string(), message, console }
    }
}

/// Sends a single request, resolving `{{var}}` / `{{vault:...}}`
/// references against the currently loaded environment and collection
/// (if any) first, and resolving `auth { mode: inherit }` against the
/// collection's auth. Runs the pre-request script (if any) before
/// building the outgoing request and the post-response script (if any)
/// after it comes back -- same order as `apicli`'s send path. Records
/// the result to response history under `request_key` (the caller's
/// choice -- the frontend uses a saved request's file path, or a stable
/// per-tab id for ad-hoc ones) before returning.
#[tauri::command]
async fn send_request(
    state: tauri::State<'_, AppState>,
    request_key: String,
    request_label: String,
    method: String,
    url: String,
    headers: HashMap<String, String>,
    body: Option<String>,
    auth: AuthPayload,
    script_pre_request: Option<String>,
    script_post_response: Option<String>,
) -> Result<SendResponseResult, RequestFailure> {
    crash::set_context("sending request");

    let (env_vars, vault) = {
        let env = state.environment.lock().unwrap();
        (env.vars.clone(), env.vault.clone())
    };
    let (collection_vars, collection_auth) = {
        let col = state.collection.lock().unwrap();
        (col.vars.clone(), col.auth.clone())
    };
    let mut vars = merge_scopes(&[&collection_vars, &env_vars]);

    let method_parsed =
        Method::from_bytes(method.to_uppercase().as_bytes()).map_err(|e| RequestFailure::internal(e.to_string(), Vec::new()))?;

    let mut visible_url = interpolate(&url, &vars);
    let mut visible_headers: IndexMap<String, String> =
        headers.iter().map(|(k, v)| (k.clone(), interpolate(v, &vars))).collect();
    let mut visible_body = body.as_deref().map(|b| interpolate(b, &vars));

    let mut console_entries: Vec<ConsoleEntryDto> = Vec::new();

    if let Some(script_source) = script_pre_request.as_deref().filter(|s| !s.trim().is_empty()) {
        let script_req = ScriptRequest {
            method: method.to_uppercase(),
            url: visible_url.clone(),
            headers: visible_headers.clone(),
            body: visible_body.clone(),
        };
        match script::run_pre_request(script_source, &vars, &script_req, &ScriptLimits::default()) {
            Ok(outcome) => {
                console_entries.extend(map_console(&outcome.console));
                vars = outcome.vars;
                visible_url = outcome.request.url;
                visible_headers = outcome.request.headers;
                visible_body = outcome.request.body;
                write_back_script_vars(&state.environment, &vars);
            }
            Err(e) => {
                let message = e.to_string();
                logging::warn(format!("pre-request script failed: {message}"));
                console_entries.push(ConsoleEntryDto { level: "error".to_string(), message: message.clone() });
                return Err(RequestFailure { kind: "script".to_string(), message, console: console_entries });
            }
        }
    }

    let send_url = resolve_vault(&visible_url, &vault);
    let send_headers: IndexMap<String, String> =
        visible_headers.iter().map(|(k, v)| (k.clone(), resolve_vault(v, &vault))).collect();
    let resolved_auth = resolve_inherited_auth(&auth.into_auth(), &collection_auth).resolve(&vars, &vault);
    let resolved_body = visible_body.as_deref().map(|b| resolve_vault(b, &vault));

    // Verbose logging (spec section 16) only ever sees the *visible*
    // headers/body -- interpolated but with `{{vault:...}}` still
    // unresolved, exactly what's already shown in the UI -- never the
    // fully-resolved wire content the vault produced.
    if logging::is_verbose() {
        let visible_headers_str = visible_headers.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join(", ");
        logging::log_verbose_request(&visible_headers_str, visible_body.as_deref());
    }

    let outgoing = OutgoingRequest {
        method: method_parsed,
        url: send_url.clone(),
        headers: send_headers,
        auth: resolved_auth,
        body: resolved_body.map(OutgoingBody::Text),
    };

    let client = HttpClient::new();
    let resp = match client.send(outgoing).await {
        Ok(resp) => resp,
        Err(EngineError::Request(e)) => {
            let (kind, message) = categorize_request_error(&e);
            logging::log_request_failed(&method, &send_url, kind.label(), &message);
            return Err(RequestFailure { kind: kind.label().to_string(), message, console: console_entries });
        }
        Err(other) => return Err(RequestFailure::internal(other.to_string(), console_entries)),
    };

    let status = resp.status.as_u16();
    let status_text = resp.status.canonical_reason().unwrap_or("").to_string();
    let content_type = resp.headers.get("content-type").cloned();

    logging::log_request_sent(&method, &send_url, status, resp.elapsed_ms);

    let body_preview = response::classify_and_preview(content_type.as_deref(), &resp.body, false);
    let cookies = response::parse_set_cookie_headers(&resp.set_cookie_headers);
    let headers_json = serde_json::to_string(&resp.headers).unwrap_or_else(|_| "{}".to_string());
    let cookies_json = serde_json::to_string(&resp.set_cookie_headers).unwrap_or_else(|_| "[]".to_string());

    if let Some(script_source) = script_post_response.as_deref().filter(|s| !s.trim().is_empty()) {
        let script_res = ScriptResponse {
            status,
            headers: resp.headers.clone(),
            body: resp.body_as_text(),
        };
        match script::run_post_response(script_source, &vars, &script_res, &ScriptLimits::default()) {
            Ok(outcome) => {
                console_entries.extend(map_console(&outcome.console));
                vars = outcome.vars;
                write_back_script_vars(&state.environment, &vars);
            }
            Err(e) => {
                let message = e.to_string();
                logging::warn(format!("post-response script failed: {message}"));
                console_entries.push(ConsoleEntryDto { level: "error".to_string(), message });
            }
        }
    }

    // History storage happens here, on the actual response bytes, *before*
    // they're gated by the size threshold above -- the gate only affects
    // what crosses IPC to the webview right now, not what's kept for
    // later ("Load full response", or opening this from history).
    let history_id = state
        .history
        .record(
            history::NewHistoryEntry {
                request_key,
                request_label,
                method: method.to_uppercase(),
                url: send_url,
                status,
                status_text: status_text.clone(),
                headers_json,
                cookies_json,
                body: resp.body,
                content_type,
                elapsed_ms: resp.elapsed_ms as u64,
            },
            DEFAULT_HISTORY_RETENTION,
        )
        .map_err(|e| RequestFailure::internal(e.to_string(), console_entries.clone()))?;

    Ok(SendResponseResult {
        status,
        status_text,
        headers: resp.headers.into_iter().collect(),
        body: body_preview,
        cookies,
        elapsed_ms: resp.elapsed_ms,
        resolved_url: visible_url,
        history_id,
        console: console_entries,
    })
}

/// Bypasses the size gate for one already-recorded response -- the "Load
/// full response" action, or opening a large historical entry.
#[tauri::command]
fn load_full_response_body(state: tauri::State<AppState>, history_id: i64) -> Result<BodyPreview, String> {
    let entry = state
        .history
        .get(history_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "history entry not found".to_string())?;
    Ok(response::classify_and_preview(entry.content_type.as_deref(), &entry.body, true))
}

#[tauri::command]
fn list_history(state: tauri::State<AppState>, request_key: String) -> Result<Vec<HistoryEntrySummary>, String> {
    state.history.list_for_request(&request_key, DEFAULT_HISTORY_RETENTION).map_err(|e| e.to_string())
}

#[tauri::command]
fn clear_history(state: tauri::State<AppState>, request_key: String) -> Result<(), String> {
    state.history.clear_for_request(&request_key).map_err(|e| e.to_string())
}

#[derive(Debug, Serialize)]
struct HistoryEntryDetail {
    id: i64,
    method: String,
    url: String,
    status: u16,
    status_text: String,
    headers: HashMap<String, String>,
    cookies: Vec<Cookie>,
    body: BodyPreview,
    elapsed_ms: u64,
    sent_at: i64,
}

/// Full detail for one history entry (status/headers/cookies/body,
/// size-gated the same way a live send is) -- for viewing a past run in
/// the response panel.
#[tauri::command]
fn get_history_entry(state: tauri::State<AppState>, id: i64) -> Result<HistoryEntryDetail, String> {
    let entry = state.history.get(id).map_err(|e| e.to_string())?.ok_or_else(|| "history entry not found".to_string())?;
    let headers: HashMap<String, String> = serde_json::from_str(&entry.headers_json).unwrap_or_default();
    let set_cookie_headers: Vec<String> = serde_json::from_str(&entry.cookies_json).unwrap_or_default();

    Ok(HistoryEntryDetail {
        id: entry.id,
        method: entry.method,
        url: entry.url,
        status: entry.status,
        status_text: entry.status_text,
        cookies: response::parse_set_cookie_headers(&set_cookie_headers),
        body: response::classify_and_preview(entry.content_type.as_deref(), &entry.body, false),
        headers,
        elapsed_ms: entry.elapsed_ms,
        sent_at: entry.sent_at,
    })
}

/// Structural diff between two history entries' bodies (spec section 10)
/// -- comparing the live response against a past run is just this with
/// `a` set to the id `send_request` just returned, since that send is
/// already in history too.
#[tauri::command]
fn diff_history(state: tauri::State<AppState>, a: i64, b: i64) -> Result<DiffNode, String> {
    let entry_a = state.history.get(a).map_err(|e| e.to_string())?.ok_or_else(|| "history entry A not found".to_string())?;
    let entry_b = state.history.get(b).map_err(|e| e.to_string())?.ok_or_else(|| "history entry B not found".to_string())?;

    let json_a: serde_json::Value =
        serde_json::from_slice(&entry_a.body).map_err(|_| "entry A's body isn't JSON -- structural diff only supports JSON".to_string())?;
    let json_b: serde_json::Value =
        serde_json::from_slice(&entry_b.body).map_err(|_| "entry B's body isn't JSON -- structural diff only supports JSON".to_string())?;

    Ok(diff::diff_json(&json_a, &json_b))
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
    crash::set_context("opening collection");
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
struct FindingDto {
    severity: String,
    rule: String,
    message: String,
    snippet: String,
}

impl From<&fluxchunk_engine::security::Finding> for FindingDto {
    fn from(f: &fluxchunk_engine::security::Finding) -> Self {
        FindingDto {
            severity: match f.severity {
                fluxchunk_engine::security::Severity::Critical => "critical",
                fluxchunk_engine::security::Severity::Warning => "warning",
            }
            .to_string(),
            rule: f.rule.clone(),
            message: f.message.clone(),
            snippet: f.snippet.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
struct RequestFindingsDto {
    request_name: String,
    findings: Vec<FindingDto>,
}

/// Spec section 8's dialog 1 ("Import summary... Cancel or Scan &
/// Continue"): counts plus, since the scan already ran, whatever it
/// found. The frontend decides whether to show dialog 2 based on whether
/// `security_findings` is empty.
#[derive(Debug, Serialize)]
struct ImportPreview {
    name: String,
    request_count: usize,
    parse_warnings: Vec<String>,
    security_findings: Vec<RequestFindingsDto>,
}

fn preview_of(imported: &import::ImportedCollection) -> ImportPreview {
    let security_findings = import::scan_imported_collection(imported)
        .into_iter()
        .map(|(request_name, findings)| RequestFindingsDto {
            request_name,
            findings: findings.iter().map(FindingDto::from).collect(),
        })
        .collect();

    ImportPreview {
        name: imported.name.clone(),
        request_count: imported.requests.len(),
        parse_warnings: imported.warnings.clone(),
        security_findings,
    }
}

#[derive(Debug, Serialize)]
struct ImportSummary {
    name: String,
    /// Where it actually landed -- `<parent_dir>/<slugified name>` -- so
    /// the frontend can immediately `open_collection` it without having
    /// to reconstruct the slug itself.
    collection_path: String,
    request_count: usize,
    warnings: Vec<String>,
}

fn commit_import(mut imported: import::ImportedCollection, parent_dir: &str, skip_flagged: bool) -> Result<ImportSummary, String> {
    if skip_flagged {
        import::strip_flagged_scripts(&mut imported);
    }

    let slug = import::slugify(&imported.name);
    let dest = PathBuf::from(parent_dir).join(if slug.is_empty() { "imported-collection".to_string() } else { slug });
    import::write_imported_collection(&imported, &dest).map_err(|e| e.to_string())?;

    Ok(ImportSummary {
        name: imported.name,
        collection_path: dest.to_string_lossy().to_string(),
        request_count: imported.requests.len(),
        warnings: imported.warnings,
    })
}

/// Parses a Postman Collection v2.1 JSON export and runs the security
/// scan (spec section 8) without writing anything -- dialog 1's data.
#[tauri::command]
fn preview_postman_import(source_path: String) -> Result<ImportPreview, String> {
    let json = std::fs::read_to_string(&source_path).map_err(|e| format!("couldn't read {source_path}: {e}"))?;
    let imported = import::postman::import_postman_collection(&json).map_err(|e| e.to_string())?;
    Ok(preview_of(&imported))
}

/// Same, for an OpenAPI 3.x or Swagger 2.0 JSON document. YAML specs
/// aren't supported yet -- see the module docs on `fluxchunk_engine::import::openapi`.
#[tauri::command]
fn preview_openapi_import(source_path: String) -> Result<ImportPreview, String> {
    let json = std::fs::read_to_string(&source_path).map_err(|e| format!("couldn't read {source_path}: {e}"))?;
    let imported = import::openapi::import_openapi_spec(&json).map_err(|e| e.to_string())?;
    Ok(preview_of(&imported))
}

/// Re-parses (parsing is cheap; there's no server-side session to hold
/// the previewed result across the two dialogs) and writes the result --
/// spec section 8's "Import & Skip Flagged Scripts" / "Import Anyway".
/// "Reject Import" needs no backend call at all; the frontend just never
/// calls this.
#[tauri::command]
fn commit_postman_import(source_path: String, parent_dir: String, skip_flagged: bool) -> Result<ImportSummary, String> {
    crash::set_context("importing collection");
    let json = std::fs::read_to_string(&source_path).map_err(|e| format!("couldn't read {source_path}: {e}"))?;
    let imported = import::postman::import_postman_collection(&json).map_err(|e| e.to_string())?;
    commit_import(imported, &parent_dir, skip_flagged)
}

#[tauri::command]
fn commit_openapi_import(source_path: String, parent_dir: String, skip_flagged: bool) -> Result<ImportSummary, String> {
    crash::set_context("importing collection");
    let json = std::fs::read_to_string(&source_path).map_err(|e| format!("couldn't read {source_path}: {e}"))?;
    let imported = import::openapi::import_openapi_spec(&json).map_err(|e| e.to_string())?;
    commit_import(imported, &parent_dir, skip_flagged)
}

#[derive(Debug, Serialize)]
struct RequestSummary {
    name: String,
    method: String,
    url: String,
    headers: HashMap<String, String>,
    auth: AuthPayload,
    body: Option<String>,
    script_pre_request: Option<String>,
    script_post_response: Option<String>,
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
        script_pre_request: file.script_pre_request.clone(),
        script_post_response: file.script_post_response.clone(),
    })
}

/// Writes the tab's edited method/url/headers/auth/body/scripts back
/// into the original `.apireq` file. Re-reads that file fresh first and
/// only overwrites those fields, so anything the UI doesn't (yet)
/// expose -- meta, params, asserts, unrecognized blocks -- survives
/// untouched rather than being silently dropped.
#[tauri::command]
fn save_request(
    path: String,
    method: String,
    url: String,
    headers: HashMap<String, String>,
    auth: AuthPayload,
    body: Option<String>,
    script_pre_request: Option<String>,
    script_post_response: Option<String>,
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
    file.script_pre_request = script_pre_request.filter(|s| !s.is_empty());
    file.script_post_response = script_post_response.filter(|s| !s.is_empty());

    std::fs::write(&path, file.to_string_pretty()).map_err(|e| format!("couldn't write {}: {e}", path.display()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init());

    // The updater plugin isn't meaningful (and may not build) on mobile
    // targets -- there's no mobile build of FluxChunk yet, but this
    // matches the plugin's own documented guidance and the mobile-only
    // cfg_attr already present below, rather than assuming desktop-only
    // forever.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());

    builder
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let settings_path = app_data_dir.join("config.toml");

            // Logging and the crash-report panic hook both need to be up
            // before anything else runs -- a panic during, say, opening
            // the history store should still produce a report.
            logging::init(&app_data_dir.join("logs"))?;
            logging::set_verbose(settings::load(&settings_path)?.verbose_logging);
            let crash_dir = app_data_dir.join("crashes");
            crash::install_panic_hook(crash_dir.clone());

            let history = HistoryStore::open(&app_data_dir.join("history.sqlite3"))?;
            let identity_path = app_data_dir.join("identity.toml");
            let identity = identity::load_or_create(&identity_path)?;
            app.manage(AppState {
                environment: Mutex::new(EnvironmentState::default()),
                collection: Mutex::new(CollectionState::default()),
                history,
                settings_path,
                identity_path,
                identity: Mutex::new(identity),
                crash_dir,
                pending_update: tokio::sync::Mutex::new(None),
                pending_update_bytes: tokio::sync::Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            send_request,
            load_environment,
            clear_environment,
            open_collection,
            close_collection,
            preview_postman_import,
            preview_openapi_import,
            commit_postman_import,
            commit_openapi_import,
            read_request,
            save_request,
            load_full_response_body,
            list_history,
            clear_history,
            get_history_entry,
            diff_history,
            load_settings,
            save_settings,
            check_for_updates,
            download_update,
            install_and_restart,
            check_pending_crash,
            read_crash_report,
            get_identity,
            set_display_name
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
