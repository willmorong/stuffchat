import ast
import os
from dataclasses import dataclass
from pathlib import Path


@dataclass
class BridgeSettings:
    discord_token: str
    discord_channel_id: int
    bridge_key: str
    listen_host: str = "127.0.0.1"
    listen_port: int = 23901

    @classmethod
    def from_env(cls) -> "BridgeSettings":
        load_dotenv()
        listen_host, listen_port = parse_listen_address(
            os.getenv("STUFFCHAT_BRIDGE_LISTEN", "127.0.0.1:23901")
        )
        return cls(
            discord_token=require_env("DISCORD_TOKEN"),
            discord_channel_id=int(require_env("DISCORD_CHANNEL_ID")),
            bridge_key=require_env("STUFFCHAT_BRIDGE_KEY"),
            listen_host=listen_host,
            listen_port=listen_port,
        )


def parse_listen_address(value: str) -> tuple[str, int]:
    host, separator, port = value.rpartition(":")
    if not separator or not host:
        raise RuntimeError("STUFFCHAT_BRIDGE_LISTEN must be in host:port format")

    try:
        parsed_port = int(port)
    except ValueError as exc:
        raise RuntimeError("STUFFCHAT_BRIDGE_LISTEN port must be an integer") from exc

    if parsed_port <= 0 or parsed_port > 65535:
        raise RuntimeError("STUFFCHAT_BRIDGE_LISTEN port must be between 1 and 65535")

    return host, parsed_port


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
