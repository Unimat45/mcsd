//! HTTP + WebSocket API under `/api`. Every route except `/api/health`
//! requires `Authorization: Bearer <token>` (or `?token=` for WebSockets,
//! since browsers can't set headers on them).

use crate::instance::{Instance, ServerInfo};
use crate::java::JavaRuntime;
use crate::logbuf::{LogLine, Stream};
use crate::manager::{CreateRequest, Manager, PropertiesEdit, PropertiesView, UpdateRequest};
use crate::registry::ServerKind;
use crate::web;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;

#[derive(Clone)]
pub struct AppState {
    pub mgr: Arc<Manager>,
    pub token: Arc<String>,
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/servers", get(list_servers).post(create_server))
        .route(
            "/servers/{name}",
            get(get_server).patch(update_server).delete(delete_server),
        )
        .route("/servers/{name}/start", post(start_server))
        .route("/servers/{name}/stop", post(stop_server))
        .route("/servers/{name}/restart", post(restart_server))
        .route("/servers/{name}/kill", post(kill_server))
        .route("/servers/{name}/reinstall", post(reinstall_server))
        .route("/servers/{name}/command", post(send_command))
        .route(
            "/servers/{name}/properties",
            get(get_properties).patch(patch_properties).put(put_properties),
        )
        .route("/servers/{name}/logs", get(get_logs))
        .route("/servers/{name}/console", get(console_ws))
        .route("/versions/{kind}", get(list_versions))
        .route("/java", get(list_java))
        .route("/java/install", post(install_java))
        .route("/system", get(system_info))
        .layer(middleware::from_fn_with_state(state.clone(), auth))
        // Registered after the layer: stays public.
        .route("/health", get(health))
        .fallback(api_not_found)
        .with_state(state);

    Router::new().nest("/api", api).fallback(web::static_handler)
}

// ------------------------------------------------------------------ errors

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(StatusCode::BAD_REQUEST, format!("{e:#}"))
    }
}

type ApiResult<T> = Result<T, ApiError>;

fn find(s: &AppState, name: &str) -> ApiResult<Arc<Instance>> {
    s.mgr
        .get(name)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("no server named '{name}'")))
}

async fn api_not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "no such endpoint".into())
}

// -------------------------------------------------------------------- auth

async fn auth(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let from_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string);
    let from_query = req
        .uri()
        .query()
        .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("token=")))
        .map(str::to_string);
    let ok = from_header
        .or(from_query)
        .is_some_and(|t| constant_time_eq(t.as_bytes(), s.token.as_bytes()));
    if !ok {
        return ApiError(StatusCode::UNAUTHORIZED, "missing or invalid token".into()).into_response();
    }
    next.run(req).await
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ---------------------------------------------------------------- handlers

async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") }))
}

async fn list_servers(State(s): State<AppState>) -> Json<Vec<ServerInfo>> {
    Json(s.mgr.list().iter().map(|i| i.info()).collect())
}

async fn create_server(State(s): State<AppState>, Json(req): Json<CreateRequest>) -> ApiResult<(StatusCode, Json<ServerInfo>)> {
    let inst = s.mgr.create(req).await?;
    Ok((StatusCode::CREATED, Json(inst.info())))
}

async fn get_server(State(s): State<AppState>, Path(name): Path<String>) -> ApiResult<Json<ServerInfo>> {
    Ok(Json(find(&s, &name)?.info()))
}

async fn update_server(
    State(s): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<UpdateRequest>,
) -> ApiResult<Json<ServerInfo>> {
    find(&s, &name)?;
    Ok(Json(s.mgr.update(&name, req).await?.info()))
}

#[derive(Deserialize)]
struct DeleteParams {
    #[serde(default)]
    delete_files: bool,
}

