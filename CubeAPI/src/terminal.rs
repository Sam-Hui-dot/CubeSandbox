// Copyright (c) 2026 Tencent Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{collections::HashMap, pin::Pin, sync::Arc, time::Duration as StdDuration};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use bytes::Bytes;
use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use futures::Stream;
use reqwest::Response;
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};

pub const ENVD_PORT: u16 = 49983;
const CONNECT_PROTOCOL_VERSION: &str = "1";
const CONNECT_CONTENT_TYPE: &str = "application/connect+json";
const CONNECT_END_STREAM_FLAG: u8 = 0x02;
const CONNECT_COMPRESSED_FLAG: u8 = 0x01;
const MAX_CONNECT_ENVELOPE_SIZE: usize = 64 * 1024 * 1024;
const DEFAULT_ENVD_USER: &str = "root";
const DEFAULT_TICKET_TTL_SECS: i64 = 60;
const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 30 * 60;

pub type PtyByteStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static>>;

#[derive(Debug, Clone)]
pub struct TerminalTicket {
    pub sandbox_id: String,
    pub domain: String,
    pub container_id: Option<String>,
    pub rows: u16,
    pub cols: u16,
    pub user: String,
    pub cwd: Option<String>,
    pub envs: HashMap<String, String>,
    pub created_by: Option<String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Default)]
pub struct TerminalTicketStore {
    tickets: Arc<DashMap<String, TerminalTicket>>,
}

impl TerminalTicketStore {
    pub fn issue(&self, ticket: TerminalTicket) -> String {
        self.prune_expired();
        let token = uuid::Uuid::new_v4().simple().to_string();
        self.tickets.insert(token.clone(), ticket);
        token
    }

    pub fn claim(&self, token: &str, sandbox_id: &str) -> AppResult<TerminalTicket> {
        let Some((_, ticket)) = self.tickets.remove(token) else {
            return Err(AppError::Unauthorized(
                "terminal ticket is invalid or already used".to_string(),
            ));
        };
        if ticket.sandbox_id != sandbox_id {
            return Err(AppError::Unauthorized(
                "terminal ticket does not match sandbox".to_string(),
            ));
        }
        if ticket.expires_at <= Utc::now() {
            return Err(AppError::Unauthorized(
                "terminal ticket has expired".to_string(),
            ));
        }
        Ok(ticket)
    }

    pub fn expires_at_from_now() -> DateTime<Utc> {
        Utc::now() + Duration::seconds(DEFAULT_TICKET_TTL_SECS)
    }

    fn prune_expired(&self) {
        let now = Utc::now();
        self.tickets.retain(|_, ticket| ticket.expires_at > now);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalCloseReason {
    ClientDisconnected,
    IdleTimeout,
    ProcessExited,
    StreamEnded,
    BackendError,
    SendFailed,
}

impl TerminalCloseReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            TerminalCloseReason::ClientDisconnected => "client_disconnected",
            TerminalCloseReason::IdleTimeout => "idle_timeout",
            TerminalCloseReason::ProcessExited => "process_exited",
            TerminalCloseReason::StreamEnded => "stream_ended",
            TerminalCloseReason::BackendError => "backend_error",
            TerminalCloseReason::SendFailed => "send_failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TerminalSession {
    pub session_id: String,
    pub sandbox_id: String,
    pub container_id: Option<String>,
    pub pid: i64,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub last_active_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub close_reason: Option<TerminalCloseReason>,
}

impl TerminalSession {
    pub fn duration_ms(&self) -> i64 {
        let end = self.closed_at.unwrap_or_else(Utc::now);
        (end - self.created_at).num_milliseconds().max(0)
    }
}

#[derive(Clone)]
pub struct TerminalSessionStore {
    sessions: Arc<DashMap<String, TerminalSession>>,
    idle_timeout: StdDuration,
}

impl Default for TerminalSessionStore {
    fn default() -> Self {
        Self::new(StdDuration::from_secs(DEFAULT_IDLE_TIMEOUT_SECS))
    }
}

impl TerminalSessionStore {
    pub fn from_env() -> Self {
        let secs = std::env::var("CUBE_API_TERMINAL_IDLE_TIMEOUT_SECS")
            .or_else(|_| std::env::var("TERMINAL_IDLE_TIMEOUT_SECS"))
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_IDLE_TIMEOUT_SECS);
        Self::new(StdDuration::from_secs(secs))
    }

