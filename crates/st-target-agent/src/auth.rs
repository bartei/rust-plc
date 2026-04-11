//! Bearer token authentication middleware for axum.

use crate::config::AuthMode;
use crate::error::ApiError;
use crate::server::AppState;
use axum::extract::{Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::Response;
use std::sync::Arc;

/// axum middleware that validates Bearer token authentication.
///
/// - `AuthMode::None`: all requests pass through
/// - `AuthMode::Token`: requires `Authorization: Bearer <token>` header
/// - `read_only`: rejects POST/PUT/DELETE when enabled (allows GET/HEAD/OPTIONS)
///
/// The `/api/v1/health` endpoint is always exempt from auth (for load balancers).
pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    // Health endpoint is always public
    if request.uri().path() == "/api/v1/health" {
        return Ok(next.run(request).await);
    }

    match state.config.auth.mode {
        AuthMode::None => Ok(next.run(request).await),
        AuthMode::Token => {
            let token = request
                .headers()
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "));

            let expected = state.config.auth.token.as_deref();

            match (token, expected) {
                (Some(t), Some(exp)) if t == exp => {
                    // Token valid — check read-only mode
                    if state.config.auth.read_only {
                        let method = request.method().clone();
                        if method == Method::POST
                            || method == Method::PUT
                            || method == Method::DELETE
                        {
                            return Err(ApiError::forbidden(
                                "Agent is in read-only mode",
                            ));
                        }
                    }
                    Ok(next.run(request).await)
                }
                (None, _) => Err(ApiError::auth_required()),
                _ => Err(ApiError::forbidden("Invalid authentication token")),
            }
        }
    }
}
