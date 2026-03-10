# stuffchat

stuffchat is a self-hosted Discord/Slack-esque chat application built with Rust and Actix.
Frontend is plain vanilla HTML/CSS/JS. Database is SQLite using sqlx to talk to it.

## Features

- File sharing
- Public and private channels
- Voice calls
- Native iOS push notifications through an external relay
- Multi-person simulatenous screen sharing at higher quality than Discord Nitro
- Message search
- Custom emojis and reactions
- Message replies and edits
- Synced playlists in calls for listening parties with auto-download
- Invite codes and user controls

## Installation

Install Rust to compile from source, then install SQLite (at least 3.9.0 for FTS5) to run your backend database. 
(I'm also using Caddy as a reverse-proxy for easy HTTPS support, which might be needed for WebRTC and WebSocket.)

Clone the repo and change the address in [config.toml](config.toml) to your server's address. Then run `cargo run --release` to start the server. It's all one binary, so you can also run `cargo build --release` to build it and put it in a separate folder as "./stuffchat" (or whatever you want to name it).

### Admin bootstrap

On startup you can grant the `admin` role to a user (and create the role if it doesn't exist) with:

```
./stuffchat --admin <user-id|username|email>
```

### Permissions and roles

Roles now carry numeric capability bits in the `roles.permissions` field. The built-in bits include:

- `1` (`PERM_ADMIN_ALL`): full admin access.
- `2` (`PERM_MANAGE_CHANNELS`): manage channels across all channels.
- `4` (`PERM_POST_MESSAGES`): post new chat messages.
- `8` (`PERM_UPLOAD_FILES`): upload and use file attachments.
- `16` (`PERM_CREATE_CHANNELS`): create channels.
- `32` (`PERM_JOIN_VOICE`): join voice calls.
- `64` (`PERM_INVITE_USERS`): create invite codes.
- `128` (`PERM_MANAGE_EMOJIS`): upload/delete custom emojis.

On startup, the server seeds role rows and member role assignments so that:

- the `admin` role always gets `ADMIN_ALL`, and
- every user has at least the `member` role if they had no role assignment.

The `--admin` flag still works for existing deployments by ensuring the named user has the `admin` role, whether or not legacy role setup existed before.

## Configuration

See [config.toml](config.toml) for configuration options.

### Bridge

Stuffchat can push call join/leave events to an external bot such as the Discord bridge in [`bridge/`](bridge/).

- Set `bridge_enabled = true` in [`config.toml`](config.toml).
- Set `bridge_url` to the bot endpoint, for example `http://127.0.0.1:23901/events`.
- On startup, Stuffchat will create `./bridge_secret` if it does not already exist.
- Configure the bot with the same secret via `STUFFCHAT_BRIDGE_KEY`.

The included Discord bridge bot runs its own HTTP API and posts notices like `alice has joined call in #main` into one configured Discord text channel.

### Push Relay

Stuffchat can deliver native iOS push notifications through the standalone relay in [`relay/`](relay/).

- Build and run the relay separately on a VPS that has the APNS key material.
- Provision a self-hosted Stuffchat server with:
  - `cargo run --manifest-path relay/Cargo.toml -- server create --label "<your server name>"`
- Copy the emitted `server_id` and `server_secret` into the main server's `config.toml` as:
  - `push_relay_server_id`
  - `push_relay_server_secret`
- Set `push_relay_enabled = true` and `push_relay_url` to the relay base URL.
- The iOS app will then use `/api/push/capabilities` plus `/api/push/devices/current` to register APNS tokens against that self-hosted server.

## License

This project is licensed under the MIT license. (you can use it however but you have to credit me somewhere)
