use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chat2db_contract::ApiError;
use chat2db_core::{AppError, AppErrorKind};

#[derive(Debug)]
pub(crate) enum WebError {
    Application(Box<AppError>),
    Request {
        status: StatusCode,
        error: Box<ApiError>,
    },
}

impl WebError {
    pub(crate) fn bad_request(code: &'static str, message: &'static str) -> Self {
        Self::Request {
            status: StatusCode::BAD_REQUEST,
            error: Box::new(ApiError::new(code, message)),
        }
    }

    pub(crate) fn into_parts(self) -> (StatusCode, ApiError) {
        match self {
            Self::Application(error) => {
                let status = status_for_kind(error.kind());
                (status, error.api_error())
            }
            Self::Request { status, error } => (status, *error),
        }
    }
}

impl From<AppError> for WebError {
    fn from(error: AppError) -> Self {
        Self::Application(Box::new(error))
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        let (status, error) = self.into_parts();
        (status, Json(error)).into_response()
    }
}

pub(crate) fn response(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

fn status_for_kind(kind: AppErrorKind) -> StatusCode {
    match kind {
        AppErrorKind::InvalidRequest => StatusCode::BAD_REQUEST,
        AppErrorKind::NotFound => StatusCode::NOT_FOUND,
        AppErrorKind::Conflict => StatusCode::CONFLICT,
        AppErrorKind::ResourceExhausted => StatusCode::INSUFFICIENT_STORAGE,
        AppErrorKind::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        AppErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
