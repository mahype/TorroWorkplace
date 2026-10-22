# Scoop (Windows, later)

Template for a Scoop bucket `mahype/scoop-torro`. `torroworkplace.json` is **one manifest for the whole
suite** (decision 8 in `docs/lebenszyklus.md`); there are no manifests per app.

- **All programs, one folder.** The manifest downloads the release zip of each of the seven programs –
  `torroworkplace` (core and control centre), `torroinbox` (Notification Hub), `torro-inbox-mail` (Mail),
  `torro-inbox-notifications` (App Feed), `torrocalendar`, `torrotasks` and `torroassistant` – into the
  same folder and puts every `.exe` on the `PATH` (`bin`). There the apps find the core and each other:
  an app starts the core when it is not running, and Mail and App Feed open the compose window of
  `torroinbox`.
- **Seven Start menu entries** (`shortcuts`), one per program and without arguments: Torro Workplace,
  Torro Notification Hub, Torro Workplace Mail, Torro App Feed, Torro Calendar, Torro Tasks and
  Torro Assistant. Mail and App Feed are programs of their own, because Windows groups the taskbar by
  `.exe` and shows the icon built into it.
- **Updates:** the `checkver`/`autoupdate` blocks let the bucket's own GitHub Action set the version,
  the seven URLs and their hashes after each release. Until the first release the hashes are
  placeholders (`TODO-sha256-of-the-zip`).

Install the suite with:

```powershell
scoop bucket add torro https://github.com/mahype/scoop-torro
scoop install torroworkplace
```

Do not use this file before Windows is a supported target (see `docs/release.md`).
