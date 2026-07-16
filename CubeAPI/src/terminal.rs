// Copyright (c) 2026 Tencent Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{collections::HashMap, sync::Arc, time::Duration as StdDuration};

use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;

use crate::error::{AppError, AppResult};

const DEFAULT_TICKET_TTL_SECS: i64 = 60;
const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 30 * 60;

#[derive(Debug, Clone)]
pub struct TerminalTicket {
    pub sandbox_id: String,
    pub container_id: Option<String>,
    pub rows: u16,
    pub cols: u16,
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
    pub exec_id: String,
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
        exec_id: String,
        created_by: String,
    ) -> TerminalSession {
        let now = Utc::now();
        let session = TerminalSession {
            session_id: uuid::Uuid::new_v4().simple().to_string(),
            sandbox_id: sandbox_id.to_string(),
            container_id,
            exec_id,
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

pub fn validated_size(rows: Option<u16>, cols: Option<u16>) -> (u16, u16) {
    let rows = rows.unwrap_or(24).clamp(5, 200);
    let cols = cols.unwrap_or(80).clamp(20, 400);
    (rows, cols)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_store_claims_once_and_rejects_reuse() {
        let store = TerminalTicketStore::default();
        let token = store.issue(TerminalTicket {
            sandbox_id: "sbx".to_string(),
            container_id: None,
            rows: 24,
            cols: 80,
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
            container_id: None,
            rows: 24,
            cols: 80,
            cwd: None,
            envs: HashMap::new(),
            created_by: Some("alice".to_string()),
            expires_at: TerminalTicketStore::expires_at_from_now(),
        });
        assert!(store.claim(&token, "sbx-b").is_err());
        assert!(store.claim(&token, "sbx-a").is_err());

        let expired = store.issue(TerminalTicket {
            sandbox_id: "sbx".to_string(),
            container_id: None,
            rows: 24,
            cols: 80,
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
        let session = store.open(
            "sbx",
            Some("ctr-main".to_string()),
            "exec-42".to_string(),
            "alice".to_string(),
        );
        let created_at = session.created_at;

        assert_eq!(store.len(), 1);
        assert_eq!(session.sandbox_id, "sbx");
        assert_eq!(session.container_id.as_deref(), Some("ctr-main"));
        assert_eq!(session.exec_id, "exec-42");
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
}
