use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use crate::{db::Db, errors::ApiError, auth::AuthUser};
use chrono::Utc;

#[derive(Serialize)]
struct ChannelResp {
    id: String, name: String, is_voice: bool, is_private: bool
}

pub async fn list_channels(db: web::Data<Db>, user: AuthUser) -> Result<HttpResponse, ApiError> {
    let rows = sqlx::query("SELECT c.id, c.name, c.is_voice, c.is_private FROM channels c
        INNER JOIN channel_members m ON m.channel_id = c.id
        WHERE m.user_id = ? AND c.deleted_at IS NULL")
        .bind(&user.user_id)
        .fetch_all(&db.0).await?;
    let list: Vec<ChannelResp> = rows.into_iter().map(|r| ChannelResp {
        id: r.get("id"),
        name: r.get::<String,_>("name"),
        is_voice: r.get::<i64,_>("is_voice") != 0,
        is_private: r.get::<i64,_>("is_private") != 0,
    }).collect();
    Ok(HttpResponse::Ok().json(list))
}

#[derive(Deserialize)]
pub struct CreateChannelReq { pub name: String, pub is_voice: Option<bool>, pub is_private: Option<bool> }

pub async fn create_channel(db: web::Data<Db>, user: AuthUser, body: web::Json<CreateChannelReq>) -> Result<HttpResponse, ApiError> {
    if body.name.trim().is_empty() { return Err(ApiError::BadRequest("name required".into())); }
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now();
    sqlx::query("INSERT INTO channels(id,name,is_voice,is_private,created_by,created_at) VALUES (?,?,?,?,?,?)")
        .bind(&id)
        .bind(&body.name)
        .bind(body.is_voice.unwrap_or(false))
        .bind(body.is_private.unwrap_or(false))
        .bind(&user.user_id)
        .bind(now)
        .execute(&db.0).await?;
    // make creator a manager
    sqlx::query("INSERT INTO channel_members(channel_id, user_id, can_read, can_write, can_manage) VALUES (?, ?, 1, 1, 1)")
        .bind(&id)
        .bind(&user.user_id)
        .execute(&db.0).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({"id": id})))
}

pub async fn delete_channel(db: web::Data<Db>, user: AuthUser, path: web::Path<String>) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    // verify can_manage
    let row = sqlx::query("SELECT can_manage FROM channel_members WHERE channel_id = ? AND user_id = ?")
        .bind(&id)
        .bind(&user.user_id)
        .fetch_optional(&db.0).await?;
    let row = row.ok_or(ApiError::Forbidden)?;
    let can_manage: i64 = row.get("can_manage");
    if can_manage == 0 { return Err(ApiError::Forbidden); }

    sqlx::query("UPDATE channels SET deleted_at = ? WHERE id = ?")
        .bind(chrono::Utc::now())
        .bind(&id)
        .execute(&db.0).await?;
    Ok(HttpResponse::Ok().finish())
}

pub async fn join_channel(db: web::Data<Db>, user: AuthUser, path: web::Path<String>) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    // For private channels you’d enforce an invite; for now, allow if not private
    let row = sqlx::query("SELECT is_private FROM channels WHERE id = ? AND deleted_at IS NULL")
        .bind(&id).fetch_optional(&db.0).await?;
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

pub async fn leave_channel(db: web::Data<Db>, user: AuthUser, path: web::Path<String>) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    sqlx::query("DELETE FROM channel_members WHERE channel_id = ? AND user_id = ?")
        .bind(&id)
        .bind(&user.user_id)
        .execute(&db.0).await?;
    Ok(HttpResponse::Ok().finish())
}

pub async fn list_members(db: web::Data<Db>, user: AuthUser, path: web::Path<String>) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    // require read membership
    let r = sqlx::query("SELECT 1 FROM channel_members WHERE channel_id = ? AND user_id = ? AND can_read = 1")
        .bind(&id)
        .bind(&user.user_id)
        .fetch_optional(&db.0).await?;
    if r.is_none() { return Err(ApiError::Forbidden); }

    let rows = sqlx::query("SELECT user_id, can_read, can_write, can_manage FROM channel_members WHERE channel_id = ?")
        .bind(&id).fetch_all(&db.0).await?;
    let members: Vec<_> = rows.into_iter().map(|r| serde_json::json!({
        "user_id": r.get::<String,_>("user_id"),
        "can_read": r.get::<i64,_>("can_read") != 0,
        "can_write": r.get::<i64,_>("can_write") != 0,
        "can_manage": r.get::<i64,_>("can_manage") != 0,
    })).collect();
    Ok(HttpResponse::Ok().json(members))
}

// Admin/manager: add or remove users
#[derive(Deserialize)]
pub struct ModifyMembersReq {
    pub add: Option<Vec<String>>,
    pub remove: Option<Vec<String>>,
}

pub async fn modify_members(db: web::Data<Db>, user: AuthUser, path: web::Path<String>, body: web::Json<ModifyMembersReq>) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    let row = sqlx::query("SELECT can_manage FROM channel_members WHERE channel_id = ? AND user_id = ?")
        .bind(&id).bind(&user.user_id).fetch_optional(&db.0).await?;
    let row = row.ok_or(ApiError::Forbidden)?;
    if row.get::<i64,_>("can_manage") == 0 { return Err(ApiError::Forbidden); }

    if let Some(add) = &body.add {
        for uid in add {
            sqlx::query("INSERT OR IGNORE INTO channel_members(channel_id, user_id, can_read, can_write, can_manage) VALUES (?, ?, 1, 1, 0)")
                .bind(&id).bind(uid).execute(&db.0).await?;
        }
    }
    if let Some(remove) = &body.remove {
        for uid in remove {
            sqlx::query("DELETE FROM channel_members WHERE channel_id = ? AND user_id = ?")
                .bind(&id).bind(uid).execute(&db.0).await?;
        }
    }
    Ok(HttpResponse::Ok().finish())
}