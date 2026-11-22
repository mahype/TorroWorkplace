# Windows resources

The `build.rs` of each GUI crate (`crates/*/build.rs`, using `winresource`) embeds one of these icons into its
executable, together with product name, version and publisher. Explorer, the taskbar and SmartScreen show them.

| Icon | Program | App |
|---|---|---|
| `torro-workplace.ico` | `torroworkplace` | Torro Workplace (control centre and core) |
| `torro-inbox.ico` | `torroinbox` | Torro Notification Hub |
| `torro-inbox-mail.ico` | `torro-inbox-mail` | Torro Workplace Mail |
| `torro-inbox-notifications.ico` | `torro-inbox-notifications` | Torro App Feed |
| `torro-calendar.ico` | `torrocalendar` | Torro Calendar |
| `torro-tasks.ico` | `torrotasks` | Torro Tasks |
| `torro-assistant.ico` | `torroassistant` | Torro Assistant |
| `torro-chat.ico` | `torrochat` | Torro Chat |
| `torro-contacts.ico` | `torrocontacts` | Torro Contacts |

Each `.ico` holds the sizes 16, 24, 32, 48, 64, 128 and 256 as PNG images with transparency (valid for Windows Vista
and later), largest last.

The native icons of Chat and Contacts are derived from their existing `linux/icons/torro-chat.svg` and
`linux/icons/torro-contacts.svg`: the Windows ICO keeps the flat crop; the macOS PNG restores the suite's 1024 canvas, 100-pixel body inset and drop shadow.

## Where they come from

