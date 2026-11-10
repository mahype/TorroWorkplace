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

### Gruppe `waiting_for_me` – Persönlich

| Schlüssel | Deutsch | Was es ist | Erkannt durch |
|---|---|---|---|
| `request` | Anfrage | Frage oder Bitte an mich, eine Antwort wird erwartet | Modell |
| `task` | Auftrag | Auftrag oder Zusage mit Frist, die ich einhalten muss | Modell |
| `approval` | Freigabe | Unterschrift, Bestätigung, Genehmigung verlangt | Modell |
| `security` | Sicherheit | Anmeldecode, Zwei-Faktor, Passwort zurücksetzen | Modell |
| `personal` | Von Menschen | Nachricht eines Menschen, nichts zu tun | Modell |
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
| `other` | Sonstiges | nicht einzuordnen, das Modell ist zu unsicher oder nennt keine Kategorie (#398) | Rückfall |

Bis #338 hieß die Gruppe `junk` „Technik und Müll“ und hielt auch Unzustellbar, Abwesenheit und
Sonstiges. Unter der Schwelle 0,6 landet jede Mail in „Sonstiges“, auch eine echte persönliche
Mail, bei der das Modell nur unsicher war; darum steht sie in einer eigenen Gruppe zum gezielten
Durchsehen, nicht im Müll. Die Gruppe hält nur „Sonstiges“ (#402): Der Dienst lehnt es ab, eine eigene oder
eine andere eingebaute Kategorie dorthin anzulegen oder zu verschieben (`error`); eine, die schon dort liegt, bleibt
und lässt sich weiter ändern. Die Kategorie Spam heißt deutsch „Spam“, damit sie nicht wie ihre
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
| `rule` | Aus einer Kopfzeile, dem Ordner oder einer gemerkten Absenderregel | Gefüllte Pille, kein Tooltip |
| `model` | Vom Entscheidermodell | Gefüllte Pille, Tooltip „… · Vorschlag“ |
| `person` | Der Mensch hat sie von Hand gesetzt | Gefüllte Pille, kein Tooltip |

Die Pille ist seit Issue #613 in der Gruppenfarbe gefüllt, mit weißem Icon und Wort, und steht im Lesebereich, auf
den Karten und in der ersten Zeile der Mail-Liste des Mail-Fensters (bei wenig Platz nur als Feld mit dem Icon). Ein
Vorschlag ist nur noch im Tooltip erkennbar, damit die Pille auf jedem Grund lesbar bleibt.

## Die Reihenfolge beim Einordnen

Wie beim Sortieren in Spuren ([eingang.md](eingang.md), Grundregel 6: erst feste Regeln, dann
lernen, KI höchstens als Vorschlag). Die Reihenfolge ist fest: feste Regeln (gemerkte zuerst, siehe
[Vorrang](#vorrang)), feste Merkmale, Kontakte, das Modell, zuletzt `other`. Passt keine Regel und ist das Modell nicht sicher genug, bleibt die Mail `other`.

1. **Feste Merkmale** entscheiden, was in den Kopfzeilen steht: `invitation` (iMIP `REQUEST`),
   `auto_reply` (`Auto-Submitted` ungleich `no`), `bounce` (mit E9.3a), `spam` für Mail im
   Junk-Ordner des Servers. Herkunft `rule`.
2. **Gemerkte Regeln** je Absender und Domain (`RuleEffect::Category`): lesbar über
   `inbox.list_rules`, löschbar über `inbox.remove_rule`. Herkunft `rule`.
3. **Kontakte** (Issue #390): Steht die Absenderadresse in einem Adressbuch, wird die Mail
   `personal` („Von Menschen“), Herkunft `rule`, auch ohne Modell. Ein Modell wählt danach nur noch in
   der Gruppe von `personal` oder `phishing` (siehe [Vorrang](#vorrang)).
4. **Modell** für den Rest, gewählt über den Platz *Sortieren* ([ki.md](ki.md), Abschnitt „Plätze“) oder
   eine Ausnahme je Funktion: **ein** Aufruf über die SystemOne-/Jev-Schnittstelle mit **einer**
   `choice`-Frage über die 20 Kategorien aus Schritt „Der Katalog“ und zwei Zusatzfragen. Gewählt
   werden kann ein **Entscheidermodell** (Ollama, auf diesem Rechner oder mit Erlaubnis als
   `:cloud`-Modell), **Jev** in der Cloud (Issue #323) oder ein **Chat-Modell**. Ein Entscheidermodell antwortet direkt; unter der Konfidenzschwelle bleibt
   es `other`. Ein **Chat-Modell** antwortet über den Ersatzweg (`docs/ki.md`, „Ersatzweg beim Sortieren“): streng
   gelesen, ohne Konfidenz, eine nicht angebotene Kategorie wird `other`. Herkunft `model`.
5. **Von Hand** setzt der Mensch jede Kategorie über `inbox.set_item_category`, mit der Rückfrage
   „Always for this sender?“ wie beim Umsortieren der Spur. Herkunft `person`. Eine Kategorie
   `person` fassen das Einordnen beim Eintreffen und der Altbestand **nie** an; nur ein
   ausdrückliches `inbox.classify_item` überschreibt auch sie – wer den Knopf drückt, will genau
   das.

Vor Schritt 1 stehen die festen Regeln einer Kategorie: Sortierregeln und Mail-Filter, die sie setzen.
Passen Regeln mehrerer Kategorien, gewinnt seit Issue #481 die Kategorie, die in der Liste weiter oben
steht; gemerkte Regeln „Always for this sender?“ gehen allen anderen vor (siehe [Vorrang](#vorrang)).

Zwischen Schritt 3 und 4 steht seit Issue #506 das **Abfangen von Betrug und Spam** (siehe
[Phishing und Betrug abfangen](#phishing-und-betrug-abfangen)): Neue Mail eines Fremden, die bis
dahin nichts eingeordnet hat, wird ohne Modell auf Warnzeichen geprüft; reichen sie, fragt ein enger
Modellaufruf „Betrug oder Spam?“, bevor die Mail eingetragen wird.

**Wann:** beim Eintreffen jeder neuen Mail. Was unter Einstellungen › Categorization steht, gilt immer
für jede neue Mail; **vorhandene Mail wird nur sortiert, wenn der Mensch das ausdrücklich anstößt**
(„Sort existing mail…“, siehe [Vorhandene Mail sortieren](#vorhandene-mail-sortieren), Issue #454).

**Tempo (Issue #420):** Altbestand und Sortieraufträge fragen ein **Decision-Modell** ohne Pause:
Jev 8 Mails gleichzeitig, ein Cloud-Modell von Ollama 4, jedes andere eine Mail nach der anderen
(ein lokales Ollama rechnet je Modell eine Anfrage zugleich, parallel war es langsamer). Die Antworten
werden in der Reihenfolge der Mails geschrieben; Pausieren und Abbrechen verwerfen, was noch
unterwegs ist. Der **Ersatzweg** über ein Chat-Modell bleibt bei einer Mail nach der anderen mit
250 ms Pause, damit der Rechner nicht ausgelastet ist. Ist das Modell beschäftigt (HTTP 429, 402),
wartet der Lauf 1, 2 und 4 Sekunden und fragt erneut. Lehnt der Server eine Mail selbst ab (HTTP 400,
413, 422, etwa zu lang für den Kontext eines kleinen Modells), bleibt sie wie eine Mail ohne lesbare
Antwort ohne Kategorie; erst 10 solche Mails in Folge stoppen den Lauf.

**Ohne KI-Konto** fehlen genau die Kategorien aus Schritt 4; kein Hinweisbanner, kein „Upgrade“
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
  Fähigkeit `decision` und reicht nur solche an `/v1/systemone` weiter. `jeff-serve` kommt nicht als
  eigener Anbieter dazu (siehe [Abgrenzung](#abgrenzung)).
- Anbieter **Jev** von TypeSafe (Issue #323): gehostet in den USA („Cloud · US“), geschlossene Gewichte.
  Endpunkt `POST https://api.typesafe.ai/v1/systemone`, Modell `jev-latest`, Schlüssel als
  `Authorization: Bearer`, angelegt in der Konsole (`https://console.typesafe.ai/keys`, zurzeit Early
  Access mit Warteliste). Preis laut TypeSafe 0,042 $ je Million Eingabe-Tokens, Ausgabe frei; bei rund
  1.700 Tokens je Mail etwa 0,15 $ je 2.000 Mails. Jev listet keine Modelle: Torro kennt das eine Modell
  fest und prüft den Schlüssel beim Verbinden mit einer kleinen Probe-Entscheidung. Jev sortiert nur,
  chatten kann man damit nicht. Der Filter für offene Modelle versteckt Jev nicht, weil ein
  Entscheidermodell keinen Text schreibt; in der Auswahl steht Jev nach den Modellen auf diesem
  Rechner.

### Die drei Fragen

Ein Aufruf je Mail stellt drei Fragen; das Protokoll erlaubt mehrere in einem Durchlauf.

| Name | Typ | Was sie fragt | Wie die Antwort gelesen wird |
|---|---|---|---|
| `category` | `choice` | Welche der 20 Kategorien? | `choice`, aber nur wenn `confidence` ≥ Schwelle, sonst `other`; ein Chat-Modell (Ersatzweg) gibt keine Konfidenz, dort zählt jede angebotene Kategorie |
| `needs_reply` | `noul` | Wartet eine Antwort auf mich? | `noul` ist eine Wahrscheinlichkeit; ≥ 0,5 heißt ja |
| `urgency` | `score` | Routine / Soon / Urgent | `score` ist eine Zahl 0–2, gerundet auf die nächste Stufe |

`needs_reply` und `urgency` sind **Beiwerk**: Sie ändern weder Spur noch „dringend“. „Dringend“ im
Sinne des Eingangs bleibt eine feste, kleine Menge ([eingang.md](eingang.md)). Beide werden
trotzdem **mit der Mail gespeichert**, obwohl sie heute niemand liest: Derselbe Aufruf beantwortet
sie ohnehin, und sie später nachzurüsten hieße, das Modell über den ganzen Altbestand noch einmal
laufen zu lassen. Sie liegen in zwei eigenen Spalten neben der Kategorie und erscheinen in keiner
Antwort der API, solange sie niemand liest.

**Fragen nach Aktionen (#603).** Derselbe Aufruf trägt je eingeschalteter Art eines Vorschlags
eine Ja/Nein-Frage mehr (`action:event`, `action:task`, ≥ 0,5 heißt ja). Was angeschlagen hat,
liest ein Chat-Modell danach aus. Mail, die eine Regel ohne Modell einsortiert, bekommt nur diese
Fragen; Mail der Gruppen „Zur Info“ und „Unerwünscht“ gar keine. Einzelheiten:
[vorschlaege.md](vorschlaege.md).

### Konfidenzschwelle

**0,6, fest im Code, keine Einstellung** (Entscheidungen statt Optionen). Unter der Schwelle steht
`other`. Sie gilt nicht für ein Chat-Modell über den Ersatzweg, das keine Konfidenz angibt. Der Wert ist
**nicht an echten Mails gemessen** – das war in der Planung nicht möglich, da
dafür ein eingerichtetes Entscheidermodell und ein echtes Postfach nötig sind. Nachjustiert wird er
in einem eigenen Schritt, an den Fixtures unter `crates/torroworkplace/tests/fixtures/` und an
echten Mails.

## Datenregel und Sicherheit

- Solange die Erlaubnis des Platzes *Sortieren* aus ist (`decide_mail_to_cloud`, [ki.md](ki.md), Abschnitt
  „Erlaubnis je Platz“), gehen Mails nur an Modelle **auf diesem Rechner** (`AiModel::remote`,
  `data_rule::place_of`, [ki.md](ki.md) Grundsatz 4). Modelle in der Cloud stehen trotzdem in der Auswahl
  unter Einstellungen › AI, Platz *Sorting* (seit #545 der einzige Ort), getrennt unter „Elsewhere“ und mit Wolke
  (#424); eins zu wählen fragt vorher („Send mail outside this computer?“), und erst „Use“ gibt die Erlaubnis. Ein lokales Modell nimmt sie zurück.
  **Jev ist ein Cloud-Modell**: Es ist nach dem Verbinden aus wie jedes Cloud-Modell und bekommt Mail nur
  mit dieser Erlaubnis.
  „Automatic“ an einem Konto auf
  diesem Rechner nimmt nur Modelle hier, auch bei eingeschalteter Erlaubnis ([ki.md](ki.md), „Erlaubnis je
  Platz“): ein `:cloud`-Modell des lokalen Ollama sortiert nur, wenn ein Mensch es ausdrücklich wählt.
- Kein Training, keine gesammelten Mails. Das Protokoll (`audit.jsonl`) hält nur Funktion, Konto,
  Modell, Tokens und Dauer – nie Betreff, Absender oder Text.
- Nichts wird von einer KI verschoben, gelöscht oder beantwortet. Die Kategorie ist ein Vorschlag mit
  Kennzeichnung, das Original bleibt sichtbar. Einzige Ausnahme vom Ausblenden ist abgefangener Betrug und
  Spam (Issue #506): Er steht nicht in „All“ und im Eingang, aber jederzeit unter Unerwünscht › Betrug
  bzw. Spam, und „Kein Betrug“ holt ihn zurück.
- Die Warnzeichen werden ohne Modell und ohne Netz berechnet; sie gehen nur an das Modell, das ohnehin
  sortiert, unter derselben Erlaubnis. Das Protokoll nennt beim Abfangen und bei „Kein Betrug“ nur Konto
  und Anzahl, nie Absender oder Betreff.
- Der Text der Mail ist Fremdtext: Er steht im `state` zwischen `<message>` und `</message>`, eine
  schließende Marke darin wird aufgebrochen, und nichts daraus wird je als Anweisung gelesen.
  Dasselbe Verfahren wie bei `inbox.suggest_task_from_item` (`crate::suggest`).
- Keine serverseitigen Filter, kein Verschieben in den Junk-Ordner: abgefangene Mail bleibt auf dem Server
  im Posteingang (Issue #506, Entscheidung 1). Auf dem Handy und im Webmail ist sie darum weiter sichtbar.

## Phishing und Betrug abfangen

`phishing` hat immer einen sichtbaren Eingriff, der nur aus einem zusätzlichen Klick besteht:

- ein deutliches Band oben in der Nachricht, mit den Warnzeichen, die zutrafen, und „Kein Betrug“,
- die Links sind entschärft: nicht mit einem Klick erreichbar, das Ziel steht im Klartext daneben,
  Öffnen erst nach einer Bestätigung.

Seit Issue #506 fängt Torro sicheren Betrug und Spam ab, bevor er im Eingang ankommt:

1. **Warnzeichen ohne Modell** (`mail/warnings.rs`): bei jeder neuen Mail eines Fremden, die kein Filter,
   keine Regel und keine Kopfzeile eingeordnet hat. Gewicht 3: Name nennt eine Marke, die Adresse gehört
   nicht dazu; Domain sieht einer Marke nur ähnlich; DMARC durchgefallen; Link-Text zeigt ein anderes
   Ziel; Name eines Kontakts mit fremder Adresse; Anhang, der Programme ausführt (`.html`, `.iso`, `.exe`
   …) oder zwei Endungen hat. Gewicht 2: SPF oder DKIM durchgefallen; „Antworten an“ an eine andere
   Domain; Freemail mit Markennamen; Link auf IP-Adresse oder URL-Kürzer; Punycode; Druck („Konto
   prüfen“, „Sperrung“, Frist); Zahlungs- oder Kontoänderung, Chef-Masche. Gewicht 1: keine
   Absenderprüfung; anderer Envelope-Absender (`Return-Path`); ungewöhnliche Endung (`.zip`, `.top`
   …); gepackter Anhang; nur ein Bild; keine persönliche Anrede; erster Kontakt; „Re:“ ohne Antwortbezug.
   Ab der Summe 3 ist die Mail verdächtig. Fakten, die nicht gelesen wurden (Mail von vor dem Update
   hat keine Absenderprüfung und keinen Antwortbezug), geben kein Zeichen.
2. **Enger Betrugs-Aufruf** an das Modell, das sortiert (`categorize::judge_request`): zwei Ja-Nein-Fragen
   „Betrug?“ und „Spam?“ mit den Warnzeichen als Worten der Suite außerhalb des Zauns. Die verdächtige
   Mail einer Runde wartet darauf zusammen höchstens eine Minute, ohne Eintrag und ohne
   Benachrichtigung; was dann noch offen ist, kommt ohne Urteil an. Gelesen wird dafür nur, was der
   Speicher hält (Text und HTML, sonst Kopfzeilen und Anfang), nie der Server.
3. **Wirkung:** ab 90 % Betrug (`SURE`) wird sie `phishing`, ab 90 % Spam `spam`, und sie ist
   **abgefangen** (`MessageSummary::caught`): nicht in „All“, nicht in Zahlen, Eingang und
   Benachrichtigungen, aber unter Unerwünscht › Betrug bzw. Spam mit Band und entschärften Links. Ab
   50 % Betrug (`UNSURE`) bleibt sie im Eingang, als `phishing` mit Band. Sonst, bei Fehler oder ohne
   Antwort wird sie eingetragen wie jede andere. Ohne Sortier-Modell fängt Torro nichts ab.
4. **„Kein Betrug“** (bzw. „Kein Spam“) holt eine Mail zurück: nicht mehr abgefangen, ohne die Kategorie
   (eine von Hand gesetzte bleibt, sie ändert der Mensch im Kategorie-Menü),
   als neuer Eintrag ohne Benachrichtigung, das Modell ordnet sie neu ein, und ihr Absender gilt ab jetzt
   als vertraut (`RuleEffect::Trusted`, zu sehen und zu entfernen auf den Seiten Spam und Betrug unter
   Einstellungen › Categorization).

**Bekannte werden nie abgefangen**, egal welche Warnzeichen zutreffen: Adressbuch-Kontakte (auch wenn
„Mail von Kontakten ist persönlich“ aus ist), Adressen, an die ein Konto geschrieben hat (`To`/`Cc` im
Gesendet-Ordner), Antworten in Unterhaltungen mit einer Mail aus dem Gesendet-Ordner (Mail im eigenen
Namen in anderen Ordnern zählt nicht, sie kann gefälscht sein) und mit „Kein Betrug“ gemerkte Absender.
Die eigene Adresse zählt nur, solange ihre Absenderprüfung (SPF, DMARC) nicht durchfällt: Die
Erpressungsmasche „Ich habe dein Konto gehackt“ schickt in ihrem Namen. Ein Bekannter, dessen Adresse
gefälscht wird, bleibt erkennbar wie bisher (Issue #390): Das Modell wählt nur zwischen `personal` und
`phishing`, ein Treffer bleibt im Eingang mit Band.

Gelöscht wird nichts, und auf dem Server wird nichts verschoben.

## Nur Mail

Einträge von GitHub, Talk und ntfy lassen die Kategorie leer. Sie haben mit ihren Facets (`reason`,
`result`, `kind`) schon genauere Merkmale, und ein Modellaufruf wäre dort verschenkt.

## Die drei Wege

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Kategorie sehen | Pille in Liste, Leser, Paneel | Feld `category` in `inbox.list_items`, `inbox.get_item`, `mail.search_messages`, `mail.get_message` · Read; eine Kategorie mit `source: model` ist ein **Vorschlag mit Konfidenz**, keine Tatsache | über die vorhandenen Tools |
| Nach Kategorie filtern | Gruppen der Kategorien oben in der Seitenleiste (seit #337 ohne Überschrift „Categories“) | Feld `categories` in `inbox.list_items` und `mail.search_messages` · Read | über die vorhandenen Tools |
| Katalog der Kategorien | Seitenleiste, Menü „Category“ | `inbox.list_categories` · Read; die Zahlen zählen nur Konten, die der Schlüssel lesen darf | `inbox_list_categories` |
| Kategorie ändern, „immer so“ | Menü an Zeile und Leser | `inbox.set_item_category` · Write, mit „merken“ Admin | `inbox_set_item_category` |
| Neu einordnen lassen | Knopf im Leser | `inbox.classify_item` · Write | `inbox_classify_item` |
| Stand der Kategorisierung | Einstellungen › Categorization | `inbox.get_categorization` · Read | `inbox_get_categorization` |
| Altbestand nachrechnen, abbrechen | Einstellungen › Categorization | `inbox.catch_up_categories` · Admin (bis zu 500 Modellaufrufe je Konto) | `inbox_catch_up_categories` |
| Beliebigen Umfang zählen | Einstellungen › Categorization | `inbox.preview_category_job` · Read; alle lokal bekannten Mails, gelesene eingeschlossen | `inbox_preview_category_job` |
| Sortierauftrag starten und verfolgen | Einstellungen › Categorization | `inbox.start_category_job` · Write (ein genanntes Modell anderswo für Clients nur das, mit dem gerade sortiert wird; ein Lauf ohne eigenes Modell hält an, sobald die Person fürs Sortieren ein anderes wählt, und nimmt es beim Fortsetzen), `inbox.get_category_job` · Read | `inbox_start_category_job`, `inbox_get_category_job` |
| Auftrag pausieren, fortsetzen, abbrechen | Einstellungen › Categorization | `inbox.control_category_job` · Write (Fortsetzen mit einem Modell anderswo für Clients nur, solange damit gerade sortiert wird) | `inbox_control_category_job` |
| Alle Kategorien vorab zählen | Einstellungen › Categorization | `inbox.preview_clear_categories` · Read | `inbox_preview_clear_categories` |
| Kategorien aller Mails entfernen | Einstellungen › Categorization | `inbox.clear_categories` · Delete, mit expliziter All-Konten-Bestätigung und optionaler Deaktivierung der Automatik | `inbox_clear_categories` |
| Entfernen pausieren, fortsetzen, abbrechen | Einstellungen › Categorization | `inbox.control_clear_categories` · Delete | `inbox_control_clear_categories` |
| Entscheidermodelle anbieten | Einstellungen › AI, Platz *Sorting* (#545) | `ai.list_models` um das Feld `decision` erweitert · Read | vorhanden |
| Plätze und Zuordnung lesen | Steuerzentrale › AI, Einstellungen › AI und › Categorization | `ai.list_assignments` (Feld `slots`) · Read | `ai_list_assignments` |
| Modell für *Sortieren* setzen | Steuerzentrale › AI, Einstellungen › AI (Categorization verweist dorthin, #545) | `ai.set_slot` mit `slot: decide` · Admin; ein Modell anderswo nur Menschen | `ai_set_slot` (Freigabe) |
| Ausnahme je Funktion zurücknehmen | Einstellungen › AI, beim Setzen von *Sorting* (#545: Categorization setzt keine mehr) | `ai.set_assignment` · Admin | `ai_set_assignment` |
| Warnzeichen sehen (Issue #506) | Band bzw. leise Zeile im Leser | Feld `warnings` in `mail.get_message` · Read; für einen Client-Schlüssel ohne die Zeichen, die Adressbücher und andere Konten verraten würden | `mail_get_message` |
| Abgefangenes sehen (Issue #506) | Unerwünscht › Betrug bzw. Spam | Feld und Filter `caught` in `mail.search_messages` · Read | `mail_search_messages` |
| „Kein Betrug“ (Issue #506) | Knopf im Band | `inbox.release_mail` mit `trust` · Admin (ein Agent nur mit Freigabe, der App-Schlüssel direkt) | `inbox_release_mail` |
| Vertraute Absender (Issue #506) | Einstellungen › Categorization, Seiten Spam und Betrug | `inbox.list_rules` (Art `trusted`) · Read, `inbox.remove_rule` · Admin | `inbox_list_rules`, `inbox_remove_rule` |
| Mail von Kontakten als persönlich (Issue #390) | Einstellungen › Categorization › Contacts | `settings.set_setting` mit `mail_contacts_personal` · Admin, lesen über `settings.get_settings` (`mailContactsPersonal`) · Read | `settings_set_setting`, `settings_get_settings` |

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
- Die **fünf mitgelieferten Gruppen** bleiben als Kennungen fest und erhalten je eine Farbe aus der
  Gruppenpalette des aktiven Designs. Seit #343 heißen sie in den Mail-Einstellungen **Hauptkategorien**, und der Mensch legt
  eigene dazu an (`custom_group_<n>`, `<n>` ab 6, nie neu vergeben): mit Name, Icon und einer der
  fünf Farben (0–4). Name, Icon und Farbe lassen sich bei jeder Hauptkategorie ändern, die
  Kennung bleibt; eine mitgelieferte bekommt mit leerem Namen die Worte der Suite zurück. Eigene
  Hauptkategorien halten Unterkategorien wie die mitgelieferten; „Nicht eingeordnet“ bleibt die
  Gruppe für das, was nichts trifft. Höchstens 20 Hauptkategorien. Jede Unterkategorie hat ein einfarbiges Icon in der Farbe ihrer Gruppe. Die 23
  mitgelieferten Kategorien haben feste Standard-Icons; bei eigenen Kategorien kann das Icon
  gewählt werden. Die Icon-Auswahl ist auch für mitgelieferte Kategorien verfügbar. Gruppenname
  und Einrückung zeigen die Hierarchie zusätzlich zur Farbe.
- **Zeichen im Kreis der Mail-Liste** (#458): Die Liste des Mail-Fensters zeigt vor jeder Mail einen Kreis in der Farbe
  des Absenders (fest aus seiner Adresse, zehn Farben ohne Rot). Darin steht das Zeichen der Kategorie, bei Kategorien
  mit dem Personen-Zeichen ein gezeichnetes Porträt, ohne Kategorie ein Umschlag; die Gruppe „Unerwünscht“ hat immer
  einen dunkelroten Kreis mit Totenkopf. Seit #458 hat die Auswahl neun Zeichen mehr (Megafon, Roboter, Quittung, Paket,
  Totenkopf, Zeitung, Gebäude, Bank, Warnschild). Die mitgelieferten Kategorien zeigen davon: Newsletter Zeitung, Werbung
  Megafon, Belege Quittung, Versand Paket, Automatische Antwort Roboter, Kontoauszug Bank, Spam Totenkopf, Phishing
  Warnschild – solange der Mensch ihr Zeichen nicht selbst geändert hat (gespeichert wird nur eine eigene Wahl).
- Die **Beschreibung** ist das, was das Modell über die Kategorie liest – mit Beispielen und
  Ausnahmen in den Worten des Menschen. Sie lenkt das Modell, garantiert aber nichts: Eine harte
  Ausnahme ist eine Sortierregel.

### Sortierregeln

Eine Regel nennt eine Kategorie, **Bedingungen** (alle müssen gelten) und **Ausnahmen** (keine darf
gelten). Felder: Absender (gleich), Domain (gleich oder Subdomain), Betreff (enthält), Text
(enthält, so weit die Suite den Text hat). Groß- und Kleinschreibung zählt nicht. Seit Issue #481 stehen
die Regeln auf der Seite ihrer Kategorie („Fixed rules · go before the AI“, je Regel ein Schalter),
zusammen mit den Mail-Filtern, die diese Kategorie setzen (Zeile mit „Search“, öffnet den Filter-Dialog),
und bei Persönlich der eingebauten Regel „From someone in your address books“. „+ Rule“ bietet eine Regel
oder einen Filter aus einer Suche an. Innerhalb einer Kategorie ist die Reihenfolge der Regeln egal;
abgeschaltete zählen nicht. Filter ohne Kategorie (nur gelesen oder Papierkorb) stehen unter „Other
filters“ am Ende des Tabs Categories.

Die gemerkten Regeln aus „Always for this sender?“ sind Sortierregeln mit einer Bedingung
„Absender“ und stehen bei ihrer Kategorie. Über `inbox.list_rules` und `inbox.remove_rule` sind sie
weiter erreichbar.

### Vorrang

1. **Von Hand** gesetzt (`person`): bleibt, bis der Mensch selbst etwas anderes wählt oder
   ausdrücklich `inbox.classify_item` drückt.
2. **Feste Regeln** (Issue #481): Sortierregeln, die gelten, und Mail-Filter ([mail.md](mail.md#filter)),
   die die Mail finden und eine Kategorie setzen; das Modell wird nicht gefragt (`rule`). Ein Filter, der
   nur liest oder in den Papierkorb legt, entscheidet nichts.
   - Eine gemerkte Regel „Always for this sender?“, die gilt, geht allen anderen vor: Der Mensch hat über
     genau diesen Absender entschieden. Das gilt, solange sie einen Absender oder eine Domain ohne Ausnahme
     nennt; im Editor zu mehr umgebaut, ist sie eine Regel wie jede andere ihrer Kategorie.
   - Sonst gewinnt unter den passenden Regeln und Filtern die Kategorie, die in der Liste des Tabs
     Categories weiter oben steht: Gruppen von oben, darin Kategorien von oben. Der Mensch ordnet sie am
     Griff (`inbox.move_category`, in der Gruppe und in eine andere). Innerhalb einer Kategorie ist die
     Reihenfolge ihrer Regeln egal.
   - Die gemeinsame Reihenfolge von Regeln und Filtern aus Issue #454 gibt es nicht mehr; sie ging beim
     Update (Schema 20) verloren. Ein Hinweis auf konkurrierende Regeln fehlt bewusst: Die Suchen von
     Filtern lassen sich nicht verlässlich vergleichen.
   Eine Kategorie, die nicht mehr im Katalog steht, überspringt der Filter. Eine von Hand gesetzte
   Kategorie überschreibt er nie, auch nicht beim Anwenden auf vorhandene Mail (`mail.apply_filter`).
3. **Feste Merkmale**: Einladung, Abwesenheit, Junk-Ordner (`rule`).
4. **Kontakte** (Issue #390, die eingebaute Zeile „From someone in your address books“ auf der Seite von
   Persönlich unter Einstellungen › Categorization, anfangs an; sie greift wie bisher erst nach allen
   eigenen Regeln und Filtern, Issue #481): Mail, deren Absenderadresse ein Kontakt hat, wird `personal` (`rule`).
   Es zählen alle Adressen eines Kontakts, Groß- und Kleinschreibung egal, keine Aliase, und nur
   Adressbücher, die nicht ausgeblendet sind („Kürzlich kontaktiert“ zählt nicht). Mail an Listen oder
   von Maschinen (`List-Id`, `Precedence`, `Auto-Submitted`) bleibt außen vor: Der Newsletter eines
   Kontakts ist keine persönliche Mail. Die Stufe gilt für neue Mail und für „Sort again“ und
   Sortieraufträge; Mail mit einer Kategorie ordnet sie nicht von selbst um. Eine Kategorie aus einer
   Regel oder einem Filter überschreibt die Stufe auch bei „Sort again“ nicht (ein Modell kann das
   dort weiterhin, wie vor Issue #390). Mail von der Adresse des Kontos selbst (eine eigene Karte im
   Adressbuch, etwa im Ordner „Gesendet“ eines Sortierauftrags) zählt nicht als Mail eines Kontakts;
   andere Adressen und Aliase der Person erkennt die Stufe nicht.
5. **Modell** (`model`), nur für Kategorien, die es wählen darf: Das ist der Normalfall; „AI may not file
   mail here“ auf der Seite einer Kategorie nimmt sie heraus (anfangs aus). Beim Update auf Schema 20
   (Issue #481) darf das Modell einmalig wieder jede Kategorie wählen, außer denen aus den Kopfzeilen;
   hätte eine Gruppe damit mehr als 20, bleiben die untersten ihrer Kategorien draußen, das Update
   scheitert nicht. Bei Mail eines Kontakts bietet der
   Dienst nur die Kategorien der Gruppe von `personal` und `phishing` an, damit ein gefälschter
   Absender erkennbar bleibt; ist sich das Modell unsicher oder nennt es etwas anderes, bleibt
   `personal`.

Ein Filter mit Kategorie wirkt wie eine Sortierregel: Ihn anzulegen, zu ändern oder anzuwenden braucht
außer Admin im Bereich `mail` auch Admin im Bereich `inbox`.

**Rechte beim Verschieben** (Issue #481): `inbox.move_category` braucht Admin im Bereich `inbox`, aber kein
Recht für Mail-Filter. Wer eine Kategorie verschiebt, bestimmt damit zwar, welcher Filter gewinnt, ändert
aber keinen Filter; seit #481 reicht dafür das Recht für Kategorien. Die Freigabe nennt die Kategorie,
die Zielgruppe und die Kategorie, vor der sie landet („before …“ bzw. „at the end of …“), und die
Wirkung; hat sich die Zielgruppe seit der Freigabe geändert, endet der Befehl mit `conflict`.

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
- Neue Regeln und geänderte Beschreibungen wirken auf **neue** Mail. Bestehende Mail ordnet nur
  „Sort existing mail…“ neu (siehe unten). Von Hand gesetzte Kategorien bleiben.
- „Remove categories from all mail…“ unten im Tab „Categories“ nimmt nur die Zuordnungen von den Mails
  (`inbox.clear_categories` mit `disable_automatic: false`). Es schaltet das Modell nicht aus und sortiert
  nicht neu.

### Vorhandene Mail sortieren

Der Dialog „Sort existing mail“ (Issue #454) startet einen Lauf (`inbox.start_category_job`) über
Konten, Ordner und Zeitraum. **Was er anwendet**, sagt `scope.apply_rules` und `scope.apply_model`
(beide ohne Angabe an, mindestens eines, ein Modell nur mit `apply_model`):

- `apply_rules` („Fixed rules“): Regeln und Filter nach dem Vorrang oben, die festen Merkmale und die Kontakte. Ein Filter
  zählt nur für Mail im Posteingang (wie beim Eintreffen) und setzt hier nur seine Kategorie (`rule`, also
  geschützt); als gelesen markieren und in den Papierkorb legen gibt es bei vorhandener Mail nur über
  „Also apply…“ im Dialog des Filters (`mail.apply_filter`).
- `apply_model`: das Modell für das, was die Regeln offen lassen.
- **Mail mit Kategorie**: „Leave as it is“ oder „Ask the AI again“ (`again`: auch Kategorien des Modells
  neu, eigene Entscheidungen und Kategorien aus Regeln bleiben).

Vorschau und Lauf schätzen die Dauer (`seconds`,
`left_seconds`) nach dem Tempo des laufenden oder letzten Laufs, der genauso sortierte (dasselbe Modell
oder nur Regeln; `sorted_with`, gemessen ohne Pausen in `run_millis`). Läuft, pausiert oder scheiterte ein
Lauf oder ein Entfernen, steht unter dem Kopf der Seite eine Zeile mit Fortschritt, „Pause“/„Resume“ und
„Cancel“.

### Die drei Wege

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Katalog lesen | Einstellungen › Categorization, Seitenleiste, Menü | `inbox.list_categories` · Read | `inbox_list_categories` |
| Hauptkategorie anlegen (#343) | Einstellungen › Categorization › Categories | `inbox.create_category_group` · Admin | `inbox_create_category_group` |
| Hauptkategorie ändern (#343) | Einstellungen › Categorization › Categories | `inbox.update_category_group` · Admin | `inbox_update_category_group` |
| Kategorie anlegen | Einstellungen › Categorization | `inbox.create_category` · Admin | `inbox_create_category` |
| Kategorie ändern | Einstellungen › Categorization | `inbox.update_category` · Admin | `inbox_update_category` |
| Kategorie entfernen | Einstellungen › Categorization | `inbox.remove_category` · Admin | `inbox_remove_category` |
| Regeln lesen | Seite der Kategorie unter Einstellungen › Categorization | `inbox.list_sort_rules` · Read | `inbox_list_sort_rules` |
| Regel anlegen, ändern, entfernen | Seite der Kategorie („+ Rule“, Schalter, Seite der Regel) | `inbox.create_sort_rule`, `inbox.update_sort_rule`, `inbox.remove_sort_rule` · Admin | gleichnamig |
| Kategorie verschieben (#481), auch in eine andere Gruppe | Tab Categories, Ziehen am Griff; „Move up“/„Move down“ auf der Seite der Kategorie (in ihrer Gruppe) | `inbox.move_category` · Admin | `inbox_move_category` |
| Vorhandene Mail sortieren (#454) | „Sort existing mail…“ | `inbox.preview_category_job`, `inbox.start_category_job { apply_rules, apply_model, again }` · Read/Write | gleichnamig |
| Kategorien von allen Mails entfernen | Tab Categories, unten | `inbox.clear_categories` · Delete | `inbox_clear_categories` |

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
