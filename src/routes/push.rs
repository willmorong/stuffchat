use crate::auth::AuthUser;
use crate::db::Db;
use crate::errors::ApiError;
use crate::push::{
    PushCapabilitiesResponse, PushDeviceDeleteRequest, PushDeviceRegistrationRequest,
    PushRelayRuntime,
};
use actix_web::{HttpResponse, web};
use chrono::Utc;

pub async fn capabilities(
    push_runtime: web::Data<Option<PushRelayRuntime>>,
) -> Result<HttpResponse, ApiError> {
    let response = if push_runtime.get_ref().is_some() {
        PushCapabilitiesResponse::relay_enabled()
    } else {
        PushCapabilitiesResponse::disabled()
    };

    Ok(HttpResponse::Ok().json(response))
}

pub async fn upsert_current_device(
    db: web::Data<Db>,
    push_runtime: web::Data<Option<PushRelayRuntime>>,
    user: AuthUser,
    body: web::Json<PushDeviceRegistrationRequest>,
) -> Result<HttpResponse, ApiError> {
    if push_runtime.get_ref().is_none() {
        log::warn!(
            "rejecting push device upsert: user_id={} reason=relay_disabled",
            user.user_id
        );
        return Err(ApiError::BadRequest(
            "push relay is not enabled on this server".into(),
        ));
    }

    if body.installation_id.trim().is_empty() || body.push_token.trim().is_empty() {
        log::warn!(
            "rejecting push device upsert: user_id={} reason=missing_installation_or_token",
            user.user_id
        );
        return Err(ApiError::BadRequest(
            "installation_id and push_token are required".into(),
        ));
    }

    let now = Utc::now();
    sqlx::query(
        "INSERT INTO push_devices(
            user_id, installation_id, platform, push_token, environment,
            message_notifications, call_notifications, created_at, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(user_id, installation_id, platform)
         DO UPDATE SET
            push_token = excluded.push_token,
            environment = excluded.environment,
            message_notifications = excluded.message_notifications,
            call_notifications = excluded.call_notifications,
            updated_at = excluded.updated_at",
    )
    .bind(&user.user_id)
    .bind(body.installation_id.trim())
    .bind(body.platform.as_str())
    .bind(body.push_token.trim())
    .bind(body.environment.as_str())
    .bind(body.message_notifications)
    .bind(body.call_notifications)
    .bind(now)
    .bind(now)
    .execute(&db.0)
    .await?;

    log::info!(
        "registered push device: user_id={} installation_id={} platform={} environment={} message_notifications={} call_notifications={}",
        user.user_id,
        body.installation_id.trim(),
        body.platform.as_str(),
        body.environment.as_str(),
        body.message_notifications,
        body.call_notifications
    );

    Ok(HttpResponse::Ok().finish())
}

pub async fn delete_current_device(
    db: web::Data<Db>,
    push_runtime: web::Data<Option<PushRelayRuntime>>,
    user: AuthUser,
    body: web::Json<PushDeviceDeleteRequest>,
) -> Result<HttpResponse, ApiError> {
    if push_runtime.get_ref().is_none() {
        log::info!(
            "ignoring push device delete because relay is disabled: user_id={} installation_id={}",
            user.user_id,
            body.installation_id.trim()
        );
        return Ok(HttpResponse::Ok().finish());
    }

    if body.installation_id.trim().is_empty() {
        log::warn!(
            "rejecting push device delete: user_id={} reason=missing_installation_id",
            user.user_id
        );
        return Err(ApiError::BadRequest("installation_id is required".into()));
    }

    sqlx::query("DELETE FROM push_devices WHERE user_id = ? AND installation_id = ?")
        .bind(&user.user_id)
        .bind(body.installation_id.trim())
        .execute(&db.0)
        .await?;

    log::info!(
        "deleted push device: user_id={} installation_id={}",
        user.user_id,
        body.installation_id.trim()
    );

    Ok(HttpResponse::Ok().finish())
}
