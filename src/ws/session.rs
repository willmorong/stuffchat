use super::server::{
    Broadcast, ChatServer, Connect, DirectSignal, Disconnect, Join, Leave, SharePlayAction,
};
use crate::{auth, config::Config, db::Db};
use actix::{Actor, ActorContext, Addr, AsyncContext, Handler, Message, StreamHandler, WrapFuture};
use actix_web::{Error, HttpRequest, HttpResponse, web};
use actix_web_actors::ws;
use chrono::Utc;
use serde::{Deserialize, Serialize};

pub async fn ws_route(
    req: HttpRequest,
    stream: web::Payload,
    cfg: web::Data<Config>,
    db: web::Data<Db>,
    srv: web::Data<actix::Addr<ChatServer>>,
) -> Result<HttpResponse, Error> {
    let token = req
        .query_string()
        .split('&')
        .find_map(|kv| kv.split_once('='))
        .filter(|(k, _)| *k == "token")
        .map(|(_, v)| v.to_string());

    let claims = match token {
        Some(t) => auth::verify_access_token(&t, &cfg)
            .map_err(|_| actix_web::error::ErrorUnauthorized("bad token"))?,
        None => return Err(actix_web::error::ErrorUnauthorized("missing token")),
    };
    let user_id = claims.sub;
    let _ = sqlx::query(
        "INSERT INTO presence(user_id, last_heartbeat, status, updated_at)
         VALUES (?, ?, 'online', ?)
         ON CONFLICT(user_id) DO UPDATE SET last_heartbeat = excluded.last_heartbeat, status = 'online', updated_at = excluded.updated_at"
    )
    .bind(&user_id).bind(Utc::now()).bind(Utc::now())
    .execute(&db.0).await;

    let session_id = uuid::Uuid::new_v4().to_string();
    let session = WsSession {
        user_id,
        session_id: session_id.clone(),
        server: srv.get_ref().clone(),
        joined: None,
        voice_channel: None,
        db: db.get_ref().clone(),
    };
    let (addr, resp) = ws::WsResponseBuilder::new(session, &req, stream).start_with_addr()?;

    // Send session_id to client
    addr.do_send(ServerMsg {
        payload: serde_json::json!({
            "type": "connection_metadata",
            "session_id": session_id
        })
        .to_string(),
    });

    Ok(resp)
}

pub struct WsSession {
    pub user_id: String,
    pub session_id: String,
    pub server: Addr<ChatServer>,
    pub joined: Option<String>,
    pub voice_channel: Option<String>,
    pub db: Db,
}

