use crate::bridge::{BridgeAuth, BridgeResolvedChannel, BridgeResolvedUser, BridgeRuntime};
use crate::db::Db;
use crate::errors::ApiError;
use actix_web::{HttpResponse, web};
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Row, Sqlite};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize)]
struct BridgeStatusResponse {
    oldest_available: Option<u64>,
    latest_available: u64,
}

#[derive(Serialize)]
struct BridgeEventEnvelope {
    seq: u64,
    #[serde(rename = "type")]
    event_type: crate::bridge::BridgeEventType,
    occurred_at: chrono::DateTime<chrono::Utc>,
    user: BridgeResolvedUser,
    channel: BridgeResolvedChannel,
}

#[derive(Serialize)]
struct BridgeEventsResponse {
    reset_required: bool,
    oldest_available: Option<u64>,
    latest_available: u64,
    next_after: u64,
    events: Vec<BridgeEventEnvelope>,
}

#[derive(Deserialize)]
struct EventsQuery {
    after: Option<u64>,
    limit: Option<u16>,
}

#[derive(Deserialize)]
struct ResolveRequest {
    #[serde(default)]
    user_ids: Vec<String>,
    #[serde(default)]
    channel_ids: Vec<String>,
}

#[derive(Serialize)]
struct ResolveResponse {
    users: BTreeMap<String, BridgeResolvedUser>,
    channels: BTreeMap<String, BridgeResolvedChannel>,
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/status", web::get().to(status))
        .route("/events", web::get().to(events))
        .route("/resolve", web::post().to(resolve));
}

async fn status(
    runtime: web::Data<BridgeRuntime>,
    _auth: BridgeAuth,
) -> Result<HttpResponse, ApiError> {
    let (oldest_available, latest_available) = runtime.status();
    Ok(HttpResponse::Ok().json(BridgeStatusResponse {
        oldest_available,
        latest_available,
    }))
}

async fn events(
    runtime: web::Data<BridgeRuntime>,
    db: web::Data<Db>,
    _auth: BridgeAuth,
    query: web::Query<EventsQuery>,
) -> Result<HttpResponse, ApiError> {
    let after = query.after.unwrap_or(0);
    let limit = query.limit.unwrap_or(100).clamp(1, 500) as usize;
    let page = runtime.page(after, limit);

    let user_ids: BTreeSet<String> = page
        .events
        .iter()
        .map(|event| event.user_id.clone())
        .collect();
    let channel_ids: BTreeSet<String> = page
        .events
        .iter()
        .map(|event| event.channel_id.clone())
        .collect();
    let users = resolve_users(&db, &user_ids).await?;
    let channels = resolve_channels(&db, &channel_ids).await?;

    let events =
        page.events
            .into_iter()
            .map(|event| BridgeEventEnvelope {
                seq: event.seq,
                event_type: event.event_type,
                occurred_at: event.occurred_at,
                user: users
                    .get(&event.user_id)
                    .cloned()
                    .unwrap_or(BridgeResolvedUser {
                        id: event.user_id.clone(),
                        username: None,
                    }),
                channel: channels.get(&event.channel_id).cloned().unwrap_or(
                    BridgeResolvedChannel {
                        id: event.channel_id.clone(),
                        name: None,
                        is_voice: None,
                    },
                ),
            })
            .collect();

    Ok(HttpResponse::Ok().json(BridgeEventsResponse {
        reset_required: page.reset_required,
        oldest_available: page.oldest_available,
        latest_available: page.latest_available,
        next_after: page.next_after,
        events,
    }))
}

async fn resolve(
    db: web::Data<Db>,
    _auth: BridgeAuth,
    body: web::Json<ResolveRequest>,
) -> Result<HttpResponse, ApiError> {
    let user_ids: BTreeSet<String> = body.user_ids.iter().cloned().collect();
    let channel_ids: BTreeSet<String> = body.channel_ids.iter().cloned().collect();

    Ok(HttpResponse::Ok().json(ResolveResponse {
        users: resolve_users(&db, &user_ids).await?,
        channels: resolve_channels(&db, &channel_ids).await?,
    }))
}

async fn resolve_users(
    db: &Db,
    user_ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, BridgeResolvedUser>, ApiError> {
    if user_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let mut builder = QueryBuilder::<Sqlite>::new("SELECT id, username FROM users WHERE id IN (");
    push_bind_list(&mut builder, user_ids);
    builder.push(")");

    let rows = builder.build().fetch_all(&db.0).await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let id: String = row.get("id");
            (
                id.clone(),
                BridgeResolvedUser {
                    id,
                    username: Some(row.get("username")),
                },
            )
        })
        .collect())
}

async fn resolve_channels(
    db: &Db,
    channel_ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, BridgeResolvedChannel>, ApiError> {
    if channel_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let mut builder = QueryBuilder::<Sqlite>::new(
        "SELECT id, name, is_voice FROM channels WHERE deleted_at IS NULL AND id IN (",
    );
    push_bind_list(&mut builder, channel_ids);
    builder.push(")");

    let rows = builder.build().fetch_all(&db.0).await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let id: String = row.get("id");
            (
                id.clone(),
                BridgeResolvedChannel {
                    id,
                    name: Some(row.get("name")),
                    is_voice: Some(row.get::<i64, _>("is_voice") != 0),
                },
            )
        })
        .collect())
}

fn push_bind_list<'a>(builder: &mut QueryBuilder<'a, Sqlite>, values: &'a BTreeSet<String>) {
    let mut separated = builder.separated(", ");
    for value in values {
        separated.push_bind(value);
    }
    separated.push_unseparated("");
}
