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
- `GET /api/users/me`: Get current user profile (includes roles).
- `PATCH /api/users/me`: Update profile. Body: `{ "username": "...", "email": "..." }`
- `PUT /api/users/me/password`: Change password. Body: `{ "current_password": "...", "new_password": "..." }`
- `PUT /api/users/me/avatar`: Upload avatar (multipart form data).
- `GET /api/users/{id}`: Get user by ID.
- `GET /api/users/{id}/avatar`: Get user avatar (redirects to file).

## Admin
- `GET /api/admin/users`: List all users with roles (admin only).
- `PATCH /api/admin/users/{id}`: Update user username/email. Body: `{ "username": "...", "email": "..." }`
- `PUT /api/admin/users/{id}/password`: Set user password. Body: `{ "new_password": "..." }`
- `PUT /api/admin/users/{id}/avatar`: Upload avatar for user (multipart form data).
- `PUT /api/admin/users/{id}/roles`: Replace user roles. Body: `{ "role_ids": ["..."] }`
- `GET /api/admin/roles`: List roles.
- `POST /api/admin/roles`: Create role. Body: `{ "name": "...", "permissions": 0 }`
- `DELETE /api/admin/roles/{id}`: Delete role.
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
### Reactions
- `PUT /api/messages/{id}/reactions/{emoji}`: Toggle a reaction (adds if not present, removes if present). Emoji is URL-encoded.
- `GET /api/messages/{id}/reactions`: List grouped reactions for a message.
### Emojis
- `GET /api/emojis`: List all custom emojis.
- `POST /api/emojis`: Upload a custom emoji. Content-Type: `multipart/form-data` (fields: `name`, `file`).
- `DELETE /api/emojis/{name}`: Delete a custom emoji by name.
- `GET /api/emojis/{name}/image`: Get emoji image (PNG).
### SharePlay (HTTP)
- `GET /api/shareplay/{channel_id}/current`: Get current song ID.
- `GET /api/shareplay/song/{song_id}`: Stream song audio.
## Response Structures
All timestamps are ISO 8601 strings (e.g. `"2026-02-12T23:36:16Z"`). All IDs are UUID v4 strings. Fields marked with `?` are nullable/optional (may be `null` or absent).
### Authentication
Returned by `POST /api/auth/register`, `/login`, `/refresh`:
```json
{
  "access_token": "string",
  "refresh_token": "string",
  "refresh_token_id": "string",
  "user_id": "string"
}
```
`POST /api/auth/logout` returns `200 OK` with an empty body.
### Users
**`GET /api/users`** — Array of public user objects:
```json
[
  {
    "id": "string",
    "username": "string",
    "avatar_file_id": "string?"
  }
]
```
**`GET /api/users/{id}`** — Single public user object (same shape as above).

**`GET /api/users/me`** — Full profile of the authenticated user (includes email and roles):
```json
{
  "id": "string",
  "username": "string",
  "email": "string?",
  "avatar_file_id": "string?",
  "created_at": "timestamp",
  "updated_at": "timestamp",
  "roles": [
    { "id": "string", "name": "string" }
  ]
}
```
**`PATCH /api/users/me`** — Returns the updated full profile (same shape as `GET /api/users/me`).

**`PUT /api/users/me/password`** — Returns `200 OK` with an empty body.

**`PUT /api/users/me/avatar`** — Returns:
```json
{ "avatar_file_id": "string" }
```
**`GET /api/users/{id}/avatar`** — Returns `302 Found` redirect to `/files/{file_id}/{filename}`.
### Channels
**`GET /api/channels`** — Array of channels the user is a member of:
```json
[
  {
    "id": "string",
    "name": "string",
    "is_voice": false,
    "is_private": false,
    "is_owner": true,
    "last_message_at": "timestamp?"
  }
]
```
**`POST /api/channels`** — Returns:
```json
{ "id": "string" }
```
**`GET /api/channels/unread`** — Array of unread state objects:
```json
[
  {
    "channel_id": "string",
    "last_read_message_id": "string?",
    "last_read_at": "timestamp?"
  }
]
```
**`GET /api/channels/{id}/members`** — Array of member objects:
```json
[
  {
    "user_id": "string",
    "can_read": true,
    "can_write": true,
    "can_manage": false
  }
]
```
**`GET /api/channels/{id}/ownership`** — Returns:
```json
{ "is_owner": true }
```
**`PATCH /api/channels/{id}`**, **`DELETE /api/channels/{id}`**, **`POST /api/channels/{id}/join`**, **`POST /api/channels/{id}/leave`**, **`POST /api/channels/{id}/read`**, **`POST /api/channels/{id}/notified`**, **`POST /api/channels/{id}/members`** — Return `200 OK` with an empty body.
### Messages
**`GET /api/channels/{id}/messages`** — Array of message objects (newest first):
```json
[
  {
    "id": "string",
    "channel_id": "string",
    "user_id": "string",
    "content": "string?",
    "file_url": "string?",
    "filename": "string?",
    "file_size": 12345,
    "created_at": "timestamp",
    "edited_at": "timestamp?",
    "reactions": [
      {
        "emoji": "👍",
        "users": ["user_id_1", "user_id_2"],
        "count": 2
      }
    ]
  }
]
```
> `file_url`, `filename`, and `file_size` are present only when the message has an attachment. `file_url` is a path in the form `/files/{file_id}/{original_name}`.

