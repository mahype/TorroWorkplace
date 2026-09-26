from __future__ import annotations

import argparse
import json
import os
import sys
from typing import Any
from urllib.parse import urlparse

from . import SLOTS, STRATEGIES, bindings
from .browser import browser_family
from .capture import capture
from .hyprland import Hyprland, HyprlandError
from .restore import MODES, RestoreCoordinator
from .storage import LANGUAGES, StorageError, Store, atomic_json, now, read_json, slugify, summary

# Exit code for errors the UI resolves by asking the user (see ``StorageError.code``).
NEEDS_CHOICE = 3


def slot(value: str) -> int:
    number = int(value.upper().removeprefix("F")) if value.upper().removeprefix("F").isdigit() else 0
    if number not in SLOTS:
        raise argparse.ArgumentTypeError("slot must be F1..F10")
    return number


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(prog="workspace-setups")
    commands = root.add_subparsers(dest="command", required=True)
    for name in ("list", "inspect", "status"):
        item = commands.add_parser(name)
        item.add_argument("--json", action="store_true", required=True)
    save = commands.add_parser("save", help="save the active workspace as a new template")
    save.add_argument("name")
    for name in ("update", "delete"):
        commands.add_parser(name).add_argument("name")
    rename = commands.add_parser("rename")
    rename.add_argument("old")
    rename.add_argument("new")
    save_slot = commands.add_parser("save-slot", help="save the active workspace on a shortcut slot")
    save_slot.add_argument("slot", type=slot)
    save_slot.add_argument("--force", action="store_true", help="overwrite the template on that slot")
    for name in ("load", "load-slot"):
        load = commands.add_parser(name)
        load.add_argument("slot" if name == "load-slot" else "name", type=slot if name == "load-slot" else str)
        load.add_argument("--mode", choices=MODES, default="ask")
        load.add_argument("--workspace", help="target workspace (default: the active one)")
    startup = commands.add_parser("set-startup")
    startup.add_argument("name")
    startup.add_argument("state", choices=("on", "off"))
    startup.add_argument("--workspace")
    shortcut = commands.add_parser("set-shortcut")
    shortcut.add_argument("name")
    shortcut.add_argument("slot", type=lambda value: None if value == "none" else slot(value))
    shortcut.add_argument("--force", action="store_true", help="take the slot from another template")
    strategy = commands.add_parser("set-strategy")
    strategy.add_argument("name")
    strategy.add_argument("window")
    strategy.add_argument("strategy", choices=STRATEGIES)
    urls = commands.add_parser("set-urls", help="set the tabs a browser window opens with")
    urls.add_argument("name")
    urls.add_argument("window")
    urls.add_argument("urls", nargs="*")
    commands.add_parser("set-language").add_argument("language", choices=LANGUAGES)
    commands.add_parser("startup").add_argument("--force", action="store_true")
    commands.add_parser("bindings-status")
    commands.add_parser("install-bindings")
    commands.add_parser("cancel")
    return root


def output(value: Any) -> None:
    print(json.dumps(value, ensure_ascii=False, sort_keys=True))


def run_startup(store: Store, hyprland: Hyprland, force: bool) -> dict[str, Any]:
    """Restore every startup template once per Hyprland session."""
    instance = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE", "")
    marker = read_json(store.paths.startup_marker, {})
    if not force and isinstance(marker, dict) and instance and marker.get("instance") == instance:
        return {"skipped": True, "results": []}
    atomic_json(store.paths.startup_marker, {"instance": instance, "startedAt": now()})
    templates = sorted((item for item in store.all() if item.get("restoreOnStartup")),
                       key=lambda item: int(item["startupWorkspace"]))
    active = hyprland.query("activeworkspace").get("name") if templates else None
    results = []
    coordinator = RestoreCoordinator(store, hyprland)
    for template in templates:
        try:
            state = coordinator.restore(template, "add", template["startupWorkspace"], focus=False)["state"]
            results.append({"template": template["name"], "state": state})
        except (StorageError, HyprlandError) as error:
            results.append({"template": template["name"], "state": "failed", "error": str(error)})
    if active:
        try:
            hyprland.focus_workspace(str(active))
        except HyprlandError:
            pass
    return {"skipped": False, "results": results}


