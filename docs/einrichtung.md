# Einrichtung beim ersten Start

Stand: 2026-10-14. **Freigegeben** (Entscheidungen am Ende). Klickbarer Entwurf:
[prototypes/setup/wizard.html](prototypes/setup/wizard.html). Aufgaben: [tasks/einrichtung.md](tasks/einrichtung.md).

Wer Torro Workplace zum ersten Mal startet, sieht bisher die Steuerzentrale mit leeren Seiten und muss
selbst wissen, wo er anfängt. Die Einrichtung führt einmal durch alles, was die Suite braucht:
**zuerst Nextcloud, dann die Wahl der Komponenten, dann jede gewählte Komponente einzeln, am Ende die KI.**

## Grundsätze

1. **Nichts ist Pflicht.** Jeder Schritt lässt sich überspringen. Ohne Nextcloud geht alles, nur
   ohne die Vorbelegung: Kalender kommen dann über CalDAV, Aufgaben über CalDAV oder GitHub.
2. **Was verbunden ist, ist gespeichert.** Die Einrichtung ist ein geführter Weg durch dieselben
   Requests wie die Steuerzentrale, kein eigener Speicher. Wer mittendrin schließt, verliert nichts
   und landet beim nächsten Öffnen beim ersten offenen Schritt.
3. **Eine Komponente nach der anderen.** Feste Reihenfolge: Kalender, Aufgaben, Mail, Eingang, KI.
   Abgewählte Komponenten verschwinden aus der Schrittliste.
4. **Keine zweite Umsetzung.** Die Verbindungswege gibt es im Dialog „Add account“ (`ui/wizard.rs`).
   Nextcloud und GitHub sind Bausteine in `ui/connect/`, die beide benutzen; CalDAV hat dort einen kurzen
   Weg nur für Kalender und Aufgaben. Für Mail, TorroMail und ntfy öffnet die Einrichtung den Dialog selbst
   über sich, auf dem passenden Weg (W.7).
5. **Open First.** Lokale KI steht oben und wird von selbst gesucht. Cloud-Anbieter folgen, mit dem
   Sitz des Anbieters in jeder Zeile, damit die Wahl bewusst fällt ([ki.md](ki.md), Grundsatz 4).
6. **Geheimnisse nur in der Oberfläche.** Passwörter und Schlüssel werden nur in Felder der
   Einrichtung getippt. Ein Agent kann die Einrichtung nicht stellvertretend durchlaufen (die
   `connect_*`-Befehle mit Geheimnis bleiben `Never`).

## Wann sie erscheint

| Anlass | Was passiert |
|---|---|
| Dienst startet, **kein Konto** und Einrichtung nie abgeschlossen | Der Dienst öffnet `torroworkplace ui --setup` (eine offene Steuerzentrale kommt nach vorn). Das wiederholt sich bei jedem Start, bis die Einrichtung abgeschlossen oder verschoben ist |
| „Set up later“ in der Einrichtung | Stand `skipped`; Overview zeigt oben die Karte „Finish setup“ mit „Continue setup“ |
| App ohne Konto, „Open Settings“ (`torro-gui/src/connect.rs`) | Solange die Einrichtung nicht fertig ist, öffnet der Knopf die Einrichtung beim Schritt der App |
| General › „Run setup again“ | Startet bei Schritt 1; Verbundenes steht schon als erledigt da |
| Neue Version bringt einen neuen Schritt (z. B. KI) | Overview: „New in setup: AI“, nicht ungefragt öffnen |

Gespeichert wird nur der Stand in `settings.json`: `setup: { state: pending | skipped | done, version }`, dazu
`componentsOff` mit den abgeschalteten Komponenten.
Mit `version` erkennt die Suite, dass es seit dem letzten Durchlauf neue Schritte gibt. Welcher Schritt
erledigt ist, leitet die Oberfläche aus den Daten ab (Konto da, Komponente gewählt …), nicht aus einem
eigenen Merker.

## Fenster

Die Einrichtung ist ein **eigener Modus der Steuerzentrale** (`torroworkplace ui --setup`), kein Dialog:
acht Schritte mit Listen brauchen den Platz, und die Schrittliste zeigt jederzeit, wo man steht.

