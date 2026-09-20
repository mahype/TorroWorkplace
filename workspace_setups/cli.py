from __future__ import annotations

import argparse
import json
import os
import sys
from typing import Any

from .capture import capture
from .hyprland import Hyprland, HyprlandError
from .paths import Paths
from .restore import RestoreCoordinator, RestoreError
from .storage import StorageError, Store, atomic_json, now, read_json, slugify


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(prog="workspace-setups")
    commands = root.add_subparsers(dest="command", required=True)
    for name in ("list", "inspect", "status"):
        item = commands.add_parser(name)
        item.add_argument("--json", action="store_true", required=True)
    for name in ("save", "update", "delete"):
        item = commands.add_parser(name)
        item.add_argument("name")
    rename = commands.add_parser("rename")
    rename.add_argument("old")
    rename.add_argument("new")
    load = commands.add_parser("load")
    load.add_argument("name")
    load.add_argument("--mode", choices=("empty", "add", "replace"), default="empty")
    load.add_argument("--confirm-replace", action="store_true")
    startup = commands.add_parser("set-startup")
    startup.add_argument("mode", choices=("none", "fixed", "last", "ask"))
    startup.add_argument("name", nargs="?")
    commands.add_parser("cancel")
    return root


def output(value: Any) -> None:
    print(json.dumps(value, ensure_ascii=False, sort_keys=True))


def main(argv: list[str] | None = None, store: Store | None = None, hyprland: Hyprland | None = None) -> int:
    args = parser().parse_args(argv)
    store = store or Store()
    hyprland = hyprland or Hyprland()
    try:
        if args.command == "list":
            output(store.list())
        elif args.command == "inspect":
            output(capture("Current workspace", hyprland))
        elif args.command in {"save", "update"}:
            existing = store.get(args.name) if args.command == "update" else None
            profile = capture(existing["name"] if existing else args.name, hyprland, existing)
            store.save(profile, replace=bool(existing))
            output(profile)
        elif args.command == "delete":
            store.delete(args.name)
            output({"deleted": slugify(args.name)})
        elif args.command == "rename":
            output(store.rename(args.old, args.new))
        elif args.command == "load":
            output(RestoreCoordinator(store, hyprland).restore(store.get(args.name), args.mode, args.confirm_replace))
        elif args.command == "set-startup":
            if args.mode == "fixed" and not args.name:
                raise StorageError("fixed startup mode requires a profile name")
            if args.mode != "fixed" and args.name:
                raise StorageError("profile name is only valid with fixed startup mode")
            profile_id = store.get(args.name)["id"] if args.name else None
            config = store.config()
            config.update(startupMode=args.mode, startupProfile=profile_id)
            store.write_config(config)
            output(config)
        elif args.command == "status":
            status = read_json(store.paths.status, {"schemaVersion": 1, "state": "idle"})
            if isinstance(status, dict) and status.get("state") == "running":
                try:
                    os.kill(int(status.get("pid", 0)), 0)
                except (OSError, TypeError, ValueError):
                    status = {**status, "state": "failed", "phase": "failed", "message": "Restore process ended unexpectedly"}
            output(status)
        elif args.command == "cancel":
            status = read_json(store.paths.status, {})
            run_id = status.get("runId") if isinstance(status, dict) and status.get("state") == "running" else None
            if run_id:
                store.paths.state_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
                atomic_json(store.paths.cancel, {"requestedAt": now(), "runId": run_id})
            output({"cancelled": bool(run_id), "runId": run_id})
        return 0
    except (StorageError, RestoreError, HyprlandError) as error:
        print(json.dumps({"error": str(error)}, ensure_ascii=False), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
