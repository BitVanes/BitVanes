//! `bitvanes daemon` — local HTTP daemon bound to **127.0.0.1 only** (zero-trust:
//! never reachable off-box). Provides `/health`, `/filter` (POST body →
//! sanitized text), `/scrub` (POST JSON → redacted text + categorized findings
//! for the dashboard), and optionally serves the built web dashboard either
//! from disk (`--dashboard-dir`) or compile-time-embedded via `rust-embed`
//! (the `dashboard` feature).
//!
//! Future hardening (not v1 blockers): multipart file upload, true streaming of
//! the HTTP request body (the current handlers buffer the body before
//! sanitizing), and request audit logging.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use clap::Args;
use tower_http::services::ServeDir;

use bitvanes_core::pii::Scrubber;
use bitvanes_core::sanitizer::{RedactionPolicy, sanitize_text};

use crate::entitlement::{EntitlementChecker, EntitlementStatus, resolve_checker};
use crate::shared::{ConfigArg, RulesArg, resolve_config};
/// `bitvanes daemon --port <P> [--config ...] [--dashboard-dir <DIR>]`.
#[derive(Args, Debug, Clone)]
pub struct DaemonArgs {
    /// Port to listen on (loopback only).
    #[arg(long, default_value = "8080")]
    pub port: u16,

    #[command(flatten)]
    pub rules: RulesArg,

    #[command(flatten)]
    pub config: ConfigArg,

    /// Directory of built web dashboard assets to serve at `/` (optional).
    #[arg(long, value_name = "DIR")]
    pub dashboard_dir: Option<std::path::PathBuf>,

    /// License key (overrides `BITVANES_LICENSE_KEY`). See `src/entitlement.rs`
    /// for the wire-format contract.
    #[arg(long, value_name = "KEY")]
    pub license_key: Option<String>,
}

/// Shared state for every request: the compiled scrubber + output policy +
/// resolved entitlement.
#[derive(Clone)]
struct DaemonState {
    scrubber: Arc<Scrubber>,
    policy: RedactionPolicy,
    entitlement: Arc<dyn EntitlementChecker>,
}

/// Entry point for `bitvanes daemon`.
pub fn run(args: DaemonArgs) -> Result<(), Box<dyn std::error::Error>> {
    let resolved = resolve_config(args.config.config.as_deref(), args.rules.rules.as_deref())?;
    let entitlement = resolve_checker(args.license_key.as_deref());
    let status = entitlement.status();
    eprintln!("BitVanes entitlement: {}", status.label());
    let state = DaemonState {
        scrubber: Arc::new(resolved.scrubber),
        policy: resolved.policy,
        entitlement,
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(serve(args.port, args.dashboard_dir.clone(), state))
}

async fn serve(
    port: u16,
    dashboard_dir: Option<std::path::PathBuf>,
    state: DaemonState,
) -> Result<(), Box<dyn std::error::Error>> {
    // Invariant: loopback only. Never bind 0.0.0.0.
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let mut router = Router::new()
        .route("/health", get(health))
        .route("/entitlement", get(entitlement))
        .route("/filter", post(filter))
        .route("/scrub", post(scrub))
        .with_state(state);
    #[cfg(feature = "dashboard")]
    if dashboard_dir.is_none() {
        router = router.fallback(get(embedded_asset));
    }
    if let Some(dir) = dashboard_dir {
        router = router.fallback_service(ServeDir::new(dir));
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("BitVanes daemon listening on http://{addr} (loopback only)");
    axum::serve(listener, router).await?;
    Ok(())
}

/// Compile-time-embedded dashboard assets (the built `web/dist`). Only present
/// when the `dashboard` feature is enabled AND `../web/dist` existed at build
/// time. Releases enable this so `bitvanes daemon` serves the dashboard with no
/// `--dashboard-dir`; dev builds use `--dashboard-dir ../web/dist`.
#[cfg(feature = "dashboard")]
mod embedded {
    use axum::{body::Body, extract::Request, http::StatusCode, response::Response};
    use rust_embed::RustEmbed;

    #[derive(RustEmbed)]
    #[folder = "../web/dist/"]
    struct DashboardAsset;

    /// Fallback handler: serves the embedded asset matching the request path,
    /// falling back to `index.html` for the SPA shell.
    pub async fn asset(req: Request) -> Response {
        let uri = req.uri().clone();
        let path = uri.path().trim_start_matches('/');
        let key = if path.is_empty() { "index.html" } else { path };
        match DashboardAsset::get(key) {
            Some(file) => file_response(key, file.data.into_owned()),
            None => match DashboardAsset::get("index.html") {
                Some(index) => file_response("index.html", index.data.into_owned()),
                None => Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("dashboard not built; pass --dashboard-dir"))
                    .unwrap(),
            },
        }
    }

    fn file_response(path: &str, bytes: Vec<u8>) -> Response {
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", mime_for(path))
            .body(Body::from(bytes))
            .unwrap()
    }

    fn mime_for(path: &str) -> &'static str {
        match path.rsplit('.').next() {
            Some("html") => "text/html; charset=utf-8",
            Some("js") => "application/javascript; charset=utf-8",
            Some("css") => "text/css; charset=utf-8",
            Some("json") => "application/json",
            Some("svg") => "image/svg+xml",
            Some("png") => "image/png",
            Some("ico") => "image/x-icon",
            _ => "application/octet-stream",
        }
    }
}

