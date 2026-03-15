use crate::push::PushRelayRuntime;
use crate::shareplay::SharePlayState;
use actix::{Actor, AsyncContext, Context, Handler, Message};
use std::collections::{HashMap, HashSet};
use std::time::Duration;


const VOICE_RECONNECT_GRACE: Duration = Duration::from_secs(10);

pub struct ChatServer {
    rooms: HashMap<String, HashSet<actix::Addr<super::session::WsSession>>>,
    room_members: HashMap<String, HashSet<(String, String)>>, // channel_id -> set of (user_id, session_id)
    voice_participants: HashMap<String, HashSet<(String, String)>>, // channel_id -> set of (user_id, session_id)
    user_sessions: HashMap<String, HashMap<String, actix::Addr<super::session::WsSession>>>, // user_id -> { session_id -> addr }
    pending_bridge_call_left: HashMap<(String, String), u64>,
    pending_voice_disconnect_left: HashMap<(String, String, String), u64>,
    pending_shareplay_cleanup: HashMap<String, u64>,
    pub shareplay_states: HashMap<String, SharePlayState>,
    bridge_runtime: Option<crate::bridge::BridgeRuntime>,
    push_runtime: Option<PushRelayRuntime>,
}

impl ChatServer {
    pub fn new(
        bridge_runtime: Option<crate::bridge::BridgeRuntime>,
        push_runtime: Option<PushRelayRuntime>,
    ) -> Self {
        Self {
            rooms: HashMap::new(),
            room_members: HashMap::new(),
            voice_participants: HashMap::new(),
            user_sessions: HashMap::new(),
            pending_bridge_call_left: HashMap::new(),
            pending_voice_disconnect_left: HashMap::new(),
            pending_shareplay_cleanup: HashMap::new(),
            shareplay_states: HashMap::new(),
            bridge_runtime,
            push_runtime,
        }
    }
}

impl Actor for ChatServer {
    type Context = Context<Self>;
}

impl ChatServer {
    fn active_user_ids_for_channel(&self, channel_id: &str, include_voice: bool) -> Vec<String> {
        let mut users = HashSet::new();
        if let Some(room_members) = self.room_members.get(channel_id) {
            for (user_id, _) in room_members {
                users.insert(user_id.clone());
            }
        }
        if include_voice {
            if let Some(voice_users) = self.voice_participants.get(channel_id) {
                for (user_id, _) in voice_users {
                    users.insert(user_id.clone());
                }
            }
        }
        users.into_iter().collect()
    }

    fn trigger_pending_downloads(&mut self, channel_id: String, ctx: &mut Context<Self>) {
        if let Some(state) = self.shareplay_states.get_mut(&channel_id) {
            let active_count = state
                .queue
                .iter()
                .filter(|i| i.download_status == "grabbing" || i.download_status == "downloading")
                .count();

            let slots = if active_count < 2 {
                2 - active_count
            } else {
                0
            };

            // Collect items to download (mark as grabbing first to prevent re-triggering)
            let mut to_download = Vec::new();
            for item in state.queue.iter_mut() {
                if to_download.len() >= slots {
                    break;
                }
                if item.download_status == "pending" {
                    item.download_status = "grabbing".to_string();
                    to_download.push((item.url.clone(), item.id.clone()));
                }
            }

            // Now spawn the downloads
            for (url, id) in to_download {
                log::info!(
                    "Triggering pending download: channel_id={}, id={}",
                    channel_id,
                    id
                );
                crate::shareplay::start_single_download(url, id, ctx.address(), channel_id.clone());
            }
        }
    }

    fn clear_pending_bridge_call_left(&mut self, channel_id: &str, user_id: &str) -> bool {
        let key = (channel_id.to_string(), user_id.to_string());
        self.pending_bridge_call_left.remove(&key).is_some()
    }

    fn clear_pending_voice_disconnect_left(
        &mut self,
        channel_id: &str,
        user_id: &str,
        session_id: &str,
    ) {
        let key = (
            channel_id.to_string(),
            user_id.to_string(),
            session_id.to_string(),
        );
        self.pending_voice_disconnect_left.remove(&key);
    }

    fn clear_pending_shareplay_cleanup(&mut self, channel_id: &str) {
        self.pending_shareplay_cleanup.remove(channel_id);
    }

