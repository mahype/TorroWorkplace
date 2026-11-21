#!/usr/bin/env python3
"""Build the assistant's Windows runtime archive (issue #773): sherpa-onnx-c-api.dll and onnxruntime.dll.

On Windows torroassistant links sherpa-onnx as a DLL. cargo-dist only packs binaries into an app's archive, so the
two DLLs ship as a release asset of their own, `torroassistant-runtime-<target>.zip` with its `.sha256`, built
by `[[dist.extra-artifacts]]` in dist-workspace.toml and again by build-installers.yml for the setups. Setup, the
service's app installer and Scoop put them beside torroassistant.exe.

Everything is pinned in packaging/windows/assistant-runtime.json: k2-fsa's archive and each DLL by SHA-256, the
sherpa-onnx version against Cargo.lock. Entries, times and modes of the zip are fixed, so a run with the same
Python gives the same bytes; its `.sha256` is written beside it. Only Python's standard library is needed.
"""

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import sys
import tarfile
import tempfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parent
PINNED = ROOT / "windows" / "assistant-runtime.json"
LOCK = ROOT.parent / "Cargo.lock"


def pinned():
    value = json.loads(PINNED.read_text())
    if value["schema"] != 1 or not value["files"]:
        raise ValueError("unexpected assistant-runtime.json")
    return value


def archive_name(value):
    return f'torroassistant-runtime-{value["target"]}.zip'


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def locked_version(lock):
    """The version of sherpa-onnx-sys in Cargo.lock: the libraries the program was linked against."""
    found = re.search(r'name = "sherpa-onnx-sys"\nversion = "([^"]+)"', lock.read_text())
    if not found:
        raise ValueError("sherpa-onnx-sys is not in Cargo.lock")
    return found.group(1)


def fetch(url, into):
    with urllib.request.urlopen(url, timeout=120) as response, into.open("wb") as out:
        for block in iter(lambda: response.read(1024 * 1024), b""):
            out.write(block)


def dlls(value, source):
    """The pinned DLLs out of k2-fsa's archive `source`, each checked against its SHA-256."""
    data = source.read_bytes()
    if sha256(data) != value["sha256"]:
        raise ValueError(f"invalid SHA-256: {source.name}")
    found = {}
    with tarfile.open(source, "r:bz2") as tar:
        for member in tar.getmembers():
            path = PurePosixPath(member.name)
            if path.name not in value["files"] or path.parent.name != "lib":
                continue
            if not member.isfile() or path.name in found:
                raise ValueError(f"unexpected archive member: {member.name}")
            with tar.extractfile(member) as stream:
                found[path.name] = stream.read()
    for name, expected in value["files"].items():
        if name not in found or sha256(found[name]) != expected:
            raise ValueError(f"missing or invalid DLL: {name}")
    return found


def write_zip(files, path):
    # Fixed order, time and mode: what goes in never depends on the runner's clock or file system.
    with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_DEFLATED) as zipped:
        for name in sorted(files):
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            zipped.writestr(info, files[name])


def build(output, archive=None, lock=LOCK):
    value = pinned()
    if locked_version(lock) != value["sherpa_onnx"]:
        raise ValueError("sherpa-onnx-sys in Cargo.lock does not match assistant-runtime.json")
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as temp:
        source = Path(archive) if archive else Path(temp) / value["archive"]
        if not archive:
            fetch(value["url"], source)
        files = dlls(value, source)
        name = archive_name(value)
        staged = Path(temp) / name
        write_zip(files, staged)
        path = output / name
        path.write_bytes(staged.read_bytes())
    Path(str(path) + ".sha256").write_text(f"{sha256(path.read_bytes())}  {path.name}\n")
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--archive", help="k2-fsa's archive already on disk instead of downloading it")
    args = parser.parse_args()
    try:
        print(build(args.output, args.archive))
    except (OSError, ValueError, KeyError, tarfile.TarError) as err:
        print(f"assistant runtime: {err}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
