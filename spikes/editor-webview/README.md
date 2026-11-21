# editor-webview (Versuch #917)

Ein tao/wry-Fenster mit dem Editor Squire (lokal in `assets/`, kein Netz). Ergebnis und Zahlen:
`docs/research.md`, Abschnitt 9. Kein Produktcode.

```bash
cargo run --release                       # Fenster; jedes Ereignis als JSON-Zeile auf stdout
cargo run --release -- --selftest         # HTML hin und zurück gegen torro_mail::html (Exit 0 = ok)
cargo run --release -- --measure          # Startzeit und Speicher (PSS), fünf Läufe
./auto-check.sh nodmabuf                  # Zeichnen, Tippen, Auswahl, Einfügen auf einem laufenden Hyprland
```

**Auf NVIDIA unter Wayland** stürzt WebKitGTK ohne Umgehung ab (`Missing acquire timeline`). Starte mit
`WEBKIT_DISABLE_DMABUF_RENDERER=1` oder `__NV_DISABLE_EXPLICIT_SYNC=1`.

## Checkliste von Hand

1. Ein Bild aus dem Dateimanager auf den Text ziehen, dann auf den freien Bereich, dann ein PDF: im stdout `native-drop`
   mit Pfad und Position, `dom-drop` mit `inText`.
2. Mit der Maus über beide Bilder Text markieren: `selection` mit `images`.
3. Text mit Fett und einer Tabelle aus Chromium und aus LibreOffice kopieren und einfügen: `paste-offered`
   (Typen), `paste-sanitized`.
4. Optional mit einer Eingabemethode (fcitx5) schreiben.

Beim Kopieren lässt sich das HTML auch mit `wl-paste --list-types` und `wl-paste --type text/html` ansehen.