    fn queue_bridge_call_left(
        &mut self,
        ctx: &mut Context<Self>,
        channel_id: String,
        user_id: String,
    ) {
        let key = (channel_id.clone(), user_id.clone());
        let version = self
            .pending_bridge_call_left
            .entry(key.clone())
            .and_modify(|v| *v += 1)
            .or_insert(1);
        let expected_version = *version;
        let bridge_runtime = self.bridge_runtime.clone();
        let channel_id = key.0.clone();
        let user_id = key.1.clone();
        ctx.run_later(VOICE_RECONNECT_GRACE, move |act, _ctx| {
            if act.pending_bridge_call_left.get(&key).copied().unwrap_or(0) != expected_version {
                return;
            }
            act.pending_bridge_call_left.remove(&key);
            if let Some(bridge_runtime) = bridge_runtime {
                bridge_runtime.record_call_left(channel_id, user_id);
            }
        });
    }

    fn queue_voice_disconnect_left(
        &mut self,
        ctx: &mut Context<Self>,
        channel_id: String,
        user_id: String,
        session_id: String,
    ) {
        let key = (channel_id.clone(), user_id.clone(), session_id.clone());
        let version = self
            .pending_voice_disconnect_left
            .entry(key.clone())
            .and_modify(|v| *v += 1)
            .or_insert(1);
        let expected_version = *version;

        ctx.run_later(VOICE_RECONNECT_GRACE, move |act, ctx| {
            if act
                .pending_voice_disconnect_left
                .get(&key)
                .copied()
                .unwrap_or(0)
                != expected_version
            {
                return;
            }
            act.pending_voice_disconnect_left.remove(&key);
            act.remove_voice_session(&key.0, &key.1, &key.2, ctx);
        });
    }

    fn queue_shareplay_cleanup(&mut self, ctx: &mut Context<Self>, channel_id: String) {
        let version = self
            .pending_shareplay_cleanup
            .entry(channel_id.clone())
            .and_modify(|v| *v += 1)
            .or_insert(1);
        let expected_version = *version;
        let chan = channel_id.clone();
        ctx.run_later(VOICE_RECONNECT_GRACE, move |act, ctx| {
            if act
                .pending_shareplay_cleanup
                .get(&chan)
                .copied()
                .unwrap_or(0)
                != expected_version
            {
                return;
            }
            act.pending_shareplay_cleanup.remove(&chan);

            if act
                .voice_participants
                .get(&chan)
                .is_some_and(|sessions| !sessions.is_empty())
            {
                return;
            }

            if let Some(state) = act.shareplay_states.remove(&chan) {
                crate::shareplay::cleanup_channel_files(&state);
                log::info!("Cleaned up SharePlay for empty channel {}", chan);

                let clear_payload = serde_json::json!({
                    "type": "shareplay_cleared",
                    "channel_id": chan
                })
                .to_string();
                ctx.notify(Broadcast {
                    channel_id: chan.clone(),
                    payload: clear_payload,
                });
            }
        });
    }

    fn broadcast_voice_left(
        &self,
        ctx: &mut Context<Self>,
        channel_id: &str,
        user_id: &str,
        session_id: &str,
    ) {
        let payload = serde_json::json!({
            "type": "voice_left",
            "channel_id": channel_id,
            "user_id": user_id,
            "session_id": session_id
        })
        .to_string();
        ctx.notify(Broadcast {
            channel_id: channel_id.to_string(),
            payload,
        });
    }

