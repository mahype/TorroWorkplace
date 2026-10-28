# Kategorien eingehender Mail

Stand: 2026-10-28. Geplant zu [Issue #221](https://github.com/mahype/TorroWorkplace/issues/221), Gruppen neu
geordnet mit [Issue #338](https://github.com/mahype/TorroWorkplace/issues/338).
Ergänzt [eingang.md](eingang.md) (Spuren „Für mich“ und „Zur Info“) und [ki.md](ki.md)
(Grundsätze für KI). Aufgaben: [tasks/eingang.md › E12](tasks/eingang.md#e12-kategorien-eingehender-mail-issue-221).

Die Spur sagt, **ob** etwas auf mich wartet. Die Kategorie sagt, **was** es ist. Beides sind eigene
Dimensionen: An Spuren, Zählern, Tray-Zahl, Benachrichtigungen und gespeicherten Einträgen ändert
die Kategorie nichts.

## Der Katalog

Englischer Schlüssel ist maßgeblich, die deutsche Beschriftung steht in
`crates/torro-core/src/i18n/de.rs`. Fünf Gruppen, 23 Kategorien.

### Gruppe `waiting_for_me` – Wartet auf mich

| Schlüssel | Deutsch | Was es ist | Erkannt durch |
|---|---|---|---|
| `request` | Anfrage | Frage oder Bitte an mich, eine Antwort wird erwartet | Modell |
| `task` | Auftrag | Auftrag oder Zusage mit Frist, die ich einhalten muss | Modell |
| `approval` | Freigabe | Unterschrift, Bestätigung, Genehmigung verlangt | Modell |
| `security` | Sicherheit | Anmeldecode, Zwei-Faktor, Passwort zurücksetzen | Modell |
| `personal` | Persönlich | Nachricht eines Menschen, nichts zu tun | Modell |
| `invitation` | Einladung | Termineinladung | **Regel** (iMIP `text/calendar`) |
| `bounce` | Unzustellbar | Zustellungsbenachrichtigung, Fehlbericht (DSN); meist ist eine Adresse zu korrigieren | **Regel**, kommt mit E9.3a |

### Gruppe `paper_trail` – Belege

| Schlüssel | Deutsch | Was es ist | Erkannt durch |
|---|---|---|---|
| `invoice` | Rechnung | Rechnung, Mahnung, Zahlungsaufforderung | Modell |
| `receipt` | Zahlungsbeleg | Zahlung bestätigt, Quittung, Kassenbeleg | Modell |
| `order` | Bestellung | Bestell- und Auftragsbestätigung | Modell |
| `shipping` | Sendung | Versandbestätigung, Sendungsverfolgung, Zustellversuch | Modell |
| `booking` | Buchung | Reise, Hotel, Ticket, Terminbestätigung | Modell |
| `statement` | Abrechnung | Kontoauszug, Gehaltsabrechnung, Jahresabrechnung | Modell |
| `document` | Schriftstück | Vertrag, Police, Kündigung, Behördenpost | Modell |

### Gruppe `info` – Zur Info

| Schlüssel | Deutsch | Was es ist | Erkannt durch |
|---|---|---|---|
| `newsletter` | Newsletter | Redaktionelles, Abo, Blog | Modell |
| `promotion` | Werbung | Angebot, Rabatt, Produktwerbung | Modell |
| `service_notice` | Dienstmeldung | Wartung, geänderte Bedingungen, Statusbericht | Modell |
| `social` | Netzwerk | Soziale Netzwerke, Foren, Communitys | Modell |
| `event_invite` | Veranstaltung | Webinar, Konferenz, Einladung zu einer Veranstaltung | Modell |
| `auto_reply` | Abwesenheit | Automatische Antwort, Abwesenheitsnotiz | **Regel** (`Auto-Submitted`) |

### Gruppe `junk` – Unerwünscht (englisch „Unwanted“)

| Schlüssel | Deutsch | Was es ist | Erkannt durch |
|---|---|---|---|
| `spam` | Spam | Massenmail ohne Bezug zu mir | Modell, Junk-Ordner des Servers |
| `phishing` | Betrug | Täuschung, gefälschter Absender, Schadlink | Modell |

### Gruppe `unsorted` – Nicht eingeordnet (englisch „Unsorted“)

| Schlüssel | Deutsch | Was es ist | Erkannt durch |
|---|---|---|---|
| `other` | Sonstiges | nicht einzuordnen, oder das Modell ist zu unsicher | Rückfall |

Bis #338 hieß die Gruppe `junk` „Technik und Müll“ und hielt auch Unzustellbar, Abwesenheit und
Sonstiges. Unter der Schwelle 0,6 landet jede Mail in „Sonstiges“, auch eine echte persönliche
Mail, bei der das Modell nur unsicher war; darum steht sie in einer eigenen Gruppe zum gezielten
Durchsehen, nicht im Müll. Die Kategorie Spam heißt deutsch „Spam“, damit sie nicht wie ihre
Gruppe heißt.

Der **Schlüssel `junk` bleibt**, nur die Namen, die Menschen lesen, haben sich geändert: Eigene
Kategorien in `junk` und Agenten, die `junk` senden, gelten weiter. Neu ist nur der Wert
`unsorted`; ein neuer Wert ist für Clients verträglich, die API bleibt bei `v: 1`. Gespeicherte
Kataloge brauchen keine Umstellung: Der Dienst speichert die Gruppe einer mitgelieferten Kategorie
nur, wenn der Mensch sie geändert hat. Wer nichts geändert hat, bekommt die neue Zuordnung von
selbst, wer eine Kategorie verschoben hat, behält seine.

**Genau 20 Kategorien entscheidet das Modell**: alle außer `invitation`, `bounce` und `auto_reply`,
die aus den Kopfzeilen folgen. 20 ist auch die engste dokumentierte Obergrenze einer
`choice`-Frage (siehe „Das Protokoll“), der Katalog passt also in **einen** Aufruf.

## Woher eine Kategorie kommt

Jede Kategorie trägt ihre Herkunft (`CategorySource`), weil `docs/ki.md` Grundsatz 5 verlangt, dass
ein KI-Ergebnis als Vorschlag gekennzeichnet ist:

| Herkunft | Bedeutung | In der Oberfläche |
|---|---|---|
| `rule` | Aus einer Kopfzeile, dem Ordner oder einer gemerkten Absenderregel | Pille ohne Zusatz |
| `model` | Vom Entscheidermodell | Pille mit Vorschlags-Kennzeichen |
| `person` | Der Mensch hat sie von Hand gesetzt | Pille ohne Zusatz |

## Die Reihenfolge beim Einordnen

Wie beim Sortieren in Spuren ([eingang.md](eingang.md), Grundregel 6: erst feste Regeln, dann
lernen, KI höchstens als Vorschlag):

1. **Feste Merkmale** entscheiden, was in den Kopfzeilen steht: `invitation` (iMIP `REQUEST`),
   `auto_reply` (`Auto-Submitted` ungleich `no`), `bounce` (mit E9.3a), `spam` für Mail im
   Junk-Ordner des Servers. Herkunft `rule`.
2. **Gemerkte Regeln** je Absender und Domain (`RuleEffect::Category`), dieselbe Mechanik wie
   `inbox.hide_sender`: lesbar über `inbox.list_rules`, löschbar über `inbox.remove_rule`.
   Herkunft `rule`.
3. **Lokales Entscheidermodell** für den Rest: **ein** Aufruf über die SystemOne-/Jev-Schnittstelle
   mit **einer** `choice`-Frage über die 20 Kategorien aus Schritt „Der Katalog“ und zwei
   Zusatzfragen. Unter der Konfidenzschwelle bleibt es `other`. Herkunft `model`.
4. **Von Hand** setzt der Mensch jede Kategorie über `inbox.set_item_category`, mit der Rückfrage
   „Always for this sender?“ wie beim Umsortieren der Spur. Herkunft `person`. Eine Kategorie
   `person` fassen das Einordnen beim Eintreffen und der Altbestand **nie** an; nur ein
   ausdrückliches `inbox.classify_item` überschreibt auch sie – wer den Knopf drückt, will genau
   das.

**Wann:** beim Eintreffen jeder neuen Mail. Zusätzlich rechnet der Kern den **Altbestand** einmal
nach, begrenzt auf 30 Tage oder 500 Nachrichten je Konto, im Hintergrund und gedrosselt, mit
Fortschritt und Abbrechen auf der Einstellungsseite.

**Ohne KI-Konto** fehlen genau die Kategorien aus Schritt 3; kein Hinweisbanner, kein „Upgrade“
([ki.md](ki.md), Grundsatz 1).

## Das Protokoll: SystemOne/Jev

Ein Entscheidermodell schreibt nicht, es **entscheidet**: Ein Aufruf bekommt einen Zustand und
benannte Fragen und liefert je Frage Wahrscheinlichkeiten, die gewählte Option und eine Konfidenz.

- Endpunkt `POST <base>/systemone` (bei Ollama `http://localhost:11434/v1/systemone`).
- Fragetypen: `choice` (benannte Optionen mit Beschreibung), `noul` (ja/nein, die Antwort ist eine
  **Wahrscheinlichkeit** 0–1, kein Wahrheitswert), `score` (geordnete Stufen, die Antwort ist eine
  **Zahl**, auch gebrochen, mit Konfidenz).
- Grenzen: Ollama dokumentiert bis zu 64 Fragen und 2–26 Optionen je `choice`; die Jev-Referenz
  (jevmodel.org) nennt 1–8 Fragen, 2–20 Optionen, 2–10 Stufen und 8 000 Zeichen Zustand. **Torro
  hält die engeren Werte ein**, damit später auch `jeff-serve` und die Jev-Referenz bedient werden
  können, ohne dass der Aufruf umgebaut wird.
- Anbieter: **Ollama ≥ 0.35** zuerst. Ollama markiert taugliche Modelle in `/api/show` mit der
  Fähigkeit `decision` und reicht nur solche an `/v1/systemone` weiter. `jeff-serve` als eigener
  Anbieter kommt in einem zweiten Schritt.

### Die drei Fragen

Ein Aufruf je Mail stellt drei Fragen; das Protokoll erlaubt mehrere in einem Durchlauf.

| Name | Typ | Was sie fragt | Wie die Antwort gelesen wird |
|---|---|---|---|
| `category` | `choice` | Welche der 20 Kategorien? | `choice`, aber nur wenn `confidence` ≥ Schwelle, sonst `other` |
| `needs_reply` | `noul` | Wartet eine Antwort auf mich? | `noul` ist eine Wahrscheinlichkeit; ≥ 0,5 heißt ja |
| `urgency` | `score` | Routine / Soon / Urgent | `score` ist eine Zahl 0–2, gerundet auf die nächste Stufe |

`needs_reply` und `urgency` sind **Beiwerk**: Sie ändern weder Spur noch „dringend“. „Dringend“ im
Sinne des Eingangs bleibt eine feste, kleine Menge ([eingang.md](eingang.md)). Beide werden
trotzdem **mit der Mail gespeichert**, obwohl sie heute niemand liest: Derselbe Aufruf beantwortet
sie ohnehin, und sie später nachzurüsten hieße, das Modell über den ganzen Altbestand noch einmal
laufen zu lassen. Sie liegen in zwei eigenen Spalten neben der Kategorie und erscheinen in keiner
Antwort der API, solange sie niemand liest.

### Konfidenzschwelle

**0,6, fest im Code, keine Einstellung** (Entscheidungen statt Optionen). Unter der Schwelle steht
`other`. Der Wert ist **nicht an echten Mails gemessen** – das war in der Planung nicht möglich, da
dafür ein eingerichtetes Entscheidermodell und ein echtes Postfach nötig sind. Nachjustiert wird er
in einem eigenen Schritt, an den Fixtures unter `crates/torroworkplace/tests/fixtures/` und an
echten Mails.

## Datenregel und Sicherheit

- Solange `mail_to_cloud` aus ist, gehen Mails nur an Modelle **auf diesem Rechner**
  (`AiModel::remote`, `data_rule::place_of`, [ki.md](ki.md) Grundsatz 4). Ein Entscheidermodell in
  der Cloud wird für Mail gar nicht angeboten.
- Kein Training, keine gesammelten Mails. Das Protokoll (`audit.jsonl`) hält nur Funktion, Konto,
  Modell, Tokens und Dauer – nie Betreff, Absender oder Text.
- Nichts wird von einer KI versteckt, verschoben, gelöscht oder beantwortet. Die Kategorie ist ein
  Vorschlag mit Kennzeichnung, das Original bleibt sichtbar.
- Der Text der Mail ist Fremdtext: Er steht im `state` zwischen `<message>` und `</message>`, eine
  schließende Marke darin wird aufgebrochen, und nichts daraus wird je als Anweisung gelesen.
  Dasselbe Verfahren wie bei `inbox.suggest_task_from_item` (`crate::suggest`).
- Keine serverseitigen Filter, keine Junk-Automatik, kein Verschieben in den Junk-Ordner.

## Phishing

`phishing` ist der einzige Fall mit einem sichtbaren Eingriff, und der besteht nur aus einem
zusätzlichen Klick:

- ein deutliches Band oben in der Nachricht,
- die Links sind entschärft: nicht mit einem Klick erreichbar, das Ziel steht im Klartext daneben,
  Öffnen erst nach einer Bestätigung.

Versteckt, verschoben oder gelöscht wird nichts.

## Nur Mail

Einträge von GitHub, Talk und ntfy lassen die Kategorie leer. Sie haben mit ihren Facets (`reason`,
`result`, `kind`) schon genauere Merkmale, und ein Modellaufruf wäre dort verschenkt.

## Die drei Wege

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Kategorie sehen | Pille in Liste, Leser, Paneel | Feld `category` in `inbox.list_items`, `inbox.get_item`, `mail.search_messages`, `mail.get_message` · Read; eine Kategorie mit `source: model` ist ein **Vorschlag mit Konfidenz**, keine Tatsache | über die vorhandenen Tools |
| Nach Kategorie filtern | Gruppe „Categories“ der Seitenleiste | Feld `categories` in `inbox.list_items` und `mail.search_messages` · Read | über die vorhandenen Tools |
| Katalog der Kategorien | Seitenleiste, Menü „Category“ | `inbox.list_categories` · Read; die Zahlen zählen nur Konten, die der Schlüssel lesen darf | `inbox_list_categories` |
| Kategorie ändern, „immer so“ | Menü an Zeile und Leser | `inbox.set_item_category` · Write, mit „merken“ Admin | `inbox_set_item_category` |
| Neu einordnen lassen | Knopf im Leser | `inbox.classify_item` · Write | `inbox_classify_item` |
| Stand der Kategorisierung | Einstellungen › Categorization | `inbox.get_categorization` · Read | `inbox_get_categorization` |
| Altbestand nachrechnen, abbrechen | Einstellungen › Categorization | `inbox.catch_up_categories` · Admin (bis zu 500 Modellaufrufe je Konto) | `inbox_catch_up_categories` |
| Beliebigen Umfang zählen | Einstellungen › Categorization | `inbox.preview_category_job` · Read; alle lokal bekannten Mails, gelesene eingeschlossen | `inbox_preview_category_job` |
| Sortierauftrag starten und verfolgen | Einstellungen › Categorization | `inbox.start_category_job` · Write, `inbox.get_category_job` · Read | `inbox_start_category_job`, `inbox_get_category_job` |
| Auftrag pausieren, fortsetzen, abbrechen | Einstellungen › Categorization | `inbox.control_category_job` · Write | `inbox_control_category_job` |
| Alle Kategorien vorab zählen | Einstellungen › Categorization | `inbox.preview_clear_categories` · Read | `inbox_preview_clear_categories` |
| Kategorien aller Mails entfernen | Einstellungen › Categorization | `inbox.clear_categories` · Delete, mit expliziter All-Konten-Bestätigung und optionaler Deaktivierung der Automatik | `inbox_clear_categories` |
| Entfernen pausieren, fortsetzen, abbrechen | Einstellungen › Categorization | `inbox.control_clear_categories` · Delete | `inbox_control_clear_categories` |
| Entscheidermodelle anbieten | Einstellungen › Categorization | `ai.list_models` um das Feld `decision` erweitert · Read | vorhanden |
| Modell je Funktion lesen | Einstellungen › Categorization | `ai.list_assignments` · Read | `ai_list_assignments` |
| Modell je Funktion setzen | Einstellungen › Categorization | `ai.set_assignment` · Admin | `ai_set_assignment` |

Kein Befehl braucht `Never`: Es sind keine Geheimnisse beteiligt, Schlüssel bleiben wie bisher im
Dienst und werden nur über die Oberfläche eingetragen.

## Eigene Kategorien und Sortierregeln (Issue #255)

Stand: 2026-10-21. Aufgaben: [tasks/eingang.md › E15](tasks/eingang.md#e15-kategorien-und-sortierregeln-frei-bearbeiten-issue-255).
Der Katalog oben ist seitdem die **Vorgabe**, nicht mehr fest.
Die Darstellung mit Gruppenfarben und individuellen Kategorie-Icons gehört zu
[Issue #265](https://github.com/mahype/TorroWorkplace/issues/265).

### Kennung und Angaben

- Jede Kategorie hat eine **Kennung, die nie wechselt**: die 23 Schlüssel oben und für eigene
  Kategorien `custom_<n>`. `<n>` wird nie neu vergeben, auch nicht nach dem Entfernen.
- Bearbeitbar sind bei **jeder** Kategorie Name, Gruppe, Beschreibung und „das Modell darf sie
  wählen“. Umbenennen ändert die Kennung nicht, Mail und Regeln bleiben gültig. Eine mitgelieferte
  Kategorie lässt sich auf die Worte der Suite zurücksetzen, aber nicht entfernen.
- Die **fünf Gruppen** bleiben fest und erhalten je eine Farbe aus der Gruppenpalette des aktiven
  Designs. Jede Unterkategorie hat ein einfarbiges Icon in der Farbe ihrer Gruppe. Die 23
  mitgelieferten Kategorien haben feste Standard-Icons; bei eigenen Kategorien kann das Icon
  gewählt werden. Die Icon-Auswahl ist auch für mitgelieferte Kategorien verfügbar. Gruppenname
  und Einrückung zeigen die Hierarchie zusätzlich zur Farbe.
- Die **Beschreibung** ist das, was das Modell über die Kategorie liest – mit Beispielen und
  Ausnahmen in den Worten des Menschen. Sie lenkt das Modell, garantiert aber nichts: Eine harte
  Ausnahme ist eine Sortierregel.

### Sortierregeln

Eine Regel nennt eine Kategorie, **Bedingungen** (alle müssen gelten) und **Ausnahmen** (keine darf
gelten). Felder: Absender (gleich), Domain (gleich oder Subdomain), Betreff (enthält), Text
(enthält, so weit die Suite den Text hat). Groß- und Kleinschreibung zählt nicht. Die Regeln stehen
in einer sichtbaren Reihenfolge; **die erste passende entscheidet**, abgeschaltete zählen nicht.

Die gemerkten Regeln aus „Always for this sender?“ sind Sortierregeln mit einer Bedingung
„Absender“ und stehen in derselben Liste. Über `inbox.list_rules` und `inbox.remove_rule` sind sie
weiter erreichbar.

### Vorrang

1. **Von Hand** gesetzt (`person`): bleibt, bis der Mensch selbst etwas anderes wählt oder
   ausdrücklich `inbox.classify_item` drückt.
2. **Sortierregeln** in ihrer Reihenfolge (`rule`).
3. **Feste Merkmale**: Einladung, Abwesenheit, Junk-Ordner (`rule`).
4. **Modell** (`model`), nur für Kategorien, die es wählen darf.

### Mehr als 20 Kategorien fürs Modell

Eine `choice`-Frage hat höchstens 20 Optionen. Bis 20 Modellkategorien bleibt es bei einem
Aufruf. Darüber fragt der erste Aufruf die **Gruppe** (dazu `needs_reply` und `urgency`), der
zweite die **Kategorie** in dieser Gruppe; mit nur einer Kategorie in der Gruppe entfällt er. Je
Gruppe sind höchstens 20 Modellkategorien erlaubt, darum fällt keine stillschweigend weg. Die
Schwelle 0,6 gilt für beide Antworten.

### Entfernen und neu einordnen

- Eine eigene Kategorie ohne Mail, Einträge und Regeln verschwindet sofort. Sonst braucht es eine
  **Ersatzkategorie**: Der Dienst merkt sie sich, stellt Regeln, Mail und Einträge um und entfernt
  die Kategorie zuletzt. Bricht das ab, setzt der nächste Start es fort; die Herkunft der
  Kategorie bleibt dabei erhalten.
- Neue Regeln und geänderte Beschreibungen wirken auf **neue** Mail. Bestehende Mail ordnet
  „Sort again“ neu (`inbox.catch_up_categories` mit `again`), über dasselbe Fenster wie der
  Altbestand, mit Fortschritt und Abbrechen. Von Hand gesetzte Kategorien bleiben.

### Die drei Wege

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Katalog lesen | Einstellungen › Categorization, Seitenleiste, Menü | `inbox.list_categories` · Read | `inbox_list_categories` |
| Kategorie anlegen | Einstellungen › Categorization | `inbox.create_category` · Admin | `inbox_create_category` |
| Kategorie ändern | Einstellungen › Categorization | `inbox.update_category` · Admin | `inbox_update_category` |
| Kategorie entfernen | Einstellungen › Categorization | `inbox.remove_category` · Admin | `inbox_remove_category` |
| Regeln lesen | Einstellungen › Categorization | `inbox.list_sort_rules` · Read | `inbox_list_sort_rules` |
| Regel anlegen, ändern, entfernen | Einstellungen › Categorization | `inbox.create_sort_rule`, `inbox.update_sort_rule`, `inbox.remove_sort_rule` · Admin | gleichnamig |
| Reihenfolge ändern | Einstellungen › Categorization | `inbox.move_sort_rule` · Admin | `inbox_move_sort_rule` |
| Neu einordnen | Einstellungen › Categorization | `inbox.catch_up_categories { again }` · Admin | `inbox_catch_up_categories` |

Admin, weil jede dieser Änderungen dauerhaft bestimmt, wie alle Mail eingeordnet wird – wie
„Always for this sender?“. Ein Agent braucht dafür die Freigabe eines Menschen.

## Abgrenzung

- Nur Mail bekommt Kategorien.
- `jeff-serve` kommt nicht als eigener Anbieter dazu.
- Kein Umbau der Listendarstellung über Pille, Seitenleisteneinträge, Menü und Einstellungsseite
  hinaus.
- Die dritte Spur „Belege“ gibt es nicht; „Paper trail“ ist eine Gruppe der Seitenleiste.

## Quellen

- [Ollama: decision capability](https://docs.ollama.com/capabilities/decision) – Endpunkt,
  Fragetypen, Grenzen (64 Fragen, 2–26 Optionen), `/api/show` mit `decision`, Ollama 0.35
- [Ollama 0.35: Jev-style decision models](https://ollama.com/blog/ollama-now-supports-jev-style-decision-models)
- [Jev-API-Doku](https://jevmodel.org/docs/) – Referenzgrenzen (1–8 Fragen, 2–20 Optionen,
  2–10 Stufen, 8 000 Zeichen Zustand), Antwortfelder je Fragetyp
- [Jeff](https://github.com/firelex/jeff) – offenes Entscheidermodell, zweiter Schritt
