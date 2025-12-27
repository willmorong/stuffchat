use crate::{auth, db::Db, errors::ApiError};
use actix_web::{HttpResponse, web};
use serde::Serialize;
use sqlx::Row;

#[derive(Serialize)]
pub struct InviteResp {
    code: String,
    created_by: String,
    joined_user_id: Option<String>,
    joined_username: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn create_invite(
    db: web::Data<Db>,
    auth: auth::AuthUser,
) -> Result<HttpResponse, ApiError> {
    let code = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now();

    sqlx::query("INSERT INTO invites(code, created_by, created_at) VALUES (?, ?, ?)")
        .bind(&code)
        .bind(&auth.user_id)
        .bind(now)
        .execute(&db.0)
        .await?;

    Ok(HttpResponse::Ok().json(InviteResp {
        code,
        created_by: auth.user_id,
        joined_user_id: None,
        joined_username: None,
        created_at: now,
    }))
}

pub async fn list_my_invites(
    db: web::Data<Db>,
    auth: auth::AuthUser,
) -> Result<HttpResponse, ApiError> {
    let rows = sqlx::query(
        "SELECT i.code, i.created_by, i.joined_user_id, i.created_at, u.username as joined_username 
         FROM invites i 
         LEFT JOIN users u ON i.joined_user_id = u.id 
         WHERE i.created_by = ? 
         ORDER BY i.created_at DESC"
    )
    .bind(&auth.user_id)
    .fetch_all(&db.0).await?;

    let invites: Vec<InviteResp> = rows
        .into_iter()
        .map(|r| InviteResp {
            code: r.get("code"),
            created_by: r.get("created_by"),
            joined_user_id: r.get("joined_user_id"),
            joined_username: r.get("joined_username"),
            created_at: r.get("created_at"),
        })
        .collect();

    Ok(HttpResponse::Ok().json(invites))
}
