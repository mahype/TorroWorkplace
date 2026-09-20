from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Paths:
    config_dir: Path
    data_dir: Path
    state_dir: Path

    @classmethod
    def from_env(cls, env: dict[str, str] | None = None) -> "Paths":
        env = os.environ if env is None else env
        home = Path(env.get("HOME", "~")).expanduser()

        def root(override: str, xdg: str, fallback: Path) -> Path:
            return Path(env.get(override) or env.get(xdg) or fallback).expanduser()

        return cls(
            root("WORKSPACE_SETUPS_CONFIG_HOME", "XDG_CONFIG_HOME", home / ".config") / "workspace-setups",
            root("WORKSPACE_SETUPS_DATA_HOME", "XDG_DATA_HOME", home / ".local/share") / "workspace-setups",
            root("WORKSPACE_SETUPS_STATE_HOME", "XDG_STATE_HOME", home / ".local/state") / "workspace-setups",
        )

    @property
    def profiles(self) -> Path:
        return self.data_dir / "profiles"

    @property
    def backups(self) -> Path:
        return self.data_dir / "backups"

    @property
    def config(self) -> Path:
        return self.config_dir / "config.json"

    @property
    def status(self) -> Path:
        return self.state_dir / "restore.json"

    @property
    def lock(self) -> Path:
        return self.state_dir / "restore.lock"

    @property
    def cancel(self) -> Path:
        return self.state_dir / "cancel"
