// Copyright (c) 2026 Tencent Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, VecDeque};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{
        header::{HOST, ORIGIN},
        HeaderMap, StatusCode,
    },
    response::IntoResponse,
    Json,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use futures::{stream::SplitSink, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use utoipa::ToSchema;

use crate::{
    error::{AppError, AppResult},
    logging::{LogEvent, LogLevel},
    models::{SandboxContainer, SandboxDetail, SandboxState},
    state::AppState,
    terminal::{
        validated_size, ConnectJsonParser, EnvdPtyClient, PtyByteStream, PtyEvent,
        TerminalCloseReason, TerminalTicket, TerminalTicketStore,
    },
};

const TERMINAL_WS_MAX_MESSAGE_SIZE: usize = 256 * 1024;
const TERMINAL_WS_MAX_FRAME_SIZE: usize = 256 * 1024;

#[derive(Debug, Deserialize, Default, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TerminalTicketRequest {
    #[serde(rename = "containerID", alias = "containerId", alias = "container_id")]
    pub container_id: Option<String>,
    pub rows: Option<u16>,
    pub cols: Option<u16>,
    pub cwd: Option<String>,
    pub envs: Option<HashMap<String, String>>,
    pub user: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TerminalTicketResponse {
    pub ticket: String,
    pub expires_at: String,
    pub websocket_url: String,
    #[serde(rename = "containerID", skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TerminalWsQuery {
    pub ticket: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TerminalClientMessage {
    #[serde(rename = "type")]
    kind: String,
    data: Option<String>,
    rows: Option<u16>,
    cols: Option<u16>,
}

#[derive(Debug, PartialEq, Eq)]
enum TerminalClientCommand {
    Input(Vec<u8>),
    Resize { rows: u16, cols: u16 },
    Kill,
    Ping,
}

#[utoipa::path(
    post,
    path = "/sandboxes/{sandboxID}/terminal/tickets",
    params(
        ("sandboxID" = String, Path, description = "Sandbox identifier")
    ),
    request_body = TerminalTicketRequest,
    responses(
        (status = 201, description = "Short-lived terminal WebSocket ticket", body = TerminalTicketResponse),
        (status = 400, description = "Invalid terminal request", body = crate::models::ApiError),
        (status = 401, description = "Unauthorized", body = crate::models::ApiError),
        (status = 404, description = "Sandbox or container not found", body = crate::models::ApiError),
        (status = 409, description = "Sandbox or container is not loggable", body = crate::models::ApiError),
        (status = 500, description = "Unexpected backend error", body = crate::models::ApiError)
    )
)]
pub async fn create_terminal_ticket(
    State(state): State<AppState>,
    Path(sandbox_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<TerminalTicketRequest>,
) -> AppResult<impl IntoResponse> {
    let created_by = validate_terminal_access(&state, &headers).await?;
    let detail = state.services.sandboxes.get_sandbox(&sandbox_id).await?;
    if detail.state != SandboxState::Running {
        return Err(AppError::Conflict(format!(
            "sandbox {} must be running before opening a terminal",
            sandbox_id
        )));
    }
    let container_id = select_terminal_container(&detail, body.container_id)?;

    let (rows, cols) = validated_size(body.rows, body.cols);
    let user = body
        .user
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "root".to_string());
    let expires_at = TerminalTicketStore::expires_at_from_now();
    let ticket = TerminalTicket {
        sandbox_id: sandbox_id.clone(),
        domain: detail
            .domain
            .unwrap_or_else(|| state.config.sandbox_domain.clone()),
        container_id: container_id.clone(),
        rows,
        cols,
        user,
        cwd: body.cwd.filter(|value| !value.trim().is_empty()),
        envs: body.envs.unwrap_or_default(),
        created_by,
        expires_at,
    };
    let token = state.terminal_tickets.issue(ticket);

    state
        .logger
        .log(
            LogEvent::new(LogLevel::Info, "terminal.ticket.issued")
                .field("sandbox_id", &sandbox_id)
                .field("container_id", container_id.as_deref().unwrap_or(""))
                .field_value("rows", rows)
                .field_value("cols", cols),
        )
        .await;

    Ok((
        StatusCode::CREATED,
        Json(TerminalTicketResponse {
            ticket: token.clone(),
            expires_at: expires_at.to_rfc3339(),
            websocket_url: format!(
                "/cubeapi/v1/sandboxes/{}/terminal/ws?ticket={}",
                sandbox_id, token
            ),
            container_id,
        }),
    ))
}