    fn remove_voice_session(
        &mut self,
        channel_id: &str,
        user_id: &str,
        session_id: &str,
        ctx: &mut Context<Self>,
    ) {
        let Some(voice_users) = self.voice_participants.get_mut(channel_id) else {
            return;
        };
        let already_removed = !voice_users.remove(&(user_id.to_string(), session_id.to_string()));
        if already_removed {
            return;
        }
        let user_still_in_call = voice_users.iter().any(|(uid, _)| uid == user_id);
        let voice_channel_empty = voice_users.is_empty();

        self.broadcast_voice_left(ctx, channel_id, user_id, session_id);

        if !user_still_in_call {
            self.queue_bridge_call_left(ctx, channel_id.to_string(), user_id.to_string());
        }

        if voice_channel_empty {
            self.voice_participants.remove(channel_id);
            self.queue_shareplay_cleanup(ctx, channel_id.to_string());
        }
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct Join {
    pub channel_id: String,
    pub addr: actix::Addr<super::session::WsSession>,
    pub user_id: String,
    pub session_id: String,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct Leave {
    pub channel_id: String,
    pub addr: actix::Addr<super::session::WsSession>,
    pub user_id: String,
    pub session_id: String,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct Broadcast {
    pub channel_id: String,
    pub payload: String,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct JoinVoice {
    pub channel_id: String,
    pub user_id: String,
    pub session_id: String,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct LeaveVoice {
    pub channel_id: String,
    pub user_id: String,
    pub session_id: String,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct DisconnectVoice {
    pub channel_id: String,
    pub user_id: String,
    pub session_id: String,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct Connect {
    pub user_id: String,
    pub session_id: String,
    pub addr: actix::Addr<super::session::WsSession>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct Disconnect {
    pub user_id: String,
    pub session_id: String,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct DirectSignal {
    pub to_user_id: String,
    pub to_session_id: Option<String>,
    pub payload: String,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct BroadcastAll {
    pub payload: String,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct NotifyUsers {
    pub user_ids: Vec<String>,
    pub payload: String,
    pub skip_channel: Option<String>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct SharePlayAction {
    pub channel_id: String,
    pub user_id: String,
    pub action_type: String, // "play", "pause", "next", "prev", "seek", "add", "track", "toggle_repeat", "remove"
    pub data: Option<String>, // url for add, timestamp for seek, index for track
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct SharePlayDownloadResult {
    pub channel_id: String,
    pub id: String,
    pub success: bool,
    pub title: String,
    pub file_path: Option<String>,
    pub thumbnail_path: Option<String>,
    pub duration: u64,
    pub error: Option<String>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct SharePlayMetadataResult {
    pub channel_id: String,
    pub id: String,
    pub success: bool,
    pub title: String,
    pub duration: u64,
    pub thumbnail_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct SharePlayPlaylistResult {
    pub channel_id: String,
    pub placeholder_id: String,
    pub entries: Vec<(String, String, u64)>, // url, title, duration
}

#[derive(Message)]
#[rtype(result = "Result<Option<String>, ()>")]
pub struct GetSharePlaySongId {
    pub channel_id: String,
}

#[derive(Message)]
#[rtype(result = "Result<Option<String>, ()>")]
pub struct GetSharePlaySongPath {
    pub song_id: String,
}

#[derive(Message)]
#[rtype(result = "Result<Option<String>, ()>")]
pub struct GetSharePlayThumbnailPath {
    pub item_id: String,
}

#[derive(Message)]
#[rtype(result = "Result<Vec<String>, ()>")]
pub struct GetChannelActiveUsers {
    pub channel_id: String,
    pub include_voice: bool,
}

impl Handler<Join> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: Join, _: &mut Context<Self>) {
        self.rooms
            .entry(msg.channel_id.clone())
            .or_default()
            .insert(msg.addr.clone());
        self.room_members
            .entry(msg.channel_id.clone())
            .or_default()
            .insert((msg.user_id, msg.session_id));

        // Send current voice participants to the joining user
        if let Some(voice_users) = self.voice_participants.get(&msg.channel_id) {
            // For room state, we still just send user IDs to avoid leaking session IDs unnecessarily
            // The frontend only needs to know who is in the room.
            // However, for signaling, it WILL need session IDs of others.
            // Let's include session IDs in room state too.
            let users_vec: Vec<(String, String)> = voice_users.iter().cloned().collect();
            msg.addr.do_send(super::session::RoomState {
                channel_id: msg.channel_id.clone(),
                voice_users: users_vec,
            });
        }

        // Send SharePlay state if exists
        if let Some(state) = self.shareplay_states.get(&msg.channel_id) {
            let payload = serde_json::json!({
                "type": "shareplay_state",
                "channel_id": msg.channel_id,
                "state": state
            })
            .to_string();
            msg.addr.do_send(super::session::ServerMsg { payload });
        }
    }
}
impl Handler<Leave> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: Leave, ctx: &mut Context<Self>) {
        if let Some(s) = self.rooms.get_mut(&msg.channel_id) {
            s.retain(|a| a != &msg.addr);
        }
        if let Some(members) = self.room_members.get_mut(&msg.channel_id) {
            members.remove(&(msg.user_id.clone(), msg.session_id.clone()));
            if members.is_empty() {
                self.room_members.remove(&msg.channel_id);
            }
        }
        self.remove_voice_session(
            msg.channel_id.as_str(),
            msg.user_id.as_str(),
            msg.session_id.as_str(),
            ctx,
        );
    }
}
impl Handler<Broadcast> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: Broadcast, _: &mut Context<Self>) {
        if let Some(sessions) = self.rooms.get(&msg.channel_id) {
            for s in sessions {
                s.do_send(super::session::ServerMsg {
                    payload: msg.payload.clone(),
                });
            }
        }
    }
}

impl Handler<JoinVoice> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: JoinVoice, ctx: &mut Context<Self>) {
        log::info!(
            "ChatServer handling JoinVoice: user_id={}, session_id={}, channel_id={}",
            msg.user_id,
            msg.session_id,
            msg.channel_id
        );
        let user_resumed = self.clear_pending_bridge_call_left(&msg.channel_id, &msg.user_id);
        self.clear_pending_shareplay_cleanup(&msg.channel_id);
        let (inserted, user_already_in_call, channel_was_empty) = {
            let voice_users = self
                .voice_participants
                .entry(msg.channel_id.clone())
                .or_default();
            let user_already_in_call = voice_users.iter().any(|(uid, _)| uid == &msg.user_id);
            let channel_was_empty = voice_users.is_empty();
            let inserted = voice_users.insert((msg.user_id.clone(), msg.session_id.clone()));
            (inserted, user_already_in_call, channel_was_empty)
        };
        if inserted {
            if !user_already_in_call {
                if !user_resumed {
                    if let Some(bridge_runtime) = &self.bridge_runtime {
                        bridge_runtime
                            .record_call_joined(msg.channel_id.clone(), msg.user_id.clone());
                    }
                }
            }
            if channel_was_empty && !user_resumed {
                if let Some(push_runtime) = &self.push_runtime {
                    push_runtime.record_call_started(
                        msg.channel_id.clone(),
                        msg.user_id.clone(),
                        self.active_user_ids_for_channel(&msg.channel_id, true),
                    );
                }
            }
            let payload = serde_json::json!({
                "type": "voice_joined",
                "channel_id": msg.channel_id,
                "user_id": msg.user_id,
                "session_id": msg.session_id
            })
            .to_string();
            ctx.notify(Broadcast {
                channel_id: msg.channel_id,
                payload,
            });
        }
    }
}

impl Handler<LeaveVoice> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: LeaveVoice, ctx: &mut Context<Self>) {
        log::info!(
            "ChatServer handling LeaveVoice: user_id={}, session_id={}, channel_id={}",
            msg.user_id,
            msg.session_id,
            msg.channel_id
        );
        self.remove_voice_session(
            msg.channel_id.as_str(),
            msg.user_id.as_str(),
            msg.session_id.as_str(),
            ctx,
        );
    }
}

impl Handler<DisconnectVoice> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: DisconnectVoice, ctx: &mut Context<Self>) {
        log::info!(
            "ChatServer handling DisconnectVoice: user_id={}, session_id={}, channel_id={}",
            msg.user_id,
            msg.session_id,
            msg.channel_id
        );
        self.clear_pending_voice_disconnect_left(&msg.channel_id, &msg.user_id, &msg.session_id);
        self.queue_voice_disconnect_left(ctx, msg.channel_id, msg.user_id, msg.session_id);
    }
}

impl Handler<Connect> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: Connect, _: &mut Context<Self>) {
        log::info!(
            "ChatServer connected: user_id={}, session_id={}",
            msg.user_id,
            msg.session_id
        );
        self.user_sessions
            .entry(msg.user_id)
            .or_default()
            .insert(msg.session_id, msg.addr);
    }
}

impl Handler<Disconnect> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) {
        log::info!(
            "ChatServer disconnected: user_id={}, session_id={}",
            msg.user_id,
            msg.session_id
        );
        if let Some(sessions) = self.user_sessions.get_mut(&msg.user_id) {
            sessions.remove(&msg.session_id);
            if sessions.is_empty() {
                self.user_sessions.remove(&msg.user_id);
            }
        }
    }
}

