use crate::{auth::AuthUser, db::Db, errors::ApiError};
use actix_web::{HttpResponse, web};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[derive(Serialize)]
struct ChannelResp {
    id: String,
    name: String,
    is_voice: bool,
    is_private: bool,
    is_owner: bool,
    last_message_at: Option<chrono::DateTime<Utc>>,
}

pub async fn list_channels(db: web::Data<Db>, user: AuthUser) -> Result<HttpResponse, ApiError> {
    let rows = sqlx::query(
        "SELECT c.id, c.name, c.is_voice, c.is_private, c.created_by,
        (SELECT created_at FROM messages WHERE channel_id = c.id AND deleted_at IS NULL ORDER BY created_at DESC LIMIT 1) as last_message_at
        FROM channels c
        INNER JOIN channel_members m ON m.channel_id = c.id
        WHERE m.user_id = ? AND c.deleted_at IS NULL",
    )
    .bind(&user.user_id)
    .fetch_all(&db.0)
    .await?;
    let list: Vec<ChannelResp> = rows
        .into_iter()
        .map(|r| ChannelResp {
            id: r.get("id"),
            name: r.get::<String, _>("name"),
            is_voice: r.get::<i64, _>("is_voice") != 0,
            is_private: r.get::<i64, _>("is_private") != 0,
            is_owner: r.get::<String, _>("created_by") == user.user_id,
            last_message_at: r.get("last_message_at"),
        })
        .collect();
    Ok(HttpResponse::Ok().json(list))
}

#[derive(Serialize)]
struct UnreadState {
    channel_id: String,
    last_read_message_id: Option<String>,
    last_read_at: Option<chrono::DateTime<Utc>>,
}

pub async fn get_unread(db: web::Data<Db>, user: AuthUser) -> Result<HttpResponse, ApiError> {
    let rows = sqlx::query(
        "SELECT channel_id, last_read_message_id, last_read_at FROM channel_unread WHERE user_id = ?"
    )
    .bind(&user.user_id)
    .fetch_all(&db.0)
    .await?;

    let list: Vec<UnreadState> = rows
        .into_iter()
        .map(|r| UnreadState {
            channel_id: r.get("channel_id"),
            last_read_message_id: r.get("last_read_message_id"),
            last_read_at: r.get("last_read_at"),
        })
        .collect();

    Ok(HttpResponse::Ok().json(list))
}

#[derive(Deserialize)]
pub struct MarkReadReq {
    pub message_id: String,
}

pub async fn mark_read(
    db: web::Data<Db>,
    user: AuthUser,
    path: web::Path<String>,
    body: web::Json<MarkReadReq>,
) -> Result<HttpResponse, ApiError> {
    let channel_id = path.into_inner();
    let now = Utc::now();

    // Verify user is a member of this channel
    let membership =
        sqlx::query("SELECT can_read FROM channel_members WHERE channel_id = ? AND user_id = ?")
            .bind(&channel_id)
            .bind(&user.user_id)
            .fetch_optional(&db.0)
            .await?;
    if membership.is_none() {
        return Err(ApiError::Forbidden);
    }

    // Get message created_at
    let row = sqlx::query("SELECT created_at FROM messages WHERE id = ?")
        .bind(&body.message_id)
        .fetch_optional(&db.0)
        .await?;

    let created_at: chrono::DateTime<Utc> = match row {
        Some(r) => r.get("created_at"),
        None => now, // Fallback if message not found? Should effectively be "now"
    };

    sqlx::query(
        "INSERT INTO channel_unread (channel_id, user_id, last_read_message_id, last_read_at, updated_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(channel_id, user_id) DO UPDATE SET last_read_message_id = ?, last_read_at = ?, updated_at = ?"
    )
    .bind(&channel_id).bind(&user.user_id).bind(&body.message_id).bind(created_at).bind(now)
    .bind(&body.message_id).bind(created_at).bind(now)
    .execute(&db.0).await?;

    Ok(HttpResponse::Ok().finish())
}

#[derive(Deserialize)]
pub struct MarkNotifiedReq {
    pub message_id: String,
}

pub async fn mark_notified(
    db: web::Data<Db>,
    user: AuthUser,
    path: web::Path<String>,
    body: web::Json<MarkNotifiedReq>,
) -> Result<HttpResponse, ApiError> {
    let channel_id = path.into_inner();
    let now = Utc::now();

    // Verify user is a member of this channel
    let membership =
        sqlx::query("SELECT can_read FROM channel_members WHERE channel_id = ? AND user_id = ?")
            .bind(&channel_id)
            .bind(&user.user_id)
            .fetch_optional(&db.0)
            .await?;
    if membership.is_none() {
        return Err(ApiError::Forbidden);
    }

    sqlx::query(
        "INSERT INTO channel_unread (channel_id, user_id, last_notified_message_id, updated_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(channel_id, user_id) DO UPDATE SET last_notified_message_id = ?, updated_at = ?"
    )
    .bind(&channel_id).bind(&user.user_id).bind(&body.message_id).bind(now)
    .bind(&body.message_id).bind(now)
    .execute(&db.0).await?;

    Ok(HttpResponse::Ok().finish())
}