fn select_terminal_container(
    detail: &SandboxDetail,
    requested: Option<String>,
) -> AppResult<Option<String>> {
    let requested = requested
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let containers = detail.containers.as_deref().unwrap_or(&[]);

    if let Some(requested) = requested {
        let Some(container) = containers.iter().find(|container| {
            container.container_id == requested
                || container.name.as_deref() == Some(requested.as_str())
        }) else {
            return Err(AppError::NotFound(format!(
                "container {} was not found in sandbox {}",
                requested, detail.sandbox_id
            )));
        };
        ensure_terminal_container_running(detail, container)?;
        return Ok(Some(effective_container_id(container).to_string()));
    }

    if containers.is_empty() {
        return Ok(None);
    }

    let running: Vec<_> = containers
        .iter()
        .filter(|container| container.state == SandboxState::Running)
        .collect();

    match running.as_slice() {
        [] => Err(AppError::Conflict(format!(
            "sandbox {} has no running container available for terminal",
            detail.sandbox_id
        ))),
        [container] => Ok(Some(effective_container_id(container).to_string())),
        _ => Err(AppError::BadRequest(
            "containerID is required when a sandbox has multiple running containers".to_string(),
        )),
    }
}

fn ensure_terminal_container_running(
    detail: &SandboxDetail,
    container: &SandboxContainer,
) -> AppResult<()> {
    if container.state == SandboxState::Running {
        return Ok(());
    }
    Err(AppError::Conflict(format!(
        "container {} in sandbox {} is not running",
        effective_container_id(container),
        detail.sandbox_id
    )))
}

fn effective_container_id(container: &SandboxContainer) -> &str {
    if container.container_id.trim().is_empty() {
        container.name.as_deref().unwrap_or("")
    } else {
        container.container_id.as_str()
    }
}

#[utoipa::path(
    get,
    path = "/sandboxes/{sandboxID}/terminal/ws",
    params(
        ("sandboxID" = String, Path, description = "Sandbox identifier"),
        ("ticket" = String, Query, description = "Short-lived one-time terminal ticket")
    ),
    responses(
        (status = 101, description = "Terminal WebSocket upgrade"),
        (status = 401, description = "Unauthorized", body = crate::models::ApiError)
    )
)]
pub async fn terminal_websocket(
    State(state): State<AppState>,
    Path(sandbox_id): Path<String>,
    Query(query): Query<TerminalWsQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> AppResult<impl IntoResponse> {
    if !origin_allowed(&headers) {
        return Err(AppError::Unauthorized(
            "terminal websocket origin is not allowed".to_string(),
        ));
    }

    let ticket = state.terminal_tickets.claim(&query.ticket, &sandbox_id)?;
    Ok(ws
        .max_message_size(TERMINAL_WS_MAX_MESSAGE_SIZE)
        .max_frame_size(TERMINAL_WS_MAX_FRAME_SIZE)
        .on_upgrade(move |socket| handle_terminal_socket(state, ticket, socket)))
}

async fn validate_terminal_access(
    state: &AppState,
    headers: &HeaderMap,
) -> AppResult<Option<String>> {
    let auth_configured = state
        .config
        .auth_callback_url
        .as_deref()
        .is_some_and(|url| !url.trim().is_empty());
    let has_api_credential = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim().to_ascii_lowercase().starts_with("bearer "))
        || headers.get("x-api-key").is_some();

    if auth_configured && has_api_credential {
        return Ok(Some("api-credential".to_string()));
    }

    let Some(store) = &state.agenthub_store else {
        return Ok(None);
    };

    let token = headers
        .get("x-session-token")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| AppError::Unauthorized("web session is required".to_string()))?;

    let username = store
        .validate_session(token)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("failed to validate session: {}", e)))?
        .ok_or_else(|| AppError::Unauthorized("web session is invalid or expired".to_string()))?;

    Ok(Some(username))
}

fn origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(ORIGIN).and_then(|v| v.to_str().ok()) else {
        return headers
            .get(HOST)
            .and_then(|v| v.to_str().ok())
            .is_some_and(is_loopback_host);
    };
    let Some(host) = headers.get(HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };

    let origin = origin.trim().to_ascii_lowercase();
    let host = host.trim().to_ascii_lowercase();
    origin == format!("http://{}", host)
        || origin == format!("https://{}", host)
        || (is_loopback_origin(&origin) && is_loopback_host(&host))
}

