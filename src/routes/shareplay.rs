use crate::ws::server::{ChatServer, GetSharePlayCurrent};
use actix_files::NamedFile;
use actix_web::{Error, web};
use std::path::PathBuf;

pub async fn get_current_track(
    path: web::Path<String>,
    app_state: web::Data<actix::Addr<ChatServer>>,
) -> Result<NamedFile, Error> {
    let channel_id = path.into_inner();
    log::info!("get_current_track called for channel_id={}", channel_id);

    let res = app_state
        .send(GetSharePlayCurrent {
            channel_id: channel_id.clone(),
        })
        .await;

    log::info!("GetSharePlayCurrent result: {:?}", res);

    match res {
        Ok(Ok(Some(file_path))) => {
            log::info!(
                "Serving SharePlay file for channel {}: {}",
                channel_id,
                file_path
            );

            let path = PathBuf::from(&file_path);

            // Check if file exists before trying to open
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
            log::warn!("No current track for channel {}", channel_id);
            Err(actix_web::error::ErrorNotFound(
                "No track currently playing",
            ))
        }
        Ok(Err(_)) => {
            log::warn!("SharePlay state error for channel {}", channel_id);
            Err(actix_web::error::ErrorNotFound("SharePlay state not found"))
        }
        Err(e) => {
            log::error!("Actor mailbox error: {}", e);
            Err(actix_web::error::ErrorInternalServerError(
                "Internal server error",
            ))
        }
    }
}
