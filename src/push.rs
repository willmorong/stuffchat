use crate::config::Config;
use crate::db::Db;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use hmac::{Hmac, Mac};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::sync::Arc;
use tokio::sync::Notify;
use tokio::time::{Duration, sleep};

type HmacSha256 = Hmac<Sha256>;

const PUSH_PROVIDER: &str = "relay";
const DEFAULT_POLL_INTERVAL_SECS: u64 = 5;
const MAX_DISPATCH_BATCH: i64 = 25;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PushPlatform {
    Ios,
    Android,
}

impl PushPlatform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ios => "ios",
            Self::Android => "android",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PushEnvironment {
    Development,
    Production,
}

impl PushEnvironment {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Production => "production",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PushEventType {
    Message,
    CallStarted,
}

impl PushEventType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::CallStarted => "call_started",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushPlatformAvailability {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushCapabilitiesResponse {
    pub enabled: bool,
    pub provider: String,
    pub message_notifications: bool,
    pub call_notifications: bool,
    pub ios: PushPlatformAvailability,
    pub android: PushPlatformAvailability,
}

impl PushCapabilitiesResponse {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            provider: PUSH_PROVIDER.to_string(),
            message_notifications: false,
            call_notifications: false,
            ios: PushPlatformAvailability { enabled: false },
            android: PushPlatformAvailability { enabled: false },
        }
    }

