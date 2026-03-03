import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from bridge.formatting import format_event_message
from bridge.settings import BridgeSettings
from bridge.state import CursorStore

try:
    from bridge.client import BridgeAuthError, BridgeClient
except ModuleNotFoundError:
    BridgeAuthError = None
    BridgeClient = None


class CursorStoreTests(unittest.TestCase):
    def test_load_returns_none_for_missing_or_invalid_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            store = CursorStore(Path(tmp_dir) / "cursor.json")
            self.assertIsNone(store.load())
            store.path.write_text("{not-json")
            self.assertIsNone(store.load())

    def test_save_and_load_round_trip(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            store = CursorStore(Path(tmp_dir) / "cursor.json")
            store.save(42)
            self.assertEqual(store.load(), 42)


class FormatEventMessageTests(unittest.TestCase):
    def test_formats_join_event(self) -> None:
        message = format_event_message(
            {
                "type": "call_joined",
                "user": {"id": "user-1", "username": "alice"},
                "channel": {"id": "channel-1", "name": "main"},
            }
        )
        self.assertEqual(message, "alice has joined call in #main")

    def test_uses_fallbacks_for_missing_metadata(self) -> None:
        message = format_event_message(
            {
                "type": "call_left",
                "user": {"id": "user-1", "username": None},
                "channel": {"id": "channel-1", "name": None},
            }
        )
        self.assertEqual(message, "user user-1 has left call in channel channel-1")


class BridgeSettingsTests(unittest.TestCase):
    def test_from_env_loads_values_from_dotenv(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            env_path = Path(tmp_dir) / ".env"
            env_path.write_text(
                "\n".join(
                    [
                        "DISCORD_TOKEN=discord-token",
                        "DISCORD_CHANNEL_ID=1234",
                        "STUFFCHAT_BRIDGE_BASE_URL=https://example.com",
                        "STUFFCHAT_BRIDGE_KEY=bridge-secret",
                        'STUFFCHAT_BRIDGE_STATE_FILE="custom cursor.json"',
                    ]
                )
            )
            original_cwd = Path.cwd()
            try:
                os.chdir(tmp_dir)
                with mock.patch.dict(os.environ, {}, clear=True):
                    settings = BridgeSettings.from_env()
            finally:
                os.chdir(original_cwd)

        self.assertEqual(settings.discord_token, "discord-token")
        self.assertEqual(settings.discord_channel_id, 1234)
        self.assertEqual(settings.base_url, "https://example.com")
        self.assertEqual(settings.bridge_key, "bridge-secret")
        self.assertEqual(settings.state_file, "custom cursor.json")

    def test_from_env_keeps_existing_environment_values(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            env_path = Path(tmp_dir) / ".env"
            env_path.write_text(
                "\n".join(
                    [
                        "DISCORD_TOKEN=from-dotenv",
                        "DISCORD_CHANNEL_ID=1234",
                        "STUFFCHAT_BRIDGE_BASE_URL=https://example.com",
                        "STUFFCHAT_BRIDGE_KEY=bridge-secret",
                    ]
                )
            )
            original_cwd = Path.cwd()
            try:
                os.chdir(tmp_dir)
                with mock.patch.dict(
                    os.environ,
                    {"DISCORD_TOKEN": "from-env"},
                    clear=True,
                ):
                    settings = BridgeSettings.from_env()
            finally:
                os.chdir(original_cwd)

        self.assertEqual(settings.discord_token, "from-env")


@unittest.skipIf(BridgeClient is None, "aiohttp is not installed")
class BridgeClientTests(unittest.IsolatedAsyncioTestCase):
    async def test_request_with_backoff_propagates_bridge_auth(self) -> None:
        async with BridgeClient("https://example.com", "secret") as client:
            request = mock.AsyncMock(side_effect=BridgeAuthError("bad auth"))
            with self.assertRaises(BridgeAuthError):
                await client.request_with_backoff(request)


if __name__ == "__main__":
    unittest.main()
