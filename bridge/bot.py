import asyncio
import contextlib
import logging

import discord

try:
    from .client import BridgeAuthError, BridgeClient
    from .formatting import format_event_message
    from .settings import BridgeSettings
    from .state import CursorStore
except ImportError:
    from client import BridgeAuthError, BridgeClient
    from formatting import format_event_message
    from settings import BridgeSettings
    from state import CursorStore


logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
LOGGER = logging.getLogger("stuffchat-bridge")


class StuffchatBridgeBot(discord.Client):
    def __init__(self, settings: BridgeSettings) -> None:
        super().__init__(intents=discord.Intents.none())
        self.settings = settings
        self.cursor_store = CursorStore(settings.state_file)
        self.destination_channel: discord.abc.Messageable | None = None
        self._bridge_client: BridgeClient | None = None
        self._poll_task: asyncio.Task[None] | None = None

    async def setup_hook(self) -> None:
        self._bridge_client = BridgeClient(
            self.settings.base_url,
            self.settings.bridge_key,
            timeout_seconds=self.settings.http_timeout_seconds,
        )
        await self._bridge_client.__aenter__()

    async def close(self) -> None:
        if self._poll_task is not None:
            self._poll_task.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await self._poll_task
        if self._bridge_client is not None:
            await self._bridge_client.__aexit__(None, None, None)
            self._bridge_client = None
        await super().close()

    async def on_ready(self) -> None:
        if self.destination_channel is None:
            channel = await self.fetch_channel(self.settings.discord_channel_id)
            if not isinstance(channel, discord.abc.Messageable):
                raise RuntimeError("configured Discord channel is not messageable")
            self.destination_channel = channel
            LOGGER.info("Connected to Discord as %s", self.user)

        if self._poll_task is None:
            self._poll_task = asyncio.create_task(self.poll_loop())

    async def poll_loop(self) -> None:
        assert self._bridge_client is not None
        assert self.destination_channel is not None

        cursor = self.cursor_store.load()
        if cursor is None:
            status = await self._bridge_client.request_with_backoff(self._bridge_client.status)
            cursor = int(status.get("latest_available", 0))
            self.cursor_store.save(cursor)

        while not self.is_closed():
            try:
                payload = await self._bridge_client.request_with_backoff(
                    self._bridge_client.events,
                    cursor,
                    self.settings.poll_limit,
                )
                next_after = int(payload.get("next_after", cursor))
                if payload.get("reset_required"):
                    LOGGER.warning("Bridge cursor reset required; skipping to %s", next_after)
                    cursor = next_after
                    self.cursor_store.save(cursor)
                    await asyncio.sleep(self.settings.poll_interval_seconds)
                    continue

                for event in payload.get("events", []):
                    message = format_event_message(event)
                    await self.destination_channel.send(message)

                cursor = next_after
                self.cursor_store.save(cursor)
            except BridgeAuthError:
                LOGGER.error("Bridge authentication failed; shutting down")
                await self.close()
                return
            except asyncio.CancelledError:
                raise
            except Exception as exc:  # noqa: BLE001
                LOGGER.exception("Bridge polling failed: %s", exc)

            await asyncio.sleep(self.settings.poll_interval_seconds)


def main() -> None:
    settings = BridgeSettings.from_env()
    client = StuffchatBridgeBot(settings)
    client.run(settings.discord_token, log_handler=None)


if __name__ == "__main__":
    main()
