//! API error types with consistent JSON responses.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// Consistent JSON error response for all API endpoints.
#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    pub error: String,
    pub code: String,
}

impl ApiError {
    pub fn not_found(msg: impl Into<String>) -> Self {
        ApiError {
            error: msg.into(),
            code: "program_not_found".to_string(),
        }
    }

    pub fn already_running() -> Self {
        ApiError {
            error: "Runtime is already running".to_string(),
            code: "runtime_already_running".to_string(),
        }
    }

    pub fn not_running() -> Self {
        ApiError {
            error: "Runtime is not running".to_string(),
            code: "runtime_not_running".to_string(),
        }
    }

    pub fn invalid_bundle(msg: impl Into<String>) -> Self {
        ApiError {
            error: msg.into(),
            code: "invalid_bundle".to_string(),
        }
    }

    /// Online change rejected because the new module is not layout-compatible
    /// with the running one. The HTTP `update` handler catches this and falls
    /// back to a stop+upload+start sequence; tests assert on the `code` field.
    pub fn online_change_incompatible(msg: impl Into<String>) -> Self {
        ApiError {
            error: msg.into(),
            code: "online_change_incompatible".to_string(),
        }
    }

    pub fn auth_required() -> Self {
        ApiError {
            error: "Authentication required".to_string(),
            code: "auth_required".to_string(),
        }
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        ApiError {
            error: msg.into(),
            code: "forbidden".to_string(),
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        ApiError {
            error: msg.into(),
            code: "internal_error".to_string(),
        }
    }

    fn status_code(&self) -> StatusCode {
        match self.code.as_str() {
            "program_not_found" => StatusCode::NOT_FOUND,
            "runtime_already_running"
            | "runtime_not_running"
            | "online_change_incompatible" => StatusCode::CONFLICT,
            "invalid_bundle" => StatusCode::BAD_REQUEST,
            "auth_required" => StatusCode::UNAUTHORIZED,
            "forbidden" => StatusCode::FORBIDDEN,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = axum::Json(self);
        (status, body).into_response()
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.error)
    }
}

// Status-code mapping and JSON serialization are exercised end-to-end by
// `crates/st-target-agent/tests/api_integration.rs` (every error test
// asserts both the HTTP status and the JSON `error`/`code` fields), so the
// previously-redundant `mod tests` here was removed.
