//! Response body classification and the large-body gate (spec section 10:
//! "Large bodies (>5MB threshold) parsed in the Rust engine, not the
//! webview -- ship a lightweight tree structure to the frontend, not a
//! raw multi-MB string" / "Explicit 'Load full response' action past the
//! size threshold"), plus `Set-Cookie` parsing for the Cookies tab.
//!
//! Virtualized *rendering* of the resulting tree is a frontend concern
//! (`apps/desktop/src/lib/JsonTree.svelte`) and, per that component's
//! notes, isn't full DOM windowing yet -- a real virtualization pass is
//! its own follow-up.

use serde::Serialize;

/// Spec section 10's own number.
pub const LARGE_BODY_THRESHOLD: usize = 5 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BodyKind {
    Json,
    Html,
    Text,
    Image,
    Pdf,
    Binary,
}

/// What the frontend needs to render the Pretty/Raw/Preview tabs, without
/// necessarily including the actual content -- see `exceeds_threshold`.
#[derive(Debug, Clone, Serialize)]
pub struct BodyPreview {
    pub kind: BodyKind,
    pub content_type: Option<String>,
    pub size_bytes: usize,
    /// `true` when `size_bytes > LARGE_BODY_THRESHOLD` and `force` was
    /// `false` -- `json`/`text` are `None` in that case; the frontend
    /// shows a gate ("Response is 8.2 MB -- Load full response?") instead
    /// of silently rendering it, and calls back in with `force: true` (or
    /// fetches a stored history entry the same way) to actually get it.
    pub exceeds_threshold: bool,
    /// Present only for `BodyKind::Json`, and only when not gated.
    pub json: Option<serde_json::Value>,
    /// Present for `Html`/`Text`, and only when not gated. `Json` bodies
    /// use `json` instead once parsed; if JSON parsing fails despite a
    /// `application/json` content-type, this carries the raw text so
    /// there's still something to show.
    pub text: Option<String>,
}

fn classify(content_type: Option<&str>, bytes: &[u8]) -> BodyKind {
    let content_type = content_type.unwrap_or("").to_ascii_lowercase();
    if content_type.contains("application/json") || content_type.contains("+json") {
        BodyKind::Json
    } else if content_type.contains("text/html") {
        BodyKind::Html
    } else if content_type.starts_with("image/") {
        BodyKind::Image
    } else if content_type.contains("application/pdf") {
        BodyKind::Pdf
    } else if content_type.starts_with("text/")
        || content_type.contains("xml")
        || content_type.contains("application/javascript")
        || (content_type.is_empty() && std::str::from_utf8(bytes).is_ok())
    {
        BodyKind::Text
    } else {
        BodyKind::Binary
    }
}

