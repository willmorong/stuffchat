use crate::config::Config;
use crate::models::{
    PushActorInfo, PushChannelInfo, PushEnvironment, PushEventType, PushMessageInfo,
    RelayPushDevice,
};
use async_trait::async_trait;
use chrono::Utc;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;

const APNS_SANDBOX_URL: &str = "https://api.sandbox.push.apple.com";
const APNS_PRODUCTION_URL: &str = "https://api.push.apple.com";

#[derive(Debug, Clone)]
pub struct ApnsNotification {
    pub event_type: PushEventType,
    pub channel: PushChannelInfo,
    pub actor: PushActorInfo,
    pub message: Option<PushMessageInfo>,
    pub device: RelayPushDevice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApnsOutcome {
    Delivered { apns_id: Option<String> },
    InvalidToken { reason: String },
    Retryable { reason: String },
}

#[async_trait]
pub trait ApnsSender: Send + Sync {
    async fn send(&self, notification: ApnsNotification) -> Result<ApnsOutcome, String>;
}

pub type SharedApnsSender = Arc<dyn ApnsSender>;

pub struct RealApnsSender {
    client: reqwest::Client,
    key_id: String,
    team_id: String,
    topic: String,
    encoding_key: EncodingKey,
}

impl RealApnsSender {
    pub fn new(config: &Config) -> Result<Self, String> {
        let key_id = config
            .apns_key_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "apns_key_id is required".to_string())?;
        let team_id = config
            .apns_team_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "apns_team_id is required".to_string())?;
        let topic = config
            .apns_topic
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "apns_topic is required".to_string())?;
        let private_key_path = config
            .apns_private_key_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "apns_private_key_path is required".to_string())?;
        let pem = std::fs::read(private_key_path).map_err(|err| err.to_string())?;
        let encoding_key =
            EncodingKey::from_ec_pem(&pem).map_err(|err| format!("invalid apns key: {err}"))?;
        let client = reqwest::Client::builder()
            .build()
            .map_err(|err| err.to_string())?;

        Ok(Self {
            client,
            key_id: key_id.to_string(),
            team_id: team_id.to_string(),
            topic: topic.to_string(),
            encoding_key,
        })
    }

    fn authorization_token(&self) -> Result<String, String> {
        #[derive(Serialize)]
        struct Claims<'a> {
            iss: &'a str,
            iat: usize,
        }

        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(self.key_id.clone());
        jsonwebtoken::encode(
            &header,
            &Claims {
                iss: &self.team_id,
                iat: Utc::now().timestamp() as usize,
            },
            &self.encoding_key,
        )
        .map_err(|err| err.to_string())
    }
}

#[async_trait]
impl ApnsSender for RealApnsSender {
    async fn send(&self, notification: ApnsNotification) -> Result<ApnsOutcome, String> {
        let authorization = self.authorization_token()?;
        let request = build_request_parts(&notification);
        let base_url = match notification.device.environment {
            PushEnvironment::Development => APNS_SANDBOX_URL,
            PushEnvironment::Production => APNS_PRODUCTION_URL,
        };
        let url = format!("{base_url}/3/device/{}", notification.device.push_token);

        let response = self
            .client
            .post(url)
            .header("authorization", format!("bearer {authorization}"))
            .header("apns-push-type", "alert")
            .header("apns-topic", &self.topic)
            .header("apns-priority", "10")
            .header("apns-collapse-id", request.collapse_id)
            .json(&request.payload)
            .send()
            .await
            .map_err(|err| err.to_string())?;

        let apns_id = response
            .headers()
            .get("apns-id")
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);

        if response.status().is_success() {
            return Ok(ApnsOutcome::Delivered { apns_id });
        }

        let status = response.status();
        let body = response
            .json::<serde_json::Value>()
            .await
            .unwrap_or_else(|_| json!({}));
        let reason = body
            .get("reason")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
            .to_string();

        if status == StatusCode::BAD_REQUEST || status == StatusCode::GONE {
            if matches!(
                reason.as_str(),
                "BadDeviceToken" | "DeviceTokenNotForTopic" | "Unregistered"
            ) {
                return Ok(ApnsOutcome::InvalidToken { reason });
            }
        }

        Ok(ApnsOutcome::Retryable {
            reason: format!("{status}: {reason}"),
        })
    }
}

struct BuiltApnsRequest {
    collapse_id: String,
    payload: serde_json::Value,
}

fn build_request_parts(notification: &ApnsNotification) -> BuiltApnsRequest {
    let title = match notification.event_type {
        PushEventType::Message => {
            let channel_name = notification.channel.name.trim().trim_start_matches('#');
            format!("{} (#{})", notification.actor.username, channel_name)
        }
        PushEventType::CallStarted => "Call Started".to_string(),
    };
    let body = match notification.event_type {
        PushEventType::Message => {
            let preview = notification
                .message
                .as_ref()
                .map(|message| message.preview.as_str())
                .filter(|preview| !preview.trim().is_empty())
                .unwrap_or("Sent an attachment");
            preview.to_string()
        }
        PushEventType::CallStarted => {
            format!(
                "{} started a call in {}",
                notification.actor.username, notification.channel.name
            )
        }
    };
    let collapse_id = match notification.event_type {
        PushEventType::Message => format!("message:{}", notification.channel.id),
        PushEventType::CallStarted => format!("call_started:{}", notification.channel.id),
    };
    let message_id = notification
        .message
        .as_ref()
        .map(|message| message.id.clone());
    let payload = json!({
        "aps": {
            "alert": {
                "title": title,
                "body": body,
            },
            "sound": "default",
            "thread-id": notification.channel.id,
        },
        "stuffchat": {
            "type": notification.event_type,
            "channel_id": notification.channel.id,
            "message_id": message_id,
        }
    });

    BuiltApnsRequest {
        collapse_id,
        payload,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{PushEnvironment, PushEventType};

    #[test]
    fn message_request_contains_thread_and_payload() {
        let built = build_request_parts(&ApnsNotification {
            event_type: PushEventType::Message,
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
            device: RelayPushDevice {
                installation_id: "installation-1".to_string(),
                push_token: "token".to_string(),
                environment: PushEnvironment::Development,
            },
        });

        assert_eq!(built.collapse_id, "message:channel-1");
        assert_eq!(
            built.payload["stuffchat"]["message_id"].as_str(),
            Some("message-1")
        );
        assert_eq!(
            built.payload["aps"]["thread-id"].as_str(),
            Some("channel-1")
        );
        assert_eq!(built.payload["aps"]["alert"]["title"].as_str(), Some("alice (#general)"));
        assert_eq!(built.payload["aps"]["alert"]["body"].as_str(), Some("hello"));
    }

    #[test]
    fn message_title_normalizes_channel_prefix() {
        let built = build_request_parts(&ApnsNotification {
            event_type: PushEventType::Message,
            channel: PushChannelInfo {
                id: "channel-1".to_string(),
                name: "#random".to_string(),
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
            device: RelayPushDevice {
                installation_id: "installation-1".to_string(),
                push_token: "token".to_string(),
                environment: PushEnvironment::Development,
            },
        });

        assert_eq!(built.payload["aps"]["alert"]["title"].as_str(), Some("alice (#random)"));
    }
}
