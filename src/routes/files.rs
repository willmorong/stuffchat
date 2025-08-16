use crate::{auth::AuthUser, config::Config, db::Db, errors::ApiError};
use actix_multipart::Multipart;
use actix_web::{HttpRequest, HttpResponse, web};
use futures_util::TryStreamExt as _;
use sanitize_filename::sanitize;
use sqlx::Row;
use std::io::Write;
use std::path::Path;
use actix_web::http::header::{ContentDisposition, DispositionParam, DispositionType};

#[derive(serde::Serialize)]
pub struct UploadResp {
    pub file_id: String,
}

pub async fn upload_file(
    cfg: web::Data<Config>,
    db: web::Data<Db>,
    user: AuthUser,
    mut payload: Multipart,
) -> Result<HttpResponse, ApiError> {
    let mut saved: Option<SavedFile> = None;
    while let Some(item) = payload
        .try_next()
        .await
        .map_err(|_| ApiError::BadRequest("invalid multipart".into()))?
    {
        let s = save_multipart_file(&cfg, &db, &user.user_id, item).await?;
        saved = Some(s);
        break;
    }
    let saved = saved.ok_or(ApiError::BadRequest("no file part".into()))?;
    Ok(HttpResponse::Ok().json(UploadResp {
        file_id: saved.file_id,
    }))
}

pub struct SavedFile {
    pub file_id: String,
    pub stored_name: String,
}

pub async fn save_multipart_file(
    cfg: &Config,
    db: &Db,
    user_id: &str,
    mut field: actix_multipart::Field,
) -> Result<SavedFile, ApiError> {
    let content_disposition = field.content_disposition().cloned();
    let original = content_disposition
        .and_then(|cd| cd.get_filename().map(|s| s.to_string()))
        .unwrap_or_else(|| "upload.bin".into());
    let original_safe = sanitize(&original);
    let mut data: Vec<u8> = Vec::new();
    while let Some(chunk) = field
        .try_next()
        .await
        .map_err(|_| ApiError::BadRequest("upload read error".into()))?
    {
        data.extend_from_slice(&chunk);
        if data.len() > cfg.max_upload_size {
            return Err(ApiError::BadRequest("file too large".into()));
        }
    }
    let mime = infer::get(&data).map(|t| t.mime_type().to_string());
    let id = uuid::Uuid::new_v4().to_string();
    let ext = Path::new(&original_safe)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("bin");
    let stored_name = format!("{}.{}", id, ext);
    let path = std::path::Path::new(&cfg.uploads_dir).join(&stored_name);
    let mut f = std::fs::File::create(&path).map_err(|_| ApiError::Internal)?;
    f.write_all(&data).map_err(|_| ApiError::Internal)?;

    sqlx::query("INSERT INTO files(id, user_id, original_name, stored_name, mime_type, size_bytes, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)")
        .bind(&id).bind(user_id).bind(&original_safe).bind(&stored_name).bind(&mime)
        .bind(data.len() as i64).bind(chrono::Utc::now())
        .execute(&db.0).await?;

    Ok(SavedFile {
        file_id: id,
        stored_name,
    })
}

// Updated to accept a filename segment for the URL, but only use the ID for lookup.
// Route pattern should be something like: .route("/files/{id}/{filename:.*}", web::get().to(get_file))
pub async fn get_file(
    cfg: web::Data<Config>,
    db: web::Data<Db>,
    req: HttpRequest,
    path: web::Path<(String, String)>, // (id, filename) - filename is ignored for lookup
) -> Result<HttpResponse, ApiError> {
    let (id, _filename) = path.into_inner();

    let row = sqlx::query("SELECT stored_name, original_name, mime_type FROM files WHERE id = ?")
        .bind(&id)
        .fetch_optional(&db.0)
        .await?;
    let row = row.ok_or(ApiError::NotFound)?;
    let stored: String = row.get("stored_name");
    let original: String = row.get("original_name"); // already sanitized at upload
    let mime: Option<String> = row.get("mime_type");

    let p = std::path::Path::new(&cfg.uploads_dir).join(&stored);
    if !p.exists() { return Err(ApiError::NotFound); }

    let named = actix_files::NamedFile::open_async(p).await
        .map_err(|_| ApiError::Internal)?
        .use_last_modified(true)
        .prefer_utf8(true)
        .set_content_disposition(ContentDisposition {
            disposition: DispositionType::Inline,
            parameters: vec![DispositionParam::Filename(original.clone())],
        });

    let mut resp = named.into_response(&req);
    if let Some(m) = mime {
        if let Ok(val) = actix_web::http::header::HeaderValue::from_str(&m) {
            resp.headers_mut().insert(
                actix_web::http::header::CONTENT_TYPE,
                val,
            );
        }
    }
    Ok(resp)
}