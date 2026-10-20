# Spike WZ.1: printpdf setzt Markdown nach Vorlage

Aufgabe WZ.1 in [docs/tasks/assistent.md](../../docs/tasks/assistent.md), Issue #225, Stand 2026-10-19.
Eigenes Cargo-Projekt außerhalb des Workspace, kein Produktcode.

```bash
cargo run --release                     # setzt fixtures/sample.md in vier Varianten nach out/
cargo run --release -- html experiments/lists2.html   # rohes HTML mit denselben Schriften (Experimente)
cargo build --release --no-default-features           # ohne printpdf (Größenvergleich)
cargo build --release --no-default-features --features text   # nur Textschicht (Weg B, Größenvergleich)
```

Varianten in `out/` (gitignored): `a4-inter.pdf` (A4, Ränder 20 mm, Inter), `a4-inter-builtin.pdf` (Kopf, Fuß
und Seitenzahl von printpdf selbst), `a5-lato.pdf` (A5, Ränder 15 mm, Lato aus dem Design „channelr“),
`letter-inter.pdf` (US Letter, Ränder 25 mm). Eingabe: [fixtures/sample.md](fixtures/sample.md) mit allem aus
dem Umfang im Konzept, dazu rohes HTML, ein Bild und ein `javascript:`-Link, die wegfallen müssen.

## Ergebnis: Weg A (HTML-Weg von printpdf), mit sechs Umgehungen

Der HTML-Weg trägt, aber nicht mit dem HTML, das pulldown-cmark von sich aus schreibt. Der Kern schreibt das
HTML selbst (Ereignisse von pulldown-cmark, Text immer maskiert) und umgeht diese Schwächen von azul-layout 0.0.16:

1. **Kopf, Fuß, Seitenzahl und Logo** zeichnet der Kern nach dem Satz mit der Textschicht auf jede Seite
   (`PdfDocument::pages[i].ops`, Schrift über `add_font`, Logo über `add_image` + `Op::UseXobject`). Die
   eingebauten Felder von `GeneratePdfOptions` taugen nicht (Punkt 1 unten).
2. **Listen** als Flex-Zeilen (`<div class="item"><div class="marker">1.</div><div class="content">…`), Marker
   und Nummern setzt der Kern. Echte `<ol>/<li>` wiederholen den Text eines Eintrags mit Unterliste bzw. setzen
   doppelte Marker, sobald ein Eintrag Blöcke enthält.
3. **Tabellen** als Flex-Zeilen mit Spaltenbreiten aus der längsten Zelle (4–40 Zeichen, in Prozent) und
   `text-align` aus der Markdown-Ausrichtung. Echte `<table>` zeichnen keine Zellrahmen und brechen nach etwa
   zwei Dritteln der Seite um.
4. **Fett** über eine eigene Schriftfamilie: Jede Schrift steht unter ihrem Dateinamen im Font-Map
   (`Inter-SemiBold`, `Lato-Bold`), das CSS nennt diesen Namen. `font-weight` wählt keinen Schnitt.
5. **Nur eingebettete Schriften:** `from_html_with_cache` mit `html::build_font_pool(fonts,
   Some(&["torro-no-system-fonts"]))`. Ohne das durchsucht printpdf die Systemschriften, und es landete
   `HelveticaNeue` des Macs in der PDF (gleiches Ergebnis auf allen Systemen verlangt das Konzept).
