use axum::{
    extract::{Json, State},
    http::{HeaderMap, Method, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex, RwLock};
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};

pub const VERSION: &str = "0.3.0";
pub const MAX_EVENTS: usize = 50;

// ──────────────────────────────────────────────
// ConnectorRunMode
// ──────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorRunMode {
    Simulation,
    Diagnostic,
    Live,
}

impl Default for ConnectorRunMode {
    fn default() -> Self {
        ConnectorRunMode::Simulation
    }
}

impl ConnectorRunMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConnectorRunMode::Simulation => "simulation",
            ConnectorRunMode::Diagnostic => "diagnostic",
            ConnectorRunMode::Live       => "live",
        }
    }
}

// ──────────────────────────────────────────────
// Config  (all original fields preserved + connector_run_mode added)
// ──────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorConfig {
    #[serde(default = "default_printer_type")]
    pub printer_type: String,
    #[serde(default)]
    pub printer_ip: String,
    #[serde(default = "default_port")]
    pub printer_port: u16,
    #[serde(default = "default_kick_cmd")]
    pub drawer_kick_command: String,
    #[serde(default)]
    pub pairing_token: String,
    #[serde(default = "default_origins")]
    pub allowed_origins: Vec<String>,
    #[serde(default)]
    pub dev_mode: bool,
    #[serde(default = "default_true")]
    pub start_at_login: bool,
    #[serde(default)]
    pub setup_complete: bool,
    #[serde(default)]
    pub hardware_verified: bool,
    #[serde(default)]
    pub last_hardware_verified_at: Option<String>,
    #[serde(default)]
    pub hardware_verification_source: Option<String>,
    #[serde(default)]
    pub connector_run_mode: ConnectorRunMode,
}

fn default_printer_type() -> String { "network_escpos".to_string() }
fn default_port() -> u16 { 9100 }
fn default_kick_cmd() -> String { "1B700019FA".to_string() }
fn default_origins() -> Vec<String> { vec!["https://franchisetech.ro".to_string()] }
fn default_true() -> bool { true }

pub fn default_config() -> ConnectorConfig {
    ConnectorConfig {
        printer_type: default_printer_type(),
        printer_ip: String::new(),
        printer_port: 9100,
        drawer_kick_command: default_kick_cmd(),
        pairing_token: String::new(),
        allowed_origins: default_origins(),
        dev_mode: false,
        start_at_login: true,
        setup_complete: false,
        hardware_verified: false,
        last_hardware_verified_at: None,
        hardware_verification_source: None,
        connector_run_mode: ConnectorRunMode::Simulation,
    }
}

pub fn load_config_from_path(path: &str) -> ConnectorConfig {
    if let Ok(contents) = std::fs::read_to_string(path) {
        if let Ok(cfg) = serde_json::from_str::<ConnectorConfig>(&contents) {
            return cfg;
        }
    }
    default_config()
}

pub fn save_config_to_path(path: &std::path::PathBuf, config: &ConnectorConfig) -> Result<(), String> {
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

pub fn is_configured(config: &ConnectorConfig) -> bool {
    !config.printer_ip.is_empty()
}

// ──────────────────────────────────────────────
// Printer discovery  (no-op on non-Windows)
// ──────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PrinterCandidate {
    pub name: String,
    pub ip: String,
    pub port: u16,
    pub status: String,
}

pub async fn discover_network_printers() -> Vec<PrinterCandidate> {
    // Lightweight: scan common printer subnet entries
    vec![]
}

pub async fn test_network_connection(ip: &str, port: u16) -> Result<(), String> {
    let addr = format!("{}:{}", ip, port);
    tokio::time::timeout(
        Duration::from_millis(1500),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    .map_err(|_| "timeout".to_string())?
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn send_network_escpos(ip: &str, port: u16) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let addr = format!("{}:{}", ip, port);
    let mut stream = tokio::time::timeout(
        Duration::from_millis(1500),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    .map_err(|_| "timeout".to_string())?
    .map_err(|e| e.to_string())?;
    stream
        .write_all(&[0x1B, 0x70, 0x00, 0x19, 0xFA])
        .await
        .map_err(|e| e.to_string())
}

// ──────────────────────────────────────────────
// Event log
// ──────────────────────────────────────────────

#[derive(Clone, Serialize)]
pub struct ConnectorEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub severity: &'static str,
    pub message: String,
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "createdAt")]
    pub created_at: u64,
    pub details: Value,
}

// ──────────────────────────────────────────────
// AppState
// ──────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<ConnectorConfig>>,
    last_open: Arc<Mutex<Option<Instant>>>,
    pending_pairing: Arc<Mutex<Option<PendingPairing>>>,
    pub event_log: Arc<Mutex<VecDeque<ConnectorEvent>>>,
}

pub fn new_state(config: ConnectorConfig) -> AppState {
    AppState {
        config: Arc::new(RwLock::new(config)),
        last_open: Arc::new(Mutex::new(None)),
        pending_pairing: Arc::new(Mutex::new(None)),
        event_log: Arc::new(Mutex::new(VecDeque::with_capacity(MAX_EVENTS + 1))),
    }
}

// ──────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────