    pub fn relay_enabled() -> Self {
        Self {
            enabled: true,
            provider: PUSH_PROVIDER.to_string(),
            message_notifications: true,
            call_notifications: true,
            ios: PushPlatformAvailability { enabled: true },
            android: PushPlatformAvailability { enabled: false },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushDeviceRegistrationRequest {
    pub installation_id: String,
    pub platform: PushPlatform,
    pub push_token: String,
    pub environment: PushEnvironment,
    pub message_notifications: bool,
    pub call_notifications: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushDeviceDeleteRequest {
    pub installation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushChannelInfo {
    pub id: String,
    pub name: String,
    pub is_voice: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushActorInfo {
    pub id: String,
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushMessageInfo {
    pub id: String,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedPushPayload {
    pub occurred_at: DateTime<Utc>,
    pub channel: PushChannelInfo,
    pub actor: PushActorInfo,
    pub message: Option<PushMessageInfo>,
    pub recipient_user_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayPushDevice {
    pub installation_id: String,
    pub push_token: String,
    pub environment: PushEnvironment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayPushBatchRequest {
    pub request_id: String,
    pub event_id: String,
    pub event_type: PushEventType,
    pub occurred_at: DateTime<Utc>,
    pub channel: PushChannelInfo,
    pub actor: PushActorInfo,
    pub message: Option<PushMessageInfo>,
    pub devices: Vec<RelayPushDevice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayPushBatchResponse {
    pub accepted: bool,
    pub invalid_installation_ids: Vec<String>,
    pub retryable_failures: usize,
}

#[derive(Clone)]
pub struct PushRelayRuntime {
    inner: Arc<PushRelayRuntimeInner>,
}

struct PushRelayRuntimeInner {
    db: Db,
    relay_url: Url,
    relay_server_id: String,
    relay_server_secret: String,
    client: reqwest::Client,
    notify: Notify,
}

#[derive(Debug)]
struct PendingPushEvent {
    id: String,
    event_id: String,
    event_type: PushEventType,
    payload: QueuedPushPayload,
    attempt_count: i64,
}

impl PushRelayRuntime {
    pub fn new(cfg: &Config, db: Db) -> Result<Self, String> {
        let relay_url_raw = cfg
            .push_relay_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "push_relay_url is required".to_string())?;
        let relay_server_id = cfg
            .push_relay_server_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "push_relay_server_id is required".to_string())?;
        let relay_server_secret = cfg
            .push_relay_server_secret
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "push_relay_server_secret is required".to_string())?;

        let relay_url = Url::parse(relay_url_raw).map_err(|err| err.to_string())?;
        if !is_allowed_relay_url(&relay_url) {
            return Err("push_relay_url must use https unless it targets localhost".to_string());
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(cfg.push_relay_timeout_secs.max(1)))
            .build()
            .map_err(|err| err.to_string())?;

        let runtime = Self {
            inner: Arc::new(PushRelayRuntimeInner {
                db,
                relay_url,
                relay_server_id: relay_server_id.to_string(),
                relay_server_secret: relay_server_secret.to_string(),
                client,
                notify: Notify::new(),
            }),
        };
        runtime.spawn_worker();
        Ok(runtime)
    }

    pub async fn enqueue_message_event(
        &self,
        channel: PushChannelInfo,
        actor: PushActorInfo,
        message: PushMessageInfo,
        recipient_user_ids: Vec<String>,
        occurred_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        if recipient_user_ids.is_empty() {
            return Ok(());
        }

        let event_id = format!("msg:{}", message.id);
        let payload = QueuedPushPayload {
            occurred_at,
            channel,
            actor,
            message: Some(message),
            recipient_user_ids,
        };

        self.insert_event(PushEventType::Message, event_id, payload).await?;
        self.inner.notify.notify_one();
        Ok(())
    }

    pub fn record_call_started(
        &self,
        channel_id: String,
        actor_user_id: String,
        suppressed_user_ids: Vec<String>,
    ) {
        let runtime = self.clone();
        tokio::spawn(async move {
            if let Err(err) = runtime
                .enqueue_call_started(channel_id, actor_user_id, suppressed_user_ids)
                .await
            {
                log::error!("failed to enqueue call_started push: {err}");
            }
        });
    }

    pub async fn dispatch_pending_once(&self) -> Result<usize, String> {
        self.inner.dispatch_pending_once().await
    }

    fn spawn_worker(&self) {
        let runtime = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(err) = runtime.dispatch_pending_once().await {
                    log::error!("push relay dispatch failed: {err}");
                }

                tokio::select! {
                    _ = sleep(Duration::from_secs(DEFAULT_POLL_INTERVAL_SECS)) => {}
                    _ = runtime.inner.notify.notified() => {}
                }
            }
        });
    }

    async fn enqueue_call_started(
        &self,
        channel_id: String,
        actor_user_id: String,
        suppressed_user_ids: Vec<String>,
    ) -> Result<(), String> {
        let user_row = sqlx::query("SELECT username FROM users WHERE id = ?")
            .bind(&actor_user_id)
            .fetch_optional(&self.inner.db.0)
            .await
            .map_err(|err| err.to_string())?
            .ok_or_else(|| "call_started actor not found".to_string())?;
        let actor_username: String = user_row.get("username");

        let channel_row =
            sqlx::query("SELECT name, is_voice FROM channels WHERE id = ? AND deleted_at IS NULL")
                .bind(&channel_id)
                .fetch_optional(&self.inner.db.0)
                .await
                .map_err(|err| err.to_string())?
                .ok_or_else(|| "call_started channel not found".to_string())?;
        let channel_name: String = channel_row.get("name");
        let is_voice = channel_row.get::<i64, _>("is_voice") != 0;

        let member_rows =
            sqlx::query("SELECT user_id FROM channel_members WHERE channel_id = ? AND can_read = 1")
                .bind(&channel_id)
                .fetch_all(&self.inner.db.0)
                .await
                .map_err(|err| err.to_string())?;

        let suppressed: std::collections::HashSet<String> = suppressed_user_ids.into_iter().collect();
        let recipient_user_ids: Vec<String> = member_rows
            .into_iter()
            .map(|row| row.get::<String, _>("user_id"))
            .filter(|user_id| user_id != &actor_user_id)
            .filter(|user_id| !suppressed.contains(user_id))
            .collect();

        if recipient_user_ids.is_empty() {
            return Ok(());
        }

        let occurred_at = Utc::now();
        let payload = QueuedPushPayload {
            occurred_at,
            channel: PushChannelInfo {
                id: channel_id.clone(),
                name: channel_name,
                is_voice,
            },
            actor: PushActorInfo {
                id: actor_user_id.clone(),
                username: actor_username,
            },
            message: None,
            recipient_user_ids,
        };

        self.insert_event(
            PushEventType::CallStarted,
            format!("call:{channel_id}:{}", occurred_at.timestamp_millis()),
            payload,
        )
        .await
        .map_err(|err| err.to_string())?;
        self.inner.notify.notify_one();
        Ok(())
    }

    async fn insert_event(
        &self,
        event_type: PushEventType,
        event_id: String,
        payload: QueuedPushPayload,
    ) -> Result<(), sqlx::Error> {
        let now = Utc::now();
        let payload_json = serde_json::to_string(&payload)
            .map_err(|err| sqlx::Error::Protocol(err.to_string()))?;

        sqlx::query(
            "INSERT OR IGNORE INTO push_events(
                id, event_id, event_type, channel_id, actor_user_id, payload_json,
                attempt_count, next_attempt_at, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?, ?)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(event_id)
        .bind(event_type.as_str())
        .bind(&payload.channel.id)
        .bind(&payload.actor.id)
        .bind(payload_json)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&self.inner.db.0)
        .await?;

        Ok(())
    }
}

impl PushRelayRuntimeInner {
    async fn dispatch_pending_once(&self) -> Result<usize, String> {
        let pending_rows = sqlx::query(
            "SELECT id, event_id, event_type, payload_json, attempt_count
             FROM push_events
             WHERE dispatched_at IS NULL AND next_attempt_at <= ?
             ORDER BY created_at ASC
             LIMIT ?",
        )
        .bind(Utc::now())
        .bind(MAX_DISPATCH_BATCH)
        .fetch_all(&self.db.0)
        .await
        .map_err(|err| err.to_string())?;

        let mut dispatched = 0usize;
        for row in pending_rows {
            let event_type = parse_push_event_type(row.get::<String, _>("event_type").as_str())
                .ok_or_else(|| "invalid push event type in queue".to_string())?;
            let payload_json: String = row.get("payload_json");
            let payload: QueuedPushPayload =
                serde_json::from_str(&payload_json).map_err(|err| err.to_string())?;
            let event = PendingPushEvent {
                id: row.get("id"),
                event_id: row.get("event_id"),
                event_type,
                payload,
                attempt_count: row.get("attempt_count"),
            };

            match self.dispatch_event(&event).await {
                Ok(()) => {
                    dispatched += 1;
                }
                Err(err) => {
                    self.mark_retry(&event.id, event.attempt_count + 1, &err)
                        .await
                        .map_err(|db_err| db_err.to_string())?;
                }
            }
        }

        Ok(dispatched)
    }

    async fn dispatch_event(&self, event: &PendingPushEvent) -> Result<(), String> {
        let devices = self.load_devices_for_event(event).await?;
        if devices.is_empty() {
            self.mark_dispatched(&event.id)
                .await
                .map_err(|err| err.to_string())?;
            return Ok(());
        }

        let body = RelayPushBatchRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            event_id: event.event_id.clone(),
            event_type: event.event_type.clone(),
            occurred_at: event.payload.occurred_at,
            channel: event.payload.channel.clone(),
            actor: event.payload.actor.clone(),
            message: event.payload.message.clone(),
            devices,
        };

        let body_bytes = serde_json::to_vec(&body).map_err(|err| err.to_string())?;
        let request_url = self
            .relay_url
            .join("/v1/push/batches")
            .map_err(|err| err.to_string())?;
        let path = request_url.path().to_string();
        let timestamp = Utc::now().timestamp().to_string();
        let nonce = uuid::Uuid::new_v4().to_string();
        let signature = build_relay_signature(
            "POST",
            &path,
            &timestamp,
            &nonce,
            &body_bytes,
            &self.relay_server_secret,
        )?;

        let response = self
            .client
            .post(request_url)
            .header("Content-Type", "application/json")
            .header("X-Stuffchat-Relay-Server", &self.relay_server_id)
            .header("X-Stuffchat-Relay-Timestamp", timestamp)
            .header("X-Stuffchat-Relay-Nonce", nonce)
            .header("X-Stuffchat-Relay-Signature", signature)
            .body(body_bytes)
            .send()
            .await
            .map_err(|err| err.to_string())?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unavailable>".to_string());
            return Err(format!("relay returned {status}: {body}"));
        }

        let relay_response: RelayPushBatchResponse =
            response.json().await.map_err(|err| err.to_string())?;
        if !relay_response.invalid_installation_ids.is_empty() {
            self.delete_installations(&relay_response.invalid_installation_ids)
                .await
                .map_err(|err| err.to_string())?;
        }

        if relay_response.accepted && relay_response.retryable_failures == 0 {
            self.mark_dispatched(&event.id)
                .await
                .map_err(|err| err.to_string())?;
            return Ok(());
        }

        Err(format!(
            "relay reported {} retryable failures",
            relay_response.retryable_failures
        ))
    }

    async fn load_devices_for_event(
        &self,
        event: &PendingPushEvent,
    ) -> Result<Vec<RelayPushDevice>, String> {
        if event.payload.recipient_user_ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = event
            .payload
            .recipient_user_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");

        let preference_column = match event.event_type {
            PushEventType::Message => "message_notifications",
            PushEventType::CallStarted => "call_notifications",
        };

        let query = format!(
            "SELECT installation_id, push_token, environment
             FROM push_devices
             WHERE platform = 'ios'
               AND {preference_column} = 1
               AND user_id IN ({placeholders})"
        );

        let mut built = sqlx::query(&query);
        for user_id in &event.payload.recipient_user_ids {
            built = built.bind(user_id);
        }

        let rows = built
            .fetch_all(&self.db.0)
            .await
            .map_err(|err| err.to_string())?;

        rows.into_iter()
            .map(|row| {
                let environment_str: String = row.get("environment");
                let environment = parse_push_environment(&environment_str)
                    .ok_or_else(|| format!("invalid push environment: {environment_str}"))?;
                Ok(RelayPushDevice {
                    installation_id: row.get("installation_id"),
                    push_token: row.get("push_token"),
                    environment,
                })
            })
            .collect()
    }

    async fn delete_installations(&self, installation_ids: &[String]) -> Result<(), sqlx::Error> {
        if installation_ids.is_empty() {
            return Ok(());
        }

        let placeholders = installation_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");
        let query = format!("DELETE FROM push_devices WHERE installation_id IN ({placeholders})");
        let mut built = sqlx::query(&query);
        for installation_id in installation_ids {
            built = built.bind(installation_id);
        }
        built.execute(&self.db.0).await?;
        Ok(())
    }

    async fn mark_dispatched(&self, event_id: &str) -> Result<(), sqlx::Error> {
        let now = Utc::now();
        sqlx::query("UPDATE push_events SET dispatched_at = ?, updated_at = ?, last_error = NULL WHERE id = ?")
            .bind(now)
            .bind(now)
            .bind(event_id)
            .execute(&self.db.0)
            .await?;
        Ok(())
    }

    async fn mark_retry(
        &self,
        event_id: &str,
        attempt_count: i64,
        last_error: &str,
    ) -> Result<(), sqlx::Error> {
        let now = Utc::now();
        let delay = retry_delay(attempt_count);
        sqlx::query(
            "UPDATE push_events
             SET attempt_count = ?, next_attempt_at = ?, last_error = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(attempt_count)
        .bind(now + delay)
        .bind(last_error)
        .bind(now)
        .bind(event_id)
        .execute(&self.db.0)
        .await?;
        Ok(())
    }
}

fn retry_delay(attempt_count: i64) -> ChronoDuration {
    match attempt_count {
        0 | 1 => ChronoDuration::seconds(5),
        2 => ChronoDuration::seconds(15),
        3 => ChronoDuration::seconds(60),
        _ => ChronoDuration::seconds(300),
    }
}

fn build_relay_signature(
    method: &str,
    path: &str,
    timestamp: &str,
    nonce: &str,
    body: &[u8],
    secret: &str,
) -> Result<String, String> {
    let body_hash = hex::encode(Sha256::digest(body));
    let signing_input = format!("{method}\n{path}\n{timestamp}\n{nonce}\n{body_hash}");
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).map_err(|err| err.to_string())?;
    mac.update(signing_input.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

fn is_allowed_relay_url(url: &Url) -> bool {
    if url.scheme() == "https" {
        return true;
    }

    if url.scheme() != "http" {
        return false;
    }

    matches!(
        url.host_str(),
        Some("localhost") | Some("127.0.0.1") | Some("::1")
    )
}

fn parse_push_environment(value: &str) -> Option<PushEnvironment> {
    match value {
        "development" => Some(PushEnvironment::Development),
        "production" => Some(PushEnvironment::Production),
        _ => None,
    }
}

fn parse_push_event_type(value: &str) -> Option<PushEventType> {
    match value {
        "message" => Some(PushEventType::Message),
        "call_started" => Some(PushEventType::CallStarted),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use std::path::PathBuf;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn relay_signature_is_stable() {
        let signature = build_relay_signature(
            "POST",
            "/v1/push/batches",
            "1700000000",
            "nonce",
            br#"{"x":1}"#,
            "secret",
        )
        .expect("signature");

        assert_eq!(
            signature,
            "075ef05a715f9ca742d018fc9d35ae22b8d55e2e47bf707cf3290f09e2d5aeaa"
        );
    }

    #[test]
    fn relay_url_requires_https_except_localhost() {
        assert!(is_allowed_relay_url(&Url::parse("https://relay.example.com").expect("url")));
        assert!(is_allowed_relay_url(&Url::parse("http://127.0.0.1:8080").expect("url")));
        assert!(!is_allowed_relay_url(&Url::parse("http://relay.example.com").expect("url")));
    }

    #[tokio::test]
    async fn enqueue_call_started_excludes_actor_and_suppressed_users() {
        let harness = TestHarness::new("http://127.0.0.1:9").await;
        insert_user(&harness.db, "user-1", "alice").await;
        insert_user(&harness.db, "user-2", "bob").await;
        insert_user(&harness.db, "user-3", "carol").await;
        insert_channel(&harness.db, "channel-1", "general", true).await;
        insert_member(&harness.db, "channel-1", "user-1").await;
        insert_member(&harness.db, "channel-1", "user-2").await;
        insert_member(&harness.db, "channel-1", "user-3").await;

        harness
            .runtime
            .enqueue_call_started(
                "channel-1".to_string(),
                "user-1".to_string(),
                vec!["user-3".to_string()],
            )
            .await
            .expect("enqueue call");

        let row = sqlx::query("SELECT payload_json FROM push_events WHERE event_type = 'call_started'")
            .fetch_one(&harness.db.0)
            .await
            .expect("push event row");
        let payload: QueuedPushPayload =
            serde_json::from_str(&row.get::<String, _>("payload_json")).expect("payload");

        assert_eq!(payload.actor.username, "alice");
        assert_eq!(payload.recipient_user_ids, vec!["user-2".to_string()]);
    }

    #[tokio::test]
    async fn dispatch_prunes_invalid_installations() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let address = listener.local_addr().expect("local addr");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept connection");
            let mut buffer = vec![0u8; 4096];
            let _ = stream.read(&mut buffer).await.expect("read request");
            let body = r#"{"accepted":true,"invalid_installation_ids":["installation-1"],"retryable_failures":0}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write response");
        });

        let harness = TestHarness::new(&format!("http://{}", address)).await;
        insert_user(&harness.db, "user-1", "alice").await;
        insert_user(&harness.db, "user-2", "bob").await;
        insert_channel(&harness.db, "channel-1", "general", false).await;
        insert_member(&harness.db, "channel-1", "user-1").await;
        insert_member(&harness.db, "channel-1", "user-2").await;
        insert_device(&harness.db, "user-2", "installation-1").await;

        let payload = QueuedPushPayload {
            occurred_at: Utc::now(),
            channel: PushChannelInfo {
                id: "channel-1".to_string(),
                name: "general".to_string(),
                is_voice: false,
            },
            actor: PushActorInfo {
                id: "user-1".to_string(),
                username: "alice".to_string(),
            },
            message: Some(PushMessageInfo {
                id: "message-1".to_string(),
                preview: "hello".to_string(),
            }),
            recipient_user_ids: vec!["user-2".to_string()],
        };
        harness
            .runtime
            .insert_event(
                PushEventType::Message,
                "msg:message-1".to_string(),
                payload,
            )
            .await
            .expect("insert message event");

        harness
            .runtime
            .dispatch_pending_once()
            .await
            .expect("dispatch");

        let remaining_devices =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM push_devices WHERE installation_id = ?")
                .bind("installation-1")
                .fetch_one(&harness.db.0)
                .await
                .expect("remaining devices");
        let dispatched_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM push_events WHERE dispatched_at IS NOT NULL")
                .fetch_one(&harness.db.0)
                .await
                .expect("dispatched count");

        server.await.expect("server task");
        assert_eq!(remaining_devices, 0);
        assert_eq!(dispatched_count, 1);
    }

    struct TestHarness {
        _root_dir: PathBuf,
        db: Db,
        runtime: PushRelayRuntime,
    }

    impl TestHarness {
        async fn new(relay_url: &str) -> Self {
            let root_dir =
                std::env::temp_dir().join(format!("stuffchat-push-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root_dir).expect("create temp root");
            let database_path = root_dir.join("test.sqlite3");
            let db = Db::connect_and_migrate(database_path.to_str().expect("db path"))
                .await
                .expect("db init");
            let cfg = Config {
                listen: "127.0.0.1:0".to_string(),
                database_path: database_path.to_string_lossy().into_owned(),
                uploads_dir: root_dir.join("uploads").to_string_lossy().into_owned(),
                jwt_secret: Some("jwt-test-secret".to_string()),
                allowed_origins: vec!["http://localhost".to_string()],
                max_upload_size: 1024 * 1024,
                presence_timeout_secs: 60,
                invite_only: false,
                bridge_enabled: false,
                bridge_url: None,
                push_relay_enabled: true,
                push_relay_url: Some(relay_url.to_string()),
                push_relay_server_id: Some("server-1".to_string()),
                push_relay_server_secret: Some("secret".to_string()),
                push_relay_timeout_secs: 5,
            };
            let runtime = PushRelayRuntime::new(&cfg, db.clone()).expect("push runtime");

            Self {
                _root_dir: root_dir,
                db,
                runtime,
            }
        }
    }

    async fn insert_user(db: &Db, id: &str, username: &str) {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO users(id, username, email, password_hash, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(username)
        .bind(format!("{username}@example.com"))
        .bind("$argon2id$v=19$m=19456,t=2,p=1$abcdefghijklmnop$abcdefghijklmnop")
        .bind(now)
        .bind(now)
        .execute(&db.0)
        .await
        .expect("insert user");
    }

    async fn insert_channel(db: &Db, id: &str, name: &str, is_voice: bool) {
        sqlx::query(
            "INSERT INTO channels(id, name, is_voice, is_private, created_by, created_at) VALUES (?, ?, ?, 0, ?, ?)",
        )
        .bind(id)
        .bind(name)
        .bind(is_voice)
        .bind("user-1")
        .bind(Utc::now())
        .execute(&db.0)
        .await
        .expect("insert channel");
    }

    async fn insert_member(db: &Db, channel_id: &str, user_id: &str) {
        sqlx::query(
            "INSERT INTO channel_members(channel_id, user_id, can_read, can_write, can_manage) VALUES (?, ?, 1, 1, 0)",
        )
        .bind(channel_id)
        .bind(user_id)
        .execute(&db.0)
        .await
        .expect("insert member");
    }

    async fn insert_device(db: &Db, user_id: &str, installation_id: &str) {
        sqlx::query(
            "INSERT INTO push_devices(
                user_id, installation_id, platform, push_token, environment,
                message_notifications, call_notifications, created_at, updated_at
             ) VALUES (?, ?, 'ios', ?, 'development', 1, 1, ?, ?)",
        )
        .bind(user_id)
        .bind(installation_id)
        .bind("device-token")
        .bind(Utc::now())
        .bind(Utc::now())
        .execute(&db.0)
        .await
        .expect("insert device");
    }
}