6. **Links** bekommen je eine eigene Farbe (16 × 16 Töne von #0b57d0, unsichtbar verschieden). Nach dem Satz
   sucht der Kern die Glyphen dieser Farbe (`SetFillColor` + `SetTextMatrix` je Glyphe) und legt je Zeile eine
   `Op::LinkAnnotation` mit `Actions::Uri` und Rahmenbreite 0 darüber. Der HTML-Weg selbst erzeugt keine
   Link-Annotationen.

Rahmen gehen nur in `px` (`1px solid`), nicht in `pt`; `border-left` allein zeichnet nichts, das Zitat hat deshalb
einen Balken aus einem Flex-Kind mit Hintergrundfarbe. Codeblöcke brauchen `white-space: pre`.

## Punkte der Aufgabe

| Punkt | Ergebnis | Beleg |
|---|---|---|
| Kopf- und Fußzeile mit Seitenzahl über `GeneratePdfOptions` | **nein**: `header_text`, `footer_text` und `show_page_numbers` erscheinen mit `from_html` in 0.12.8 gar nicht (`pdftotext` findet weder „Page“ noch den Fußtext) | `a4-inter-builtin.pdf`, Seiten 1–3 |
| „Page X of Y“ fest englisch? | **ja**, fest im Code: `azul-layout-0.0.16/src/solver3/pagination.rs`, `build_margin_content` (`"Page "`, `" of "`) | Quelltext |
| Seitenzahl selbst in die Fußzeile | **ja**, über die Textschicht nach dem Satz, Seitenzahl bekannt (`pages.len()`), rechtsbündig über die Glyphenbreiten; „Seite 1 von 3“ | `a4-inter.pdf`, Seite 1 unten rechts |
| Tabellen über Seitengrenzen | **ja** als Flex-Zeilen (Umbruch zwischen Zeilen, volle Seite); Kopfzeile wird **nicht** wiederholt. Echte `<table>`: nein (keine Rahmen, Umbruch bei etwa 65 %) | `a4-inter.pdf`, Seiten 2–3; `experiments/table.html` |
| Umlaute und `ß` mit eingebetteter TTF | **ja**, Inter aus `crates/torro-gui/assets/fonts/` und Lato aus `fonts/channelr/`, auch in Kopf und Fuß | `a4-inter.pdf` und `a5-lato.pdf`, Seite 1 |
| Fett ohne Bold-Datei (Inter) | **ja** mit der SemiBold-Datei als eigener Familie; Lato mit Lato-Bold | `a4-inter.pdf`, Seite 1 („fett“, Überschriften, Tabellenkopf) |
| Kursiv ohne Italic-Datei | **nein**: azul schrägt nicht nach, kein Design-Paket hat eine Italic-Datei. Kursiv fällt aufrecht aus (Text bleibt); die Basis-14-Schrift `Helvetica-Oblique` ginge, passt aber nicht zur Suite | `a4-inter.pdf`, Seite 1 („kursiv“); `experiments/fonts.html` |
| `page-break-before: always` für `\pagebreak` | **ja** (`<div class="pagebreak">`), nur als eigener Absatz | `a4-inter.pdf`: „Preise“ beginnt auf Seite 2 |
| verschachtelte Listen | **ja** als Flex-Zeilen, drei Ebenen; mit `<li>` nein (doppelter Text) | `a4-inter.pdf`, Seite 1; `experiments/lists.html` |
| Codeblöcke | **ja** mit `white-space: pre` in der Basis-14-Schrift `Courier` (eingebettet als `CourierNewPSMT`; deckt nur Win-1252, Zeichen außerhalb fehlen) | `a4-inter.pdf`, Seite 1 |
| Zitate | **ja** mit Balken aus einem Flex-Kind | `a4-inter.pdf`, Seite 1 |
| klickbare Links | **ja** über Farbmarke und `LinkAnnotation` (`/URI` für `https://torro.example/angebot` und `mailto:anna@example.org` in der PDF); `javascript:` bleibt Text | `a4-inter.pdf`, Seite 1 |
| Logo (PNG) in der Kopfzeile | **ja**, auf jeder Seite über `add_image` + `UseXobject` (8 mm); zusätzlich als `<img>` im Text getestet | `a4-inter.pdf`, Seiten 1–3 oben rechts |
| Papier A4, A5, US Letter mit Rändern | **ja**: 595×842 pt, 420×595 pt, 612×792 pt | `pdfinfo` der drei Varianten |
| rohes HTML, Bilder | fallen weg, ihr Text bleibt („alert(1)“ als Text, Alt-Text „Ein Bild“) | `a4-inter.pdf`, Seite 1 |
| Satzzeit | 200–280 ms je Dokument (Release, mit Aufbau des Font-Pools) | Ausgabe von `cargo run --release` |

## Bauzeit und Größe (macOS, aarch64, Release mit `lto = "thin"`, `strip = "debuginfo"`)

| Bau | Zeit (frisch) | Binary |
|---|---|---|
| ohne printpdf (nur pulldown-cmark) | 2,7 s | 0,8 MB |
| nur Textschicht (Weg B, `default-features = false`, `png`, `jpeg`) | 22,6 s | 2,9 MB |
| HTML-Weg (Weg A, `html`, `png`, `jpeg`) | 43,3 s | 15,0 MB |

Weg A bringt also etwa **14 MB** mit (der Kern hat heute etwa 28 MB). Ein großer Teil sind die
Silbentrennungs-Wörterbücher aller Sprachen: azul-layout zieht `hyphenation` mit `embed_all`, abschalten geht ohne
Fork nicht. 147 Crates im Baum; Lizenzen alle frei, darunter `azul-simplecss` (MPL-2.0, wie Stylo schon im
Workspace) und BSD-3-Clause.

## Linux und Windows

- `cargo check --release --target x86_64-pc-windows-msvc`: grün.
- `cargo check --release --target x86_64-unknown-linux-gnu`: grün (Ziel dafür nachinstalliert).
- Erzeugt und angesehen nur auf macOS. Eine PDF unter Linux und Windows zu erzeugen bleibt von Hand
  (Verifikation von #225).

## Empfehlung

**Weg A.** Der HTML-Weg setzt alles aus dem Umfang mit den sechs Umgehungen oben; Weg B müsste Umbruch, Listen,
Tabellen, Seitenumbruch und Silbentrennung selbst bauen. Kosten von A: etwa 14 MB mehr im Binary und 20 s mehr
im frischen Release-Bau. Was A nicht kann und WZ.3 so übernimmt:

- Kursiv fällt aufrecht aus, solange ein Design-Paket keine Italic-Datei hat.
- Der Tabellenkopf wiederholt sich auf Folgeseiten nicht.
- Durchstreichen und Unterstreichen sitzen um eine Glyphe versetzt (Fehler in azul-layout, rein optisch).
- Code nur in Courier (Win-1252); eine Monospace-Schrift gibt es in keinem Design-Paket.
- Die Fläche der Link-Annotation endet nach der letzten Glyphe mit einer Schätzung; WZ.3 nimmt die echte
  Glyphenbreite (`ParsedFont::get_glyph_width_internal` über die `gid` der Glyphe).