fn generate_request_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let n: u32 = rng.gen_range(0..=0xFFFFFF);
    let t = unix_now_secs() % 0xFFFF;
    format!("req-{:04x}{:06x}", t, n)
}

pub fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

pub async fn log_event(
    log: &Arc<Mutex<VecDeque<ConnectorEvent>>>,
    event_type: &str,
    severity: &'static str,
    message: &str,
    request_id: &str,
    details: Value,
) {
    let event = ConnectorEvent {
        id: generate_request_id(),
        event_type: event_type.to_string(),
        severity,
        message: message.to_string(),
        request_id: request_id.to_string(),
        created_at: unix_now_secs(),
        details,
    };
    let mut guard = log.lock().await;
    guard.push_back(event);
    if guard.len() > MAX_EVENTS {
        guard.pop_front();
    }
}

pub fn generate_pairing_token() -> String {
    use rand::distributions::Alphanumeric;
    use rand::Rng;
    let suffix: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(24)
        .map(char::from)
        .collect();
    format!("FT-{}", suffix)
}

fn allowed_origin(config: &ConnectorConfig) -> &str {
    config.allowed_origins.first().map(String::as_str).unwrap_or("")
}

fn validate_auth(headers: &HeaderMap, config: &ConnectorConfig) -> Result<(), ApiError> {
    let origin_header = headers
        .get("origin")
        .or_else(|| headers.get("referer"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let ao = allowed_origin(config);
    if !ao.is_empty() && !origin_header.starts_with(ao.trim_end_matches('/')) {
        return Err(ApiError::OriginRejected);
    }

    let token = headers
        .get("x-connector-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if config.pairing_token.is_empty() {
        return Err(ApiError::NotPaired);
    }
    if token.is_empty() {
        return Err(ApiError::MissingToken);
    }
    if token != config.pairing_token {
        return Err(ApiError::InvalidToken);
    }

    Ok(())
}

fn check_rate_limit(last: &mut Option<Instant>) -> Result<(), ApiError> {
    let now = Instant::now();
    if let Some(prev) = *last {
        if now.duration_since(prev) < Duration::from_secs(1) {
            return Err(ApiError::RateLimited);
        }
    }
    *last = Some(now);
    Ok(())
}

// ──────────────────────────────────────────────
// Error enum
// ──────────────────────────────────────────────

#[derive(Debug)]
pub enum ApiError {
    OriginRejected,
    MissingToken,
    InvalidToken,
    NotPaired,
    RateLimited,
    PrinterIpMissing,
    HardwareNotVerified,
    SimulationOnly,
    DiagnosticOnly,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, result, error_code, message, suggestion) = match self {
            ApiError::OriginRejected => (
                StatusCode::FORBIDDEN, "origin_rejected", "ORIGIN_REJECTED",
                "Request origin is not in the allowed list.",
                "Check Allowed Origin in the connector settings.",
            ),
            ApiError::MissingToken => (
                StatusCode::UNAUTHORIZED, "missing_token", "MISSING_TOKEN",
                "x-connector-token header is required.",
                "Re-pair this terminal with the connector.",
            ),
            ApiError::InvalidToken => (
                StatusCode::UNAUTHORIZED, "invalid_token", "INVALID_TOKEN",
                "Connector token does not match.",
                "Re-pair this terminal with the connector.",
            ),
            ApiError::NotPaired => (
                StatusCode::UNAUTHORIZED, "not_paired", "NOT_PAIRED",
                "No pairing token is configured.",
                "Complete the connector setup wizard.",
            ),
            ApiError::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS, "rate_limited", "RATE_LIMITED",
                "Too many requests. Wait 1 second.",
                "Do not retry automatically in live mode.",
            ),
            ApiError::PrinterIpMissing => (
                StatusCode::UNPROCESSABLE_ENTITY, "printer_ip_missing", "PRINTER_IP_MISSING",
                "Printer IP address is not configured.",
                "Enter the printer IP in the connector Setup tab.",
            ),
            ApiError::HardwareNotVerified => (
                StatusCode::PRECONDITION_FAILED, "hardware_not_verified", "HARDWARE_NOT_VERIFIED",
                "Hardware has not been verified. Open drawer manually.",
                "Run the hardware verification step in Setup.",
            ),
            ApiError::SimulationOnly => (
                StatusCode::OK, "simulation_only", "SIMULATION_ONLY",
                "Simulation mode is enabled. Open drawer manually.",
                "Switch connector to Live mode to send real commands.",
            ),
            ApiError::DiagnosticOnly => (
                StatusCode::OK, "diagnostic_only", "DIAGNOSTIC_ONLY",
                "Diagnostic mode is active. No bytes sent to printer.",
                "Switch connector to Live mode to send real commands.",
            ),
        };

        let body = json!({
            "ok": false,
            "result": result,
            "errorCode": error_code,
            "message": message,
            "suggestion": suggestion,
            "connectorVersion": VERSION,
        });

        (status, Json(body)).into_response()
    }
}

// ──────────────────────────────────────────────
// Pairing
// ──────────────────────────────────────────────

