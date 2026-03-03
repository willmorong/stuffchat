use actix_web::{FromRequest, HttpRequest, dev::Payload, web::Data};
use chrono::{DateTime, Utc};
use futures_util::future::{Ready, err, ok};
use rand::TryRngCore;
use serde::Serialize;
use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::errors::ApiError;

const BRIDGE_SECRET_FILE: &str = "./bridge_secret";
const MAX_EVENTS: usize = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeEventType {
    CallJoined,
    CallLeft,
}

#[derive(Debug, Clone)]
pub struct BridgeEventRecord {
    pub seq: u64,
    pub event_type: BridgeEventType,
    pub channel_id: String,
    pub user_id: String,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct BridgeEventPage {
    pub reset_required: bool,
    pub oldest_available: Option<u64>,
    pub latest_available: u64,
    pub next_after: u64,
    pub events: Vec<BridgeEventRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeResolvedUser {
    pub id: String,
    pub username: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeResolvedChannel {
    pub id: String,
    pub name: Option<String>,
    pub is_voice: Option<bool>,
}

#[derive(Debug)]
struct BridgeState {
    next_seq: u64,
    events: VecDeque<BridgeEventRecord>,
}

#[derive(Clone)]
pub struct BridgeRuntime {
    secret: Arc<String>,
    state: Arc<Mutex<BridgeState>>,
}

impl BridgeRuntime {
    pub fn new(secret: String) -> Self {
        Self {
            secret: Arc::new(secret),
            state: Arc::new(Mutex::new(BridgeState {
                next_seq: 1,
                events: VecDeque::new(),
            })),
        }
    }

    pub fn push_event(
        &self,
        event_type: BridgeEventType,
        channel_id: impl Into<String>,
        user_id: impl Into<String>,
    ) -> u64 {
        let mut state = self.state.lock().expect("bridge state poisoned");
        let seq = state.next_seq;
        state.next_seq += 1;
        state.events.push_back(BridgeEventRecord {
            seq,
            event_type,
            channel_id: channel_id.into(),
            user_id: user_id.into(),
            occurred_at: Utc::now(),
        });
        while state.events.len() > MAX_EVENTS {
            state.events.pop_front();
        }
        seq
    }

    pub fn record_call_joined(&self, channel_id: impl Into<String>, user_id: impl Into<String>) {
        self.push_event(BridgeEventType::CallJoined, channel_id, user_id);
    }

    pub fn record_call_left(&self, channel_id: impl Into<String>, user_id: impl Into<String>) {
        self.push_event(BridgeEventType::CallLeft, channel_id, user_id);
    }

    pub fn status(&self) -> (Option<u64>, u64) {
        let state = self.state.lock().expect("bridge state poisoned");
        let oldest = state.events.front().map(|event| event.seq);
        let latest = state.events.back().map(|event| event.seq).unwrap_or(0);
        (oldest, latest)
    }

    pub fn page(&self, after: u64, limit: usize) -> BridgeEventPage {
        let state = self.state.lock().expect("bridge state poisoned");
        let oldest = state.events.front().map(|event| event.seq);
        let latest = state.events.back().map(|event| event.seq).unwrap_or(0);

        if let Some(oldest_seq) = oldest {
            if after.saturating_add(1) < oldest_seq {
                return BridgeEventPage {
                    reset_required: true,
                    oldest_available: oldest,
                    latest_available: latest,
                    next_after: latest,
                    events: Vec::new(),
                };
            }
        }

        let events = state
            .events
            .iter()
            .filter(|event| event.seq > after)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let next_after = events.last().map(|event| event.seq).unwrap_or(after);

        BridgeEventPage {
            reset_required: false,
            oldest_available: oldest,
            latest_available: latest,
            next_after,
            events,
        }
    }

    pub fn validate_secret(&self, candidate: &str) -> bool {
        constant_time_eq(self.secret.as_bytes(), candidate.as_bytes())
    }
}

pub fn bridge_secret_path() -> &'static Path {
    Path::new(BRIDGE_SECRET_FILE)
}

pub fn load_or_create_bridge_secret(path: &Path) -> std::io::Result<String> {
    match std::fs::File::open(path) {
        Ok(mut file) => {
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;
            let secret = contents.trim().to_string();
            if secret.is_empty() {
                return Err(std::io::Error::new(
                    ErrorKind::InvalidData,
                    "bridge secret file is empty",
                ));
            }
            Ok(secret)
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let mut bytes = [0u8; 32];
            rand::rngs::OsRng
                .try_fill_bytes(&mut bytes)
                .map_err(|rng_err| std::io::Error::other(rng_err.to_string()))?;
            let secret = encode_base64url_no_pad(&bytes);
            let tmp_path = path.with_extension("tmp");

            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&tmp_path)?;
            file.write_all(secret.as_bytes())?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            std::fs::rename(&tmp_path, path)?;

            Ok(secret)
        }
        Err(err) => Err(err),
    }
}

fn extract_bearer_token(req: &HttpRequest) -> Option<&str> {
    req.headers()
        .get("Authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let max_len = left.len().max(right.len());
    let mut diff = left.len() ^ right.len();

    for idx in 0..max_len {
        let left_byte = left.get(idx).copied().unwrap_or(0);
        let right_byte = right.get(idx).copied().unwrap_or(0);
        diff |= (left_byte ^ right_byte) as usize;
    }

    diff == 0
}

fn encode_base64url_no_pad(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::with_capacity((bytes.len() * 4).div_ceil(3));
    let mut index = 0;

    while index + 3 <= bytes.len() {
        let chunk = ((bytes[index] as u32) << 16)
            | ((bytes[index + 1] as u32) << 8)
            | (bytes[index + 2] as u32);
        output.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
        output.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
        output.push(TABLE[((chunk >> 6) & 0x3f) as usize] as char);
        output.push(TABLE[(chunk & 0x3f) as usize] as char);
        index += 3;
    }

    match bytes.len() - index {
        1 => {
            let chunk = (bytes[index] as u32) << 16;
            output.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
            output.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
        }
        2 => {
            let chunk = ((bytes[index] as u32) << 16) | ((bytes[index + 1] as u32) << 8);
            output.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
            output.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
            output.push(TABLE[((chunk >> 6) & 0x3f) as usize] as char);
        }
        _ => {}
    }

    output
}

#[derive(Debug, Clone)]
pub struct BridgeAuth;

impl FromRequest for BridgeAuth {
    type Error = ApiError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        let Some(runtime) = req.app_data::<Data<BridgeRuntime>>() else {
            return err(ApiError::Unauthorized);
        };
        let Some(token) = extract_bearer_token(req) else {
            return err(ApiError::Unauthorized);
        };

        if runtime.validate_secret(token) {
            ok(BridgeAuth)
        } else {
            err(ApiError::Unauthorized)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BridgeEventType, BridgeRuntime, constant_time_eq};

    #[test]
    fn queue_assigns_monotonic_sequence_ids() {
        let runtime = BridgeRuntime::new("secret".into());
        assert_eq!(
            runtime.push_event(BridgeEventType::CallJoined, "chan-1", "user-1"),
            1
        );
        assert_eq!(
            runtime.push_event(BridgeEventType::CallLeft, "chan-1", "user-1"),
            2
        );
    }

    #[test]
    fn queue_evicts_oldest_records_when_full() {
        let runtime = BridgeRuntime::new("secret".into());
        for idx in 0..5002 {
            runtime.push_event(BridgeEventType::CallJoined, format!("chan-{idx}"), "user-1");
        }

        let (oldest_available, latest_available) = runtime.status();
        let page = runtime.page(2, 5000);
        assert_eq!(page.events.len(), 5000);
        assert_eq!(oldest_available, Some(3));
        assert_eq!(latest_available, 5002);
    }

    #[test]
    fn pagination_returns_only_records_after_cursor() {
        let runtime = BridgeRuntime::new("secret".into());
        for idx in 0..3 {
            runtime.push_event(BridgeEventType::CallJoined, format!("chan-{idx}"), "user-1");
        }

        let page = runtime.page(1, 10);
        assert_eq!(page.events.len(), 2);
        assert_eq!(page.events[0].seq, 2);
        assert_eq!(page.events[1].seq, 3);
        assert_eq!(page.next_after, 3);
    }

    #[test]
    fn stale_cursor_requires_reset() {
        let runtime = BridgeRuntime::new("secret".into());
        for idx in 0..5002 {
            runtime.push_event(BridgeEventType::CallJoined, format!("chan-{idx}"), "user-1");
        }

        let page = runtime.page(1, 100);
        assert!(page.reset_required);
        assert_eq!(page.next_after, 5002);
        assert!(page.events.is_empty());
    }

    #[test]
    fn constant_time_token_compare_rejects_mismatch() {
        assert!(constant_time_eq(b"alpha", b"alpha"));
        assert!(!constant_time_eq(b"alpha", b"alphx"));
        assert!(!constant_time_eq(b"alpha", b"alpha-more"));
    }
}
