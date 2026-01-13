use crate::{auth, config::Config, db::Db, errors::ApiError};
use actix_web::{HttpResponse, web};
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[derive(Deserialize)]
pub struct RegisterReq {
    pub username: String,
    pub email: Option<String>,
    pub password: String,
    pub invite_code: Option<String>,
}
#[derive(Serialize)]
pub struct AuthResp {
    access_token: String,
    refresh_token_id: String,
    refresh_token: String,
    user_id: String,
}

pub async fn register(
    cfg: web::Data<Config>,
    db: web::Data<Db>,
    body: web::Json<RegisterReq>,
) -> Result<HttpResponse, ApiError> {
    if body.username.len() < 3 || body.password.len() < 8 {
        return Err(ApiError::BadRequest("invalid username/password".into()));
    }

    if cfg.invite_only {
        let code = body
            .invite_code
            .as_ref()
            .ok_or_else(|| ApiError::BadRequest("invite code required".into()))?;
        let invite =
            sqlx::query("SELECT code FROM invites WHERE code = ? AND joined_user_id IS NULL")
                .bind(code)
                .fetch_optional(&db.0)
                .await?;

        if invite.is_none() {
            return Err(ApiError::BadRequest(
                "invalid or already used invite code".into(),
            ));
        }
    }

    let hash = auth::hash_password(&body.password)?;
    let user_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now();

    let mut tx = db.0.begin().await?;

    let res = sqlx::query("INSERT INTO users(id, username, email, password_hash, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&user_id)
        .bind(&body.username)
        .bind(&body.email)
        .bind(&hash)
        .bind(now)
        .bind(now)
        .execute(&mut *tx).await;

    if let Err(e) = res {
        if let sqlx::Error::Database(db_err) = &e {
            if db_err.message().contains("UNIQUE") {
                return Err(ApiError::Conflict(
                    "username or email already exists".into(),
                ));
            }
        }
        return Err(e.into());
    }

    if cfg.invite_only {
        if let Some(code) = &body.invite_code {
            sqlx::query("UPDATE invites SET joined_user_id = ? WHERE code = ?")
                .bind(&user_id)
                .bind(code)
                .execute(&mut *tx)
                .await?;
        }
    }

    // Add new user to all public channels
    sqlx::query("INSERT INTO channel_members(channel_id, user_id, can_read, can_write, can_manage) SELECT id, ?, 1, 1, 0 FROM channels WHERE is_private = 0 AND deleted_at IS NULL")
        .bind(&user_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    let access_token = auth::create_access_token(&user_id, &cfg)?;
    let (rt_id, rt) = auth::create_refresh_token(&db, &user_id).await?;
    Ok(HttpResponse::Ok().json(AuthResp {
        access_token,
        refresh_token_id: rt_id,
        refresh_token: rt,
        user_id,
    }))
}

#[derive(Deserialize)]
pub struct LoginReq {
    pub username_or_email: String,
    pub password: String,
}

pub async fn login(
    cfg: web::Data<Config>,
    db: web::Data<Db>,
    body: web::Json<LoginReq>,
) -> Result<HttpResponse, ApiError> {
    let row = sqlx::query("SELECT id, password_hash FROM users WHERE username = ? OR email = ?")
        .bind(&body.username_or_email)
        .bind(&body.username_or_email)
        .fetch_optional(&db.0)
        .await?;

    let row = row.ok_or(ApiError::Unauthorized)?;
    let user_id: String = row.get("id");
    let password_hash: String = row.get("password_hash");

    if !auth::verify_password(&password_hash, &body.password) {
        return Err(ApiError::Unauthorized);
    }

    let access_token = auth::create_access_token(&user_id, &cfg)?;
    let (rt_id, rt) = auth::create_refresh_token(&db, &user_id).await?;
    Ok(HttpResponse::Ok().json(AuthResp {
        access_token,
        refresh_token_id: rt_id,
        refresh_token: rt,
        user_id,
    }))
}

#[derive(Deserialize)]
pub struct RefreshReq {
    pub refresh_token_id: String,
    pub refresh_token: String,
}

pub async fn refresh(
    cfg: web::Data<Config>,
    db: web::Data<Db>,
    body: web::Json<RefreshReq>,
) -> Result<HttpResponse, ApiError> {
    let user_id =
        auth::verify_and_rotate_refresh_token(&db, &body.refresh_token_id, &body.refresh_token)
            .await?;
    // issue new refresh token
    let (new_id, new_rt) = auth::create_refresh_token(&db, &user_id).await?;
    let access_token = auth::create_access_token(&user_id, &cfg)?;
    Ok(HttpResponse::Ok().json(AuthResp {
        access_token,
        refresh_token_id: new_id,
        refresh_token: new_rt,
        user_id,
    }))
}

#[derive(Deserialize)]
pub struct LogoutReq {
    pub refresh_token_id: String,
}

pub async fn logout(
    db: web::Data<Db>,
    user: auth::AuthUser,
    body: web::Json<LogoutReq>,
) -> Result<HttpResponse, ApiError> {
    // Verify the refresh token belongs to this user
    let row = sqlx::query("SELECT user_id FROM refresh_tokens WHERE id = ?")
        .bind(&body.refresh_token_id)
        .fetch_optional(&db.0)
        .await?;

    let row = row.ok_or(ApiError::NotFound)?;
    let token_owner: String = row.get("user_id");

    if token_owner != user.user_id {
        return Err(ApiError::Forbidden);
    }

    sqlx::query("UPDATE refresh_tokens SET revoked_at = ? WHERE id = ?")
        .bind(chrono::Utc::now())
        .bind(&body.refresh_token_id)
        .execute(&db.0)
        .await?;
    Ok(HttpResponse::Ok().finish())
}
