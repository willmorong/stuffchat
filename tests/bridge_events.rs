mod common;

use actix::Actor;
use actix_web::{App, http::StatusCode, test, web::Data};
use common::{bridge_get, insert_channel, insert_user, test_context};
use serde_json::Value;
use stuffchat::app;
use stuffchat::bridge::BridgeRuntime;
use stuffchat::ws::server::{ChatServer, JoinVoice, LeaveVoice};

#[actix_web::test]
async fn bridge_status_reports_empty_queue() {
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

    let response: Value = test::call_and_read_body_json(
        &app,
        bridge_get("/api/bridge/status", Some(&ctx.bridge_secret)).to_request(),
    )
    .await;
    assert_eq!(response["oldest_available"], Value::Null);
    assert_eq!(response["latest_available"], 0);
}

#[actix_web::test]
async fn bridge_events_paginate_with_resolved_metadata() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "user-1", "alice").await;
    insert_channel(&ctx.db, "channel-1", "main", "user-1", true).await;
    ctx.bridge_runtime.record_call_joined("channel-1", "user-1");
    ctx.bridge_runtime.record_call_left("channel-1", "user-1");

    let app = test::init_service(
        App::new()
            .app_data(Data::new(ctx.cfg.clone()))
            .app_data(Data::new(ctx.db.clone()))
            .app_data(Data::new(ctx.chat_server.clone()))
            .app_data(Data::new(ctx.bridge_runtime.clone()))
            .configure(|cfg| app::configure(cfg, true)),
    )
    .await;

    let response: Value = test::call_and_read_body_json(
        &app,
        bridge_get(
            "/api/bridge/events?after=0&limit=1",
            Some(&ctx.bridge_secret),
        )
        .to_request(),
    )
    .await;

    assert_eq!(response["reset_required"], false);
    assert_eq!(response["oldest_available"], 1);
    assert_eq!(response["latest_available"], 2);
    assert_eq!(response["next_after"], 1);
    assert_eq!(response["events"][0]["type"], "call_joined");
    assert_eq!(response["events"][0]["user"]["username"], "alice");
    assert_eq!(response["events"][0]["channel"]["name"], "main");
}

#[actix_web::test]
async fn bridge_events_require_reset_when_cursor_is_stale() {
    let ctx = test_context().await;
    for idx in 0..5002 {
        ctx.bridge_runtime
            .record_call_joined(format!("channel-{idx}"), "user-1");
    }

    let app = test::init_service(
        App::new()
            .app_data(Data::new(ctx.cfg.clone()))
            .app_data(Data::new(ctx.db.clone()))
            .app_data(Data::new(ctx.chat_server.clone()))
            .app_data(Data::new(ctx.bridge_runtime.clone()))
            .configure(|cfg| app::configure(cfg, true)),
    )
    .await;

    let response: Value = test::call_and_read_body_json(
        &app,
        bridge_get("/api/bridge/events?after=1", Some(&ctx.bridge_secret)).to_request(),
    )
    .await;

    assert_eq!(response["reset_required"], true);
    assert_eq!(response["events"], Value::Array(vec![]));
    assert_eq!(response["next_after"], 5002);
}

#[actix_web::test]
async fn bridge_emits_join_and_leave_once_per_user_presence() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "user-1", "alice").await;
    insert_channel(&ctx.db, "channel-1", "main", "user-1", true).await;

    let bridge_runtime = BridgeRuntime::new(ctx.bridge_secret.clone());
    let server = ChatServer::new(Some(bridge_runtime.clone())).start();

    server
        .send(JoinVoice {
            channel_id: "channel-1".to_string(),
            user_id: "user-1".to_string(),
            session_id: "session-1".to_string(),
        })
        .await
        .expect("join 1");
    server
        .send(JoinVoice {
            channel_id: "channel-1".to_string(),
            user_id: "user-1".to_string(),
            session_id: "session-2".to_string(),
        })
        .await
        .expect("join 2");
    server
        .send(LeaveVoice {
            channel_id: "channel-1".to_string(),
            user_id: "user-1".to_string(),
            session_id: "session-1".to_string(),
        })
        .await
        .expect("leave 1");
    server
        .send(LeaveVoice {
            channel_id: "channel-1".to_string(),
            user_id: "user-1".to_string(),
            session_id: "session-2".to_string(),
        })
        .await
        .expect("leave 2");

    let app = test::init_service(
        App::new()
            .app_data(Data::new(ctx.cfg.clone()))
            .app_data(Data::new(ctx.db.clone()))
            .app_data(Data::new(server.clone()))
            .app_data(Data::new(bridge_runtime.clone()))
            .configure(|cfg| app::configure(cfg, true)),
    )
    .await;

    let response = test::call_service(
        &app,
        bridge_get(
            "/api/bridge/events?after=0&limit=10",
            Some(&ctx.bridge_secret),
        )
        .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = test::read_body_json(response).await;
    assert_eq!(body["events"].as_array().expect("events len").len(), 2);
    assert_eq!(body["events"][0]["type"], "call_joined");
    assert_eq!(body["events"][1]["type"], "call_left");
}

#[actix_web::test]
async fn bridge_events_return_null_metadata_for_unknown_entities() {
    let ctx = test_context().await;
    ctx.bridge_runtime
        .record_call_joined("missing-channel", "missing-user");

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
        bridge_get("/api/bridge/events?after=0", Some(&ctx.bridge_secret)).to_request(),
    )
    .await;

    assert_eq!(body["events"][0]["user"]["username"], Value::Null);
    assert_eq!(body["events"][0]["channel"]["name"], Value::Null);
    assert_eq!(body["events"][0]["channel"]["is_voice"], Value::Null);
}
