# Erweiterungen von Drittanbietern

Stand: 2026-10-13, Issue #79. **Diskussionsgrundlage, nichts entschieden.** Frage: An welchen Stellen lässt sich
die Suite durch Komponenten erweitern, die nicht mit ihr ausgeliefert werden, und wie, ohne dass
Sicherheit, Stabilität und die Regel „jede Funktion dreimal“ ([api.md](api.md)) leiden?

## Grundsätze (Vorschlag)

1. **Außerhalb des Prozesses.** Erweiterungen laufen nie im Dienst. Kein Laden fremder Bibliotheken
   (`dlopen`): Rust hat keine stabile ABI, der Workspace verbietet `unsafe`, und ein Absturz der
   Erweiterung darf den Dienst nicht mitnehmen. Erweiterungen sind **Daten** (wie Design-Pakete)
   oder **eigene Programme**, die über die API sprechen.
2. **Ein Client wie jeder andere.** Eine Erweiterung mit Code bekommt einen eigenen Schlüssel und
   Rechte je Bereich, angelegt beim Installieren in der Steuerzentrale. Sie sieht nur, was die
   Person ihr gibt; Geheimnisse und „Was nie nach außen geht“ gelten unverändert.
3. **Manifest.** Jede Erweiterung beschreibt sich in einer Datei (`extension.toml`: Name, Version,
   Herausgeber, Art, benötigte Bereiche, Programm). Die Steuerzentrale zeigt vor dem Installieren,
   was sie verlangt.
4. **Was sie bringt, gibt es dreimal.** Bringt eine Erweiterung neue Fähigkeiten, erscheinen sie im
   Katalog (Bereich der Erweiterung), damit Oberfläche, API, MCP und der Assistent
   ([assistent.md](assistent.md)) sie gleich nutzen können.
5. **Abschaltbar und entfernbar** ohne Reste: Ordner unter `<data>/extensions/<id>/`, Entfernen nimmt
   Schlüssel, Rechte und Katalogeinträge mit.
6. **Open First.** Format und Protokolle offen dokumentiert; keine Pflicht zu einem Marktplatz.

## Mögliche Erweiterungspunkte

Sortiert nach Aufwand und Risiko, von „gibt es schon fast“ bis „großer Umbau“.

| # | Punkt | Was eine Erweiterung tut | Wie | Stand heute |
|---|---|---|---|---|
| 1 | **API/MCP-Clients** | ein fremdes Programm liest und schreibt Daten der Suite | Schlüssel + Rechte, Socket bzw. später HTTP | **gibt es** (Agents & API, Script) |
| 2 | **Quellen des Eingangs** | meldet Einträge (Monitoring, CI, Messenger, Home Assistant) | `inbox.post_item` mit eigenem Client | **gibt es** für Skripte; fehlt: Manifest, Symbol, eigene Quelle in der Seitenleiste |
| 3 | **Design-Pakete** | Farben, Schriften, Logo | nur Daten, geprüft | **gibt es** (L.1–L.5) |
| 4 | **Werkzeuge für den Assistenten** | zusätzliche MCP-Server im Chat (z. B. Home Assistant, Notion, eigene Skripte) | MCP-Server der Erweiterung, eingehängt im Antrieb des Assistenten; Rechte je Server: an/aus, Freigabe je Aufruf | neu, eng verbunden mit dem Assistenten |
| 5 | **Skills / Wissen für den Assistenten** | Text-Bausteine: wie man mit einem Fachgebiet umgeht | nur Daten (Markdown), werden dem Prompt hinzugefügt | neu, einfach |
| 6 | **Apps** | eigene Fenster-App in der Suite (z. B. Zeiterfassung) | eigenes Programm mit Manifest, erscheint in `apps.list_apps`, Fensterklasse für Workspace-Vorlagen, optional `torro-gui` für gleiches Aussehen | `AppId` heute fest im Code |
| 7 | **Datenquellen / Konten** | neuer Anbieter für Kalender, Aufgaben, Mail (Todoist, Google, Exchange) | Provider als eigener Prozess mit einem kleinen Protokoll (JSON über stdio, ähnlich MCP), der Kern bleibt Cache und Rechteprüfung | Provider heute im Code (`torro-pim-*`) |
| 8 | **Fenster-Manager** | Anbindung an weitere Tiling-Manager | Adapter als Prozess, Protokoll aus dem `torro-wm`-Trait abgeleitet | im Code (Hyprland, geplant AeroSpace, GlazeWM) |
| 9 | **KI-Anbieter** | weitere Modellanbieter | meist reicht ein Katalogeintrag (OpenAI-kompatibel); sonst wie 7 | geplant ([ki.md](ki.md)) |
| 10 | **Aktionen und Automatisierung** | „wenn X im Eingang, dann Y“; Knöpfe in Einträgen | Regeln rufen Befehle des Katalogs oder Programme einer Erweiterung | Regeln des Eingangs gibt es, ohne fremde Aktionen |
| 11 | **Leisten-Widgets** | Omarchy/Waybar, macOS-Menüleiste | `status --json` + eigene Darstellung | geplant (Phase 5) |

## Was man nicht erweiterbar machen sollte (Vorschlag)

- Rechte, Schlüssel, Freigaben und Protokoll: bleiben ganz im Kern.
- Eingriffe in fremde Oberflächen (eigene Knöpfe in der Kalender-App): erst, wenn es feste
  Andockstellen gibt; sonst bricht jede Änderung der Oberfläche Erweiterungen.
- Layout-Sprachen und Code in Design-Paketen (siehe [design-pakete.md](design-pakete.md)).

## Verteilung und Vertrauen

- **Stufe 1:** Ordner von Hand nach `<data>/extensions/` kopieren, Steuerzentrale zeigt Manifest und
  verlangte Rechte, Person bestätigt.
- **Stufe 2:** Installieren aus einer Adresse (Git-Repository, Archiv) mit Prüfsumme.
- **Stufe 3:** Verzeichnis / Katalog, Signaturen der Herausgeber, Updates.
- Grenze wie in [api.md](api.md) ehrlich festhalten: Eine Erweiterung mit eigenem Programm läuft als
  derselbe Benutzer; Rechte schützen vor einer Erweiterung, die sich an die API hält, nicht vor
  bösartigem Code. Darum: Hinweis beim Installieren, Datenerweiterungen (Designs, Skills) bevorzugen.

## Fragen zur Diskussion

1. Welche Punkte zuerst? Vorschlag: 4 und 5 (Assistent), weil sie dem Assistenten sofort mehr
   Fähigkeiten geben, dann 2 (Eingang) und 6 (Apps).
2. Brauchen Datenquellen (7) wirklich Fremdanbieter, oder bauen wir die wichtigen selbst?
3. Ein eigenes Provider-Protokoll für 7 und 8, oder gleich MCP als Protokoll für alles (MCP-Server
   der Erweiterung, der Kern als MCP-Client)?
4. Ab wann Stufe 2 und 3 der Verteilung?
5. Lizenzfragen: Dürfen Erweiterungen geschlossen sein? Open First spricht für offene, verbietet
   geschlossene aber nicht zwingend.
