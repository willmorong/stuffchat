mod common;

use actix::Actor;
use actix_web::{App, HttpRequest, HttpResponse, HttpServer, web};
use common::{insert_channel, insert_user, test_context};
use serde_json::Value;
use std::net::TcpListener;
use stuffchat::bridge::BridgeRuntime;
use stuffchat::ws::server::{ChatServer, JoinVoice, LeaveVoice};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::time::{Duration, timeout};

#[derive(Debug)]
struct ReceivedBridgeRequest {
    authorization: Option<String>,
    body: Value,
}

async fn capture_request(
    request: HttpRequest,
    body: web::Json<Value>,
    sender: web::Data<UnboundedSender<ReceivedBridgeRequest>>,
) -> HttpResponse {
    sender
        .send(ReceivedBridgeRequest {
            authorization: request
                .headers()
                .get("Authorization")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
            body: body.into_inner(),
        })
        .expect("capture bridge request");
    HttpResponse::Ok().finish()
}

async fn spawn_bridge_receiver() -> (
    String,
    UnboundedReceiver<ReceivedBridgeRequest>,
    actix_web::dev::ServerHandle,
) {
    let (sender, receiver) = unbounded_channel();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind bridge receiver");
    let address = listener.local_addr().expect("receiver addr");

    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(sender.clone()))
            .route("/bridge/events", web::post().to(capture_request))
    })
    .listen(listener)
    .expect("listen bridge receiver")
    .run();

    let handle = server.handle();
    tokio::spawn(server);

    (format!("http://{address}/bridge/events"), receiver, handle)
}

async fn recv_request(
    receiver: &mut UnboundedReceiver<ReceivedBridgeRequest>,
) -> ReceivedBridgeRequest {
    timeout(Duration::from_secs(3), receiver.recv())
        .await
        .expect("bridge request timeout")
        .expect("bridge request payload")
}

#[actix_web::test]
async fn bridge_posts_resolved_metadata() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "user-1", "alice").await;
    insert_channel(&ctx.db, "channel-1", "main", "user-1", true).await;

    let (bridge_url, mut receiver, handle) = spawn_bridge_receiver().await;
    let runtime = BridgeRuntime::new(ctx.bridge_secret.clone(), bridge_url, ctx.db.clone());
    runtime.record_call_joined("channel-1", "user-1");

    let request = recv_request(&mut receiver).await;
    assert_eq!(
        request.authorization.as_deref(),
        Some("Bearer bridge-test-secret")
    );
    assert_eq!(request.body["type"], "call_joined");
    assert_eq!(request.body["user"]["id"], "user-1");
    assert_eq!(request.body["user"]["username"], "alice");
    assert_eq!(request.body["channel"]["id"], "channel-1");
    assert_eq!(request.body["channel"]["name"], "main");
    assert_eq!(request.body["channel"]["is_voice"], true);
    assert!(request.body["occurred_at"].as_str().is_some());

    handle.stop(true).await;
}

#[actix_web::test]
async fn bridge_posts_null_metadata_for_unknown_entities() {
    let ctx = test_context().await;
    let (bridge_url, mut receiver, handle) = spawn_bridge_receiver().await;
    let runtime = BridgeRuntime::new(ctx.bridge_secret.clone(), bridge_url, ctx.db.clone());
    runtime.record_call_left("missing-channel", "missing-user");

    let request = recv_request(&mut receiver).await;
    assert_eq!(request.body["type"], "call_left");
    assert_eq!(request.body["user"]["id"], "missing-user");
    assert_eq!(request.body["user"]["username"], Value::Null);
    assert_eq!(request.body["channel"]["id"], "missing-channel");
    assert_eq!(request.body["channel"]["name"], Value::Null);
    assert_eq!(request.body["channel"]["is_voice"], Value::Null);

    handle.stop(true).await;
}

#[actix_web::test]
async fn bridge_emits_join_and_leave_once_per_user_presence() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "user-1", "alice").await;
    insert_channel(&ctx.db, "channel-1", "main", "user-1", true).await;

    let (bridge_url, mut receiver, handle) = spawn_bridge_receiver().await;
    let bridge_runtime = BridgeRuntime::new(ctx.bridge_secret.clone(), bridge_url, ctx.db.clone());
    let server = ChatServer::new(Some(bridge_runtime), None).start();

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

    let first = recv_request(&mut receiver).await;
    // Leave is now deferred briefly to allow for quick reconnect recovery.
    let second = timeout(Duration::from_millis(12000), receiver.recv())
        .await
        .expect("bridge call_left should arrive after reconnect grace window")
        .expect("bridge call_left payload");

    assert_eq!(first.body["type"], "call_joined");
    assert_eq!(second.body["type"], "call_left");
    assert!(
        timeout(Duration::from_millis(250), receiver.recv())
            .await
            .is_err()
    );

    handle.stop(true).await;
}

#[actix_web::test]
async fn bridge_reconnect_suppresses_graceful_left_join_chatter() {
    let ctx = test_context().await;
    insert_user(&ctx.db, "user-1", "alice").await;
    insert_channel(&ctx.db, "channel-1", "main", "user-1", true).await;

    let (bridge_url, mut receiver, handle) = spawn_bridge_receiver().await;
    let bridge_runtime = BridgeRuntime::new(ctx.bridge_secret.clone(), bridge_url, ctx.db.clone());
    let server = ChatServer::new(Some(bridge_runtime), None).start();

    server
        .send(JoinVoice {
            channel_id: "channel-1".to_string(),
            user_id: "user-1".to_string(),
            session_id: "session-1".to_string(),
        })
        .await
        .expect("join");

    let first = recv_request(&mut receiver).await;
    assert_eq!(first.body["type"], "call_joined");

    server
        .send(LeaveVoice {
            channel_id: "channel-1".to_string(),
            user_id: "user-1".to_string(),
            session_id: "session-1".to_string(),
        })
        .await
        .expect("leave");
    server
        .send(JoinVoice {
            channel_id: "channel-1".to_string(),
            user_id: "user-1".to_string(),
            session_id: "session-2".to_string(),
        })
        .await
        .expect("resume");

    assert!(
        timeout(Duration::from_millis(250), receiver.recv())
            .await
            .is_err(),
        "bridge should suppress reconnect chatter",
    );

    handle.stop(true).await;
}