impl Handler<GetChannelActiveUsers> for ChatServer {
    type Result = Result<Vec<String>, ()>;

    fn handle(&mut self, msg: GetChannelActiveUsers, _: &mut Context<Self>) -> Self::Result {
        Ok(self.active_user_ids_for_channel(&msg.channel_id, msg.include_voice))
    }
}

impl Handler<DirectSignal> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: DirectSignal, _: &mut Context<Self>) {
        if let Some(sessions) = self.user_sessions.get(&msg.to_user_id) {
            if let Some(sid) = msg.to_session_id {
                if let Some(s) = sessions.get(&sid) {
                    s.do_send(super::session::ServerMsg {
                        payload: msg.payload,
                    });
                }
            } else {
                for s in sessions.values() {
                    s.do_send(super::session::ServerMsg {
                        payload: msg.payload.clone(),
                    });
                }
            }
        }
    }
}

impl Handler<NotifyUsers> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: NotifyUsers, _: &mut Context<Self>) {
        let skip_addrs: HashSet<actix::Addr<super::session::WsSession>> = match &msg.skip_channel {
            Some(ch) => self.rooms.get(ch).cloned().unwrap_or_default(),
            None => HashSet::new(),
        };

        for uid in msg.user_ids {
            if let Some(sessions) = self.user_sessions.get(&uid) {
                for addr in sessions.values() {
                    // If skip_channel is set, check if this session is in that room
                    // Note: session instance identity (Addr) check
                    if msg.skip_channel.is_some() && skip_addrs.contains(addr) {
                        continue;
                    }
                    addr.do_send(super::session::ServerMsg {
                        payload: msg.payload.clone(),
                    });
                }
            }
        }
    }
}

