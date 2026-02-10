use crate::ws::server::{
    ChatServer, GetSharePlaySongId, GetSharePlaySongPath, GetSharePlayThumbnailPath,
};
use actix_files::NamedFile;
use actix_web::{Error, HttpResponse, http::header, web};
use std::path::PathBuf;

pub async fn get_current_track(
    path: web::Path<String>,
    app_state: web::Data<actix::Addr<ChatServer>>,
) -> Result<HttpResponse, Error> {
    let channel_id = path.into_inner();
    log::info!(
        "get_current_track (ID lookup) called for channel_id={}",
        channel_id
    );

    let res = app_state
        .send(GetSharePlaySongId {
            channel_id: channel_id.clone(),
        })
        .await;

    match res {
        Ok(Ok(Some(song_id))) => Ok(HttpResponse::Ok()
            .insert_header((
                header::CACHE_CONTROL,
                "no-store, no-cache, must-revalidate, proxy-revalidate, max-age=0",
            ))
            .insert_header((header::PRAGMA, "no-cache"))
            .insert_header((header::EXPIRES, "0"))
            .json(serde_json::json!({ "song_id": song_id }))),
        Ok(Ok(None)) => {
            log::warn!("No current track ID for channel {}", channel_id);
            Err(actix_web::error::ErrorNotFound(
                "No track currently playing",
            ))
        }
        _ => {
            log::error!("Error getting song ID for channel {}", channel_id);
            Err(actix_web::error::ErrorInternalServerError(
                "Internal server error",
            ))
        }
    }
}

pub async fn get_song_by_id(
    path: web::Path<String>,
    app_state: web::Data<actix::Addr<ChatServer>>,
) -> Result<NamedFile, Error> {
    let song_id = path.into_inner();
    log::info!("get_song_by_id called for song_id={}", song_id);

    let res = app_state
        .send(GetSharePlaySongPath {
            song_id: song_id.clone(),
        })
        .await;

    match res {
        Ok(Ok(Some(file_path))) => {
            let path = PathBuf::from(&file_path);
            if !path.exists() {
                log::error!("File does not exist: {}", file_path);
                return Err(actix_web::error::ErrorNotFound(format!(
                    "File not found: {}",
                    file_path
                )));
            }
            Ok(NamedFile::open(path)?)
        }
        Ok(Ok(None)) => {
            log::warn!("Song ID {} not found", song_id);
            Err(actix_web::error::ErrorNotFound("Song not found"))
        }
        _ => {
            log::error!("Error getting song path for ID {}", song_id);
            Err(actix_web::error::ErrorInternalServerError(
                "Internal server error",
            ))
        }
    }
}
pub async fn get_thumbnail_by_id(
    path: web::Path<String>,
    app_state: web::Data<actix::Addr<ChatServer>>,
) -> Result<NamedFile, Error> {
    let item_id = path.into_inner();
    log::info!("get_thumbnail_by_id called for item_id={}", item_id);

    let res = app_state
        .send(GetSharePlayThumbnailPath {
            item_id: item_id.clone(),
        })
        .await;

    match res {
        Ok(Ok(Some(file_path))) => {
            let path = PathBuf::from(&file_path);
            if !path.exists() {
                log::error!("Thumbnail file does not exist: {}", file_path);
                return Err(actix_web::error::ErrorNotFound(format!(
                    "Thumbnail file not found: {}",
                    file_path
                )));
            }
            Ok(NamedFile::open(path)?)
        }
        Ok(Ok(None)) => {
            log::warn!("Thumbnail for item ID {} not found", item_id);
            Err(actix_web::error::ErrorNotFound("Thumbnail not found"))
        }
        _ => {
            log::error!("Error getting thumbnail path for ID {}", item_id);
            Err(actix_web::error::ErrorInternalServerError(
                "Internal server error",
            ))
        }
    }
}
