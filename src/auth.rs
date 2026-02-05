use crate::config::Config;
use crate::db::Db;
use crate::errors::ApiError;
use actix_web::{FromRequest, HttpRequest, dev::Payload};
use argon2::password_hash::{PasswordHash, SaltString, rand_core::OsRng};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use chrono::{Duration, Utc};
use futures_util::future::{Ready, err, ok};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // user_id
    pub exp: usize,
}

pub fn hash_password(plain: &str) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|_| ApiError::Internal)?
        .to_string())
}

pub fn verify_password(hash: &str, plain: &str) -> bool {
    let parsed = PasswordHash::new(hash);
    if parsed.is_err() {
        return false;
    }
    Argon2::default()
        .verify_password(plain.as_bytes(), &parsed.unwrap())
        .is_ok()
}

pub fn create_access_token(user_id: &str, cfg: &Config) -> Result<String, ApiError> {
    let exp = (Utc::now() + Duration::minutes(15)).timestamp() as usize;
    let claims = Claims {
        sub: user_id.to_string(),
        exp,
    };
    jsonwebtoken::encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(cfg.jwt_secret_bytes()),
    )
    .map_err(|_| ApiError::Internal)
}

pub fn verify_access_token(token: &str, cfg: &Config) -> Result<Claims, ApiError> {
    let mut v = Validation::new(Algorithm::HS256);
    v.validate_exp = true;
    jsonwebtoken::decode::<Claims>(token, &DecodingKey::from_secret(cfg.jwt_secret_bytes()), &v)
        .map(|data| data.claims)
        .map_err(|_| ApiError::Unauthorized)
}

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: String,
}

impl FromRequest for AuthUser {
    type Error = ApiError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        let cfg = req.app_data::<actix_web::web::Data<Config>>().unwrap();
        if let Some(h) = req.headers().get("Authorization") {
            if let Ok(s) = h.to_str() {
                if let Some(token) = s.strip_prefix("Bearer ") {
                    if let Ok(claims) = verify_access_token(token, cfg) {
                        return ok(AuthUser {
                            user_id: claims.sub,
                        });
                    }
                }
            }
        }
        err(ApiError::Unauthorized)
    }
}

// Refresh tokens
pub async fn create_refresh_token(db: &Db, user_id: &str) -> Result<(String, String), ApiError> {
    let token_raw = uuid::Uuid::new_v4().to_string() + &uuid::Uuid::new_v4().to_string();
    let token_hash = hash_password(&token_raw)?;
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now();
    let expires_at = created_at + chrono::Duration::days(30);

    sqlx::query("INSERT INTO refresh_tokens(id, user_id, token_hash, created_at, expires_at) VALUES (?, ?, ?, ?, ?)")
        .bind(&id)
        .bind(user_id)
        .bind(&token_hash)
        .bind(created_at)
        .bind(expires_at)
        .execute(&db.0).await?;

    Ok((id, token_raw))
}

pub async fn verify_and_rotate_refresh_token(
    db: &Db,
    token_id: &str,
    token_raw: &str,
) -> Result<String, ApiError> {
    let row = sqlx::query(
        "SELECT user_id, token_hash, expires_at, revoked_at FROM refresh_tokens WHERE id = ?",
    )
    .bind(token_id)
    .fetch_optional(&db.0)
    .await?;

    let row = row.ok_or(ApiError::Unauthorized)?;
    let user_id: String = row.get("user_id");
    let token_hash: String = row.get("token_hash");
    let expires_at: chrono::DateTime<chrono::Utc> = row.get("expires_at");
    let revoked_at: Option<chrono::DateTime<chrono::Utc>> = row.get("revoked_at");

    if revoked_at.is_some() || chrono::Utc::now() > expires_at {
        return Err(ApiError::Unauthorized);
    }
    if !verify_password(&token_hash, token_raw) {
        return Err(ApiError::Unauthorized);
    }
    // Revoke old and create new
    sqlx::query("UPDATE refresh_tokens SET revoked_at = ? WHERE id = ?")
        .bind(chrono::Utc::now())
        .bind(token_id)
        .execute(&db.0)
        .await?;

    Ok(user_id)
}

pub async fn cleanup_refresh_tokens(db: &Db) -> Result<u64, ApiError> {
    let result =
        sqlx::query("DELETE FROM refresh_tokens WHERE revoked_at IS NOT NULL OR expires_at < ?")
            .bind(chrono::Utc::now())
            .execute(&db.0)
            .await?;

    Ok(result.rows_affected())
}
