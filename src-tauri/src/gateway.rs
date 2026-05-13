use std::future::Future;
use std::net::TcpListener as StdTcpListener;
use std::sync::Arc;

#[cfg(test)]
use std::net::SocketAddr;

use axum::body::Body;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::net::TcpListener;

use crate::gateway_adapter::{
    anthropic_error_payload, build_messages_upstream_request, forward_upstream_request,
    forward_upstream_stream, request_wants_stream, GatewayProviderResponse, GatewayProviderStream,
};
use crate::model_catalog::{
    DesktopModel, ModelCatalog, RequestOptions, RouteError, RouteResolution,
};
use crate::provider::Provider;

pub const DEFAULT_GATEWAY_PORT: u16 = 18080;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GatewayHealth {
    pub mode: GatewayMode,
    pub running: bool,
    pub base_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GatewayMode {
    LocalGateway,
}

pub fn planned_gateway_health() -> GatewayHealth {
    planned_gateway_health_for_port(DEFAULT_GATEWAY_PORT)
}

pub fn planned_gateway_health_for_port(port: u16) -> GatewayHealth {
    GatewayHealth {
        mode: GatewayMode::LocalGateway,
        running: false,
        base_url: gateway_base_url(port),
    }
}

pub fn gateway_base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub fn resolve_gateway_route(
    catalog: &ModelCatalog,
    route_id: &str,
) -> Result<RouteResolution, RouteError> {
    catalog.resolve_route(route_id)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GatewayModelsResponse {
    pub data: Vec<DesktopModel>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GatewayMessagesRequest {
    pub model: String,
    #[serde(default)]
    pub use_max: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GatewayUpstreamRequest {
    pub route_id: String,
    pub provider_id: String,
    pub upstream_model: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GatewayError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct GatewayErrorEnvelope {
    #[serde(rename = "type")]
    pub envelope_type: String,
    pub error: GatewayErrorBody,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GatewayErrorBody {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
    pub status: u16,
}

#[derive(Clone)]
struct GatewayRouterState {
    catalog: Arc<ModelCatalog>,
    provider: Option<Arc<Provider>>,
    client: reqwest::Client,
    recorder: GatewayRecorder,
    auth: Option<GatewayAuth>,
}

#[derive(Clone)]
struct GatewayAuth {
    api_key: Arc<str>,
}

impl GatewayAuth {
    fn new(api_key: String) -> Option<Self> {
        let api_key = api_key.trim().to_owned();
        if api_key.is_empty() {
            return None;
        }
        Some(Self {
            api_key: Arc::from(api_key),
        })
    }

    fn required(api_key: String) -> Self {
        Self::new(api_key).expect("local gateway API key must not be empty")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewayRequestEvent {
    pub endpoint: &'static str,
    pub method: &'static str,
    pub status: u16,
    pub code: String,
    pub route_id: Option<String>,
}

impl GatewayRequestEvent {
    pub fn succeeded(&self) -> bool {
        self.status < 400
    }
}

#[derive(Clone, Default)]
pub struct GatewayRecorder {
    callback: Option<Arc<dyn Fn(GatewayRequestEvent) + Send + Sync>>,
}

impl GatewayRecorder {
    pub fn new(callback: impl Fn(GatewayRequestEvent) + Send + Sync + 'static) -> Self {
        Self {
            callback: Some(Arc::new(callback)),
        }
    }

    pub fn record(&self, event: GatewayRequestEvent) {
        if let Some(callback) = &self.callback {
            callback(event);
        }
    }
}

#[cfg(test)]
fn gateway_router(catalog: ModelCatalog) -> Router {
    gateway_router_with_state(catalog, None)
}

#[cfg(test)]
fn gateway_router_with_provider(catalog: ModelCatalog, provider: Provider) -> Router {
    gateway_router_with_state(catalog, Some(provider))
}

#[cfg(test)]
#[allow(dead_code)]
fn gateway_router_with_provider_and_recorder(
    catalog: ModelCatalog,
    provider: Provider,
    recorder: GatewayRecorder,
) -> Router {
    gateway_router_with_state_and_recorder(catalog, Some(provider), None, recorder)
}

pub fn gateway_router_with_auth(catalog: ModelCatalog, gateway_api_key: String) -> Router {
    gateway_router_with_state_and_recorder(
        catalog,
        None,
        Some(GatewayAuth::required(gateway_api_key)),
        GatewayRecorder::default(),
    )
}

pub fn gateway_router_with_provider_auth_and_recorder(
    catalog: ModelCatalog,
    provider: Provider,
    gateway_api_key: String,
    recorder: GatewayRecorder,
) -> Router {
    gateway_router_with_state_and_recorder(
        catalog,
        Some(provider),
        Some(GatewayAuth::required(gateway_api_key)),
        recorder,
    )
}

#[cfg(test)]
fn gateway_router_with_state(catalog: ModelCatalog, provider: Option<Provider>) -> Router {
    gateway_router_with_state_and_recorder(catalog, provider, None, GatewayRecorder::default())
}

fn gateway_router_with_state_and_recorder(
    catalog: ModelCatalog,
    provider: Option<Provider>,
    auth: Option<GatewayAuth>,
    recorder: GatewayRecorder,
) -> Router {
    Router::new()
        .route("/v1/models", get(handle_models))
        .route("/v1/messages", post(handle_messages))
        .with_state(GatewayRouterState {
            catalog: Arc::new(catalog),
            provider: provider.map(Arc::new),
            client: reqwest::Client::new(),
            recorder,
            auth,
        })
}

#[cfg(test)]
#[allow(dead_code)]
async fn serve_gateway(
    addr: SocketAddr,
    catalog: ModelCatalog,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), std::io::Error> {
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, gateway_router(catalog))
        .with_graceful_shutdown(shutdown)
        .await
}

#[cfg(test)]
#[allow(dead_code)]
async fn serve_gateway_with_provider(
    addr: SocketAddr,
    catalog: ModelCatalog,
    provider: Provider,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), std::io::Error> {
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, gateway_router_with_provider(catalog, provider))
        .with_graceful_shutdown(shutdown)
        .await
}

pub async fn serve_gateway_router(
    listener: StdTcpListener,
    router: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), std::io::Error> {
    listener.set_nonblocking(true)?;
    let listener = TcpListener::from_std(listener)?;
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
}

pub fn gateway_models_response(catalog: &ModelCatalog) -> GatewayModelsResponse {
    GatewayModelsResponse {
        data: catalog.desktop_models(),
    }
}

pub fn map_messages_request(
    catalog: &ModelCatalog,
    request: GatewayMessagesRequest,
) -> Result<GatewayUpstreamRequest, GatewayError> {
    let resolution = resolve_messages_request(catalog, request)?;

    Ok(GatewayUpstreamRequest {
        route_id: resolution.route_id,
        provider_id: resolution.provider_id,
        upstream_model: resolution.upstream_model,
    })
}

pub fn resolve_messages_request(
    catalog: &ModelCatalog,
    request: GatewayMessagesRequest,
) -> Result<RouteResolution, GatewayError> {
    catalog
        .validate_request_options(
            &request.model,
            RequestOptions {
                use_max: request.use_max,
            },
        )
        .map_err(GatewayError::from)
}

impl From<RouteError> for GatewayError {
    fn from(error: RouteError) -> Self {
        let message = if error.code == "gateway.unmapped_model_route" {
            "model route is not mapped; Default is not a fallback".to_owned()
        } else {
            error.message
        };
        Self {
            status: 400,
            code: error.code,
            message,
        }
    }
}

async fn handle_models(State(state): State<GatewayRouterState>, headers: HeaderMap) -> Response {
    if let Err(error) = authorize_gateway_request(&state, &headers) {
        return gateway_auth_error_response_with_record(&state, "/v1/models", "GET", error);
    }
    state.recorder.record(GatewayRequestEvent {
        endpoint: "/v1/models",
        method: "GET",
        status: 200,
        code: "gateway.models".to_owned(),
        route_id: None,
    });
    Json(gateway_models_response(&state.catalog)).into_response()
}

async fn handle_messages(
    State(state): State<GatewayRouterState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(error) = authorize_gateway_request(&state, &headers) {
        return gateway_auth_error_response_with_record(&state, "/v1/messages", "POST", error);
    }
    let body: Value = match serde_json::from_slice(&body) {
        Ok(body) => body,
        Err(error) => {
            return gateway_error_response_with_record(
                &state,
                GatewayError {
                    status: 400,
                    code: "provider.api_format_mismatch".to_owned(),
                    message: format!("messages request body must be valid JSON: {error}"),
                },
                None,
            )
        }
    };
    let request = match messages_request_from_body(&body) {
        Ok(request) => request,
        Err(error) => return gateway_error_response_with_record(&state, error, None),
    };
    let resolution = match resolve_messages_request(&state.catalog, request) {
        Ok(resolution) => resolution,
        Err(error) => return gateway_error_response_with_record(&state, error, None),
    };

    if let Some(provider) = state.provider.as_deref() {
        let upstream_request = match build_messages_upstream_request(provider, &resolution, body) {
            Ok(request) => request,
            Err(error) => {
                return gateway_upstream_error_response_with_record(
                    &state,
                    error,
                    Some(resolution.route_id.clone()),
                )
            }
        };
        if request_wants_stream(&upstream_request) {
            return match forward_upstream_stream(&state.client, upstream_request, &resolution).await
            {
                Ok(response) => {
                    gateway_stream_response_with_record(&state, response, Some(resolution.route_id))
                }
                Err(error) => gateway_upstream_error_response_with_record(
                    &state,
                    error,
                    Some(resolution.route_id),
                ),
            };
        }
        return match forward_upstream_request(&state.client, upstream_request, &resolution).await {
            Ok(response) => {
                gateway_provider_response_with_record(&state, response, Some(resolution.route_id))
            }
            Err(error) => gateway_upstream_error_response_with_record(
                &state,
                error,
                Some(resolution.route_id),
            ),
        };
    }

    gateway_error_response_with_record(
        &state,
        GatewayError {
            status: 501,
            code: "gateway.upstream_not_implemented".to_owned(),
            message: "gateway route is valid, but upstream forwarding is not implemented yet"
                .to_owned(),
        },
        Some(resolution.route_id),
    )
}

fn authorize_gateway_request(
    state: &GatewayRouterState,
    headers: &HeaderMap,
) -> Result<(), GatewayError> {
    let Some(auth) = &state.auth else {
        return Ok(());
    };
    let provided = gateway_auth_token(headers);
    match provided {
        None => Err(GatewayError {
            status: 401,
            code: "gateway.auth_missing".to_owned(),
            message: "local gateway API key is required".to_owned(),
        }),
        Some(token) if constant_time_eq(token.as_bytes(), auth.api_key.as_bytes()) => Ok(()),
        Some(_) => Err(GatewayError {
            status: 401,
            code: "gateway.auth_invalid".to_owned(),
            message: "local gateway API key is invalid".to_owned(),
        }),
    }
}

fn gateway_auth_token(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        let value = value.to_str().ok()?.trim();
        let (scheme, token) = value.split_once(' ')?;
        if scheme.eq_ignore_ascii_case("bearer") {
            let token = token.trim();
            if !token.is_empty() {
                return Some(token.to_owned());
            }
        }
        return Some(String::new());
    }

    headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right.iter())
        .fold(0_u8, |acc, (left, right)| acc | (left ^ right))
        == 0
}

fn gateway_error_response(error: GatewayError) -> Response {
    let status = StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let payload = GatewayErrorEnvelope {
        envelope_type: "error".to_owned(),
        error: GatewayErrorBody {
            error_type: error.code,
            message: error.message,
            status: error.status,
        },
    };
    (status, Json(payload)).into_response()
}

fn gateway_provider_response(response: GatewayProviderResponse) -> Response {
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK);
    (status, Json(response.body)).into_response()
}

fn gateway_provider_response_with_record(
    state: &GatewayRouterState,
    response: GatewayProviderResponse,
    route_id: Option<String>,
) -> Response {
    state.recorder.record(GatewayRequestEvent {
        endpoint: "/v1/messages",
        method: "POST",
        status: response.status,
        code: "gateway.messages".to_owned(),
        route_id,
    });
    gateway_provider_response(response)
}

fn gateway_stream_response(response: GatewayProviderStream) -> Response {
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK);
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, response.content_type)
        .body(Body::from_stream(response.body))
        .expect("stream response builder should be valid")
}

fn gateway_upstream_error_response(error: crate::gateway_adapter::UpstreamError) -> Response {
    let status = if (400..600).contains(&error.status) {
        StatusCode::from_u16(error.status).unwrap_or(StatusCode::BAD_GATEWAY)
    } else {
        StatusCode::BAD_GATEWAY
    };
    (status, Json(anthropic_error_payload(&error))).into_response()
}

fn gateway_stream_response_with_record(
    state: &GatewayRouterState,
    response: GatewayProviderStream,
    route_id: Option<String>,
) -> Response {
    state.recorder.record(GatewayRequestEvent {
        endpoint: "/v1/messages",
        method: "POST",
        status: response.status,
        code: "gateway.messages.stream".to_owned(),
        route_id,
    });
    gateway_stream_response(response)
}

fn gateway_error_response_with_record(
    state: &GatewayRouterState,
    error: GatewayError,
    route_id: Option<String>,
) -> Response {
    state.recorder.record(GatewayRequestEvent {
        endpoint: "/v1/messages",
        method: "POST",
        status: error.status,
        code: error.code.clone(),
        route_id,
    });
    gateway_error_response(error)
}

fn gateway_auth_error_response_with_record(
    state: &GatewayRouterState,
    endpoint: &'static str,
    method: &'static str,
    error: GatewayError,
) -> Response {
    state.recorder.record(GatewayRequestEvent {
        endpoint,
        method,
        status: error.status,
        code: error.code.clone(),
        route_id: None,
    });
    gateway_error_response(error)
}

fn gateway_upstream_error_response_with_record(
    state: &GatewayRouterState,
    error: crate::gateway_adapter::UpstreamError,
    route_id: Option<String>,
) -> Response {
    let status = if (400..600).contains(&error.status) {
        error.status
    } else {
        502
    };
    state.recorder.record(GatewayRequestEvent {
        endpoint: "/v1/messages",
        method: "POST",
        status,
        code: error.code.clone(),
        route_id,
    });
    gateway_upstream_error_response(error)
}

fn messages_request_from_body(body: &Value) -> Result<GatewayMessagesRequest, GatewayError> {
    let Some(object) = body.as_object() else {
        return Err(GatewayError {
            status: 400,
            code: "provider.api_format_mismatch".to_owned(),
            message: "messages request body must be a JSON object".to_owned(),
        });
    };
    let model = object
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.trim().is_empty())
        .ok_or_else(|| GatewayError {
            status: 400,
            code: "gateway.unmapped_model_route".to_owned(),
            message: "messages request must include a mapped model route".to_owned(),
        })?;
    let use_max = object
        .get("useMax")
        .or_else(|| object.get("use_max"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    Ok(GatewayMessagesRequest {
        model: model.to_owned(),
        use_max,
    })
}

#[cfg(test)]
mod tests {
    use axum::body::{to_bytes, Body};
    use axum::extract::State as AxumState;
    use axum::http::{Request, StatusCode};
    use axum::routing::post as axum_post;
    use axum::Router as AxumRouter;
    use serde_json::Value;
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tower::ServiceExt;

    use crate::{
        model_catalog::ModelCatalog,
        provider::{ApiFormat, AuthScheme, Provider, ProviderDraft},
    };

    use super::*;

    fn provider() -> Provider {
        ProviderDraft {
            provider_id: None,
            display_name: "DeepSeek".to_owned(),
            base_url: "https://api.deepseek.com/anthropic".to_owned(),
            auth_scheme: AuthScheme::Bearer,
            api_key: "sk-test".to_owned(),
            api_format: ApiFormat::Anthropic,
        }
        .into_provider()
        .unwrap()
    }

    fn catalog() -> ModelCatalog {
        ModelCatalog::for_provider(&provider())
    }

    fn openai_provider(base_url: String) -> Provider {
        ProviderDraft {
            provider_id: Some("provider-deepseek".to_owned()),
            display_name: "DeepSeek".to_owned(),
            base_url,
            auth_scheme: AuthScheme::Bearer,
            api_key: "sk-test".to_owned(),
            api_format: ApiFormat::OpenAiChat,
        }
        .into_provider()
        .unwrap()
    }

    async fn spawn_json_upstream(path: &'static str, response: Value) -> String {
        async fn handler(
            AxumState(response): AxumState<Arc<Value>>,
            Json(_body): Json<Value>,
        ) -> Json<Value> {
            Json((*response).clone())
        }

        let app = AxumRouter::new()
            .route(path, axum_post(handler))
            .with_state(Arc::new(response));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    #[test]
    fn models_response_uses_desktop_safe_catalog_only() {
        let response = gateway_models_response(&catalog());
        let response_json = serde_json::to_string(&response).unwrap();

        assert_eq!(response.data.len(), 2);
        assert_eq!(response.data[0].id, "claude-deepseek-v4-pro");
        assert!(!response_json.contains("\"id\":\"deepseek-v4-pro\""));
        assert!(!response_json.contains("\"displayName\":\"deepseek-v4-pro\""));
        assert!(!response_json.contains("upstreamModel"));
        assert!(!response_json.contains("Default"));
    }

    #[test]
    fn messages_request_maps_safe_route_to_upstream_model() {
        let upstream = map_messages_request(
            &catalog(),
            GatewayMessagesRequest {
                model: "claude-deepseek-reasoner".to_owned(),
                use_max: true,
            },
        )
        .unwrap();

        assert_eq!(upstream.route_id, "claude-deepseek-reasoner");
        assert_eq!(upstream.provider_id, "provider-deepseek");
        assert_eq!(upstream.upstream_model, "deepseek-reasoner");
    }

    #[test]
    fn messages_request_rejects_unmapped_route_with_400() {
        let error = map_messages_request(
            &catalog(),
            GatewayMessagesRequest {
                model: "claude-missing-route".to_owned(),
                use_max: false,
            },
        )
        .unwrap_err();

        assert_eq!(error.status, 400);
        assert_eq!(error.code, "gateway.unmapped_model_route");
        assert!(error.message.contains("Default is not a fallback"));
    }

    #[test]
    fn messages_request_rejects_unsupported_max_with_400() {
        let error = map_messages_request(
            &catalog(),
            GatewayMessagesRequest {
                model: "claude-deepseek-v4-pro".to_owned(),
                use_max: true,
            },
        )
        .unwrap_err();

        assert_eq!(error.status, 400);
        assert_eq!(error.code, "provider.max_not_supported");
    }

    #[tokio::test]
    async fn router_models_endpoint_exposes_safe_routes_only() {
        let response = gateway_router(catalog())
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();

        assert_eq!(payload["data"][0]["id"], "claude-deepseek-v4-pro");
        assert!(!body.contains("\"id\":\"deepseek-v4-pro\""));
        assert!(!body.contains("upstreamModel"));
    }

    #[tokio::test]
    async fn router_models_endpoint_requires_gateway_auth_when_configured() {
        let response = gateway_router_with_auth(catalog(), "gateway-secret".to_owned())
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["type"], "error");
        assert_eq!(payload["error"]["type"], "gateway.auth_missing");
        assert!(!String::from_utf8(body.to_vec())
            .unwrap()
            .contains("gateway-secret"));
    }

    #[tokio::test]
    async fn router_models_endpoint_accepts_bearer_gateway_auth() {
        let response = gateway_router_with_auth(catalog(), "gateway-secret".to_owned())
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .header("authorization", "Bearer gateway-secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["data"][0]["id"], "claude-deepseek-v4-pro");
    }

    #[tokio::test]
    async fn router_messages_endpoint_rejects_unmapped_route() {
        let response = gateway_router(catalog())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/messages")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"claude-missing-route","useMax":false}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["type"], "error");
        assert_eq!(payload["error"]["type"], "gateway.unmapped_model_route");
    }

    #[tokio::test]
    async fn router_messages_endpoint_does_not_echo_raw_unmapped_model() {
        let captured = Arc::new(std::sync::Mutex::new(Vec::<GatewayRequestEvent>::new()));
        let captured_for_recorder = Arc::clone(&captured);
        let recorder = GatewayRecorder::new(move |event| {
            captured_for_recorder.lock().unwrap().push(event);
        });
        let response = gateway_router_with_state_and_recorder(catalog(), None, None, recorder)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/messages")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"claude-raw-upstream-model","useMax":false}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("gateway.unmapped_model_route"));
        assert!(!body.contains("claude-raw-upstream-model"));
        let events = captured.lock().unwrap();
        assert_eq!(events[0].route_id, None);
    }

    #[tokio::test]
    async fn router_messages_endpoint_rejects_invalid_gateway_auth() {
        let captured = Arc::new(std::sync::Mutex::new(Vec::<GatewayRequestEvent>::new()));
        let captured_for_recorder = Arc::clone(&captured);
        let recorder = GatewayRecorder::new(move |event| {
            captured_for_recorder.lock().unwrap().push(event);
        });
        let response = gateway_router_with_state_and_recorder(
            catalog(),
            None,
            GatewayAuth::new("gateway-secret".to_owned()),
            recorder,
        )
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/messages")
                .header("content-type", "application/json")
                .header("authorization", "Bearer wrong-secret")
                .body(Body::from(
                    r#"{"model":"claude-deepseek-v4-pro","useMax":false}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        let payload: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(payload["type"], "error");
        assert_eq!(payload["error"]["type"], "gateway.auth_invalid");
        assert!(!body.contains("wrong-secret"));
        assert!(!body.contains("gateway-secret"));
        let events = captured.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].code, "gateway.auth_invalid");
        assert_eq!(events[0].status, 401);
        assert_eq!(events[0].route_id, None);
    }

    #[tokio::test]
    async fn router_messages_endpoint_accepts_x_api_key_gateway_auth() {
        let response = gateway_router_with_auth(catalog(), "gateway-secret".to_owned())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/messages")
                    .header("content-type", "application/json")
                    .header("x-api-key", "gateway-secret")
                    .body(Body::from(
                        r#"{"model":"claude-missing-route","useMax":false}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["type"], "error");
        assert_eq!(payload["error"]["type"], "gateway.unmapped_model_route");
    }

    #[tokio::test]
    async fn router_messages_endpoint_does_not_echo_upstream_model() {
        let response = gateway_router(catalog())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/messages")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"claude-deepseek-reasoner","useMax":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("gateway.upstream_not_implemented"));
        assert!(!body.contains("deepseek-reasoner"));
        assert!(!body.contains("upstreamModel"));
    }

    #[tokio::test]
    async fn router_messages_endpoint_accepts_full_body_for_future_adapter() {
        let base_url = spawn_json_upstream(
            "/v1/chat/completions",
            serde_json::json!({
                "id": "chatcmpl-test",
                "model": "deepseek-v4-pro",
                "choices": [{
                    "message": {"role": "assistant", "content": "ok"},
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 3, "completion_tokens": 1}
            }),
        )
        .await;
        let response = gateway_router_with_provider(catalog(), openai_provider(base_url))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/messages")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"model":"claude-deepseek-v4-pro","messages":[{"role":"user","content":[{"type":"text","text":"hello"}]}],"max_tokens":32}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        let payload: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(payload["model"], "claude-deepseek-v4-pro");
        assert_eq!(payload["content"][0]["text"], "ok");
        assert!(!body.contains(r#""model":"deepseek-v4-pro""#));
        assert!(!body.contains("upstreamModel"));
    }

    #[tokio::test]
    async fn router_messages_endpoint_streams_event_stream_response() {
        async fn handler() -> (StatusCode, [(&'static str, &'static str); 1], &'static str) {
            (
                StatusCode::OK,
                [("content-type", "text/event-stream")],
                "data: {\"id\":\"chatcmpl-test\",\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
            )
        }

        let app = AxumRouter::new().route("/v1/chat/completions", axum_post(handler));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let response = gateway_router_with_provider(
            catalog(),
            openai_provider(format!("http://{addr}")),
        )
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/messages")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"model":"claude-deepseek-v4-pro","messages":[{"role":"user","content":"hello"}],"max_tokens":32,"stream":true}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/event-stream"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("event: message_start"));
        assert!(body.contains("event: content_block_delta"));
        assert!(body.contains(r#""model":"claude-deepseek-v4-pro""#));
        assert!(!body.contains(r#""model":"deepseek-v4-pro""#));
    }

    #[tokio::test]
    async fn router_messages_endpoint_rejects_stream_content_type_mismatch() {
        async fn handler() -> (StatusCode, [(&'static str, &'static str); 1], &'static str) {
            (
                StatusCode::OK,
                [("content-type", "application/json")],
                r#"{"error":"not stream","apiKey":"sk-stream-secret"}"#,
            )
        }

        let app = AxumRouter::new().route("/v1/chat/completions", axum_post(handler));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let response = gateway_router_with_provider(
            catalog(),
            openai_provider(format!("http://{addr}")),
        )
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/messages")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"model":"claude-deepseek-v4-pro","messages":[{"role":"user","content":"hello"}],"max_tokens":32,"stream":true}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("gateway.invalid_stream_content_type"));
        assert!(!body.contains("sk-stream-secret"));
        assert!(body.contains("[REDACTED:key]"));
    }
}
