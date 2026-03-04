import contextlib
import json
import logging
import secrets
from collections import defaultdict
from collections.abc import Awaitable, Callable
from typing import Any

import discord
from aiohttp import ContentTypeError, web

try:
    from .formatting import format_call_ended_message, format_event_message
    from .settings import BridgeSettings
except ImportError:
    from formatting import format_call_ended_message, format_event_message
    from settings import BridgeSettings


logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
LOGGER = logging.getLogger("stuffchat-bridge")


def extract_bearer_token(header_value: str | None) -> str | None:
    if not header_value:
        return None
    prefix = "Bearer "
    if not header_value.startswith(prefix):
        return None
    token = header_value[len(prefix) :].strip()
    return token or None


def is_authorized(header_value: str | None, bridge_key: str) -> bool:
    token = extract_bearer_token(header_value)
    return token is not None and secrets.compare_digest(token, bridge_key)


async def healthcheck(_: web.Request) -> web.Response:
    return web.json_response({"ok": True})


async def receive_event(request: web.Request) -> web.Response:
    if not is_authorized(request.headers.get("Authorization"), request.app["bridge_key"]):
        raise web.HTTPUnauthorized(text="unauthorized")

    try:
        payload = await request.json()
    except (ContentTypeError, json.JSONDecodeError) as exc:
        raise web.HTTPBadRequest(text="invalid JSON body") from exc

    if not isinstance(payload, dict):
        raise web.HTTPBadRequest(text="bridge event payload must be an object")

    dispatch_event: Callable[[dict[str, Any]], Awaitable[None]] = request.app["dispatch_event"]
    try:
        await dispatch_event(payload)
    except ValueError as exc:
        raise web.HTTPBadRequest(text=str(exc)) from exc
    return web.json_response({"ok": True})


def create_bridge_app(
    bridge_key: str,
    dispatch_event: Callable[[dict[str, Any]], Awaitable[None]],
) -> web.Application:
    app = web.Application()
    app["bridge_key"] = bridge_key
    app["dispatch_event"] = dispatch_event
    app.router.add_get("/health", healthcheck)
    app.router.add_post("/events", receive_event)
    return app


class CallStateTracker:
    def __init__(self) -> None:
        self._participants_by_channel: dict[str, set[str]] = defaultdict(set)

    def messages_for_event(self, payload: dict[str, Any]) -> list[str]:
        messages = [format_event_message(payload)]
        event_type = payload.get("type")
        channel_id = ((payload.get("channel") or {}).get("id")) or "unknown-channel"
        user_id = ((payload.get("user") or {}).get("id")) or "unknown-user"

        if event_type == "call_joined":
            self._participants_by_channel[channel_id].add(user_id)
            return messages

        if event_type == "call_left":
            participants = self._participants_by_channel.get(channel_id)
            if participants is None or user_id not in participants:
                return messages

            participants.remove(user_id)
            if not participants:
                del self._participants_by_channel[channel_id]
                messages.append(format_call_ended_message(payload))
            return messages

        raise ValueError(f"unsupported bridge event type: {event_type}")


class StuffchatBridgeBot(discord.Client):
    def __init__(self, settings: BridgeSettings) -> None:
        super().__init__(intents=discord.Intents.none())
        self.settings = settings
        self.destination_channel: discord.abc.Messageable | None = None
        self._app_runner: web.AppRunner | None = None
        self._app_site: web.TCPSite | None = None
        self._logged_ready = False
        self._call_state = CallStateTracker()

    async def setup_hook(self) -> None:
        app = create_bridge_app(self.settings.bridge_key, self.dispatch_bridge_event)
        self._app_runner = web.AppRunner(app, access_log=None)
        await self._app_runner.setup()
        self._app_site = web.TCPSite(
            self._app_runner,
            host=self.settings.listen_host,
            port=self.settings.listen_port,
        )
        await self._app_site.start()
        LOGGER.info(
            "Bridge API listening on http://%s:%s/events",
            self.settings.listen_host,
            self.settings.listen_port,
        )

    async def close(self) -> None:
        if self._app_runner is not None:
            with contextlib.suppress(Exception):
                await self._app_runner.cleanup()
            self._app_runner = None
            self._app_site = None
        await super().close()

    async def on_ready(self) -> None:
        await self.get_destination_channel()
        if not self._logged_ready:
            LOGGER.info("Connected to Discord as %s", self.user)
            self._logged_ready = True

    async def get_destination_channel(self) -> discord.abc.Messageable:
        if self.destination_channel is None:
            channel = await self.fetch_channel(self.settings.discord_channel_id)
            if not isinstance(channel, discord.abc.Messageable):
                raise RuntimeError("configured Discord channel is not messageable")
            self.destination_channel = channel
        return self.destination_channel

    async def dispatch_bridge_event(self, payload: dict[str, Any]) -> None:
        messages = self._call_state.messages_for_event(payload)
        await self.wait_until_ready()
        destination_channel = await self.get_destination_channel()
        for message in messages:
            await destination_channel.send(message)


def main() -> None:
    settings = BridgeSettings.from_env()
    client = StuffchatBridgeBot(settings)
    client.run(settings.discord_token, log_handler=None)


if __name__ == "__main__":
    main()