    pub fn new(idle_timeout: StdDuration) -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            idle_timeout,
        }
    }

    pub fn open(
        &self,
        sandbox_id: &str,
        container_id: Option<String>,
        pid: i64,
        created_by: String,
    ) -> TerminalSession {
        let now = Utc::now();
        let session = TerminalSession {
            session_id: uuid::Uuid::new_v4().simple().to_string(),
            sandbox_id: sandbox_id.to_string(),
            container_id,
            pid,
            created_by,
            created_at: now,
            last_active_at: now,
            closed_at: None,
            close_reason: None,
        };
        self.sessions
            .insert(session.session_id.clone(), session.clone());
        session
    }

    pub fn touch(&self, session_id: &str) {
        if let Some(mut session) = self.sessions.get_mut(session_id) {
            session.last_active_at = Utc::now();
        }
    }

    pub fn close(&self, session_id: &str, reason: TerminalCloseReason) -> Option<TerminalSession> {
        let Some((_, mut session)) = self.sessions.remove(session_id) else {
            return None;
        };
        session.closed_at = Some(Utc::now());
        session.close_reason = Some(reason);
        Some(session)
    }

    pub fn idle_timeout(&self) -> StdDuration {
        self.idle_timeout
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }
}

#[derive(Debug)]
pub enum PtyEvent {
    Start {
        pid: i64,
    },
    Output {
        data: Vec<u8>,
    },
    End {
        exit_code: Option<i64>,
        error: Option<String>,
    },
    StreamEnd,
}

#[derive(Default)]
pub struct ConnectJsonParser {
    buffer: Vec<u8>,
}

impl ConnectJsonParser {
    pub fn push(&mut self, chunk: &[u8]) -> AppResult<Vec<PtyEvent>> {
        if chunk.is_empty() {
            return Ok(Vec::new());
        }

        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();

        while self.buffer.len() >= 5 {
            let flags = self.buffer[0];
            let size = u32::from_be_bytes([
                self.buffer[1],
                self.buffer[2],
                self.buffer[3],
                self.buffer[4],
            ]) as usize;

            if size > MAX_CONNECT_ENVELOPE_SIZE {
                return Err(AppError::Internal(anyhow::anyhow!(
                    "envd terminal stream message too large: {} bytes",
                    size
                )));
            }
            if self.buffer.len() < 5 + size {
                break;
            }

            let raw = self.buffer[5..5 + size].to_vec();
            self.buffer.drain(..5 + size);

            if flags & CONNECT_COMPRESSED_FLAG != 0 {
                return Err(AppError::Internal(anyhow::anyhow!(
                    "compressed envd terminal frames are not supported"
                )));
            }
            if flags & CONNECT_END_STREAM_FLAG != 0 {
                if !raw.is_empty() {
                    let value: Value = serde_json::from_slice(&raw).map_err(|e| {
                        AppError::Internal(anyhow::anyhow!(
                            "invalid envd terminal trailer JSON: {}",
                            e
                        ))
                    })?;
                    if let Some(error) = value.get("error") {
                        return Err(AppError::Internal(anyhow::anyhow!(
                            "envd terminal stream error: {}",
                            error
                        )));
                    }
                }
                events.push(PtyEvent::StreamEnd);
                continue;
            }

            let value: Value = serde_json::from_slice(&raw).map_err(|e| {
                AppError::Internal(anyhow::anyhow!("invalid envd terminal JSON event: {}", e))
            })?;
            if let Some(event) = value.get("event") {
                parse_process_event(event, &mut events)?;
            }
        }

        Ok(events)
    }
}

fn parse_process_event(event: &Value, events: &mut Vec<PtyEvent>) -> AppResult<()> {
    if let Some(start) = event.get("start") {
        let pid = start
            .get("pid")
            .and_then(Value::as_i64)
            .or_else(|| start.get("pid").and_then(Value::as_str)?.parse().ok())
            .ok_or_else(|| {
                AppError::Internal(anyhow::anyhow!("envd terminal start event missing pid"))
            })?;
        events.push(PtyEvent::Start { pid });
    }

    if let Some(data) = event.get("data") {
        if let Some(pty) = data.get("pty").and_then(Value::as_str) {
            let decoded = BASE64.decode(pty).map_err(|e| {
                AppError::Internal(anyhow::anyhow!(
                    "invalid envd terminal output base64: {}",
                    e
                ))
            })?;
            events.push(PtyEvent::Output { data: decoded });
        }
    }

    if let Some(end) = event.get("end") {
        let exit_code = end
            .get("exitCode")
            .and_then(Value::as_i64)
            .or_else(|| end.get("exit_code").and_then(Value::as_i64))
            .or_else(|| parse_exit_status(end.get("status").and_then(Value::as_str)));
        let error = end
            .get("error")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        events.push(PtyEvent::End { exit_code, error });
    }

    Ok(())
}

