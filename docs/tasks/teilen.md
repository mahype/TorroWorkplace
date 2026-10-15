# Aufgaben: Teilen, Stufen TL1–TL3

Konzept: [teilen.md](../teilen.md). Regeln: [CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill
`torro-task`. Wer einen Request anfasst, lädt `torro-api`; nach jeder Funktion prüft `torro-api-review`.
Nach jeder UI-Änderung prüft `torro-design-review`.

**Für alle Aufgaben gilt** (Entscheidungen 1 bis 4 in [teilen.md](../teilen.md)):
- Kein eigener Bereich, kein eigenes Recht. Wer teilt, braucht Lesen des Inhalts samt Auswahl und das
  Recht des Ziels. Ein Inhalt außerhalb der Auswahl ist `not-found`.
- Die Karte baut immer der Kern aus seinem Cache; ein Client schickt nur Art, Kennung und Auswahl.
- Eine geteilte vCard wird neu gebaut, nur mit den gewählten Feldern, nie die gespeicherte.
- Ins Protokoll kommen Art und Zahl der Inhalte, nie Namen oder Felder.

Aufgaben, die den Katalog ändern (TL1.1, TL1.3), laufen nie parallel zu anderen Katalog-Aufgaben.

```
KO1.1a ── TL1.0 ── TL1.1 (braucht KO1.5) ─┬─ TL1.2 (Teilen-Blatt)
                                          └─ TL1.3 (Mail) ── TL1.4 (Schreibfenster)
TL1.5 (torrocontacts) braucht TL1.2, TL1.3, KO1.7
TL2 setzt der Chat-Plan um (CH3.1–CH3.3 in chat.md) · TL3 je Art mit ihrem Plan
```

## TL1 Kontakte teilen: Mail, Kopieren, Datei, QR-Code

### TL1.0 Karte eines Kontakts in `torro-contacts`
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** KO1.1a · **Dateien:** neu `crates/torro-contacts/src/share.rs`,
  `crates/torro-contacts/src/lib.rs`, `crates/torro-contacts/tests/fixtures/share/*`
- **`ShareFields`:** eine Menge von Feldarten (Name, Firma und Position, Telefon, Mail, Adresse
  Arbeit, Adresse privat, Web, Messenger, Geburtstag, Notiz, Foto, Gruppen). `ShareFields::default()`
  ist die Voreinstellung aus Entscheidung 4; `ShareFields::private()` nennt die ab Werk aus
  geschalteten Arten.
- **`share_text(contacts, fields)`:** kurzer Text, ein Block je Kontakt. Ein Feld je Zeile, mit Art
  („mobile“, „work“) über `tr()`.
- **`share_vcard(contacts, fields)`:** neue vCard 3.0 je Kontakt, mehrere hintereinander in einer
  Datei. Mit `UID` des Originals, damit ein Empfänger Dubletten erkennt. Ohne `X-`-Felder, `GEO`,
  `CATEGORIES` und `REV`; `PHOTO` nur mit dem Feld Foto, dann als 256-px-JPEG.
- **Dateiname:** `<Name>.vcf`, bei mehreren `contacts.vcf`. Zeichen, die Dateisysteme nicht mögen,
  werden ersetzt.
- **API/MCP:** keine (Bibliothek). Requests kommen in TL1.1.
- **Abnahme:** Tests: Voreinstellung lässt Notiz, private Adresse und Geburtstag weg; eine Notiz mit
  „Ignore previous instructions“ geht nur mit dem Feld Notiz mit; die vCard lässt sich mit dem Leser
  aus KO1.1a wieder lesen und enthält nur die gewählten Felder; Sonderzeichen im Dateinamen.

### TL1.1 Gerüst: `ShareItem`, `contacts.share_card`, Kern-Modul `share`
- [ ] erledigt
- **Modell:** Opus · **Braucht:** TL1.0, KO1.5 (beide ändern `mcp.rs`) · **Dateien:** `crates/torro-ipc/src/{lib,api,contacts}.rs`,
  `crates/torro-ipc/Cargo.toml`, neu `crates/torro-ipc/src/share.rs`, neu
  `crates/torroworkplace/src/share/mod.rs`, `crates/torroworkplace/src/contacts/service.rs`,
  `crates/torroworkplace/src/{core,mcp,audit}.rs`, `docs/api.md` (Abschnitt „Teilen“ mit der Regel
  „Reine Darstellung ist kein eigener Befehl“ aus Entscheidung 7), `docs/api-reference.md`
- **Typen** in `torro-ipc/src/share.rs`: `ShareItem { kind, id }`, `ShareKind` (`Contact`),
  `ShareCard { title, text, file: Option<ShareFile>, included: Vec<ShareField>, untrusted }` mit
  `untrusted` immer `true`, `ShareFile { name, mime, data }` mit `data` in Base64. Mit JSON-Schema.
- **Request** `contacts.share_card { ids, fields?, include_file? }` · Read · `contacts_share_card`.
  Ohne `include_file` kommt keine Datei mit. Höchstens 50
  Kontakte, die Datei höchstens 5 MB. Die Beschreibung sagt, dass die Felder Daten sind und dass
  ohne `fields` die sparsame Voreinstellung gilt.
- **Kern `share/mod.rs`:** `card_for(caller, items, fields) -> Result<ShareCard, …>`: holt je Art die
  Karte über deren Dienst, mit den Rechten des Aufrufers. Mischen verschiedener Arten wird abgelehnt
  (`error`). Diese Funktion benutzen TL1.3 und später der Chat (CH3.2).
- **Protokoll:** Befehl, Art und Zahl der Inhalte, Kennungen als Prüfsumme.
- **Assistent:** `share_card` folgt denselben Regeln wie `get_contact`, auch für entfernte Modelle.
- **Abnahme:**
  - Ein Client ohne das Adressbuch bekommt `not-found`, auch wenn nur einer von drei Kontakten fehlt.
  - Ohne `fields` gilt die Voreinstellung.
  - Der Test `contacts_descriptions_say_fields_are_data` aus KO1.1 deckt `share_card`.
  - `torro-api-review` ohne Befund.

### TL1.2 Teilen-Blatt in `torro-gui`
- [ ] erledigt
- **Modell:** Opus · **Braucht:** TL1.1 (Typ `ShareCard`) · **Dateien:** neu `crates/torro-gui/src/share.rs`,
  `crates/torro-gui/src/lib.rs`, `crates/torro-gui/Cargo.toml`, Wurzel-`Cargo.toml` (`qrcode`, und für
  die Zwischenablage, falls Iced sie nicht schon kann, eine Bibliothek, die die Aufgabe nennt)
- Skill `torro-design` laden. Ein Blatt (Dialog aus `torro_gui::dialog`) für alle Apps:
  - **Vorschau** der `ShareCard`, bei mehreren Inhalten „3 contacts“ mit aufklappbarer Liste.
  - **„What to include“:** Häkchen je Feld, die privaten mit Schloss und „Private“. Jede Änderung
    holt die Karte neu (Nachricht an die App, das Blatt lädt nichts selbst).
  - **Ziele:** Mail (mit Kontowahl ab zwei Konten), Copy, Save as file…, QR code. Chat erscheint,
    sobald die App ein Chat-Ziel meldet (TL2, CH3.3). Ziele ohne Konto werden nicht gezeigt.
  - **QR code** groß im Blatt, Fehlerkorrektur M; passt die vCard nicht, der Hinweis aus dem Konzept.
  - Tastatur: `Esc` schließt, Pfeiltasten wählen das Ziel, Enter führt aus.
- Das Blatt kennt keine Art; es bekommt Karte, Feldliste und Ziele von der App.
- **API/MCP:** keine (Widget). Die Ziele schicken über die App TL1.1/TL1.3.
- **Abnahme:** `torro-design-review` mit einer Demo-Ansicht, hell und dunkel; Tests der Nachrichten
  (Häkchen ändert die Auswahl, Ziel wird gemeldet); QR-Grenze getestet.

### TL1.3 Mail: Inhalte als Anhang, Entwurf öffnen
- [ ] erledigt
- **Modell:** Opus · **Braucht:** TL1.1 · **Dateien:** `crates/torro-mail/src/message.rs`
  (`DraftAttachment::Items`, `ItemRef`), `crates/torro-ipc/src/{mail,api}.rs`,
  `crates/torroworkplace/src/mail/*` (Entwurf bauen; `store.rs`: Karten je Entwurf),
  `crates/torroworkplace/src/{core,approvals,apps,audit,mcp}.rs`, `crates/torroworkplace/src/assistant/*`
  (nur die Zuordnung zu `contacts`), `crates/torroinbox/src/main.rs` (nur `--draft` annehmen),
  `docs/api.md`, `docs/api-reference.md`
- **`DraftAttachment::Items { items: Vec<ItemRef>, fields }`**, `ItemRef { kind, id }` in `torro-mail`
  (Text, weil `torro-mail` von nichts abhängt; `kind` des Anhangs ist der serde-Tag, darum `items`).
  Ein Anhang ergibt eine Datei. `torro-ipc` prüft Art und Kennungen wie andere Kennungen.
- **`mail.save_draft`:**
  - Der Kern holt die Karte über `share::card_for` mit den Rechten des Aufrufers und hängt die Datei
    an. Fehlt das Recht auf einen Inhalt: `not-found` für den ganzen Entwurf.
  - Geprüft wird vor der Freigabe und noch einmal bei „Allow“, mit derselben Reichweite samt Grenzen
    eines geliehenen Schlüssels (Vorbild `respond_to_invitation` in `core.rs`).
  - Der Assistent ordnet den Befehl mit `items` auch `contacts` zu (Vorbild Einladung in `api.md`).
  - Die Beschreibung von `save_draft` nennt die neue Anhang-Art.
- **Karten je Entwurf (`mail/store.rs`):** Der Kern merkt sich Kennungen und Auswahl neben dem
  Entwurf, dazu einen Fingerabdruck des gespeicherten Entwurfs (IMAP-UID und Prüfsumme der `.vcf`).
  Beim Senden vergleicht er ihn. Speichert ein Client den Entwurf neu ohne diese Anhänge, oder ändert
  ihn jemand außerhalb der Suite, fällt der Eintrag weg.
- **`mail.get_message`** bekommt für Entwürfe, die der Aufrufer öffnen darf, das neue optionale Feld
  `shared_items` (Kennungen und Auswahl je Kontakt-Anhang). So gibt das Schreibfenster sie beim
  Speichern wieder als `items` mit (TL1.4). API-Referenz neu erzeugen.
- **Freigaben:** `save_draft` und `send_draft` nennen jede Karte: „Contact card: Anna Muster (6
  fields)“, und Felder über die Voreinstellung hinaus („+ private address“). Beim Senden kommt das aus
  dem gemerkten Eintrag und dem Kontakt-Cache, nie aus der `.vcf` im Entwurf. Andere `.vcf` erscheinen
  nur als Datei. Namen werden entschärft (eine Zeile, keine Steuerzeichen, 60 Zeichen); unbekannt →
  „unknown contact“.
- **`apps.open_app` mit `target: "draft:<m:id>"`:**
  - Die Kennung bestimmt das Konto; es muss in der Reichweite liegen (wie `reads_mail`).
  - Ein Client-Schlüssel öffnet nur eigene Entwürfe, der App-Schlüssel alle; sonst `not-found`.
  - Neu `Opened::Draft`, das zu `torroinbox --compose --draft=<id>` wird; das Ziel gehört zur App
    `torroinbox` (Schreibfenster `torro-compose`).
  - Wie bei den übrigen Zielen kommt nur auf die Befehlszeile, was der Kern gefunden hat.
  - `torroinbox` nimmt `--draft` in dieser Aufgabe nur an und öffnet ein leeres Schreibfenster; das
    Laden des Entwurfs kommt in TL1.4. Die Abnahme prüft `target_command`.
- **MCP:** `mail_save_draft` und `apps_open_app` bleiben, ihre Beschreibungen nennen das Neue.
- **Abnahme:**
  - Ein Entwurf mit zwei Kontakten hat eine `contacts.vcf` mit genau den gewählten Feldern.
  - Ein Agent mit Mail-, aber ohne Kontakt-Recht bekommt `not-found`.
  - Die Freigabe zum Senden nennt die Karten und zusätzliche private Felder.
  - `draft:` öffnet für einen Client nur eigene Entwürfe, für den App-Schlüssel alle.
  - Eine fremde `.vcf` aus einer empfangenen Mail erscheint in der Freigabe nur als Datei.
  - Nimmt eine Freigabe zwischen Anfrage und „Allow“ das Adressbuch weg, wird nichts gespeichert.
  - `torro-api-review` ohne Befund.

### TL1.4 Schreibfenster: Entwurf öffnen, „Attach contact…“
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** TL1.3, KO1.5 · **Dateien:** `crates/torroinbox/src/{main,compose,mail}.rs`
- **Oberfläche:**
  - `torroinbox --compose --draft <id>` lädt den Entwurf (Empfänger, Betreff, Text, Anhänge) zum
    Weiterschreiben. Speichern ersetzt ihn (Delete, mit dem App-Schlüssel ohne Rückfrage) und gibt
    die Kontakt-Anhänge aus `shared_items` (TL1.3) wieder als `items` mit, damit die Karten in der
    Freigabe bleiben.
  - Knopf „Attach contact…“ öffnet eine Kontaktsuche (`contacts.list_contacts`), danach das
    Teilen-Blatt nur mit „What to include“; „Attach“ hängt die Karte an und fügt auf Wunsch den
    Text in die Mail ein.
  - Ohne Recht auf Kontakte fehlt der Knopf.
- **API:** `mail.get_message` (Entwurf lesen), `mail.save_draft`, `contacts.list_contacts`,
  `contacts.share_card`. **MCP:** dieselben Tools.
- **Abnahme:** Tests der Nachrichten und der Startargumente; Sichtprüfung.

### TL1.5 `torrocontacts`: „Share…“
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** TL1.1, TL1.2, TL1.3, KO1.7 · **Dateien:** `crates/torrocontacts/src/*`,
  `crates/torro-contacts/src/{source,demo}.rs` (Methode `share_card`), `crates/torro-ipc/src/contacts.rs`
  (`ContactsClient::share_card`)
- **Oberfläche:**
  - Knopf „Share…“ im Kopf eines Kontakts, im Kontextmenü der Liste und für die Mehrfachauswahl
    (Shift/⌘-Klick in der Liste).
  - `⌘⇧S`/`Ctrl+Shift+S`.
  - **Mail:** `mail.save_draft` mit `DraftAttachment::Items` und dem Text der Karte, danach
    `apps.open_app` mit `draft:`.
  - **Copy:** der Text der Karte in die Zwischenablage.
  - **Save as file…:** der Dateidialog, die `.vcf` aus der Karte.
  - **QR code:** im Blatt.
  - **Demo:** Mail zeigt nur eine Meldung „Draft created (demo)“.
- **API:** `contacts.share_card`, `mail.save_draft`, `apps.open_app`. **MCP:** dieselben Tools.
- **Abnahme:** Sichtprüfung mit `--demo --open share`; Tests der Nachrichten (die Auswahl geht mit).

## TL2 Chat (mit der Chat-App)

**Umgesetzt wird diese Stufe in CH3 von [chat.md](chat.md)** (CH3.1 Anbieter, CH3.2 Befehl, CH3.3
Teilen-Blatt); dort stehen Dateien, Abnahme und Reihenfolge. Dieser Abschnitt hält fest, was das
Teilen davon braucht:
- **Befehl:** `chat.send_message { conversation, text, items?, fields? }` im Bereich `chat`, Write und
  nach außen, MCP `chat_send_message`. `items` sind `ShareItem`s mit Auswahl, aufgelöst über
  `share::card_for` mit den Rechten des Aufrufers.
- **Senden mit Datei:** in den Talk-Anhangsordner laden (Ordner aus der Talk-Konfiguration), dann in
  die Unterhaltung teilen, mit dem Text der Karte als Beschriftung. Prüfen gegen die Talk-Doku und die
  ältesten unterstützten Talk-Versionen; ohne Beschriftung zwei Nachrichten.
- **Freigabe:** Unterhaltung (Name aus dem Cache), jede Karte wie bei Mail und dass eine Datei in
  den Talk-Ordner des Menschen geladen wird.
- **Rechte:** Write in `chat` mit dem Konto in `chat_accounts`, der Schalter „Send on my behalf“
  (`Rights::outbound`, kein eigener Schalter; [chat.md](../chat.md), Entscheidung 4) und Lesen der
  Inhalte. Die Reichweite der Inhalte wird wie bei Mail vor der Freigabe und bei „Allow“ geprüft.
- **Teilen-Blatt:** Ziel „Chat“ mit Liste der letzten Unterhaltungen und Suche; `torrocontacts`
  meldet es, sobald ein Konto mit Talk da ist und der Aufrufer `chat` hat.

## TL3 Weitere Arten

Je Art eine kleine Aufgabe in ihrem eigenen Plan, nach dem Muster von TL1.0, TL1.1 und TL1.5:
`ShareKind` erweitern, `<bereich>.share_card` mit eigener Auswahl, Fall in `share::card_for`,
„Share…“ in der App. Vorgemerkt:
- **Notizen:** Markdown-Text, Datei `.md`.
- **Termine und Aufgaben:** Text, Datei `.ics` (nur der eine Termin, keine Serie anderer).
- **Fotos:** die Datei, verkleinert, ohne Ortsdaten.
