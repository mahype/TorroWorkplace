#!/usr/bin/env python3
"""Build native per-app installers from checked dist artifacts; never publish or fetch anything.

Only Python's standard library is needed. Builders use pinned, externally provisioned tools;
all entry points fail closed and stage outputs until the entire requested matrix is complete.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parent
REPO = "https://github.com/mahype/TorroWorkplace"
LINUX = ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu")
MACOS = ("x86_64-apple-darwin", "aarch64-apple-darwin")
WINDOWS = "x86_64-pc-windows-msvc"
FORMATS = ("deb", "rpm", "archlinux")


def apps():
    value = json.loads((ROOT / "apps.json").read_text())
    result = value["apps"]
    if value["schema"] != 1 or len(result) != 7 or len({a["program"] for a in result}) != 7:
        raise ValueError("expected seven unique apps")
    return result


def version(tag):
    match = re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-rc\.([1-9][0-9]*))?", tag)
    if not match or any(int(n) > 65534 for n in match.groups() if n):
        raise ValueError("expected vMAJOR.MINOR.PATCH or vMAJOR.MINOR.PATCH-rc.N (components <= 65534)")
    return tag[1:]


def native_version(ver, fmt):
    # Tilde sorts before stable in dpkg/rpm. Arch vercmp knows the rc suffix; '-' is pkgrel there.
    return ver.replace("-rc.", "rc." if fmt == "archlinux" else "~rc.")


def regular(path):
    if not stat.S_ISREG(path.lstat().st_mode) or path.stat().st_size == 0:
        raise ValueError(f"missing, empty or non-regular file: {path}")
    return path


def digest(path):
    with regular(path).open("rb") as stream:
        hashed = hashlib.sha256()
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            hashed.update(block)
        return hashed.hexdigest()


def checked(path):
    words = regular(Path(str(path) + ".sha256")).read_text().split()
    if len(words) != 2 or words[1].lstrip("*") != path.name or words[0].lower() != digest(path):
        raise ValueError(f"invalid SHA-256: {path.name}")
    return path


def checksum(path):
    Path(str(path) + ".sha256").write_text(f"{digest(path)}  {path.name}\n")


def source_plan(folder, tag):
    plan = json.loads(regular(folder / "plan-dist-manifest.json").read_text())
    releases = {r["app_name"]: r["app_version"] for r in plan["releases"]}
    if plan["announcement_tag"] != tag or releases != {a["program"]: version(tag) for a in apps()}:
        raise ValueError("dist source plan does not match tag and seven-app matrix")
    return plan


def safe_name(name):
    path = PurePosixPath(name)
    if not name or "\\" in name or ":" in name or path.is_absolute() or ".." in path.parts:
        raise ValueError(f"unsafe archive path: {name!r}")
    return path


def unpack_program(folder, program, target, scratch):
    suffix = ".zip" if target == WINDOWS else ".tar.xz"
    archive = checked(folder / f"{program}-{target}{suffix}")
    wanted = f"{program}.exe" if target == WINDOWS else f"{program}-{target}/{program}"
    into = scratch / (program + (".exe" if target == WINDOWS else ""))
    if suffix == ".zip":
        with zipfile.ZipFile(archive) as zipped:
            seen = set()
            for member in zipped.infolist():
                safe_name(member.filename)
                mode = member.external_attr >> 16
                if stat.S_ISLNK(mode) or (stat.S_IFMT(mode) not in (0, stat.S_IFREG, stat.S_IFDIR)):
                    raise ValueError("links and special archive members are forbidden")
                if member.filename in seen:
                    raise ValueError("duplicate archive path")
                seen.add(member.filename)
            into.write_bytes(zipped.read(wanted))
    else:
        with tarfile.open(archive, "r:xz") as tar:
            seen = set()
            for member in tar.getmembers():
                safe_name(member.name)
                if not (member.isfile() or member.isdir()) or member.name in seen:
                    raise ValueError("links, special or duplicate archive members are forbidden")
                seen.add(member.name)
            member = tar.getmember(wanted)
            if not member.isfile():
                raise ValueError("program is not a regular archive member")
            with tar.extractfile(member) as stream:
                into.write_bytes(stream.read())
    regular(into).chmod(0o755)
    return into


def marker(program, manager, package, path):
    return {"schema": 1, "manager": manager, "package": package, "program": path}


def run(args):
    subprocess.run([str(arg) for arg in args], check=True)


def record(path, app, target, fmt, package=None, requires=()):
    checksum(path)
    return {"app": app, "target": target, "format": fmt, "file": path.name,
            "sha256": digest(path), "package": package, "requires": list(requires)}


def write_manifest(out, name, tag, entries):
    if not entries:
        raise ValueError("empty installer matrix")
    (out / name).write_text(json.dumps({"schema": 1, "tag": tag, "artifacts": entries}, indent=2) + "\n")


def linux_config(app, package, target, fmt, ver, binaries, scratch):
    core = package == "torroworkplace-bin"
    main = package == "torro-workplace"
    arch = "amd64" if target.startswith("x86_64") else "arm64"
    v = native_version(ver, fmt)
    required = ([] if core else ["torroworkplace-bin"]) + ([] if core or main else [p + "-bin" for p in app["requires"]])
    deps = [f"{p} (>= {v}-1)" if fmt == "deb" else f"{p}>={v}-1" if fmt == "archlinux" else f"{p} >= {v}-1" for p in required]
    # winit loads the X11 helpers at runtime; ELF dependency scanning alone cannot find them.
    runtime = {
        "deb": ["libc6 (>= 2.35)", "libgcc-s1", "libfontconfig1", "libxkbcommon0", "libxkbcommon-x11-0",
                "libwayland-client0", "libegl1", "libvulkan1", "libx11-6", "libxcursor1", "libxi6", "libx11-xcb1"],
        "rpm": ["glibc >= 2.35", "libgcc", "fontconfig", "libxkbcommon", "libxkbcommon-x11", "libwayland-client",
                "libglvnd-egl", "vulkan-loader", "libX11", "libXcursor", "libXi", "libX11-xcb"],
        "archlinux": ["glibc>=2.35", "gcc-libs", "fontconfig", "libxkbcommon", "libxkbcommon-x11", "wayland",
                      "libglvnd", "vulkan-icd-loader", "libx11", "libxcursor", "libxi"],
    }
    contents = []

    def content(src, dst, mode=0o644):
        contents.append({"src": str(src), "dst": dst, "file_info": {"mode": mode}})

    program = "torroworkplace" if core else app["program"]
    if not main:
        content(binaries[program], f"/usr/bin/{program}", 0o755)
        mark = scratch / (package + ".json")
        mark.write_text(json.dumps(marker(program, "pacman" if fmt == "archlinux" else fmt, package, f"/usr/bin/{program}")))
        content(mark, f"/usr/bin/{program}.torro-package.json")
    if core:
        content(ROOT / "linux/torroworkplace.service", "/usr/lib/systemd/user/torroworkplace.service")
    else:
        ident = app["id"]
        content(ROOT / f"linux/{ident}.desktop", f"/usr/share/applications/{ident}.desktop")
        content(ROOT / f"linux/icons/{ident}.svg", f"/usr/share/icons/hicolor/scalable/apps/{ident}.svg")
    for license_file in ("LICENSE-MIT", "LICENSE-APACHE"):
        content(ROOT.parent / license_file, f"/usr/share/licenses/{package}/{license_file}")
    return {"name": package, "arch": arch, "platform": "linux", "version": v,
            "version_schema": "none", "release": "1", "section": "utils", "priority": "optional",
            "maintainer": "TorroWorkplace contributors",
            "description": "TorroWorkplace background service" if core else app["title"],
            "homepage": REPO, "license": "MIT OR Apache-2.0", "depends": deps + runtime[fmt],
            "contents": contents}, required


def build_linux(folder, out, tag):
    source_plan(folder, tag)
    tool = os.environ.get("NFPM", "nfpm")
    reported = subprocess.check_output([tool, "--version"], text=True)
    if not re.search(r"\b2\.48\.0\b", reported):
        raise ValueError("nFPM 2.48.0 is required")
    entries = []
    with tempfile.TemporaryDirectory() as temp:
        scratch = Path(temp)
        for target in LINUX:
            binaries = {a["program"]: unpack_program(folder, a["program"], target, scratch) for a in apps()}
            packages = [(None, "torroworkplace-bin")] + [(a, "torro-workplace" if a["program"] == "torroworkplace" else a["program"] + "-bin") for a in apps()]
            for app, package in packages:
                for fmt in FORMATS:
                    config, required = linux_config(app, package, target, fmt, version(tag), binaries, scratch)
                    spec = scratch / "nfpm.json"
                    spec.write_text(json.dumps(config))  # JSON is valid YAML, no YAML dependency.
                    dest = scratch / "package"
                    dest.mkdir(exist_ok=True)
                    run([tool, "package", "--config", spec, "--packager", fmt, "--target", dest])
                    files = list(dest.iterdir())
                    if len(files) != 1:
                        raise ValueError("nFPM must produce exactly one package")
                    path = out / files[0].name
                    if path.exists():
                        raise ValueError("duplicate native artifact name")
                    shutil.move(files[0], path)
                    entries.append(record(path, "service" if app is None else app["id"], target, fmt, package, required))
    write_manifest(out, "installers-linux.json", tag, entries)


def windows_payload(app):
    return list(dict.fromkeys([app["program"], "torroworkplace", *app["requires"]]))


def build_windows(folder, out, tag):
    source_plan(folder, tag)
    tool = os.environ.get("ISCC", "ISCC.exe")
    # Inno 7's help identifies only the major version; query the engine explicitly.
    reported = subprocess.run([tool, "--version"], capture_output=True, text=True, check=False)
    if reported.returncode != 0 or reported.stdout.strip() != "7.1.1":
        raise ValueError("Inno Setup 7.1.1 is required")
    ver = version(tag)
    base, _, rc = ver.partition("-rc.")
    resource = f"{base}.{rc or '65535'}"
    entries = []
    with tempfile.TemporaryDirectory() as temp:
        scratch = Path(temp)
        binaries = {a["program"]: unpack_program(folder, a["program"], WINDOWS, scratch) for a in apps()}
        for app in apps():
            lines = []
            checks = []
            locks = []
            for program in windows_payload(app):
                # Marker uses an Inno {code:...} constant so Program Files is resolved by Windows,
                # not guessed from the build runner's locale or drive.
                lines.extend([
                    f'Source: "{binaries[program]}"; DestDir: "{{code:CopyDirectory}}"; Flags: ignoreversion sharedfile uninsnosharedfileprompt 64bit; AfterInstall: VerifyProgram(\'{program}\', \'{digest(binaries[program])}\')',
                    f'Source: "{scratch / (program + ".version")}"; DestDir: "{{code:CopyDirectory}}"; Flags: ignoreversion sharedfile uninsnosharedfileprompt 64bit; AfterInstall: VerifyVersion(\'{program}\')',
                    f'Source: "{scratch / (program + ".exe.torro-package.json")}"; DestDir: "{{code:CopyDirectory}}"; Flags: ignoreversion sharedfile uninsnosharedfileprompt 64bit; AfterInstall: WriteMarker(\'{program}\')',
                ])
                (scratch / (program + ".version")).write_text(ver + "\n")
                # Inno writes a fixed, administrator-only install marker from an expanded template.
                mark = marker(program, "setup", program, "@PROGRAM@")
                (scratch / (program + ".exe.torro-package.json")).write_text(json.dumps(mark))
                checks.append(f"  if not CheckProgram('{program}') then begin Result := False; exit; end;")
                for suffix in (".exe", ".version", ".exe.torro-package.json"):
                    locks.append(f"  CheckUnlocked('{program}{suffix}');")
            # ExternalSize avoids eager source-size probing. This mandatory last entry must expand
            # its source after every payload callback, outside Inno's swallowed Check/entry handlers.
            lines.append('Source: "{code:FinalInstallCheck}"; DestDir: "{tmp}"; Flags: external skipifsourcedoesntexist; ExternalSize: 0')
            (scratch / "files.iss").write_text("\n".join(lines) + "\n")
            (scratch / "checks.iss").write_text("\n".join(checks) + "\n")
            (scratch / "locks.iss").write_text("\n".join(locks) + "\n")
            name = f'{app["program"]}-{ver}-{WINDOWS}-setup'
            run([tool, f"/DAppProgram={app['program']}", f"/DAppIdentity={app['id']}", f"/DAppTitle={app['title']}",
                 f"/DAppArgs={' '.join(app['args'])}", f"/DAppVersion={ver}", f"/DResourceVersion={resource}",
                 f"/DPayload={scratch}", f"/O{out}", f"/F{name}", ROOT / "windows/setup.iss"])
            entries.append(record(out / (name + ".exe"), app["id"], WINDOWS, "setup", requires=app["requires"]))
    write_manifest(out, "installers-windows.json", tag, entries)


def expected_matrix():
    expected = {(a["id"], t, "dmg", None) for a in apps() for t in MACOS}
    expected |= {(a["id"], WINDOWS, "setup", None) for a in apps()}
    for target in LINUX:
        for fmt in FORMATS:
            expected.add(("service", target, fmt, "torroworkplace-bin"))
            expected |= {(a["id"], target, fmt, "torro-workplace" if a["program"] == "torroworkplace" else a["program"] + "-bin") for a in apps()}
    return expected


def verified_entries(folder, tag):
    entries = []
    for name in ("installers-linux.json", "installers-windows.json"):
        manifest = json.loads(regular(folder / name).read_text())
        if manifest["schema"] != 1 or manifest["tag"] != tag or not manifest["artifacts"]:
            raise ValueError("invalid or empty installer manifest")
        entries.extend(manifest["artifacts"])
    ver = version(tag)
    for app in apps():
        for target in MACOS:
            path = folder / f'{app["program"]}-{ver}-{target}.dmg'
            # Checksums originate after signing, notarisation and stapling, never before.
            checked(path)
            entries.append({"app": app["id"], "target": target, "format": "dmg", "file": path.name,
                            "sha256": digest(path), "package": None, "requires": app["requires"]})
    actual = set()
    names = set()
    for entry in entries:
        key = (entry["app"], entry["target"], entry["format"], entry["package"])
        if key in actual or entry["file"] in names or safe_name(entry["file"]).name != entry["file"]:
            raise ValueError("duplicate or unsafe installer entry")
        actual.add(key)
        names.add(entry["file"])
        if digest(checked(folder / entry["file"])) != entry["sha256"]:
            raise ValueError("installer manifest checksum mismatch")
        app = next((a for a in apps() if a["id"] == entry["app"]), None)
        if entry["format"] in FORMATS:
            expected_requires = [] if app is None else ["torroworkplace-bin", *[p + "-bin" for p in app["requires"]]]
            suffix = {"deb": ".deb", "rpm": ".rpm", "archlinux": ".pkg.tar.zst"}[entry["format"]]
            if not entry["file"].endswith(suffix):
                raise ValueError("wrong native package format")
        else:
            expected_requires = app["requires"]
        if entry["requires"] != expected_requires:
            raise ValueError("wrong companion dependencies")
        if entry["format"] == "setup":
            if entry["file"] != f'{app["program"]}-{ver}-{WINDOWS}-setup.exe':
                raise ValueError("wrong setup filename/version")
    if actual != expected_matrix():
        raise ValueError("incomplete or unexpected installer matrix")
    return entries


def downloads(folder, out, tag):
    entries = verified_entries(folder, tag)
    write_manifest(out, "installers-manifest.json", tag, entries)
    url = f"{REPO}/releases/download/{tag}/"
    lines = ["## App downloads", "", "Choose your app, platform and architecture. [Installation instructions](" + REPO + "/blob/" + tag + "/docs/install.md).", "",
             "Linux: download the app AND the listed companion packages in the SAME format and architecture.",
             "macOS: drag ALL displayed apps to Applications. Windows: all-users Setup (administrator required).", "",
             "| App | Platform / architecture | Download | Companion packages |", "|---|---|---|---|"]
    for entry in entries:
        name = entry["file"]
        required = entry["requires"]
        if entry["format"] in FORMATS:
            companions = []
            for package in required:
                companion = next(e for e in entries if e["package"] == package and e["target"] == entry["target"] and e["format"] == entry["format"])
                companions.append(f'[{package}]({url}{companion["file"]})')
            required = companions
        else:
            required = ["included" if required or entry["format"] == "setup" else "service included"]
        lines.append(f'| {entry["app"]} | {entry["format"]} / {entry["target"]} | [{name}]({url}{name}) · [SHA-256]({url}{name}.sha256) | {", ".join(required) or "—"} |')
    (out / "downloads.md").write_text("\n".join(lines) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["build-linux", "build-windows", "downloads", "verify"])
    parser.add_argument("--tag", required=True)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    version(args.tag)
    folder = args.input.resolve(strict=True)
    # No partial output on a failing tool, missing input or matrix mismatch.
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".installers-", dir=args.output) as temp:
        staged = Path(temp)
        if args.command == "verify":
            write_manifest(staged, "installers-manifest.json", args.tag, verified_entries(folder, args.tag))
        elif args.command == "downloads":
            downloads(folder, staged, args.tag)
        elif args.command == "build-linux":
            build_linux(folder, staged, args.tag)
        else:
            build_windows(folder, staged, args.tag)
        for file in staged.iterdir():
            os.replace(file, args.output / file.name)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError, subprocess.SubprocessError, tarfile.TarError, zipfile.BadZipFile) as error:
        print(f"installers: {error}", file=sys.stderr)
        sys.exit(1)
