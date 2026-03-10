mod apns;
mod auth;
mod config;
mod db;
mod models;

use actix_web::http::StatusCode;
use actix_web::middleware::Logger;
use actix_web::web::{self, Bytes, Data};
use actix_web::{App, HttpRequest, HttpResponse, HttpServer};
use anyhow::Context;
use apns::{ApnsNotification, ApnsOutcome, RealApnsSender, SharedApnsSender};
use chrono::Utc;
use config::Config;
use db::Db;
use env_logger::Env;
use models::{RelayPushBatchRequest, RelayPushBatchResponse};
use rand::TryRngCore;
use sqlx::Row;
use std::sync::Arc;

#[derive(Clone)]
struct AppState {
    db: Db,
    apns_sender: SharedApnsSender,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    if let Err(err) = run().await {
        eprintln!("{err:#}");
        return Err(std::io::Error::other(err.to_string()));
    }

    Ok(())
}

async fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = Config::load();
    let db = Db::connect_and_migrate(&config.database_path)
        .await
        .context("database init failed")?;

    if let Some(outcome) = maybe_run_cli(&db, &args).await? {
        println!("{outcome}");
        return Ok(());
    }

    let apns_sender: SharedApnsSender =
        Arc::new(RealApnsSender::new(&config).map_err(anyhow::Error::msg)?);
    let state = AppState { db, apns_sender };
    let listen_addr = config.listen.clone();

    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(Data::new(state.clone()))
            .route("/v1/push/batches", web::post().to(post_push_batch))
    })
    .bind(listen_addr)?
    .run()
    .await?;

    Ok(())
}

async fn maybe_run_cli(db: &Db, args: &[String]) -> anyhow::Result<Option<String>> {
    if args.is_empty() {
        return Ok(None);
    }

    if args.len() >= 4 && args[0] == "server" && args[1] == "create" && args[2] == "--label" {
        let label = args[3..].join(" ").trim().to_string();
        anyhow::ensure!(!label.is_empty(), "label is required");

        let server_id = uuid::Uuid::new_v4().to_string();
        let server_secret = random_secret()?;
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO relay_servers(server_id, label, secret, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&server_id)
        .bind(&label)
        .bind(&server_secret)
        .bind(now)
        .execute(&db.0)
        .await?;
        return Ok(Some(format!(
            "server_id={server_id}\nserver_secret={server_secret}"
        )));
    }

    if args.len() == 4 && args[0] == "server" && args[1] == "revoke" && args[2] == "--server-id" {
        let server_id = args[3].trim();
        anyhow::ensure!(!server_id.is_empty(), "server_id is required");
        sqlx::query("UPDATE relay_servers SET revoked_at = ? WHERE server_id = ?")
            .bind(Utc::now())
            .bind(server_id)
            .execute(&db.0)
            .await?;
        return Ok(Some(format!("revoked={server_id}")));
    }

    anyhow::bail!("unsupported arguments");
}

async fn post_push_batch(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: Bytes,
) -> HttpResponse {
    let server_id = match auth::verify_request(&req, body.as_ref(), &state.db).await {
        Ok(server_id) => server_id,
        Err(err) => {
            log::warn!("rejected push batch request: error={}", err);
            return HttpResponse::build(StatusCode::UNAUTHORIZED)
                .json(serde_json::json!({ "error": err }));
        }
    };

    let batch: RelayPushBatchRequest = match serde_json::from_slice(body.as_ref()) {
        Ok(batch) => batch,
        Err(err) => {
            log::warn!(
                "rejected push batch request from server_id={}: invalid_json={}",
                server_id,
                err
            );
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": err.to_string()
            }));
        }
    };

    match process_batch(&state, &server_id, batch).await {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(err) => {
            log::error!(
                "failed to process push batch from server_id={}: {}",
                server_id,
                err
            );
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": err
            }))
        }
    }
}

