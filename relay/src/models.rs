use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PushEnvironment {
    Development,
    Production,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PushEventType {
    Message,
    CallStarted,
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
