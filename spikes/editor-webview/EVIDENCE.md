# Belege zu #917 (Auszug)

Lauf am 2026-11-20/21, Hyprland (Wayland), RTX 4090, WebKitGTK 2.54.1. Roh-Protokolle liegen nicht im Repo; das
Fenster schreibt sie auf stdout, die Befehle stehen unten.

## Abstürze (Fenster 3 s offen, Absturz = Exit ≠ 124 oder „Protocol error“)

| Variante | Abstürze |
|---|---|
| ohne Umgehung | 15 von 15 (`Gdk-Message: Error 71 (Protocol error)`; `WAYLAND_DEBUG=1`: `wp_linux_drm_syncobj_surface_v1#53 … "Missing acquire timeline"`) |
| `WEBKIT_DISABLE_DMABUF_RENDERER=1` | 0 von 15 |
| `__NV_DISABLE_EXPLICIT_SYNC=1` | 0 von 5 |
| `WEBKIT_DISABLE_COMPOSITING_MODE=1` | 5 von 5 |
| `GDK_BACKEND=x11` | 5 von 5 (Ursache nicht untersucht) |

Befehl je Start: `[VAR=…] timeout 3 ./target/release/editor-webview`. Die ersten Läufe ohne Umgehung gingen nur, weil der
Bildschirm per DPMS aus war.

## Messung (`--measure`, 5 Läufe, Median / Spanne)

- `WEBKIT_DISABLE_DMABUF_RENDERER=1`: bereit nach 231 ms (222–243), PSS 226 998 kB (226 888–227 280), 3 Prozesse.
- `__NV_DISABLE_EXPLICIT_SYNC=1`: 349 ms (344–360), PSS 240 831 kB (240 753–241 046).
- `torroinbox` lief nicht; kein Vergleichswert.

## Tasten (`hyprctl dispatch 'hl.dsp.send_shortcut({ mods = "CTRL", key = "A", window = "class:editor-webview" })'`; Z und V als `code:52`, `code:55`)

- Strg+A: `{"kind":"selection","chars":226,"images":2}`; danach Tippen von `Q`: `images: 0` (alles ersetzt).
- Strg+Z: `undo-state`, Text zurück. Strg+V mit `wl-copy --type text/html < fixtures/paste-synthetic.html`:
  `paste-sanitized` = `<p>Büro Größe <b>fett</b> und <a href="https://example.com/x">Link</a></p><table>…</table>`
  (Skript und Fremdbild entfernt; das Fremdbild stand vorher im Editor, `images` 2 → 3).
- `wtype` mit Strg-Kombinationen kam nicht an; Umlaute-Tippen (`wtype "Größe äöü ß „Öl“"`) ging.

## Von Hand (Nutzer, Fenster mit Umgehung)

- Drop aus dem Dateimanager: `{"native-drop":"enter","paths":["/home/…/screenshot-….png"],"position":[407,132]}`, danach
  `"drop"` mit Pfad und Position, `"leave"` beim Abbruch; die Seite meldet dazu `dom-drop` mit
  `"types":["text/uri-list","text/html"]`, `"files":[]`. Squire fügte die `file://`-Adresse als Text ein.
- Ziehen eines Bildes im Text: `native-drop` mit Pfad `torro://img/logo@torro.example`.
- Maus-Auswahl: `selection` bis `chars` 376 und `images` 3.
- Einfügen aus Chromium: `paste-offered` Typen `text/html`, `text/plain`, HTML 818 Bytes mit Inline-Stilen;
  `paste-sanitized` = reiner Text ohne Stile. Zweite Quelle: Typen `text/plain`, `text/html`, 570 Bytes,
  `paste-sanitized` = `<p>…</p>`. Ob die zweite Quelle LibreOffice war und eine Tabelle dabei war, ist nicht belegt.
