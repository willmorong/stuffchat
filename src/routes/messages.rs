use crate::{auth::AuthUser, db::Db, errors::ApiError, ws::server::Broadcast};
use actix_web::{HttpResponse, web};
use chrono::Utc;
use serde::Deserialize;
use sqlx::Row;

#[derive(Deserialize)]
pub struct ListQuery {
    pub before: Option<String>,
    pub limit: Option<i64>,
}

pub async fn list_messages(
    db: web::Data<Db>,
    user: AuthUser,
    path: web::Path<String>,
    q: web::Query<ListQuery>,
) -> Result<HttpResponse, ApiError> {
    let channel_id = path.into_inner();
    let m =
        sqlx::query("SELECT can_read FROM channel_members WHERE channel_id = ? AND user_id = ?")
            .bind(&channel_id)
            .bind(&user.user_id)
            .fetch_optional(&db.0)
            .await?;
    let m = m.ok_or(ApiError::Forbidden)?;
    if m.get::<i64, _>("can_read") == 0 {
        return Err(ApiError::Forbidden);
    }

    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let rows = if let Some(before_id) = &q.before {
        // Get created_at of before_id for pagination
        let ref_row =
            sqlx::query("SELECT created_at FROM messages WHERE id = ? AND channel_id = ?")
                .bind(before_id)
                .bind(&channel_id)
                .fetch_optional(&db.0)
                .await?;
        let ts: chrono::DateTime<chrono::Utc> =
            ref_row.map(|r| r.get("created_at")).unwrap_or(Utc::now());
        sqlx::query(
            "SELECT m.id, m.user_id, m.content, m.file_id, m.created_at, m.edited_at, f.original_name
             FROM messages m
             LEFT JOIN files f ON f.id = m.file_id
             WHERE m.channel_id = ? AND m.deleted_at IS NULL AND m.created_at < ?
             ORDER BY m.created_at DESC LIMIT ?"
        )
            .bind(&channel_id).bind(ts).bind(limit).fetch_all(&db.0).await?
    } else {
        sqlx::query(
            "SELECT m.id, m.user_id, m.content, m.file_id, m.created_at, m.edited_at, f.original_name
             FROM messages m
             LEFT JOIN files f ON f.id = m.file_id
             WHERE m.channel_id = ? AND m.deleted_at IS NULL
             ORDER BY m.created_at DESC LIMIT ?"
        )
            .bind(&channel_id).bind(limit).fetch_all(&db.0).await?
    };

    let msgs: Vec<_> = rows
        .into_iter()
        .map(|r| {
            let file_id: Option<String> = r.get("file_id");
            let original_name: Option<String> = r.get("original_name");
            let file_url = match (file_id.as_deref(), original_name.as_deref()) {
                (Some(fid), Some(name)) => Some(format!("/files/{}/{}", fid, name)),
                _ => None,
            };

            serde_json::json!({
                "id": r.get::<String,_>("id"),
                "channel_id": channel_id,
                "user_id": r.get::<String,_>("user_id"),
                "content": r.get::<Option<String>,_>("content"),
                "file_url": file_url,
                "created_at": r.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),
                "edited_at": r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("edited_at"),
            })
        })
        .collect();

    Ok(HttpResponse::Ok().json(msgs))
}

#[derive(Deserialize)]
pub struct PostMessageReq {
    pub content: Option<String>,
    pub file_id: Option<String>,
}

pub async fn post_message(
    db: web::Data<Db>,
    chat: web::Data<actix::Addr<crate::ws::server::ChatServer>>,
    user: AuthUser,
    path: web::Path<String>,
    body: web::Json<PostMessageReq>,
) -> Result<HttpResponse, ApiError> {
    let channel_id = path.into_inner();
    let m =
        sqlx::query("SELECT can_write FROM channel_members WHERE channel_id = ? AND user_id = ?")
            .bind(&channel_id)
            .bind(&user.user_id)
            .fetch_optional(&db.0)
            .await?;
    let m = m.ok_or(ApiError::Forbidden)?;
    if m.get::<i64, _>("can_write") == 0 {
        return Err(ApiError::Forbidden);
    }

    if body
        .content
        .as_deref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
        && body.file_id.is_none()
    {
        return Err(ApiError::BadRequest(
            "message must have content or file".into(),
        ));
    }

    // Resolve original filename for broadcast (if a file is attached)
    let file_url = if let Some(fid) = &body.file_id {
        let row = sqlx::query("SELECT original_name FROM files WHERE id = ?")
            .bind(fid)
            .fetch_optional(&db.0)
            .await?;
        row.map(|r| {
            let original: String = r.get("original_name");
            format!("/files/{}/{}", fid, original)
        })
    } else {
        None
    };

    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now();
    sqlx::query("INSERT INTO messages(id, channel_id, user_id, content, file_id, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&id).bind(&channel_id).bind(&user.user_id).bind(&body.content).bind(&body.file_id).bind(now)
        .execute(&db.0).await?;

    // Broadcast to WS
    // Broadcast to WS
    let payload = serde_json::json!({
        "type": "message_created",
        "id": id,
        "channel_id": channel_id,
        "user_id": user.user_id,
        "content": body.content,
        "file_url": file_url,
        "created_at": now,
    })
    .to_string();
    chat.do_send(Broadcast {
        channel_id: channel_id.clone(),
        payload: payload.clone(),
    });

    // Notify other members (skipping those in the channel room)
    let member_rows = sqlx::query("SELECT user_id FROM channel_members WHERE channel_id = ?")
        .bind(&channel_id)
        .fetch_all(&db.0)
        .await?;
    let member_ids: Vec<String> = member_rows
        .into_iter()
        .map(|r| r.get("user_id"))
        .filter(|uid| uid != &user.user_id)
        .collect();

    if !member_ids.is_empty() {
        chat.do_send(crate::ws::server::NotifyUsers {
            user_ids: member_ids,
            payload,
            skip_channel: Some(channel_id.clone()),
        });
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({ "id": id })))
}