impl Actor for WsSession {
    type Context = ws::WebsocketContext<Self>;
    fn started(&mut self, ctx: &mut Self::Context) {
        log::info!("WsSession started: user_id={}", self.user_id);
        self.server.do_send(Connect {
            user_id: self.user_id.clone(),
            session_id: self.session_id.clone(),
            addr: ctx.address(),
        });
    }
    fn stopped(&mut self, ctx: &mut Self::Context) {
        log::info!("WsSession stopped: user_id={}", self.user_id);
        self.server.do_send(Disconnect {
            user_id: self.user_id.clone(),
            session_id: self.session_id.clone(),
        });
        if let Some(ch) = self.joined.take() {
            self.server.do_send(Leave {
                channel_id: ch,
                addr: ctx.address(),
                user_id: self.user_id.clone(),
            });
        }
        if let Some(voice_ch) = self.voice_channel.take() {
            self.server.do_send(super::server::LeaveVoice {
                channel_id: voice_ch,
                user_id: self.user_id.clone(),
                session_id: self.session_id.clone(),
            });
        }
        // Set presence to offline on disconnect
        let user_id = self.user_id.clone();
        let db = self.db.clone();
        ctx.spawn(
            async move {
                let _ = sqlx::query(
                    "UPDATE presence SET status = 'offline', updated_at = ?, last_heartbeat = ? WHERE user_id = ?",
                )
                .bind(Utc::now())
                .bind(Utc::now())
                .bind(&user_id)
                .execute(&db.0)
                .await;
            }
            .into_actor(self),
        );
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct ServerMsg {
    pub payload: String,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct RoomState {
    pub channel_id: String,
    pub voice_users: Vec<(String, String)>,
}

impl Handler<ServerMsg> for WsSession {
    type Result = ();
    fn handle(&mut self, msg: ServerMsg, ctx: &mut Self::Context) {
        ctx.text(msg.payload);
    }
}

impl Handler<RoomState> for WsSession {
    type Result = ();
    fn handle(&mut self, msg: RoomState, ctx: &mut Self::Context) {
        let payload = serde_json::json!({
            "type": "room_state",
            "channel_id": msg.channel_id,
            "voice_users": msg.voice_users
        })
        .to_string();
        ctx.text(payload);
    }
}

/// Check if user can read this channel
async fn can_read(db: &crate::db::Db, user_id: &str, channel_id: &str) -> bool {
    sqlx::query_scalar::<_, i64>(
        "SELECT can_read FROM channel_members WHERE channel_id = ? AND user_id = ?",
    )
    .bind(channel_id)
    .bind(user_id)
    .fetch_optional(&db.0)
    .await
    .ok()
    .flatten()
    .map(|v| v != 0)
    .unwrap_or(false)
}

/// Check if user can write to this channel
async fn can_write(db: &crate::db::Db, user_id: &str, channel_id: &str) -> bool {
    sqlx::query_scalar::<_, i64>(
        "SELECT can_write FROM channel_members WHERE channel_id = ? AND user_id = ?",
    )
    .bind(channel_id)
    .bind(user_id)
    .fetch_optional(&db.0)
    .await
    .ok()
    .flatten()
    .map(|v| v != 0)
    .unwrap_or(false)
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientEvent {
    Join {
        channel_id: String,
    },
    Leave {
        channel_id: String,
    },
    ChatMessage {
        channel_id: String,
        content: String,
    },
    Typing {
        channel_id: String,
        started: bool,
    },
    WebrtcSignal {
        channel_id: String,
        to_user_id: String,
        to_session_id: Option<String>,
        data: serde_json::Value,
    },
    JoinCall {
        channel_id: String,
    },
    LeaveCall {
        channel_id: String,
    },
    #[serde(rename = "shareplay_action")]
    SharePlayAction {
        channel_id: String,
        action_type: String,
        data: Option<String>,
    },
    Ping,
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsSession {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Text(text)) => {
                log::debug!("WsSession received text: {}", text);
                if let Ok(ev) = serde_json::from_str::<ClientEvent>(&text) {
                    match ev {
                        ClientEvent::Join { channel_id } => {
                            let db = self.db.clone();
                            let user_id = self.user_id.clone();
                            let server = self.server.clone();
                            let addr = ctx.address();
                            let cid = channel_id.clone();
                            ctx.spawn(
                                async move {
                                    if can_read(&db, &user_id, &cid).await {
                                        server.do_send(Join {
                                            channel_id: cid,
                                            addr,
                                        });
                                    } else {
                                        log::warn!(
                                            "User {} denied access to channel {}",
                                            user_id,
                                            cid
                                        );
                                    }
                                }
                                .into_actor(self),
                            );
                            // Set joined optimistically - we still track the channel locally
                            self.joined = Some(channel_id);
                        }
                        ClientEvent::Leave { channel_id } => {
                            if self.joined.as_deref() == Some(&channel_id) {
                                self.server.do_send(Leave {
                                    channel_id,
                                    addr: ctx.address(),
                                    user_id: self.user_id.clone(),
                                });
                                self.joined = None;
                            }
                        }
                        ClientEvent::ChatMessage {
                            channel_id,
                            content,
                        } => {
                            let db = self.db.clone();
                            let user_id = self.user_id.clone();
                            let server = self.server.clone();
                            ctx.spawn(
                                async move {
                                    if can_write(&db, &user_id, &channel_id).await {
                                        let payload = serde_json::json!({
                                            "type": "chat_message",
                                            "channel_id": channel_id,
                                            "user_id": user_id,
                                            "content": content,
                                        })
                                        .to_string();
                                        server.do_send(Broadcast {
                                            channel_id,
                                            payload,
                                        });
                                    } else {
                                        log::warn!(
                                            "User {} denied write to channel {}",
                                            user_id,
                                            channel_id
                                        );
                                    }
                                }
                                .into_actor(self),
                            );
                        }
                        ClientEvent::Typing {
                            channel_id,
                            started,
                        } => {
                            let db = self.db.clone();
                            let user_id = self.user_id.clone();
                            let server = self.server.clone();
                            ctx.spawn(
                                async move {
                                    if can_read(&db, &user_id, &channel_id).await {
                                        let payload = serde_json::json!({
                                            "type": "typing",
                                            "channel_id": channel_id,
                                            "user_id": user_id,
                                            "started": started
                                        })
                                        .to_string();
                                        server.do_send(Broadcast {
                                            channel_id,
                                            payload,
                                        });
                                    }
                                }
                                .into_actor(self),
                            );
                        }
                        ClientEvent::WebrtcSignal {
                            channel_id,
                            to_user_id,
                            to_session_id,
                            data,
                        } => {
                            let payload = serde_json::json!({
                                "type": "webrtc_signal",
                                "channel_id": channel_id,
                                "from_user_id": self.user_id,
                                "from_session_id": self.session_id,
                                "to_session_id": to_session_id,
                                "data": data
                            })
                            .to_string();
                            self.server.do_send(DirectSignal {
                                to_user_id,
                                to_session_id,
                                payload,
                            });
                        }
                        ClientEvent::Ping => {
                            ctx.text(r#"{"type":"pong"}"#);
                        }
                        ClientEvent::JoinCall { channel_id } => {
                            log::info!(
                                "WsSession handling JoinCall: user_id={}, session_id={}, channel_id={}",
                                self.user_id,
                                self.session_id,
                                channel_id
                            );
                            let db = self.db.clone();
                            let user_id = self.user_id.clone();
                            let session_id = self.session_id.clone();
                            let server = self.server.clone();
                            let cid = channel_id.clone();
                            ctx.spawn(
                                async move {
                                    if can_read(&db, &user_id, &cid).await {
                                        server.do_send(super::server::JoinVoice {
                                            channel_id: cid,
                                            user_id,
                                            session_id,
                                        });
                                    } else {
                                        log::warn!(
                                            "User {} denied voice access to channel {}",
                                            user_id,
                                            cid
                                        );
                                    }
                                }
                                .into_actor(self),
                            );
                            self.voice_channel = Some(channel_id);
                        }
                        ClientEvent::LeaveCall { channel_id } => {
                            log::info!(
                                "WsSession handling LeaveCall: user_id={}, session_id={}, channel_id={}",
                                self.user_id,
                                self.session_id,
                                channel_id
                            );
                            self.voice_channel = None;
                            self.server.do_send(super::server::LeaveVoice {
                                channel_id,
                                user_id: self.user_id.clone(),
                                session_id: self.session_id.clone(),
                            });
                        }
                        ClientEvent::SharePlayAction {
                            channel_id,
                            action_type,
                            data,
                        } => {
                            let db = self.db.clone();
                            let user_id = self.user_id.clone();
                            let server = self.server.clone();
                            let cid = channel_id.clone();
                            ctx.spawn(
                                async move {
                                    // Must be in voice/read permission to control?
                                    // For now check read permission
                                    if can_read(&db, &user_id, &cid).await {
                                        log::info!("WsSession sending SharePlayAction to server: user_id={}, channel_id={}, action={}", user_id, cid, action_type);
                                        server.do_send(SharePlayAction {
                                            channel_id: cid,
                                            user_id,
                                            action_type,
                                            data,
                                        });
                                    }
                                }
                                .into_actor(self),
                            );
                        }
                    }
                }
            }
            Ok(ws::Message::Ping(bytes)) => ctx.pong(&bytes),
            Ok(ws::Message::Close(_)) => ctx.stop(),
            _ => {}
        }
    }
}
