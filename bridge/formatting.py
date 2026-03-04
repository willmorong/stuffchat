from typing import Any


def escape_markdown(value: str) -> str:
    escaped = []
    special = "\\`*_{}[]()#+-.!|>"
    for char in value:
        if char in special:
            escaped.append("\\")
        escaped.append(char)
    return "".join(escaped)


def format_channel_name(event: dict[str, Any]) -> str:
    channel = event.get("channel") or {}
    channel_id = channel.get("id", "unknown-channel")
    channel_name = channel.get("name")
    return f"#{escape_markdown(channel_name)}" if channel_name else f"channel {channel_id}"


def format_user_name(event: dict[str, Any]) -> str:
    user = event.get("user") or {}
    user_id = user.get("id", "unknown-user")
    username = user.get("username")
    return escape_markdown(username) if username else f"user {user_id}"


def format_event_message(event: dict[str, Any]) -> str:
    event_type = event.get("type")
    safe_user = format_user_name(event)
    safe_channel = format_channel_name(event)

    if event_type == "call_joined":
        return f"{safe_user} has joined call in {safe_channel}"
    if event_type == "call_left":
        return f"{safe_user} has left call in {safe_channel}"
    raise ValueError(f"unsupported bridge event type: {event_type}")


def format_call_ended_message(event: dict[str, Any]) -> str:
    return f"Call in {format_channel_name(event)} has ended"