async fn delete_server(
    State(s): State<AppState>,
    Path(name): Path<String>,
    Query(p): Query<DeleteParams>,
) -> ApiResult<StatusCode> {
    find(&s, &name)?;
    s.mgr.remove(&name, p.delete_files).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct WaitParams {
    /// `false`: return immediately and let the operation run in the background
    /// (the dashboard polls status instead of holding a request open).
    wait: Option<bool>,
}

#[derive(Clone, Copy)]
enum Action {
    Start,
    Stop,
    Restart,
}

async fn lifecycle(s: AppState, name: String, p: WaitParams, action: Action) -> ApiResult<Json<ServerInfo>> {
    let inst = find(&s, &name)?;
    let run = {
        let inst = inst.clone();
        async move {
            match action {
                Action::Start => inst.start().await,
                Action::Stop => inst.stop().await,
                Action::Restart => inst.restart().await,
            }
        }
    };
    if p.wait.unwrap_or(true) {
        run.await?;
    } else {
        tokio::spawn(run);
        // Give fast transitions (e.g. -> starting) a moment to show up.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
    Ok(Json(inst.info()))
}

async fn start_server(State(s): State<AppState>, Path(name): Path<String>, Query(p): Query<WaitParams>) -> ApiResult<Json<ServerInfo>> {
    lifecycle(s, name, p, Action::Start).await
}
async fn stop_server(State(s): State<AppState>, Path(name): Path<String>, Query(p): Query<WaitParams>) -> ApiResult<Json<ServerInfo>> {
    lifecycle(s, name, p, Action::Stop).await
}
async fn restart_server(State(s): State<AppState>, Path(name): Path<String>, Query(p): Query<WaitParams>) -> ApiResult<Json<ServerInfo>> {
    lifecycle(s, name, p, Action::Restart).await
}

async fn kill_server(State(s): State<AppState>, Path(name): Path<String>) -> ApiResult<Json<ServerInfo>> {
    let inst = find(&s, &name)?;
    inst.kill().await?;
    Ok(Json(inst.info()))
}

async fn reinstall_server(State(s): State<AppState>, Path(name): Path<String>) -> ApiResult<Json<ServerInfo>> {
    let inst = find(&s, &name)?;
    let bg = inst.clone();
    tokio::spawn(async move { bg.install(None).await });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    Ok(Json(inst.info()))
}

#[derive(Deserialize)]
struct CommandBody {
    command: String,
}

async fn send_command(
    State(s): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<CommandBody>,
) -> ApiResult<StatusCode> {
    find(&s, &name)?.send_command(&body.command).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_properties(State(s): State<AppState>, Path(name): Path<String>) -> ApiResult<Json<PropertiesView>> {
    find(&s, &name)?;
    Ok(Json(s.mgr.properties(&name)?))
}

#[derive(Deserialize)]
struct PropertiesPatch {
    #[serde(default)]
    set: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    remove: Vec<String>,
}

/// Change some keys, preserving the rest of the file (comments, order).
async fn patch_properties(
    State(s): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<PropertiesPatch>,
) -> ApiResult<Json<PropertiesView>> {
    find(&s, &name)?;
    let edit = PropertiesEdit::Patch { set: body.set, remove: body.remove };
    Ok(Json(s.mgr.edit_properties(&name, edit).await?))
}

#[derive(Deserialize)]
struct PropertiesRaw {
    raw: String,
}

/// Replace the whole file.
async fn put_properties(
    State(s): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<PropertiesRaw>,
) -> ApiResult<Json<PropertiesView>> {
    find(&s, &name)?;
    if body.raw.len() > 256 * 1024 {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE, "server.properties is too large".into()));
    }
    Ok(Json(s.mgr.edit_properties(&name, PropertiesEdit::Raw(body.raw)).await?))
}

#[derive(Deserialize)]
struct LogParams {
    tail: Option<usize>,
    after: Option<u64>,
}

async fn get_logs(State(s): State<AppState>, Path(name): Path<String>, Query(p): Query<LogParams>) -> ApiResult<Json<Vec<LogLine>>> {
    let inst = find(&s, &name)?;
    Ok(Json(match p.after {
        Some(seq) => inst.logs_after(seq, 1000),
        None => inst.tail(p.tail.unwrap_or(200).min(2000)),
    }))
}

async fn console_ws(State(s): State<AppState>, Path(name): Path<String>, ws: WebSocketUpgrade) -> ApiResult<Response> {
    let inst = find(&s, &name)?;
    Ok(ws.on_upgrade(move |socket| console_session(socket, inst)))
}

/// Backlog first, then live lines; text frames from the client are commands.
async fn console_session(socket: WebSocket, inst: Arc<Instance>) {
    let (mut tx, mut rx) = socket.split();
    // Subscribe before snapshotting so no line falls between the two.
    let mut live = inst.subscribe();
    let mut last_seq = 0;
    for line in inst.tail(500) {
        last_seq = line.seq;
        if tx.send(to_msg(&line)).await.is_err() {
            return;
        }
    }
    loop {
        tokio::select! {
            msg = live.recv() => match msg {
                Ok(line) => {
                    if line.seq <= last_seq { continue; }
                    last_seq = line.seq;
                    if tx.send(to_msg(&line)).await.is_err() { break; }
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            },
            incoming = rx.next() => match incoming {
                Some(Ok(Message::Text(cmd))) => {
                    if let Err(e) = inst.send_command(cmd.as_str()).await {
                        let err = LogLine { seq: 0, ts: chrono::Utc::now().timestamp_millis(), stream: Stream::Sys, text: format!("error: {e:#}") };
                        if tx.send(to_msg(&err)).await.is_err() { break; }
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            }
        }
    }
}

fn to_msg(line: &LogLine) -> Message {
    Message::Text(serde_json::to_string(line).unwrap_or_default().into())
}

async fn list_versions(State(s): State<AppState>, Path(kind): Path<String>) -> ApiResult<Json<Vec<String>>> {
    let kind: ServerKind = kind.parse()?;
    Ok(Json(s.mgr.ctx.providers.versions(kind).await?))
}

#[derive(Deserialize)]
struct JavaListParams {
    #[serde(default)]
    refresh: bool,
}

async fn list_java(State(s): State<AppState>, Query(p): Query<JavaListParams>) -> Json<Vec<JavaRuntime>> {
    Json(s.mgr.ctx.java.list(p.refresh).await)
}

#[derive(Deserialize)]
struct JavaInstallBody {
    major: u32,
}

async fn install_java(State(s): State<AppState>, Json(body): Json<JavaInstallBody>) -> ApiResult<Json<JavaRuntime>> {
    let rt = s
        .mgr
        .ctx
        .java
        .install(body.major, &|m| tracing::info!("{m}"))
        .await?;
    Ok(Json(rt))
}

async fn system_info(State(s): State<AppState>) -> Json<Value> {
    let (total, available) = match sysinfo::System::new() {
        Ok(mut sys) => {
            sys.refresh_memory();
            (sys.total_memory(), sys.available_memory())
        }
        Err(_) => (0, 0),
    };
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "data_dir": s.mgr.ctx.paths.root.display().to_string(),
        "cpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        "total_memory_bytes": total,
        "available_memory_bytes": available,
    }))
}
