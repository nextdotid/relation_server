use lambda_http::http::StatusCode;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    // general
    #[error("{0}")]
    General(String, StatusCode),
    // http
    #[error("Param missing: {0}")]
    ParamMissing(String),
    #[error("Param error: {0}")]
    ParamError(String),
    #[error("no body provided")]
    BodyMissing,
    #[error("JSON parse error")]
    NoResult,
    #[error("No result")]
    ParseError(#[from] serde_json::error::Error),
    #[error("HTTP general error")]
    HttpError(#[from] lambda_http::http::Error),
    #[error("Config error: {0}")]
    ConfigError(#[from] config::ConfigError),
    #[error("Database error: {0}")]
    SignatureValidationError(String),
    #[error("Parse hex error: {0}")]
    HttpClientError(#[from] hyper::Error),
    #[error("ArangoDB error: {0}")]
    ArangoDBError(#[from] aragog::Error),
}

impl Error {
    pub fn http_status(&self) -> StatusCode {
        match self {
            Error::General(_, status) => *status,
            Error::ParamMissing(_) => StatusCode::BAD_REQUEST,
            Error::ParamError(_) => StatusCode::BAD_REQUEST,
            Error::BodyMissing => StatusCode::BAD_REQUEST,
            Error::ParseError(_) => StatusCode::BAD_REQUEST,
            Error::NoResult => StatusCode::BAD_REQUEST,
            Error::HttpError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::ConfigError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::HttpClientError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::SignatureValidationError(_) => StatusCode::BAD_REQUEST,
            Error::ArangoDBError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
