import json
from pathlib import Path


class CursorStore:
    def __init__(self, path: str | Path) -> None:
        self.path = Path(path)

    def load(self) -> int | None:
        try:
            payload = json.loads(self.path.read_text())
        except FileNotFoundError:
            return None
        except json.JSONDecodeError:
            return None

        last_seq = payload.get("last_seq")
        if isinstance(last_seq, int) and last_seq >= 0:
            return last_seq
        return None

    def save(self, last_seq: int) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        temp_path = self.path.with_suffix(f"{self.path.suffix}.tmp")
        temp_path.write_text(json.dumps({"last_seq": last_seq}))
        temp_path.replace(self.path)