async fn process_batch(
    state: &AppState,
    server_id: &str,
    batch: RelayPushBatchRequest,
) -> Result<RelayPushBatchResponse, String> {
    let request_id = batch.request_id.clone();
    let event_id = batch.event_id.clone();
    let event_type = format!("{:?}", batch.event_type);
    let channel_id = batch.channel.id.clone();
    let device_count = batch.devices.len();
    log::info!(
        "received push batch: server_id={} request_id={} event_id={} event_type={} channel_id={} devices={}",
        server_id,
        request_id,
        event_id,
        event_type,
        channel_id,
        device_count
    );

    let mut invalid_installation_ids = Vec::new();
    let mut retryable_failures = 0usize;

    for device in batch.devices {
        let existing = sqlx::query(
            "SELECT status FROM push_deliveries
             WHERE server_id = ? AND event_id = ? AND installation_id = ?",
        )
        .bind(server_id)
        .bind(&batch.event_id)
        .bind(&device.installation_id)
        .fetch_optional(&state.db.0)
        .await
        .map_err(|err| err.to_string())?;

        if let Some(row) = existing {
            let status: String = row.get("status");
            match status.as_str() {
                "delivered" => {
                    log::info!(
                        "skipping previously delivered push device: server_id={} event_id={} installation_id={}",
                        server_id,
                        batch.event_id,
                        device.installation_id
                    );
                    continue;
                }
                "invalid" => {
                    log::warn!(
                        "skipping known invalid push device: server_id={} event_id={} installation_id={}",
                        server_id,
                        batch.event_id,
                        device.installation_id
                    );
                    invalid_installation_ids.push(device.installation_id.clone());
                    continue;
                }
                _ => {}
            }
        }

        let notification = ApnsNotification {
            event_type: batch.event_type.clone(),
            channel: batch.channel.clone(),
            actor: batch.actor.clone(),
            message: batch.message.clone(),
            device: device.clone(),
        };

        let outcome = match state.apns_sender.send(notification).await {
            Ok(outcome) => outcome,
            Err(err) => {
                let message = format!(
                    "APNS send failed: server_id={} event_id={} installation_id={} error={}",
                    server_id, batch.event_id, device.installation_id, err
                );
                log::error!("{message}");
                return Err(message);
            }
        };
        match outcome {
            ApnsOutcome::Delivered { apns_id } => {
                log::info!(
                    "delivered APNS notification: server_id={} event_id={} installation_id={} environment={:?} apns_id={}",
                    server_id,
                    batch.event_id,
                    device.installation_id,
                    device.environment,
                    apns_id.as_deref().unwrap_or("-")
                );
                upsert_delivery(
                    &state.db,
                    server_id,
                    &batch.event_id,
                    &device.installation_id,
                    "delivered",
                    None,
                    apns_id.as_deref(),
                    true,
                )
                .await
                .map_err(|err| err.to_string())?;
            }
            ApnsOutcome::InvalidToken { reason } => {
                log::warn!(
                    "APNS rejected device token: server_id={} event_id={} installation_id={} environment={:?} reason={}",
                    server_id,
                    batch.event_id,
                    device.installation_id,
                    device.environment,
                    reason
                );
                invalid_installation_ids.push(device.installation_id.clone());
                upsert_delivery(
                    &state.db,
                    server_id,
                    &batch.event_id,
                    &device.installation_id,
                    "invalid",
                    Some(&reason),
                    None,
                    false,
                )
                .await
                .map_err(|err| err.to_string())?;
            }
            ApnsOutcome::Retryable { reason } => {
                log::warn!(
                    "APNS returned retryable failure: server_id={} event_id={} installation_id={} environment={:?} reason={}",
                    server_id,
                    batch.event_id,
                    device.installation_id,
                    device.environment,
                    reason
                );
                retryable_failures += 1;
                upsert_delivery(
                    &state.db,
                    server_id,
                    &batch.event_id,
                    &device.installation_id,
                    "retryable",
                    Some(&reason),
                    None,
                    false,
                )
                .await
                .map_err(|err| err.to_string())?;
            }
        }
    }

    log::info!(
        "completed push batch: server_id={} request_id={} event_id={} event_type={} channel_id={} devices={} invalid_installations={} retryable_failures={}",
        server_id,
        request_id,
        event_id,
        event_type,
        channel_id,
        device_count,
        invalid_installation_ids.len(),
        retryable_failures
    );
    Ok(RelayPushBatchResponse {
        accepted: retryable_failures == 0,
        invalid_installation_ids,
        retryable_failures,
    })
}

