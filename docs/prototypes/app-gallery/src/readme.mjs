// Schreibt README.md und index.html der Studie. Der Text steht hier, die Galerie je App wird aus apps.mjs erzeugt.
//   node docs/prototypes/app-gallery/src/readme.mjs [--base <url-prefix>]
// Mit --base entstehen absolute Bild-URLs (für den Kommentar im Issue), sonst relative Pfade.
import { writeFileSync, readdirSync, statSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { APPS, IDEAS, real, planned, byId } from './apps.mjs';
import { hues } from './pages.mjs';

const OUT = join(dirname(fileURLToPath(import.meta.url)), '..');
const baseArg = process.argv.indexOf('--base');
const BASE = baseArg > 0 ? process.argv[baseArg + 1].replace(/\/?$/, '/') : '';
const img = (path, alt, w) => `<img src="${BASE}${path}" alt="${alt}"${w ? ` width="${w}"` : ''}>`;
const kb = (f) => Math.round(statSync(join(OUT, f)).size / 1024);

const H = hues();
const statusLabel = { catalog: 'im Katalog (heute)', host: 'Hauptanwendung (kein Katalogeintrag)', planned: 'geplant, noch nicht im Katalog', parked: 'zurückgestellt (Konzept in Issue-Kommentaren)' };

const inventory = APPS.map((a) => {
  const d = H.dark.find((r) => r.id === a.id), l = H.light.find((r) => r.id === a.id);
  return `| **${a.name}** | ${statusLabel[a.status]} | \`${a.cls}\` | ${a.q} | \`${a.hue.dark}\` / \`${a.hue.light}\` | ${d.cIcon.toFixed(1)} / ${l.cIcon.toFixed(1)} | ${a.src ?? (a.status === 'host' ? '`AppId::WORKPLACE`' : '`AppId::ALL`, `ui/apps.rs`')} |`;
}).join('\n');

const ideas = IDEAS.map(([n, d, s]) => `| ${n} | ${d} | ${s} |`).join('\n');

const perApp = APPS.map((a) => {
  const mark = a.status === 'catalog' ? '' : a.status === 'host' ? ' · Hauptanwendung' : ' · **geplant, nicht realisiert**';
  const light = a.status === 'catalog' || a.status === 'host';
  return `### ${a.name}${mark}

*${a.q}* — ${a.s}

| Karte (Dunkel) | Karte (Hell) |
|---|---|
| ${img(`cards/${a.id}-dark.png`, `${a.name}: Karte dunkel`, 440)} | ${img(`cards/${a.id}-light.png`, `${a.name}: Karte hell`, 440)} |

Welcome-Screen (Start-Ansicht beim ersten Öffnen), ${light ? 'dunkel und hell' : 'dunkel'}:

| Dunkel | ${light ? 'Hell' : ''} |
|---|---|
| ${img(`welcome/${a.id}-dark.png`, `${a.name}: Welcome dunkel`, 440)} | ${light ? img(`welcome/${a.id}-light.png`, `${a.name}: Welcome hell`, 440) : ''} |
`;
}).join('\n');

const readme = `# App-Galerie: Karten und Start-Screens für jede Torro-App

Studie zu Issue [#970](https://github.com/mahype/TorroWorkplace/issues/970). **Nur Mockups und Review, kein Redesign:** Im Produktcode
ändert sich nichts. Erstellt in der Nacht zum 23.11.2026.

## Kurzfassung

- **Ist-Zustand** als echte Screenshots der Steuerzentrale (Abschnitt 1). Der Katalog hat heute **8 Apps** in einer
  einheitlichen, nüchternen Karte (Icon + Text); die Karte ist sauber, aber die Apps sind nur am Namen zu erkennen.
- **15 Karten-Mockups** mit je eigenem Motiv (8 Katalog-Apps, die Hauptanwendung und 6 geplante Apps), alle in einer
  Bildsprache: getönter Grund in der Appfarbe, Glühen, Punktraster, darauf eine abstrahierte Szene der App (Abschnitt 5).
- **Drei Gesamtlayouts**: A Editorial, B Creative Suite, C Product Preview, je hell und dunkel, dazu B in Mittel, Schmal und kleinstem Schmal (360)
  (Abschnitt 6). **Empfehlung: B**, mit dem Detail aus C als Seitenpaneel und dem Spotlight aus A nur für Updates.
- **Welcome-Screens** für jede App aus *einer* Vorlage plus Hero-Varianten L/M/S (Abschnitt 7).
- **Befund aus der Kontrastprüfung:** Das Hell-Grün der Aufgaben (\`#1F9D68\`) erreicht auf seiner eigenen Tönung nur 2,88:1.
  Im Vorschlag gilt \`#1A9262\` (3,25:1). Alle anderen Appfarben liegen auf Tönung und Fläche über 3:1 (Abschnitt 8).
- **9 Entscheidungspunkte** (Abschnitt 10) und eine **Umsetzungsskizze** (Abschnitt 11). Vor der Freigabe entsteht kein Code.

## Was echt ist und was nicht (bitte lesen)

| Material | Herkunft |
|---|---|
| \`ist/*.png\` | **Echte Screenshots.** \`torroworkplace ui --demo --shots\` (ohne Fenster, Beispieldaten im Speicher), Dunkel/Hell, Größen 1600×2000, 800×2800, 480×3600, 320×480, 240×160, 1200×200. Programm: ein vorhandener Debug-Build aus einem anderen Worktree (Commit \`bc227fc9\`, 22.11.2026 18:15), **nicht** der aktuelle \`main\` (\`7789963d\`). Die Übersicht ist seit #803 unverändert aufgebaut; ich habe nicht neu gebaut, weil auf dem Rechner andere Agenten schwere Rust-Builds fahren. |
| \`cards/\`, \`layouts/\`, \`system/\`, \`welcome/\` | **Mockups, keine Iced-Ausgabe.** HTML/SVG, mit Chromium zu PNG gerendert. Farben, Maße, Radius, Schrift (Inter aus \`torro-gui\`) und Icons (Lucide aus \`assets/icons\`) folgen dem Skill \`torro-design\`. Die Motive sind eigene Zeichnungen; keine fremden Assets, keine Adobe-Vorlagen. |
| \`art/*.svg\` | Die 30 Motive als einzelne SVGs (400×200), 3 bis 8 KB je Datei. Grundlage für spätere Assets. |
| Texte in den Mockups | Beispieltexte, englisch wie die Software. Die Katalogtexte der 8 Apps sind die echten aus \`ui/apps.rs\` (\`words()\`). |

**Nicht möglich bzw. nicht gemacht:** (1) Die Fenster der Apps selbst (Kalender, Mail, …) habe ich nicht neu aufgenommen; es lag nur
\`torroworkplace\` als fertiger Build vor. (2) Das Installationsfenster (\`--app-job\`) lässt sich nicht aus Beispieldaten zeichnen. Seine Schritte
stehen im Konzept von #411. (3) Wie die Motive in Iced aussehen (resvg: Filter, Verläufe, Text in SVG) ist **nicht geprüft**; das gehört in die
Umsetzung (Risiko in Abschnitt 11). (4) Ladezeit und Speicher sind geschätzt, nicht gemessen.

## Inhalt

1. [Ist-Zustand](#1-ist-zustand)
2. [Inventar aller Apps](#2-inventar-aller-apps)
3. [Referenzen und Regeln](#3-referenzen-und-regeln)
4. [Gemeinsames Kartensystem](#4-gemeinsames-kartensystem)
5. [Bildsprache und Motive](#5-bildsprache-und-motive)
6. [Drei Gesamtlayouts, Vergleich und Empfehlung](#6-drei-gesamtlayouts)
7. [Welcome-Screens und Hero-Header](#7-welcome-screens-und-hero-header)
8. [Prüfung](#8-prüfung)
9. [Empfehlung](#9-empfehlung)
10. [Entscheidungspunkte](#10-entscheidungspunkte)
11. [Umsetzungsskizze](#11-umsetzungsskizze)
12. [Galerie je App](#12-galerie-je-app)
13. [Neu erzeugen und Dateien](#13-neu-erzeugen-und-dateien)

## 1. Ist-Zustand

Die App-Karten stehen seit #803 in der **Übersicht** der Steuerzentrale, unter „At a glance“ und „Needs your attention“
(Code: \`crates/torroworkplace/src/ui/apps.rs\`, Entwurf \`docs/prototypes/app-catalog/\`, Ziel in #411). Aufbau: „Installed“ (5) oben, „Available“ (3) darunter;
Karte = Kachel 36 mit Icon, Name, Frage, Zustandspille, ein Satz, Version, Fensterklasse, ein Knopf und „…“.

${img('ist/overview-dark-1600x2000.png', 'Ist: Übersicht dunkel, breit', 900)}

| Hell, Mittel (800) | Dunkel, Schmal (480) | Dunkel, Mini (320×480) |
|---|---|---|
| ${img('ist/overview-light-800x2800.png', 'Ist: Mittel hell', 300)} | ${img('ist/overview-dark-480x3600.png', 'Ist: Schmal dunkel', 240)} | ${img('ist/overview-dark-320x480.png', 'Ist: Mini', 240)} |

Weitere echte Aufnahmen: \`ist/overview-light-1600x2000.png\`, \`ist/overview-dark-480x3600.png\`/\`-light-\`, \`ist/overview-dark-240x160.png\`, \`ist/overview-dark-1200x200.png\`.
Mini (240×160, 320×480) zeigt nur „2 need attention“, die Leiste (1200×200) nur die vier Kennzahlen (Core, Window manager, Accounts, Workspaces): **in beiden Klassen gibt es heute keinen App-Katalog.**

**Der einzige heutige „Welcome“-Bildschirm** ist die Einrichtung der Suite (nicht je App):

| Einrichtung · Start | Einrichtung · fertig |
|---|---|
| ${img('ist/setup-welcome-dark-1600x1000.png', 'Ist: Einrichtung Start', 440)} | ${img('ist/setup-done-dark-1600x1000.png', 'Ist: Einrichtung fertig', 440)} |

**Befunde (Ist):**

| # | Befund | Folge |
|---|---|---|
| I1 | Alle 8 Karten sehen gleich aus; nur Name und Icon (16 px in einer 36-px-Kachel) unterscheiden sie. | Wiedererkennbarkeit gering, die Seite wirkt wie eine Tabelle. |
| I2 | Die Karte trägt 9 Informationen (Icon, Name, Frage, Pille, Satz, Version, Klasse, Knopf, „…“). | Dicht; die Klasse (\`torro-calendar\`) ist für Fensterregeln, nicht für die Wahl. |
| I3 | Breit: 3 Spalten, 5 installierte Karten → 3 + 2, die zweite Reihe hat ein Loch. | Wirkt unfertig, ändert sich mit jeder Zahl. |
| I4 | Mittel und Schmal: eine Spalte; jede Karte ist rund 220 px hoch, 8 Apps = rund 1.900 px Scroll (gemessen im Ist-Screenshot, Mittel). | Der Katalog ist nur zum Scrollen. |
| I5 | Einzelne Apps haben keinen eigenen Start-Bildschirm; „erster Start ohne Konto“ (#429) verbindet direkt. | Kein Ort für Motiv, Frage und erste Schritte. |
| I6 | Geplante Apps (Notizen, Fotos, …) sind nirgends sichtbar; #411 hat „Coming soon“-Karten ausdrücklich verworfen. | Entscheidungspunkt 3. |

## 2. Inventar aller Apps

Quelle: \`torro_core::AppId::ALL\` (8 Katalogeinträge, die Hauptanwendung \`WORKPLACE\` ist kein Katalogeintrag), \`docs/app-catalog.md\`, \`docs/notizen.md\`,
\`docs/fotos.md\`, Issues #348/#367. Farben: Vorschlag (Dunkel / Hell). „Kontrast“ ist Icon auf eigener Tönung, Dunkel / Hell, Soll ≥ 3:1.

| App | Status | Fensterklasse | Frage | Farbe | Kontrast | Quelle |
|---|---|---|---|---|---|---|
${inventory}

**Ideen und fremde Produkte ohne eigenen Entwurf** (nur im Inventar; sie sind weder realisiert noch entschieden):

| Name | Frage / Inhalt | Status |
|---|---|---|
${ideas}

Hinweis: „Mail“ ist im Katalog **Torro Mail** (\`torroinbox --view mail\`), nicht TorroMail. Hub, Mail und App Feed teilen ein Programm (\`torroinbox\`), sind aber drei Katalogkarten.

## 3. Referenzen und Regeln

- **Skill \`torro-design\`**: Linear-Ruhe, Tokens, Radius 4, Haarlinien, Inter, „Rot ist Marke, nie Status“, Farbe nur getönt (\`source_tile\`), Farbe allein trägt nichts, keine Bewegung ohne Arbeit.
- **#803** (Apps in die Übersicht), **#411** (Katalog, Zustände, „Managed by …“, kein „Coming soon“), **#548** (Breite, Auswahl, Reiter; keine Obergrenze für Übersichten), **#528** (Suite-Regeln). Der Vorschlag hält sich daran: gleiche Zustände und Knopf-Regeln, 1 Primärknopf je Fenster, Seite füllt die Breite.
- **Adobe Creative Suite** als Qualitäts- und Konsistenzreferenz, nicht als Vorlage: *eine* Kartenform für alle Apps, ein *eigenes* Motiv und eine *eigene* Farbe je App, große Kachel am Bildrand, ruhige Zustände. Es wurden keine Adobe-Assets, -Logos oder -Illustrationen verwendet oder nachgezeichnet.

## 4. Gemeinsames Kartensystem

Drei Formen derselben Karte (Daten und Zustände sind gleich, nur Bildfläche und Dichte ändern sich):

| Form | Wofür | Bildfläche |
|---|---|---|
| **Medien-Karte** (A) | Breit, wenige Apps, Editorial | 2:1 |
| **Kachel-Karte** (B) | Standard im Raster (4/2/1 Spalten) | 150 hoch, Kachel 44 überlappt |
| **Zeile** (C, Mini) | Liste, Mini, Master-Detail | keine |

${img('system/anatomy-dark.png', 'Anatomie der Karte', 900)}

Zustände (alle mit Wort oder Icon, nie nur Farbe): Available, Hover, Fokus, Installing (mit Fortschritt), Update available, Installed, Managed by …, Turned off, Not available here, Install failed, dazu *Planned* nur in der Studie.

${img('system/states-dark.png', 'Zustände dunkel', 900)}

${img('system/states-light.png', 'Zustände hell', 900)}

**Raster und Größen** (Größenklassen nach \`docs/groessenklassen.md\`): Breit 4 Spalten (Karte ≥ 300), Mittel 2 Spalten, Schmal 1 Spalte mit niedrigerer Bildfläche (120),
ab 360 Breite die Zeile ohne Bild; Mini und Leiste bleiben Anzeigen und zeigen den Katalog nicht (wie heute). Abstände 16 (\`panel_padding\`), Karten-Abstand 16 (12 in Dense), Name \`body_strong\`, Frage \`meta\` in \`TEXT_3\`.

**Hover/Fokus:** Rand \`LINE_SELECTED\` und weicher Schatten, Fokus \`FOCUS_RING\` 2 px mit 1 px Abstand; keine Bewegung der Bildfläche (Dauerbetrieb, ruhig).

## 5. Bildsprache und Motive

Jedes Motiv besteht aus vier Ebenen; nur Ebene 4 und die Farbe unterscheiden die Apps. Das hält die Galerie zusammen.

${img('system/motif-layers-calendar-dark.png', 'Aufbau eines Motivs', 900)}

| App | Motiv (Idee) |
|---|---|
| Calendar | Wochenraster, Termine als Farbbalken, Jetzt-Linie und Heute-Kreis (rot, wie im Produkt), Karte „Up next“ |
| Tasks | Zeilen mit Kreis, starke Fokus-Karte mit Ring und Fortschritt, Fokus-Plätze, Label-Chips |
| Mail | Liste mit Absender-Kreisen, Leser mit Kategorie-Pille, schwebende Aktionsleiste mit rotem „Reply“ |
| Notification Hub | Gestapelte Entscheidungskarten: Freigabe, wartender Agent, Konto-Problem |
| App Feed | Ruhiger Strom mit Quellen-Kacheln (GitHub, Nextcloud, Talk, ntfy), Aktionsleiste |
| Assistant | Gespräch mit Werkzeug-Karte, Eingabefeld mit Mikrofon, Funken |
| Chat | Kontaktliste mit Status, Unterhaltung mit Bild, Anruf-Karte |
| Contacts | Gruppierte Liste, Kontaktkarte, Karte ohne Schlüssel mit Pin, Geburtstag |
| Control centre | Kachel-Aufteilung mit den Apps als Farbleiste, Tray-Menü mit Workspace-Vorlagen |
| Notes · Photos · Frame · Agents · GitHub · Pinboard | geplant; gleiche Ebenen, eigene Szene (Markdown-Editor, Foto-Zeitleiste, Bild mit Passepartout, Sitzungen mit Verbrauch, PR-Zeilen mit Prüfungen, Zettel auf Raster) |

Die Fotos im Motiv von Photos und Frame sind **abstrakte Verläufe mit Bergen und Sonne**, keine Bilder.

## 6. Drei Gesamtlayouts

Alle drei sind die **Übersicht der Steuerzentrale** (Seitenleiste, obere Zeile, Titel wie heute), nur der Abschnitt „Apps“ ist neu. Die 8 Katalog-Apps stehen
in „Installed“ und „Available“ wie in #411; B und C zeigen unten zusätzlich *geplante* Apps, damit man sieht, dass das System mit 15 Apps trägt (Entscheidungspunkt 3).

### A · Editorial

Spotlight für die App mit Update, darunter große Medien-Karten (3 Spalten, Bild 2:1).

${img('layouts/a-editorial-dark.png', 'Layout A dunkel', 900)}

${img('layouts/a-editorial-light.png', 'Layout A hell', 900)}

### B · Creative Suite

Gleichmäßiges Raster aus Kachel-Karten (4 Spalten), Filter-Chips (All · Plan · Inbox · People · AI), überlappende Appkachel wie bei den bekannten Creative-Launchern, geplante Apps gedämpft und gestrichelt.

${img('layouts/b-creative-suite-dark.png', 'Layout B dunkel', 900)}

${img('layouts/b-creative-suite-light.png', 'Layout B hell', 900)}

**B mit Detail-Paneel** (Klick auf eine Karte öffnet das 400-px-Paneel, siehe \`torro_gui::panel\`):

${img('layouts/b-detail-panel-dark.png', 'Layout B mit Detail dunkel', 900)}

**B in Mittel (800, Leiste), Schmal (480, Menü) und kleinstem Schmal (360, Zeilen ohne Bild).** Mini (< 360) und Leiste bleiben wie heute *Anzeigen ohne Katalog* (Ist: „2 need attention“ bzw. vier Kennzahlen); der Katalog gehört nicht dorthin.

| Mittel hell | Schmal dunkel | Schmal 360 dunkel |
|---|---|---|
| ${img('layouts/b-medium-light.png', 'B Mittel', 300)} | ${img('layouts/b-narrow-dark.png', 'B Schmal', 240)} | ${img('layouts/b-narrow-360-dark.png', 'B Schmal 360', 240)} |

### C · Product Preview

Master-Detail: links eine ruhige Liste (Zeilen ohne Bild), rechts eine große Vorschau mit Fensterrahmen, Beschreibung, Knöpfen und Fakten.

${img('layouts/c-product-preview-dark.png', 'Layout C dunkel', 900)}

${img('layouts/c-product-preview-light.png', 'Layout C hell', 900)}

### Vergleich

Zahlen am Mockup gemessen (1600 breit, nur die 8 Katalog-Apps): A ≈ 1.800 px Gesamthöhe, B ≈ 1.220 px, C = feste Höhe, Liste scrollt.

| Kriterium | A Editorial | B Creative Suite | C Product Preview |
|---|---|---|---|
| Apps auf dem ersten Bildschirm (1600×1000) | Spotlight + 3 | 4 (+ Anfang Reihe 2) | alle 8 in der Liste, 1 mit Vorschau |
| Höhe für 8 Apps (breit) | ≈ 1.800 | ≈ 1.220 | fest |
| Wirkung | Magazin, großzügig, viel Motiv | Launcher, dicht, gleichmäßig | Werkzeug, ruhig, Motiv nur im Detail |
| Wiedererkennbarkeit je App | hoch (großes Bild) | hoch (Bild + Kachel) | nur im Detail |
| Skaliert auf 15+ Apps | schlecht (Scroll) | gut (Filter, 4 Spalten) | gut (Liste) |
| Mittel / Schmal (bis 360) | 2 / 1 Spalte, hoch | 2 / 1 Spalte, Bild niedriger, ab 360 Zeile | Liste, Detail ersetzt Liste |
| Installieren/Aktualisieren | auf der Karte | auf der Karte, Details im Paneel | im Detail, ein Klick mehr |
| Aufwand in Iced | mittel (große SVG) | mittel (kleine SVG, ein Baustein) | höher (Master-Detail, Vorschau) |
| Risiko „überladen“ | hoch (8 große Bilder) | mittel (Bildfläche 150) | niedrig |
| Nachteil | zu viel Fläche für einen Katalog | Bildfläche ist Dekoration, kostet Höhe | Entdecken braucht Klicks |

## 7. Welcome-Screens und Hero-Header

Eine Vorlage für alle Apps: Hero (300 hoch) mit Kachel, Überschrift, einem Satz und **genau einem** Primärknopf, rechts das App-Motiv, darunter „Get started“ (3 Aktionen,
die erste ist die des Primärknopfs), „What will appear here“ (gedämpftes Beispiel, mit „Example“ gekennzeichnet) und zwei Tipps. Der Dienst-Hinweis unten sagt, dass nichts den Rechner verlässt, bis ein Konto verbunden ist.

${img('welcome/calendar-dark.png', 'Welcome Calendar dunkel', 900)}

${img('welcome/mail-light.png', 'Welcome Mail hell', 900)}

**Hero-Varianten** (dieselbe Bildsprache, drei Größen): **L** erster Start und leere Zustände, **M** „Was ist neu“ (schließbar), **S** jeder normale Start (nur Kachel in der Kopfzeile).

${img('system/hero-variants-calendar-dark.png', 'Hero-Varianten', 900)}

Welcome-Screens aller 15 Apps: Abschnitt 12.

## 8. Prüfung

| Kriterium | Ergebnis |
|---|---|
| **Lesbarkeit** | Text bleibt in den Tokens (\`TEXT\` ≥ 14,6:1 auf dem Motivgrund, gemessen). Auf dem Motiv steht kein Text; Beschriftungen stehen darunter. Frage einzeilig mit „…“, Satz höchstens 2 Zeilen. |
| **Wiedererkennbarkeit** | Jede App hat ein eigenes Silhouetten-Motiv *und* eine eigene Farbe; die vier Eingangs- und Planungs-Apps (Hub, Mail, Calendar, Tasks) lassen sich auch in Graustufen an der Szene unterscheiden. Zwei Paare liegen farblich nah (Chat/Tasks: Gelbgrün/Grün; Frame/Photos: Rosa-Töne); dort trägt die Szene. |
| **Kontrast** | Icon auf eigener Tönung (Dunkel / Hell): min. 3,96 (Control centre) / 3,25 (Tasks, nach Anpassung). Icon auf der Fläche min. 4,87 / 3,94. Vorher: Tasks hell 2,88 / 3,45 (Befund). Zustände immer mit Wort; Farbe nur im Punkt der Pille. Tabelle: \`system/colours-dark.png\`, \`-light.png\`. |
| **Überladung** | Pro Karte: 1 Bild, 1 Kachel, Name, Frage, Satz (2 Zeilen), Pille, 1 Knopf. Version und Fensterklasse wandern aus der Karte ins Detail (heute 9 Elemente, danach 7 inklusive Bild). Das Motiv ist die einzige Zierde; kein Verlauf auf Flächen, kein Schatten außer in den Szenen. |
| **Zustände** | alle 10 aus #411 plus Hover/Fokus, gleich in jeder Form (\`system/states-*.png\`). |
| **Plattform und Größen** | Gleiche Karte auf Linux, macOS, Windows (kein Plattformcode). Bei 360 Breite wird „Torro Notification Hub“ in der Zeile gekürzt (Pille nimmt Platz); Lösung für die Umsetzung: Pille unter den Namen oder nur Punkt. Mini und Leiste zeigen keinen Katalog. In Dense (Skala 2–24) sinkt die Bildfläche auf 120, in Comfort 170 (nicht gerendert, Entscheidungspunkt 6). |
| **Ladeperformance** | Motive als SVG: 3–8 KB, 15 je Erscheinungsbild ≈ 95 KB gesamt; keine Netzzugriffe, kein Bild-Decoding großer Rasterdateien; gezeichnet erst, wenn der Katalog sichtbar ist. Als PNG wären es etwa das Zehnfache. **Nicht gemessen**, siehe Risiko in Abschnitt 11. |
| **Wartbarkeit** | Eine Karte, ein Motiv-Generator (\`src/art.mjs\`, eine Funktion je App), Farben als Tokens. Neue App = ein Eintrag + eine Szene. Gefahr: handgezeichnete SVGs veralten, wenn sich die echte Oberfläche ändert; deshalb zeigen sie nur Grundformen (Raster, Zeilen, Karten), nie Texte. |
| **Barrierefreiheit** | Motiv ist Dekoration (kein Inhalt, \`aria-label\` nur im Mockup); alles Wichtige steht als Text im Widget. Keine Bewegung. Ein Schalter „Dekoration reduzieren“ ist Entscheidungspunkt 8. |

## 9. Empfehlung

**B (Creative Suite) als Standard der Übersicht**, mit zwei Anleihen:

1. **Detail aus C** als Seitenpaneel (\`torro_gui::panel\`, 400 breit): größeres Motiv, Version, Download, Fensterklasse, Rechte, „Release notes“. Karte bleibt kurz, Details sind einen Klick entfernt.
2. **Spotlight aus A** nur als Frage-Karte für „Update available“ der Hauptanwendung (wie \`update_card\` heute), nicht als Dauer-Element.

Gründe: B zeigt die meisten Apps auf einem Bildschirm, trägt 15+ Apps, bleibt ruhig, und das Motiv hilft beim Wiederfinden, ohne die Liste zu dominieren (A) oder zu verstecken (C). Welcome-Screens (Abschnitt 7) nutzen dasselbe Motiv groß und machen den Mehraufwand zweimal nützlich.

**Nicht empfohlen:** Motive als Rasterfotos (schwer, veraltet), ein Verlauf als Kartenfläche (bricht „Farbe nur getönt“), Animation (Dauerbetrieb).

## 10. Entscheidungspunkte

| # | Frage | Empfehlung |
|---|---|---|
| 1 | **Appfarbe je App** erweitert die Palette um Farben (heute 5 Kalenderfarben + 3 Eingangs-Farben). Nur getönt, nie als Fläche. | Ja; Werte siehe Abschnitt 2, Tasks hell \`#1A9262\`. Alternative: nur Icon neutral, Farbe nur im Motiv. |
| 2 | **Bildfläche im Katalog** überhaupt, und welches Layout? | B + Detail-Paneel (A und C verworfen). Alternative: Status quo, nur Welcome-Screens. |
| 3 | **Geplante Apps zeigen?** #411 hat „Coming soon“ verworfen. | Nicht im Produkt. Nur dieses Dokument und die Roadmap. |
| 4 | **Motiv-Assets:** einmal erzeugte SVGs im Repo, SVGs zur Bauzeit erzeugt, oder Raster? | SVGs im Repo (\`assets/app-art/\`), Generator bleibt in \`docs/prototypes\`. |
| 5 | **Welcome je App:** Hero L beim ersten Start einer App ohne Konto (#429), oder nur die Suite-Einrichtung? | Hero L je App, die Suite-Einrichtung bleibt. Primärknopf rot, genau einer. |
| 6 | **Abstandsstufen:** Bildfläche Dense 120 / Standard 150 / Comfort 170? | Ja, mit der Skala der Stufe. |
| 7 | **Hauptanwendung als Karte?** Heute kein Katalogeintrag. | Nein, nur die Update-Frage-Karte; das Motiv dient dem Welcome. |
| 8 | **Schalter „Decoration“** (Appearance › Display) für Karten ohne Bild. | Ja, Standard an; hilft Kontrast, Last und Geschmack. |
| 9 | **TorroMail, TorroCast, TorroEQ** (eigene Produkte) im Katalog? Brauchen eigene Motive. | Erst nach Entscheidung #354. Photos und Frame als *eine* Karte? Pinboard ist zurückgestellt. |

## 11. Umsetzungsskizze

**Keine Umsetzung vor Freigabe.** Vorschlag für Issues in dieser Reihenfolge:

1. **Tokens:** \`Palette::app_hue(AppId)\` (Dunkel/Hell, Farbschema darf überschreiben) und \`Palette::app_tint\`, erweitert \`shell::app_hue\` auf alle Apps. Test über *alle* Appfarben: ≥ 3:1 auf Tönung und Fläche in jedem eingebauten Farbschema (wie der bestehende Kontrasttest der Kategorie-Pille).
2. **Assets:** \`assets/app-art/<fensterklasse>-{dark,light}.svg\` aus \`art/*.svg\` (Text in Pfade umwandeln oder weglassen: resvg und Systemschrift). Geladen wie die Icons (\`torro_gui::icon\`), mit Test auf Größe (≤ 12 KB je Datei) und auf vorhandene Datei je \`AppId::ALL\`.
3. **Baustein** \`torro_gui::widgets::app_card(AppCard, Form, Zustand)\`: \`Form::Media | Tile | Row\`, nutzt \`widgets::card\`, \`source_tile\` (neue \`TileSize::Xl\` = 44), \`status_pill\`. Anschauen in \`gallery --cards\`.
4. **Übersicht:** \`ui/apps.rs\` ersetzt \`card()\` durch den Baustein; Raster mit Breite (4/2/1), Filter-Chips (Segmente nach \`widgets::choice_chip\`), Paneel für Detail (\`Panel\`), Knöpfe und Zustände unverändert (\`Offer\`). Texte: nur Chips und „Details“ neu (\`tr()\`, \`de.rs\`).
5. **Start-Screen:** \`torro_gui::welcome::Welcome { app, headline, sub, actions, peek, tips }\` mit Hero L/M/S; jede App liefert nur Texte (\`tr()\`). Auslöser: erster Start ohne Konto (#429) und leere Ansichten.
6. **Sichtprüfung** gezielt mit \`shots.sh\` (Übersicht, \`setup-*\`, Welcome je App; Breit/Mittel/Schmal, hell/dunkel, Dense einmal).

**Drei Wege (CLAUDE.md):** keine neue Funktion, daher **kein neuer Request, kein Katalogeintrag, kein MCP-Tool**. Der Katalog bleibt \`apps.list_catalog\`/\`apps.install_app\`; Motiv und Farbe sind reine Oberfläche. Wird später eine App mit Motiv *vom Dienst* geliefert (Erweiterungen), braucht das ein eigenes Issue mit API.

**Risiken:** (a) SVG-Wiedergabe in Iced/resvg (Filter \`feDropShadow\`, Radialverläufe, Text) nicht geprüft; Rückfall: Schatten und Text aus den Motiven entfernen. (b) 15 Szenen von Hand zu pflegen; mitigiert durch die Beschränkung auf Grundformen. (c) Rot in Motiven (Heute, Jetzt, Reply) folgt dem Produkt, kann aber „Rot nur Primärknopf“ widersprechen; Entscheidung im Design-Review.

## 12. Galerie je App

${perApp}

## 13. Neu erzeugen und Dateien

\`\`\`bash
# einmalig: Chromium für Playwright in einem Ordner außerhalb des Repos
mkdir -p /tmp/pw && cd /tmp/pw && npm init -y && npm i playwright@1.49.1 && npx playwright install chromium-headless-shell
# aus dem Repo
PLAYWRIGHT_DIR=/tmp/pw node docs/prototypes/app-gallery/src/build.mjs            # alles (oder: cards layouts system welcome art)
node docs/prototypes/app-gallery/src/readme.mjs                                   # README.md und index.html
\`\`\`

| Ordner | Inhalt |
|---|---|
| \`ist/\` | Echte Screenshots (14 Dateien) |
| \`cards/\` | Karte je App, hell und dunkel (30) |
| \`layouts/\` | A, B, C hell/dunkel; B Detail, Mittel, Schmal, Schmal 360 (14) |
| \`system/\` | Anatomie, Zustände, Farben, Motiv-Ebenen, Hero-Varianten (10) |
| \`welcome/\` | Start-Screen je App (24) |
| \`art/\` | Motive als SVG (30, zusammen ${Math.round(readdirSync(join(OUT, 'art')).reduce((n, f) => n + statSync(join(OUT, 'art', f)).size, 0) / 1024)} KB) |
| \`src/\` | Generator: \`kit\` (Tokens, Zeichner), \`apps\` (Inventar), \`art\` (Motive), \`ui\`, \`pages\`, \`build\`, \`render\`, \`readme\` |
`;

writeFileSync(join(OUT, BASE ? 'README.issue.md' : 'README.md'), readme);

if (!BASE) {
  const groups = { 'Ist-Zustand': 'ist', Karten: 'cards', Layouts: 'layouts', System: 'system', 'Welcome-Screens': 'welcome' };
  const body = Object.entries(groups).map(([t, d]) => `<h2>${t}</h2><div class="g">${readdirSync(join(OUT, d)).filter((f) => f.endsWith('.png')).map((f) => `<figure><a href="${d}/${f}"><img loading="lazy" src="${d}/${f}"></a><figcaption>${d}/${f}</figcaption></figure>`).join('')}</div>`).join('');
  writeFileSync(join(OUT, 'index.html'), `<!doctype html><html lang="en"><meta charset="utf-8"><title>Torro app gallery study (#970)</title><meta name="viewport" content="width=device-width,initial-scale=1"><style>body{font:14px/22px system-ui,sans-serif;margin:0;padding:24px;background:#f4f5f6;color:#18191b}@media(prefers-color-scheme:dark){body{background:#08090a;color:#edeef0}}h1{font-size:28px}h2{margin:32px 0 12px;font-size:20px}.g{display:grid;grid-template-columns:repeat(auto-fill,minmax(320px,1fr));gap:16px}figure{margin:0}img{width:100%;height:auto;border:1px solid rgba(128,128,128,.25);border-radius:4px;display:block}figcaption{font-size:12px;opacity:.7;margin-top:4px;word-break:break-all}</style><h1>Torro app gallery study · issue #970</h1><p>Mockups (HTML/SVG rendered to PNG) and real screenshots (<code>ist/</code>). Text and decisions: <a href="README.md">README.md</a>.</p>${body}</html>`);
}
console.log('written', kb(BASE ? 'README.issue.md' : 'README.md'), 'KB');
