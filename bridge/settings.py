import ast
import os
from dataclasses import dataclass
from pathlib import Path


@dataclass
class BridgeSettings:
    discord_token: str
    discord_channel_id: int
    base_url: str
    bridge_key: str
    poll_interval_seconds: float = 2.0
    poll_limit: int = 100
    state_file: str = "bridge/.cursor.json"
    http_timeout_seconds: float = 10.0

    @classmethod
    def from_env(cls) -> "BridgeSettings":
        load_dotenv()
        return cls(
            discord_token=require_env("DISCORD_TOKEN"),
            discord_channel_id=int(require_env("DISCORD_CHANNEL_ID")),
            base_url=require_env("STUFFCHAT_BRIDGE_BASE_URL"),
            bridge_key=require_env("STUFFCHAT_BRIDGE_KEY"),
            poll_interval_seconds=float(os.getenv("STUFFCHAT_BRIDGE_POLL_INTERVAL_SECONDS", "2.0")),
            poll_limit=int(os.getenv("STUFFCHAT_BRIDGE_POLL_LIMIT", "100")),
            state_file=os.getenv("STUFFCHAT_BRIDGE_STATE_FILE", "bridge/.cursor.json"),
            http_timeout_seconds=float(os.getenv("STUFFCHAT_BRIDGE_HTTP_TIMEOUT_SECONDS", "10")),
        )


def require_env(name: str) -> str:
    value = os.getenv(name)
    if not value:
        raise RuntimeError(f"missing required environment variable {name}")
    return value


def load_dotenv() -> None:
    seen_paths: set[Path] = set()
    for path in dotenv_candidates():
        resolved = path.resolve()
        if resolved in seen_paths or not path.is_file():
            continue
        seen_paths.add(resolved)
        load_dotenv_file(path)


def dotenv_candidates() -> tuple[Path, ...]:
    repo_root = Path(__file__).resolve().parent.parent
    return (Path.cwd() / ".env", repo_root / ".env")


def load_dotenv_file(path: Path) -> None:
    for line in path.read_text().splitlines():
        key, value = parse_dotenv_line(line)
        if key is None or key in os.environ:
            continue
        os.environ[key] = value


def parse_dotenv_line(line: str) -> tuple[str | None, str]:
    stripped = line.strip()
    if not stripped or stripped.startswith("#"):
        return None, ""

    if stripped.startswith("export "):
        stripped = stripped[len("export ") :].lstrip()

    if "=" not in stripped:
        return None, ""

    key, raw_value = stripped.split("=", 1)
    key = key.strip()
    if not key:
        return None, ""

    value = raw_value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
        try:
            value = ast.literal_eval(value)
        except (SyntaxError, ValueError):
            value = value[1:-1]
    else:
        value = value.split(" #", 1)[0].rstrip()

    return key, value
