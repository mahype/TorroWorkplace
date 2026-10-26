# Erweiterungen (Plugins)

Stand: 2026-10-24, Issue #79; MCP-Server sind mit Issue #302 gebaut. **Entscheidungsentwurf** bleibt der
Rest: Die Vorschläge unten sind ausgearbeitet, entschieden ist für MCP-Server, was im Abschnitt
„MCP-Server (#302)“ und unter „Entscheidungen“ steht; die übrigen Fragen bleiben offen.

Frage: An welchen Stellen lässt sich die Suite durch Komponenten erweitern, die nicht mit ihr
ausgeliefert werden, und wie, ohne dass Sicherheit, Stabilität und die Regel „jede Funktion
dreimal“ ([api.md](api.md)) leiden? Und wie bleibt die Suite dabei mit jedem **Standard-Nextcloud**
verträglich?

## Abgrenzung: Basis und Erweiterung

- **Die Basis bildet ab, was ein Standard-Nextcloud kann** (Kern, mitgelieferte Apps und die
  verbreiteten Apps aus dem App Store wie Notes, Deck, Tables, Talk), jede Funktion erkannt über die
  Capabilities des Servers. Sie verlangt auf dem Server **nichts**: keine eigene Server-App, keinen
  Admin, keine Einstellung.
- **Alles darüber hinaus ist eine Erweiterung.** Auch eine Erweiterung verlangt auf dem Server
  nichts, was ein Standard-Nextcloud nicht hat. Braucht sie eine optionale Nextcloud-App, nennt sie
  das im Manifest (siehe unten), und die Suite schaltet sie ohne diese App ab.
- **Maßstab:** die aktuelle Nextcloud-Version und die zwei davor. Am 2026-10-15 ist das Nextcloud 35
  (Hub 26 Summer, 35.0.0 vom 16.09.2026), also 33 bis 35.

## Grundsätze

1. **Außerhalb des Prozesses.** Erweiterungen laufen nie im Dienst. Kein Laden fremder Bibliotheken
   (`dlopen`): Rust hat keine stabile ABI, der Workspace verbietet `unsafe`, und ein Absturz der
   Erweiterung darf den Dienst nicht mitnehmen. Erweiterungen sind **Daten** (wie Design-Pakete)
   oder **eigene Programme**, die über die API sprechen.
2. **Ein Client wie jeder andere.** Eine Erweiterung mit Code bekommt einen eigenen Schlüssel und
   Rechte je Bereich, angelegt beim Installieren in der Steuerzentrale. Sie sieht nur, was die
   Person ihr gibt; Geheimnisse und „Was nie nach außen geht“ gelten unverändert.
3. **Manifest.** Jede Erweiterung beschreibt sich in einer Datei (`extension.toml`). Die
   Steuerzentrale zeigt vor dem Installieren, was sie verlangt.
4. **Was sie bringt, gibt es dreimal.** Bringt eine Erweiterung neue Fähigkeiten, erscheinen sie im
   Katalog (Bereich der Erweiterung), damit Oberfläche, API, MCP und der Assistent
   ([assistent.md](assistent.md)) sie gleich nutzen können.
5. **Der Dienst ist das einzige Tor nach außen.** Erweiterungen sprechen nie selbst mit Nextcloud
   oder einem anderen Konto der Suite, sondern über Befehle des Dienstes.
6. **Sie sieht aus wie die Suite.** Wo es geht, liefert eine Erweiterung Daten, und die Suite
   zeichnet sie im Torro-Design (siehe „Nahtlos einbinden“).
7. **Abschaltbar und entfernbar** ohne Reste: Ordner unter `<data>/extensions/<id>/`, Entfernen nimmt
   Schlüssel, Rechte und Katalogeinträge mit.
8. **Open First.** Format und Protokolle offen dokumentiert; keine Pflicht zu einem Marktplatz.

## Was Nextcloud bietet

Recherche vom 2026-10-15 (Agent `nextcloud-expert`, Quellcode `nextcloud/server` Zweig `stable35`
und offizielle Doku). Für Erweiterungen zählt, was ein Client **mit dem normalen App-Passwort**
nutzen kann.

### Generisch nutzbar, ohne Server-App

| Schnittstelle | Wo | Nutzen |
|---|---|---|
| **Unified Search** (`/ocs/v2.php/search/providers`, `…/{id}/search`) | Kern | eine Suche über **alle** Server-Apps, einheitliche Treffer (Titel, Unterzeile, Link, Icon) |
| **Dashboard-Einträge v2** (`/ocs/v2.php/apps/dashboard/api/v2/widget-items`, `sinceIds`) | mitgeliefert, an | Daten **beliebiger** Server-Apps als einheitliche Liste, also als Kachel, ohne die App zu kennen |
| **Links auflösen** (`/ocs/v2.php/references/resolve`) | Kern | Vorschau für Links in Eingang, Notizen, Chat; Rückfall Open Graph |
| **Activity** (`/ocs/v2.php/apps/activity/api/v2/activity`, `since`) | mitgeliefert, an | Verlauf aller Apps |
| **TaskProcessing** (`/ocs/v2.php/taskprocessing/…`) | Kern seit Nextcloud 30 | KI-Aufgaben an Nextcloud **übergeben** und Ergebnisse abholen; höchstens 20 Aufträge je 120 s und Nutzer; ohne eingerichteten Anbieter ist die Liste der Aufgabentypen leer |
| Dateien, Papierkorb, Versionen per WebDAV | Kern | Ablage |
| Kalender, Aufgaben, Kontakte per CalDAV/CardDAV | Kern | gibt es in der Basis |
| Kommentare, System-Tags, Freigaben, Benutzerstatus, Projekte (Collaborative Resources) | Kern bzw. mitgeliefert | Verknüpfen, Markieren, Teilen |
| Notes, Deck, Tables, Forms, Talk | App Store bzw. empfohlen | nur nach Capability |

### Nicht ohne Admin oder Server-Code

- **Webhooks** (`webhook_listeners`): nur für Admins, und der Server müsste unseren Rechner erreichen.
- **Flow**, **Talk-Bots** (`occ talk:bot:install`): Admin bzw. Kommandozeile auf dem Server.
- **KI-Anbieter für Nextcloud sein:** Die Endpunkte der Worker sind nur für ExApps (AppAPI). Die
  Suite kann Nextcloud als KI-Anbieter **nutzen**, aber nicht selbst einer werden.
- **AppAPI / ExApps:** mitgeliefert und an, braucht aber einen Admin, einen Deploy-Daemon (Docker)
  und oft eine GPU. Der richtige Weg für eine Erweiterung, die wirklich Server-Code braucht, aber
  nicht mehr „Standard-Nextcloud“ (Frage 4).

### Ereignisse

Push gibt es nur mit `notify_push` (App Store, Admin richtet Redis und einen eigenen Dienst ein;
Capability `notify_push.endpoints.websocket`). Sonst nur Abfragen, je Art mit dem passenden Mittel:

| Daten | Nur Änderungen holen |
|---|---|
| CalDAV/CardDAV | `sync-collection` mit `sync-token` |
| Dateien | ETag des Ordners (PROPFIND Depth 0), bei Änderung absteigen; **kein** `sync-collection` |
| Benachrichtigungen | ETag (gibt es, E5.8) |
| Activity | `since` |
| Dashboard v2 | `sinceIds` |

Daraus folgt: **Der Dienst fragt einmal für alle**, mit `notify_push`, wenn der Server es hat, und
Erweiterungen abonnieren beim Dienst (`subscribe`, siehe unten).

### Ablage für Erweiterungen

| Ort | Vorteil | Nachteil |
|---|---|---|
| **Ordner per WebDAV**, `/.torro/<erweiterung>/` | gibt es immer, geräteübergreifend, `If-Match`, Versionen, Papierkorb | für die Person sichtbar, ihr Desktop-Client synchronisiert mit |
| Eigene DAV-Properties (eigener Namensraum) | kleine Markierungen an Dateien, Kalendern, Adressbüchern | gelten nur für diesen Nutzer |
| X-Properties in iCalendar/vCard | reisen mit Termin oder Kontakt | andere Programme können sie verwerfen (wie bei `X-TORRO-LINK`) |
| Tables | strukturiert, teilbar | optional; vermutlich „wer zuletzt schreibt, gewinnt“ (nicht belegt) |

**Vorschlag:** Ordner per WebDAV als Standard, DAV-Properties für Markierungen. Schreiben tut der
Dienst mit ETag; eine Erweiterung erreicht nur ihren eigenen Unterordner.

### Rechte bei Nextcloud

- Ein App-Passwort kann **fast alles**. Einschränken lässt es sich nur mit dem Schalter
  `filesystem` (an/aus). OAuth2 hat keine Scopes, und mit einem App-Passwort lässt sich kein
  weiteres erzeugen. Ein eigenes App-Passwort je Erweiterung brächte also mehr Geheimnisse, aber
  keine feineren Rechte.
- **Darum sieht keine Erweiterung je das App-Passwort.** Wer es hat, hat das ganze Konto.

## Wie offen der Code heute ist

| Andockstelle | Stand | Folge für Erweiterungen |
|---|---|---|
| Schlüssel, Rechte, Freigaben, Protokoll ([api.md](api.md)) | gibt es | trägt: eine Erweiterung ist ein Client |
| `inbox.post_item`, `tasks.save_todo`, `create_task_from_item`, `calendar.save_event` | gibt es | Aufgaben, Termine und Meldungen übergeben geht schon |
| `torro_ipc::api::Area` (17 feste Bereiche, seit #302 mit `extensions`), `Request::operation()` (vollständiges `match`) | geschlossen | keine eigenen Befehle je Erweiterung; die Werkzeuge eines MCP-Servers laufen über `extensions.call_tool` mit Stufe je Server, nicht als Katalogbefehle |
| `torro_core::app::AppId::ALL` | fest im Code | Erweiterungs-Apps fehlen im Umschalter, in Vorlagen und in `apps.list_apps` |
| Änderungsmeldungen (`subscribe`) | nur geplant ([api.md](api.md#transport)) | jede Erweiterung müsste selbst abfragen |
| Design | `torro-gui` nur für Rust/Iced; die API nennt nur die Pakete (`settings.list_designs`) | Erweiterungen in anderen Sprachen bekommen Farben und Schriften nicht |
| `torro-nextcloud` | Capabilities, Benachrichtigungen, Talk | Suche, Dashboard, Links, Activity, WebDAV-Ablage fehlen |
| Element-Interface des Fenster-Modus ([modes.md](modes.md)) | geplant, nur im Prozess (Rust) | fremde Kacheln nur als Daten möglich |

## Vorschlag: Aufbau

### MCP als Protokoll

Eine Erweiterung mit Code ist ein **MCP-Server** (stdio), der Dienst ist sein **Client**. MCP bringt
Werkzeuge, Ressourcen und Benachrichtigungen mit, es gibt fertige SDKs in jeder Sprache, und der
Assistent spricht es schon. Für Datenquellen und Fenster-Manager (Punkte 7 und 8 unten) gilt
dasselbe Protokoll mit festen Werkzeugnamen statt eines eigenen.

Die Erweiterung ruft umgekehrt die Suite über die API mit ihrem eigenen Schlüssel (wie heute ein
Script-Client). Für MCP-Server aus #302 gilt das nicht: Sie bekommen keinen Schlüssel und rufen Torro nie auf
(Abschnitt „MCP-Server (#302)“).

### Arten

| Art | Was | Risiko |
|---|---|---|
| **Daten** | Design-Pakete (gibt es), Skills, Bots als Paket ([assistent.md](assistent.md#bots-stufe-2)) | gering, darum bevorzugen |
| **Befehle** | eigene Befehle im Katalog, Bereich `ext:<id>` | mittel |
| **App** | eigenes Fenster, Klasse `torro-ext-<id>` | mittel |
| **Quelle / Anbieter** | Eingangsquelle, Kalender- oder Aufgabenanbieter, KI-Anbieter, Fenster-Manager | hoch (eigene Konten) |

### Manifest

```toml
# <data>/extensions/<id>/extension.toml
id = "timetracking"            # klein, [a-z0-9-], eindeutig
name = "Time tracking"
version = "0.3.0"
publisher = "…"
license = "MIT"
kinds = ["commands", "app"]
program = ["bin/torro-timetracking", "mcp"]   # Argumentliste, keine Shell

[rights]                       # was die Erweiterung selbst über die API braucht
tasks = "write"
calendar = "read"

[nextcloud]
requires = ["tables"]          # optionale Server-Apps; fehlen sie, bleibt die Erweiterung aus

[app]
window_class = "torro-ext-timetracking"
```

### Befehle einer Erweiterung im Katalog

- Der Dienst startet die Erweiterung, fragt ihre Werkzeuge ab und trägt sie als **Bereich
  `ext:<id>`** in den Katalog ein: API-Name `ext.<id>.<befehl>`, MCP-Tool `ext_<id>_<befehl>`.
- Den **Zugriff** (`Read`, `Write`, `Delete`) nennt das Manifest je Befehl. Ohne Angabe gilt
  `Delete`, also die strengste Stufe mit Freigabe. `Admin` und `Never` kann eine Erweiterung nicht
  vergeben.
- Jeder Aufruf geht durch den Dienst: Schlüssel → Rechte im Bereich `ext:<id>` → Freigabe → an die
  Erweiterung → Protokoll. Oberfläche, API und MCP entstehen so ohne Zutun der Erweiterung.
- Die Rechte für `ext:<id>` vergibt die Person im Rechte-Editor (API, MCP clients, Assistent › Access) wie für jeden Bereich, Standard
  „Kein Zugriff“.

### Nahtlos einbinden: Daten statt fremder Oberfläche

Wir übernehmen die Formen, die Nextcloud selbst nutzt. Eine Erweiterung liefert Daten, die Suite
zeichnet sie im Torro-Design. So sehen Erweiterungen aus wie der Rest, und Nextcloud-Apps und
Erweiterungen erscheinen in derselben Form.

| Andockstelle | Form | Wo es erscheint |
|---|---|---|
| **Kachel-Einträge** | wie Dashboard v2: Titel, Unterzeile, Link, Icon, `since` | Fenster-Modus, Overview, Leisten-Widgets |
| **Suchtreffer** | wie Unified Search: Anbieter, Treffer mit Titel, Unterzeile, Link | eine Suche der Suite über Nextcloud und alle Erweiterungen |
| **Link-Vorschau** | wie `references/resolve` | Eingang, Notizen, Chat des Assistenten |
| **Aktionen an Einträgen** | Befehl `ext.<id>.<aktion>` mit Kennung | Knöpfe im Eingang, Regeln (Punkt 10) |

Für eigene Fenster gibt es `torro-gui` (Rust) und für alle anderen Sprachen einen neuen Befehl, der
das **aufgelöste aktive Design** als JSON liefert (Farben, Schriften, Abstände).

## Was die API dafür braucht

In dieser Reihenfolge; jeder Punkt wird nach [api.md](api.md) dreifach geplant.

| # | Was | API · Zugriff | MCP | Warum |
|---|---|---|---|---|
| 1 | **Änderungsmeldungen** | `system.subscribe` · Read (je Bereich, nur was die Rechte zeigen) | MCP-Ressourcen-Benachrichtigung | sonst fragt jede Erweiterung selbst ab |
| 2 | **Nextcloud-Suche** | `nextcloud.search`, `nextcloud.list_search_providers` · Read | `nextcloud_search` … | nützt sofort, auch ohne Erweiterungen |
| 3 | **Nextcloud-Kacheln** | `nextcloud.widget_items` · Read | `nextcloud_widget_items` | Daten aller Server-Apps |
| 4 | **Links auflösen** | `nextcloud.resolve_link` · Read | `nextcloud_resolve_link` | Vorschauen |
| 5 | **Activity** | `nextcloud.activity` · Read | `nextcloud_activity` | Verlauf |
| 6 | **Design als Daten** | `settings.get_design_tokens` · Read | `settings_get_design_tokens` | gleiches Aussehen in jeder Sprache |
| 7 | **Dynamischer Katalog** | Bereich `ext:<id>`, `ext.<id>.<befehl>` | `ext_<id>_<befehl>` | eigene Befehle |
| 8 | **Erweiterungen verwalten** | `extensions.list` · Read; `extensions.install`, `extensions.remove`, `extensions.set_enabled` · Admin | `extensions_list`; Installieren **Never** (Frage 5) | Steuerzentrale |
| 9 | **Ablage je Erweiterung** | `ext.storage_get`, `ext.storage_put` · Write, nur `/.torro/<id>/`, ETag | nur für die Erweiterung selbst | geräteübergreifende Daten |
| 10 | **KI über den Dienst** | `ai.run` · Write (Funktion, Eingabe; Konto und Modell wählt der Dienst) | `ai_run` | Datenregeln aus [ki.md](ki.md) bleiben im Dienst |
| 11 | **Nextcloud als KI-Anbieter** | TaskProcessing im Dienst, Ergebnis per Abfrage | – (Anbieter, kein Befehl) | `ki.md`, offener Punkt 3 |
| 12 | optional nach Capability | eigene Bereiche `talk`, `deck`, `tables` | ebenso | wie Notizen ([notizen.md](notizen.md)) |

Namen und Zugriff sind Vorschläge; sie werden in der ersten Aufgabe jeder Funktion festgelegt. Für MCP-Server
gelten die gebauten Namen aus #302 (`extensions.list_extensions`, `extensions.call_tool`, `extensions.install_extension`
u. a., Abschnitt „MCP-Server (#302)“), nicht die Namen der Zeilen 7 und 8 oben.

## Sicherheit

### Nextcloud

- **Kein App-Passwort für Erweiterungen**, auf keinem Weg (Regel 1 in „Was nie nach außen geht“).
- **Kein allgemeiner Proxy** für beliebige Pfade des Servers, nur feste Befehle. Nie erreichbar:
  `core/apppassword`, `core/getapppassword`, `cloud/users` (schreiben), `webhook_listeners`,
  Remote Wipe.
- **Anmeldefehler behandelt nur der Dienst.** Einmal 401 → Wipe prüfen → Person neu anmelden lassen,
  nie in einer Schleife wiederholen; bei 429 und 503 warten. Laut Recherche zählt der Brute-Force-Schutz je
  IP-Adresse, eine Schleife sperrt also das ganze Netz der Person, auch ihre anderen Geräte (noch im
  Quellcode zu bestätigen; der Skill `nextcloud` sagt bisher „trifft den Nutzer“).
- **Freigaben anlegen und in Talk schreiben sind „nach außen“**: Sie gelten wie Mail senden
  (`Rights.outbound`, Standard „Mit Freigabe“).
- **Links aus Antworten** des Servers nur auf dem Origin des Kontos (`same_origin`), auch
  Vorschaubilder aus `references/resolve`.
- **Ablage:** Pfade der Erweiterung nur unterhalb von `/.torro/<id>/`, geprüft nach dem Dekodieren,
  ohne `..`.

### Erweiterungen

- **Grenze ehrlich benannt** (wie in [api.md](api.md)): Eine Erweiterung mit eigenem Programm läuft
  als derselbe Benutzer. Rechte schützen vor einer Erweiterung, die sich an die API hält, nicht vor
  bösartigem Code. Darum Hinweis beim Installieren, Daten-Erweiterungen bevorzugen, später Signaturen.
- **Fremder Text:** Werkzeugbeschreibungen und Antworten einer Erweiterung sind Fremdtext
  (`untrusted: true`), gesäubert wie Titel im Eingang. Eigene Namensräume (`ext_<id>_…`), damit kein
  Werkzeug ein Kern-Werkzeug überdeckt.
- **Im Auftrag:** Ruft ein Agent ein Werkzeug einer Erweiterung, gilt die **Schnittmenge** der Rechte
  von Agent und Erweiterung, wie bei den Bots. Ketten (Erweiterung ruft Erweiterung, die zurückruft)
  begrenzt der Dienst auf Tiefe 1.
- **Freigaben:** Bei Befehlen einer Erweiterung kennt der Dienst den Gegenstand nicht aus seinem
  Cache. Die Zusammenfassung kommt von der Erweiterung und wird so gekennzeichnet („laut Time
  tracking: …“).
- **Was eine Erweiterung nie darf:** Befehle in Kern-Bereichen anlegen, `Admin` oder `Never` vergeben,
  Rechte, Schlüssel oder Freigaben verwalten, das Mikrofon einschalten.
- **Eigene Geheimnisse** einer Erweiterung (etwa der Token eines fremden Dienstes) gibt die Person
  nur in der Oberfläche ein (Regel 5). Der Dienst legt sie in den Schlüsselbund und übergibt sie
  beim Start nur **dieser** Erweiterung, nie über die API.
- **Protokoll** wie immer ohne Inhalte: Client der Erweiterung, Befehl, Kennungen.
- **Entfernen** nimmt Schlüssel, Rechte, Katalogeinträge, Geheimnisse und den lokalen Ordner mit; was
  mit `/.torro/<id>/` auf dem Server passiert, ist offen (Frage 8).

## Mögliche Erweiterungspunkte

Sortiert nach Aufwand und Risiko, von „gibt es schon fast“ bis „großer Umbau“.

| # | Punkt | Was eine Erweiterung tut | Wie | Stand heute |
|---|---|---|---|---|
| 1 | **API/MCP-Clients** | ein fremdes Programm liest und schreibt Daten der Suite | Schlüssel + Rechte, Socket bzw. später HTTP | **gibt es** (Steuerzentrale › API bzw. MCP) |
| 2 | **Quellen des Eingangs** | meldet Einträge (Monitoring, CI, Messenger, Home Assistant) | `inbox.post_item` mit eigenem Client | **gibt es** für Skripte; fehlt: Manifest, Symbol, eigene Quelle in der Seitenleiste |
| 3 | **Design-Pakete** | Farben, Schriften, Logo | nur Daten, geprüft | **gibt es** (L.1–L.5) |
| 4 | **Werkzeuge für den Assistenten** | zusätzliche Werkzeuge im Chat | über den Katalog (`ext:<id>`), damit Rechte und Freigaben gelten; fremde MCP-Server nie direkt am Antrieb | neu |
| 5 | **Skills / Wissen für den Assistenten** | Text-Bausteine: wie man mit einem Fachgebiet umgeht | nur Daten (Markdown), werden dem Prompt hinzugefügt | neu, einfach |
| 6 | **Apps** | eigene Fenster-App in der Suite (z. B. Zeiterfassung) | eigenes Programm mit Manifest, erscheint in `apps.list_apps`, Fensterklasse `torro-ext-<id>`, `torro-gui` oder Design als JSON | `AppId` heute fest im Code |
| 7 | **Datenquellen / Konten** | neuer Anbieter für Kalender, Aufgaben, Mail (Todoist, Google, Exchange) | MCP mit festen Werkzeugnamen; der Kern bleibt Cache und Rechteprüfung | Anbieter heute im Code (`torro-pim-*`) |
| 8 | **Fenster-Manager** | Anbindung an weitere Tiling-Manager | MCP mit festen Werkzeugnamen, abgeleitet aus dem `torro-wm`-Trait | im Code (Hyprland, geplant AeroSpace, GlazeWM) |
| 9 | **KI-Anbieter** | weitere Modellanbieter | meist reicht ein Katalogeintrag (OpenAI-kompatibel); Nextcloud TaskProcessing über das vorhandene Konto; sonst wie 7 | geplant ([ki.md](ki.md)) |
| 10 | **Aktionen und Automatisierung** | „wenn X im Eingang, dann Y“; Knöpfe in Einträgen | Regeln rufen Befehle des Katalogs, auch `ext.<id>.<aktion>` | Regeln des Eingangs gibt es, ohne fremde Aktionen |
| 11 | **Kacheln und Leisten-Widgets** | Fenster-Modus, Overview, Omarchy/Waybar, macOS-Menüleiste | Kachel-Einträge in der Form von Dashboard v2; Leisten über `status --json` | geplant (Phase 5, [modes.md](modes.md)) |
| 12 | **Suchanbieter** | Treffer in der Suche der Suite | Form der Unified Search | neu |

## Was man nicht erweiterbar machen sollte

- Rechte, Schlüssel, Freigaben und Protokoll: bleiben ganz im Kern.
- Anmeldung bei Konten und die Fehlerbehandlung dazu (401, Wipe, Drosselung): nur im Dienst.
- Eingriffe in fremde Oberflächen (eigene Knöpfe in der Kalender-App): nur über die festen
  Andockstellen oben; sonst bricht jede Änderung der Oberfläche Erweiterungen.
- Layout-Sprachen und Code in Design-Paketen (siehe [design-pakete.md](design-pakete.md)).

## Verteilung und Vertrauen

- **Stufe 1:** Ordner von Hand nach `<data>/extensions/` kopieren, Steuerzentrale zeigt Manifest und
  verlangte Rechte, Person bestätigt.
- **Stufe 2:** Installieren aus einer Adresse (Git-Repository, Archiv) mit Prüfsumme.
- **Stufe 3:** Verzeichnis / Katalog, Signaturen der Herausgeber, Updates.

## Nebenbei gefunden (2026-10-15)

- `docs/fotos.md` nennt „Nextcloud 36“; das ist der Entwicklungszweig, aktuell ist 35.
- Der Skill `nextcloud` sagt, `sync-collection` gehe auch für Dateien, und Fehlversuche bei der
  Anmeldung träfen den Nutzer. Laut Recherche geht `sync-collection` nur für Kalender und
  Adressbücher, und die Sperre zählt je IP-Adresse. Vor einer Korrektur im Quellcode nachprüfen.

## MCP-Server (#302)

Umgesetzt in den Aufgaben MA.1–MA.11 ([tasks/mcp-anbindungen.md](tasks/mcp-anbindungen.md)). Ein MCP-Server ist ein
fremdes Programm auf dem Rechner (stdio) oder eine Adresse (Streamable HTTP). Der Dienst ist sein MCP-Client: Er
startet das Programm bei Bedarf, liest seine Werkzeuge und bietet die erlaubten dem Assistenten, seinen Bots und den
MCP-Clients an. Der Server spricht nie mit Torro, hat keinen eigenen Schlüssel und sieht das App-Passwort nicht.

### Festlegungen

- **Bereich und Stufen:** Der feste Bereich `extensions` (Auflisten, Katalog, Ein- und Ausschalten, Entfernen) steht
  in den Vorlagen wie jeder Bereich. Die Werkzeuge eines Servers öffnet eine Stufe je Server
  (`Rights::extensions`: Kennung → Kein Zugriff · Lesen · Lesen und Schreiben · Voll; fehlt = Kein Zugriff). Keine
  Vorlage öffnet einen Server, auch *Full access* nicht. Ein einzelnes Werkzeug sperrt `ext.<id>.<werkzeug>` in
  `Rights::denied`. Kein `Area` je Server.
- **Zugriff je Werkzeug:** nur aus dem Katalog-Eintrag (`Read`, `Write`, `Delete`), ohne Angabe `Delete`. `Admin` gibt
  es für Werkzeuge nicht; Hinweise des Servers (`readOnlyHint`, `destructiveHint`) zählen nicht.
- **Namen nach außen:** `ext_<id>_<werkzeug>`, höchstens 64 Zeichen, nur `A–Z`, `a–z`, `0–9`, `_` und `-`. Die Kennung
  hat `a–z`, `0–9` und `-`, beginnt mit einem Buchstaben und hat höchstens 24 Zeichen. Ein Werkzeug, dessen Name das
  nicht erfüllt, wird nicht angeboten, und sein Status nennt es.
- **Zustimmen:** Installieren startet den Server einmal, liest seine Werkzeuge und speichert ihn aus, Status `review`.
  Erst „Allow tools“ (`extensions.confirm_tools` mit dem Fingerabdruck der gezeigten Liste) merkt sich die Werkzeuge
  und schaltet den Server ein. Fingerabdruck je Werkzeug: SHA-256 über kanonisches JSON aus Name, Beschreibung und
  Eingabe-Schema. Meldet der Server später neue oder geänderte Werkzeuge (beim Start oder über
  `notifications/tools/list_changed`), werden genau diese nicht angeboten, bis ein Mensch zustimmt. Unveränderte
  laufen weiter, verschwundene fallen weg.
- **Fremder Text:** Beschreibungen, Schemas und Antworten eines Servers sind `untrusted`, überall. Die Beschreibung
  eines Werkzeugs beginnt bei Agenten mit „From <Name> (external, untrusted): …“.
- **Nicht angeboten:** Elicitation, Sampling und Roots. Torro meldet diese Fähigkeiten beim `initialize` nicht an;
  Sampling kommt später. Ordnergrenzen bekommen Server nicht als Roots, sondern als Startoptionen (#303, Abschnitt
  „Dokumente mit ArtCraft“).
- **Umgebung des Programms:** leer, dann nur `HOME`, `USER`, `LOGNAME`, `LANG`, `LC_ALL` und `TMPDIR` (unter Windows
  einige mehr), dann der `PATH` (der Ordner des gefundenen Programms vorn), dann die Variablen des Servers. Gesperrt und
  beim Installieren mit Fehler abgelehnt: was ein Programm anderen Code laden lässt (`LD_*`, `DYLD_*`,
  `PYTHONPATH`, `NODE_OPTIONS`, `JAVA_TOOL_OPTIONS`, `GIT_SSH_COMMAND`, `ComSpec` u. a.), woher Pakete und
  Einstellungen kommen (`npm_config_*`, `UV_*`, `PIP_*`, `HOME`, `APPDATA` u. a.), was Geheimnisse woandershin
  schickt (`DOCKER_HOST`, `DOCKER_CONFIG`, eigene Zertifikate wie `SSL_CERT_FILE`), dazu `PATH`, `PATHEXT` und
  `TORRO_*`. Die vollständige Liste steht bei `blocked_env` in `torro-ipc`. Geheimnisse gehen nur beim Start an
  genau diesen Server.
- **Adressen:** nur Streamable HTTP. `https://` ist Pflicht, `http://` nur für `localhost`, `127.0.0.1` und `::1`;
  keine Weiterleitungen, für `http://` kein Proxy; Anmeldung nur über Header. OAuth kommt als eigenes Issue.
- **Prozess:** Start bei Bedarf (erster Aufruf, Installieren, Zustimmen), Startzeit höchstens 30 s, beim Installieren
  und Aktualisieren 180 s. Ein Aufruf höchstens 60 s. Nach 10 Minuten ohne Aufruf beendet. Nach einem Absturz neu beim
  nächsten Bedarf, höchstens drei Starts in fünf Minuten, dann Status `crashed` bis zum Einschalten. Beenden nimmt den
  ganzen Prozessbaum mit (Unix: Prozessgruppe; Windows: Job Object). Die Ausgabe (stderr) bleibt im Speicher, die letzten
  200 Zeilen bzw. 64 KB, und ist nur über „Output“ in der Steuerzentrale lesbar, nie im Protokoll.
- **Speicher:** `<data>/extensions/extensions.json` (Ordner 0700, Datei 0600), die Geheimnisse im Schlüsselbund unter
  `ext:<id>:<NAME>`. Ein Server wird in der Steuerzentrale eingerichtet: aus dem eingebauten Katalog, über das Formular
  oder durch Einfügen eines `mcpServers`-Blocks. Ein Manifest je Erweiterung (oben) ist dafür nicht gebaut.
- **Katalog:** eingebaut (`assets/extensions/catalog.toml`) mit Time, Sequential Thinking und GitHub, seit #303 auch
  WordCraft, DeckCraft, GridCraft und PdfCraft. Fehlt eine Voraussetzung (`npx`, `uvx`, Docker, `<app>-cli`), steht
  das in der Zeile, und der Server lässt sich nicht installieren.
- **Protokoll und Log** nennen Kennung des Servers, Werkzeugname und Fehlerart, nie Argumente, Antworten oder Werte
  von Variablen.
- **Nur aus der Oberfläche:** Installieren, Einschalten, Zustimmen, Aktualisieren, Geheimnisse setzen, Ausgabe lesen
  (Punkt 10 in [api.md](api.md)). Ausschalten und Entfernen darf ein Agent mit Admin-Schalter und Freigabe.

### Abweichungen von den Festlegungen (MA.1–MA.10)

- **Geheimnisse bei „Update“:** Bei eigenen Servern bleiben gespeicherte Geheimnisse nur bei gleichem Befehl und
  gleichen Argumenten (stdio) bzw. gleichem Ursprung, also Schema, Host und Port (HTTP). Sonst gehen sie. Bei
  Katalog-Servern bleiben sie beim Versionswechsel aus dem Katalog.
- **Sichtbarkeit fremder Schlüssel:** Ein Schlüssel, der nicht der App-Schlüssel ist, sieht in
  `list_extensions` und `get_extension` nur Server, für die er eine Stufe hat. Deren Einrichtung ist geschwärzt:
  Argumente und Klartextwerte stehen als „…“, ein Befehl mit Pfad nur mit seinem letzten Teil („…/server“, auch im Status `missing`), eine
  Adresse nur mit ihrem Ursprung. Geheimnisse bleiben „gespeichert“ bzw. „fehlt“. Vollständig sieht sie nur der
  App-Schlüssel.
- **Geheimnisse gehören in Variablen oder Header**, nie in die Argumente. Argumente sind kein Geheimnis: Sie stehen
  in `extensions.json` und erscheinen für den App-Schlüssel (Steuerzentrale, Formular „Update…“) im Klartext. Nur
  Variablen und Header lassen sich als „Secret“ markieren und liegen dann im Schlüsselbund. Ein Block aus Claude
  Desktop mit einem Token in `args` gehört deshalb vor dem Hinzufügen umgestellt, etwa auf eine Variable, die der
  Server liest.
- **Länge der Argumente:** Ein Werkzeugaufruf (`call_tool`) hat höchstens 64 KB Argumente als JSON; darüber `error`.
- **Höher eingestuft:** Ein Werkzeug, das der Katalog nach einem Update höher einstuft (etwa von Lesen auf Schreiben),
  wartet wieder auf Zustimmung.
- **Abschalten für eine Unterhaltung:** Wird der Bereich `extensions` für eine Unterhaltung des Assistenten
  abgeschaltet, sind auch alle Server für sie aus.
- **Nicht „nach außen“:** Ein Server-Werkzeug zählt nicht als ausgehend (`Rights::outbound`). Ob es auf Freigabe
  wartet, entscheidet allein sein Zugriff aus dem Katalog. Die Folgen stehen bei den Rechten in [api.md](api.md).
- **Hook-Schlüssel:** Hooks haben keinen MCP-Server. `extensions.list_tools` ist für sie leer, `extensions.call_tool`
  liefert `not-found`.
- **Keine Liste im Gespräch:** Torros MCP-Server meldet geänderte Werkzeuglisten nicht (kein `list_changed`). Fremde
  Agenten sehen neue Server-Werkzeuge erst in der nächsten Sitzung.
- **Oberfläche:** Steuerzentrale › MCP servers hat vier Teile, Allowed folders (#304), Built-in tools (mit den Unterseiten „PDF templates“
  und „Search providers“ der Websuche, #305), Catalog und Your own servers; jeder Server hat eine eigene Seite. Assistent › Tools zeigt nur noch einen Verweis.

## Dokumente mit ArtCraft (#303)

Umgesetzt in den Aufgaben AC.1–AC.7 ([tasks/artcraft.md](tasks/artcraft.md)), Stand der Quellen am 2026-10-26:
WordCraft v0.11.0, DeckCraft v0.11.1, GridCraft v0.12.1, PdfCraft v0.14.0 (https://github.com/storytold), je
`MIT OR Apache-2.0`. Torro bearbeitet Dokumente nicht selbst und bindet keine ArtCraft-Bibliothek ein: Die Person
installiert die Apps, Torro startet ihren MCP-Server (`<app>-cli mcp`, ohne Fenster) und vergibt die Rechte, die
ArtCraft selbst nicht kennt.

- **Vier Katalog-Einträge**, jede App ein eigener Server mit eigener Stufe: `wordcraft`, `deckcraft`, `gridcraft`,
  `pdfcraft`, „Experimental“, Link zu den Releases der App. PdfCraft ohne `--compact`: alle 181 Werkzeuge einzeln
  eingestuft (49 Lesen, 109 Schreiben, 17 mit Freigabe, 6 gesperrt). Nie `--connect`, nie `--root`. `version` ist die
  geprüfte Version („tested with“); die Regel „ein Argument nennt die Version“ gilt nur für `npx`, `uvx`, `docker`.
  `<app>-cli` liegt unter Linux in `/usr/bin`, unter macOS nur im eigenen Zip der Release (die Person legt es nach
  `/usr/local/bin` oder `~/.local/bin`), unter Windows in `%ProgramFiles%\<App>` (Katalogfeld `look_in`).
- **Ein Ordner je App:** Die Person wählt auf der Seite der App einen der freigegebenen Ordner
  (`extensions.set_folder`, Admin, nur App-Schlüssel, MCP Never). Torro startet WordCraft, DeckCraft und PdfCraft mit
  ihm als `--automation-read-root` und, bei „Read and write“, als `--automation-write-root`; sonst ist der eigene
  Arbeitsordner `<data>/extensions/<id>/work` die Schreibgrenze. Ohne gewählten oder ohne nutzbaren Ordner ist der
  Arbeitsordner beide Grenzen und jeder Aufruf wird abgelehnt. Ein anderer Ordner startet den Server beim nächsten
  Aufruf neu.
- **Wählbar** ist ein freigegebener Ordner ohne gesperrten Inhalt (Schlüsselspeicher, Ordner der Suite, Dateien der
  Clients) und, bei „Read and write“, ohne inneren Ordner mit „Read“. Geprüft beim Wählen, bei jedem Start und bei
  jedem Aufruf.
- **Torro prüft die Pfade selbst**, vor der Freigabe und beim Ausführen: die Argumente, die der Katalog als gelesen
  (`reads`) oder als Ziel (`target`) nennt, und in beliebiger Tiefe jeden Text unter `path`, `paths`, `out`,
  `out_dir`, `folder`, `file`, `image`, `directories`. Jeder liegt im gewählten Ordner, nicht bloß in irgendeinem
  freigegebenen, und ist nicht gesperrt. Bei GridCraft, das keine eigene Grenze hat, ist das die einzige Grenze.
- **Speichern nur als neue Datei:** Ein Ziel darf es nicht geben, auch nicht als symbolischen Link (`conflict`). Wo
  ein fehlendes Ziel „über das Original“ heißt (`save_document`, `save_presentation`, `save_workbook`, `doc_save`),
  ist es Pflicht (`error`). Werkzeuge mit mehreren oder unbekannt benannten Ausgaben (DeckCraft `export`, PdfCraft
  `doc_split` u. a.), Drucken, Skripte und die Allzweck-Befehle sind `delete`, also mit Freigabe. GridCraft speichert
  nur mit Freigabe.
- **Gesperrt** (nie angeboten, nie zustimmbar, `ToolState::Blocked`; als Wert unter `name`, `id`, `command` oder
  `method` in den Argumenten abgelehnt, ohne Rücksicht auf Groß- und Kleinschreibung): die Werkzeuge, die das Fenster
  der laufenden App bedienen (`screenshot`, `click`, `key`, `ui_inspect`, `pointer`, `call_app`, Befehle `ui.*`),
  GridCraft `execute_command`, PdfCraft `js_run` und die Signatur-Werkzeuge mit Schlüsseln des Systems
  (`sign_document`, `sign_trust`, `sign_id_create`, `sign_keychain_ids`, `sign_windows_ids`, Befehle
  `tools.js_console` und `sign.digital`).
- **Den Ordner sehen** der App-Schlüssel und jeder Client mit einer Stufe für den Server (`ExtensionInfo::folder`),
  damit ein Agent absolute Pfade darin bilden kann. Er ist eine Grenze, kein Geheimnis.
- **Oberfläche:** Seite des Servers unter MCP servers, Gruppe „Folder“ vor „Tools“ mit „Choose…“ (Menü der
  freigegebenen Ordner und „None“); GridCraft sagt in Katalog, Dialog und Seite, dass es sich an keinen Ordner hält.
  Die Einrichtung stellt die Apps im Schritt „Documents“ vor ([einrichtung.md](einrichtung.md)).

### Abweichungen und offene Punkte (AC.1–AC.7)

- **Startoptionen statt Roots:** Die Grenzen sind Startoptionen (`Launch::bounds`), keine MCP-Roots: ArtCraft meldet
  keine Roots-Fähigkeit und ignoriert `roots/list_changed`.
- **Sperren nach Namen** (`*.torro-backup`, `.Trash-*`) im gewählten Ordner findet die Prüfung beim Wählen nicht;
  symbolische Links im Ordner löst ArtCraft beim Prüfen seiner Grenze selbst auf.
- **Allzweck-Befehle können überschreiben** (`execute`, `batch`, `run_command`, `tool_call`, `command_run`, etwa mit
  `file.saveAs`). Schutz ist dort allein die Freigabe, die ihre Argumente zeigt; mit „Freigabe vorher: Nie“ entfällt
  sie, bei PdfCraft auch fürs Drucken.
- **Ziele ohne Endung:** PdfCraft `image_save` hängt eine fehlende Endung selbst an; Torro prüft das Ziel ohne sie.
- **Strenger als geplant** (aus dem API-Review): Bei GridCraft sind Werkzeuge, die der Katalog nicht nennt, gesperrt
  statt `delete`, denn dort ist Torros Pfadprüfung die einzige Grenze und kennt nur die Argumente der genannten.
  PdfCraft `link_open` ist `delete` statt `read`: Es öffnet die Datei, die ein Link nennt, auch eine absolute.
  Gesperrte Namen sucht Torro auch unter `tool`, `tools`, `op`, `action`, `cmd` und `commands`.
- **Eintrag nicht mehr im Katalog:** Grenzen und Pfadprüfung hängen am Katalog-Eintrag. Nimmt ein späteres Update
  einen ArtCraft-Eintrag heraus oder benennt ihn um, wird eine gespeicherte Installation, die in einem Ordner
  gearbeitet hat (gewählter Ordner oder eine Kennung aus `catalog::WORK_IN_A_FOLDER`), bei jedem Aufruf abgelehnt:
  nie ohne Grenze. Die Person entfernt sie und installiert sie neu.
- **Fehlalarme:** Gesperrte Namen gelten in den Argumenten jedes Werkzeugs, nicht nur der Allzweck-Befehle; ein
  Formularfeld namens „key“ in WordCraft wird darum abgelehnt.
- DeckCraft schreibt durch symbolische Links, und WordCraft v0.11.0 prüft den Verzeichniseintrag noch nicht; Torro
  deckt das nur für die Ziele ab, die es selbst prüft.
- Die Werkzeugtabelle von PdfCraft steht in `crates/automation/src/tools.rs`, nicht in `crates/mcp`. GridCraft
  v0.12.1 hat kein `list_windows`; der Eintrag nennt es nicht.
- **Neubewertung um 2027-01-26:** Rechtliches (Nachbau im Aussehen von Adobe- und Microsoft-Programmen), Rechte-Stufen
  und mehrere Ordner bei ArtCraft, Ordnergrenzen in GridCraft.

## Entscheidungen und offene Fragen

1. **Protokoll:** **Entschieden (#302):** MCP für Erweiterungen mit Code (Erweiterung als MCP-Server, Dienst als
   Client). Ob Datenquellen (Punkt 7) und Fenster-Manager (Punkt 8) denselben Weg gehen, bleibt offen, bis sie gebaut
   werden.
2. **Feste Schnittstellen öffnen:** **Entschieden (#302):** ein fester Bereich `extensions` und die Stufe je Server in
   `Rights::extensions`, kein `Area` und kein `Request` je Server. Die Werkzeuge kommen über `extensions.call_tool`
   mit Kennung. Fensterklassen (`AppId`) bleiben offen.
3. **Reihenfolge:** Vorschlag `subscribe` → Nextcloud-Lesebefehle (Suche, Kacheln, Links,
   Activity) → Design als Daten → Katalog und Manifest → Apps und Kacheln → Quellen und Anbieter.
   Die Nextcloud-Lesebefehle nützen auch ohne Erweiterungen sofort.
4. **ExApps:** ganz draußen lassen, oder als gekennzeichnete „Server-Stufe“ zulassen, die über die
   Capabilities erkannt wird und nie Voraussetzung der Basis ist?
5. **Installieren über die API:** **Entschieden (#302):** nur aus der Oberfläche (`Never`); Agenten bekommen weder
   Installieren noch Zustimmen, auch nicht mit Freigabe.
6. **Geheimnisse an Erweiterungen:** **Entschieden (#302):** Der Dienst übergibt einem Server sein Geheimnis beim
   Start, nur an diesen Server. Die Werkzeugaufrufe macht der Dienst als MCP-Client wie sonst; das Geheimnis geht dabei
   nicht weiter.
7. **Datenquellen von Dritten** (Punkt 7): wirklich öffnen, oder bauen wir die wichtigen selbst?
8. **Ablage auf dem Server:** Bleibt `/.torro/<id>/` beim Entfernen liegen (die Daten gehören der
   Person, andere Geräte nutzen sie vielleicht noch), oder fragt die Suite?
9. **Ab wann Stufe 2 und 3** der Verteilung, und brauchen wir Signaturen schon in Stufe 2?
10. **Lizenzen:** Dürfen Erweiterungen geschlossen sein? Open First spricht für offene, verbietet
    geschlossene aber nicht zwingend.
