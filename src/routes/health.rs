use crate::config::Config;
use actix_web::{HttpResponse, web};

pub async fn health_check(cfg: web::Data<Config>) -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({
        "health": true,
        "version": "1.0.0",
        "config": {
            "invite_only": cfg.invite_only
        }
    }))
}
