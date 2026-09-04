use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("invalid .apireq format: {0}")]
    ParseFormat(String),

    #[error("invalid header name/value: {0}")]
    InvalidHeader(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("script error: {0}")]
    Script(String),

    #[error("history store error: {0}")]
    History(#[from] rusqlite::Error),
}

impl From<rquickjs::Error> for EngineError {
    fn from(e: rquickjs::Error) -> Self {
        EngineError::Script(e.to_string())
    }
}

/// Coarse, user-facing categorization of a failed request (spec section
/// 16: "DNS failure, timeout, TLS error, etc. -- categorized, not a
/// generic error badge"). reqwest/hyper don't expose a stable typed API
/// for *why* a connect failed, so beyond the two cases reqwest itself
/// distinguishes (`is_timeout`/`is_connect`), this falls back to
/// matching known substrings across the error's full source chain.
/// Heuristic, not exhaustive -- `Other` (reqwest's own message) is
/// always a safe fallback for whatever this doesn't recognize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestErrorKind {
    Dns,
    Timeout,
    Tls,
    ConnectionRefused,
    Other,
}

impl RequestErrorKind {
    pub fn label(&self) -> &'static str {
        match self {
            RequestErrorKind::Dns => "dns",
            RequestErrorKind::Timeout => "timeout",
            RequestErrorKind::Tls => "tls",
            RequestErrorKind::ConnectionRefused => "connection_refused",
            RequestErrorKind::Other => "other",
        }
    }
}

/// Returns the category plus a short, friendly explanation -- safe to
/// show directly in the UI and to log, since it never repeats the raw
/// error text for the categorized cases (which can otherwise leak
/// resolved URLs/hostnames from deep inside hyper/rustls' own error
/// messages beyond what the caller already redacts).
pub fn categorize_request_error(e: &reqwest::Error) -> (RequestErrorKind, String) {
    let chain = source_chain_text(e);

    let kind = if e.is_timeout() {
        RequestErrorKind::Timeout
    } else if chain.contains("dns error") || chain.contains("failed to lookup address") || chain.contains("no record found") {
        RequestErrorKind::Dns
    } else if chain.contains("certificate") || chain.contains("tls") || chain.contains("ssl") || chain.contains("handshake") {
        RequestErrorKind::Tls
    } else if chain.contains("connection refused") || chain.contains("actively refused") || chain.contains("os error 10061") {
        // "connection refused" (Linux/macOS) vs. Windows' own wording
        // ("actively refused it", os error 10061 / WSAECONNREFUSED).
        RequestErrorKind::ConnectionRefused
    } else {
        RequestErrorKind::Other
    };

    let message = match kind {
        RequestErrorKind::Dns => "Couldn't resolve the host name. Check the URL and your network connection.".to_string(),
        RequestErrorKind::Timeout => "The request timed out waiting for a response.".to_string(),
        RequestErrorKind::Tls => "TLS/certificate error while connecting. The server's certificate may be invalid or untrusted.".to_string(),
        RequestErrorKind::ConnectionRefused => "Connection refused. The server may be down or not listening on that port.".to_string(),
        RequestErrorKind::Other => e.to_string(),
    };

    (kind, message)
}

fn source_chain_text(e: &(dyn std::error::Error + 'static)) -> String {
    let mut text = e.to_string().to_lowercase();
    let mut source = e.source();
    while let Some(s) = source {
        text.push(' ');
        text.push_str(&s.to_string().to_lowercase());
        source = s.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // Both tests below talk to loopback only -- no real network/DNS
    // needed -- to stay consistent with the rest of this crate's tests
    // never making live external requests.

    #[tokio::test]
    async fn connection_refused_is_categorized() {
        // Port 1 is a reserved, virtually-never-bound port -- binding it
        // even requires elevated privileges on most systems, so nothing
        // should be listening there.
        let err = reqwest::Client::new().get("http://127.0.0.1:1").send().await.unwrap_err();
        let (kind, message) = categorize_request_error(&err);
        assert_eq!(kind, RequestErrorKind::ConnectionRefused);
        assert!(message.contains("Connection refused"));
    }

    #[tokio::test]
    async fn timeout_is_categorized() {
        // A listener that accepts the TCP connection but never writes an
        // HTTP response -- the client's own request timeout fires
        // instead, independent of any real network conditions.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                // Hold the connection open without responding.
                let _ = stream;
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });

        let client = reqwest::Client::builder().timeout(Duration::from_millis(100)).build().unwrap();
        let err = client.get(format!("http://{addr}")).send().await.unwrap_err();
        let (kind, message) = categorize_request_error(&err);
        assert_eq!(kind, RequestErrorKind::Timeout);
        assert!(message.contains("timed out"));
    }
}