pub async fn check_ownership(
    db: web::Data<Db>,
    user: AuthUser,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let channel_id = path.into_inner();
    let row = sqlx::query("SELECT created_by FROM channels WHERE id = ? AND deleted_at IS NULL")
        .bind(&channel_id)
        .fetch_optional(&db.0)
        .await?;
    let row = row.ok_or(ApiError::NotFound)?;
    let created_by: String = row.get("created_by");
    Ok(HttpResponse::Ok().json(serde_json::json!({ "is_owner": created_by == user.user_id })))
}

#[derive(Deserialize)]
pub struct EditChannelReq {
    pub name: Option<String>,
    pub is_voice: Option<bool>,
    pub is_private: Option<bool>,
}

pub async fn edit_channel(
    db: web::Data<Db>,
    user: AuthUser,
    path: web::Path<String>,
    body: web::Json<EditChannelReq>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();

    // verify ownership
    let row = sqlx::query("SELECT created_by FROM channels WHERE id = ? AND deleted_at IS NULL")
        .bind(&id)
        .fetch_optional(&db.0)
        .await?;
    let row = row.ok_or(ApiError::NotFound)?;
    let created_by: String = row.get("created_by");
    if created_by != user.user_id {
        return Err(ApiError::Forbidden);
    }

    let mut query = String::from("UPDATE channels SET ");
    let mut params = Vec::new();
    let mut updates = Vec::new();

    if let Some(name) = &body.name {
        if name.trim().is_empty() {
            return Err(ApiError::BadRequest("name cannot be empty".into()));
        }
        updates.push("name = ?");
        params.push(name.clone());
    }
    if let Some(is_voice) = body.is_voice {
        updates.push("is_voice = ?");
        params.push(if is_voice { "1" } else { "0" }.to_string());
    }
    if let Some(is_private) = body.is_private {
        updates.push("is_private = ?");
        params.push(if is_private { "1" } else { "0" }.to_string());
    }

    if updates.is_empty() {
        return Ok(HttpResponse::Ok().finish());
    }

    query.push_str(&updates.join(", "));
    query.push_str(" WHERE id = ?");

    let mut q = sqlx::query(&query);
    for p in params {
        if p == "1" || p == "0" {
            q = q.bind(p == "1");
        } else {
            q = q.bind(p);
        }
    }
    q = q.bind(&id);
    q.execute(&db.0).await?;

    Ok(HttpResponse::Ok().finish())
}

#[derive(Deserialize)]
pub struct CreateChannelReq {
    pub name: String,
    pub is_voice: Option<bool>,
    pub is_private: Option<bool>,
    // Optional list of user IDs to add as initial members (used for private channels)
    pub members: Option<Vec<String>>,
}

pub async fn create_channel(
    db: web::Data<Db>,
    user: AuthUser,
    body: web::Json<CreateChannelReq>,
) -> Result<HttpResponse, ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name required".into()));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now();
    let is_private = body.is_private.unwrap_or(false);

    let mut tx = db.0.begin().await?;

    sqlx::query("INSERT INTO channels(id,name,is_voice,is_private,created_by,created_at) VALUES (?,?,?,?,?,?)")
        .bind(&id)
        .bind(&body.name)
        .bind(body.is_voice.unwrap_or(false))
        .bind(is_private)
        .bind(&user.user_id)
        .bind(now)
        .execute(&mut *tx).await?;

    if is_private {
        // For private channels, add the creator as a manager.
        sqlx::query("INSERT INTO channel_members(channel_id, user_id, can_read, can_write, can_manage) VALUES (?, ?, 1, 1, 1)")
            .bind(&id)
            .bind(&user.user_id)
            .execute(&mut *tx).await?;

        // Add any specified members (non-managers by default). Ignore the creator if included.
        if let Some(members) = &body.members {
            for uid in members {
                if uid == &user.user_id {
                    continue;
                }
                sqlx::query("INSERT OR IGNORE INTO channel_members(channel_id, user_id, can_read, can_write, can_manage) VALUES (?, ?, 1, 1, 0)")
                    .bind(&id)
                    .bind(uid)
                    .execute(&mut *tx).await?;
            }
        }
    } else {
        // For public channels, add all non-creator users as members.
        sqlx::query("INSERT INTO channel_members(channel_id, user_id, can_read, can_write, can_manage) SELECT ?, id, 1, 1, 0 FROM users WHERE id != ?")
            .bind(&id)
            .bind(&user.user_id)
            .execute(&mut *tx).await?;
        // And add the creator as a manager.
        sqlx::query("INSERT INTO channel_members(channel_id, user_id, can_read, can_write, can_manage) VALUES (?, ?, 1, 1, 1)")
            .bind(&id)
            .bind(&user.user_id)
            .execute(&mut *tx).await?;
    }

    tx.commit().await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({"id": id})))
}