#[derive(Clone)]
pub struct PendingPairing {
    pub token: String,
    pub terminal_id: String,
    pub location_id: String,
    pub origin: String,
    pub requested_at: SystemTime,
}

#[derive(Clone, Serialize)]
pub struct PendingPairingView {
    pub token: String,
    pub origin: String,
}

#[derive(Deserialize)]
pub struct PairStartRequest {
    #[allow(dead_code)]
    pub terminal_id: Option<String>,
    #[allow(dead_code)]
    pub location_id: Option<String>,
}

// ──────────────────────────────────────────────
// Tauri helpers
// ──────────────────────────────────────────────

pub async fn pending_pairing_request(state: &AppState) -> Option<PendingPairingView> {
    let guard = state.pending_pairing.lock().await;
    guard.as_ref().map(|p| PendingPairingView {
        token: p.token.clone(),
        origin: p.origin.clone(),
    })
}

pub async fn approve_pairing_request(state: &AppState) -> Result<String, String> {
    let mut pending = state.pending_pairing.lock().await;
    if let Some(p) = pending.take() {
        let token = p.token.clone();
        let mut config = state.config.write().await;
        config.pairing_token = token.clone();
        // NOTE: does NOT auto-set setup_complete
        log_event(
            &state.event_log,
            "pair_approved",
            "success",
            "Pairing approved",
            &generate_request_id(),
            json!({ "origin": p.origin }),
        )
        .await;
        return Ok(token);
    }
    Err("No pending pairing request".to_string())
}

pub async fn deny_pairing_request(state: &AppState) -> Result<(), String> {
    let mut pending = state.pending_pairing.lock().await;
    *pending = None;
    log_event(
        &state.event_log,
        "pair_denied",
        "info",
        "Pairing denied",
        &generate_request_id(),
        json!({}),
    )
    .await;
    Ok(())
}

pub async fn finish_setup_state(state: &AppState, hardware_verified: bool) {
    let mut config = state.config.write().await;
    config.setup_complete = true;
    if hardware_verified {
        config.hardware_verified = true;
        config.last_hardware_verified_at = Some(unix_now_secs().to_string());
        config.hardware_verification_source = Some("wizard".to_string());
    }
    log_event(
        &state.event_log,
        "setup_completed",
        "success",
        "Setup wizard completed",
        &generate_request_id(),
        json!({ "hardwareVerified": hardware_verified }),
    )
    .await;
}

// ──────────────────────────────────────────────
// Router
// ──────────────────────────────────────────────

pub fn build_router(state: AppState, allowed_origins: Vec<String>) -> Router {
    let origins: Vec<_> = allowed_origins
        .iter()
        .filter_map(|o| o.parse().ok())
        .collect();

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods(AllowMethods::list([Method::GET, Method::POST, Method::OPTIONS]))
        .allow_headers(AllowHeaders::any());

    Router::new()
        .route("/health", get(health))
        .route("/status", get(status))
        .route("/pair/start", post(pair_start))
        .route("/pair/status", get(pair_status))
        .route("/open-drawer", post(open_drawer))
        .route("/test-drawer", post(open_drawer))
        .route("/simulate", post(simulate))
        .route("/diagnostics/run", post(diagnostics_run))
        .route("/events/recent", get(events_recent))
        .with_state(state)
        .layer(cors)
}

pub async fn try_run_server_with_state(state: AppState) -> Result<(), String> {
    let allowed = {
        let config = state.config.read().await;
        let mut origins = config.allowed_origins.clone();
        if !origins.contains(&"http://localhost:3000".to_string()) {
            origins.push("http://localhost:3000".to_string());
        }
        if !origins.contains(&"http://localhost:3001".to_string()) {
            origins.push("http://localhost:3001".to_string());
        }
        origins
    };
    let app = build_router(state, allowed);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:17878")
        .await
        .map_err(|e| e.to_string())?;
    axum::serve(listener, app).await.map_err(|e| e.to_string())
}

// ──────────────────────────────────────────────
// GET /health
// ──────────────────────────────────────────────

pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let config = state.config.read().await;
    Json(json!({
        "ok": true,
        "name": "franchisetech-connector",
        "version": VERSION,
        "serverTime": unix_now_secs(),
        "mode": config.connector_run_mode.as_str(),
    }))
}

// ──────────────────────────────────────────────
// GET /status
// ──────────────────────────────────────────────

