import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from aiohttp.test_utils import TestClient, TestServer

from bridge.bot import CallStateTracker, create_bridge_app, is_authorized
from bridge.formatting import format_call_ended_message, format_event_message
from bridge.settings import BridgeSettings, parse_listen_address


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

    def test_formats_call_ended_message(self) -> None:
        message = format_call_ended_message(
            {
                "channel": {"id": "channel-1", "name": "main"},
            }
        )
        self.assertEqual(message, "Call in #main has ended")


class CallStateTrackerTests(unittest.TestCase):
    def test_sends_call_ended_when_last_participant_leaves(self) -> None:
        tracker = CallStateTracker()

        join_messages = tracker.messages_for_event(
            {
                "type": "call_joined",
                "user": {"id": "user-1", "username": "alice"},
                "channel": {"id": "channel-1", "name": "main"},
            }
        )
        leave_messages = tracker.messages_for_event(
            {
                "type": "call_left",
                "user": {"id": "user-1", "username": "alice"},
                "channel": {"id": "channel-1", "name": "main"},
            }
        )

        self.assertEqual(join_messages, ["alice has joined call in #main"])
        self.assertEqual(
            leave_messages,
            [
                "alice has left call in #main",
                "Call in #main has ended",
            ],
        )

    def test_does_not_send_call_ended_when_other_participants_remain(self) -> None:
        tracker = CallStateTracker()

        tracker.messages_for_event(
            {
                "type": "call_joined",
                "user": {"id": "user-1", "username": "alice"},
                "channel": {"id": "channel-1", "name": "main"},
            }
        )
        tracker.messages_for_event(
            {
                "type": "call_joined",
                "user": {"id": "user-2", "username": "bob"},
                "channel": {"id": "channel-1", "name": "main"},
            }
        )

        leave_messages = tracker.messages_for_event(
            {
                "type": "call_left",
                "user": {"id": "user-1", "username": "alice"},
                "channel": {"id": "channel-1", "name": "main"},
            }
        )

        self.assertEqual(leave_messages, ["alice has left call in #main"])

    def test_does_not_send_call_ended_for_untracked_leave(self) -> None:
        tracker = CallStateTracker()

        leave_messages = tracker.messages_for_event(
            {
                "type": "call_left",
                "user": {"id": "user-1", "username": "alice"},
                "channel": {"id": "channel-1", "name": "main"},
            }
        )

        self.assertEqual(leave_messages, ["alice has left call in #main"])


class BridgeSettingsTests(unittest.TestCase):
    def test_from_env_loads_values_from_dotenv(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            env_path = Path(tmp_dir) / ".env"
            env_path.write_text(
                "\n".join(
                    [
                        "DISCORD_TOKEN=discord-token",
                        "DISCORD_CHANNEL_ID=1234",
                        "STUFFCHAT_BRIDGE_KEY=bridge-secret",
                        "STUFFCHAT_BRIDGE_LISTEN=0.0.0.0:24000",
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
        self.assertEqual(settings.bridge_key, "bridge-secret")
        self.assertEqual(settings.listen_host, "0.0.0.0")
        self.assertEqual(settings.listen_port, 24000)

    def test_from_env_keeps_existing_environment_values(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            env_path = Path(tmp_dir) / ".env"
            env_path.write_text(
                "\n".join(
                    [
                        "DISCORD_TOKEN=from-dotenv",
                        "DISCORD_CHANNEL_ID=1234",
                        "STUFFCHAT_BRIDGE_KEY=dotenv-secret",
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

    def test_parse_listen_address_rejects_invalid_values(self) -> None:
        with self.assertRaises(RuntimeError):
            parse_listen_address("127.0.0.1")
        with self.assertRaises(RuntimeError):
            parse_listen_address("127.0.0.1:not-a-port")


class BridgeAuthorizationTests(unittest.TestCase):
    def test_authorization_requires_matching_bearer_token(self) -> None:
        self.assertTrue(is_authorized("Bearer secret", "secret"))
        self.assertFalse(is_authorized("Bearer wrong", "secret"))
        self.assertFalse(is_authorized("Basic secret", "secret"))
        self.assertFalse(is_authorized(None, "secret"))


class BridgeApiTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.received_payloads: list[dict] = []

        async def dispatch(payload: dict) -> None:
            self.received_payloads.append(payload)

        self.client = TestClient(TestServer(create_bridge_app("secret", dispatch)))
        await self.client.start_server()

    async def asyncTearDown(self) -> None:
        await self.client.close()

    async def test_events_endpoint_requires_authorization(self) -> None:
        response = await self.client.post("/events", json={"type": "call_joined"})
        self.assertEqual(response.status, 401)

    async def test_events_endpoint_dispatches_payload(self) -> None:
        response = await self.client.post(
            "/events",
            json={"type": "call_joined", "user": {}, "channel": {}},
            headers={"Authorization": "Bearer secret"},
        )
        self.assertEqual(response.status, 200)
        self.assertEqual(self.received_payloads, [{"type": "call_joined", "user": {}, "channel": {}}])

    async def test_events_endpoint_rejects_invalid_json(self) -> None:
        response = await self.client.post(
            "/events",
            data="not-json",
            headers={"Authorization": "Bearer secret"},
        )
        self.assertEqual(response.status, 400)


if __name__ == "__main__":
    unittest.main()
