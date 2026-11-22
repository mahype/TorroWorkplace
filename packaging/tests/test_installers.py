"""Offline installer contracts, failure cases and the complete download matrix."""

import io
import json
from pathlib import Path
import re
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import installers as pkg


def source_plan(folder, tag="v0.1.0"):
    (folder / "plan-dist-manifest.json").write_text(json.dumps({
        "announcement_tag": tag,
        "releases": [{"app_name": a["program"], "app_version": pkg.version(tag)} for a in pkg.apps()],
    }))


def archive(folder, program, target, extra=None):
    body = b"#!/bin/sh\necho fixture 0.1.0\n"
    name = f"{program}-{target}"
    if target == pkg.WINDOWS:
        path = folder / (name + ".zip")
        with zipfile.ZipFile(path, "w") as zipped:
            zipped.writestr(program + ".exe", body)
            if extra:
                zipped.writestr(extra, b"bad")
    else:
        path = folder / (name + ".tar.xz")
        with tarfile.open(path, "w:xz") as tar:
            member = tarfile.TarInfo(f"{name}/{program}")
            member.size = len(body)
            member.mode = 0o755
            tar.addfile(member, io.BytesIO(body))
            if extra:
                member = tarfile.TarInfo(extra)
                member.type = tarfile.SYMTYPE
                member.linkname = "/etc/passwd"
                tar.addfile(member)
    pkg.checksum(path)
    return path


RUNTIME_FILES = {"sherpa-onnx-c-api.dll": b"MZ sherpa", "onnxruntime.dll": b"MZ onnxruntime"}


def runtime(folder, files=RUNTIME_FILES):
    """The assistant's runtime archive (issue #773) as packaging/assistant_runtime.py writes it."""
    path = folder / f"torroassistant-runtime-{pkg.WINDOWS}.zip"
    with zipfile.ZipFile(path, "w") as zipped:
        for name, body in files.items():
            zipped.writestr(name, body)
    pkg.checksum(path)
    return path


def runtime_pins():
    return {name: pkg.hashlib.sha256(body).hexdigest() for name, body in RUNTIME_FILES.items()}


def complete(folder, tag="v0.1.0"):
    groups = {"linux": [], "windows": []}
    ver = pkg.version(tag)
    for app, target, fmt, package in sorted(pkg.expected_matrix(), key=str):
        ident = next((a for a in pkg.apps() if a["id"] == app), None)
        if fmt == "dmg":
            name = f'{ident["program"]}-{ver}-{target}.dmg'
            requires = ident["requires"]
        elif fmt == "setup":
            name = f'{ident["program"]}-{ver}-{target}-setup.exe'
            requires = ident["requires"]
        else:
            suffix = {"deb": "deb", "rpm": "rpm", "archlinux": "pkg.tar.zst"}[fmt]
            name = f"{package}-{pkg.native_version(ver, fmt)}-1-{target}.{suffix}"
            requires = [] if ident is None else ["torroworkplace-bin", *[p + "-bin" for p in ident["requires"]]]
        path = folder / name
        path.write_bytes(b"fixture installer")
        entry = pkg.record(path, app, target, fmt, package, requires)
        if fmt != "dmg":
            groups["windows" if fmt == "setup" else "linux"].append(entry)
    for group, entries in groups.items():
        pkg.write_manifest(folder, f"installers-{group}.json", tag, entries)


