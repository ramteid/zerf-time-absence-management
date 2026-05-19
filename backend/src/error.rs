use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Not authenticated")]
    Unauthorized,
    #[error("Forbidden")]
    Forbidden,
    #[error("Not found")]
    NotFound,
    #[error("{0}")]
    BadRequest(String),
    #[error("Conflict: {0}")]
    Conflict(String),
    #[error("{0}")]
    Internal(String),
}

impl From<crate::db::SqlxError> for AppError {
    fn from(e: crate::db::SqlxError) -> Self {
        match e {
            crate::db::SqlxError::RowNotFound => AppError::NotFound,
            // Stringify privately for the log; do NOT surface SQL details to the client.
            other => {
                tracing::error!(target: "zerf::db", "sqlx error: {other}");
                AppError::Internal("database error".into())
            }
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!(target: "zerf::any", "anyhow error: {e:#}");
        AppError::Internal("internal error".into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::Forbidden => (StatusCode::FORBIDDEN, self.to_string()),
            AppError::NotFound => (StatusCode::NOT_FOUND, self.to_string()),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::Conflict(_) => (StatusCode::CONFLICT, self.to_string()),
            AppError::Internal(_) => {
                // Already logged where it was created; hide details from the client.
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn response_error_message(response: Response) -> String {
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should be readable");
        let value: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be valid json");
        value
            .get("error")
            .and_then(serde_json::Value::as_str)
            .expect("error field should be present")
            .to_string()
    }

    #[tokio::test]
    async fn maps_public_error_variants_to_expected_status_and_message() {
        let cases = vec![
            (AppError::Unauthorized, StatusCode::UNAUTHORIZED, "Not authenticated"),
            (AppError::Forbidden, StatusCode::FORBIDDEN, "Forbidden"),
            (AppError::NotFound, StatusCode::NOT_FOUND, "Not found"),
            (
                AppError::BadRequest("invalid input".into()),
                StatusCode::BAD_REQUEST,
                "invalid input",
            ),
            (
                AppError::Conflict("already exists".into()),
                StatusCode::CONFLICT,
                "Conflict: already exists",
            ),
        ];

        for (error, expected_status, expected_message) in cases {
            let response = error.into_response();
            assert_eq!(response.status(), expected_status);
            assert_eq!(response_error_message(response).await, expected_message);
        }
    }

    #[tokio::test]
    async fn internal_errors_hide_private_details() {
        let response = AppError::Internal("db: relation users missing".into()).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            response_error_message(response).await,
            "Internal server error"
        );
    }
}
