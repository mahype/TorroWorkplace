# Mail-Backup und Wiederherstellung (#267)

Stand: 2026-10-21. Konzept für [Issue #267](https://github.com/mahype/TorroWorkplace/issues/267).
Die Bedienung liegt im **Eingang/Mail-Fenster → Einstellungen → Konten → Konto → Backup & Restore**.
Die allgemeinen Einstellungen enthalten höchstens den Link zum Mail-Fenster. Ein klickbarer
Entwurf liegt unter [prototypes/mail-backup/index.html](prototypes/mail-backup/index.html).

## Geltungsbereich und Vollständigkeit

Ein Backup umfasst eine gewählte Quelle und optional alle Nachrichten ab einem Kalendertag
(einschließlich, anhand von IMAP `INTERNALDATE`). Ohne Datum gilt die ganze Quelle. „Server“
meint alle erreichbaren, auswählbaren IMAP-Ordner und ihre Nachrichten; `\All` und andere
virtuelle Sammelansichten werden zuletzt gelesen, damit Nachrichten, die nur dort liegen,
erhalten bleiben. Jede UID wird als Ordnerzuordnung archiviert, auch in `\All`: Aus
bytegleichen Rohdaten allein lässt sich nicht beweisen, ob zwei UIDs dieselbe Mail oder
eigenständige Exemplare sind. Die Zähler meinen daher Ordner-Einträge; eine virtuelle
Sammelansicht kann darin zusätzliche Kopien enthalten. Beim Restore muss die Zuordnung
dieser Ansicht bewusst geprüft werden. Nicht auswählbare
Ordner werden im Manifest mit Grund erfasst; wenn dort Nachrichten vermutet werden, ist die
Vollständigkeit unklar und niemals stillschweigend „100 %“.

„Lokal“ meint nur Nachrichten, deren **vollständige RFC-822-Originalbytes** auf dem Gerät
vorliegen. Der lokale Text- und Suchcache enthält für ältere Bestände nicht verlässlich
eine EML. Im Modus „Alle Mailtexte offline“ (#263) hält das unabhängige Originaldepot nun
beim Abruf die Rohbytes, SHA-256, Ordner/UIDVALIDITY/UID, `INTERNALDATE` und Flags/Keywords
in der kontobezogenen SQLite-Datei. Ein Cache-Auftrag holt auch bereits vorhandene Texte
erneut, wenn deren Original noch fehlt. Das Depot wird weder beim Pruning von Texten noch
beim Entfernen veralteter Header geleert; es wird mit dem Konto entfernt. Sein Platzbedarf
wächst daher bis zur Größe der aufbewahrten Originalnachrichten. Andere Cache-Modi legen
keine neuen Originale an. Bestehende Header ohne Original werden gezählt und im lokalen
Backup-Bericht als ausgelassen ausgewiesen. Das Backup holt im Lokal-Modus nichts vom Server.

„Vollständig“ bedeutet: Alle beim ersten Inventar gefundenen passenden Ordner/UIDs liegen
nach Integritätsprüfung im Archiv, und ein Schlussinventar hat keine ungeklärte Abweichung.
`UIDVALIDITY`-Wechsel, verschwundene UIDs, unlesbare Ordner, Fetch-Fehler, Datenlücken und
Prüfsummenfehler lassen den Lauf scheitern oder als unvollständig enden. Neue Nachrichten
während des Laufs werden als Änderung gemeldet und nach einer begrenzten Wiederholung
erfasst; bleibt der Bestand instabil, gibt es keinen `complete`-Status. IMAP bietet keinen
kontoweiten atomaren Snapshot. Der Bericht nennt daher Start/Ende des Inventars und den
verifizierten Zustand, behauptet aber keinen garantierten historischen Zeitpunkt.

## Archiv v1

Eine **normale ZIP64-Datei** mit UTF-8-Namen. Sie ist ohne Torro mit ZIP-Programmen, einem
Texteditor und jedem EML-fähigen Mailprogramm lesbar. Sie ist standardmäßig **unverschlüsselt**;
die Oberfläche zeigt diesen Umstand vor dem Speichern. Das ZIP wird erst nach erfolgreicher
Prüfung als finale Datei umbenannt. Abgebrochene temporäre Dateien tragen keinen
`complete`-Status und werden nicht als fertiges Archiv angeboten.

```text
mail-example-2026-10-21.zip
├── README.txt
├── manifest.json
├── messages.csv
├── SHA256SUMS
└── mail/
    ├── f000001/
    │   ├── m000000001.eml
    │   └── m000000002.eml
    └── f000002/
        └── m000000003.eml
```

Die IDs in Pfaden sind archivinterne, sichere Kennungen, keine vom Server gelesenen Pfade.
Ordnername und IMAP-Pfad stehen in `manifest.json`. Jede `.eml` enthält unveränderte
Nachrichtenbytes einschließlich MIME-Struktur und Anhängen. `manifest.json` enthält mindestens
`format`, `version`, `status`, Quelltyp, Zeitraum, Start/Ende, eine Kontobeschreibung ohne
Zugangsdaten, Ordnerliste mit Trennzeichen und Rollen, pro Eintrag Ordner-ID, EML-Pfad,
Bytezahl, SHA-256, `INTERNALDATE`, Flags/Keywords und Herkunfts-UID/`UIDVALIDITY`, Zähler,
Ausnahmen und Inventarvergleich. UIDs sind Hinweise zur Herkunft, keine beim Restore
übertragbaren Identitäten. `messages.csv` wiederholt die wichtigsten Spalten für Menschen.
`SHA256SUMS` umfasst alle EMLs und die drei Metadateien; es umfasst sich selbst nicht.
`messages.csv` hat die Spalten `folder_id,message_id,path,internal_date,bytes,sha256` in
dieser Reihenfolge; fehlende Datumswerte bleiben leer. `README.txt` beschreibt Öffnen,
Prüfen und Grenzen. Schema/Version und Feldsemantik werden
mit einem anonymen Beispielarchiv und einem dokumentierten JSON-Schema festgeschrieben.

**Vertrag v1 (MB.1):** Ein [anonymes Beispielarchiv](../crates/torro-mail-backup/tests/fixtures/sample-v1.zip),
sein [Manifest](../crates/torro-mail-backup/tests/fixtures/manifest-v1.json)
und sein [JSON-Schema](../crates/torro-mail-backup/tests/fixtures/manifest-v1.schema.json)
liegen als geprüfte Fixtures im Repository. Das ZIP lässt sich mit `unzip` öffnen und
seine `.eml` ohne Torro als Rohmail lesen. `source` ist `server` oder `local`, `status`
ist `complete` oder `incomplete`. `since` ist ein inklusiver Kalendertag oder `null`.
`started_at` und `finished_at` sind UTC-Zeitpunkte; pro Mail ist `internal_date` mit
Zeitzone gespeichert. `folders[].id` und `messages[].id` sind fortlaufende archivinterne
Kennungen, während `folders[].path` den tatsächlichen IMAP-Namen als Daten enthält.
`messages[].path` muss exakt zu den beiden Kennungen passen. `uid` und `uid_validity`
sind entweder beide gesetzt oder beide leer; die UID darf je Ordner nur einmal vorkommen.
Jeder auswählbare Ordner eines vollständigen Archivs hat eine `inventory` mit Prüfzeit,
Zahl der passenden Quellnachrichten und bei Server-Backups einem SHA-256 der sortierten
UIDs (jeweils vier Bytes in Netzwerk-Reihenfolge). Der Prüfer gleicht diese Angaben
gegen die gespeicherten Nachrichten ab. Bei lokalen Backups enthält die Inventur nur
die Zahl der vollständig verfügbaren Originale. `role` hält die erkannte Ordnerrolle,
sofern eine vorliegt.
`counts` stimmt mit den Tabellen und der Summe der EML-Bytezahlen überein. Das Feld
`warnings` trägt ausdrücklich gemeldete Lücken. Die aktuell geprüfte ZIP-Struktur enthält
genau die vier oben gezeigten Metadateien und die referenzierten EMLs; andere Einträge,
doppelte Pfade und nicht zum Manifest passende Prüfsummen werden abgewiesen.

Die Sicherheitsgrenzen des v1-Prüfers liegen bei 100.000 Ordnern, 10 Millionen
Nachrichten und 1 PiB logischer Maildaten je Archiv sowie 256 MiB Manifest und 512 MiB
Indexdatei. Wird eine Grenze erreicht, meldet der Prüfer einen Fehler; er bescheinigt
einem solchen Archiv keine Vollständigkeit. Die Grenzen werden mit der Schreib-Engine
abgeglichen, bevor sie freigeschaltet wird.
Eine Offline-Prüfung kann die Konsistenz und Unversehrtheit des Archivs belegen, aber
ohne erneute Serverabfrage nicht unabhängig beweisen, welche Mails auf dem IMAP-Server
zum Sicherungszeitpunkt existierten. `complete` ist deshalb der dokumentierte Befund
des Backup-Laufs samt Inventur, kein Versprechen eines atomaren Server-Snapshots.

Grenzen: Serverinterne Zustände wie MODSEQ, ACLs, Quota, Threads und anbieterspezifische
Labels außerhalb der IMAP-Flags gehören nicht zum portablen Kernformat. Unterstützte
Flags/Keywords und `INTERNALDATE` werden erhalten; nicht übertragbare Werte erscheinen im
Restore-Bericht. ZIP-Pfade dürfen nie aus ungeprüften Ordnernamen entstehen. Das Verifizieren
begrenzt Eintragszahl, Größe, Entpackverhältnis und Metadatenlänge, um schädliche Archive
abzuweisen.

## Sichern und Prüfen

Die Archiv-Engine (MB.2) liest jede Originalmail in höchstens 64-KiB-Blöcken und schreibt
die Bytes unverändert als eigene `.eml` in das ZIP. Beim Schreiben entstehen Bytezahl und
SHA-256; der Writer erzeugt die sicheren IDs, `README.txt`, `manifest.json`, `messages.csv`
und `SHA256SUMS`. Vor der Veröffentlichung prüft der Dienst die fertige Datei noch einmal
offline, einschließlich EML-Hashes, Metadaten-Prüfsummen und CSV/Manifest-Abgleich. Eine
bekannt unvollständige Quelle oder ein abgebrochener Lauf wird nicht als finale ZIP-Datei
veröffentlicht. Die temporäre Datei liegt im privaten Backup-Ordner und wird bei Fehlern
entfernt; ein vorhandener Zielname wird nie überschrieben.

Die Serverquelle (MB.3) liest die Ordnerliste und öffnet jeden auswählbaren Ordner mit
`EXAMINE`. Sie ermittelt zuerst alle UIDs, holt Flags, Keywords, `INTERNALDATE` und
`RFC822.SIZE` und liest die Originalbytes über `UID FETCH BODY.PEEK[]<offset.count>` in
höchstens 64-KiB-Stücken. Der inklusive Datumsfilter verwendet das tatsächliche
`INTERNALDATE` mit seiner Zeitzone. Nach jedem Ordner prüft sie UIDVALIDITY und UID-Liste,
holt neue UIDs höchstens zweimal nach und bricht bei verschwundenen Nachrichten oder
fortlaufender Änderung ab. Nach dem letzten Ordner vergleicht sie die gesamte Ordnerliste
und alle auswählbaren UID-Inventare erneut. Ein solcher Abbruch veröffentlicht keine fertige
Datei. Das archivierte UID-Inventar wird mit den geschriebenen EML-Einträgen verglichen;
ein vollständiges Archiv wird zusätzlich offline gegen seine Hashes geprüft. Der Job meldet
Zähler, Phase und Abbruchstatus; `verify` benötigt weder Kontozugang noch Netzwerk.

Die Lokalquelle (MB.4) inventarisiert das Originaldepot seitenweise und liest jede EML
als höchstens 64-KiB-BLOB-Stücke in eine temporäre Datei. Gespeicherte SHA-256 und Länge
müssen stimmen. Ein Schlussinventar nach allen EML-Lesevorgängen und ein erneuter Lückenzähler erkennen
Änderungen während des Backups; bei Abweichung erscheint keine fertige ZIP. Ein lokales
Archiv mit `complete` bescheinigt die vollständige Sicherung der **vorhandenen Originale**,
nicht der bekannten Header ohne Original. Deren Zahl steht als Warnung im Manifest und
Job-Bericht. Bei älteren lokalen Originalen aus verschiedenen UIDVALIDITY-Generationen
desselben Ordners bleibt die Herkunfts-UID im Archiv leer; Rohbytes und Ordnerzuordnung
bleiben erhalten.

## Testlauf und Restore

Der Testlauf validiert das Archiv offline und liest das Zielkonto nur mit IMAP-Kommandos,
die keinen Zustand ändern (`EXAMINE` statt `SELECT`, kein `APPEND`, `CREATE`, `STORE` oder
`SUBSCRIBE`). Er legt auch lokal keine Restore-Journaldatei an. Er meldet geplante
Ordnerzuordnung, „hinzufügen“, „bereits vorhanden“ und „unklar“ pro Ordner sowie
Größe, Konflikte und nicht übertragbare Metadaten. Das Ergebnis ist eine Schätzung zum
Zeitpunkt des Testlaufs; vor echtem Schreiben wird es erneut geprüft.

Der echte Restore validiert vorab erneut und fügt standardmäßig nur hinzu. Er erstellt
fehlende Ordner nach ausdrücklicher Bestätigung, führt `APPEND` mit den Originalbytes,
übertragbaren Flags/Keywords und `INTERNALDATE` aus und prüft das Ergebnis. Er löscht
und überschreibt keine vorhandenen Mails. Die Kombination aus Archiv-ID,
Ordnerzuordnung und Hash ist die Wiederaufnahme-Identität; ein Journal protokolliert
bestätigte Anhänge. Bei verlorener Verbindung zwischen `APPEND` und Bestätigung wird der
Eintrag als unklar markiert und im Ziel abgeglichen, bevor er erneut angehängt werden darf.
`Message-ID` allein ist kein Duplikatschlüssel: Sie kann fehlen oder mehrfach vorkommen.
Unvollständige Archive erfordern eine eigene Bestätigung und werden im Ergebnis so markiert.

## Oberfläche, CLI und Rechte

Im Mail-Konto stehen Quelle, Datum, Zielordner, Start und Fortschritt sowie „Archiv prüfen“,
„Testlauf“ und „Wiederherstellen“. Der Ablauf zeigt Leerlauf, laufende Arbeit, Abbruch,
Fehler und Ergebnisse in schmalen wie breiten Fenstern. Die Kontoauswahl für den Restore
ist explizit. Neue Texte sind Englisch mit `tr()` und deutscher Übersetzung.

Vorgesehene CLI-Form, konsistent mit bestehenden `ctl`-Befehlen:

```text
torroworkplace ctl mail backup create --account mail:anna@example.org --source server --since 2020-01-01 --json
torroworkplace ctl mail backup verify --file /erlaubter/ordner/backup.zip --json
torroworkplace ctl mail restore --file /erlaubter/ordner/backup.zip --account mail:anna@example.org --dry-run --json
torroworkplace ctl mail restore --file /erlaubter/ordner/backup.zip --account mail:anna@example.org --apply --json
```

Die CLI verwendet dieselbe Engine wie die Oberfläche, klare Exit-Codes und einen nicht
interaktiven Modus. Der Zieldateiname darf über den erlaubten Backup-Ordner gesteuert
werden; ein beliebiger Pfad von einem Client-Schlüssel ist ausgeschlossen. Kontozugang
kommt aus dem Schlüsselbund des laufenden Dienstes, nie aus CLI-Argumenten oder Logs.
Getrennte Rechte für Backup lesen, Archiv schreiben und Restore schreiben werden mit den
bestehenden `Rights`-Regeln geplant; `verify` und Testlauf bekommen kein Ziel-Schreibrecht.
Die erste Implementierungsaufgabe legt `torro_ipc::Request`, Befehlskatalog und Rechte
zusammen fest. MCP bietet sichere, berechtigte Operationen auf dem konfigurierten
Backup-Ordner; freie Pfade und echte Restore-Ausführung ohne explizite Freigabe bleiben
ausgeschlossen.

## Abnahme

- Anonymes Testkonto mit verschachtelten/Unicode-Ordnern, doppelter Ordnerkopie,
  Flags/Keywords, großer Mail, MIME-Anhang, Datum vor/nach Grenze und paralleler Änderung.
- ZIP mit Systemwerkzeug öffnen, `.eml` unabhängig lesen, `SHA256SUMS` prüfen;
  manipuliertes Archiv wird vor jedem Restore abgewiesen.
- Lokal-Modus ohne Original zählt fehlende Inhalte und markiert sich nicht als 100 %.
- Testlauf gegen instrumentierten IMAP-Testserver enthält **keinen Schreibbefehl**.
- Restore in leeres und teilweise gefülltes Ziel, Unterbrechung direkt nach `APPEND`,
  Wiederaufnahme und Abgleich; keine stille Doppelung.
- UI in allen Größenklassen und hell/dunkel sichten; CLI-JSON/Exit-Codes und
  Rechtefälle mit Tests prüfen.

Spätere Ausbaustufen: inkrementelle Sicherungen, Rotation/Aufbewahrung und Zeitplan.
