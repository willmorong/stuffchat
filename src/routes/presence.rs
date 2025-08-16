use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use chrono::{Utc, Duration};
use crate::{db::Db, errors::ApiError, auth::AuthUser};

const OFFLINE_AFTER_SECS: i64 = 60;

#[derive(Deserialize)]
pub struct HeartbeatReq {
    pub status: Option<String>, // 'online' | 'away' | 'dnd' | 'invisible' | 'offline'
}

pub async fn heartbeat(db: web::Data<Db>, user: AuthUser, body: web::Json<HeartbeatReq>) -> Result<HttpResponse, ApiError> {
    let now = Utc::now();
    let status = body.status.as_deref().unwrap_or("online");
    // Basic whitelist
    let status = match status {
        "online" | "away" | "dnd" | "invisible" | "offline" => status,
        _ => "online",
    };
    sqlx::query(
        "INSERT INTO presence(user_id, last_heartbeat, status, updated_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(user_id) DO UPDATE SET last_heartbeat = excluded.last_heartbeat, status = excluded.status, updated_at = excluded.updated_at"
    )
    .bind(&user.user_id).bind(now).bind(status).bind(now)
    .execute(&db.0).await?;
    Ok(HttpResponse::Ok().finish())
}

#[derive(Deserialize)]
pub struct UsersQuery {
    pub ids: Option<String>, // comma-separated user ids; if omitted -> self
}

#[derive(Serialize)]
pub struct PresenceResp {
    pub user_id: String,
    pub status: String, // computed
    pub last_heartbeat: chrono::DateTime<chrono::Utc>,
}

pub async fn get_users_presence(db: web::Data<Db>, user: AuthUser, q: web::Query<UsersQuery>) -> Result<HttpResponse, ApiError> {
    let ids: Vec<String> = if let Some(ids) = &q.ids {
        ids.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    } else {
        vec![user.user_id.clone()]
    };
    if ids.is_empty() {
        return Ok(HttpResponse::Ok().json(Vec::<PresenceResp>::new()));
    }

    // Use a simple IN clause. For many IDs, consider temp table join.
    let placeholders = std::iter::repeat("?").take(ids.len()).collect::<Vec<_>>().join(",");
    let sql = format!("SELECT user_id, last_heartbeat, status FROM presence WHERE user_id IN ({})", placeholders);
    let mut query = sqlx::query(&sql);
    for id in &ids { query = query.bind(id); }
    let rows = query.fetch_all(&db.0).await?;

    let now = Utc::now();
    let mut map = std::collections::HashMap::<String, PresenceResp>::new();
    for r in rows {
        let uid: String = r.get("user_id");
        let last: chrono::DateTime<chrono::Utc> = r.get("last_heartbeat");
        let desired: String = r.get::<String,_>("status");
        // Compute offline if stale
        let computed = if now - last > Duration::seconds(OFFLINE_AFTER_SECS) {
            "offline".to_string()
        } else {
            desired
        };
        map.insert(uid.clone(), PresenceResp { user_id: uid, status: computed, last_heartbeat: last });
    }
    // Fill missing users as offline
    for uid in ids {
        map.entry(uid.clone()).or_insert(PresenceResp {
            user_id: uid.clone(),
            status: "offline".into(),
            last_heartbeat: now - Duration::seconds(OFFLINE_AFTER_SECS + 1),
        });
    }

    let mut out: Vec<_> = map.into_values().collect();
    out.sort_by(|a,b| a.user_id.cmp(&b.user_id));
    Ok(HttpResponse::Ok().json(out))
}