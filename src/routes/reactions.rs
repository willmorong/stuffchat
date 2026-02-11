use crate::{auth::AuthUser, db::Db, errors::ApiError, ws::server::Broadcast};
use actix_web::{HttpResponse, web};
use chrono::Utc;
use sqlx::Row;

pub async fn toggle_reaction(
    db: web::Data<Db>,
    chat: web::Data<actix::Addr<crate::ws::server::ChatServer>>,
    user: AuthUser,
    path: web::Path<(String, String)>,
) -> Result<HttpResponse, ApiError> {
    let (message_id, emoji) = path.into_inner();

    // Decode emoji (it comes URL-encoded)
    let emoji = urlencoding::decode(&emoji)
        .map(|s| s.into_owned())
        .unwrap_or(emoji);

    // Look up message to get channel_id
    let row = sqlx::query("SELECT channel_id FROM messages WHERE id = ? AND deleted_at IS NULL")
        .bind(&message_id)
        .fetch_optional(&db.0)
        .await?;
    let row = row.ok_or(ApiError::NotFound)?;
    let channel_id: String = row.get("channel_id");

    // Check user can read the channel
    let perm =
        sqlx::query("SELECT can_read FROM channel_members WHERE channel_id = ? AND user_id = ?")
            .bind(&channel_id)
            .bind(&user.user_id)
            .fetch_optional(&db.0)
            .await?;
    let perm = perm.ok_or(ApiError::Forbidden)?;
    if perm.get::<i64, _>("can_read") == 0 {
        return Err(ApiError::Forbidden);
    }

    // Check if reaction already exists
    let existing = sqlx::query(
        "SELECT 1 FROM message_reactions WHERE message_id = ? AND user_id = ? AND emoji = ?",
    )
    .bind(&message_id)
    .bind(&user.user_id)
    .bind(&emoji)
    .fetch_optional(&db.0)
    .await?;

    if existing.is_some() {
        // Remove reaction
        sqlx::query(
            "DELETE FROM message_reactions WHERE message_id = ? AND user_id = ? AND emoji = ?",
        )
        .bind(&message_id)
        .bind(&user.user_id)
        .bind(&emoji)
        .execute(&db.0)
        .await?;
    } else {
        // Add reaction
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO message_reactions (message_id, user_id, emoji, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&message_id)
        .bind(&user.user_id)
        .bind(&emoji)
        .bind(now)
        .execute(&db.0)
        .await?;
    }

    // Build updated reactions list for this message
    let reactions = build_reactions(&db, &message_id).await?;

    // Broadcast to channel
    let payload = serde_json::json!({
        "type": "reaction_updated",
        "message_id": message_id,
        "channel_id": channel_id,
        "reactions": reactions,
    })
    .to_string();
    chat.do_send(Broadcast {
        channel_id: channel_id.clone(),
        payload,
    });

    Ok(HttpResponse::Ok().json(serde_json::json!({ "reactions": reactions })))
}

pub async fn list_reactions(
    db: web::Data<Db>,
    user: AuthUser,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let message_id = path.into_inner();

    // Look up message to get channel_id
    let row = sqlx::query("SELECT channel_id FROM messages WHERE id = ? AND deleted_at IS NULL")
        .bind(&message_id)
        .fetch_optional(&db.0)
        .await?;
    let row = row.ok_or(ApiError::NotFound)?;
    let channel_id: String = row.get("channel_id");

    // Check user can read the channel
    let perm =
        sqlx::query("SELECT can_read FROM channel_members WHERE channel_id = ? AND user_id = ?")
            .bind(&channel_id)
            .bind(&user.user_id)
            .fetch_optional(&db.0)
            .await?;
    let perm = perm.ok_or(ApiError::Forbidden)?;
    if perm.get::<i64, _>("can_read") == 0 {
        return Err(ApiError::Forbidden);
    }

    let reactions = build_reactions(&db, &message_id).await?;
    Ok(HttpResponse::Ok().json(reactions))
}

/// Build grouped reactions for a message: [{ emoji, users: [user_id, ...], count }]
async fn build_reactions(db: &Db, message_id: &str) -> Result<Vec<serde_json::Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT emoji, user_id FROM message_reactions WHERE message_id = ? ORDER BY created_at ASC",
    )
    .bind(message_id)
    .fetch_all(&db.0)
    .await?;

    // Group by emoji preserving insertion order
    let mut order: Vec<String> = Vec::new();
    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for r in rows {
        let emoji: String = r.get("emoji");
        let uid: String = r.get("user_id");
        if !map.contains_key(&emoji) {
            order.push(emoji.clone());
        }
        map.entry(emoji).or_default().push(uid);
    }

    let reactions: Vec<serde_json::Value> = order
        .into_iter()
        .map(|emoji| {
            let users = map.remove(&emoji).unwrap_or_default();
            serde_json::json!({
                "emoji": emoji,
                "users": users,
                "count": users.len(),
            })
        })
        .collect();

    Ok(reactions)
}