pub async fn delete_channel(
    db: web::Data<Db>,
    user: AuthUser,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    // verify can_manage
    let row =
        sqlx::query("SELECT can_manage FROM channel_members WHERE channel_id = ? AND user_id = ?")
            .bind(&id)
            .bind(&user.user_id)
            .fetch_optional(&db.0)
            .await?;
    let row = row.ok_or(ApiError::Forbidden)?;
    let can_manage: i64 = row.get("can_manage");
    if can_manage == 0 {
        return Err(ApiError::Forbidden);
    }

    sqlx::query("UPDATE channels SET deleted_at = ? WHERE id = ?")
        .bind(chrono::Utc::now())
        .bind(&id)
        .execute(&db.0)
        .await?;
    Ok(HttpResponse::Ok().finish())
}

pub async fn join_channel(
    db: web::Data<Db>,
    user: AuthUser,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    // For private channels you’d enforce an invite; for now, allow if not private
    let row = sqlx::query("SELECT is_private FROM channels WHERE id = ? AND deleted_at IS NULL")
        .bind(&id)
        .fetch_optional(&db.0)
        .await?;
    let row = row.ok_or(ApiError::NotFound)?;
    let is_private: i64 = row.get("is_private");
    if is_private != 0 {
        return Err(ApiError::Forbidden);
    }
    sqlx::query("INSERT OR IGNORE INTO channel_members(channel_id, user_id, can_read, can_write, can_manage) VALUES (?, ?, 1, 1, 0)")
        .bind(&id)
        .bind(&user.user_id)
        .execute(&db.0).await?;
    Ok(HttpResponse::Ok().finish())
}

pub async fn leave_channel(
    db: web::Data<Db>,
    user: AuthUser,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    sqlx::query("DELETE FROM channel_members WHERE channel_id = ? AND user_id = ?")
        .bind(&id)
        .bind(&user.user_id)
        .execute(&db.0)
        .await?;
    Ok(HttpResponse::Ok().finish())
}

pub async fn list_members(
    db: web::Data<Db>,
    user: AuthUser,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    // require read membership
    let r = sqlx::query(
        "SELECT 1 FROM channel_members WHERE channel_id = ? AND user_id = ? AND can_read = 1",
    )
    .bind(&id)
    .bind(&user.user_id)
    .fetch_optional(&db.0)
    .await?;
    if r.is_none() {
        return Err(ApiError::Forbidden);
    }

    let rows = sqlx::query(
        "SELECT user_id, can_read, can_write, can_manage FROM channel_members WHERE channel_id = ?",
    )
    .bind(&id)
    .fetch_all(&db.0)
    .await?;
    let members: Vec<_> = rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "user_id": r.get::<String,_>("user_id"),
                "can_read": r.get::<i64,_>("can_read") != 0,
                "can_write": r.get::<i64,_>("can_write") != 0,
                "can_manage": r.get::<i64,_>("can_manage") != 0,
            })
        })
        .collect();
    Ok(HttpResponse::Ok().json(members))
}

// Admin/manager: add or remove users
#[derive(Deserialize)]
pub struct ModifyMembersReq {
    pub add: Option<Vec<String>>,
    pub remove: Option<Vec<String>>,
}

pub async fn modify_members(
    db: web::Data<Db>,
    user: AuthUser,
    path: web::Path<String>,
    body: web::Json<ModifyMembersReq>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    let row =
        sqlx::query("SELECT can_manage FROM channel_members WHERE channel_id = ? AND user_id = ?")
            .bind(&id)
            .bind(&user.user_id)
            .fetch_optional(&db.0)
            .await?;
    let row = row.ok_or(ApiError::Forbidden)?;
    if row.get::<i64, _>("can_manage") == 0 {
        return Err(ApiError::Forbidden);
    }

    if let Some(add) = &body.add {
        for uid in add {
            sqlx::query("INSERT OR IGNORE INTO channel_members(channel_id, user_id, can_read, can_write, can_manage) VALUES (?, ?, 1, 1, 0)")
                .bind(&id).bind(uid).execute(&db.0).await?;
        }
    }
    if let Some(remove) = &body.remove {
        for uid in remove {
            sqlx::query("DELETE FROM channel_members WHERE channel_id = ? AND user_id = ?")
                .bind(&id)
                .bind(uid)
                .execute(&db.0)
                .await?;
        }
    }
    Ok(HttpResponse::Ok().finish())
}
