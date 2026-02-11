# stuffchat

stuffchat is a self-hosted Discord/Slack-esque chat application built with Rust and Actix.
Frontend is plain vanilla HTML/CSS/JS. Database is SQLite using sqlx to talk to it.

## Features

- File sharing
- Public and private channels
- Voice calls
- Multi-person simulatenous screen sharing at higher quality than Discord Nitro
- Synced playlists in calls for listening parties with auto-download
- Invite codes and user controls

## Installation

Clone the repo and change the address in [config.toml](config.toml) to your server's address. Then run `cargo run --release` to start the server. It's all one binary, so you can also run `cargo build --release` to build it and put it in a separate folder.

### Admin bootstrap

On startup you can grant the `admin` role to a user (and create the role if it doesn't exist) with:

```
cargo run --release -- --admin <user-id|username|email>
```

## Configuration

See [config.toml](config.toml) for configuration options.

## License

This project is licensed under the MIT license. (you can use it however but you have to credit me somewhere)