fn is_loopback_origin(value: &str) -> bool {
    let Some(authority) = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
    else {
        return false;
    };
    let authority = authority.split('/').next().unwrap_or_default();
    is_loopback_host(authority)
}

fn is_loopback_host(value: &str) -> bool {
    let host = value.trim();
    if host.is_empty() {
        return false;
    }
    let host = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or_default()
    } else {
        host.split(':').next().unwrap_or_default()
    };
    matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1"
    )
}

async fn handle_terminal_socket(state: AppState, ticket: TerminalTicket, socket: WebSocket) {
    let sandbox_id = ticket.sandbox_id.clone();
    let container_id = ticket.container_id.clone();
    let created_by = ticket
        .created_by
        .clone()
        .unwrap_or_else(|| "anonymous".to_string());
    let client = EnvdPtyClient::new(
        state.http_client.clone(),
        &ticket.sandbox_id,
        &ticket.domain,
    );

    let response = match client
        .start(
            ticket.rows,
            ticket.cols,
            &ticket.user,
            ticket.cwd.as_deref(),
            &ticket.envs,
        )
        .await
    {
        Ok(response) => response,
        Err(error) => {
            log_terminal_open_failed(
                &state,
                &sandbox_id,
                container_id.as_deref(),
                &created_by,
                "envd_start_failed",
                &error.to_string(),
            )
            .await;
            let (mut sender, _) = socket.split();
            let _ = send_json(
                &mut sender,
                json!({ "type": "error", "message": error.to_string() }),
            )
            .await;
            return;
        }
    };

    let mut upstream = Box::pin(response.bytes_stream()) as PtyByteStream;
    let mut parser = ConnectJsonParser::default();
    let mut pending = VecDeque::new();
    let pid = match wait_for_start(&mut upstream, &mut parser, &mut pending).await {
        Ok(pid) => pid,
        Err(error) => {
            log_terminal_open_failed(
                &state,
                &sandbox_id,
                container_id.as_deref(),
                &created_by,
                "start_event_failed",
                &error.to_string(),
            )
            .await;
            let (mut sender, _) = socket.split();
            let _ = send_json(
                &mut sender,
                json!({ "type": "error", "message": error.to_string() }),
            )
            .await;
            return;
        }
    };

    let session =
        state
            .terminal_sessions
            .open(&sandbox_id, container_id.clone(), pid, created_by.clone());
    let session_id = session.session_id.clone();

    state
        .logger
        .log(
            LogEvent::new(LogLevel::Info, "terminal.opened")
                .field("sandbox_id", &sandbox_id)
                .field("container_id", container_id.as_deref().unwrap_or(""))
                .field("session_id", &session_id)
                .field("created_by", created_by.clone())
                .field_value("pid", pid),
        )
        .await;

    let (mut sender, mut receiver) = socket.split();
    if !send_json(
        &mut sender,
        json!({ "type": "start", "pid": pid, "sessionId": session_id }),
    )
    .await
    {
        close_terminal_session(
            &state,
            &client,
            &session_id,
            &sandbox_id,
            pid,
            false,
            TerminalCloseReason::SendFailed,
        )
        .await;
        return;
    }

    let mut process_ended = false;
    let mut close_reason = TerminalCloseReason::ClientDisconnected;
    let idle_timeout = state.terminal_sessions.idle_timeout();
    let mut idle_sleep = Box::pin(tokio::time::sleep(idle_timeout));

    while let Some(event) = pending.pop_front() {
        if !send_pty_event(&mut sender, event).await {
            close_terminal_session(
                &state,
                &client,
                &session_id,
                &sandbox_id,
                pid,
                process_ended,
                TerminalCloseReason::SendFailed,
            )
            .await;
            return;
        }
        refresh_terminal_activity(&state, &session_id, &mut idle_sleep, idle_timeout);
    }

    loop {
        tokio::select! {
            _ = &mut idle_sleep => {
                close_reason = TerminalCloseReason::IdleTimeout;
                let _ = send_json(
                    &mut sender,
                    json!({
                        "type": "idleTimeout",
                        "message": "terminal session closed after idle timeout",
                        "idleTimeoutSeconds": idle_timeout.as_secs(),
                    }),
                )
                .await;
                break;
            }
            chunk = upstream.next() => {
                match chunk {
                    Some(Ok(bytes)) => match parser.push(&bytes) {
                        Ok(events) => {
                            let mut send_failed = false;
                            for event in events {
                                if matches!(event, PtyEvent::End { .. }) {
                                    process_ended = true;
                                    close_reason = TerminalCloseReason::ProcessExited;
                                } else if matches!(event, PtyEvent::StreamEnd) {
                                    process_ended = true;
                                    close_reason = TerminalCloseReason::StreamEnded;
                                }
                                if !send_pty_event(&mut sender, event).await {
                                    close_reason = TerminalCloseReason::SendFailed;
                                    send_failed = true;
                                    break;
                                }
                                refresh_terminal_activity(&state, &session_id, &mut idle_sleep, idle_timeout);
                            }
                            if send_failed || process_ended {
                                break;
                            }
                        }
                        Err(error) => {
                            close_reason = TerminalCloseReason::BackendError;
                            let _ = send_json(&mut sender, json!({ "type": "error", "message": error.to_string() })).await;
                            break;
                        }
                    },
                    Some(Err(error)) => {
                        close_reason = TerminalCloseReason::BackendError;
                        let _ = send_json(&mut sender, json!({ "type": "error", "message": format!("terminal stream failed: {}", error) })).await;
                        break;
                    }
                    None => {
                        close_reason = TerminalCloseReason::StreamEnded;
                        process_ended = true;
                        break;
                    },
                }
            }
            message = receiver.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        refresh_terminal_activity(&state, &session_id, &mut idle_sleep, idle_timeout);
                        if let Err(error) = handle_client_text(&client, pid, &text).await {
                            let _ = send_json(&mut sender, json!({ "type": "error", "message": error.to_string() })).await;
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        refresh_terminal_activity(&state, &session_id, &mut idle_sleep, idle_timeout);
                        if let Err(error) = client.send_input(pid, &data).await {
                            let _ = send_json(&mut sender, json!({ "type": "error", "message": error.to_string() })).await;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        close_reason = TerminalCloseReason::ClientDisconnected;
                        break;
                    }
                    Some(Ok(Message::Ping(data))) => {
                        refresh_terminal_activity(&state, &session_id, &mut idle_sleep, idle_timeout);
                        let _ = sender.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Pong(_))) => {
                        refresh_terminal_activity(&state, &session_id, &mut idle_sleep, idle_timeout);
                    }
                    Some(Err(error)) => {
                        close_reason = TerminalCloseReason::ClientDisconnected;
                        tracing::warn!(sandbox_id = %sandbox_id, error = %error, "terminal websocket receive error");
                        break;
                    }
                }
            }
        }
    }

    close_terminal_session(
        &state,
        &client,
        &session_id,
        &sandbox_id,
        pid,
        process_ended,
        close_reason,
    )
    .await;
}