```
┌ Schritte 232 (BG) ──────┐┌ Inhalt (PANEL, eingesetzt) ─────────────────────────────────────────┐
│ [T] Set up Torro        ││ Step 4 of 8 · Calendar                        Set up later           │
│                         │├──────────────────────────────────────────────────────────────────────┤
│ ✓ Welcome               ││   Calendar                                   (headline, max 640)     │
│ ✓ Nextcloud             ││   Choose which calendars the suite shows.    (TEXT_2)                │
│ ✓ Components            ││                                                                      │
│ ● Calendar              ││   Nextcloud · anna@cloud.example.org         (Gruppe)                │
│ ○ Tasks                 ││   ● Work                         [on]                                │
│ ○ Mail                  ││   ● Family                       [on]                                │
│ ○ Inbox                 ││   ● Birthdays  Read-only         [off]                               │
│ ○ AI                    ││   Other calendars                                                     │
│ ○ Done                  ││   + Add a CalDAV calendar   mailbox.org, iCloud, Fastmail …          │
│                         ││                                                                      │
│                         │├──────────────────────────────────────────────────────────────────────┤
│ Torro Workplace · 0.4   ││ (Fehler / Status)                     Back   Skip   [Continue]        │
└─────────────────────────┘└──────────────────────────────────────────────────────────────────────┘
```

- **Schrittliste** links auf dem Fenstergrund, Zeilen 28 wie die Seitenleiste: Icon des Schritts,
  Name, rechts der Zustand als Icon (Häkchen erledigt, leise „Skipped“). Der aktuelle Schritt ist
  `SELECTED`, nie rot. Jeder Schritt ist anklickbar, auch zurück.
- **Kopfleiste** 44: „Step 4 of 8 · Calendar“, rechts „Set up later“ (ghost).
- **Inhalt** höchstens `size::form_max()` 640 breit, links bündig: Titel `typo::headline()`, ein Satz
  in `TEXT_2`, dann Gruppen (`widgets::group`) mit Zeilen 44/56 und einem erklärenden Satz darunter.
- **Fuß** mit Linie: links Status oder Fehler (Symbol + Wort), rechts „Back“ (ghost), „Skip“
  (secondary) und **genau ein** Primärknopf („Continue“, in Unterschritten „Connect“, am Ende
  „Finish“). Enter = Primärknopf, Escape = Unterschritt schließen bzw. „Back“.
- **Mittel** (< 1024): Schrittliste als Leiste 52 (Icons, Name als Tooltip).
- **Schmal** (< 600): keine Schrittliste. Kopf „‹ Back · 4/8 · Calendar“, darunter ein Fortschritt
  `progress_bar`. Fuß wie oben, Knöpfe auf ganzer Breite wenn nötig.
- **Mindestgröße** 360×320 wie die Steuerzentrale. Mini und Leiste zeigen keine Einrichtung, sondern
  in der Overview-Kachel „Setup isn't finished“.

## Die Schritte

### 1 Welcome

„Set up Torro Workplace“, ein Satz, was gleich passiert („Connect your Nextcloud, choose what you
need, and add more calendars, mail and AI. Every step can be skipped.“). Darunter **Language** und
**Appearance** als Segmente, weil sie alles Folgende betreffen. Primär „Get started“.

### 2 Nextcloud

Der vorhandene Weg (Adresse → Anmeldung im Browser → Prüfen), eingebettet statt als Dialog. Nach dem
Prüfen zeigt der Schritt, **was der Server hat**, als Liste mit Zustand:

| Dienst | Woran erkannt | Wofür in der Suite |
|---|---|---|
| Calendars | CalDAV antwortet, mindestens ein Kalender mit `VEVENT` | Kalender |
| Tasks | Kalender mit `VTODO` (die Tasks-App ist nur die Weboberfläche) | Aufgaben |
| Contacts | CardDAV antwortet (noch nicht abgefragt) | später |
| Notifications | Capabilities `notifications` | Eingang |
| Talk | Capabilities `spreed` | Eingang |
| Push | Capabilities `notify_push` | Eingang ohne Abfragen |
| Deck, Notes | Capabilities `deck`, `notes` | später |

Wichtig: CalDAV gehört in Nextcloud zum Kern (App `dav`), die Kalender-App ist nur die Weboberfläche.
„Kalender auf Nextcloud aktiviert“ heißt darum für die Suite: CalDAV antwortet und hat Kalender. Fehlt
ein Dienst, steht er grau da mit „Not on this server“, ohne Vorwurf.

Zweiter Weg: „I don't use Nextcloud“ (ghost) überspringt.

### 3 Components