/// Classifies `bytes` and, unless it's gated behind the size threshold,
/// parses it into something render-ready. Pass `force: true` to bypass
/// the gate (the "Load full response" action, or opening a history
/// entry someone already explicitly asked to see).
pub fn classify_and_preview(content_type: Option<&str>, bytes: &[u8], force: bool) -> BodyPreview {
    let kind = classify(content_type, bytes);
    let size_bytes = bytes.len();
    let exceeds_threshold = size_bytes > LARGE_BODY_THRESHOLD && !force;

    let (json, text) = if exceeds_threshold {
        (None, None)
    } else {
        match kind {
            BodyKind::Json => {
                let text_lossy = String::from_utf8_lossy(bytes);
                match serde_json::from_str::<serde_json::Value>(&text_lossy) {
                    Ok(value) => (Some(value), None),
                    Err(_) => (None, Some(text_lossy.into_owned())),
                }
            }
            BodyKind::Html | BodyKind::Text => (None, Some(String::from_utf8_lossy(bytes).into_owned())),
            BodyKind::Image | BodyKind::Pdf | BodyKind::Binary => (None, None),
        }
    };

    BodyPreview {
        kind,
        content_type: content_type.map(|s| s.to_string()),
        size_bytes,
        exceeds_threshold,
        json,
        text,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: Option<String>,
    pub path: Option<String>,
    pub expires: Option<String>,
    pub max_age: Option<String>,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: Option<String>,
}

/// Parses a set of raw `Set-Cookie` header values (spec section 10's
/// Cookies tab). Not RFC 6265-exhaustive -- good enough to display what a
/// response actually set, which is the tab's whole job.
pub fn parse_set_cookie_headers(headers: &[String]) -> Vec<Cookie> {
    headers.iter().filter_map(|h| parse_one(h)).collect()
}

fn parse_one(header: &str) -> Option<Cookie> {
    let mut parts = header.split(';');
    let (name, value) = parts.next()?.trim().split_once('=')?;

    let mut cookie = Cookie {
        name: name.trim().to_string(),
        value: value.trim().to_string(),
        domain: None,
        path: None,
        expires: None,
        max_age: None,
        secure: false,
        http_only: false,
        same_site: None,
    };

    for attr in parts {
        let attr = attr.trim();
        if attr.is_empty() {
            continue;
        }
        match attr.split_once('=') {
            Some((k, v)) => match k.trim().to_ascii_lowercase().as_str() {
                "domain" => cookie.domain = Some(v.trim().to_string()),
                "path" => cookie.path = Some(v.trim().to_string()),
                "expires" => cookie.expires = Some(v.trim().to_string()),
                "max-age" => cookie.max_age = Some(v.trim().to_string()),
                "samesite" => cookie.same_site = Some(v.trim().to_string()),
                _ => {}
            },
            None => match attr.to_ascii_lowercase().as_str() {
                "secure" => cookie.secure = true,
                "httponly" => cookie.http_only = true,
                _ => {}
            },
        }
    }

    Some(cookie)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_json_by_content_type() {
        let preview = classify_and_preview(Some("application/json; charset=utf-8"), br#"{"a":1}"#, false);
        assert_eq!(preview.kind, BodyKind::Json);
        assert_eq!(preview.json, Some(serde_json::json!({"a": 1})));
    }

    #[test]
    fn json_content_type_with_invalid_json_falls_back_to_text() {
        let preview = classify_and_preview(Some("application/json"), b"not json", false);
        assert_eq!(preview.kind, BodyKind::Json);
        assert!(preview.json.is_none());
        assert_eq!(preview.text.as_deref(), Some("not json"));
    }

    #[test]
    fn classifies_html() {
        let preview = classify_and_preview(Some("text/html; charset=utf-8"), b"<h1>hi</h1>", false);
        assert_eq!(preview.kind, BodyKind::Html);
        assert_eq!(preview.text.as_deref(), Some("<h1>hi</h1>"));
    }

    #[test]
    fn classifies_image_and_pdf_as_binary_no_content() {
        let img = classify_and_preview(Some("image/png"), &[0u8; 10], false);
        assert_eq!(img.kind, BodyKind::Image);
        assert!(img.json.is_none() && img.text.is_none());

        let pdf = classify_and_preview(Some("application/pdf"), &[0u8; 10], false);
        assert_eq!(pdf.kind, BodyKind::Pdf);
    }

    #[test]
    fn unknown_binary_content_type_is_binary() {
        let preview = classify_and_preview(Some("application/octet-stream"), &[0u8; 10], false);
        assert_eq!(preview.kind, BodyKind::Binary);
    }

    #[test]
    fn large_body_is_gated_unless_forced() {
        let big = vec![b'x'; LARGE_BODY_THRESHOLD + 1];
        let gated = classify_and_preview(Some("text/plain"), &big, false);
        assert!(gated.exceeds_threshold);
        assert!(gated.text.is_none());

        let forced = classify_and_preview(Some("text/plain"), &big, true);
        assert!(!forced.exceeds_threshold);
        assert!(forced.text.is_some());
    }

    #[test]
    fn exactly_at_threshold_is_not_gated() {
        let exact = vec![b'x'; LARGE_BODY_THRESHOLD];
        let preview = classify_and_preview(Some("text/plain"), &exact, false);
        assert!(!preview.exceeds_threshold);
    }

    #[test]
    fn parses_basic_cookie() {
        let cookies = parse_set_cookie_headers(&["session=abc123; Path=/; HttpOnly; Secure; SameSite=Strict".to_string()]);
        assert_eq!(cookies.len(), 1);
        let c = &cookies[0];
        assert_eq!(c.name, "session");
        assert_eq!(c.value, "abc123");
        assert_eq!(c.path.as_deref(), Some("/"));
        assert!(c.http_only);
        assert!(c.secure);
        assert_eq!(c.same_site.as_deref(), Some("Strict"));
    }

    #[test]
    fn parses_multiple_set_cookie_headers() {
        let cookies = parse_set_cookie_headers(&["a=1".to_string(), "b=2; Domain=example.com".to_string()]);
        assert_eq!(cookies.len(), 2);
        assert_eq!(cookies[0].name, "a");
        assert_eq!(cookies[1].domain.as_deref(), Some("example.com"));
    }

    #[test]
    fn malformed_cookie_without_equals_is_skipped_not_panicking() {
        let cookies = parse_set_cookie_headers(&["garbage".to_string(), "ok=1".to_string()]);
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].name, "ok");
    }
}