**`POST /api/channels/{id}/messages`** — Returns:
```json
{ "id": "string" }
```
**`PATCH /api/messages/{id}`**, **`DELETE /api/messages/{id}`** — Return `200 OK` with an empty body.
### Reactions
**`PUT /api/messages/{id}/reactions/{emoji}`** — Returns updated reactions for the message:
```json
{
  "reactions": [
    { "emoji": "👍", "users": ["user_id_1"], "count": 1 }
  ]
}
```
**`GET /api/messages/{id}/reactions`** — Returns the reactions array directly:
```json
[
  { "emoji": "👍", "users": ["user_id_1"], "count": 1 }
]
```
### Files
**`POST /api/files`** — Returns:
```json
{ "file_id": "string" }
```
**`GET /files/{id}/{filename}`** — Returns the file content with appropriate `Content-Type` and `Content-Disposition: inline` headers.
### Invites
**`POST /api/invites`** and **`GET /api/invites`** — Returns (array for list, single object for create):
```json
{
  "code": "string",
  "created_by": "string",
  "joined_user_id": "string?",
  "joined_username": "string?",
  "created_at": "timestamp"
}
```
### Presence
**`POST /api/presence/heartbeat`** — Returns `200 OK` with an empty body.

**`GET /api/presence/users`** — Array of presence objects:
```json
[
  {
    "user_id": "string",
    "status": "online",
    "last_heartbeat": "timestamp"
  }
]
```
> `status` is one of: `"online"`, `"away"`, `"dnd"`, `"invisible"`, `"offline"`. It is computed server-side — a user is reported as `"offline"` if their last heartbeat is older than 60 seconds, regardless of their requested status.
### Emojis
**`GET /api/emojis`** — Array of custom emoji objects:
```json
[
  {
    "name": "string",
    "created_at": "string",
    "created_by": "string"
  }
]
```
**`POST /api/emojis`** — Returns:
```json
{ "status": "ok", "name": "string" }
```
**`DELETE /api/emojis/{name}`** — Returns `204 No Content`.

**`GET /api/emojis/{name}/image`** — Returns the image bytes with `Content-Type: image/png`.
### Admin
**`GET /api/admin/users`** — Array of admin user objects with roles:
```json
[
  {
    "id": "string",
    "username": "string",
    "email": "string?",
    "avatar_file_id": "string?",
    "created_at": "timestamp",
    "updated_at": "timestamp",
    "roles": [
      { "id": "string", "name": "string" }
    ]
  }
]
```
**`GET /api/admin/roles`** — Array of role objects:
```json
[
  {
    "id": "string",
    "name": "string",
    "permissions": 0,
    "created_at": "timestamp"
  }
]
```
**`POST /api/admin/roles`** — Returns the created role (same shape as a single role above).

**`PATCH /api/admin/users/{id}`** — Returns the updated admin user object (same shape as `GET /api/admin/users` element).

**`PUT /api/admin/users/{id}/password`**, **`PUT /api/admin/users/{id}/avatar`**, **`PUT /api/admin/users/{id}/roles`**, **`DELETE /api/admin/roles/{id}`** — Return `200 OK` with an empty body.
### SharePlay (HTTP)
**`GET /api/shareplay/{channel_id}/current`** — Returns current song ID or `null`.

**`GET /api/shareplay/song/{song_id}`** — Streams audio content.
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