Eine Gruppe mit einer Zeile je Komponente: Symbol, Name, darunter woher die Daten kommen, rechts der
Schalter (keine Karten: Einstellungsseiten bestehen aus Gruppen, Design-Skill Abschnitt 5):

| Komponente | Vorbelegung | Untertitel (Beispiel) |
|---|---|---|
| Calendar | an, wenn Nextcloud Kalender hat | „From Nextcloud and other CalDAV calendars“ |
| Tasks | an, wenn Nextcloud Aufgabenlisten hat | „From Nextcloud, CalDAV and GitHub issues“ |
| Mail | aus | „IMAP accounts, or take them over from TorroMail“ |
| Inbox | an | „Notifications, mentions and reviews in one place“ |
| Assistant | aus | „Chat with a local or cloud AI“ |
| Workspaces | an mit Tiling-Manager, sonst aus | „Save and restore window layouts (Hyprland found)“ |

Was „aus“ bedeutet, siehe [Komponenten an und aus](#komponenten-an-und-aus).

### 4 Calendar

- **Gruppe „Nextcloud · anna@…“** (nur mit Nextcloud-Konto, das Kalender hat): je Kalender Punkt in
  seiner Farbe, Name, „Read-only“ wo zutreffend, Schalter. Fußsatz: „Hidden calendars stay on the
  server. Change this later under Accounts.“
- **Gruppe „Other calendars“:** schon verbundene CalDAV-Konten mit ihren Kalendern, darunter
  „Add a CalDAV calendar“. Das öffnet im Schritt den CalDAV-Weg: Anbieter wählen (mailbox.org, Posteo,
  Fastmail, iCloud, „Other CalDAV server“), Adresse bzw. E-Mail, Passwort (App-Passwort-Hinweis, wo der
  Anbieter es verlangt), „Connect“. Danach erscheinen seine Kalender in derselben Liste.
- Ohne Nextcloud und ohne Konto: leere Gruppe mit einem Satz und „Add a CalDAV calendar“ als Zeile.
- Später als weitere Zeilen, kein Umbau: Kalender abonnieren (ICS-Link), Google, Microsoft 365.

### 5 Tasks

Wie Kalender: Nextcloud-Listen mit Schalter, weitere CalDAV-Listen (oft dasselbe Konto wie in Schritt 4,
dann schon da), dazu **GitHub** („Show issues and pull requests as lists“) mit den zwei bekannten Wegen
(GitHub CLI übernehmen oder Token einfügen).

### 6 Mail

Oben „Use accounts from TorroMail?“, falls es welche gibt (vorhanden, M7.4). Darunter
„Add mail account“ mit dem vorhandenen Weg (Adresse → Anbieter finden → Passwort → Dienste). Bietet der
Anbieter auch Kalender an, sagt der Schritt es und hängt sie an Schritt 4 („2 calendars added“).

### 7 Inbox

Die Quellen als Liste mit Schalter, vorbelegt nach dem, was schon verbunden ist: Nextcloud
Notifications, Nextcloud Talk, GitHub (Reviews, Erwähnungen), Mail. Dazu „Subscribe to ntfy topics“
(vorhandener Weg). Ein Satz zur Ruhe: „Quiet during busy events“ als Schalter.

### 8 AI

Zwei Gruppen, dann die Verwendung:

- **On this computer:** beim Öffnen sucht der Kern Ollama und LM Studio. Gefundene mit Zahl der
  Modelle und „Connect“ (ohne Schlüssel), andere grau „Not running“. Dazu „Other local server“
  (vLLM, llama.cpp, LocalAI: Adresse).
- **Cloud:** eine Zeile je Anbieter mit Name, Sitz und „Add key“. Ein Klick klappt in der Zeile ein
  maskiertes Feld mit Link zur Schlüsselseite des Anbieters auf; „Connect“ prüft den Schlüssel, indem es
  die Modelle holt. Reihenfolge nach Open First: offene Modelle, dann Anbieter.
- **Use for:** sobald ein Konto da ist: „Assistant“ mit Konto und Modell (Auswahl aus der Modellliste),
  später die vorgeschlagene Zuordnung je Funktion aus [ki.md](ki.md) mit einem Klick.

Ein Satz unter der Cloud-Gruppe: „Cloud providers receive what you send them. Mail goes to local
models only unless you change it.“

#### Anbieter-Katalog

| Gruppe | Anbieter | Protokoll | Sitz | Hinweis |
|---|---|---|---|---|
| Lokal | Ollama, LM Studio | OpenAI-kompatibel | dieser Rechner | gibt es (KI.1a) |
| Lokal | Other local server (vLLM, llama.cpp, LocalAI) | OpenAI-kompatibel | dieser Rechner/Netz | gibt es als `openai_compatible` |
| Selbst gehostet | Nextcloud Assistant | Nextcloud TaskProcessing | eigener Server | später, ohne neuen Schlüssel ([ki.md](ki.md), offen 3) |
| Offene Modelle | OpenRouter | OpenAI-kompatibel | USA | gibt es |
| Anbieter | Anthropic (Claude) | **Anthropic** | USA | gibt es (KI.1b–KI.1d) |
| Anbieter | OpenAI | OpenAI | USA | |
| Anbieter | Google Gemini | OpenAI-kompatibel | USA | |
| Anbieter | Mistral | OpenAI-kompatibel | EU | |
| Anbieter | xAI (Grok), Groq | OpenAI-kompatibel | USA | optional |
| Anbieter China | DeepSeek | OpenAI-kompatibel | China | offene Gewichte |
| Anbieter China | Qwen (Alibaba Cloud Model Studio) | OpenAI-kompatibel | China/Singapur | offene Gewichte, internationaler Endpunkt |
| Anbieter China | Kimi (Moonshot AI) | OpenAI-kompatibel | China | offene Gewichte |
| Anbieter China | GLM (Z.ai / Zhipu) | OpenAI-kompatibel | China | offene Gewichte |
| Anbieter China | MiniMax | OpenAI-kompatibel | China | |
| Sonstige | Other OpenAI-compatible server | OpenAI-kompatibel | – | gibt es |

- Je Anbieter stehen im Katalog des Kerns: Name, Standard-Adresse, Protokoll, Link zur Schlüsselseite,
  Sitz, ob offene Gewichte. Die Adressen werden bei der Umsetzung geprüft; bei den chinesischen
  Anbietern ist der **internationale** Endpunkt Standard, der chinesische eine Option unter „Advanced“.
- Der Sitz steht neutral in der Zeile („Cloud · EU“, „Cloud · China“), kein Warnbanner. Die Datenregel
  je Quelle ([ki.md](ki.md)) gilt für alle Cloud-Anbieter gleich.
- Bis auf Anthropic sprechen alle das OpenAI-Protokoll; ein neuer Anbieter ist ein Eintrag im Katalog,
  kein neuer Code.

### 9 Web search

Seit Issue #305 (Konzept: „Werkzeug Web“ in [assistent.md](assistent.md)). Nur, wenn die Komponente Assistent an
ist. Dieselbe Liste wie in Assistent › Einstellungen › Tools: je Anbieter ein Schalter, darunter wohin die
Suchwörter gehen („Search words go to duckduckgo.com · USA“), ein Link „Terms“ und bei DuckDuckGo und Brave der
Hinweis; bei Brave, Tavily, Exa und Mojeek ein maskiertes Feld für den Schlüssel, bei SearXNG das Feld für die
Adresse. Vorbelegt ist nichts (Aus). Wer einen Anbieter einschaltet, schaltet das Werkzeug `web` ein; der Haken
„Let the assistant use it“ (vorbelegt an) gibt dem Assistenten das Leserecht für `web`. Ein Satz darunter: „The
assistant asks once in each conversation before it searches.“ Überspringen lässt die Websuche aus.

Mit dem Schritt steigt `Setup::VERSION` auf 2; wer die Einrichtung schon abgeschlossen hat, sieht in der Overview
„New in setup“.

### 10 Done

Eine Zeile je gewählter Komponente mit Zustand und Kurzinfo („Calendar · 3 calendars“,
„Mail · Skipped“, „AI · Ollama, qwen3:8b“), daneben „Open“ für die App. Primär „Finish“ setzt den Stand
auf `done` und öffnet die Overview. Ein Satz: „Change any of this later in Settings.“
API und MCP gehören nicht in die Einrichtung; die Fertig-Seite verlinkt MCP leise („Connect an agent…“).

## Komponenten an und aus

Neu: eine Einstellung je Komponente (`calendar`, `tasks`, `mail`, `inbox`, `assistant`, `workspaces`),
Standard **an**, damit bestehende Installationen nichts merken.

Eine abgeschaltete Komponente
- erscheint nicht im App-Umschalter, im Tray-Menü und in der Overview; General › Apps zeigt sie mit
  Schalter „Off“ (dort schaltet man sie wieder ein),
- verursacht keine Hintergrundarbeit, die nur ihr dient (Mail-Abruf, Abfragen von Eingangsquellen,
  Spracherkennung des Assistenten),
- behält ihre Konten und Daten. Kalender und Aufgaben teilen sich Konten; ein Konto wird erst entfernt,
  wenn der Nutzer es unter Accounts entfernt.

Die Requests einer abgeschalteten Komponente antworten weiter (Entscheidung 1).

## API und MCP

Die Einrichtung selbst bekommt **keinen** eigenen Befehl: Sie ist eine Oberfläche über vorhandene
Befehle. Neu sind nur zwei Einstellungen und ein Feld.

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Stand der Einrichtung lesen | Start der Steuerzentrale, Overview | neues Feld `Settings::setup` über `settings.get_settings` · Read | `settings_get_settings` (gibt es) |
| Einrichtung abschließen / verschieben / neu starten | „Finish“, „Set up later“, General › „Run setup again“ | `Request::SetSetting { key: Setup }` · `settings.set_setting` · Admin | `settings_set_setting` (gibt es, Admin → Freigabe) |
| Komponente an/aus | Schritt 3, General › Apps | `Request::SetSetting { key: Component, value: {"component": "mail", "enabled": false} }` (Form wie `ListOrder`) · Admin | `settings_set_setting` |
| Dienste eines Nextcloud-Kontos | Schritt 2, 4, 5, 7; Kontoseite | neues optionales Feld `AccountInfo::services` über `accounts.list_accounts` · Read | `accounts_list_accounts` (gibt es) |
| Nextcloud verbinden | Schritt 2 | `ConnectNextcloud` · Admin | `accounts_connect_nextcloud` (gibt es; Anmeldung im Browser macht der Mensch) |
| CalDAV, Mail, GitHub-Token, ntfy, KI verbinden | Schritte 4–8 | vorhandene `Connect*` · Admin, nur App-Schlüssel | **Never** (Punkt 5: Geheimnisse nur über die Oberfläche) |
| GitHub per CLI | Schritt 5 | `ConnectGithub` · Admin | `accounts_connect_github` (gibt es) |
| Kalender/Listen zeigen | Schritte 4, 5 | `SetCollectionShown` · Admin | `settings_set_collection_shown` (gibt es) |
| Eingangsquellen | Schritt 7 | `inbox.list_sources`, `inbox.mute_source` | `inbox_list_sources`, `inbox_mute_source` (gibt es) |
| Lokale KI finden, Modelle | Schritt 8 | `FindAiServers`, `AiModels` · Read | `ai_find_local_servers`, `ai_list_models` (gibt es) |
| Modell des Assistenten | Schritt 8 | `SetSetting { key: AssistantDefault }` · Admin | `settings_set_setting` |
| Websuche: Anbieter, Adresse, Schlüssel, Recht des Assistenten | Schritt 9 | `tools.list_search_providers` · Read; `SetSetting { key: SearchProvider \| SearxngAddress \| ToolEnabled }` · Admin, einschalten nur Menschen; `accounts.set_search_key` · Admin, nur App-Schlüssel; `clients.update_client` | `tools_list_search_providers`; `settings_set_setting` nur fürs Ausschalten; Schlüssel und Rechte **Never** (Punkte 2, 5) |
| KI-Anbieter-Katalog | Schritt 8, Accounts › Add › AI | statisch in `torro_core::ai` (keine Daten, nur Beschreibung); Agenten brauchen ihn nicht, weil sie keine KI-Konten verbinden | – |

## Entscheidungen

Entschieden am 2026-10-14 (Vorschläge übernommen):

1. **Komponente aus** betrifft Sichtbarkeit und Hintergrundarbeit. Die Requests der Komponente
   antworten weiter, weil die Daten noch da sind.
2. **`AiProvider` bekommt je Anbieter eine Variante** (OpenAI, Gemini, Mistral, DeepSeek, Qwen, Kimi,
   GLM, MiniMax; Anthropic mit KI.1b). Neue Enum-Werte sind freigegeben (W.3).
3. **Chinesische Anbieter:** internationale Endpunkte als Standard, China-Endpunkte unter „Advanced“.
4. **Workspaces:** die Einrichtung zeigt nur, ob ein Tiling-Manager gefunden wurde. Die Wahl zwischen
   Tiling- und Fenster-Modus kommt mit [modes.md](modes.md).
5. **Kalender-Abos (ICS), Google und Microsoft** kommen später als weitere Zeilen, nicht in die erste
   Version.
