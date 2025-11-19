use actix::{Actor, ActorContext, Addr, AsyncContext, Handler, Message, StreamHandler, WrapFuture};
use actix_web::{HttpRequest, HttpResponse, web, Error};
use actix_web_actors::ws;
use chrono::Utc;
use serde::{Serialize, Deserialize};
use crate::{auth, config::Config, db::Db};
use super::server::{ChatServer, Join, Leave, Broadcast};

pub async fn ws_route(req: HttpRequest, stream: web::Payload, cfg: web::Data<Config>, db: web::Data<Db>, srv: web::Data<actix::Addr<ChatServer>>) -> Result<HttpResponse, Error> {
    let token = req.query_string().split('&')
        .find_map(|kv| kv.split_once('='))
        .filter(|(k,_)| *k == "token")
        .map(|(_,v)| v.to_string());

    let claims = match token {
        Some(t) => auth::verify_access_token(&t, &cfg).map_err(|_| actix_web::error::ErrorUnauthorized("bad token"))?,
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

    let session = WsSession {
        user_id,
        server: srv.get_ref().clone(),
        joined: None,
        db: db.get_ref().clone(),
    };
    ws::start(session, &req, stream)

}

pub struct WsSession {
    pub user_id: String,
    pub server: Addr<ChatServer>,
    pub joined: Option<String>,
    pub db: Db
}

impl Actor for WsSession {
    type Context = ws::WebsocketContext<Self>;
    fn stopped(&mut self, ctx: &mut Self::Context) {
        if let Some(ch) = self.joined.take() {
            self.server.do_send(Leave { channel_id: ch, addr: ctx.address(), user_id: self.user_id.clone() });
        }
        // Set presence to offline on disconnect
        let user_id = self.user_id.clone();
        let db = self.db.clone();
        ctx.spawn(async move {
            let _ = sqlx::query("UPDATE presence SET status = 'offline', updated_at = ?, last_heartbeat = ? WHERE user_id = ?")
                .bind(Utc::now()).bind(Utc::now()).bind(&user_id).execute(&db.0).await;
        }.into_actor(self));
    }
}


#[derive(Message)]
#[rtype(result="()")]
pub struct ServerMsg { pub payload: String }

#[derive(Message)]
#[rtype(result="()")]
pub struct RoomState { pub voice_users: Vec<String> }

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
            "voice_users": msg.voice_users
        }).to_string();
        ctx.text(payload);
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag="type", rename_all="snake_case")]
enum ClientEvent {
    Join { channel_id: String },
    Leave { channel_id: String },
    ChatMessage { channel_id: String, content: String },
    Typing { channel_id: String, started: bool },
    WebrtcSignal { channel_id: String, to_user_id: String, data: serde_json::Value },
    JoinCall { channel_id: String },
    LeaveCall { channel_id: String },
    Ping,
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsSession {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Text(text)) => {
                if let Ok(ev) = serde_json::from_str::<ClientEvent>(&text) {
                    match ev {
                        ClientEvent::Join { channel_id } => {
                            self.joined = Some(channel_id.clone());
                            self.server.do_send(Join { channel_id, addr: ctx.address() });
                        }
                        ClientEvent::Leave { channel_id } => {
                            if self.joined.as_deref() == Some(&channel_id) {
                                self.server.do_send(Leave { channel_id, addr: ctx.address(), user_id: self.user_id.clone() });
                                self.joined = None;
                            }
                        }
                        ClientEvent::ChatMessage { channel_id, content } => {
                            // In real impl: validate membership, persist to DB, then broadcast.
                            let payload = serde_json::json!({
                                "type": "chat_message",
                                "channel_id": channel_id,
                                "user_id": self.user_id,
                                "content": content,
                            }).to_string();
                            self.server.do_send(Broadcast { channel_id, payload });
                        }
                        ClientEvent::Typing { channel_id, started } => {
                            let payload = serde_json::json!({
                                "type": "typing",
                                "channel_id": channel_id,
                                "user_id": self.user_id,
                                "started": started
                            }).to_string();
                            self.server.do_send(Broadcast { channel_id, payload });
                        }
                        ClientEvent::WebrtcSignal { channel_id, to_user_id: _, data } => {
                            // For MVP, broadcast to room; in production you may direct-route
                            let payload = serde_json::json!({
                                "type": "webrtc_signal",
                                "channel_id": channel_id,
                                "from_user_id": self.user_id,
                                "data": data
                            }).to_string();
                            self.server.do_send(Broadcast { channel_id, payload });
                        }
                        ClientEvent::Ping => {
                            ctx.text(r#"{"type":"pong"}"#);
                        }
                        ClientEvent::JoinCall { channel_id } => {
                            self.server.do_send(super::server::JoinVoice { channel_id, user_id: self.user_id.clone() });
                        }
                        ClientEvent::LeaveCall { channel_id } => {
                            self.server.do_send(super::server::LeaveVoice { channel_id, user_id: self.user_id.clone() });
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