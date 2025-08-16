use actix::{Actor, Context, Handler, Message};
use std::collections::{HashMap, HashSet};

pub struct ChatServer {
    rooms: HashMap<String, HashSet<actix::Addr<super::session::WsSession>>>,
}

impl ChatServer {
    pub fn new() -> Self {
        Self { rooms: HashMap::new() }
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
pub struct Leave { pub channel_id: String, pub addr: actix::Addr<super::session::WsSession> }

#[derive(Message)]
#[rtype(result="()")]
pub struct Broadcast { pub channel_id: String, pub payload: String }

impl Handler<Join> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: Join, _: &mut Context<Self>) {
        self.rooms.entry(msg.channel_id).or_default().insert(msg.addr);
    }
}
impl Handler<Leave> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: Leave, _: &mut Context<Self>) {
        if let Some(s) = self.rooms.get_mut(&msg.channel_id) {
            s.retain(|a| a != &msg.addr);
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