import asyncio
from typing import Any

import aiohttp


class BridgeAuthError(RuntimeError):
    pass


class BridgeClient:
    def __init__(self, base_url: str, bridge_key: str, timeout_seconds: float = 10.0) -> None:
        self.base_url = base_url.rstrip("/")
        self.bridge_key = bridge_key
        self.timeout_seconds = timeout_seconds
        self._session: aiohttp.ClientSession | None = None

    async def __aenter__(self) -> "BridgeClient":
        self._session = aiohttp.ClientSession(
            timeout=aiohttp.ClientTimeout(total=self.timeout_seconds),
            headers={"Authorization": f"Bearer {self.bridge_key}"},
        )
        return self

    async def __aexit__(self, exc_type, exc, tb) -> None:
        if self._session is not None:
            await self._session.close()
            self._session = None

    async def status(self) -> dict[str, Any]:
        return await self._request_json("GET", "/api/bridge/status")

    async def events(self, after: int, limit: int) -> dict[str, Any]:
        return await self._request_json(
            "GET",
            f"/api/bridge/events?after={after}&limit={limit}",
        )

    async def resolve(self, user_ids: list[str], channel_ids: list[str]) -> dict[str, Any]:
        return await self._request_json(
            "POST",
            "/api/bridge/resolve",
            json={"user_ids": user_ids, "channel_ids": channel_ids},
        )

    async def request_with_backoff(self, request_fn, *args, **kwargs) -> dict[str, Any]:
        delays = [0.0, 1.0, 2.0, 5.0]
        last_error: Exception | None = None

        for delay in delays:
            if delay:
                await asyncio.sleep(delay)
            try:
                return await request_fn(*args, **kwargs)
            except BridgeAuthError:
                raise
            except (aiohttp.ClientError, asyncio.TimeoutError) as exc:
                last_error = exc

        if last_error is None:
            raise RuntimeError("request_with_backoff exhausted without an error")
        raise last_error

    async def _request_json(self, method: str, path: str, **kwargs) -> dict[str, Any]:
        if self._session is None:
            raise RuntimeError("BridgeClient must be used as an async context manager")

        async with self._session.request(method, f"{self.base_url}{path}", **kwargs) as response:
            if response.status == 401:
                raise BridgeAuthError("bridge authentication failed")
            response.raise_for_status()
            payload = await response.json()
            if not isinstance(payload, dict):
                raise RuntimeError("bridge API returned non-object JSON")
            return payload