async fn log_terminal_open_failed(
    state: &AppState,
    sandbox_id: &str,
    container_id: Option<&str>,
    created_by: &str,
    stage: &str,
    error: &str,
) {
    state
        .logger
        .log(
            LogEvent::new(LogLevel::Warn, "terminal.open_failed")
                .field("sandbox_id", sandbox_id)
                .field("container_id", container_id.unwrap_or(""))
                .field("created_by", created_by)
                .field("stage", stage)
                .field("error", error),
        )
        .await;
}

fn refresh_terminal_activity(
    state: &AppState,
    session_id: &str,
    idle_sleep: &mut std::pin::Pin<Box<tokio::time::Sleep>>,
    idle_timeout: std::time::Duration,
) {
    state.terminal_sessions.touch(session_id);
    idle_sleep
        .as_mut()
        .reset(tokio::time::Instant::now() + idle_timeout);
}

async fn close_terminal_session(
    state: &AppState,
    client: &EnvdPtyClient,
    session_id: &str,
    sandbox_id: &str,
    pid: i64,
    process_ended: bool,
    reason: TerminalCloseReason,
) {
    if !process_ended {
        let _ = client.kill(pid).await;
    }
    let closed = state.terminal_sessions.close(session_id, reason.clone());
    let mut event = LogEvent::new(LogLevel::Info, "terminal.closed")
        .field("sandbox_id", sandbox_id)
        .field("session_id", session_id)
        .field("close_reason", reason.as_str())
        .field_value("pid", pid)
        .field_value("process_ended", process_ended);

    if let Some(session) = closed {
        let duration_ms = session.duration_ms();
        let last_active_at = session.last_active_at.to_rfc3339();
        event = event
            .field("created_by", session.created_by)
            .field(
                "container_id",
                session.container_id.as_deref().unwrap_or(""),
            )
            .field_value("duration_ms", duration_ms)
            .field_value("last_active_at", last_active_at);
    }

    state.logger.log(event).await;
}