The artwork is the variant "red" from `docs/prototypes/icons/red/` (task AP.7, issue #279). Windows and Linux use the
same flat version: `../linux/icons/<name>.svg`, cropped to a margin of 4 % and without the drop shadow, because these
systems put no common padding around icons. The macOS bundles take the version with the Big Sur margin and shadow
from `../macos/icons/<name>.png`.

When the artwork changes, change the SVG in `../linux/icons/` first, then make the `.ico` again: render the SVG at
each of the seven sizes as a PNG and pack the PNGs into one file. With ImageMagick:

```sh
magick -background none ../linux/icons/torro-tasks.svg \
  -define icon:auto-resize=256,128,64,48,32,24,16 torro-tasks.ico
```

Without ImageMagick, render each size with any SVG renderer (`rsvg-convert -w 32 -h 32 …`, or AppKit's `NSImage` on
macOS, which reads SVG) and pack the PNGs, smallest first, with a few lines of Python:

```python
import struct, sys  # pack.py <out.ico> <16.png> <24.png> … <256.png>
out, pngs = sys.argv[1], [open(p, "rb").read() for p in sys.argv[2:]]
head, offset, entries = struct.pack("<HHH", 0, 1, len(pngs)), 6 + 16 * len(pngs), b""
for png in pngs:
    size = struct.unpack(">I", png[16:20])[0]  # width from the PNG header
    side = 0 if size >= 256 else size  # 0 means 256 in an ICO directory entry
    entries += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(png), offset)
    offset += len(png)
open(out, "wb").write(head + entries + b"".join(pngs))
```

Check the result: the file has seven entries, and each is a PNG of the size in its directory entry.

`build.rs` only does something when the build target is Windows; it needs a resource compiler there (`rc.exe` from the
Windows SDK with MSVC, or `llvm-rc` when cross building).

## The assistant's DLLs (#773)

On Windows `torroassistant` links sherpa-onnx as a DLL (feature `shared`, `crates/torroassistant/Cargo.toml`):
k2-fsa builds its static Windows libraries only against the static C runtime, which forced `+crt-static` on every
program of the suite. Now only the assistant needs two files beside its program, `sherpa-onnx-c-api.dll` and
`onnxruntime.dll`; both carry their own C runtime. The C runtime of the programs is as before #762 again: a plain
`cargo build` links it dynamically (Rust's default), the release links it statically because dist 0.32 sets
`msvc-crt-static` by default. The assistant links in both ways, since the DLLs bring their own.

- **Build:** sherpa-onnx's build script copies the DLLs next to the program in `target/<target>/<profile>/`, so
  `cargo run` works. Also when cross building with `cargo xwin build -p torroassistant --target x86_64-pc-windows-msvc`.
- **Release:** dist packs only programs into an app's archive. The DLLs go out as an asset of their own,
  `torroassistant-runtime-x86_64-pc-windows-msvc.zip` with its `.sha256`, built by `../assistant_runtime.py` from
  k2-fsa's archive. `assistant-runtime.json` pins that archive and each DLL by SHA-256 and the sherpa-onnx version
  against `Cargo.lock`; on an update of sherpa-onnx change it there. dist's global job builds the asset
  (`[package.metadata.dist.extra-artifacts]`); the Windows job of `build-installers.yml` builds it again, because
  it runs in parallel with that job.
- **Setup, the service's app installer and Scoop** put the DLLs beside `torroassistant.exe` and remove them with
  the assistant.
- **dist's PowerShell installer** copies only programs. The global job of `release.yml` (a hand edit) runs
  `../assistant_runtime.py --patch-powershell` on `torroassistant-installer.ps1` before the upload. The installer
  then also downloads the runtime archive and its `.sha256` from the same release, checks it, checks each DLL
  against the SHA-256 pinned into the script and copies both beside `torroassistant.exe`. The patch refuses a
  script whose layout it does not know, so a dist upgrade fails the release instead of shipping an installer
  without DLLs. The PowerShell installer has no uninstall, like for every program.

## All-users Setup (#589)

`setup.iss` is compiled with integrity-pinned Inno Setup **7.1.1** by `../installers.py build-windows`.
Nine stable AppIds (`io.github.mahype.<app-id>`), each with its own uninstall log/directory and Start
menu entry. Installation is x64/admin-only into the fixed `{autopf64}\TorroWorkplace` directory;
Workplace's shortcut passes `ui`. Other apps do not silently add the control-centre shortcut.

Each Setup includes its executable, the service and (Mail/App Feed) the Hub; the assistant's Setup also its DLLs,
each checked against its pinned SHA-256 after copying and kept as a shared file like the programs. Shared executables,
native markers and SemVer companion files use `sharedfile uninsnosharedfileprompt 64bit`, with
`UninstallLogMode=append`, so repeating/upgrading one app and uninstalling another retain components
until the last owner goes. Preflight rejects downgrades, missing/inconsistent markers and foreign files
before copying. `/DIR` and saved nonstandard destinations are rejected. Declining Restart Manager
closure aborts before installation. After its close attempt an exclusive-access check covers every
payload program, version and marker before the first program is copied; unresolved locks abort.
Per-file hash/version checks also prevent Skip from advancing metadata beyond an old executable. Setup
never starts an app elevated, installs a system service, or removes personal data.

Inno swallows Before/AfterInstall and Check exceptions. Closing the wizard also failed in the native
noninteractive silent test, despite reporting success and changing uninstall files. Callbacks now only
record safety failures. Mandatory code constants propagate `Abort` outside those swallowed handlers:
the `[Dirs]` name after Restart Manager and before uninstall metadata, each payload destination, and
an unconditional final external source after the last callback. The final source is a nonexistent file
with `skipifsourcedoesntexist` and `ExternalSize: 0`, so success copies nothing and avoids eager size
probing. No Check/component/task may skip this final guard. `/NOCANCEL` and `/LOADINF` remain rejected.
The unchanged production Calendar Setup returned failure with unchanged payload/uninstall hashes and
uninstall registry metadata in the focused native lock test. Injected executable/final-marker exceptions
also returned failure without publishing uninstall metadata; the executable failure did not advance its
version sidecar. Existing payloads were not fully restored (one/five files changed respectively), so a
damaged installation after a late failure must be removed before reinstalling. A full lifecycle rerun
passed with the production RC/Stable Setups after preserving and uninstalling fixtures from an OS-reboot-
interrupted run: all seven programs loaded with matching versions, repeat/upgrade, unsafe-INF/locked-upgrade
and downgrade refusal, both removal orders, shared-file lifetimes and retained test data. This is native
guest-agent SYSTEM evidence, not interactive UAC, SmartScreen or companion-function verification.

Unsigned by decision: users may see SmartScreen. `tests/windows-smoke.ps1` (relative to `packaging/`)
must be run only in a fresh disposable x64 Windows VM as administrator, with older RC and newer stable
Setups. It tests all apps, repeat/upgrade, downgrade refusal, both uninstall orders and shared lifetimes.
UAC, SmartScreen, app start and locked-file cancellation still need manual verification. See
[installation](../../docs/install.md) and [release gates](../../docs/release.md).
