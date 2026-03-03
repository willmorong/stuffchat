# stuffchat

stuffchat is a self-hosted Discord/Slack-esque chat application built with Rust and Actix.
Frontend is plain vanilla HTML/CSS/JS. Database is SQLite using sqlx to talk to it.

## Features

- File sharing
- Public and private channels
- Voice calls
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

## Configuration

See [config.toml](config.toml) for configuration options.

### Bridge

Stuffchat can expose a small read-only bridge API for external bots such as the Discord bridge in [`bridge/`](bridge/).

- Set `bridge_enabled = true` in [`config.toml`](config.toml).
- On startup, Stuffchat will create `./bridge_secret` if it does not already exist.
- Use the secret from that file as the bearer token for bridge clients.

The bridge API is intended for trusted machine clients and currently exposes:

- `GET /api/bridge/status`
- `GET /api/bridge/events`
- `POST /api/bridge/resolve`

The included Discord bridge bot polls those endpoints and posts notices like `alice has joined call in #main` into one configured Discord text channel.

## License

This project is licensed under the MIT license. (you can use it however but you have to credit me somewhere)
