//! Axum transport adapter for `Chat2DB` product services.

use std::{
    error::Error,
    fmt::{Display, Formatter},
    net::SocketAddr,
    sync::Arc,
};

use axum::{
    Json, Router,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use chat2db_contract::{ApiError, ProductInfo, RuntimeStatus};
use chat2db_core::Application;
use subtle::ConstantTimeEq;
use tower_http::trace::TraceLayer;

const MINIMUM_ACCESS_TOKEN_BYTES: usize = 32;

/// Validation failure for a non-loopback Web listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessPolicyError {
    /// A non-loopback listener has no bearer token.
    MissingToken,
    /// The configured token is too short for the external boundary.
    WeakToken,
}

impl Display for AccessPolicyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingToken => formatter
                .write_str("CHAT2DB_ACCESS_TOKEN is required when CHAT2DB_BIND is not loopback"),
            Self::WeakToken => formatter.write_str(
                "CHAT2DB_ACCESS_TOKEN must contain at least 32 bytes when Web is not loopback",
            ),
        }
    }
}

impl Error for AccessPolicyError {}

/// Authentication policy applied before every Web route.
#[derive(Clone, Default)]
pub struct AccessPolicy {
    bearer_token: Option<Arc<[u8]>>,
}

impl std::fmt::Debug for AccessPolicy {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccessPolicy")
            .field("bearer_token_configured", &self.bearer_token.is_some())
            .finish()
    }
}

impl AccessPolicy {
    /// Builds a fail-closed policy for the selected listener.
    ///
    /// # Errors
    ///
    /// Returns [`AccessPolicyError::MissingToken`] or
    /// [`AccessPolicyError::WeakToken`] when a non-loopback listener does not
    /// have a sufficiently strong bearer token.
    pub fn for_bind(
        address: SocketAddr,
        configured_token: Option<String>,
    ) -> Result<Self, AccessPolicyError> {
        if address.ip().is_loopback() {
            return Ok(Self::default());
        }

        let token = configured_token
            .filter(|candidate| !candidate.is_empty())
            .ok_or(AccessPolicyError::MissingToken)?;
        if token.len() < MINIMUM_ACCESS_TOKEN_BYTES {
            return Err(AccessPolicyError::WeakToken);
        }

        Ok(Self {
            bearer_token: Some(Arc::from(token.into_bytes())),
        })
    }
}

/// Builds the loopback Web router around a shared application service root.
pub fn router(application: Application) -> Router {
    router_with_policy(application, AccessPolicy::default())
}

/// Builds the complete Web router with its listener-derived access policy.
pub fn router_with_policy(application: Application, access_policy: AccessPolicy) -> Router {
    Router::new()
        .route("/api/v1/system/health", get(health))
        .route("/api/v1/system/info", get(info))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(access_policy, authorize))
        .with_state(application)
}

async fn health(State(application): State<Application>) -> Response {
    let health = application.health();
    let status = if health.status == RuntimeStatus::Unavailable {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    (status, Json(health)).into_response()
}

async fn info(State(application): State<Application>) -> Json<ProductInfo> {
    Json(application.health().product)
}

async fn authorize(State(policy): State<AccessPolicy>, request: Request, next: Next) -> Response {
    let Some(expected_token) = policy.bearer_token.as_deref() else {
        return next.run(request).await;
    };

    let provided_token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::as_bytes);

    if provided_token.is_some_and(|token| bool::from(expected_token.ct_eq(token))) {
        return next.run(request).await;
    }

    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer")],
        Json(ApiError::new(
            "unauthorized",
            "A valid Chat2DB access token is required",
        )),
    )
        .into_response()
}

async fn not_found() -> Response {
    api_error(
        StatusCode::NOT_FOUND,
        "route_not_found",
        "The requested route does not exist",
    )
}

async fn method_not_allowed() -> Response {
    api_error(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "The requested method is not allowed for this route",
    )
}

fn api_error(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use axum::{
        body::Body,
        http::{Method, Request, StatusCode, header},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::{AccessPolicy, AccessPolicyError, router, router_with_policy};
    use chat2db_contract::{ApiError, HealthResponse, RuntimeStatus};
    use chat2db_core::Application;

    #[tokio::test]
    async fn health_route_uses_the_shared_contract() {
        let response = router(Application::new())
            .oneshot(request(Method::GET, "/api/v1/system/health"))
            .await
            .expect("router must respond");

        assert_eq!(response.status(), StatusCode::OK);
        let health: HealthResponse = response_json(response).await;
        assert_eq!(health.product.name, "Chat2DB Rust");
    }

    #[tokio::test]
    async fn unavailable_health_maps_to_service_unavailable() {
        let response = router(Application::with_runtime_status(RuntimeStatus::Unavailable))
            .oneshot(request(Method::GET, "/api/v1/system/health"))
            .await
            .expect("router must respond");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let health: HealthResponse = response_json(response).await;
        assert_eq!(health.status, RuntimeStatus::Unavailable);
    }

    #[tokio::test]
    async fn unknown_routes_use_the_error_contract() {
        let response = router(Application::new())
            .oneshot(request(Method::GET, "/api/v1/missing"))
            .await
            .expect("router must respond");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let error: ApiError = response_json(response).await;
        assert_eq!(error.code, "route_not_found");
    }

    #[tokio::test]
    async fn unsupported_methods_use_the_error_contract() {
        let response = router(Application::new())
            .oneshot(request(Method::POST, "/api/v1/system/health"))
            .await
            .expect("router must respond");

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        let error: ApiError = response_json(response).await;
        assert_eq!(error.code, "method_not_allowed");
    }

    #[test]
    fn non_loopback_policy_fails_without_a_strong_token() {
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 4200);

        assert_eq!(
            AccessPolicy::for_bind(address, None).expect_err("token must be required"),
            AccessPolicyError::MissingToken
        );
        assert_eq!(
            AccessPolicy::for_bind(address, Some("short".to_owned()))
                .expect_err("short token must fail"),
            AccessPolicyError::WeakToken
        );
    }

    #[tokio::test]
    async fn non_loopback_policy_requires_the_configured_bearer_token() {
        let token = "0123456789abcdef0123456789abcdef";
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 4200);
        let policy = AccessPolicy::for_bind(address, Some(token.to_owned()))
            .expect("strong token must build policy");
        let application = router_with_policy(Application::new(), policy);

        let unauthorized = application
            .clone()
            .oneshot(request(Method::GET, "/api/v1/system/health"))
            .await
            .expect("router must respond");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(unauthorized.headers()[header::WWW_AUTHENTICATE], "Bearer");

        let authorized = application
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/system/health")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request must build"),
            )
            .await
            .expect("router must respond");
        assert_eq!(authorized.status(), StatusCode::OK);
    }

    fn request(method: Method, uri: &'static str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("request must build")
    }

    async fn response_json<T>(response: axum::response::Response) -> T
    where
        T: serde::de::DeserializeOwned,
    {
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body must collect")
            .to_bytes();
        serde_json::from_slice(&body).expect("response body must match contract")
    }
}