impl Handler<BroadcastAll> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: BroadcastAll, _: &mut Context<Self>) {
        for sessions in self.user_sessions.values() {
            for addr in sessions.values() {
                addr.do_send(super::session::ServerMsg {
                    payload: msg.payload.clone(),
                });
            }
        }
    }
}

impl Handler<SharePlayAction> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: SharePlayAction, ctx: &mut Context<Self>) {
        let state = self
            .shareplay_states
            .entry(msg.channel_id.clone())
            .or_insert_with(SharePlayState::new);

        log::info!(
            "ChatServer handling SharePlayAction: channel_id={}, user_id={}, action={}",
            msg.channel_id,
            msg.user_id,
            msg.action_type
        );

        match msg.action_type.as_str() {
            "add" => {
                if let Some(url) = msg.data {
                    let id = state.add_item(url.clone());
                    // Trigger download
                    crate::shareplay::start_download(
                        url,
                        id,
                        ctx.address(),
                        msg.channel_id.clone(),
                    );
                }
            }
            "play" => state.play(),
            "pause" => state.pause(),
            "next" => state.next(),
            "prev" => state.prev(),
            "seek" => {
                if let Some(ts_str) = msg.data {
                    if let Ok(ts) = ts_str.parse::<f64>() {
                        state.seek(ts);
                    }
                }
            }
            "track" => {
                if let Some(idx_str) = msg.data {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        state.set_track(idx);
                    }
                }
            }
            "toggle_repeat" => state.toggle_repeat(),
            "remove" => {
                if let Some(idx_str) = msg.data {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        state.remove_item(idx);
                    }
                }
            }
            _ => {}
        }

        // Broadcast update
        let payload = serde_json::json!({
            "type": "shareplay_update",
            "channel_id": msg.channel_id,
            "state": state
        })
        .to_string();

        ctx.notify(Broadcast {
            channel_id: msg.channel_id,
            payload,
        });
    }
}

impl Handler<SharePlayDownloadResult> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: SharePlayDownloadResult, ctx: &mut Context<Self>) {
        log::info!(
            "ChatServer received SharePlayDownloadResult: channel_id={}, id={}, success={}",
            msg.channel_id,
            msg.id,
            msg.success
        );
        if let Some(state) = self.shareplay_states.get_mut(&msg.channel_id) {
            if msg.success {
                state.update_item_success(
                    &msg.id,
                    msg.title,
                    msg.file_path.unwrap_or_default(),
                    msg.thumbnail_path,
                    msg.duration,
                );
            } else {
                state.update_item_error(&msg.id, msg.error.unwrap_or("Unknown error".to_string()));
            }

            // Broadcast update
            let payload = serde_json::json!({
                "type": "shareplay_update",
                "channel_id": msg.channel_id,
                "state": state
            })
            .to_string();

            ctx.notify(Broadcast {
                channel_id: msg.channel_id.clone(),
                payload,
            });
        }