async fn wait_for_start(
    upstream: &mut PtyByteStream,
    parser: &mut ConnectJsonParser,
    pending: &mut VecDeque<PtyEvent>,
) -> AppResult<i64> {
    while let Some(chunk) = upstream.next().await {
        let bytes = chunk.map_err(|e| {
            AppError::Internal(anyhow::anyhow!("failed reading envd terminal start: {}", e))
        })?;
        for event in parser.push(&bytes)? {
            match event {
                PtyEvent::Start { pid } => return Ok(pid),
                other => pending.push_back(other),
            }
        }
    }
    Err(AppError::Internal(anyhow::anyhow!(
        "envd terminal stream closed before start event"
    )))
}

async fn handle_client_text(client: &EnvdPtyClient, pid: i64, text: &str) -> AppResult<()> {
    match parse_terminal_client_message(text)? {
        TerminalClientCommand::Input(data) => client.send_input(pid, &data).await,
        TerminalClientCommand::Resize { rows, cols } => client.resize(pid, rows, cols).await,
        TerminalClientCommand::Kill => client.kill(pid).await.map(|_| ()),
        TerminalClientCommand::Ping => Ok(()),
    }
}

fn parse_terminal_client_message(text: &str) -> AppResult<TerminalClientCommand> {
    let msg: TerminalClientMessage = serde_json::from_str(text)
        .map_err(|e| AppError::BadRequest(format!("invalid terminal message: {}", e)))?;
    match msg.kind.as_str() {
        "input" | "stdin" => {
            let data = msg.data.unwrap_or_default();
            Ok(TerminalClientCommand::Input(data.into_bytes()))
        }
        "inputBase64" | "stdinBase64" => {
            let data = BASE64.decode(msg.data.unwrap_or_default()).map_err(|e| {
                AppError::BadRequest(format!("invalid terminal input base64: {}", e))
            })?;
            Ok(TerminalClientCommand::Input(data))
        }
        "resize" => {
            let (rows, cols) = validated_size(msg.rows, msg.cols);
            Ok(TerminalClientCommand::Resize { rows, cols })
        }
        "kill" => Ok(TerminalClientCommand::Kill),
        "ping" => Ok(TerminalClientCommand::Ping),
        other => Err(AppError::BadRequest(format!(
            "unsupported terminal message type: {}",
            other
        ))),
    }
}

async fn send_pty_event(sender: &mut SplitSink<WebSocket, Message>, event: PtyEvent) -> bool {
    match event {
        PtyEvent::Start { pid } => send_json(sender, json!({ "type": "start", "pid": pid })).await,
        PtyEvent::Output { data } => {
            send_json(
                sender,
                json!({ "type": "output", "data": BASE64.encode(data) }),
            )
            .await
        }
        PtyEvent::End { exit_code, error } => {
            send_json(
                sender,
                json!({ "type": "exit", "exitCode": exit_code, "error": error }),
            )
            .await
        }
        PtyEvent::StreamEnd => send_json(sender, json!({ "type": "streamEnd" })).await,
    }
}