pub async fn status(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let config = state.config.read().await;
    let mode = config.connector_run_mode.as_str();
    let has_printer = !config.printer_ip.is_empty();
    let paired = !config.pairing_token.is_empty();

    let origin = headers
        .get("origin")
        .or_else(|| headers.get("referer"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let ao = allowed_origin(&config);
    let origin_ok = ao.is_empty() || origin.starts_with(ao.trim_end_matches('/'));

    let events = state.event_log.lock().await;
    let last_event = events.back().map(|e| json!({
        "type": e.event_type,
        "severity": e.severity,
        "message": e.message,
        "createdAt": e.created_at,
    }));
    drop(events);

    Json(json!({
        "ok": true,
        "name": "franchisetech-connector",
        "version": VERSION,
        "mode": mode,
        "paired": paired,
        "originAllowed": origin_ok,
        "configured": has_printer,
        "setupComplete": config.setup_complete,
        "hardwareVerified": config.hardware_verified,
        "capabilities": {
            "pairing": true,
            "simulation": true,
            "diagnostics": true,
            "events": true,
            "printerDiscovery": true,
            "liveEscposDrawerKick": true,
        },
        "printer": {
            "configured": has_printer,
            "ip": if has_printer { config.printer_ip.clone() } else { String::new() },
        },
        "hardware": {
            "verified": config.hardware_verified,
            "verifiedAt": config.last_hardware_verified_at,
            "source": config.hardware_verification_source,
        },
        "lastEvent": last_event,
        "connectorVersion": VERSION,
        "serverTime": unix_now_secs(),
    }))
}

// ──────────────────────────────────────────────
// POST /pair/start
// ──────────────────────────────────────────────

pub async fn pair_start(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(_body): Json<PairStartRequest>,
) -> impl IntoResponse {
    let config = state.config.read().await;

    let origin = headers
        .get("origin")
        .or_else(|| headers.get("referer"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let ao = allowed_origin(&config).to_string();
    if !ao.is_empty() && !origin.starts_with(ao.trim_end_matches('/')) {
        return ApiError::OriginRejected.into_response();
    }
    drop(config);

    let token = generate_pairing_token();
    let pending = PendingPairing {
        token: token.clone(),
        terminal_id: String::new(),
        location_id: String::new(),
        origin: origin.clone(),
        requested_at: SystemTime::now(),
    };

    *state.pending_pairing.lock().await = Some(pending);

    log_event(
        &state.event_log,
        "pair_requested",
        "info",
        "Pairing requested",
        &generate_request_id(),
        json!({ "origin": origin }),
    )
    .await;

    Json(json!({
        "ok": true,
        "token": token,
        "message": "Pairing request created. Approve in the connector UI.",
    }))
    .into_response()
}

// ──────────────────────────────────────────────
// GET /pair/status
// ──────────────────────────────────────────────

pub async fn pair_status(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let config = state.config.read().await;

    let origin = headers
        .get("origin")
        .or_else(|| headers.get("referer"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let ao = allowed_origin(&config);
    if !ao.is_empty() && !origin.starts_with(ao.trim_end_matches('/')) {
        return ApiError::OriginRejected.into_response();
    }

    if !config.pairing_token.is_empty() {
        return Json(json!({
            "ok": true,
            "status": "approved",
            "setupComplete": config.setup_complete,
            "hardwareVerified": config.hardware_verified,
            "mode": config.connector_run_mode.as_str(),
            "connectorVersion": VERSION,
        }))
        .into_response();
    }

    let pending = state.pending_pairing.lock().await;
    let waiting = pending.is_some();
    drop(pending);

    Json(json!({
        "ok": true,
        "status": if waiting { "pending" } else { "unpaired" },
        "connectorVersion": VERSION,
    }))
    .into_response()
}

// ──────────────────────────────────────────────
// POST /open-drawer and /test-drawer
// ──────────────────────────────────────────────

#[derive(Deserialize)]
pub struct DrawerRequest {
    pub reason: Option<String>,
}

pub async fn open_drawer(
    headers: HeaderMap,
    State(state): State<AppState>,
    body: Option<Json<DrawerRequest>>,
) -> impl IntoResponse {
    let config = state.config.read().await;

    if let Err(e) = validate_auth(&headers, &config) {
        return e.into_response();
    }

    let mode = config.connector_run_mode.clone();
    let printer_ip = config.printer_ip.clone();
    let reason = body
        .and_then(|b| b.reason.clone())
        .unwrap_or_else(|| "sale".to_string());
    drop(config);

    let req_id = generate_request_id();

    if mode == ConnectorRunMode::Simulation {
        log_event(&state.event_log, "drawer_simulation_only", "info",
            "Simulation mode — no bytes sent", &req_id, json!({ "reason": reason })).await;
        return ApiError::SimulationOnly.into_response();
    }

    if mode == ConnectorRunMode::Diagnostic {
        log_event(&state.event_log, "drawer_diagnostic_only", "info",
            "Diagnostic mode — no bytes sent", &req_id, json!({ "reason": reason })).await;
        return ApiError::DiagnosticOnly.into_response();
    }

    {
        let mut last = state.last_open.lock().await;
        if let Err(e) = check_rate_limit(&mut last) {
            return e.into_response();
        }
    }

    if printer_ip.is_empty() {
        log_event(&state.event_log, "drawer_printer_ip_missing", "error",
            "Printer IP not configured", &req_id, json!({ "reason": reason })).await;
        return ApiError::PrinterIpMissing.into_response();
    }

    send_drawer_kick(&printer_ip, &req_id, &state.event_log, &reason).await.into_response()
}

async fn send_drawer_kick(
    ip: &str,
    req_id: &str,
    event_log: &Arc<Mutex<VecDeque<ConnectorEvent>>>,
    reason: &str,
) -> impl IntoResponse {
    use tokio::io::AsyncWriteExt;
    let addr = format!("{}:9100", ip);

    let connect_result = tokio::time::timeout(
        Duration::from_millis(1500),
        tokio::net::TcpStream::connect(&addr),
    )
    .await;

    match connect_result {
        Err(_) => {
            log_event(event_log, "drawer_connection_timeout", "error",
                "TCP connection to printer timed out", req_id,
                json!({ "printerIp": ip, "reason": reason })).await;
            (StatusCode::SERVICE_UNAVAILABLE, Json(json!({
                "ok": false, "result": "printer_connection_timeout",
                "errorCode": "PRINTER_CONNECTION_TIMEOUT",
                "message": "Printer connection timed out.",
                "suggestion": "Check the printer IP and that port 9100 is reachable.",
                "requestId": req_id, "connectorVersion": VERSION,
            }))).into_response()
        }
        Ok(Err(e)) => {
            let (rc, msg) = if e.kind() == std::io::ErrorKind::ConnectionRefused {
                ("printer_connection_refused", "Printer refused TCP connection on port 9100.")
            } else {
                ("printer_unreachable", "Printer is not reachable.")
            };
            log_event(event_log, "drawer_connection_failed", "error", msg, req_id,
                json!({ "printerIp": ip, "error": e.to_string(), "reason": reason })).await;
            (StatusCode::SERVICE_UNAVAILABLE, Json(json!({
                "ok": false, "result": rc,
                "errorCode": rc.to_uppercase().replace('-', "_"),
                "message": msg,
                "suggestion": "Ensure printer is powered on and port 9100 is open.",
                "requestId": req_id, "connectorVersion": VERSION,
            }))).into_response()
        }
        Ok(Ok(mut stream)) => {
            match stream.write_all(&[0x1B, 0x70, 0x00, 0x19, 0xFA]).await {
                Ok(_) => {
                    log_event(event_log, "drawer_command_sent", "success",
                        "ESC/POS drawer kick sent", req_id,
                        json!({ "printerIp": ip, "reason": reason })).await;
                    Json(json!({
                        "ok": true, "result": "command_sent",
                        "message": "Drawer kick command sent.",
                        "requestId": req_id, "connectorVersion": VERSION,
                    })).into_response()
                }
                Err(e) => {
                    log_event(event_log, "drawer_write_failed", "error",
                        "Failed to write ESC/POS bytes", req_id,
                        json!({ "printerIp": ip, "error": e.to_string(), "reason": reason })).await;
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({
                        "ok": false, "result": "printer_write_failed",
                        "errorCode": "PRINTER_WRITE_FAILED",
                        "message": "Connected but failed to write ESC/POS bytes.",
                        "suggestion": "Check printer cable and ESC/POS support.",
                        "requestId": req_id, "connectorVersion": VERSION,
                    }))).into_response()
                }
            }
        }
    }
}

// ──────────────────────────────────────────────
// POST /simulate
// ──────────────────────────────────────────────

pub async fn simulate(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let config = state.config.read().await;
    if let Err(e) = validate_auth(&headers, &config) {
        drop(config);
        return e.into_response();
    }
    drop(config);

    let req_id = generate_request_id();
    let start = Instant::now();

    log_event(&state.event_log, "simulation_run", "info",
        "Simulation test passed — no printer needed", &req_id, json!({})).await;

    let duration_ms = start.elapsed().as_millis() as u64;

    Json(json!({
        "ok": true,
        "result": "simulation_success",
        "message": "Simulation succeeded. Connector ↔ web communication is working.",
        "durationMs": duration_ms,
        "requestId": req_id,
        "connectorVersion": VERSION,
    }))
    .into_response()
}

// ──────────────────────────────────────────────
// POST /diagnostics/run
// ──────────────────────────────────────────────

#[derive(Serialize)]
struct DiagCheck {
    name: &'static str,
    result: &'static str,
    message: String,
    #[serde(rename = "suggestion", skip_serializing_if = "Option::is_none")]
    suggestion: Option<&'static str>,
}

pub async fn diagnostics_run(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let config = state.config.read().await;
    if let Err(e) = validate_auth(&headers, &config) {
        drop(config);
        return e.into_response();
    }

    let mode = config.connector_run_mode.clone();
    let printer_ip = config.printer_ip.clone();
    let paired = !config.pairing_token.is_empty();
    let setup_complete = config.setup_complete;
    let hw_verified = config.hardware_verified;
    drop(config);

    let req_id = generate_request_id();
    let start = Instant::now();
    let mut checks: Vec<DiagCheck> = Vec::new();
    let mut overall = "passed";

    checks.push(DiagCheck { name: "connector_alive", result: "passed",
        message: format!("Connector v{} running in {} mode.", VERSION, mode.as_str()),
        suggestion: None });

    checks.push(DiagCheck { name: "paired",
        result: if paired { "passed" } else { "failed" },
        message: if paired { "Terminal is paired.".into() } else { "No pairing token configured.".into() },
        suggestion: if paired { None } else { Some("Complete the pairing step in the connector wizard.") },
    });
    if !paired { overall = "failed"; }

    checks.push(DiagCheck { name: "setup_complete",
        result: if setup_complete { "passed" } else { "warning" },
        message: if setup_complete { "Setup wizard completed.".into() } else { "Setup not complete.".into() },
        suggestion: if setup_complete { None } else { Some("Finish the Setup wizard.") },
    });
    if !setup_complete && overall == "passed" { overall = "warning"; }

    let has_ip = !printer_ip.is_empty();
    checks.push(DiagCheck { name: "printer_ip_configured",
        result: if has_ip { "passed" } else { "failed" },
        message: if has_ip { format!("Printer IP: {}", printer_ip) } else { "Printer IP not configured.".into() },
        suggestion: if has_ip { None } else { Some("Enter printer IP in Setup tab.") },
    });
    if !has_ip { overall = "failed"; }

    if has_ip {
        let addr = format!("{}:9100", printer_ip);
        let reachable = tokio::time::timeout(
            Duration::from_millis(800),
            tokio::net::TcpStream::connect(&addr),
        ).await;

        let (port_result, port_msg, port_sug) = match reachable {
            Err(_) => ("failed", format!("TCP to {}:9100 timed out.", printer_ip),
                Some("Check firewall / ensure printer is on.")),
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => (
                "failed", format!("Port 9100 refused on {}.", printer_ip),
                Some("ESC/POS network printing may not be enabled.")),
            Ok(Err(_)) => ("failed", format!("Cannot reach {} on port 9100.", printer_ip),
                Some("Check IP and that printer is on same LAN.")),
            Ok(Ok(_)) => ("passed", format!("Port 9100 open on {}.", printer_ip), None),
        };
        let is_passed = port_result == "passed";
        checks.push(DiagCheck { name: "printer_port_9100", result: port_result,
            message: port_msg, suggestion: port_sug });
        if !is_passed { overall = "failed"; }
    }

    checks.push(DiagCheck { name: "hardware_verified",
        result: if hw_verified { "passed" } else { "warning" },
        message: if hw_verified { "Cash drawer hardware verified.".into() }
                 else { "Hardware not verified.".into() },
        suggestion: if hw_verified { None } else { Some("Run live test in Setup to verify hardware.") },
    });
    if !hw_verified && overall == "passed" { overall = "warning"; }

    let (lr, lm) = match mode {
        ConnectorRunMode::Live => ("passed", "Connector in Live mode — ready."),
        ConnectorRunMode::Simulation => ("warning", "Simulation mode — no commands sent to printer."),
        ConnectorRunMode::Diagnostic => ("warning", "Diagnostic mode — no commands sent to printer."),
    };
    checks.push(DiagCheck { name: "live_ready", result: lr, message: lm.into(),
        suggestion: if mode == ConnectorRunMode::Live { None }
                    else { Some("Switch to Live mode in Advanced when ready.") },
    });
    if mode != ConnectorRunMode::Live && overall == "passed" { overall = "warning"; }

    let duration_ms = start.elapsed().as_millis() as u64;
    let sev = if overall == "passed" { "success" } else if overall == "warning" { "warning" } else { "error" };
    log_event(&state.event_log, "diagnostics_run", sev,
        &format!("Diagnostics: {}", overall), &req_id,
        json!({ "result": overall, "checks": checks.len() })).await;

    Json(json!({
        "ok": overall != "failed",
        "result": format!("diagnostic_{}", overall),
        "overall": overall,
        "checks": checks,
        "durationMs": duration_ms,
        "requestId": req_id,
        "connectorVersion": VERSION,
    }))
    .into_response()
}

// ──────────────────────────────────────────────
// GET /events/recent
// ──────────────────────────────────────────────

pub async fn events_recent(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let config = state.config.read().await;
    if let Err(e) = validate_auth(&headers, &config) {
        drop(config);
        return e.into_response();
    }
    drop(config);

    let log = state.event_log.lock().await;
    let events: Vec<&ConnectorEvent> = log.iter().rev().take(MAX_EVENTS).collect();
    Json(json!({
        "ok": true,
        "events": events,
        "count": events.len(),
    }))
    .into_response()
}

// ──────────────────────────────────────────────
// Headless server entry point
// ──────────────────────────────────────────────

pub async fn run_headless() {
    let config_path = "/tmp/franchisetech-connector-config.json";
    let config = load_config_from_path(config_path);
    let state = new_state(config);
    if let Err(e) = try_run_server_with_state(state).await {
        eprintln!("Server error: {}", e);
    }
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::{Request, StatusCode}};
    use tower::ServiceExt;

    fn test_config() -> ConnectorConfig {
        ConnectorConfig {
            printer_type: "network_escpos".to_string(),
            printer_ip: "192.168.1.100".to_string(),
            printer_port: 9100,
            drawer_kick_command: "1B700019FA".to_string(),
            pairing_token: "FT-testtoken123".to_string(),
            allowed_origins: vec!["http://localhost:3000".to_string()],
            dev_mode: false,
            start_at_login: false,
            setup_complete: true,
            hardware_verified: true,
            last_hardware_verified_at: None,
            hardware_verification_source: None,
            connector_run_mode: ConnectorRunMode::Live,
        }
    }

    async fn post_json(app: Router, path: &str, origin: &str, token: &str, body: &str) -> axum::response::Response {
        let req = Request::builder().method("POST").uri(path)
            .header("content-type", "application/json")
            .header("origin", origin)
            .header("x-connector-token", token)
            .body(Body::from(body.to_string())).unwrap();
        app.oneshot(req).await.unwrap()
    }

    async fn get_req(app: Router, path: &str, origin: &str, token: &str) -> axum::response::Response {
        let req = Request::builder().method("GET").uri(path)
            .header("origin", origin)
            .header("x-connector-token", token)
            .body(Body::empty()).unwrap();
        app.oneshot(req).await.unwrap()
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let app = build_router(new_state(test_config()), vec!["http://localhost:3000".to_string()]);
        let res = get_req(app, "/health", "http://localhost:3000", "FT-testtoken123").await;
        assert_eq!(res.status(), StatusCode::OK);
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(body["ok"], true);
        assert_eq!(body["name"], "franchisetech-connector");
    }

    #[tokio::test]
    async fn health_includes_mode_and_server_time() {
        let app = build_router(new_state(test_config()), vec!["http://localhost:3000".to_string()]);
        let res = get_req(app, "/health", "http://localhost:3000", "FT-testtoken123").await;
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(body["mode"], "live");
        assert!(body["serverTime"].as_u64().unwrap_or(0) > 0);
    }

    #[tokio::test]
    async fn pair_status_returns_approved() {
        let app = build_router(new_state(test_config()), vec!["http://localhost:3000".to_string()]);
        let res = get_req(app, "/pair/status", "http://localhost:3000", "FT-testtoken123").await;
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(body["status"], "approved");
    }

    #[tokio::test]
    async fn pairing_token_has_ft_prefix() {
        let token = generate_pairing_token();
        assert!(token.starts_with("FT-"), "Token should start with FT-, got: {}", token);
    }

    #[tokio::test]
    async fn hardware_verified_defaults_to_false() {
        let cfg = default_config();
        assert!(!cfg.hardware_verified);
        assert!(cfg.last_hardware_verified_at.is_none());
    }

    #[tokio::test]
    async fn connector_run_mode_defaults_to_simulation() {
        let cfg = default_config();
        assert_eq!(cfg.connector_run_mode, ConnectorRunMode::Simulation);
    }

    #[tokio::test]
    async fn origin_rejected_returns_403() {
        let app = build_router(new_state(test_config()), vec!["http://localhost:3000".to_string()]);
        let res = post_json(app, "/open-drawer", "http://evil.com", "FT-testtoken123", r#"{"reason":"sale"}"#).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(body["result"], "origin_rejected");
    }

    #[tokio::test]
    async fn invalid_token_returns_401() {
        let app = build_router(new_state(test_config()), vec!["http://localhost:3000".to_string()]);
        let res = post_json(app, "/open-drawer", "http://localhost:3000", "FT-wrong", r#"{"reason":"sale"}"#).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(body["result"], "invalid_token");
    }

    #[tokio::test]
    async fn missing_token_returns_401() {
        let app = build_router(new_state(test_config()), vec!["http://localhost:3000".to_string()]);
        let req = Request::builder().method("POST").uri("/open-drawer")
            .header("content-type", "application/json")
            .header("origin", "http://localhost:3000")
            .body(Body::from(r#"{"reason":"sale"}"#)).unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(body["result"], "missing_token");
    }

    #[tokio::test]
    async fn approve_pairing_does_not_auto_set_setup_complete() {
        let mut cfg = default_config();
        cfg.allowed_origins = vec!["http://localhost:3000".to_string()];
        let state = new_state(cfg);
        let pending = PendingPairing {
            token: "FT-abc".to_string(),
            terminal_id: String::new(),
            location_id: String::new(),
            origin: "http://localhost:3000".to_string(),
            requested_at: SystemTime::now(),
        };
        *state.pending_pairing.lock().await = Some(pending);
        approve_pairing_request(&state).await.unwrap();
        let config = state.config.read().await;
        assert_eq!(config.pairing_token, "FT-abc");
        assert!(!config.setup_complete, "setup_complete must not be auto-set");
    }

    #[tokio::test]
    async fn simulation_mode_open_drawer_returns_simulation_only() {
        let mut cfg = test_config();
        cfg.connector_run_mode = ConnectorRunMode::Simulation;
        cfg.printer_ip = String::new();
        let app = build_router(new_state(cfg), vec!["http://localhost:3000".to_string()]);
        let res = post_json(app, "/open-drawer", "http://localhost:3000", "FT-testtoken123", r#"{"reason":"sale"}"#).await;
        assert_eq!(res.status(), StatusCode::OK);
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(body["result"], "simulation_only");
    }

    #[tokio::test]
    async fn simulation_mode_test_drawer_returns_simulation_only() {
        let mut cfg = test_config();
        cfg.connector_run_mode = ConnectorRunMode::Simulation;
        cfg.printer_ip = String::new();
        let app = build_router(new_state(cfg), vec!["http://localhost:3000".to_string()]);
        let res = post_json(app, "/test-drawer", "http://localhost:3000", "FT-testtoken123", r#"{}"#).await;
        assert_eq!(res.status(), StatusCode::OK);
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(body["result"], "simulation_only");
    }

    #[tokio::test]
    async fn diagnostic_mode_returns_diagnostic_only() {
        let mut cfg = test_config();
        cfg.connector_run_mode = ConnectorRunMode::Diagnostic;
        let app = build_router(new_state(cfg), vec!["http://localhost:3000".to_string()]);
        let res = post_json(app, "/open-drawer", "http://localhost:3000", "FT-testtoken123", r#"{"reason":"sale"}"#).await;
        assert_eq!(res.status(), StatusCode::OK);
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(body["result"], "diagnostic_only");
    }

    #[tokio::test]
    async fn simulate_endpoint_returns_simulation_success() {
        let app = build_router(new_state(test_config()), vec!["http://localhost:3000".to_string()]);
        let res = post_json(app, "/simulate", "http://localhost:3000", "FT-testtoken123", r#"{}"#).await;
        assert_eq!(res.status(), StatusCode::OK);
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(body["result"], "simulation_success");
        assert_eq!(body["ok"], true);
        assert!(body["durationMs"].as_u64().is_some());
    }

    #[tokio::test]
    async fn simulate_endpoint_works_without_printer_ip() {
        let mut cfg = test_config();
        cfg.printer_ip = String::new();
        let app = build_router(new_state(cfg), vec!["http://localhost:3000".to_string()]);
        let res = post_json(app, "/simulate", "http://localhost:3000", "FT-testtoken123", r#"{}"#).await;
        assert_eq!(res.status(), StatusCode::OK);
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(body["result"], "simulation_success");
    }

    #[tokio::test]
    async fn diagnostics_returns_printer_ip_missing_check_when_no_ip() {
        let mut cfg = test_config();
        cfg.printer_ip = String::new();
        let app = build_router(new_state(cfg), vec!["http://localhost:3000".to_string()]);
        let res = post_json(app, "/diagnostics/run", "http://localhost:3000", "FT-testtoken123", r#"{}"#).await;
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
        let checks = body["checks"].as_array().unwrap();
        let ip_check = checks.iter().find(|c| c["name"] == "printer_ip_configured").unwrap();
        assert_eq!(ip_check["result"], "failed");
        assert_eq!(body["overall"], "failed");
    }

    #[tokio::test]
    async fn diagnostics_run_includes_required_checks() {
        let app = build_router(new_state(test_config()), vec!["http://localhost:3000".to_string()]);
        let res = post_json(app, "/diagnostics/run", "http://localhost:3000", "FT-testtoken123", r#"{}"#).await;
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
        let checks = body["checks"].as_array().unwrap();
        let names: Vec<&str> = checks.iter().filter_map(|c| c["name"].as_str()).collect();
        assert!(names.contains(&"connector_alive"));
        assert!(names.contains(&"paired"));
        assert!(names.contains(&"printer_ip_configured"));
        assert!(names.contains(&"hardware_verified"));
        assert!(names.contains(&"live_ready"));
    }

    #[tokio::test]
    async fn events_recent_returns_log() {
        let state = new_state(test_config());
        let app_for_sim = build_router(state.clone(), vec!["http://localhost:3000".to_string()]);
        let _ = post_json(app_for_sim, "/simulate", "http://localhost:3000", "FT-testtoken123", r#"{}"#).await;
        let app = build_router(state, vec!["http://localhost:3000".to_string()]);
        let res = get_req(app, "/events/recent", "http://localhost:3000", "FT-testtoken123").await;
        assert_eq!(res.status(), StatusCode::OK);
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert!(body["events"].as_array().unwrap().len() > 0);
    }

    #[tokio::test]
    async fn events_do_not_include_token() {
        let state = new_state(test_config());
        log_event(&state.event_log, "test", "info", "test event", "req-123",
            json!({"note": "should-not-contain-token"})).await;
        let app = build_router(state, vec!["http://localhost:3000".to_string()]);
        let res = get_req(app, "/events/recent", "http://localhost:3000", "FT-testtoken123").await;
        let body = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(!body_str.contains("FT-testtoken123"));
    }

    #[tokio::test]
    async fn live_mode_printer_ip_missing_returns_specific_error() {
        let mut cfg = test_config();
        cfg.connector_run_mode = ConnectorRunMode::Live;
        cfg.printer_ip = String::new();
        let app = build_router(new_state(cfg), vec!["http://localhost:3000".to_string()]);
        let res = post_json(app, "/open-drawer", "http://localhost:3000", "FT-testtoken123", r#"{"reason":"sale"}"#).await;
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(body["result"], "printer_ip_missing");
    }

    #[tokio::test]
    async fn finish_setup_state_sets_setup_complete() {
        let state = new_state(default_config());
        finish_setup_state(&state, true).await;
        let config = state.config.read().await;
        assert!(config.setup_complete);
        assert!(config.hardware_verified);
        assert!(config.last_hardware_verified_at.is_some());
    }

    #[tokio::test]
    async fn finish_setup_without_hw_does_not_set_hw_verified() {
        let state = new_state(default_config());
        finish_setup_state(&state, false).await;
        let config = state.config.read().await;
        assert!(config.setup_complete);
        assert!(!config.hardware_verified);
    }
}
