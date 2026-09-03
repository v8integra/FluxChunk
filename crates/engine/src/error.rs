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
}

impl From<rquickjs::Error> for EngineError {
    fn from(e: rquickjs::Error) -> Self {
        EngineError::Script(e.to_string())
    }
}
