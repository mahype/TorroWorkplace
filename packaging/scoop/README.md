# Scoop (Windows, later)

Templates for a Scoop bucket `mahype/scoop-torro`: **one manifest per app**, so every app can be installed on
its own and runs without the main application (issue #411, `docs/einzelne-apps.md`).

| Manifest | Downloads (release zips) | On the `PATH` and in the Start menu |
|---|---|---|
| `torroworkplace.json` | `torroworkplace` | `torroworkplace.exe`, "Torro Workplace" (the main application) |
| `torroinbox.json` | `torroinbox`, `torroworkplace` | `torroinbox.exe`, "Torro Notification Hub" |
| `torro-inbox-mail.json` | `torro-inbox-mail`, `torroinbox`, `torroworkplace` | `torro-inbox-mail.exe`, "Torro Workplace Mail" |
| `torro-inbox-notifications.json` | `torro-inbox-notifications`, `torroinbox`, `torroworkplace` | `torro-inbox-notifications.exe`, "Torro App Feed" |
| `torrocalendar.json` | `torrocalendar`, `torroworkplace` | `torrocalendar.exe`, "Torro Calendar" |
| `torrotasks.json` | `torrotasks`, `torroworkplace` | `torrotasks.exe`, "Torro Tasks" |
| `torroassistant.json` | `torroassistant`, `torroworkplace` | `torroassistant.exe`, "Torro Assistant" |
| `torro-suite.json` | nothing; `depends` on all seven | – |

- **Each app brings the service.** Its folder gets the app's zip and the zip of `torroworkplace` (the
  background service), so the app finds the service next to itself. Mail and App Feed also get `torroinbox`,
  because they open its compose window. Only the app's own program goes on the `PATH` (`bin`) and into the
  Start menu (`shortcuts`), so the copies do not collide.
- **The main application** is `torroworkplace.json`. Its Start menu entry "Torro Workplace" is what tells the
  service that the main application is installed: then it shows the tray icon and the setup.
- **Updates:** the `checkver`/`autoupdate` blocks let the bucket's own GitHub Action set the version, the URLs
  and their hashes after each release. Until the first release the hashes are placeholders
  (`TODO-sha256-of-the-zip`). `torro-suite.json` has no files and needs no version bump.

Install one app, the main application or the whole suite:

```powershell
scoop bucket add torro https://github.com/mahype/scoop-torro
scoop install torrocalendar
scoop install torroworkplace
scoop install torro-suite
```

Do not use these files before Windows is a supported target (see `docs/release.md`).
