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

Each `.ico` holds the sizes 16, 24, 32, 48, 64, 128 and 256 as PNG images with transparency (valid for Windows Vista
and later), largest last.

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

## All-users Setup (#589)

`setup.iss` is compiled with integrity-pinned Inno Setup **7.1.1** by `../installers.py build-windows`.
Seven stable AppIds (`io.github.mahype.<app-id>`), each with its own uninstall log/directory and Start
menu entry. Installation is x64/admin-only into the fixed `{autopf64}\TorroWorkplace` directory;
Workplace's shortcut passes `ui`. Other apps do not silently add the control-centre shortcut.

Each Setup includes its executable, the service and (Mail/App Feed) the Hub. Shared executables,
native markers and SemVer companion files use `sharedfile uninsnosharedfileprompt 64bit`, with
`UninstallLogMode=append`, so repeating/upgrading one app and uninstalling another retain components
until the last owner goes. Preflight rejects downgrades, missing/inconsistent markers and foreign files
before copying. `/DIR` and saved nonstandard destinations are rejected. Declining Restart Manager
closure aborts before installation. After its close attempt an exclusive-access check covers every
payload program, version and marker before the first program is copied; unresolved locks abort.
Per-file hash/version checks also prevent Skip from advancing metadata beyond an old executable. Setup
never starts an app elevated, installs a system service, or removes personal data.

Inno swallows Before/AfterInstall exceptions. Safety failures therefore request native cancellation
through `WizardForm.Close` and block all subsequent payload entries, rather than relying on a thrown
exception. The lock gate runs from `[Dirs]`, after Restart Manager; Inno processes cancellation before
copying any application files. `/NOCANCEL` is rejected so this works in silent mode too. This path is
source-checked against pinned Inno 7.1.1 but still requires the actual Windows-VM smoke test.

Unsigned by decision: users may see SmartScreen. `tests/windows-smoke.ps1` (relative to `packaging/`)
must be run only in a fresh disposable x64 Windows VM as administrator, with older RC and newer stable
Setups. It tests all apps, repeat/upgrade, downgrade refusal, both uninstall orders and shared lifetimes.
UAC, SmartScreen, app start and locked-file cancellation still need manual verification. See
[installation](../../docs/install.md) and [release gates](../../docs/release.md).
