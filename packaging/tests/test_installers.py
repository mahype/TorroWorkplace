"""Offline installer contracts, failure cases and the complete download matrix."""

import io
import json
from pathlib import Path
import stat
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

    def test_seven_identities_and_program_launch_arguments_match_rust_and_desktop(self):
        self.assertEqual(len(pkg.apps()), 7)
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
        self.assertEqual(len([p for p in owners if p.startswith("/usr/bin/") and not p.endswith(".json")]), 7)

    def test_stable_and_rc_native_versions(self):
        for fmt in pkg.FORMATS:
            self.assertEqual(pkg.native_version("1.2.3", fmt), "1.2.3")
        self.assertEqual(pkg.native_version("1.2.3-rc.10", "deb"), "1.2.3~rc.10")
        self.assertEqual(pkg.native_version("1.2.3-rc.10", "rpm"), "1.2.3~rc.10")
        self.assertEqual(pkg.native_version("1.2.3-rc.10", "archlinux"), "1.2.3rc.10")
        for tag in ["", "v1.2", "../v1.2.3", "v1.2.3;cmd", "v1.2.3-rc.0", "v01.2.3", "v65535.0.0"]:
            with self.assertRaises(ValueError):
                pkg.version(tag)

    def test_checked_sources_plan_and_archive_paths(self):
        source_plan(self.root)
        pkg.source_plan(self.root, "v0.1.0")
        with self.assertRaises(ValueError):
            pkg.source_plan(self.root, "v0.2.0")
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
        self.assertEqual(len(pkg.verified_entries(self.root, "v0.1.0")), 69)
        pkg.downloads(self.root, self.root, "v0.1.0")
        text = (self.root / "downloads.md").read_text()
        self.assertIn("torroinbox-bin", text)
        self.assertIn("torroworkplace-bin", text)
        self.assertIn(".dmg)", text)
        self.assertIn("-setup.exe)", text)
        self.assertIn(".pkg.tar.zst)", text)

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
            self.assertEqual(files.count("Check: CanCopy"), 3 * len(pkg.windows_payload(app)))
            self.assertEqual(files.count("sharedfile uninsnosharedfileprompt 64bit"), 3 * len(pkg.windows_payload(app)))
            name = next(str(a)[2:] for a in args if str(a).startswith("/F"))
            (out / (name + ".exe")).write_bytes(b"fixture setup")
            compiled.append(defines["/DAppIdentity"])

        with patch.object(pkg.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, "7.1.1", "")), patch.object(pkg, "run", side_effect=compiler):
            pkg.build_windows(self.root, out, "v0.1.0")
        self.assertEqual(len(set(compiled)), 7)
        self.assertEqual(len(list(out.glob("*-setup.exe"))), 7)

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
        self.assertIn('Name: "{app}"; BeforeInstall: CheckReadyToCopy', spec)
        self.assertIn("WizardForm.Close;", spec)
        self.assertIn("FatalFailure := True;", spec)
        self.assertIn("Result := not FatalFailure;", spec)
        self.assertIn("Confirm := False;", spec)
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


if __name__ == "__main__":
    unittest.main()
