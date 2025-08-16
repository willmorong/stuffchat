use actix_web::{web, HttpResponse};
use serde::Deserialize;
use sqlx::Row;
use crate::{db::Db, errors::ApiError, auth::AuthUser, config::Config, auth};
use actix_multipart::Multipart;
use futures_util::TryStreamExt as _;

pub async fn me(db: web::Data<Db>, user: super::super::auth::AuthUser) -> Result<HttpResponse, ApiError> {
    let row = sqlx::query("SELECT id, username, email, avatar_file_id, created_at, updated_at FROM users WHERE id = ?")
        .bind(&user.user_id)
        .fetch_optional(&db.0).await?;
    let row = row.ok_or(ApiError::NotFound)?;
    let user = serde_json::json!({
        "id": row.get::<String,_>("id"),
        "username": row.get::<String,_>("username"),
        "email": row.get::<Option<String>,_>("email"),
        "avatar_file_id": row.get::<Option<String>,_>("avatar_file_id"),
        "created_at": row.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),
        "updated_at": row.get::<chrono::DateTime<chrono::Utc>,_>("updated_at"),
    });
    Ok(HttpResponse::Ok().json(user))
}

#[derive(Deserialize)]
pub struct UpdateMeReq { pub username: Option<String>, pub email: Option<String> }

pub async fn update_me(db: web::Data<Db>, user: AuthUser, body: web::Json<UpdateMeReq>) -> Result<HttpResponse, ApiError> {
    if body.username.as_deref().map_or(false, |u| u.len() < 3) {
        return Err(ApiError::BadRequest("username too short".into()));
    }
    sqlx::query("UPDATE users SET username = COALESCE(?, username), email = COALESCE(?, email), updated_at = ? WHERE id = ?")
        .bind(&body.username)
        .bind(&body.email)
        .bind(chrono::Utc::now())
        .bind(&user.user_id)
        .execute(&db.0).await?;
    me(db, user).await
}

#[derive(Deserialize)]
pub struct ChangePasswordReq { pub current_password: String, pub new_password: String }

pub async fn change_password(db: web::Data<Db>, user: AuthUser, body: web::Json<ChangePasswordReq>) -> Result<HttpResponse, ApiError> {
    if body.new_password.len() < 8 { return Err(ApiError::BadRequest("new password too short".into())); }
    let row = sqlx::query("SELECT password_hash FROM users WHERE id = ?")
        .bind(&user.user_id)
        .fetch_one(&db.0).await?;
    let hash: String = row.get("password_hash");
    if !auth::verify_password(&hash, &body.current_password) {
        return Err(ApiError::Forbidden);
    }
    let new_hash = auth::hash_password(&body.new_password)?;
    sqlx::query("UPDATE users SET password_hash = ?, updated_at = ? WHERE id = ?")
        .bind(new_hash)
        .bind(chrono::Utc::now())
        .bind(&user.user_id)
        .execute(&db.0).await?;
    Ok(HttpResponse::Ok().finish())
}

pub async fn upload_avatar(cfg: web::Data<Config>, db: web::Data<Db>, user: AuthUser, mut payload: Multipart) -> Result<HttpResponse, ApiError> {
    use crate::routes::files::{save_multipart_file, SavedFile};
    let mut saved: Option<SavedFile> = None;

    while let Some(item) = payload.try_next().await.map_err(|_| ApiError::BadRequest("invalid multipart".into()))? {
        let field = item;
        let s = save_multipart_file(&cfg, &db, &user.user_id, field).await?;
        saved = Some(s);
        break;
    }
    let saved = saved.ok_or(ApiError::BadRequest("no file".into()))?;
    sqlx::query("UPDATE users SET avatar_file_id = ?, updated_at = ? WHERE id = ?")
        .bind(&saved.file_id)
        .bind(chrono::Utc::now())
        .bind(&user.user_id)
        .execute(&db.0).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({"avatar_file_id": saved.file_id})))
}