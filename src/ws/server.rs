use actix::{Actor, AsyncContext, Context, Handler, Message};
use std::collections::{HashMap, HashSet};

pub struct ChatServer {
    rooms: HashMap<String, HashSet<actix::Addr<super::session::WsSession>>>,
    voice_participants: HashMap<String, HashSet<String>>, // channel_id -> set of user_ids
}

impl ChatServer {
    pub fn new() -> Self {
        Self { 
            rooms: HashMap::new(),
            voice_participants: HashMap::new(),
        }
    }
}

impl Actor for ChatServer {
    type Context = Context<Self>;
}

#[derive(Message)]
#[rtype(result="()")]
pub struct Join { pub channel_id: String, pub addr: actix::Addr<super::session::WsSession> }

#[derive(Message)]
#[rtype(result="()")]
pub struct Leave { pub channel_id: String, pub addr: actix::Addr<super::session::WsSession>, pub user_id: String }

#[derive(Message)]
#[rtype(result="()")]
pub struct Broadcast { pub channel_id: String, pub payload: String }

#[derive(Message)]
#[rtype(result="()")]
pub struct JoinVoice { pub channel_id: String, pub user_id: String }

#[derive(Message)]
#[rtype(result="()")]
pub struct LeaveVoice { pub channel_id: String, pub user_id: String }

impl Handler<Join> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: Join, _: &mut Context<Self>) {
        self.rooms.entry(msg.channel_id.clone()).or_default().insert(msg.addr.clone());
        
        // Send current voice participants to the joining user
        if let Some(voice_users) = self.voice_participants.get(&msg.channel_id) {
            let users_vec: Vec<String> = voice_users.iter().cloned().collect();
            msg.addr.do_send(super::session::RoomState { voice_users: users_vec });
        }
    }
}
impl Handler<Leave> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: Leave, ctx: &mut Context<Self>) {
        if let Some(s) = self.rooms.get_mut(&msg.channel_id) {
            s.retain(|a| a != &msg.addr);
        }
        // If user was in voice, remove them
        if let Some(voice_users) = self.voice_participants.get_mut(&msg.channel_id) {
            if voice_users.remove(&msg.user_id) {
                // Broadcast voice_left
                let payload = serde_json::json!({
                    "type": "voice_left",
                    "channel_id": msg.channel_id,
                    "user_id": msg.user_id
                }).to_string();
                // We can reuse the Broadcast handler logic or call it directly? 
                // Calling do_send to self is safer to avoid borrow checker issues if we extracted logic
                ctx.notify(Broadcast { channel_id: msg.channel_id, payload });
            }
        }
    }
}
impl Handler<Broadcast> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: Broadcast, _: &mut Context<Self>) {
        if let Some(sessions) = self.rooms.get(&msg.channel_id) {
            for s in sessions {
                s.do_send(super::session::ServerMsg { payload: msg.payload.clone() });
            }
        }
    }
}

impl Handler<JoinVoice> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: JoinVoice, ctx: &mut Context<Self>) {
        let voice_users = self.voice_participants.entry(msg.channel_id.clone()).or_default();
        if voice_users.insert(msg.user_id.clone()) {
            let payload = serde_json::json!({
                "type": "voice_joined",
                "channel_id": msg.channel_id,
                "user_id": msg.user_id
            }).to_string();
            ctx.notify(Broadcast { channel_id: msg.channel_id, payload });
        }
    }
}

impl Handler<LeaveVoice> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: LeaveVoice, ctx: &mut Context<Self>) {
        if let Some(voice_users) = self.voice_participants.get_mut(&msg.channel_id) {
            if voice_users.remove(&msg.user_id) {
                let payload = serde_json::json!({
                    "type": "voice_left",
                    "channel_id": msg.channel_id,
                    "user_id": msg.user_id
                }).to_string();
                ctx.notify(Broadcast { channel_id: msg.channel_id, payload });
            }
        }
    }
}