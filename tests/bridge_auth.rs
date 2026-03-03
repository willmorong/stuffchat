mod common;

use actix_web::{App, http::StatusCode, test, web::Data};
use common::{bridge_get, test_context};
use stuffchat::app;

#[actix_web::test]
async fn bridge_routes_absent_when_disabled() {
    let ctx = test_context().await;
    let app = test::init_service(
        App::new()
            .app_data(Data::new(ctx.cfg.clone()))
            .app_data(Data::new(ctx.db.clone()))
            .app_data(Data::new(ctx.chat_server.clone()))
            .configure(|cfg| app::configure(cfg, false)),
    )
    .await;

    let response = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/bridge/status")
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[actix_web::test]
async fn bridge_requires_authentication() {
    let ctx = test_context().await;
    let app = test::init_service(
        App::new()
            .app_data(Data::new(ctx.cfg.clone()))
            .app_data(Data::new(ctx.db.clone()))
            .app_data(Data::new(ctx.chat_server.clone()))
            .app_data(Data::new(ctx.bridge_runtime.clone()))
            .configure(|cfg| app::configure(cfg, true)),
    )
    .await;

    let missing =
        test::call_service(&app, bridge_get("/api/bridge/status", None).to_request()).await;
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

    let wrong = test::call_service(
        &app,
        bridge_get("/api/bridge/status", Some("wrong-secret")).to_request(),
    )
    .await;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
}
