# Arbeitsweise und Akzeptanzprüfungen

Stand: 2026-11-21, Entscheidung von Sven. Regeln für die Arbeit: [CLAUDE.md](../../CLAUDE.md), Issue-Workflow:
[issue-workflow.md](../issue-workflow.md).

Wie alles unter `docs/tasks/` ist diese Datei eine Bestandsaufnahme. Punkt 3 ist ein offener Planungspunkt; daraus
entsteht bei Bedarf ein Issue, und dann gilt das Issue mit seinem Plan.

## 1. Umsetzen statt lange planen

Umsetzen, umsetzen, umsetzen: Issues konsequent abarbeiten und PRs erstellen, statt lange zu planen. Der
Issue-Workflow bleibt, wie er ist (Plan, Freigabe durch Menschen, Umsetzung, PR); gemeint ist das Tempo, nicht ein
Weg an den Freigaben vorbei.

## 2. Prüfungen von Hand gesammelt später

Die offenen Issues, die reine Prüfungen von Hand sind (Label `manual-check`, Titel „Prüfungen von Hand: …“, und
gleichartige Prüflisten), arbeitet Sven später gesammelt in einem Rutsch ab, wenn er Zeit hat. Am 2026-11-21 sind es
rund 78 offene Issues (davon 75 mit dem Label `manual-check`).

- Sie **blockieren die weitere Umsetzung nicht.** Ein neues Issue wartet nicht darauf, dass eine Prüfung von Hand
  eines früheren PRs erledigt ist.

## 3. Offen: Sicht- und Akzeptanzprüfungen automatisieren

Noch keine Lösung festgelegt. Zu erarbeiten ist, wie Sicht- und Akzeptanzprüfungen künftig **intern automatisiert**
laufen, **unabhängig von einem bestimmten Bot oder Agenten**. Sie sollen die an der Software gemachten Änderungen
überprüfen: in der Oberfläche und dort, wo man eine Änderung im Backend sieht (Dienst, Requests, gespeicherte Daten,
Antworten an Server).

### Was es im Repo schon gibt (Anhaltspunkte)

- **Bilder ohne Fenster:** `.claude/skills/torro-design/shots.sh` baut eine App und zeichnet ihre Ansicht mit
  `--demo --open <zustand> --shots <ordner>` in den sechs Größenklassen als Bilder, ohne Fenster und ohne Hyprland
  (`torro_gui::shots`, #492). Die Zustände kommen aus Q.1 in [design.md](design.md). Bewertet werden die Bilder heute
  vom Agenten `torro-design-review` gegen die Regeln des Skills `torro-design`.
- **Lokale CI:** `scripts/ci-local.sh` mit Stufen nach Umfang (`--quick`, `--changed`, `--scope`, voll) und
  `scripts/design-check.sh` für rohe Design-Werte im Code; die Hooks in `.githooks/` rufen sie auf.
- **Testserver in Tests:** `httpmock::MockServer` (z. B. `crates/torro-nextcloud/src/ocs.rs`), eigene Server auf
  `TcpListener` für IMAP und SMTP (`crates/torro-mail-imap/tests/support/`, `tests/imap_support/`), dazu CalDAV,
  CardDAV, ntfy und Fenstermanager-Sockets. Testdaten unter `tests/fixtures/` (z. B. Audio für den Assistenten in
  `crates/torroassistant/tests/fixtures/`). Tests brauchen kein Netz, kein Hyprland, kein echtes Home.
- **Demo-Daten:** `--demo` in allen Apps (Skill `torro-design`).
- **API und MCP:** Jede Funktion der Oberfläche ist ein `torro_ipc::Request` mit Eintrag im Befehlskatalog
  ([api.md](../api.md)). Was im Backend sichtbar ist, lässt sich also über denselben Weg abfragen wie aus der
  Oberfläche.
- **Struktur der `manual-check`-Issues:** Abschnitt „Herkunft“ (Issue und PR), dann „Prüfungen“ als Checkliste; jede
  Prüfung hat eine Zeile mit Quelle (meist „Test plan“ des PRs) und darunter **Was**, **Wo**, **Wie** (nummerierte
  Schritte) und **Erwartung** („In Ordnung, wenn … Befund, wenn …“). Viele Punkte brauchen echte Geräte, Konten,
  Mikrofon oder Netz, andere nur eine Ansicht in hell und dunkel.

### Offene Fragen

- Welche Prüfungen aus den `manual-check`-Issues lassen sich überhaupt automatisieren, und welche bleiben von Hand
  (Stimme, Mikrofon, echte Konten, Netz trennen, macOS-Eigenheiten)? Lohnt eine Einteilung der vorhandenen Issues?
- Woraus entsteht eine Akzeptanzprüfung: aus dem Test plan des PRs, aus dem Plan des Issues, aus einer eigenen
  Beschreibung im Repo? In welcher Form wird sie festgehalten, damit sie unabhängig vom Agenten läuft?
- Oberfläche: Reicht der Weg über `shots.sh` mit Zuständen aus `--open`, oder braucht es Abläufe mit Klicks und
  Eingaben? Wie wird ein Bild bewertet: Vergleich mit einem Referenzbild, Regeln, ein Modell, ein Mensch?
- Backend: Prüft man über Requests an den Dienst (API), über gespeicherte Daten, über das Log oder über das, was ein
  Testserver empfangen hat? Wie bleiben Geheimnisse dabei im Dienst?
- Wo und wann laufen die Prüfungen: lokal in `scripts/ci-local.sh` als neue Stufe, vor dem PR, nach dem Merge, vor
  einem Release? GitHub Actions kostet Geld und läuft nur bei Releases.
- Was passiert mit einem Befund: Blocker im PR, neues Issue, Eintrag im bestehenden `manual-check`-Issue?
- Welche Plattformen werden geprüft (Linux mit Hyprland, macOS, Windows), und was davon ohne laufenden Fenstermanager?
- Ersetzen automatisierte Prüfungen künftig einen Teil der `manual-check`-Issues, oder entstehen diese weiter und
  werden nur kürzer?
