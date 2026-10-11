---
name: torro-api
description: Regeln für API und MCP in TorroWorkplace – jede Funktion gibt es in Oberfläche, API (torro_ipc::Request + Befehlskatalog) und MCP (torroworkplace mcp). Laden, wenn ein Request, der Katalog, ein MCP-Tool, Rechte, Schlüssel oder Freigaben angefasst werden, und beim Planen jeder neuen Funktion.
---

# API und MCP

Konzept: `docs/api.md`. Kurz: Die Logik steht nur im Dienst. Oberfläche, API und MCP sind drei
Türen zu denselben Befehlen. Geprüft (Schlüssel, Rechte, Freigabe) wird nur im Dienst.

## Eine neue Funktion planen

Unter jede Funktion in `docs/tasks/` gehört:

```
- **Oberfläche:** <Seite/Ansicht>
- **API:** `Request::<Fall>` · `<bereich>.<befehl>` · Zugriff <Read|Write|Delete|Admin>
- **MCP:** `<bereich>_<befehl>` (oder: Never – Grund aus „Was nie nach außen geht“)
```

Request und Katalogeintrag gehören in die **erste** Aufgabe der Funktion (Gerüst), damit
Oberfläche und Tool danach parallel entstehen können.

## Einen Befehl anlegen

1. **Request-Fall** in `crates/torro-ipc/src/lib.rs`, Doc-Kommentar: was er tut, was er
   zurückgibt, welche Fehlercodes vorkommen.
2. **Klein und gezielt**: ein Befehl, eine Sache. Kein „alles ersetzen“, wenn zwei Clients
   gleichzeitig ändern könnten; lieber `set { key, value }` oder ein Objekt mit Kennung.
3. **Katalogeintrag** in `crates/torro-ipc/src/api.rs`:
   - Name `bereich.verb_objekt` in snake_case (`calendar.delete_event`, `workspaces.load_template`);
     das MCP-Tool heißt gleich mit `_` statt `.`.
   - Bereich: der Teil der Suite, für den ein Mensch Rechte vergibt.
   - Zugriff: `Read` ändert nichts · `Write` legt an, ändert, löst etwas aus · `Delete` entfernt
     oder überschreibt unwiederbringlich · `Admin` Konten, Einstellungen, Verbindungen.
     Im Zweifel die höhere Stufe.
   - Beschreibung englisch, ein bis drei Sätze: was, wann benutzen, Nebenwirkungen
     („Deletes the event on the server; it cannot be restored.“). Sie ist die Tool-Beschreibung
     für Agenten, also konkret und ohne Marketing. Kein `tr()`: Agenten lesen Englisch.
4. **Handler** im Kern (`crates/torroworkplace/src/core.rs` bzw. dem Fachmodul), Fehler mit festen
   Codes: `not-found`, `conflict`, `offline`, `auth`, `busy`, `unsupported`, `not-connected`,
   `workspace-not-empty`, `unauthorized`, `forbidden`, `pending`, `error`. `message` ist für Menschen.
5. **MCP-Tool** in `crates/torroworkplace/src/mcp.rs`: bildet auf genau **einen** Request ab.
   Bequeme Eingaben sind erlaubt (Titel/Beginn/Ende statt eines ganzen `Event`). Kein eigener
   Datenzugriff, keine eigene Rechteprüfung.
6. **Oberfläche** schickt den Request (`Action::Send`), liest und schreibt nichts selbst.

## Sicherheit (nie verhandelbar)

- Geheimnisse (Passwörter, Tokens, Schlüssel) gehen über **keinen** Befehl hinaus, auch nicht an
  den App-Schlüssel. Antworten sagen nur „gespeichert“ / „fehlt“.
- `clients.*` (Agenten, Schlüssel, Rechte) und `approvals.decide` nur mit dem App-Schlüssel,
  `Mcp::Never`. Ein Agent darf sich nie selbst Rechte oder Freigaben geben.
- Sammlungen, die ein Client nicht sehen darf, verhalten sich wie nicht vorhanden (`not-found`);
  gefiltert wird vor dem Lesen.
- Eine Freigabe gibt dem Agenten nie einen Code, mit dem er selbst bestätigen könnte.
- Protokoll ohne Inhalte (keine Titel, Suchbegriffe, Texte), Dateien 0600.
- Schlüssel nur als SHA-256 auf der Platte; Klartext nur im Schlüsselbund und beim Client.

## Tests

- Handler-Test im Kern (Temp-Verzeichnisse, kein Netz, kein Hyprland).
- Rechte: der Tabellen-Test über alle Katalogeinträge deckt den neuen Befehl automatisch ab;
  bei besonderen Regeln (Sammlungen, Freigabe) einen eigenen Fall ergänzen.
- MCP: Vertragstest „Tool ↔ Katalog“ läuft automatisch; für bequeme Eingaben einen Test, dass
  das Tool den richtigen Request baut.
- API-Referenz neu erzeugen: `TORRO_BLESS=1 cargo test -p torro-ipc` und `docs/api-reference.md`
  mit committen.

## Versionen

Neue Befehle und neue optionale Felder sind verträglich. Umbenennen, Entfernen, neue
Pflichtfelder brechen: nicht ohne Rückfrage, dann `v: 2`.