        // Always trigger next pending downloads even on success/failure
        self.trigger_pending_downloads(msg.channel_id, ctx);
    }
}

impl Handler<SharePlayPlaylistResult> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: SharePlayPlaylistResult, ctx: &mut Context<Self>) {
        log::info!(
            "ChatServer received SharePlayPlaylistResult: channel_id={}, entries={}",
            msg.channel_id,
            msg.entries.len()
        );
        if let Some(state) = self.shareplay_states.get_mut(&msg.channel_id) {
            // Remove placeholder
            let placeholder_idx = state.queue.iter().position(|i| i.id == msg.placeholder_id);
            if let Some(idx) = placeholder_idx {
                state.queue.remove(idx);
            }

            // Batch add items as pending
            for (url, title, duration) in msg.entries {
                let id = uuid::Uuid::new_v4().to_string();
                state.queue.push(crate::shareplay::QueueItem {
                    id,
                    url,
                    title,
                    file_path: None,
                    thumbnail_path: None,
                    download_error: None,
                    duration_seconds: duration,
                    download_status: "pending".to_string(),
                });
            }

            // Sync current index if it was pointing to the placeholder or empty
            if state.current_index.is_none() && !state.queue.is_empty() {
                state.current_index = Some(0);
            }

            // Broadcast update
            let payload = serde_json::json!({
                "type": "shareplay_update",
                "channel_id": msg.channel_id,
                "state": state
            })
            .to_string();

            ctx.notify(Broadcast {
                channel_id: msg.channel_id.clone(),
                payload,
            });

            // Trigger downloads
            self.trigger_pending_downloads(msg.channel_id, ctx);
        }
    }
}

impl Handler<SharePlayMetadataResult> for ChatServer {
    type Result = ();
    fn handle(&mut self, msg: SharePlayMetadataResult, ctx: &mut Context<Self>) {
        log::info!(
            "ChatServer received SharePlayMetadataResult: channel_id={}, id={}, success={}",
            msg.channel_id,
            msg.id,
            msg.success
        );
        if let Some(state) = self.shareplay_states.get_mut(&msg.channel_id) {
            if msg.success {
                state.update_item_metadata(&msg.id, msg.title, msg.duration, msg.thumbnail_path);
            } else {
                state.update_item_error(&msg.id, msg.error.unwrap_or("Unknown error".to_string()));
            }

            // Broadcast update
            let payload = serde_json::json!({
                "type": "shareplay_update",
                "channel_id": msg.channel_id,
                "state": state
            })
            .to_string();

            ctx.notify(Broadcast {
                channel_id: msg.channel_id.clone(),
                payload,
            });
        }

        // Always trigger next pending downloads
        self.trigger_pending_downloads(msg.channel_id, ctx);
    }
}

impl Handler<GetSharePlaySongId> for ChatServer {
    type Result = Result<Option<String>, ()>;

    fn handle(&mut self, msg: GetSharePlaySongId, _: &mut Context<Self>) -> Self::Result {
        if let Some(state) = self.shareplay_states.get(&msg.channel_id) {
            if let Some(idx) = state.current_index {
                if let Some(item) = state.queue.get(idx) {
                    return Ok(Some(item.id.clone()));
                }
            }
        }
        Ok(None)
    }
}

impl Handler<GetSharePlaySongPath> for ChatServer {
    type Result = Result<Option<String>, ()>;

    fn handle(&mut self, msg: GetSharePlaySongPath, _: &mut Context<Self>) -> Self::Result {
        // Search across all channels for this song ID
        for state in self.shareplay_states.values() {
            if let Some(item) = state.queue.iter().find(|i| i.id == msg.song_id) {
                return Ok(item.file_path.clone());
            }
        }
        Ok(None)
    }
}

impl Handler<GetSharePlayThumbnailPath> for ChatServer {
    type Result = Result<Option<String>, ()>;

    fn handle(&mut self, msg: GetSharePlayThumbnailPath, _: &mut Context<Self>) -> Self::Result {
        // Search across all channels for this item ID
        for state in self.shareplay_states.values() {
            if let Some(item) = state.queue.iter().find(|i| i.id == msg.item_id) {
                return Ok(item.thumbnail_path.clone());
            }
        }
        Ok(None)
    }
}