async fn send_json(sender: &mut SplitSink<WebSocket, Message>, value: Value) -> bool {
    sender.send(Message::Text(value.to_string())).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::collections::HashMap;

    fn detail_with_containers(containers: Vec<SandboxContainer>) -> SandboxDetail {
        SandboxDetail {
            template_id: "tpl".to_string(),
            alias: None,
            sandbox_id: "sbx".to_string(),
            client_id: "host".to_string(),
            started_at: Utc::now(),
            end_at: Utc::now(),
            envd_version: "unknown".to_string(),
            envd_access_token: None,
            domain: Some("cube.test".to_string()),
            cpu_count: 1,
            memory_mb: 512,
            disk_size_mb: Some(0),
            metadata: Some(HashMap::new()),
            state: SandboxState::Running,
            volume_mounts: None,
            containers: Some(containers),
        }
    }

    fn container(id: &str, state: SandboxState, kind: Option<&str>) -> SandboxContainer {
        SandboxContainer {
            container_id: id.to_string(),
            name: None,
            state,
            image: None,
            kind: kind.map(ToOwned::to_owned),
            started_at: None,
        }
    }

    #[test]
    fn origin_allows_same_host_and_loopback_dev() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "127.0.0.1:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://localhost:5173".parse().unwrap());
        assert!(origin_allowed(&headers));

        headers.insert(ORIGIN, "http://localhost.evil.example".parse().unwrap());
        assert!(!origin_allowed(&headers));

        headers.insert(HOST, "cube.example.com".parse().unwrap());
        headers.insert(ORIGIN, "https://evil.example.com".parse().unwrap());
        assert!(!origin_allowed(&headers));
    }

    #[test]
    fn origin_rejects_loopback_lookalikes() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "127.0.0.1:3000".parse().unwrap());

        headers.insert(ORIGIN, "http://localhost.evil.example".parse().unwrap());
        assert!(!origin_allowed(&headers));

        headers.insert(ORIGIN, "http://127.0.0.1.evil.example".parse().unwrap());
        assert!(!origin_allowed(&headers));
    }

    #[test]
    fn origin_without_header_is_only_allowed_for_loopback() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "cube.example.com".parse().unwrap());
        assert!(!origin_allowed(&headers));

        headers.insert(HOST, "127.0.0.1:3000".parse().unwrap());
        assert!(origin_allowed(&headers));
    }

    #[test]
    fn origin_rejects_cross_site_when_host_is_missing() {
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, "https://evil.example.com".parse().unwrap());
        assert!(!origin_allowed(&headers));
    }

    #[test]
    fn parses_terminal_client_messages_without_network_side_effects() {
        assert_eq!(
            parse_terminal_client_message(r#"{"type":"input","data":"ls\n"}"#).unwrap(),
            TerminalClientCommand::Input(b"ls\n".to_vec())
        );
        assert_eq!(
            parse_terminal_client_message(r#"{"type":"stdinBase64","data":"Y2QKLg=="}"#).unwrap(),
            TerminalClientCommand::Input(b"cd\n.".to_vec())
        );
        assert_eq!(
            parse_terminal_client_message(r#"{"type":"resize","rows":1,"cols":999}"#).unwrap(),
            TerminalClientCommand::Resize { rows: 5, cols: 400 }
        );
        assert_eq!(
            parse_terminal_client_message(r#"{"type":"kill"}"#).unwrap(),
            TerminalClientCommand::Kill
        );
        assert!(parse_terminal_client_message(r#"{"type":"teleport"}"#).is_err());
        assert!(parse_terminal_client_message(r#"{"type":"stdinBase64","data":"!!!"}"#).is_err());
    }

    #[test]
    fn terminal_container_selection_defaults_single_running_container() {
        let detail = detail_with_containers(vec![container(
            "ctr-main",
            SandboxState::Running,
            Some("sandbox"),
        )]);

        assert_eq!(
            select_terminal_container(&detail, None).unwrap().as_deref(),
            Some("ctr-main")
        );
    }

    #[test]
    fn terminal_container_selection_requires_container_for_multiple_running() {
        let detail = detail_with_containers(vec![
            container("ctr-a", SandboxState::Running, Some("sidecar")),
            container("ctr-b", SandboxState::Running, Some("sandbox")),
        ]);

        assert!(matches!(
            select_terminal_container(&detail, None),
            Err(AppError::BadRequest(_))
        ));
        assert_eq!(
            select_terminal_container(&detail, Some("ctr-b".to_string()))
                .unwrap()
                .as_deref(),
            Some("ctr-b")
        );
    }

    #[test]
    fn terminal_container_selection_accepts_container_name_alias() {
        let mut named = container("ctr-main", SandboxState::Running, Some("sandbox"));
        named.name = Some("main".to_string());
        let detail = detail_with_containers(vec![named]);

        assert_eq!(
            select_terminal_container(&detail, Some("main".to_string()))
                .unwrap()
                .as_deref(),
            Some("ctr-main")
        );
    }

    #[test]
    fn terminal_container_selection_rejects_missing_or_stopped_container() {
        let detail =
            detail_with_containers(vec![container("ctr-paused", SandboxState::Paused, None)]);

        assert!(matches!(
            select_terminal_container(&detail, Some("missing".to_string())),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            select_terminal_container(&detail, Some("ctr-paused".to_string())),
            Err(AppError::Conflict(_))
        ));
    }
}