async fn upsert_delivery(
    db: &Db,
    server_id: &str,
    event_id: &str,
    installation_id: &str,
    status: &str,
    last_error: Option<&str>,
    apns_id: Option<&str>,
    delivered: bool,
) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    let delivered_at = if delivered { Some(now) } else { None };
    sqlx::query(
        "INSERT INTO push_deliveries(
            server_id, event_id, installation_id, status, last_error, apns_id, updated_at, delivered_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(server_id, event_id, installation_id)
         DO UPDATE SET
            status = excluded.status,
            last_error = excluded.last_error,
            apns_id = excluded.apns_id,
            updated_at = excluded.updated_at,
            delivered_at = excluded.delivered_at",
    )
    .bind(server_id)
    .bind(event_id)
    .bind(installation_id)
    .bind(status)
    .bind(last_error)
    .bind(apns_id)
    .bind(now)
    .bind(delivered_at)
    .execute(&db.0)
    .await?;
    Ok(())
}

fn random_secret() -> anyhow::Result<String> {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    Ok(hex::encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test;
    use async_trait::async_trait;
    use models::{
        PushActorInfo, PushChannelInfo, PushEnvironment, PushEventType, PushMessageInfo,
        RelayPushDevice,
    };
    use std::sync::Mutex;

    struct TestSender {
        outcomes: Mutex<Vec<ApnsOutcome>>,
    }

    #[async_trait]
    impl apns::ApnsSender for TestSender {
        async fn send(&self, _notification: ApnsNotification) -> Result<ApnsOutcome, String> {
            self.outcomes
                .lock()
                .expect("lock")
                .pop()
                .ok_or_else(|| "no outcome queued".to_string())
        }
    }

    async fn test_state() -> AppState {
        let root_dir =
            std::env::temp_dir().join(format!("stuffchat-relay-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root_dir).expect("create temp relay dir");
        let db = Db::connect_and_migrate(root_dir.join("relay.sqlite3").to_str().expect("db path"))
            .await
            .expect("db");
        sqlx::query(
            "INSERT INTO relay_servers(server_id, label, secret, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind("server-1")
        .bind("Test")
        .bind("secret")
        .bind(Utc::now())
        .execute(&db.0)
        .await
        .expect("insert server");

        AppState {
            db,
            apns_sender: Arc::new(TestSender {
                outcomes: Mutex::new(vec![ApnsOutcome::InvalidToken {
                    reason: "BadDeviceToken".to_string(),
                }]),
            }),
        }
    }

    #[actix_web::test]
    async fn rejects_nonce_replay() {
        let state = test_state().await;
        let batch = RelayPushBatchRequest {
            request_id: "request-1".to_string(),
            event_id: "event-1".to_string(),
            event_type: PushEventType::Message,
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
            devices: vec![RelayPushDevice {
                installation_id: "installation-1".to_string(),
                push_token: "token".to_string(),
                environment: PushEnvironment::Development,
            }],
        };
        let body = serde_json::to_vec(&batch).expect("body");
        let timestamp = Utc::now().timestamp().to_string();
        let signature = auth::build_signature(
            "POST",
            "/v1/push/batches",
            &timestamp,
            "nonce-1",
            &body,
            "secret",
        )
        .expect("signature");

        let app = test::init_service(
            App::new()
                .app_data(Data::new(state.clone()))
                .route("/v1/push/batches", web::post().to(post_push_batch)),
        )
        .await;

        let first = test::TestRequest::post()
            .uri("/v1/push/batches")
            .insert_header(("X-Stuffchat-Relay-Server", "server-1"))
            .insert_header(("X-Stuffchat-Relay-Timestamp", timestamp.clone()))
            .insert_header(("X-Stuffchat-Relay-Nonce", "nonce-1"))
            .insert_header(("X-Stuffchat-Relay-Signature", signature.clone()))
            .set_payload(body.clone())
            .to_request();
        let first_response = test::call_service(&app, first).await;
        assert_eq!(first_response.status(), StatusCode::OK);

        let second = test::TestRequest::post()
            .uri("/v1/push/batches")
            .insert_header(("X-Stuffchat-Relay-Server", "server-1"))
            .insert_header(("X-Stuffchat-Relay-Timestamp", timestamp))
            .insert_header(("X-Stuffchat-Relay-Nonce", "nonce-1"))
            .insert_header(("X-Stuffchat-Relay-Signature", signature))
            .set_payload(body)
            .to_request();
        let second_response = test::call_service(&app, second).await;
        assert_eq!(second_response.status(), StatusCode::UNAUTHORIZED);
    }
}