fn parse_exit_status(status: Option<&str>) -> Option<i64> {
    status?
        .strip_prefix("exit status ")
        .and_then(|v| v.trim().parse::<i64>().ok())
}

#[derive(Clone)]
pub struct EnvdPtyClient {
    http_client: reqwest::Client,
    proxy_base: String,
    host: String,
}

impl EnvdPtyClient {
    pub fn new(http_client: reqwest::Client, sandbox_id: &str, domain: &str) -> Self {
        let proxy_base = std::env::var("CUBE_API_SANDBOX_PROXY_URL")
            .or_else(|_| std::env::var("AGENTHUB_SANDBOX_PROXY_URL"))
            .unwrap_or_else(|_| "http://127.0.0.1".to_string());
        Self::with_proxy_base(http_client, sandbox_id, domain, &proxy_base)
    }

    pub fn with_proxy_base(
        http_client: reqwest::Client,
        sandbox_id: &str,
        domain: &str,
        proxy_base: &str,
    ) -> Self {
        Self {
            http_client,
            proxy_base: proxy_base.trim_end_matches('/').to_string(),
            host: format!("{}-{}.{}", ENVD_PORT, sandbox_id, domain),
        }
    }

    pub async fn start(
        &self,
        rows: u16,
        cols: u16,
        user: &str,
        cwd: Option<&str>,
        envs: &HashMap<String, String>,
    ) -> AppResult<Response> {
        let mut effective_envs = envs.clone();
        effective_envs
            .entry("TERM".to_string())
            .or_insert_with(|| "xterm-256color".to_string());
        effective_envs
            .entry("LANG".to_string())
            .or_insert_with(|| "C.UTF-8".to_string());
        effective_envs
            .entry("LC_ALL".to_string())
            .or_insert_with(|| "C.UTF-8".to_string());

        let mut process = json!({
            "cmd": "/bin/bash",
            "args": ["-i", "-l"],
            "envs": effective_envs,
        });
        if let Some(cwd) = cwd.filter(|v| !v.trim().is_empty()) {
            process["cwd"] = Value::String(cwd.to_string());
        }

        let payload = json!({
            "process": process,
            "pty": {
                "size": {
                    "rows": rows,
                    "cols": cols,
                }
            }
        });

        let body = connect_envelope(&serde_json::to_vec(&payload).map_err(anyhow::Error::from)?);
        let resp = self
            .http_client
            .post(self.url("Start"))
            .header("Host", &self.host)
            .header("Content-Type", CONNECT_CONTENT_TYPE)
            .header("Connect-Protocol-Version", CONNECT_PROTOCOL_VERSION)
            .header("Connect-Content-Encoding", "identity")
            .header("Authorization", basic_auth_user(user))
            .body(body)
            .send()
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("envd PTY start failed: {}", e)))?;

        if !resp.status().is_success() {
            return Err(AppError::Internal(anyhow::anyhow!(
                "envd PTY start returned HTTP {}",
                resp.status()
            )));
        }

        Ok(resp)
    }

    pub async fn send_input(&self, pid: i64, data: &[u8]) -> AppResult<()> {
        self.unary(
            "SendInput",
            json!({
                "process": { "pid": pid },
                "input": { "pty": BASE64.encode(data) },
            }),
            false,
        )
        .await
        .map(|_| ())
    }

    pub async fn resize(&self, pid: i64, rows: u16, cols: u16) -> AppResult<()> {
        self.unary(
            "Update",
            json!({
                "process": { "pid": pid },
                "pty": { "size": { "rows": rows, "cols": cols } },
            }),
            false,
        )
        .await
        .map(|_| ())
    }

    pub async fn kill(&self, pid: i64) -> AppResult<bool> {
        self.unary(
            "SendSignal",
            json!({
                "process": { "pid": pid },
                "signal": "SIGNAL_SIGKILL",
            }),
            true,
        )
        .await
        .map(|value| value.is_some())
    }

    async fn unary(
        &self,
        method: &str,
        payload: Value,
        allow_not_found: bool,
    ) -> AppResult<Option<Value>> {
        let resp = self
            .http_client
            .post(self.url(method))
            .header("Host", &self.host)
            .header("Content-Type", "application/json")
            .header("Connect-Protocol-Version", CONNECT_PROTOCOL_VERSION)
            .header("Authorization", basic_auth_user(DEFAULT_ENVD_USER))
            .body(serde_json::to_vec(&payload).map_err(anyhow::Error::from)?)
            .send()
            .await
            .map_err(|e| {
                AppError::Internal(anyhow::anyhow!("envd PTY {} failed: {}", method, e))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let detail = resp.text().await.unwrap_or_default();
            if allow_not_found && (status.as_u16() == 404 || detail.contains("\"not_found\"")) {
                return Ok(None);
            }
            return Err(AppError::Internal(anyhow::anyhow!(
                "envd PTY {} returned HTTP {}{}",
                method,
                status,
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(": {}", detail)
                }
            )));
        }

        let body = resp.bytes().await.map_err(|e| {
            AppError::Internal(anyhow::anyhow!(
                "failed reading envd PTY {} response: {}",
                method,
                e
            ))
        })?;
        if body.is_empty() {
            return Ok(Some(Value::Null));
        }
        serde_json::from_slice(&body).map(Some).map_err(|e| {
            AppError::Internal(anyhow::anyhow!("invalid envd PTY {} JSON: {}", method, e))
        })
    }

    fn url(&self, method: &str) -> String {
        format!("{}/process.Process/{}", self.proxy_base, method)
    }
}

