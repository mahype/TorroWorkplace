# Erweiterungen (Plugins)

Stand: 2026-10-15, Issue #79. **Entscheidungsentwurf.** Die Vorschläge unten sind ausgearbeitet,
entschieden ist noch nichts; die offenen Entscheidungen stehen am Ende als Fragen.

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
| `torro_ipc::api::Area` (15 feste Bereiche), `Request::operation()` (vollständiges `match`) | geschlossen | keine eigenen Befehle, keine eigenen MCP-Tools |
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
Script-Client).

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
- Die Rechte für `ext:<id>` vergibt die Person im Rechte-Editor (API, MCP, Assistent › Access) wie für jeden Bereich, Standard
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

Namen und Zugriff sind Vorschläge; sie werden in der ersten Aufgabe jeder Funktion festgelegt.

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

## Offene Entscheidungen

1. **Protokoll:** MCP für alle Erweiterungen mit Code (Erweiterung als MCP-Server, Dienst als
   Client), auch für Datenquellen und Fenster-Manager? Vorschlag: ja.
2. **Feste Schnittstellen öffnen:** `Area`, `Request` und `AppId` bekommen je einen Fall für
   Erweiterungen (`ext:<id>`, Fensterklassen aus dem Manifest). Das ändert festgelegte
   Schnittstellen. Einverstanden, und in welcher Form (ein `Area::Extension(id)` oder ein eigener
   Katalog neben dem festen)?
3. **Reihenfolge:** Vorschlag `subscribe` → Nextcloud-Lesebefehle (Suche, Kacheln, Links,
   Activity) → Design als Daten → Katalog und Manifest → Apps und Kacheln → Quellen und Anbieter.
   Die Nextcloud-Lesebefehle nützen auch ohne Erweiterungen sofort.
4. **ExApps:** ganz draußen lassen, oder als gekennzeichnete „Server-Stufe“ zulassen, die über die
   Capabilities erkannt wird und nie Voraussetzung der Basis ist?
5. **Installieren über die API:** nur aus der Oberfläche (`Never`, wie Konten verbinden) oder für
   Agenten als `Admin` mit Freigabe? Vorschlag: nur Oberfläche, weil eine Erweiterung Code mitbringt.
6. **Geheimnisse an Erweiterungen:** Darf der Dienst einer Erweiterung ihr eigenes Geheimnis beim
   Start übergeben (sie braucht es, um ihren Dienst zu erreichen), oder führt der Dienst auch diese
   Aufrufe selbst aus?
7. **Datenquellen von Dritten** (Punkt 7): wirklich öffnen, oder bauen wir die wichtigen selbst?
8. **Ablage auf dem Server:** Bleibt `/.torro/<id>/` beim Entfernen liegen (die Daten gehören der
   Person, andere Geräte nutzen sie vielleicht noch), oder fragt die Suite?
9. **Ab wann Stufe 2 und 3** der Verteilung, und brauchen wir Signaturen schon in Stufe 2?
10. **Lizenzen:** Dürfen Erweiterungen geschlossen sein? Open First spricht für offene, verbietet
    geschlossene aber nicht zwingend.