def main(argv: list[str] | None = None, store: Store | None = None, hyprland: Hyprland | None = None) -> int:
    args = parser().parse_args(argv)
    store = store or Store()
    hyprland = hyprland or Hyprland()
    try:
        if args.command == "list":
            output({"templates": store.list(), "language": store.config().get("language", "auto"),
                    "bindingsInstalled": bindings.installed()})
        elif args.command == "inspect":
            output(capture("Current workspace", hyprland))
        elif args.command == "save":
            if store.exists(args.name):
                raise StorageError(f"template already exists: {args.name}", "exists", template=args.name)
            template = capture(args.name.strip(), hyprland)
            store.save(template)
            output(template)
        elif args.command == "update":
            existing = store.get(args.name)
            template = capture(existing["name"], hyprland, existing)
            store.save(template, replace=True)
            output(template)
        elif args.command == "save-slot":
            existing = store.by_slot(args.slot)
            if existing and not args.force:
                raise StorageError(f"F{args.slot} already holds {existing['name']}", "slot_taken",
                                   slot=args.slot, template=existing["name"])
            if existing:
                template = capture(existing["name"], hyprland, existing)
                store.save(template, replace=True)
            else:
                template = capture(store.unique_name(f"Template F{args.slot}"), hyprland)
                template["shortcut"] = args.slot
                store.save(template)
            output(template)
        elif args.command == "delete":
            store.delete(args.name)
            output({"deleted": slugify(args.name)})
        elif args.command == "rename":
            output(summary(store.rename(args.old, args.new)))
        elif args.command in {"load", "load-slot"}:
            if args.command == "load-slot":
                template = store.by_slot(args.slot)
                if template is None:
                    raise StorageError(f"no template is assigned to F{args.slot}", "slot_empty", slot=args.slot)
            else:
                template = store.get(args.name)
            output(RestoreCoordinator(store, hyprland).restore(template, args.mode, args.workspace))
        elif args.command == "set-startup":
            template = store.get(args.name)
            changes: dict[str, Any] = {"restoreOnStartup": args.state == "on"}
            if args.workspace:
                changes["startupWorkspace"] = args.workspace
            elif args.state == "on" and not template.get("startupWorkspace"):
                raise StorageError("choose a startup workspace with --workspace")
            output(summary(store.update_settings(template["id"], **changes)))
        elif args.command == "set-shortcut":
            output(summary(store.assign_shortcut(args.name, args.slot, args.force)))
        elif args.command == "set-strategy":
            template = store.get(args.name)
            window = next((item for item in template["windows"] if item["id"] == args.window), None)
            if window is None:
                raise StorageError(f"unknown window: {args.window}")
            window["strategy"] = args.strategy
            output(summary(store.update_settings(template["id"], windows=template["windows"])))
        elif args.command == "set-urls":
            template = store.get(args.name)
            window = next((item for item in template["windows"] if item["id"] == args.window), None)
            if window is None:
                raise StorageError(f"unknown window: {args.window}")
            if not browser_family(window):
                raise StorageError(f"not a browser window: {args.window}")
            window["urls"] = [url if urlparse(url).scheme else f"https://{url}" for url in (item.strip() for item in args.urls) if url]
            output(summary(store.update_settings(template["id"], windows=template["windows"])))
        elif args.command == "set-language":
            config = store.config()
            config["language"] = args.language
            store.write_config(config)
            output(config)
        elif args.command == "startup":
            output(run_startup(store, hyprland, args.force))
        elif args.command == "bindings-status":
            output({"installed": bindings.installed(), "file": str(bindings.user_bindings())})
        elif args.command == "install-bindings":
            added = bindings.install()
            if added:
                try:
                    hyprland.reload()
                except HyprlandError:  # the include still applies on the next Hyprland start
                    pass
            output({"installed": True, "added": added, "file": str(bindings.user_bindings())})
        elif args.command == "status":
            status = read_json(store.paths.status, {"schemaVersion": 2, "state": "idle"})
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
    except (StorageError, HyprlandError) as error:
        payload: dict[str, Any] = {"error": str(error)}
        code = getattr(error, "code", None)
        if code:
            payload.update(code=code, **getattr(error, "details", {}))
        print(json.dumps(payload, ensure_ascii=False), file=sys.stderr)
        return NEEDS_CHOICE if code in {"workspace_not_empty", "slot_taken", "exists"} else 2


if __name__ == "__main__":
    raise SystemExit(main())