pub fn connect_envelope(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 5);
    out.push(0);
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

pub fn basic_auth_user(user: &str) -> String {
    format!("Basic {}", BASE64.encode(format!("{}:", user)))
}

pub fn validated_size(rows: Option<u16>, cols: Option<u16>) -> (u16, u16) {
    let rows = rows.unwrap_or(24).clamp(5, 200);
    let cols = cols.unwrap_or(80).clamp(20, 400);
    (rows, cols)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Bytes as BodyBytes,
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::post,
        Router,
    };
    use tokio::{net::TcpListener, sync::mpsc};

    #[derive(Debug)]
    struct RecordedEnvdRequest {
        method: &'static str,
        host: Option<String>,
        authorization: Option<String>,
        body: Vec<u8>,
    }

    #[test]
    fn ticket_store_claims_once_and_rejects_reuse() {
        let store = TerminalTicketStore::default();
        let token = store.issue(TerminalTicket {
            sandbox_id: "sbx".to_string(),
            domain: "cube.test".to_string(),
            container_id: None,
            rows: 24,
            cols: 80,
            user: "root".to_string(),
            cwd: None,
            envs: HashMap::new(),
            created_by: None,
            expires_at: TerminalTicketStore::expires_at_from_now(),
        });

        assert!(store.claim(&token, "sbx").is_ok());
        assert!(store.claim(&token, "sbx").is_err());
    }

    #[test]
    fn ticket_store_rejects_wrong_sandbox_and_expired_ticket() {
        let store = TerminalTicketStore::default();
        let token = store.issue(TerminalTicket {
            sandbox_id: "sbx-a".to_string(),
            domain: "cube.test".to_string(),
            container_id: None,
            rows: 24,
            cols: 80,
            user: "root".to_string(),
            cwd: None,
            envs: HashMap::new(),
            created_by: Some("alice".to_string()),
            expires_at: TerminalTicketStore::expires_at_from_now(),
        });
        assert!(store.claim(&token, "sbx-b").is_err());
        assert!(store.claim(&token, "sbx-a").is_err());

        let expired = store.issue(TerminalTicket {
            sandbox_id: "sbx".to_string(),
            domain: "cube.test".to_string(),
            container_id: None,
            rows: 24,
            cols: 80,
            user: "root".to_string(),
            cwd: None,
            envs: HashMap::new(),
            created_by: None,
            expires_at: Utc::now() - Duration::seconds(1),
        });
        assert!(store.claim(&expired, "sbx").is_err());
    }

    #[test]
    fn validated_size_clamps_browser_dimensions() {
        assert_eq!(validated_size(None, None), (24, 80));
        assert_eq!(validated_size(Some(1), Some(1)), (5, 20));
        assert_eq!(validated_size(Some(999), Some(999)), (200, 400));
        assert_eq!(validated_size(Some(40), Some(132)), (40, 132));
    }

    #[test]
    fn session_store_tracks_activity_and_close_reason() {
        let store = TerminalSessionStore::new(StdDuration::from_secs(5));
        let session = store.open("sbx", Some("ctr-main".to_string()), 42, "alice".to_string());
        let created_at = session.created_at;

        assert_eq!(store.len(), 1);
        assert_eq!(session.sandbox_id, "sbx");
        assert_eq!(session.container_id.as_deref(), Some("ctr-main"));
        assert_eq!(session.pid, 42);
        assert_eq!(session.created_by, "alice");
        assert!(session.closed_at.is_none());

        store.touch(&session.session_id);
        let closed = store
            .close(&session.session_id, TerminalCloseReason::IdleTimeout)
            .expect("session should close once");

        assert_eq!(store.len(), 0);
        assert_eq!(closed.close_reason, Some(TerminalCloseReason::IdleTimeout));
        assert!(closed.closed_at.is_some());
        assert!(closed.last_active_at >= created_at);
        assert!(closed.duration_ms() >= 0);
        assert!(store
            .close(&session.session_id, TerminalCloseReason::ClientDisconnected)
            .is_none());
    }

    #[test]
    fn session_store_keeps_configured_idle_timeout() {
        let store = TerminalSessionStore::new(StdDuration::from_secs(17));
        assert_eq!(store.idle_timeout(), StdDuration::from_secs(17));
    }

    #[test]
    fn parser_extracts_start_output_and_end() {
        let payloads = [
            json!({"event":{"start":{"pid":42}}}),
            json!({"event":{"data":{"pty": BASE64.encode(b"hello")}}}),
            json!({"event":{"end":{"exitCode":0}}}),
        ];
        let mut bytes = Vec::new();
        for payload in payloads {
            bytes.extend(connect_envelope(
                serde_json::to_string(&payload).unwrap().as_bytes(),
            ));
        }

        let mut parser = ConnectJsonParser::default();
        let events = parser.push(&bytes).unwrap();
        assert!(matches!(events[0], PtyEvent::Start { pid: 42 }));
        assert!(matches!(&events[1], PtyEvent::Output { data } if data == b"hello"));
        assert!(matches!(
            events[2],
            PtyEvent::End {
                exit_code: Some(0),
                ..
            }
        ));
    }

    #[test]
    fn parser_waits_for_complete_connect_frames() {
        let payload = json!({"event":{"data":{"pty": BASE64.encode(b"partial")}}});
        let frame = connect_envelope(serde_json::to_string(&payload).unwrap().as_bytes());
        let split_at = frame.len() / 2;

        let mut parser = ConnectJsonParser::default();
        assert!(parser.push(&frame[..split_at]).unwrap().is_empty());

        let events = parser.push(&frame[split_at..]).unwrap();
        assert!(matches!(&events[0], PtyEvent::Output { data } if data == b"partial"));
    }

    #[test]
    fn parser_rejects_compressed_connect_frames() {
        let payload = json!({"event":{"start":{"pid":42}}});
        let mut frame = connect_envelope(serde_json::to_string(&payload).unwrap().as_bytes());
        frame[0] = CONNECT_COMPRESSED_FLAG;

        let mut parser = ConnectJsonParser::default();
        assert!(parser.push(&frame).is_err());
    }

    #[test]
    fn parser_surfaces_connect_trailer_errors() {
        let mut frame = connect_envelope(br#"{"error":"permission denied"}"#);
        frame[0] = CONNECT_END_STREAM_FLAG;

        let mut parser = ConnectJsonParser::default();
        let err = parser.push(&frame).unwrap_err().to_string();
        assert!(err.contains("permission denied"));
    }

    #[tokio::test]
    async fn envd_client_sends_start_input_resize_and_kill_requests() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let app = fake_envd_router(tx);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = EnvdPtyClient::with_proxy_base(
            reqwest::Client::new(),
            "sbx",
            "cube.test",
            &format!("http://{}", addr),
        );
        let mut envs = HashMap::new();
        envs.insert("CUSTOM".to_string(), "1".to_string());

        let response = client
            .start(33, 120, "ubuntu", Some("/workspace"), &envs)
            .await
            .unwrap();
        let start_events = ConnectJsonParser::default()
            .push(&response.bytes().await.unwrap())
            .unwrap();
        assert!(matches!(start_events[0], PtyEvent::Start { pid: 77 }));

        client.send_input(77, b"ls\n").await.unwrap();
        client.resize(77, 40, 132).await.unwrap();
        assert!(client.kill(77).await.unwrap());

        let mut records = Vec::new();
        for _ in 0..4 {
            records.push(rx.recv().await.unwrap());
        }

        let start = &records[0];
        let ubuntu_auth = basic_auth_user("ubuntu");
        assert_eq!(start.method, "Start");
        assert_eq!(start.host.as_deref(), Some("49983-sbx.cube.test"));
        assert_eq!(start.authorization.as_deref(), Some(ubuntu_auth.as_str()));
        let start_json = decode_connect_payload(&start.body);
        assert_eq!(start_json["process"]["cmd"], "/bin/bash");
        assert_eq!(start_json["process"]["cwd"], "/workspace");
        assert_eq!(start_json["process"]["envs"]["CUSTOM"], "1");
        assert_eq!(start_json["process"]["envs"]["TERM"], "xterm-256color");
        assert_eq!(start_json["pty"]["size"]["rows"], 33);
        assert_eq!(start_json["pty"]["size"]["cols"], 120);

        let input = decode_json_body(&records[1].body);
        let root_auth = basic_auth_user("root");
        assert_eq!(records[1].method, "SendInput");
        assert_eq!(
            records[1].authorization.as_deref(),
            Some(root_auth.as_str())
        );
        assert_eq!(input["process"]["pid"], 77);
        assert_eq!(input["input"]["pty"], BASE64.encode(b"ls\n"));

        let resize = decode_json_body(&records[2].body);
        assert_eq!(records[2].method, "Update");
        assert_eq!(resize["pty"]["size"]["rows"], 40);
        assert_eq!(resize["pty"]["size"]["cols"], 132);

        let kill = decode_json_body(&records[3].body);
        assert_eq!(records[3].method, "SendSignal");
        assert_eq!(kill["process"]["pid"], 77);
        assert_eq!(kill["signal"], "SIGNAL_SIGKILL");

        server.abort();
    }

    fn fake_envd_router(tx: mpsc::UnboundedSender<RecordedEnvdRequest>) -> Router {
        let start_tx = tx.clone();
        let input_tx = tx.clone();
        let update_tx = tx.clone();
        Router::new()
            .route(
                "/process.Process/Start",
                post(move |headers, body| {
                    record_envd_request("Start", start_tx.clone(), headers, body)
                }),
            )
            .route(
                "/process.Process/SendInput",
                post(move |headers, body| {
                    record_envd_request("SendInput", input_tx.clone(), headers, body)
                }),
            )
            .route(
                "/process.Process/Update",
                post(move |headers, body| {
                    record_envd_request("Update", update_tx.clone(), headers, body)
                }),
            )
            .route(
                "/process.Process/SendSignal",
                post(move |headers, body| {
                    record_envd_request("SendSignal", tx.clone(), headers, body)
                }),
            )
    }

    async fn record_envd_request(
        method: &'static str,
        tx: mpsc::UnboundedSender<RecordedEnvdRequest>,
        headers: HeaderMap,
        body: BodyBytes,
    ) -> impl IntoResponse {
        tx.send(RecordedEnvdRequest {
            method,
            host: headers
                .get("host")
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned),
            authorization: headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned),
            body: body.to_vec(),
        })
        .unwrap();

        if method == "Start" {
            let start = json!({"event":{"start":{"pid":77}}});
            return (
                StatusCode::OK,
                connect_envelope(serde_json::to_string(&start).unwrap().as_bytes()),
            )
                .into_response();
        }

        (StatusCode::OK, "{}").into_response()
    }

    fn decode_connect_payload(body: &[u8]) -> Value {
        assert_eq!(body[0], 0);
        let size = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;
        serde_json::from_slice(&body[5..5 + size]).unwrap()
    }

    fn decode_json_body(body: &[u8]) -> Value {
        serde_json::from_slice(body).unwrap()
    }
}
