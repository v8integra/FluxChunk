use std::collections::HashMap;

use fluxchunk_engine::format::Auth;
use fluxchunk_engine::http::{HttpClient, Method, OutgoingBody, OutgoingRequest};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct SendResponseResult {
    status: u16,
    status_text: String,
    headers: HashMap<String, String>,
    body: String,
    elapsed_ms: u128,
}

/// Sends a single request. This is the minimal slice needed for the basic
/// request/response UI (build order step 2) — variable interpolation from
/// environments, the `.apireq` file round trip, and scripting all layer on
/// top of this same `fluxchunk_engine::http` path later.
#[tauri::command]
async fn send_request(
    method: String,
    url: String,
    headers: HashMap<String, String>,
    body: Option<String>,
) -> Result<SendResponseResult, String> {
    let method = Method::from_bytes(method.to_uppercase().as_bytes()).map_err(|e| e.to_string())?;

    let outgoing = OutgoingRequest {
        method,
        url,
        headers: headers.into_iter().collect(),
        // No Auth tab in the UI yet -- same follow-up as the environment
        // picker (see engine/src/format/auth.rs for the format+resolve
        // side, which is already wired up end to end via apicli).
        auth: Auth::None,
        body: body.map(OutgoingBody::Text),
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
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![send_request])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
