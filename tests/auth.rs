mod common;

use actix_web::{App, http::StatusCode, test, web};
use common::test_context;
use serde_json::{Value, json};

#[actix_web::test]
async fn registration_reports_short_password() {
    let ctx = test_context().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(ctx.cfg.clone()))
            .app_data(web::Data::new(ctx.db.clone()))
            .app_data(web::Data::new(ctx.chat_server.clone()))
            .app_data(web::Data::new(None::<stuffchat::push::PushRelayRuntime>))
            .configure(|cfg| stuffchat::app::configure(cfg, true)),
    )
    .await;

    let request = test::TestRequest::post()
        .uri("/api/auth/register")
        .set_json(json!({
            "username": "new-user",
            "email": "new-user@example.com",
            "password": "short"
        }))
        .to_request();
    let response = test::call_service(&app, request).await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value = test::read_body_json(response).await;
    assert_eq!(
        body["error"],
        "bad request: password must be at least 8 characters"
    );
}
