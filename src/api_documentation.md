# Stuffchat API Documentation
## Overview
Stuffchat is a real-time chat application with voice calls and SharePlay (music sharing) capabilities. The backend is written in Rust (Actix Web) and uses WebSocket for real-time events.
## Authentication
Authentication is token-based (JWT).
### Endpoints
- **Register**
    - `POST /api/auth/register`
    - Body: `{ "username": "...", "password": "...", "email": "..." (opt), "invite_code": "..." (opt) }`
    - Response: `{ "access_token": "...", "refresh_token": "...", "user_id": "...", "refresh_token_id": "..." }`
- **Login**
    - `POST /api/auth/login`
    - Body: `{ "username_or_email": "...", "password": "..." }`
    - Response: Same as Register.
- **Refresh Token**
    - `POST /api/auth/refresh`
    - Body: `{ "refresh_token_id": "...", "refresh_token": "..." }`
    - Response: Same as Register (new tokens).
- **Logout**
    - `POST /api/auth/logout`
    - Body: `{ "refresh_token_id": "..." }`
    - Headers: `Authorization: Bearer <access_token>`
## HTTP API
All endpoints below require `Authorization: Bearer <access_token>` header.
### Users
- `GET /api/users`: List all users (public info).
- `GET /api/users/me`: Get current user profile.
- `PATCH /api/users/me`: Update profile. Body: `{ "username": "...", "email": "..." }`
- `PUT /api/users/me/password`: Change password. Body: `{ "current_password": "...", "new_password": "..." }`
- `PUT /api/users/me/avatar`: Upload avatar (multipart form data).
- `GET /api/users/{id}`: Get user by ID.
- `GET /api/users/{id}/avatar`: Get user avatar (redirects to file).
### Channels
- `GET /api/channels`: List channels user is a member of.
- `POST /api/channels`: Create channel. Body: `{ "name": "...", "is_voice": bool, "is_private": bool, "members": [...] (opt, for private) }`
- `GET /api/channels/unread`: Get unread state for all channels.
- `PATCH /api/channels/{id}`: Edit channel. Body: `{ "name": "...", "is_voice": bool, "is_private": bool }`
- `DELETE /api/channels/{id}`: Delete channel.
- `POST /api/channels/{id}/read`: Mark message as read. Body: `{ "message_id": "..." }`
- `POST /api/channels/{id}/notified`: Mark message as notified. Body: `{ "message_id": "..." }`
- `GET /api/channels/{id}/ownership`: Check if user owns channel.
- `POST /api/channels/{id}/join`: Join a public channel.
- `POST /api/channels/{id}/leave`: Leave a channel.
- `GET /api/channels/{id}/members`: List channel members.
- `POST /api/channels/{id}/members`: Add/remove members. Body: `{ "add": [...], "remove": [...] }`
### Messages
- `GET /api/channels/{id}/messages`: List messages. Query: `?before=<message_id>&limit=50`.
- `POST /api/channels/{id}/messages`: Post message. Body: `{ "content": "..." (opt), "file_id": "..." (opt) }`
- `PATCH /api/messages/{id}`: Edit message. Body: `{ "content": "..." }`
- `DELETE /api/messages/{id}`: Delete message.
### Files
- `POST /api/files`: Upload file. Content-Type: `multipart/form-data`. Returns `{ "file_id": "..." }`.
- `GET /files/{id}/{filename}`: Download/view file. No auth required for this specific route (handled by signed URL or public access implication usually, but code shows standard GET).
### Invites
- `POST /api/invites`: Create invite code.
- `GET /api/invites`: List invites created by current user.
### Presence
- `POST /api/presence/heartbeat`: Update status. Body: `{ "status": "online" | "away" | "dnd" | "invisible" | "offline" }`
- `GET /api/presence/users`: Get presence. Query: `?ids=uid1,uid2...`
### SharePlay (HTTP)
- `GET /api/shareplay/{channel_id}/current`: Get current song ID.
- `GET /api/shareplay/song/{song_id}`: Stream song audio.
## WebSocket Protocol
**Endpoint**: `/ws?token=<access_token>`
### Client -> Server Events
Sent as JSON strings.
| Type | Payload | Description |
|------|---------|-------------|
| [join](file:///home/will/stuffchat/src/routes/channels.rs#332-354) | `{ "channel_id": "..." }` | Subscribe to channel events |
| [leave](file:///home/will/stuffchat/src/routes/channels.rs#355-368) | `{ "channel_id": "..." }` | Unsubscribe from channel events |
| `chat_message` | `{ "channel_id": "...", "content": "..." }` | Send a chat message |
| `typing` | `{ "channel_id": "...", "started": bool }` | Send typing indicator |
| `join_call` | `{ "channel_id": "..." }` | Join voice channel |
| `leave_call` | `{ "channel_id": "..." }` | Leave voice channel |
| `webrtc_signal` | `{ "channel_id": "...", "to_user_id": "...", "data": {...} }` | Send WebRTC signal |
| `shareplay_action` | `{ "channel_id": "...", "action_type": "...", "data": "..." }` | Control SharePlay |
| `ping` | `null` | Keepalive |
**SharePlay Actions**:
- [add](file:///home/will/stuffchat/src/shareplay.rs#49-68): data = URL
- [play](file:///home/will/stuffchat/src/shareplay.rs#101-108), [pause](file:///home/will/stuffchat/src/shareplay.rs#109-121), [next](file:///home/will/stuffchat/src/shareplay.rs#129-170), [prev](file:///home/will/stuffchat/src/shareplay.rs#171-192), [toggle_repeat](file:///home/will/stuffchat/src/shareplay.rs#203-210): data = null
- [seek](file:///home/will/stuffchat/src/shareplay.rs#122-128): data = timestamp string
- [track](file:///home/will/stuffchat/src/shareplay.rs#193-202): data = index string
- [remove](file:///home/will/stuffchat/src/shareplay.rs#211-259): data = index string
### Server -> Client Events
| Type | Payload | Description |
|------|---------|-------------|
| `connection_metadata` | `{ "session_id": "...", "server_time": "..." }` | Sent on connection |
| `message_created` | `{ "id": "...", "channel_id": "...", "user_id": "...", "content": "...", "file_url": "...", "created_at": "..." }` | New message |
| `message_edited` | `{ "id": "...", "channel_id": "...", "content": "...", "edited_at": "..." }` | Message edited |
| `message_deleted` | `{ "id": "...", "channel_id": "...", "deleted_at": "..." }` | Message deleted |
| `typing` | `{ "channel_id": "...", "user_id": "...", "started": bool }` | User typing status |
| `room_state` | `{ "channel_id": "...", "voice_users": [["uid", "sid"], ...] }` | Initial voice users |
| `voice_joined` | `{ "channel_id": "...", "user_id": "...", "session_id": "..." }` | User joined voice |
| `voice_left` | `{ "channel_id": "...", "user_id": "..." }` | User left voice |
| `webrtc_signal` | `{ "channel_id": "...", "from_user_id": "...", "data": ... }` | Incoming WebRTC signal |
| `shareplay_state` | `{ "channel_id": "...", "state": {...} }` | Initial SharePlay state |
| `shareplay_update` | `{ "channel_id": "...", "state": {...} }` | SharePlay state changed |
| `shareplay_cleared` | `{ "channel_id": "..." }` | SharePlay stopped |
| `pong` | `null` | Response to ping |
## SharePlay State Object
```json
{
  "queue": [
    {
      "id": "...",
      "url": "...",
      "title": "...",
      "duration_seconds": 123,
      "download_status": "grabbing" | "downloading" | "ready" | "error"
    }
  ],
  "current_index": 0, // or null
  "status": "playing" | "paused",
  "start_time": "...", // timestamp if playing, for sync
  "current_position_secs": 0.0,
  "repeat_mode": "Off" | "One" | "All"
}