class Installers(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def test_nine_identities_and_program_launch_arguments_match_rust_and_desktop(self):
        self.assertEqual(len(pkg.apps()), 9)
        self.assertIn("torrochat", {a["program"] for a in pkg.apps()})
        self.assertIn("torrocontacts", {a["program"] for a in pkg.apps()})
        rust = (pkg.ROOT.parent / "crates/torro-core/src/app.rs").read_text()
        for app in pkg.apps():
            self.assertIn('"' + app["id"] + '"', rust)
            self.assertIn('"' + app["title"] + '"', rust)
            desktop = (pkg.ROOT / f'linux/{app["id"]}.desktop').read_text()
            self.assertIn("Exec=" + " ".join([app["program"], *app["args"]]) + "\n", desktop)
            self.assertIn("torroworkplace", pkg.windows_payload(app))
            for dependency in app["requires"]:
                self.assertIn(dependency, pkg.windows_payload(app))

    def test_streaming_digest_supports_the_runners_python_without_file_digest(self):
        path = self.root / "payload"
        path.write_bytes(b"payload" * 200_000)
        expected = pkg.hashlib.sha256(path.read_bytes()).hexdigest()
        with patch.object(pkg.hashlib, "file_digest", None, create=True):
            self.assertEqual(pkg.digest(path), expected)

    def test_native_icon_resources_cover_all_apps(self):
        for app in pkg.apps():
            with self.subTest(app=app["id"]):
                png = (pkg.ROOT / "macos/icons" / (app["id"] + ".png")).read_bytes()
                self.assertEqual(png[:8], b"\x89PNG\r\n\x1a\n")
                self.assertEqual(struct.unpack(">II", png[16:24]), (1024, 1024))
                ico = (pkg.ROOT / "windows" / (app["id"] + ".ico")).read_bytes()
                self.assertEqual(struct.unpack("<HHH", ico[:6]), (0, 1, 7))
                for index, size in enumerate((16, 24, 32, 48, 64, 128, 256)):
                    width, height, _, _, _, _, length, offset = struct.unpack_from("<BBBBHHII", ico, 6 + index * 16)
                    self.assertEqual((width or 256, height or 256), (size, size))
                    frame = ico[offset:offset + length]
                    self.assertEqual(frame[:8], b"\x89PNG\r\n\x1a\n")
                    self.assertEqual(struct.unpack(">II", frame[16:24]), (size, size))

    def test_native_packages_have_unique_ownership_dependencies_and_markers(self):
        owners = set()
        binaries = {a["program"]: self.root / a["program"] for a in pkg.apps()}
        for app in [None, *pkg.apps()]:
            package = "torroworkplace-bin" if app is None else "torro-workplace" if app["program"] == "torroworkplace" else app["program"] + "-bin"
            config, required = pkg.linux_config(app, package, pkg.LINUX[0], "deb", "0.1.0-rc.1", binaries, self.root)
            self.assertNotIn("scripts", config)
            for content in config["contents"]:
                dst = content["dst"]
                self.assertNotIn(dst, owners)
                owners.add(dst)
                if dst.endswith(".torro-package.json"):
                    mark = json.loads(Path(content["src"]).read_text())
                    self.assertEqual(mark["program"] + ".torro-package.json", dst)
                    self.assertEqual(mark["manager"], "deb")
                    self.assertEqual(mark["package"], package)
            if app is not None:
                self.assertIn("torroworkplace-bin (>= 0.1.0~rc.1-1)", config["depends"])
            if app and app["requires"]:
                self.assertIn("torroinbox-bin", required)
        self.assertEqual(len([p for p in owners if p.startswith("/usr/bin/") and not p.endswith(".json")]), 9)

    def test_stable_and_rc_native_versions(self):
        for fmt in pkg.FORMATS:
            self.assertEqual(pkg.native_version("1.2.3", fmt), "1.2.3")
        self.assertEqual(pkg.native_version("1.2.3-rc.10", "deb"), "1.2.3~rc.10")
        self.assertEqual(pkg.native_version("1.2.3-rc.10", "rpm"), "1.2.3~rc.10")
        self.assertEqual(pkg.native_version("1.2.3-rc.10", "archlinux"), "1.2.3rc.10")
        for tag in ["", "v1.2", "../v1.2.3", "v1.2.3;cmd", "v1.2.3-rc.0", "v01.2.3", "v65535.0.0"]:
            with self.assertRaises(ValueError):
                pkg.version(tag)

    def test_linux_packages_declare_linked_and_dynamically_loaded_gui_libraries(self):
        binaries = {a["program"]: self.root / a["program"] for a in pkg.apps()}
        expected = {
            "deb": ["libfontconfig1", "libxcursor1", "libxi6", "libx11-xcb1", "libxkbcommon-x11-0"],
            "rpm": ["fontconfig", "libXcursor", "libXi", "libX11-xcb", "libxkbcommon-x11"],
            "archlinux": ["fontconfig", "libxcursor", "libxi", "libxkbcommon-x11"],
        }
        for target in pkg.LINUX:
            for fmt, dependencies in expected.items():
                for app in [None, *pkg.apps()]:
                    package = "torroworkplace-bin" if app is None else "torro-workplace" if app["program"] == "torroworkplace" else app["program"] + "-bin"
                    with self.subTest(target=target, format=fmt, package=package):
                        config, _ = pkg.linux_config(app, package, target, fmt, "0.1.0", binaries, self.root)
                        for dependency in dependencies:
                            self.assertIn(dependency, config["depends"])

    def test_checked_sources_plan_and_archive_paths(self):
        source_plan(self.root)
        pkg.source_plan(self.root, "v0.1.0")
        with self.assertRaises(ValueError):
            pkg.source_plan(self.root, "v0.2.0")
        plan_path = self.root / "plan-dist-manifest.json"
        original = json.loads(plan_path.read_text())
        for releases in [original["releases"][:-1], original["releases"] + [original["releases"][0]],
                         original["releases"] + [{"app_name": "foreign", "app_version": "0.1.0"}]]:
            plan_path.write_text(json.dumps({**original, "releases": releases}))
            with self.assertRaises(ValueError):
                pkg.source_plan(self.root, "v0.1.0")
        plan_path.write_text(json.dumps(original))
        scratch = self.root / "scratch"
        scratch.mkdir()
        for target in [*pkg.LINUX, pkg.WINDOWS]:
            path = archive(self.root, "torrotasks", target)
            unpacked = pkg.unpack_program(self.root, "torrotasks", target, scratch)
            self.assertIn(b"fixture", unpacked.read_bytes())
            path.write_bytes(b"changed")
            with self.assertRaises(ValueError):
                pkg.unpack_program(self.root, "torrotasks", target, scratch)
            archive(self.root, "torrotasks", target, "../escape")
            with self.assertRaises(ValueError):
                pkg.unpack_program(self.root, "torrotasks", target, scratch)
        for unsafe in ["../x", "/x", "a/../../x", "C:/x", "a\\..\\x"]:
            with self.assertRaises(ValueError):
                pkg.safe_name(unsafe)

    def test_zip_symlinks_are_rejected(self):
        path = self.root / f"torrotasks-{pkg.WINDOWS}.zip"
        with zipfile.ZipFile(path, "w") as zipped:
            link = zipfile.ZipInfo("torrotasks.exe")
            link.create_system = 3
            link.external_attr = (stat.S_IFLNK | 0o777) << 16
            zipped.writestr(link, "/escape")
        pkg.checksum(path)
        with self.assertRaises(ValueError):
            pkg.unpack_program(self.root, "torrotasks", pkg.WINDOWS, self.root)

    def test_matrix_downloads_and_companions(self):
        complete(self.root)
        self.assertEqual(len(pkg.verified_entries(self.root, "v0.1.0")), 87)
        pkg.downloads(self.root, self.root, "v0.1.0")
        text = (self.root / "downloads.md").read_text()
        self.assertIn("torroinbox-bin", text)
        self.assertIn("torroworkplace-bin", text)
        self.assertIn(".dmg)", text)
        self.assertIn("-setup.exe)", text)
        self.assertIn(".pkg.tar.zst)", text)
        self.assertIn("torrochat-bin", text)
        self.assertIn("torrochat-0.1.0-x86_64-pc-windows-msvc-setup.exe", text)
        self.assertIn("torrocontacts-bin", text)
        self.assertIn("torrocontacts-0.1.0-x86_64-pc-windows-msvc-setup.exe", text)

    def test_macos_bundle_jobs_cover_every_native_app_and_architecture(self):
        workflow = (pkg.ROOT.parent / ".github/workflows/publish-macos-app.yml").read_text()
        entries = re.findall(r"- \{ bin: ([^,]+), service: \"1\", appid: ([^,]+), title: ([^,]+), arch: ([^,]+),", workflow)
        actual = {(program, appid, title, arch) for program, appid, title, arch in entries}
        expected = {(a["program"], a["id"], a["title"], arch) for a in pkg.apps() for arch in ("aarch64", "x86_64")}
        self.assertEqual(actual, expected)
        self.assertEqual(len(entries), len(expected))
        for app in pkg.apps():
            self.assertTrue((pkg.ROOT / "macos/icons" / (app["id"] + ".png")).is_file(), app["id"])

    def test_incomplete_empty_corrupt_duplicate_or_wrong_tag_matrix_fails(self):
        complete(self.root)
        path = self.root / "installers-linux.json"
        original = path.read_text()
        for mutation in [lambda m: m.update(tag="v0.2.0"), lambda m: m.update(artifacts=[]),
                         lambda m: m["artifacts"].pop(),
                         lambda m: m["artifacts"].append(m["artifacts"][0]),
                         lambda m: m["artifacts"][0].update(requires=["wrong"]),
                         lambda m: m["artifacts"][0].update(file="../escape")]:
            value = json.loads(original)
            mutation(value)
            path.write_text(json.dumps(value))
            with self.assertRaises((ValueError, OSError)):
                pkg.verified_entries(self.root, "v0.1.0")
        path.write_text(original)
        first = json.loads(original)["artifacts"][0]["file"]
        (self.root / first).write_bytes(b"corrupt")
        with self.assertRaises(ValueError):
            pkg.verified_entries(self.root, "v0.1.0")

    def test_tool_errors_and_missing_inputs_fail_without_publishing_partial_outputs(self):
        source_plan(self.root)
        out = self.root / "out"
        out.mkdir()
        with patch.object(pkg.subprocess, "check_output", return_value="nfpm version 2.48.0"):
            with self.assertRaises(OSError):
                pkg.build_linux(self.root, out, "v0.1.0")
        self.assertEqual(list(out.iterdir()), [])
        with patch.object(pkg.subprocess, "check_output", side_effect=subprocess.CalledProcessError(1, "nfpm")):
            with self.assertRaises(subprocess.SubprocessError):
                pkg.build_linux(self.root, out, "v0.1.0")
        with self.assertRaises(ValueError):
            pkg.write_manifest(out, "empty.json", "v0.1.0", [])
        process = subprocess.run([sys.executable, str(pkg.ROOT / "installers.py"), "verify", "--tag", "v0.1.0",
                                  "--input", str(self.root), "--output", str(out)], capture_output=True)
        self.assertNotEqual(process.returncode, 0)
        self.assertEqual(list(out.iterdir()), [])

    def test_windows_builder_emits_shared_payloads_with_unique_setup_identity(self):
        source_plan(self.root)
        for app in pkg.apps():
            archive(self.root, app["program"], pkg.WINDOWS)
        runtime(self.root)
        out = self.root / "out"
        out.mkdir()
        compiled = []

        def compiler(args):
            defines = {str(a).split("=", 1)[0]: str(a).split("=", 1)[1] for a in args if str(a).startswith("/D")}
            payload = Path(defines["/DPayload"])
            files = (payload / "files.iss").read_text()
            program = defines["/DAppProgram"]
            app = next(a for a in pkg.apps() if a["program"] == program)
            for part in pkg.windows_payload(app):
                self.assertIn(part + ".exe", files)
                self.assertIn(part + ".version", files)
                self.assertEqual((payload / (part + ".version")).read_text().strip(), "0.1.0")
                self.assertIn(f"CheckProgram('{part}')", (payload / "checks.iss").read_text())
                for suffix in (".exe", ".version", ".exe.torro-package.json"):
                    self.assertIn(f"CheckUnlocked('{part}{suffix}')", (payload / "locks.iss").read_text())
                self.assertIn(f"VerifyProgram('{part}', '{pkg.digest(payload / (part + '.exe'))}')", files)
                self.assertIn(f"VerifyVersion('{part}')", files)
                self.assertIn(f"WriteMarker('{part}')", files)
            dlls = len(RUNTIME_FILES) if pkg.ASSISTANT in pkg.windows_payload(app) else 0
            for name, pin in runtime_pins().items():
                if dlls:
                    self.assertIn(f"VerifyFile('{name}', '{pin}')", files)
                    self.assertIn(f"CheckFile('{name}')", (payload / "checks.iss").read_text())
                    self.assertIn(f"CheckUnlocked('{name}')", (payload / "locks.iss").read_text())
                    # The DLLs come before the program that loads them.
                    self.assertLess(files.index(f"VerifyFile('{name}'"), files.index("VerifyProgram('torroassistant'"))
                else:
                    self.assertNotIn(name, files)
            self.assertEqual(files.count('DestDir: "{code:CopyDirectory}"'), 3 * len(pkg.windows_payload(app)) + dlls)
            self.assertNotIn("Check: CanCopy", files)
            self.assertIn('Source: "{code:FinalInstallCheck}"', files)
            self.assertIn("external skipifsourcedoesntexist", files)
            final = files.strip().splitlines()[-1]
            self.assertIn('Source: "{code:FinalInstallCheck}"', final)
            self.assertIn("ExternalSize: 0", final)
            for selector in ("Check:", "Components:", "Tasks:", "Languages:"):
                self.assertNotIn(selector, final)
            self.assertEqual(files.count("sharedfile uninsnosharedfileprompt 64bit"), 3 * len(pkg.windows_payload(app)) + dlls)
            name = next(str(a)[2:] for a in args if str(a).startswith("/F"))
            (out / (name + ".exe")).write_bytes(b"fixture setup")
            compiled.append(defines["/DAppIdentity"])

        def compiler_version(args, **kwargs):
            # Inno 7's actual /? output only says "Inno Setup 7"; --version emits 7.1.1.
            self.assertEqual(args[1:], ["--version"])
            return subprocess.CompletedProcess(args, 0, "7.1.1\r\n", "")

        with patch.object(pkg.subprocess, "run", side_effect=compiler_version), patch.object(pkg, "run", side_effect=compiler), \
                patch.object(pkg, "assistant_runtime", return_value=runtime_pins()):
            pkg.build_windows(self.root, out, "v0.1.0")
        self.assertEqual(len(set(compiled)), 9)
        self.assertEqual(len(list(out.glob("*-setup.exe"))), 9)

    def test_the_assistants_runtime_holds_exactly_the_pinned_dlls(self):
        scratch = self.root / "scratch"
        scratch.mkdir()
        with patch.object(pkg, "assistant_runtime", return_value=runtime_pins()):
            runtime(self.root)
            unpacked = pkg.unpack_runtime(self.root, scratch)
            self.assertEqual(sorted(unpacked), sorted(RUNTIME_FILES))
            for files in [{**RUNTIME_FILES, "extra.dll": b"MZ"}, {"onnxruntime.dll": RUNTIME_FILES["onnxruntime.dll"]},
                          {**RUNTIME_FILES, "onnxruntime.dll": b"MZ other build"}]:
                with self.subTest(files=sorted(files)):
                    runtime(self.root, files)
                    with self.assertRaises(ValueError):
                        pkg.unpack_runtime(self.root, scratch)
            runtime(self.root)
            path = self.root / f"torroassistant-runtime-{pkg.WINDOWS}.zip"
            path.write_bytes(path.read_bytes() + b"tampered")
            with self.assertRaises(ValueError):
                pkg.unpack_runtime(self.root, scratch)

    def test_the_pinned_runtime_matches_the_locked_sherpa_onnx(self):
        sys.path.insert(0, str(pkg.ROOT))
        import assistant_runtime as rt
        value = rt.pinned()
        self.assertEqual(rt.locked_version(rt.LOCK), value["sherpa_onnx"])
        self.assertEqual(value["target"], pkg.WINDOWS)
        self.assertEqual(pkg.assistant_runtime(), value["files"])
        self.assertIn(f'v{value["sherpa_onnx"]}/{value["archive"]}', value["url"])
        self.assertEqual(sorted(value["files"]), ["onnxruntime.dll", "sherpa-onnx-c-api.dll"])

    def test_windows_builder_rejects_wrong_or_failed_compiler_version(self):
        source_plan(self.root)
        out = self.root / "out"
        out.mkdir()
        for result in [subprocess.CompletedProcess([], 0, "7.1.10", ""),
                       subprocess.CompletedProcess([], 1, "7.1.1", "compiler failed")]:
            with self.subTest(result=result), patch.object(pkg.subprocess, "run", return_value=result):
                with self.assertRaises(ValueError):
                    pkg.build_windows(self.root, out, "v0.1.0")
            self.assertEqual(list(out.iterdir()), [])

    def test_setup_rejects_inf_files_that_can_disable_native_cancellation(self):
        spec = (pkg.ROOT / "windows/setup.iss").read_text()
        initialize = spec.split("function InitializeSetup(): Boolean;", 1)[1].split("procedure FailInstallation", 1)[0]
        self.assertIn("Pos('/nocancel', Lowercase(ParamStr(Index))) = 1", initialize)
        # Inno also reads NoCancel=yes from /LOADINF, before InitializeSetup runs.
        self.assertIn("Pos('/loadinf', Lowercase(ParamStr(Index))) = 1", initialize)
        self.assertIn("Result := False;", initialize)

    def test_fatal_abort_uses_constant_expansion_outside_swallowed_callbacks(self):
        spec = (pkg.ROOT / "windows/setup.iss").read_text()
        # Neither swallowed entry callbacks nor a hidden Session-0 wizard can abort reliably.
        self.assertIn('Name: "{code:CheckedInstallDirectory}"', spec)
        self.assertNotIn("WizardForm.Close", spec)
        self.assertNotIn("BeforeInstall: CheckReadyToCopy", spec)
        for signature in ["function CopyDirectory(Param: String): String;",
                          "function FinalInstallCheck(Param: String): String;",
                          "function CheckedInstallDirectory(Param: String): String;"]:
            body = spec.split(signature, 1)[1].split("\nend;", 1)[0]
            self.assertIn("AbortIfFailed();", body)
            self.assertLess(body.index("AbortIfFailed();"), body.index("Result :="))
            if "CheckedInstallDirectory" in signature:
                self.assertLess(body.index("CheckReadyToCopy();"), body.index("AbortIfFailed();"))
        abort = spec.split("procedure AbortIfFailed();", 1)[1].split("\nend;", 1)[0]
        self.assertIn("if FatalFailure then", abort)
        self.assertIn("Abort;", abort)

    def test_setup_enforces_destination_and_refusal_before_copying(self):
        spec = (pkg.ROOT / "windows/setup.iss").read_text()
        self.assertIn("else if not Preflight()", spec)
        self.assertIn("if not FixedDestination() then", spec)
        self.assertIn("CurStep = ssInstall", spec)
        self.assertIn("WizardForm.PreparingNoRadio.Visible and WizardForm.PreparingNoRadio.Checked", spec)
        self.assertIn('"locks.iss"', spec)
        self.assertIn("OpenForPreflight(FileName, $40000000, 0, 0, 3, 0, 0)", spec)
        self.assertNotIn("ssPostInstall", spec)
        self.assertIn("GetSHA256OfFile", spec)
        self.assertIn('Name: "{code:CheckedInstallDirectory}"', spec)
        self.assertIn("FatalFailure := True;", spec)
        self.assertIn("Abort;", spec)
        self.assertIn("AllowCancelDuringInstall=yes", spec)
        self.assertIn("Pos('/nocancel'", spec)
        callbacks = spec[spec.index("procedure CheckUnlocked"):spec.index("procedure CurStepChanged")]
        self.assertNotIn("RaiseException", callbacks)

    def test_release_gates_are_mandatory_and_build_jobs_have_no_manual_or_pr_trigger(self):
        workflow = (pkg.ROOT.parent / ".github/workflows/release.yml").read_text()
        self.assertIn("needs.custom-ci.result == 'success'", workflow)
        self.assertIn("needs.custom-build-installers.result == 'success'", workflow)
        self.assertIn("needs.custom-publish-macos-app.result == 'success'", workflow)
        self.assertLess(workflow.index("installers.py verify"), workflow.index("dist host ${{ needs.plan.outputs.tag-flag }}"))
        for file in ["build-installers.yml", "publish-macos-app.yml"]:
            workflow = (pkg.ROOT.parent / ".github/workflows" / file).read_text()
            self.assertNotIn("workflow_dispatch:", workflow)
            self.assertNotIn("pull_request:", workflow)
            self.assertNotIn("gh release", workflow)

    def test_the_runtime_builder_checks_kfsas_archive_and_writes_the_same_zip_twice(self):
        sys.path.insert(0, str(pkg.ROOT))
        import assistant_runtime as rt
        source = self.root / "k2.tar.bz2"
        with tarfile.open(source, "w:bz2") as tar:
            for name, body in [*RUNTIME_FILES.items(), ("onnxruntime_providers_shared.dll", b"MZ not needed")]:
                member = tarfile.TarInfo(f"sherpa-onnx-lib/lib/{name}")
                member.size = len(body)
                tar.addfile(member, io.BytesIO(body))
        value = {"sha256": pkg.digest(source), "files": runtime_pins()}
        self.assertEqual(rt.dlls(value, source), RUNTIME_FILES)
        for broken in [{**value, "sha256": "0" * 64}, {**value, "files": {**runtime_pins(), "onnxruntime.dll": "0" * 64}}]:
            with self.subTest(broken=broken), self.assertRaises(ValueError):
                rt.dlls(broken, source)
        first, second = self.root / "a.zip", self.root / "b.zip"
        rt.write_zip(RUNTIME_FILES, first)
        rt.write_zip(dict(reversed(list(RUNTIME_FILES.items()))), second)
        self.assertEqual(first.read_bytes(), second.read_bytes())

    def test_the_powershell_installer_brings_the_assistants_dlls_checked(self):
        sys.path.insert(0, str(pkg.ROOT))
        import assistant_runtime as rt
        script = self.root / "torroassistant-installer.ps1"
        body = "function Download($download_url, $platforms, $arch) {\n  $bin_paths = @()\n" + rt.ANCHOR + "  }\n}\n"
        script.write_text(body)
        rt.patch_powershell(script)
        text = script.read_text()
        step = text[text.index(rt.MARKER):text.index(rt.ANCHOR)]
        self.assertIn('"$download_url/$runtime_name.sha256"', step)
        self.assertIn("Get-FileHash -Path $runtime_zip", step)
        for name, digest in rt.pinned()["files"].items():
            self.assertIn(f'"{name}" = "{digest}"', step)
        self.assertIn("$bin_paths += $dll_path", step)
        with self.assertRaises(ValueError):
            rt.patch_powershell(script)
        for layout in ["function Download() {}\n", rt.ANCHOR * 2 + "function Download(\n", rt.ANCHOR + "function Download(\n"]:
            with self.subTest(layout=layout):
                script.write_text(layout)
                with self.assertRaises(ValueError):
                    rt.patch_powershell(script)

    def test_the_global_release_job_patches_the_powershell_installer_before_the_upload(self):
        workflow = (pkg.ROOT.parent / ".github/workflows/release.yml").read_text()
        job = workflow[workflow.index("  build-global-artifacts:"):workflow.index("  custom-publish-macos-app:")]
        patch = "python3 packaging/assistant_runtime.py --patch-powershell target/distrib/torroassistant-installer.ps1"
        self.assertLess(job.index('--artifacts=global'), job.index(patch))
        self.assertLess(job.index(patch), job.index("name: artifacts-build-global"))


if __name__ == "__main__":
    unittest.main()
