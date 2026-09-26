use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use hearth_protocol::ApiError;

use crate::db::DbError;

/// An error response: a status plus a stable code and a message fit for the user.
#[derive(Debug)]
pub struct AppError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

pub type ApiResult<T> = Result<axum::Json<T>, AppError>;

impl AppError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }

    pub fn not_found(what: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("{what} not found"),
        )
    }

    pub fn internal(err: impl std::fmt::Display) -> Self {
        tracing::error!("internal error: {err}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "Something went wrong inside the assistant. Check the daemon logs for details.",
        )
    }
}

impl From<DbError> for AppError {
    fn from(err: DbError) -> Self {
        Self::internal(err)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = ApiError {
            code: self.code.to_owned(),
            message: self.message,
        };
        (self.status, Json(body)).into_response()
    }
}
