# Stuffchat Discord Bridge

This bot polls the Stuffchat bridge API and posts call join/leave notifications into one Discord text channel.

## Environment

On startup, the bot will automatically load variables from a `.env` file if one exists in the current working directory or the repository root. Existing shell environment variables take precedence over `.env`.

Required:

- `DISCORD_TOKEN`
- `DISCORD_CHANNEL_ID`
- `STUFFCHAT_BRIDGE_BASE_URL`
- `STUFFCHAT_BRIDGE_KEY`

Optional:

- `STUFFCHAT_BRIDGE_POLL_INTERVAL_SECONDS` default `2.0`
- `STUFFCHAT_BRIDGE_POLL_LIMIT` default `100`
- `STUFFCHAT_BRIDGE_STATE_FILE` default `bridge/.cursor.json`
- `STUFFCHAT_BRIDGE_HTTP_TIMEOUT_SECONDS` default `10`

## Install

```bash
python3 -m venv .venv
. .venv/bin/activate
pip install -r bridge/requirements.txt
```

## Run

```bash
python3 -m bridge.bot
```

The bot persists its polling cursor to `bridge/.cursor.json` by default so restarts do not replay old bridge events.
