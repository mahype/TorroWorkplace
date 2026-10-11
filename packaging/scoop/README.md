# Scoop (Windows, later)

Template for a Scoop bucket `mahype/scoop-torro`. Only `torroworkplace.json` exists as the pattern;
`torrocalendar.json` and `torrotasks.json` are copies with the name and the zip name changed.
The `checkver`/`autoupdate` blocks let the bucket's own GitHub Action update the manifests after each
release. A manifest describes exactly one app, so the whole suite is installed with:

```powershell
scoop bucket add torro https://github.com/mahype/scoop-torro
scoop install torroworkplace torrocalendar torrotasks
```

Do not use these files before Windows is a supported target (see docs/release.md).
