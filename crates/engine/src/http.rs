//! Thin wrapper around `reqwest` that turns a parsed `.apireq` into an
//! actual request/response cycle. Scripting (pre-request/post-response) and
//! the vault secrets boundary are separate concerns layered on top later
//! (see spec sections 4 and 9) — this module only knows about the wire.

use std::time::Instant;

use indexmap::IndexMap;
pub use reqwest::{Method, StatusCode};

use crate::error::EngineError;
use crate::format::{ApiRequestFile, Body};

#[derive(Debug, Clone)]
pub struct OutgoingRequest {
    pub method: Method,
    pub url: String,
    pub headers: IndexMap<String, String>,
    pub body: Option<OutgoingBody>,
}

#[derive(Debug, Clone)]
pub enum OutgoingBody {
    Json(String),
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct ResponseSummary {
    pub status: StatusCode,
    pub headers: IndexMap<String, String>,
    pub body: Vec<u8>,
    pub elapsed_ms: u128,
}

impl ResponseSummary {
    pub fn body_as_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

pub struct HttpClient {
    client: reqwest::Client,
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    pub async fn send(&self, req: OutgoingRequest) -> Result<ResponseSummary, EngineError> {
        let started = Instant::now();

        let mut builder = self.client.request(req.method, &req.url);
        for (key, value) in &req.headers {
            builder = builder.header(key, value);
        }
        builder = match req.body {
            Some(OutgoingBody::Json(s)) => builder
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(s),
            Some(OutgoingBody::Text(s)) => builder.body(s),
            Some(OutgoingBody::Bytes(b)) => builder.body(b),
            None => builder,
        };

        let response = builder.send().await?;
        let status = response.status();
        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
            .collect();
        let body = response.bytes().await?.to_vec();

        Ok(ResponseSummary {
            status,
            headers,
            body,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }
}

/// Builds an `OutgoingRequest` from a parsed `.apireq`, with `{{var}}`
/// interpolation already applied to the URL, headers, and body by the
/// caller (see `crate::vars::resolve_for_send`), `resolved_body` likewise
/// (pass `None` when the request has no body). Query/path params are
/// merged into the final URL by the caller, not here.
pub fn build_outgoing_request(
    file: &ApiRequestFile,
    resolved_url: String,
    resolved_headers: IndexMap<String, String>,
    resolved_body: Option<String>,
) -> Result<OutgoingRequest, EngineError> {
    let method = Method::from_bytes(file.method.to_uppercase().as_bytes())
        .map_err(|_| EngineError::ParseFormat(format!("unknown HTTP method '{}'", file.method)))?;

    // Every non-JSON body kind (raw/text/xml/form-data/urlencoded/graphql,
    // and binary until real file reads are wired up) is sent as opaque
    // text — only `body:json` gets the JSON content-type treatment.
    let body = resolved_body.map(|content| {
        if matches!(file.body, Some(Body::Json(_))) {
            OutgoingBody::Json(content)
        } else {
            OutgoingBody::Text(content)
        }
    });

    Ok(OutgoingRequest {
        method,
        url: resolved_url,
        headers: resolved_headers,
        body,
    })
}
