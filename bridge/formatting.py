from typing import Any


def escape_markdown(value: str) -> str:
    escaped = []
    special = "\\`*_{}[]()#+-.!|>"
    for char in value:
        if char in special:
            escaped.append("\\")
        escaped.append(char)
    return "".join(escaped)


def format_event_message(event: dict[str, Any]) -> str:
    event_type = event.get("type")
    user = event.get("user") or {}
    channel = event.get("channel") or {}

    user_id = user.get("id", "unknown-user")
    username = user.get("username")
    safe_user = escape_markdown(username) if username else f"user {user_id}"

    channel_id = channel.get("id", "unknown-channel")
    channel_name = channel.get("name")
    safe_channel = (
        f"#{escape_markdown(channel_name)}" if channel_name else f"channel {channel_id}"
    )

    if event_type == "call_joined":
        return f"{safe_user} has joined call in {safe_channel}"
    if event_type == "call_left":
        return f"{safe_user} has left call in {safe_channel}"
    raise ValueError(f"unsupported bridge event type: {event_type}")
