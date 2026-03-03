use chrono::{DateTime, Utc};
use rand::TryRngCore;
use reqwest::StatusCode;
use serde::Serialize;
use sqlx::Row;
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};

use crate::db::Db;

const BRIDGE_SECRET_FILE: &str = "./bridge_secret";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeEventType {
    CallJoined,
    CallLeft,
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

#[derive(Debug, Clone, Serialize)]
pub struct BridgeEventEnvelope {
    #[serde(rename = "type")]
    pub event_type: BridgeEventType,
    pub occurred_at: DateTime<Utc>,
    pub user: BridgeResolvedUser,
    pub channel: BridgeResolvedChannel,
}

#[derive(Debug)]
struct BridgeDispatchEvent {
    event_type: BridgeEventType,
    channel_id: String,
    user_id: String,
    occurred_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct BridgeRuntime {
    sender: mpsc::UnboundedSender<BridgeDispatchEvent>,
}

impl BridgeRuntime {
    pub fn new(secret: String, destination_url: String, db: Db) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let worker = BridgeWorker::new(secret, destination_url, db);
        tokio::spawn(worker.run(receiver));
        Self { sender }
    }

    pub fn record_call_joined(&self, channel_id: impl Into<String>, user_id: impl Into<String>) {
        self.enqueue(BridgeEventType::CallJoined, channel_id, user_id);
    }

    pub fn record_call_left(&self, channel_id: impl Into<String>, user_id: impl Into<String>) {
        self.enqueue(BridgeEventType::CallLeft, channel_id, user_id);
    }

    fn enqueue(
        &self,
        event_type: BridgeEventType,
        channel_id: impl Into<String>,
        user_id: impl Into<String>,
    ) {
        let event = BridgeDispatchEvent {
            event_type,
            channel_id: channel_id.into(),
            user_id: user_id.into(),
            occurred_at: Utc::now(),
        };

        if let Err(err) = self.sender.send(event) {
            log::error!(
                "bridge worker unavailable; dropping event: {:?}",
                err.0.event_type
            );
        }
    }
}

struct BridgeWorker {
    secret: Arc<String>,
    destination_url: Arc<String>,
    db: Db,
    client: reqwest::Client,
}

impl BridgeWorker {
    fn new(secret: String, destination_url: String, db: Db) -> Self {
        Self {
            secret: Arc::new(secret),
            destination_url: Arc::new(destination_url),
            db,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("bridge http client"),
        }
    }

    async fn run(self, mut receiver: mpsc::UnboundedReceiver<BridgeDispatchEvent>) {
        while let Some(event) = receiver.recv().await {
            if let Err(err) = self.deliver_with_backoff(event).await {
                log::error!("bridge delivery failed: {err}");
            }
        }
    }

    async fn deliver_with_backoff(&self, event: BridgeDispatchEvent) -> Result<(), String> {
        let delays = [
            Duration::from_secs(0),
            Duration::from_secs(1),
            Duration::from_secs(2),
        ];
        let mut last_error = String::from("bridge delivery did not execute");

        for delay in delays {
            if !delay.is_zero() {
                sleep(delay).await;
            }

            match self.deliver(&event).await {
                Ok(()) => return Ok(()),
                Err(err) => {
                    last_error = err;
                }
            }
        }

        Err(last_error)
    }

    async fn deliver(&self, event: &BridgeDispatchEvent) -> Result<(), String> {
        let payload = BridgeEventEnvelope {
            event_type: event.event_type,
            occurred_at: event.occurred_at,
            user: resolve_user(&self.db, &event.user_id)
                .await
                .map_err(|err| format!("failed to resolve user {}: {err}", event.user_id))?,
            channel: resolve_channel(&self.db, &event.channel_id)
                .await
                .map_err(|err| format!("failed to resolve channel {}: {err}", event.channel_id))?,
        };

        let response = self
            .client
            .post(self.destination_url.as_str())
            .bearer_auth(self.secret.as_str())
            .json(&payload)
            .send()
            .await
            .map_err(|err| format!("request error: {err}"))?;

        if response.status().is_success() {
            return Ok(());
        }

        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("<unavailable>"));

        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(format!(
                "authentication failed with status {status}: {body}"
            ));
        }

        Err(format!("received status {status}: {body}"))
    }
}

async fn resolve_user(db: &Db, user_id: &str) -> Result<BridgeResolvedUser, sqlx::Error> {
    let row = sqlx::query("SELECT id, username FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(&db.0)
        .await?;

    Ok(match row {
        Some(row) => BridgeResolvedUser {
            id: row.get("id"),
            username: Some(row.get("username")),
        },
        None => BridgeResolvedUser {
            id: user_id.to_string(),
            username: None,
        },
    })
}

async fn resolve_channel(db: &Db, channel_id: &str) -> Result<BridgeResolvedChannel, sqlx::Error> {
    let row =
        sqlx::query("SELECT id, name, is_voice FROM channels WHERE deleted_at IS NULL AND id = ?")
            .bind(channel_id)
            .fetch_optional(&db.0)
            .await?;

    Ok(match row {
        Some(row) => BridgeResolvedChannel {
            id: row.get("id"),
            name: Some(row.get("name")),
            is_voice: Some(row.get::<i64, _>("is_voice") != 0),
        },
        None => BridgeResolvedChannel {
            id: channel_id.to_string(),
            name: None,
            is_voice: None,
        },
    })
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