#[cfg(feature = "dashboard")]
use embedded::asset as embedded_asset;

async fn health() -> &'static str {
    "ok"
}

/// GET `/entitlement` — the resolved license/plan status. The daemon resolves
/// entitlement once at startup (no per-request network), so this is a cheap
/// cached read.
async fn entitlement(State(state): State<DaemonState>) -> impl IntoResponse {
    let s = state.entitlement.status();
    let (plan, seats) = match &s {
        EntitlementStatus::Licensed { plan, seats } => (plan.as_str(), Some(*seats)),
        _ => ("open-source", None),
    };
    Json(serde_json::json!({
        "status": s.label(),
        "plan": plan,
        "seats": seats,
        "allow_paid_features": s.allow_paid_features(),
    }))
}

/// POST `/filter` — sanitize the request body (treated as UTF-8 text) and
/// return the redacted text.
async fn filter(
    State(state): State<DaemonState>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, StatusCode> {
    let text = std::str::from_utf8(&body).map_err(|_| StatusCode::UNSUPPORTED_MEDIA_TYPE)?;
    let (_redacted, _map, findings) = state.scrubber.scrub(text);
    let out = sanitize_text(text, &findings, &state.policy).map_err(|_| {
        eprintln!("sanitize_text failed on /filter request");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok((StatusCode::OK, out))
}

/// JSON body for POST `/scrub`.
#[derive(serde::Deserialize)]
struct ScrubRequest {
    text: String,
}

/// JSON response for POST `/scrub` — the redacted text plus a categorized
/// finding breakdown so the dashboard can render a before/after diff.
#[derive(serde::Serialize)]
struct ScrubResponse {
    redacted: String,
    total: usize,
    categories: Vec<(String, usize)>,
}

/// POST `/scrub` — sanitize the provided text and return `{ redacted, total,
/// categories }`. This is the endpoint the local dashboard drives on drop.
async fn scrub(
    State(state): State<DaemonState>,
    Json(req): Json<ScrubRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    use std::collections::BTreeMap;
    let (_redacted, _map, findings) = state.scrubber.scrub(&req.text);
    let out = sanitize_text(&req.text, &findings, &state.policy)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut cats: BTreeMap<String, usize> = BTreeMap::new();
    for f in &findings {
        *cats.entry(f.entity.clone()).or_insert(0) += 1;
    }
    Ok(Json(ScrubResponse {
        redacted: out,
        total: findings.len(),
        categories: cats.into_iter().collect(),
    }))
}
