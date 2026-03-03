mod common;

use actix_web::{App, test, web::Data};
use common::{bridge_post_json, insert_channel, insert_user, test_context};
use serde_json::{Value, json};
use stuffchat::app;

#[actix_web::test]
async fn bridge_resolve_returns_only_known_entities() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "user-1", "alice").await;
    insert_channel(&ctx.db, "channel-1", "main", "user-1", true).await;

    let app = test::init_service(
        App::new()
            .app_data(Data::new(ctx.cfg.clone()))
            .app_data(Data::new(ctx.db.clone()))
            .app_data(Data::new(ctx.chat_server.clone()))
            .app_data(Data::new(ctx.bridge_runtime.clone()))
            .configure(|cfg| app::configure(cfg, true)),
    )
    .await;

    let body: Value = test::call_and_read_body_json(
        &app,
        bridge_post_json(
            "/api/bridge/resolve",
            Some(&ctx.bridge_secret),
            json!({
                "user_ids": ["user-1", "missing-user"],
                "channel_ids": ["channel-1", "missing-channel"]
            }),
        )
        .to_request(),
    )
    .await;

    assert_eq!(body["users"]["user-1"]["username"], "alice");
    assert_eq!(body["channels"]["channel-1"]["name"], "main");
    assert_eq!(body["users"]["missing-user"], Value::Null);
    assert_eq!(body["channels"]["missing-channel"], Value::Null);
}
