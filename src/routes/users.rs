use crate::{
    auth,
    auth::AuthUser,
    config::Config,
    db::Db,
    errors::ApiError,
    ws::server::{BroadcastAll, ChatServer},
};
use actix_multipart::Multipart;
use actix_web::{HttpResponse, web};
use futures_util::TryStreamExt as _;
use serde::{Deserialize, Serialize};
use sqlx::Row;

pub async fn me(
    db: web::Data<Db>,
    user: super::super::auth::AuthUser,
) -> Result<HttpResponse, ApiError> {
    let row = sqlx::query("SELECT id, username, email, avatar_file_id, created_at, updated_at FROM users WHERE id = ?")
        .bind(&user.user_id)
        .fetch_optional(&db.0).await?;
    let row = row.ok_or(ApiError::NotFound)?;
    let role_rows = sqlx::query("SELECT r.id, r.name FROM roles r INNER JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ? ORDER BY r.name ASC")
        .bind(&user.user_id)
        .fetch_all(&db.0).await?;
    let roles: Vec<serde_json::Value> = role_rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<String,_>("id"),
                "name": r.get::<String,_>("name"),
            })
        })
        .collect();
    let user = serde_json::json!({
        "id": row.get::<String,_>("id"),
        "username": row.get::<String,_>("username"),
        "email": row.get::<Option<String>,_>("email"),
        "avatar_file_id": row.get::<Option<String>,_>("avatar_file_id"),
        "created_at": row.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),
        "updated_at": row.get::<chrono::DateTime<chrono::Utc>,_>("updated_at"),
        "roles": roles,
    });
    Ok(HttpResponse::Ok().json(user))
}

#[derive(Deserialize)]
pub struct UpdateMeReq {
    pub username: Option<String>,
    pub email: Option<String>,
}

pub async fn update_me(
    db: web::Data<Db>,
    chat: web::Data<actix::Addr<ChatServer>>,
    user: AuthUser,
    body: web::Json<UpdateMeReq>,
) -> Result<HttpResponse, ApiError> {
    if body.username.as_deref().map_or(false, |u| u.len() < 3) {
        return Err(ApiError::BadRequest("username too short".into()));
    }
    sqlx::query("UPDATE users SET username = COALESCE(?, username), email = COALESCE(?, email), updated_at = ? WHERE id = ?")
        .bind(&body.username)
        .bind(&body.email)
        .bind(chrono::Utc::now())
        .bind(&user.user_id)
        .execute(&db.0).await?;

    // Broadcast profile update
    chat.do_send(BroadcastAll {
        payload: serde_json::json!({
            "type": "user_updated",
            "user_id": user.user_id,
        })
        .to_string(),
    });

    me(db, user).await
}

#[derive(Deserialize)]
pub struct ChangePasswordReq {
    pub current_password: String,
    pub new_password: String,
}

pub async fn change_password(
    db: web::Data<Db>,
    user: AuthUser,
    body: web::Json<ChangePasswordReq>,
) -> Result<HttpResponse, ApiError> {
    if body.new_password.len() < 8 {
        return Err(ApiError::BadRequest("new password too short".into()));
    }
    let row = sqlx::query("SELECT password_hash FROM users WHERE id = ?")
        .bind(&user.user_id)
        .fetch_one(&db.0)
        .await?;
    let hash: String = row.get("password_hash");
    if !auth::verify_password(&hash, &body.current_password) {
        return Err(ApiError::Forbidden);
    }
    let new_hash = auth::hash_password(&body.new_password)?;
    sqlx::query("UPDATE users SET password_hash = ?, updated_at = ? WHERE id = ?")
        .bind(new_hash)
        .bind(chrono::Utc::now())
        .bind(&user.user_id)
        .execute(&db.0)
        .await?;
    Ok(HttpResponse::Ok().finish())
}

pub async fn upload_avatar(
    cfg: web::Data<Config>,
    db: web::Data<Db>,
    chat: web::Data<actix::Addr<ChatServer>>,
    user: AuthUser,
    mut payload: Multipart,
) -> Result<HttpResponse, ApiError> {
    use crate::routes::files::{SavedFile, save_multipart_file};
    let mut saved: Option<SavedFile> = None;

    while let Some(item) = payload
        .try_next()
        .await
        .map_err(|_| ApiError::BadRequest("invalid multipart".into()))?
    {
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
        .execute(&db.0)
        .await?;

    // Broadcast profile update
    chat.do_send(BroadcastAll {
        payload: serde_json::json!({
            "type": "user_updated",
            "user_id": user.user_id,
        })
        .to_string(),
    });

    Ok(HttpResponse::Ok().json(serde_json::json!({"avatar_file_id": saved.file_id})))
}

#[derive(Serialize)]
pub struct UserPublic {
    id: String,
    username: String,
    avatar_file_id: Option<String>,
}

// List all users (basic public info). Requires authentication.
pub async fn list_users(db: web::Data<Db>, _user: AuthUser) -> Result<HttpResponse, ApiError> {
    let rows = sqlx::query("SELECT id, username, avatar_file_id FROM users ORDER BY username ASC")
        .fetch_all(&db.0)
        .await?;
    let users: Vec<UserPublic> = rows
        .into_iter()
        .map(|r| UserPublic {
            id: r.get("id"),
            username: r.get("username"),
            avatar_file_id: r.get("avatar_file_id"),
        })
        .collect();
    Ok(HttpResponse::Ok().json(users))
}

// Any authenticated user can get public info about another user.
pub async fn get_user(
    db: web::Data<Db>,
    _user: AuthUser,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let user_id = path.into_inner();
    let row = sqlx::query("SELECT id, username, avatar_file_id FROM users WHERE id = ?")
        .bind(&user_id)
        .fetch_optional(&db.0)
        .await?;
    let row = row.ok_or(ApiError::NotFound)?;
    let user = UserPublic {
        id: row.get("id"),
        username: row.get("username"),
        avatar_file_id: row.get("avatar_file_id"),
    };
    Ok(HttpResponse::Ok().json(user))
}

// Any authenticated user can get another user's avatar.
// This redirects to the actual file serving endpoint.
pub async fn get_user_avatar(
    db: web::Data<Db>,
    _user: AuthUser,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let user_id = path.into_inner();
    // This query joins users and files to get the file_id and original_name for the redirect URL.
    let row = sqlx::query(
        "SELECT f.id, f.original_name FROM files f INNER JOIN users u ON u.avatar_file_id = f.id WHERE u.id = ?")
        .bind(&user_id)
        .fetch_optional(&db.0).await?;

    let row = row.ok_or(ApiError::NotFound)?;
    let file_id: String = row.get("id");
    let original_name: String = row.get("original_name");

    // URL-encode the filename to handle special characters.
    // Note: You'll need to add the `urlencoding` crate to your Cargo.toml.
    let file_url = format!("/files/{}/{}", file_id, urlencoding::encode(&original_name));

    Ok(HttpResponse::Found()
        .append_header((actix_web::http::header::LOCATION, file_url))
        .finish())
}
