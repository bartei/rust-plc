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
            "runtime_already_running" | "runtime_not_running" => StatusCode::CONFLICT,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_map_to_correct_status() {
        assert_eq!(ApiError::not_found("x").status_code(), StatusCode::NOT_FOUND);
        assert_eq!(ApiError::already_running().status_code(), StatusCode::CONFLICT);
        assert_eq!(ApiError::not_running().status_code(), StatusCode::CONFLICT);
        assert_eq!(ApiError::invalid_bundle("x").status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(ApiError::auth_required().status_code(), StatusCode::UNAUTHORIZED);
        assert_eq!(ApiError::forbidden("x").status_code(), StatusCode::FORBIDDEN);
        assert_eq!(ApiError::internal("x").status_code(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn error_serializes_to_json() {
        let err = ApiError::not_found("No program deployed");
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("program_not_found"));
        assert!(json.contains("No program deployed"));
    }
}
