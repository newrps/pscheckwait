mod backend;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Json, Redirect},
    routing::get,
    Router,
};
use backend::{AdmitForcedOutcome, Backend, BackendFactory, EnterOutcome, RefreshOutcome};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{broadcast, Mutex, RwLock};
use tower_http::cors::CorsLayer;
use uuid::Uuid;

// ------------------------------------------------------------
// Embedded static assets
// ------------------------------------------------------------

const LOADER_JS: &str = include_str!("../../loader/pscheckwait.js");
const DEMO_HTML: &str = include_str!("../../demo/index.html");
const WAIT_HTML: &str = include_str!("../../wait/index.html");

#[derive(rust_embed::Embed)]
#[folder = "../admin-sveltekit/build/"]
struct AdminAssets;

async fn serve_loader() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/javascript; charset=utf-8")], LOADER_JS)
}

async fn serve_demo() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], DEMO_HTML)
}

async fn serve_wait() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], WAIT_HTML)
}

async fn serve_admin_index() -> impl IntoResponse {
    serve_admin_file("index.html").await
}

async fn serve_admin_path(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl IntoResponse {
    serve_admin_file(&path).await
}

async fn serve_admin_file(path: &str) -> axum::response::Response {
    let path = path.trim_start_matches('/');
    let file = AdminAssets::get(path).or_else(|| AdminAssets::get("index.html"));
    match file {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let body = content.data.into_owned();
            let headers = [(header::CONTENT_TYPE, mime.as_ref().to_string())];
            (headers, body).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn root_redirect() -> Redirect {
    Redirect::to("/admin/")
}

const ACTIVE_TTL: Duration = Duration::from_secs(60);
const WAITING_TTL: Duration = Duration::from_secs(30);

#[derive(Clone, Serialize, Deserialize)]
struct SiteConfig {
    max_active: usize,
    title: String,
    subtitle: String,
    position_label: String,
    meta_format: String,
    #[serde(default)]
    allowed_origins: Vec<String>,
    #[serde(default = "default_true")]
    verify_origin: bool,
}

fn default_true() -> bool { true }

impl Default for SiteConfig {
    fn default() -> Self {
        Self {
            max_active: 3,
            title: "접속 대기중".into(),
            subtitle: "현재 접속자가 많아 잠시 대기중입니다".into(),
            position_label: "번째 순서".into(),
            meta_format: "대기 {waiting}명 · 입장 {active}명".into(),
            allowed_origins: Vec::new(),
            verify_origin: true,
        }
    }
}

fn host_from_origin(raw: &str) -> Option<String> {
    let s = raw.trim();
    let s = s.split_once("://").map(|(_, r)| r).unwrap_or(s);
    let s = s.split('/').next()?;
    let s = s.split('?').next()?;
    let s = s.split('#').next()?;
    let host = s.rsplit_once(':').map(|(h, _)| h).unwrap_or(s);
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.is_empty() { None } else { Some(host.to_lowercase()) }
}

fn extract_origin_host(headers: &HeaderMap) -> Option<String> {
    for name in ["origin", "referer"] {
        if let Some(v) = headers.get(name).and_then(|h| h.to_str().ok()) {
            if let Some(h) = host_from_origin(v) {
                return Some(h);
            }
        }
    }
    None
}

fn check_origin(headers: &HeaderMap, domain: &str, cfg: &SiteConfig) -> Result<(), (StatusCode, &'static str)> {
    if !cfg.verify_origin { return Ok(()); }
    let origin_host = match extract_origin_host(headers) {
        Some(h) => h,
        None => return Err((StatusCode::FORBIDDEN, "missing Origin/Referer")),
    };
    // Same-origin to our own server (e.g., wait/admin pages calling API) — always allow.
    if let Some(host_hdr) = headers.get("host").and_then(|h| h.to_str().ok()) {
        let host = host_hdr.split(':').next().unwrap_or("").to_lowercase();
        if !host.is_empty() && host == origin_host {
            return Ok(());
        }
    }
    let allowed_hosts: Vec<String> = if cfg.allowed_origins.is_empty() {
        vec![domain.to_lowercase()]
    } else {
        cfg.allowed_origins.iter()
            .filter_map(|s| host_from_origin(s).or_else(|| Some(s.to_lowercase())))
            .collect()
    };
    if allowed_hosts.iter().any(|h| h == &origin_host) { Ok(()) }
    else { Err((StatusCode::FORBIDDEN, "origin not allowed")) }
}

#[derive(Clone, Debug)]
enum TokenStatus {
    Admitted,
    Waiting(usize),
}

#[derive(Clone, Debug)]
struct WsTick {
    index: Arc<HashMap<String, TokenStatus>>,
    active: usize,
    waiting: usize,
}

struct SiteState {
    config: RwLock<SiteConfig>,
    backend: Arc<dyn Backend>,
    tx: broadcast::Sender<Arc<WsTick>>,
}

impl SiteState {
    fn new(config: SiteConfig, backend: Arc<dyn Backend>) -> Self {
        let (tx, _) = broadcast::channel(16);
        Self { config: RwLock::new(config), backend, tx }
    }
}

#[derive(Clone, Serialize, Deserialize, Default)]
struct ServerConfig {
    #[serde(default)]
    admin_token: String,
    #[serde(default)]
    redis_url: String,
}

#[derive(Clone, Serialize)]
struct AdminSiteSummary {
    domain: String,
    active: usize,
    waiting: usize,
    max_active: usize,
}

#[derive(Clone, Serialize)]
struct AdminTick {
    sites: Vec<AdminSiteSummary>,
}

#[derive(Clone)]
struct AppState {
    sites: Arc<DashMap<String, Arc<SiteState>>>,
    admin_token: Arc<RwLock<String>>,
    server_config: Arc<RwLock<ServerConfig>>,
    data_dir: Arc<Option<PathBuf>>,
    save_lock: Arc<Mutex<()>>,
    factory: BackendFactory,
    admin_tx: broadcast::Sender<Arc<AdminTick>>,
}

#[derive(Serialize, Deserialize, Default)]
struct PersistedState {
    sites: BTreeMap<String, SiteConfig>,
}

impl AppState {
    fn new(
        admin_token: String,
        server_config: ServerConfig,
        data_dir: Option<PathBuf>,
        factory: BackendFactory,
    ) -> Self {
        let (admin_tx, _) = broadcast::channel(8);
        Self {
            sites: Arc::new(DashMap::new()),
            admin_token: Arc::new(RwLock::new(admin_token)),
            server_config: Arc::new(RwLock::new(server_config)),
            data_dir: Arc::new(data_dir),
            save_lock: Arc::new(Mutex::new(())),
            factory,
            admin_tx,
        }
    }

    async fn is_first_run(&self) -> bool {
        self.admin_token.read().await.is_empty()
    }

    fn sites_path(&self) -> Option<PathBuf> {
        self.data_dir.as_ref().clone().map(|d| d.join("sites.json"))
    }

    fn config_path(&self) -> Option<PathBuf> {
        self.data_dir.as_ref().clone().map(|d| d.join("config.json"))
    }

    async fn save_server_config(&self) -> std::io::Result<()> {
        let Some(path) = self.config_path() else { return Ok(()); };
        let _g = self.save_lock.lock().await;
        let cfg = self.server_config.read().await.clone();
        let bytes = serde_json::to_vec_pretty(&cfg)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp = path.with_extension("json.tmp");
        tokio::fs::write(&tmp, bytes).await?;
        tokio::fs::rename(&tmp, &path).await?;
        Ok(())
    }

    fn get_site(&self, domain: &str) -> Option<Arc<SiteState>> {
        self.sites.get(domain).map(|e| e.clone())
    }

    fn insert_site(&self, domain: String, cfg: SiteConfig) {
        let backend = self.factory.make(&domain);
        self.sites.insert(domain, Arc::new(SiteState::new(cfg, backend)));
    }

    async fn load_or_seed(&self) {
        let loaded = match self.sites_path() {
            Some(path) => match tokio::fs::read(&path).await {
                Ok(bytes) => match serde_json::from_slice::<PersistedState>(&bytes) {
                    Ok(s) => Some(s),
                    Err(e) => { tracing::error!("invalid sites.json, ignoring: {e}"); None }
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => { tracing::error!("failed to read sites.json: {e}"); None }
            },
            None => None,
        };

        if let Some(state) = loaded {
            for (domain, cfg) in state.sites {
                self.insert_site(domain, cfg);
            }
            tracing::info!("loaded {} sites from disk", self.sites.len());
        }

        if self.sites.is_empty() {
            self.insert_site("localhost".to_string(), SiteConfig::default());
            tracing::info!("seeded default 'localhost' site");
            if self.data_dir.is_some() {
                let _ = self.persist().await;
            }
        }
    }

    async fn persist(&self) -> std::io::Result<()> {
        let Some(path) = self.sites_path() else { return Ok(()); };
        let _g = self.save_lock.lock().await;

        let mut snapshot = PersistedState::default();
        for entry in self.sites.iter() {
            let cfg = entry.value().config.read().await.clone();
            snapshot.sites.insert(entry.key().clone(), cfg);
        }

        let bytes = serde_json::to_vec_pretty(&snapshot)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp = path.with_extension("json.tmp");
        tokio::fs::write(&tmp, bytes).await?;
        tokio::fs::rename(&tmp, &path).await?;
        Ok(())
    }
}

// ------------------------------------------------------------
// Public API
// ------------------------------------------------------------

#[derive(Deserialize)]
struct SiteQuery { site: String }

#[derive(Deserialize)]
struct TokenSiteQuery { site: String, token: String }

#[derive(Serialize)]
struct EnterResponse {
    token: String,
    status: String,
    position: usize,
    config: SiteConfig,
}

#[derive(Serialize)]
struct StatusResponse {
    status: String,
    position: usize,
    total_waiting: usize,
    active: usize,
    config: SiteConfig,
}

async fn enter(
    State(state): State<AppState>,
    Query(q): Query<SiteQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Some(site) = state.get_site(&q.site) else {
        return (StatusCode::NOT_FOUND, "unknown site").into_response();
    };
    let config = site.config.read().await.clone();
    if let Err((s, m)) = check_origin(&headers, &q.site, &config) {
        return (s, m).into_response();
    }
    let token = Uuid::new_v4().to_string();
    let outcome = site.backend.try_enter(&token, config.max_active).await;
    let (status, position) = match outcome {
        EnterOutcome::Admitted => ("admitted", 0),
        EnterOutcome::Waiting(p) => ("waiting", p),
    };
    Json(EnterResponse {
        token,
        status: status.into(),
        position,
        config,
    }).into_response()
}

async fn status(
    State(state): State<AppState>,
    Query(q): Query<TokenSiteQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Some(site) = state.get_site(&q.site) else {
        return (StatusCode::NOT_FOUND, "unknown site").into_response();
    };
    let config = {
        let cfg = site.config.read().await;
        if let Err((s, m)) = check_origin(&headers, &q.site, &cfg) {
            return (s, m).into_response();
        }
        cfg.clone()
    };
    let outcome = site.backend.refresh(&q.token, config.max_active).await;
    let (active_cnt, waiting_cnt) = site.backend.stats().await;
    match outcome {
        RefreshOutcome::Admitted => Json(StatusResponse {
            status: "admitted".into(),
            position: 0,
            total_waiting: waiting_cnt,
            active: active_cnt,
            config,
        }).into_response(),
        RefreshOutcome::Waiting(p) => Json(StatusResponse {
            status: "waiting".into(),
            position: p,
            total_waiting: waiting_cnt,
            active: active_cnt,
            config,
        }).into_response(),
        RefreshOutcome::Unknown => (StatusCode::NOT_FOUND, "unknown token").into_response(),
    }
}

async fn heartbeat(
    State(state): State<AppState>,
    Query(q): Query<TokenSiteQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Some(site) = state.get_site(&q.site) else { return StatusCode::NOT_FOUND; };
    {
        let cfg = site.config.read().await;
        if check_origin(&headers, &q.site, &cfg).is_err() { return StatusCode::FORBIDDEN; }
    }
    if site.backend.heartbeat(&q.token).await { StatusCode::OK } else { StatusCode::NOT_FOUND }
}

async fn leave(
    State(state): State<AppState>,
    Query(q): Query<TokenSiteQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Some(site) = state.get_site(&q.site) else { return StatusCode::NOT_FOUND; };
    {
        let cfg = site.config.read().await;
        if check_origin(&headers, &q.site, &cfg).is_err() { return StatusCode::FORBIDDEN; }
    }
    site.backend.leave(&q.token).await;
    StatusCode::OK
}

async fn health() -> &'static str { "ok" }

// ------------------------------------------------------------
// Setup wizard (first-run only)
// ------------------------------------------------------------

#[derive(Deserialize)]
struct SetupRequest {
    admin_token: String,
    #[serde(default)]
    redis_url: String,
}

#[derive(Serialize)]
struct SetupResponse {
    ok: bool,
    restart_required: bool,
}

async fn api_setup(
    State(state): State<AppState>,
    Json(body): Json<SetupRequest>,
) -> impl IntoResponse {
    if !state.is_first_run().await {
        return (StatusCode::CONFLICT, "setup already done").into_response();
    }
    let token = body.admin_token.trim().to_string();
    if token.len() < 8 {
        return (StatusCode::BAD_REQUEST, "admin_token too short (>=8 chars)").into_response();
    }
    let redis_url = body.redis_url.trim().to_string();

    {
        let mut t = state.admin_token.write().await;
        *t = token.clone();
    }
    {
        let mut c = state.server_config.write().await;
        c.admin_token = token;
        c.redis_url = redis_url.clone();
    }

    if let Err(e) = state.save_server_config().await {
        tracing::error!("failed to save server config: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "failed to save config").into_response();
    }

    let restart_required = !redis_url.is_empty()
        && matches!(state.factory, BackendFactory::Memory);

    Json(SetupResponse { ok: true, restart_required }).into_response()
}

// ------------------------------------------------------------
// WebSocket
// ------------------------------------------------------------

async fn ws_handler(
    State(state): State<AppState>,
    Query(q): Query<TokenSiteQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let Some(site) = state.get_site(&q.site) else {
        return (StatusCode::NOT_FOUND, "unknown site").into_response();
    };
    {
        let cfg = site.config.read().await;
        if let Err((s, m)) = check_origin(&headers, &q.site, &cfg) {
            return (s, m).into_response();
        }
    }
    ws.on_upgrade(move |socket| ws_session(socket, site, q.token))
        .into_response()
}

fn status_json(s: &str, position: usize, active: usize, waiting: usize) -> String {
    serde_json::json!({
        "status": s,
        "position": position,
        "active": active,
        "total_waiting": waiting,
    })
    .to_string()
}

// ------------------------------------------------------------
// Admin WebSocket
// ------------------------------------------------------------

#[derive(Deserialize)]
struct AdminWsQuery {
    token: String,
}

async fn admin_ws_handler(
    State(state): State<AppState>,
    Query(q): Query<AdminWsQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let expected = state.admin_token.read().await.clone();
    if expected.is_empty() || q.token != expected {
        return (StatusCode::UNAUTHORIZED, "invalid admin token").into_response();
    }
    ws.on_upgrade(move |socket| admin_ws_session(socket, state)).into_response()
}

async fn admin_ws_session(mut socket: WebSocket, state: AppState) {
    // 즉시 초기 상태 전송
    let mut summaries: Vec<AdminSiteSummary> = Vec::with_capacity(state.sites.len());
    for entry in state.sites.iter() {
        let domain = entry.key().clone();
        let site = entry.value();
        let max_active = site.config.read().await.max_active;
        let (active, waiting) = site.backend.stats().await;
        summaries.push(AdminSiteSummary { domain, active, waiting, max_active });
    }
    summaries.sort_by(|a, b| a.domain.cmp(&b.domain));
    let initial = serde_json::to_string(&AdminTick { sites: summaries }).unwrap_or_default();
    if socket.send(Message::Text(initial)).await.is_err() {
        return;
    }

    let mut rx = state.admin_tx.subscribe();
    let mut ping = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(20),
        Duration::from_secs(20),
    );

    loop {
        tokio::select! {
            recv = rx.recv() => {
                let tick = match recv {
                    Ok(t) => t,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                let payload = match serde_json::to_string(&*tick) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                if socket.send(Message::Text(payload)).await.is_err() {
                    break;
                }
            }
            _ = ping.tick() => {
                if socket.send(Message::Ping(Vec::new())).await.is_err() {
                    break;
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}

async fn ws_session(mut socket: WebSocket, site: Arc<SiteState>, token: String) {
    let max_active = site.config.read().await.max_active;
    let outcome = site.backend.refresh(&token, max_active).await;
    let (active, waiting) = site.backend.stats().await;

    let initial = match outcome {
        RefreshOutcome::Admitted => status_json("admitted", 0, active, waiting),
        RefreshOutcome::Waiting(p) => status_json("waiting", p, active, waiting),
        RefreshOutcome::Unknown => {
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({"status": "unknown"}).to_string(),
                ))
                .await;
            let _ = socket.close().await;
            return;
        }
    };
    if socket.send(Message::Text(initial)).await.is_err() {
        return;
    }

    let mut rx = site.tx.subscribe();
    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(15),
        Duration::from_secs(15),
    );

    loop {
        tokio::select! {
            recv = rx.recv() => {
                let tick = match recv {
                    Ok(t) => t,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                let payload = match tick.index.get(&token) {
                    Some(TokenStatus::Admitted) =>
                        status_json("admitted", 0, tick.active, tick.waiting),
                    Some(TokenStatus::Waiting(p)) =>
                        status_json("waiting", *p, tick.active, tick.waiting),
                    None => {
                        let _ = socket.send(Message::Text(
                            serde_json::json!({"status": "unknown"}).to_string()
                        )).await;
                        let _ = socket.close().await;
                        break;
                    }
                };
                if socket.send(Message::Text(payload)).await.is_err() {
                    break;
                }
            }
            _ = heartbeat.tick() => {
                if !site.backend.heartbeat(&token).await {
                    break;
                }
                if socket.send(Message::Ping(Vec::new())).await.is_err() {
                    break;
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(Message::Ping(p))) => {
                        let _ = socket.send(Message::Pong(p)).await;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => break,
                }
            }
        }
    }
}

#[derive(Serialize)]
struct SiteStats {
    domain: String,
    active: usize,
    waiting: usize,
    max_active: usize,
}

async fn stats(
    State(state): State<AppState>,
    Query(q): Query<SiteQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Some(site) = state.get_site(&q.site) else {
        return (StatusCode::NOT_FOUND, "unknown site").into_response();
    };
    let max_active = {
        let cfg = site.config.read().await;
        if let Err((s, m)) = check_origin(&headers, &q.site, &cfg) {
            return (s, m).into_response();
        }
        cfg.max_active
    };
    let (active, waiting) = site.backend.stats().await;
    Json(SiteStats { domain: q.site, active, waiting, max_active }).into_response()
}

// ------------------------------------------------------------
// Admin API
// ------------------------------------------------------------

async fn check_admin(headers: &HeaderMap, state: &AppState) -> Result<(), StatusCode> {
    let expected = state.admin_token.read().await;
    if expected.is_empty() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let auth = headers.get("authorization").and_then(|h| h.to_str().ok()).unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    if token == expected.as_str() { Ok(()) } else { Err(StatusCode::UNAUTHORIZED) }
}

fn normalize_domain(d: &str) -> String { d.trim().to_lowercase() }

#[derive(Serialize)]
struct SiteSummary {
    domain: String,
    active: usize,
    waiting: usize,
    max_active: usize,
}

async fn admin_list_sites(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = check_admin(&headers, &state).await { return e.into_response(); }
    let mut out = Vec::new();
    for entry in state.sites.iter() {
        let site = entry.value();
        let max_active = site.config.read().await.max_active;
        let (active, waiting) = site.backend.stats().await;
        out.push(SiteSummary {
            domain: entry.key().clone(),
            active,
            waiting,
            max_active,
        });
    }
    out.sort_by(|a, b| a.domain.cmp(&b.domain));
    Json(out).into_response()
}

#[derive(Deserialize)]
struct CreateSite {
    domain: String,
    #[serde(default)] max_active: Option<usize>,
    #[serde(default)] title: Option<String>,
    #[serde(default)] subtitle: Option<String>,
    #[serde(default)] position_label: Option<String>,
    #[serde(default)] meta_format: Option<String>,
    #[serde(default)] allowed_origins: Option<Vec<String>>,
    #[serde(default)] verify_origin: Option<bool>,
}

async fn admin_create_site(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateSite>,
) -> impl IntoResponse {
    if let Err(e) = check_admin(&headers, &state).await { return e.into_response(); }
    let domain = normalize_domain(&body.domain);
    if domain.is_empty() { return (StatusCode::BAD_REQUEST, "domain required").into_response(); }
    if state.sites.contains_key(&domain) { return (StatusCode::CONFLICT, "site already exists").into_response(); }
    let mut cfg = SiteConfig::default();
    if let Some(v) = body.max_active {
        if v == 0 { return (StatusCode::BAD_REQUEST, "max_active must be >= 1").into_response(); }
        cfg.max_active = v;
    }
    if let Some(v) = body.title { cfg.title = v; }
    if let Some(v) = body.subtitle { cfg.subtitle = v; }
    if let Some(v) = body.position_label { cfg.position_label = v; }
    if let Some(v) = body.meta_format { cfg.meta_format = v; }
    if let Some(v) = body.allowed_origins { cfg.allowed_origins = v; }
    if let Some(v) = body.verify_origin { cfg.verify_origin = v; }

    state.insert_site(domain.clone(), cfg.clone());
    if let Err(e) = state.persist().await { tracing::error!("persist failed: {e}"); }
    Json(serde_json::json!({ "domain": domain, "config": cfg })).into_response()
}

async fn admin_get_site(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(domain): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = check_admin(&headers, &state).await { return e.into_response(); }
    let domain = normalize_domain(&domain);
    let Some(site) = state.get_site(&domain) else { return (StatusCode::NOT_FOUND, "unknown site").into_response(); };
    let cfg = site.config.read().await.clone();
    let (active, waiting) = site.backend.stats().await;
    Json(serde_json::json!({ "domain": domain, "config": cfg, "active": active, "waiting": waiting })).into_response()
}

#[derive(Deserialize)]
struct ConfigUpdate {
    max_active: Option<usize>,
    title: Option<String>,
    subtitle: Option<String>,
    position_label: Option<String>,
    meta_format: Option<String>,
    allowed_origins: Option<Vec<String>>,
    verify_origin: Option<bool>,
}

async fn admin_update_site(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(domain): Path<String>,
    Json(update): Json<ConfigUpdate>,
) -> impl IntoResponse {
    if let Err(e) = check_admin(&headers, &state).await { return e.into_response(); }
    let domain = normalize_domain(&domain);
    let Some(site) = state.get_site(&domain) else { return (StatusCode::NOT_FOUND, "unknown site").into_response(); };
    let new_cfg = {
        let mut cfg = site.config.write().await;
        if let Some(v) = update.max_active {
            if v == 0 { return (StatusCode::BAD_REQUEST, "max_active must be >= 1").into_response(); }
            cfg.max_active = v;
        }
        if let Some(v) = update.title { cfg.title = v; }
        if let Some(v) = update.subtitle { cfg.subtitle = v; }
        if let Some(v) = update.position_label { cfg.position_label = v; }
        if let Some(v) = update.meta_format { cfg.meta_format = v; }
        if let Some(v) = update.allowed_origins { cfg.allowed_origins = v; }
        if let Some(v) = update.verify_origin { cfg.verify_origin = v; }
        cfg.clone()
    };
    site.backend.cleanup(new_cfg.max_active, ACTIVE_TTL, WAITING_TTL).await;
    if let Err(e) = state.persist().await { tracing::error!("persist failed: {e}"); }
    Json(new_cfg).into_response()
}

#[derive(Serialize)]
struct TokenInfo {
    token: String,
    idle_secs: u64,
    position: Option<usize>,
}

#[derive(Deserialize)]
struct PaginateQuery {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
}

async fn admin_list_queue(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(domain): Path<String>,
    Query(q): Query<PaginateQuery>,
) -> impl IntoResponse {
    if let Err(e) = check_admin(&headers, &state).await { return e.into_response(); }
    let domain = normalize_domain(&domain);
    let Some(site) = state.get_site(&domain) else { return (StatusCode::NOT_FOUND, "unknown site").into_response(); };
    let limit = q.limit.unwrap_or(100).min(500).max(1);
    let offset = q.offset.unwrap_or(0);
    let snap = site.backend.list_state_paginated(limit, offset).await;
    let to_info = |e: backend::TokenEntry| TokenInfo {
        token: e.token,
        idle_secs: e.idle_secs,
        position: e.position,
    };
    Json(serde_json::json!({
        "domain": domain,
        "active": snap.active.into_iter().map(to_info).collect::<Vec<_>>(),
        "waiting": snap.waiting.into_iter().map(to_info).collect::<Vec<_>>(),
        "total_active": snap.total_active,
        "total_waiting": snap.total_waiting,
        "limit": limit,
        "offset": offset,
    })).into_response()
}

#[derive(Deserialize)]
struct AdminTokenQuery { token: String }

async fn admin_kick(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(domain): Path<String>,
    Query(q): Query<AdminTokenQuery>,
) -> impl IntoResponse {
    if let Err(e) = check_admin(&headers, &state).await { return e.into_response(); }
    let domain = normalize_domain(&domain);
    let Some(site) = state.get_site(&domain) else { return (StatusCode::NOT_FOUND, "unknown site").into_response(); };
    if !site.backend.kick(&q.token).await {
        return (StatusCode::NOT_FOUND, "unknown token").into_response();
    }
    let max_active = site.config.read().await.max_active;
    site.backend.cleanup(max_active, ACTIVE_TTL, WAITING_TTL).await;
    StatusCode::NO_CONTENT.into_response()
}

async fn admin_admit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(domain): Path<String>,
    Query(q): Query<AdminTokenQuery>,
) -> impl IntoResponse {
    if let Err(e) = check_admin(&headers, &state).await { return e.into_response(); }
    let domain = normalize_domain(&domain);
    let Some(site) = state.get_site(&domain) else { return (StatusCode::NOT_FOUND, "unknown site").into_response(); };
    match site.backend.admit_forced(&q.token).await {
        AdmitForcedOutcome::AlreadyAdmitted => (StatusCode::OK, "already admitted").into_response(),
        AdmitForcedOutcome::Admitted => StatusCode::NO_CONTENT.into_response(),
        AdmitForcedOutcome::NotWaiting => (StatusCode::NOT_FOUND, "token not waiting").into_response(),
    }
}

async fn admin_clear(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(domain): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = check_admin(&headers, &state).await { return e.into_response(); }
    let domain = normalize_domain(&domain);
    let Some(site) = state.get_site(&domain) else { return (StatusCode::NOT_FOUND, "unknown site").into_response(); };
    site.backend.clear().await;
    StatusCode::NO_CONTENT.into_response()
}

async fn admin_delete_site(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(domain): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = check_admin(&headers, &state).await { return e.into_response(); }
    let domain = normalize_domain(&domain);
    let Some((_, site)) = state.sites.remove(&domain) else {
        return (StatusCode::NOT_FOUND, "unknown site").into_response();
    };
    site.backend.clear().await;
    if let Err(e) = state.persist().await { tracing::error!("persist failed: {e}"); }
    StatusCode::NO_CONTENT.into_response()
}

fn default_data_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("data")
}

async fn load_server_config(dir: &PathBuf) -> ServerConfig {
    let path = dir.join("config.json");
    match tokio::fs::read(&path).await {
        Ok(bytes) => match serde_json::from_slice::<ServerConfig>(&bytes) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("invalid config.json, ignoring: {e}");
                ServerConfig::default()
            }
        },
        Err(_) => ServerConfig::default(),
    }
}

fn open_browser(url: &str) {
    if std::env::var("PSCHECKWAIT_NO_BROWSER").ok().as_deref() == Some("1") {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let data_dir = match std::env::var("PSCHECKWAIT_DATA_DIR") {
        Ok(s) if s.eq_ignore_ascii_case("none") || s == "-" => None,
        Ok(s) => Some(PathBuf::from(s)),
        Err(_) => Some(default_data_dir()),
    };

    let mut server_config = if let Some(dir) = data_dir.as_ref() {
        load_server_config(dir).await
    } else {
        ServerConfig::default()
    };

    if let Ok(t) = std::env::var("PSCHECKWAIT_ADMIN_TOKEN") {
        if !t.is_empty() {
            server_config.admin_token = t;
        }
    }
    if let Ok(u) = std::env::var("PSCHECKWAIT_REDIS_URL") {
        if !u.is_empty() {
            server_config.redis_url = u;
        }
    }

    let factory = if !server_config.redis_url.is_empty() {
        tracing::info!("connecting to Redis at {}", server_config.redis_url);
        match redis::Client::open(server_config.redis_url.clone()) {
            Ok(client) => match redis::aio::ConnectionManager::new(client).await {
                Ok(conn) => {
                    tracing::info!("Redis connected — using RedisBackend");
                    BackendFactory::Redis(conn)
                }
                Err(e) => {
                    tracing::error!("Redis connect failed ({e}), falling back to memory");
                    BackendFactory::Memory
                }
            },
            Err(e) => {
                tracing::error!("Redis URL invalid ({e}), falling back to memory");
                BackendFactory::Memory
            }
        }
    } else {
        tracing::info!("using in-memory backend");
        BackendFactory::Memory
    };

    if let Some(p) = data_dir.as_ref() {
        tracing::info!("data directory: {}", p.display());
    } else {
        tracing::info!("persistence disabled (PSCHECKWAIT_DATA_DIR=none)");
    }

    let admin_token_initial = server_config.admin_token.clone();
    let state = AppState::new(admin_token_initial, server_config, data_dir, factory);
    state.load_or_seed().await;

    if state.is_first_run().await {
        tracing::warn!("FIRST RUN: open http://localhost:3000 to set admin token");
    }

    let bg_state = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        loop {
            tick.tick().await;
            let mut admin_summaries: Vec<AdminSiteSummary> = Vec::with_capacity(bg_state.sites.len());

            for entry in bg_state.sites.iter() {
                let domain = entry.key().clone();
                let site = entry.value().clone();
                let max_active = site.config.read().await.max_active;
                site.backend.cleanup(max_active, ACTIVE_TTL, WAITING_TTL).await;

                // 사용자 WS broadcast (구독자 있을 때만)
                if site.tx.receiver_count() > 0 {
                    let snap = site.backend.list_state().await;
                    let mut index = HashMap::with_capacity(snap.active.len() + snap.waiting.len());
                    for e in &snap.active {
                        index.insert(e.token.clone(), TokenStatus::Admitted);
                    }
                    for e in &snap.waiting {
                        index.insert(
                            e.token.clone(),
                            TokenStatus::Waiting(e.position.unwrap_or(0)),
                        );
                    }
                    let _ = site.tx.send(Arc::new(WsTick {
                        active: snap.active.len(),
                        waiting: snap.waiting.len(),
                        index: Arc::new(index),
                    }));
                }

                // 어드민용 요약 (가벼움 — 카운트만)
                if bg_state.admin_tx.receiver_count() > 0 {
                    let (active, waiting) = site.backend.stats().await;
                    admin_summaries.push(AdminSiteSummary {
                        domain,
                        active,
                        waiting,
                        max_active,
                    });
                }
            }

            // 어드민 broadcast
            if bg_state.admin_tx.receiver_count() > 0 {
                admin_summaries.sort_by(|a, b| a.domain.cmp(&b.domain));
                let _ = bg_state.admin_tx.send(Arc::new(AdminTick { sites: admin_summaries }));
            }
        }
    });

    let api = Router::new()
        .route("/enter", axum::routing::post(enter))
        .route("/status", get(status))
        .route("/heartbeat", axum::routing::post(heartbeat))
        .route("/leave", axum::routing::post(leave))
        .route("/stats", get(stats))
        .route("/health", get(health))
        .route("/ws", get(ws_handler))
        .route("/admin/ws", get(admin_ws_handler))
        .route("/setup", axum::routing::post(api_setup))
        .route("/admin/sites", get(admin_list_sites).post(admin_create_site))
        .route(
            "/admin/sites/:domain",
            get(admin_get_site).put(admin_update_site).delete(admin_delete_site),
        )
        .route("/admin/sites/:domain/queue", get(admin_list_queue))
        .route("/admin/sites/:domain/kick", axum::routing::post(admin_kick))
        .route("/admin/sites/:domain/admit", axum::routing::post(admin_admit))
        .route("/admin/sites/:domain/clear", axum::routing::post(admin_clear));

    let app = Router::new()
        .nest("/api", api)
        .route("/", get(root_redirect))
        .route("/loader/pscheckwait.js", get(serve_loader))
        .route("/loader/loader.js", get(serve_loader))
        .route("/admin", get(serve_admin_index))
        .route("/admin/", get(serve_admin_index))
        .route("/admin/*path", get(serve_admin_path))
        .route("/demo", get(serve_demo))
        .route("/demo/", get(serve_demo))
        .route("/wait", get(serve_wait))
        .route("/wait/", get(serve_wait))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let port: u16 = std::env::var("PSCHECKWAIT_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);
    let bind_host = std::env::var("PSCHECKWAIT_BIND").unwrap_or_else(|_| "0.0.0.0".into());
    let bind = format!("{bind_host}:{port}");
    let listener = tokio::net::TcpListener::bind(&bind).await.unwrap();
    let url = format!("http://localhost:{port}");
    tracing::info!("bound to {bind}");
    tracing::info!("pscheckwait server listening on {url}");

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        open_browser(&url);
    });

    axum::serve(listener, app).await.unwrap();
}