#[derive(Deserialize)]
pub struct EditMessageReq {
    pub content: String,
}

pub async fn edit_message(
    db: web::Data<Db>,
    chat: web::Data<actix::Addr<crate::ws::server::ChatServer>>,
    user: AuthUser,
    path: web::Path<String>,
    body: web::Json<EditMessageReq>,
) -> Result<HttpResponse, ApiError> {
    if body.content.trim().is_empty() {
        return Err(ApiError::BadRequest("content required".into()));
    }

    let id = path.into_inner();
    // Load message with channel and author
    let row =
        sqlx::query("SELECT channel_id, user_id FROM messages WHERE id = ? AND deleted_at IS NULL")
            .bind(&id)
            .fetch_optional(&db.0)
            .await?;
    let row = row.ok_or(ApiError::NotFound)?;
    let channel_id: String = row.get("channel_id");
    let author_id: String = row.get("user_id");

    // Permission: author or channel manager
    let can_manage =
        sqlx::query("SELECT can_manage FROM channel_members WHERE channel_id = ? AND user_id = ?")
            .bind(&channel_id)
            .bind(&user.user_id)
            .fetch_optional(&db.0)
            .await?
            .map(|r| r.get::<i64, _>("can_manage") != 0)
            .unwrap_or(false);
    if user.user_id != author_id && !can_manage {
        return Err(ApiError::Forbidden);
    }

    let now = Utc::now();
    sqlx::query("UPDATE messages SET content = ?, edited_at = ? WHERE id = ?")
        .bind(&body.content)
        .bind(now)
        .bind(&id)
        .execute(&db.0)
        .await?;

    // Broadcast update
    let payload = serde_json::json!({
        "type": "message_edited",
        "id": id,
        "channel_id": channel_id,
        "content": body.content,
        "edited_at": now,
    })
    .to_string();
    chat.do_send(Broadcast {
        channel_id: channel_id.clone(),
        payload,
    });

    Ok(HttpResponse::Ok().finish())
}

pub async fn delete_message(
    db: web::Data<Db>,
    chat: web::Data<actix::Addr<crate::ws::server::ChatServer>>,
    user: AuthUser,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    let row =
        sqlx::query("SELECT channel_id, user_id FROM messages WHERE id = ? AND deleted_at IS NULL")
            .bind(&id)
            .fetch_optional(&db.0)
            .await?;
    let row = row.ok_or(ApiError::NotFound)?;
    let channel_id: String = row.get("channel_id");
    let author_id: String = row.get("user_id");

    // Permission: author or channel manager
    let can_manage =
        sqlx::query("SELECT can_manage FROM channel_members WHERE channel_id = ? AND user_id = ?")
            .bind(&channel_id)
            .bind(&user.user_id)
            .fetch_optional(&db.0)
            .await?
            .map(|r| r.get::<i64, _>("can_manage") != 0)
            .unwrap_or(false);
    if user.user_id != author_id && !can_manage {
        return Err(ApiError::Forbidden);
    }

    let now = Utc::now();
    sqlx::query("UPDATE messages SET deleted_at = ? WHERE id = ?")
        .bind(now)
        .bind(&id)
        .execute(&db.0)
        .await?;

    // Broadcast deletion
    let payload = serde_json::json!({
        "type": "message_deleted",
        "id": id,
        "channel_id": channel_id,
        "deleted_at": now,
    })
    .to_string();
    chat.do_send(Broadcast {
        channel_id: channel_id.clone(),
        payload,
    });

    Ok(HttpResponse::Ok().finish())
}
