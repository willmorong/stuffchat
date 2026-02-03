use crate::{auth::AuthUser, config::Config, db::Db, errors::ApiError};
use actix_multipart::Multipart;
use actix_web::{HttpResponse, web};
use futures_util::TryStreamExt as _;
use image::ImageFormat;
use sqlx::Row;
use std::io::Cursor;
use std::path::Path;

#[derive(serde::Serialize, sqlx::FromRow)]
pub struct Emoji {
    pub name: String,
    pub created_at: String,
    pub created_by: String,
}

pub async fn list_emojis(db: web::Data<Db>) -> Result<HttpResponse, ApiError> {
    let emojis = sqlx::query_as::<_, Emoji>(
        "SELECT name, created_at, created_by FROM custom_emojis WHERE deleted_at IS NULL",
    )
    .fetch_all(&db.0)
    .await?;
    Ok(HttpResponse::Ok().json(emojis))
}

pub async fn upload_emoji(
    cfg: web::Data<Config>,
    db: web::Data<Db>,
    user: AuthUser,
    mut payload: Multipart,
) -> Result<HttpResponse, ApiError> {
    let mut name: Option<String> = None;
    let mut file_data: Option<Vec<u8>> = None;

    while let Some(mut field) = payload
        .try_next()
        .await
        .map_err(|_| ApiError::BadRequest("invalid multipart".into()))?
    {
        let field_name = field.name().unwrap_or("");

        if field_name == "name" {
            let mut buf = Vec::new();
            while let Some(chunk) = field.try_next().await.map_err(|_| ApiError::Internal)? {
                buf.extend_from_slice(&chunk);
            }
            name = Some(
                String::from_utf8(buf).map_err(|_| ApiError::BadRequest("invalid name".into()))?,
            );
        } else if field_name == "file" {
            let mut buf = Vec::new();
            while let Some(chunk) = field.try_next().await.map_err(|_| ApiError::Internal)? {
                buf.extend_from_slice(&chunk);
                if buf.len() > cfg.max_upload_size {
                    return Err(ApiError::BadRequest("file too large".into()));
                }
            }
            file_data = Some(buf);
        }
    }

    let name = name
        .ok_or(ApiError::BadRequest("missing name".into()))?
        .to_lowercase();
    let file_data = file_data.ok_or(ApiError::BadRequest("missing file".into()))?;

    // Validate name: lowercase letters, numbers, dashes, and underscores
    if !name
        .chars()
        .all(|c| c.is_lowercase() || c.is_numeric() || c == '-' || c == '_')
        || name.is_empty()
    {
        return Err(ApiError::BadRequest("invalid emoji name format".into()));
    }

    // Process image to 64x64 PNG
    let img = image::load_from_memory(&file_data)
        .map_err(|_| ApiError::BadRequest("invalid image file".into()))?;
    let resized = img.thumbnail(64, 64);

    let mut png_data = Cursor::new(Vec::new());
    resized
        .write_to(&mut png_data, ImageFormat::Png)
        .map_err(|_| ApiError::Internal)?;
    let png_bytes = png_data.into_inner();

    let id = uuid::Uuid::new_v4().to_string();
    let filename = format!("{}.png", id);
    let emoji_dir = Path::new(&cfg.uploads_dir).join("emojis");

    // Ensure emoji dir exists
    if !emoji_dir.exists() {
        std::fs::create_dir_all(&emoji_dir).map_err(|_| ApiError::Internal)?;
    }

    let file_path = emoji_dir.join(&filename);
    std::fs::write(&file_path, &png_bytes).map_err(|_| ApiError::Internal)?;

    // Insert into DB. We use the 'id' for the filename but 'name' for the unique identifier.
    // Store filename in id column for simplicity in serving, or add a column.
    // Let's use name as the primary lookup but keep the UUID for the file.
    // Actually, let's just store the filename in a new column or reuse ID.
    // I'll reuse ID for the UUID-based filename.
    sqlx::query("INSERT INTO custom_emojis (id, name, created_by, created_at) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(&name)
        .bind(&user.user_id)
        .bind(chrono::Utc::now())
        .execute(&db.0)
        .await
        .map_err(|e| {
            if let Some(err) = e.as_database_error() {
                if err.is_unique_violation() {
                    return ApiError::BadRequest("emoji name already exists".into());
                }
            }
            ApiError::Internal
        })?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "status": "ok", "name": name })))
}

pub async fn delete_emoji(
    db: web::Data<Db>,
    _user: AuthUser,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let name = path.into_inner();

    let res = sqlx::query(
        "UPDATE custom_emojis SET deleted_at = ? WHERE name = ? AND deleted_at IS NULL",
    )
    .bind(chrono::Utc::now())
    .bind(&name)
    .execute(&db.0)
    .await?;

    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }

    Ok(HttpResponse::NoContent().finish())
}

pub async fn get_emoji_image(
    cfg: web::Data<Config>,
    db: web::Data<Db>,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let name = path.into_inner();

    let row = sqlx::query("SELECT id FROM custom_emojis WHERE name = ? AND deleted_at IS NULL")
        .bind(&name)
        .fetch_optional(&db.0)
        .await?;

    let row = row.ok_or(ApiError::NotFound)?;
    let id: String = row.get("id");
    let filename = format!("{}.png", id);
    let p = Path::new(&cfg.uploads_dir).join("emojis").join(&filename);

    if !p.exists() {
        return Err(ApiError::NotFound);
    }

    Ok(HttpResponse::Ok()
        .content_type("image/png")
        .body(std::fs::read(p).map_err(|_| ApiError::Internal)?))
}
