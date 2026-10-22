# Mail-Backup und Wiederherstellung (#267)

Stand: 2026-10-22. Konzept für [Issue #267](https://github.com/mahype/TorroWorkplace/issues/267).
Die Bedienung liegt im **Eingang/Mail-Fenster → Einstellungen → Konten → Konto → Backup & Restore**.
Die allgemeinen Einstellungen enthalten höchstens den Link zum Mail-Fenster. Ein klickbarer
Entwurf liegt unter [prototypes/mail-backup/index.html](prototypes/mail-backup/index.html).

## Geltungsbereich und Vollständigkeit

Ein Backup umfasst eine gewählte Quelle und optional alle Nachrichten ab einem Kalendertag
(einschließlich, anhand von IMAP `INTERNALDATE`). Ohne Datum gilt die ganze Quelle. „Server“
meint alle erreichbaren, auswählbaren IMAP-Ordner und ihre Nachrichten.

**Sammelordner und Aliase.** Sammelordner zeigen Mails anderer Ordner noch einmal: `\All`
(RFC 6154) bzw. das ältere `\AllMail` auf jedem Server, `\Flagged` und `\Important` nur bei
Gmail (`X-GM-EXT-1`), das sie als virtuelle Ordner führt. Anderswo erlaubt RFC 8457 diese
Attribute an echten Ordnern, deren Mails nirgends sonst liegen; solche Ordner bekommen keine
Sammelrolle und jede Mail ihre eigene EML. Sammelordner werden zuletzt gelesen, „All Mail“ vor
den übrigen, unabhängig von der Reihenfolge der Ordnerliste. Jede UID eines Sammelordners
bleibt als Ordnerzuordnung erhalten, ihre Rohbytes aber nur einmal: Liegt dieselbe Mail
(gleicher SHA-256, gleiche Länge) schon aus einem **anderen** Ordner im Archiv, wird die UID
als **Alias** (`aliases` im Manifest, mit eigener UID, eigenem Datum und eigenen Flags und
`same_as` = Pfad der vorhandenen EML) gespeichert, sonst als eigene EML; Mails, die nur im
Sammelordner liegen, bleiben so erhalten. Dafür wird jede Mail des Sammelordners in eine private
Spool-Datei (0600) gelesen und dabei gehasht. Kann der Server `X-GM-EXT-1` (Gmail), nennt jede
Mail ihre `X-GM-MSGID`, die in allen Label-Ordnern gleich ist: Ist dieselbe ID schon aus einem
anderen Ordner mit derselben Größe archiviert (auch als eigene EML von „All Mail“), entsteht der
Alias, ohne die Mail noch einmal zu laden. Gewöhnliche Ordner werden nicht zu Aliasen
zusammengefasst: Zwei bytegleiche UIDs dort bleiben zwei EMLs. Einzige Ausnahme ist Mail, die
während des Backups ankam und ein Sammelordner schon als eigene EML archiviert hat, bevor das
Schlussinventar sie im echten Ordner fand: Dort steht sie dann als Alias dieser EML, damit keine
Bytes doppelt im Archiv liegen. Die Zähler `messages` und `bytes` meinen EMLs; die Inventur jedes Ordners
zählt seine Nachrichten und Aliase.

Nicht auswählbare Ordner stehen im Manifest als `selectable: false`; der Servergrund
(`\\Noselect` oder `\\NonExistent`) und der betroffene Pfad erscheinen in `warnings` (englisch
im Archiv, übersetzt im Job, wie bei der Lokalquelle). Ihre Nachrichten können
per IMAP nicht inventarisiert werden. `complete` bezieht sich deshalb nur auf alle erreichbaren,
auswählbaren Ordner und bedeutet keine Zusage über unzugängliche Serverinhalte.

„Lokal“ meint nur Nachrichten, deren **vollständige RFC-822-Originalbytes** auf dem Gerät
vorliegen. Der lokale Text- und Suchcache enthält für ältere Bestände nicht verlässlich
eine EML. Im Modus „Alle Mailtexte offline“ (#263) hält das unabhängige Originaldepot nun
beim Abruf die Rohbytes, SHA-256, Ordner/UIDVALIDITY/UID, `INTERNALDATE` und Flags/Keywords
in der kontobezogenen SQLite-Datei. Erfasst wird beim Sync, beim Nachladen für die Suche,
beim Öffnen einer Mail und beim Laden eines Verlaufs; liegt für dieselbe Stelle (Ordner,
UIDVALIDITY, UID) schon ein Original vor, wird nichts erneut geholt oder geschrieben. Ein
Cache-Auftrag holt auch bereits vorhandene Texte erneut, wenn deren Original noch fehlt.
Andere Cache-Modi legen keine neuen Originale an.

**Aufbewahrung.** Das Depot wird weder beim Pruning von Texten noch beim Entfernen
veralteter Header geleert. Verlässt eine Mail ihre Stelle auf dem Server nachweislich
(Expunge oder `\Deleted` im Sync, Verschieben oder Löschen aus der Suite, neue UIDVALIDITY,
Ordner nicht mehr in der Liste), markiert der Dienst ihr Original als **nicht mehr auf dem
Server** (`gone_at`). Ein lokales Backup lässt solche Originale aus und nennt ihre Zahl als
Warnung; so entstehen keine Doppel aus verschobenen Mails, und geleerte Papierkorb-Mails
kehren beim Restore nicht zurück. Taucht dieselbe UID unter derselben UIDVALIDITY wieder
auf, gilt das Original wieder als vorhanden. Der reguläre Wartungslauf nach jedem Sync
(dort, wo auch Texte ausgedünnt werden) löscht markierte Originale nach **30 Tagen**
(`GONE_ORIGINAL_RETENTION_DAYS`). Originale von Mails, die noch auf dem Server liegen,
bleiben bis zum Entfernen des Kontos; der Platzbedarf wächst bis zur Größe dieser Mails.

**Fehlende Originale.** Ein Original fehlt, wenn zu einem gespeicherten Header an seiner
Stelle (Ordnerpfad, aktuelle UIDVALIDITY des Ordners, UID) kein Original vorliegt; auch
dann, wenn der Server keinen Text liefern konnte. Diese eine Definition gilt für den
Lückenzähler des lokalen Backups, die Schätzung und die Arbeitsliste des Cache-Auftrags
im Modus „Alle Mailtexte offline“. Kann ein Original nicht erfasst werden (Server nennt
kein `INTERNALDATE` oder keine Flags, die Mail verschwindet oder ändert sich beim Lesen,
nach `RFC822.SIZE` folgen weitere Bytes, sie ist zu groß), protokolliert der Dienst das
ohne Inhalte; Sync, Lesen und Suchindex laufen weiter, und das Original bleibt fehlend.
Nur Verbindungsfehler unterbrechen wie bisher. Das Backup holt im Lokal-Modus nichts vom
Server und öffnet keine Verbindung.

Große Originale liest der Offline-Modus in 64-KiB-Teilen und speichert sie ohne
vollständigen RAM-Puffer; danach muss ein Teilabruf ab `RFC822.SIZE` leer sein, sonst
gilt das Original als gekürzt und wird nicht gespeichert. Die SQLite-Grenze
(`SQLITE_MAX_LENGTH`, 10⁹ Bytes, abzüglich 1 MiB für die übrigen Spalten) begrenzt eine
einzelne lokale Mail; eine größere wird gar nicht erst geladen, und der Lückenzähler weist
sie aus. Das Server-Backup liest auch solche Mails direkt per IMAP in Stücken.

„Vollständig“ bedeutet: Alle beim ersten Inventar gefundenen passenden Ordner/UIDs liegen
nach Integritätsprüfung im Archiv, und ein Schlussinventar hat keine ungeklärte Abweichung.
`UIDVALIDITY`-Wechsel, verschwundene UIDs, unlesbare Ordner, Fetch-Fehler, Datenlücken und
Prüfsummenfehler lassen den Lauf scheitern oder als unvollständig enden. Neue Nachrichten
während des Laufs werden im Schlussinventar nachgeholt (höchstens drei Durchgänge); bleibt der
Bestand instabil, gibt es keinen `complete`-Status. IMAP bietet keinen
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
Ein Alias hat keine eigene EML; er verweist mit `same_as` auf eine EML eines anderen Ordners.
Meist steht er in einem Sammelordner; in einem echten Ordner nur für Mail, die während des Backups
ankam (siehe oben).
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

Die Sicherheitsgrenzen des v1-Prüfers liegen bei 100.000 Ordnern, 1 Million Einträgen
(Nachrichten und Aliase zusammen) und 1 PiB logischer Maildaten je Archiv sowie 512 MiB
Manifest (kompakt geschrieben) und 512 MiB Indexdatei; je Eintrag höchstens 256 Flags, höchstens
1.000 Warnungen und Texte bis 1.024 Bytes. Eine längere Warnung (etwa mit einem langen
Ordnernamen) wird mit „…“ gekürzt, weitere Warnungen über der Grenze fasst die letzte als Zahl
zusammen. Der Writer prüft diese Grenzen schon beim Hinzufügen eines Eintrags, nicht erst am
Ende eines langen Laufs. Wird eine Grenze erreicht, meldet der Prüfer einen Fehler; er bescheinigt
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
offline, einschließlich EML-Hashes, Metadaten-Prüfsummen und CSV/Manifest-Abgleich. Ein
abgebrochener oder gescheiterter Lauf wird nicht als finale ZIP-Datei veröffentlicht, ebenso
wenig eine Serverquelle mit ungeklärter Abweichung. Einzige Ausnahme ist die Lokalquelle mit
bekannten, im Archiv benannten Lücken: Sie wird als `incomplete` veröffentlicht (siehe unten).
Die temporäre Datei liegt im Zielordner (standardmäßig dem privaten
verwalteten Backup-Ordner) und wird bei Fehlern entfernt; ein vorhandener Zielname wird
nie überschrieben. Ein gewählter Ordner muss bereits bestehen und darf selbst kein
symbolischer Link sein.

**Vor dem Start.** Bevor ein Job entsteht, prüft der Dienst den Zielordner (verwalteten oder
gewählten): Liegt dort schon etwas unter dem Archivnamen (Datei, Link oder Ordner), lehnt er sofort
mit `conflict` ab („A file named … already exists in the backup folder. Choose another name.“),
statt erst nach einem womöglich stundenlangen Lauf zu scheitern. Das Umbenennen ohne Überschreiben
am Ende bleibt als letzte Sicherung, falls die Datei in der Zwischenzeit entsteht; der Job endet
dann `incomplete` mit derselben Meldung.

**Freier Speicherplatz.** Ein Backup lässt auf dem Datenträger des Zielordners immer mindestens
**2 GiB** frei (feste Reserve statt eines Anteils: Ein Anteil würde auf einer großen, fast leeren
Platte grundlos ablehnen und auf einer kleinen zu wenig übrig lassen). Die Regel gilt für alle
Aufrufer, auch für die App, und braucht keine Freigabe; sie verhindert, dass ein Agent mit
Schreibrecht durch ein Backup nach dem anderen die Platte füllt.

- Beim Start: Ist weniger als die Reserve frei, lehnt der Dienst sofort mit `error` ab und nennt,
  wie viel frei ist (`error` statt `busy`: Ein späterer Versuch hilft nicht von selbst, erst
  Platz zu schaffen).
- Lokalquelle: Nach dem Zählen kennt sie die Bytes ihrer Originale; passen diese nicht samt Reserve
  auf den Datenträger, endet der Job `incomplete`, bevor eine Mail gelesen wird. Die Serverquelle
  kennt die Gesamtgröße vorab nicht (sie liest Größen stapelweise beim Lesen) und verlässt sich
  auf die laufende Prüfung.
- Während des Schreibens: Je 64 MiB, die in Spool-Dateien und Archiv geschrieben wurden, sieht der
  Dienst erneut nach. Fällt der freie Platz unter die Reserve, hält der Job an, entfernt seine
  Temp-Dateien wie bei jedem anderen Fehler und endet `incomplete` mit einer Warnung.
- Der freie Platz ist der des Datenträgers, dessen Einhängepunkt das längste Präfix des
  kanonischen Zielordners ist (`sysinfo`, Linux, macOS und Windows). Lässt er sich nicht
  bestimmen, blockiert das nichts.

Die Serverquelle (MB.3) arbeitet auf einer **eigenen IMAP-Sitzung**, nicht auf der
Befehlsverbindung des Kontos: Sync, Lesen und Senden laufen während eines stundenlangen Backups
weiter. Der Dienst meldet sie an wie jede Verbindung des Kontos (geprüfte öffentliche Adressen,
Passwort aus dem Schlüsselbund des Dienstes, das ihn nie verlässt) und beendet sie nach dem Lauf
mit `LOGOUT`, auch nach Fehler oder Abbruch. Sie sendet nur lesende Befehle: `LIST`, `EXAMINE`
(nie `SELECT`), `UID SEARCH`, `UID FETCH` und `LOGOUT`; ein Test mit einer Erlaubnisliste hält
das fest. Ein per `EXAMINE` geöffneter Ordner ist in der Sitzung als schreibgeschützt markiert
(`Selected::read_only`); `STORE`, `MOVE` und Löschen scheitern darauf, bevor etwas gesendet wird.

Sie liest die Ordnerliste, erst die echten Ordner samt eigenem Schlussinventar, dann die
Sammelordner, und je auswählbarem Ordner zuerst alle UIDs. So landet Mail, die währenddessen in
einem echten Ordner ankam, dort, bevor ein Sammelordner sie als eigene Mail sieht.
Flags, Keywords, `INTERNALDATE`, `RFC822.SIZE` (und bei Gmail `X-GM-MSGID`) holt sie per
`UID FETCH` über UID-Mengen in Stapeln zu 500, nicht je UID; die Flags gelten zum Zeitpunkt dieses
Lesens. Der inklusive Datumsfilter vergleicht den Kalendertag von `INTERNALDATE` in dessen eigener
Zeitzone und greift, bevor ein Body geladen wird. Die Originalbytes liest der gemeinsame
Stückleser (`BodyChunks`, auch für die Restore-Prüfung und das Originaldepot) über
`UID FETCH BODY.PEEK[]<offset.count>` in höchstens 64-KiB-Stücken; danach muss ein Abruf ab
`RFC822.SIZE` leer sein. Ein kürzerer Body oder Bytes nach der gemeldeten Größe machen den Lauf
unvollständig. Spool-Dateien, das Schreiben ins ZIP, die Prüfung und `fsync` laufen außerhalb der
asynchronen Worker.

**Schlussinventar.** Nach den echten Ordnern und noch einmal nach dem letzten Sammelordner liest
sie die Ordnerliste und jedes UID-Inventar erneut, echte Ordner vor Sammelordnern. Neue UIDs unter derselben UIDVALIDITY (während des Laufs eingetroffene Mail) holt sie
nach, solange das Archiv noch offen ist, in höchstens drei Durchgängen; ein Durchgang ohne Neues
beendet das Inventar. Eine verschwundene UID, eine neue UIDVALIDITY, ein verschwundener oder neuer
Ordner, ein Wechsel zwischen auswählbar und nicht auswählbar oder Neues noch im dritten Durchgang
machen den Lauf unvollständig; dann wird keine Datei veröffentlicht. Das archivierte
UID-Inventar wird mit den geschriebenen Nachrichten und Aliasen verglichen. Der Job meldet
Zähler, Phase und Abbruchstatus; `verify` benötigt weder Kontozugang noch Netzwerk. Ein Archiv
einer neueren Torro-Version nennt `verify` als solches, nicht als defekt.

**Veröffentlichen.** `ArchiveWriter::finish` prüft die fertige Datei offline; genau diese
geprüfte Temp-Datei wird danach per `fsync` gesichert, ohne Überschreiben umbenannt und der
Ordnereintrag ebenfalls gesichert, ohne sie ein zweites Mal vollständig zu lesen. Scheitert nur
dieses letzte `fsync` des Ordners, liegt das geprüfte Archiv schon unter seinem Namen: Der Job
meldet es dann vollständig, mit einer Warnung, dass ein Systemabsturz in diesem Moment den neuen
Namen noch kosten könnte. Ein Abbruch
setzt den Job auf `cancelling`; `cancelled` meldet er erst, wenn die Aufgabe steht und ihre
Temp-Dateien entfernt sind. Bis dahin gilt das Konto als beschäftigt. Endet die Aufgabe
unerwartet (Panic), wird der Job `incomplete` mit Warnung statt ewig aktiv zu bleiben. Im Originaldepot werden später
geänderte Flags und Keywords beim IMAP-Abgleich aktualisiert.

Die Lokalquelle (MB.4) hält zu Beginn einen **Schnappschuss** fest: die höchste
Depot-Sequenz und, im selben Augenblick, die Zahl der Header ohne Original. Gesichert wird
genau der Bestand bis zu dieser Sequenz, ohne Originale von Mails, die nicht mehr auf dem
Server liegen, und mit dem inklusiven Datumsfilter. Der Filter vergleicht bei Originalen
den Kalendertag von `INTERNALDATE` in dessen Zeitzone (wie die Serverquelle), bei Headern
ohne Original das vom Sync gespeicherte `INTERNALDATE` (UTC) oder, bei älteren Zeilen, das
Datum der Kopfzeile. Danach erfasste Originale und Header, geänderte Flags und Mails, die
währenddessen den Server verlassen, stören den Lauf nicht; Flags gelten zum Zeitpunkt des
Lesens. Jede EML wird in höchstens 64-KiB-Stücken über SQLites inkrementelles BLOB-Lesen
in eine temporäre Datei kopiert; Länge und SHA-256 müssen stimmen. Der Lauf bricht nur ab,
wenn ein eingeschlossenes Original verschwindet oder sich seine Bytes, sein Ort oder sein
Datum ändern; dann erscheint keine ZIP. Ordnername, Trennzeichen und Rolle stammen aus der
Ordnerliste des letzten Syncs.

Ein lokales Archiv ist nur `complete`, wenn **kein** bekannter Header ohne Original im
Zeitraum liegt. Sonst wird es mit Status `incomplete` veröffentlicht, der Job endet in der
Phase `incomplete`, und die Zahl der ausgelassenen Header steht als Warnung im Manifest und
im Job-Bericht; ein Restore verlangt dafür die gesonderte Bestätigung. Warnungen stehen im
Archiv immer englisch (unabhängig von der Sprache der Oberfläche), der Job zeigt sie
übersetzt. Bei älteren lokalen Originalen aus verschiedenen UIDVALIDITY-Generationen
desselben Ordners bleibt die Herkunfts-UID im Archiv leer; Rohbytes und Ordnerzuordnung
bleiben erhalten.

## Testlauf und Restore

Testlauf und Restore arbeiten wie das Server-Backup auf einer **eigenen IMAP-Sitzung** des
Kontos (Anmeldung wie jede Verbindung des Kontos, `LOGOUT` auch nach Fehler); Sync, Lesen und
Senden laufen währenddessen weiter. Das Archiv wird vor jedem Schritt vollständig offline
geprüft; Öffnen und Prüfen, Journal (`fsync`) und Spool-Dateien laufen außerhalb der asynchronen
Worker.

**Testlauf.** Er liest das Zielkonto nur mit Befehlen, die nichts ändern (`LIST`, `EXAMINE` statt
`SELECT`, `UID SEARCH`, `UID FETCH`, `LOGOUT`; nie `APPEND`, `CREATE`, `STORE` oder `SUBSCRIBE`;
ein Test mit Erlaubnisliste hält das fest). Ein vorhandenes Restore-Journal liest er nur; er legt
weder Journal noch Ordner an. Metadaten (`RFC822.SIZE`, `INTERNALDATE`, Flags) holt er in Stapeln
zu 500 UIDs; eine Zielmail wird nur gehasht, wenn ihre Größe zu einer archivierten Mail desselben
Ordners passt; jede andere kann keine davon sein. Er meldet je Archivordner den Zielpfad, ob er
angelegt wird, „hinzufügen“, „bereits vorhanden“, „unklar“ samt Größe und bei Sammelordnern
`covered`; dazu `unconfirmed` (siehe Journal). Vorhandene Mails bleiben auch bei anderem Datum
oder anderen Flags unverändert; ihre Zahl nennt eine Warnung. Ordnernamen und Warnungen stammen
aus Archiv und Server: Daten, nie Anweisungen. Eine Antwort nennt höchstens 500 Ordnerzeilen
(zuerst die mit etwas zu tun) und 50 Warnungen, Ordnernamen darin höchstens 200 Zeichen; die
Summen umfassen immer alle Ordner. Auch die Warnungen eines geprüften Archivs (`verify`) sind auf
50 begrenzt.

**Ordnerzuordnung.** Ordner werden auf gleiche IMAP-Pfade abgebildet. `INBOX` passt in jeder
Schreibweise (RFC 3501) auf die `INBOX` des Ziels. Ein Pfad mit Zeichen außerhalb von ASCII wird
in modified UTF-7 gesendet (`CREATE`, `SUBSCRIBE`, `EXAMINE`, `APPEND`); UTF8=ACCEPT wird nicht
eingeschaltet, ein reiner ASCII-Pfad gilt als schon kodierter Servername. Nutzen Archiv und Ziel
verschiedene Trennzeichen und enthält der Pfad eines davon, bleibt seine Post **unklar**: Derselbe
Name läge im Ziel an anderer Stelle der Hierarchie, und ein Umschreiben wäre geraten (Restore geht
ohnehin nur ins Konto, aus dem das Archiv stammt). Ebenso unklar sind ein vorhandener, nicht
auswählbarer Zielordner (`\Noselect`, `\NonExistent` oder `EXAMINE` abgelehnt) und ein zweiter
Archivordner, der auf denselben Zielpfad fiele. Fehlende auswählbare Ordner legt der Restore an
und abonniert sie (`SUBSCRIBE`, nur im echten Restore; scheitert es, nennt eine Warnung das).

**Sammelordner.** Ein Ordner mit der Archivrolle `all` oder `flagged` ist `aggregate`, wenn das
Ziel einen auswählbaren Ordner hat, der ihn selbst füllt: einen mit `\All` bzw. `\AllMail`, für
`flagged` einen mit `\Flagged` auf einem Gmail-Ziel. Der Archivordner wird dann auf genau diesen
Zielordner abgebildet, wie er dort auch heißt (etwa `[Gmail]/All Mail` auf `Virtual/All`), und nie
neu angelegt. Seine Aliase (Kopien von Mails anderer Ordner) zählen dann als `covered` und werden
nicht hochgeladen: Der Server zeigt sie selbst (bei Gmail erscheint jede hochgeladene Mail auch in
„All Mail“). Für `flagged` gilt das nur, wenn die Mail aus `same_as` selbst `\Flagged` trägt, denn
Gmail zeigt eine Mail nur dann unter „Starred“; sonst geht die Kopie in den Ordner hoch. `important`
ist nie `aggregate`: Gmail setzt „Important“ nach eigenen Regeln, ein Upload in einen anderen Ordner
stellt die Markierung nicht her. Eigene EMLs eines Sammelordners sind Mails, die beim Backup in
keinem anderen Ordner lagen (etwa bei Gmail archivierte); sie werden dort eingespielt. Füllt das
Ziel den Ordner nicht selbst (anderer Server, `important`, oder ein echter Ordner mit `\Important`
nach RFC 8457), und für jeden
Alias in einem echten Ordner, wird der Alias wie eine eigene Mail mit den Bytes der EML aus
`same_as` in seinen Ordner eingespielt (eigenes Journal-Feld `alias:<n>`). Echte Ordner kommen vor
Sammelordnern dran, damit ein Ziel wie Gmail eine Mail nicht zweimal bekommt. Eine abgedeckte
Kopie zählt im Ergebnis erst als übersprungen, wenn ihre Mail aus `same_as` im eigenen Ordner
vorhanden ist oder hochgeladen wurde; sonst (abgelehnt, Abbruch) ist sie unklar.

**Echter Restore.** Er gilt nur für einen noch gültigen Plan (eine Stunde) **desselben
Aufrufers** und desselben Kontos und verbraucht ihn, sobald er ein Ergebnis liefert. Vor dem
ersten Schreibbefehl prüft er erneut: dasselbe rohe `manifest.json` (sonst `conflict`), bei einem
unvollständigen Archiv `allow_incomplete`, keine unklaren Mails, bei unbestätigten Uploads
`retry_unconfirmed` (sonst `conflict`; der Kern prüft das schon vor der Freigabe). Dann liest er
das Ziel wieder ein, wobei Hashes bekannter UIDs wiederverwendet werden. Das Ziel darf **wachsen**:
neue Ordner und neue Mails sind erlaubt und können den Umfang nur verringern. Er bricht ab, wenn
ein beim Testlauf vorhandener Ordner verschwindet oder nicht mehr auswählbar ist, eine geplante
UID fehlt, eine UIDVALIDITY wechselt oder mehr Schreibvorgänge nötig wären als geprüft.

Danach legt er fehlende Ordner an (`CREATE`, `SUBSCRIBE`) und liest nur die Ordnerliste und den
neuen Ordner neu. Mails werden in Archivreihenfolge mit Originalbytes, `INTERNALDATE` und den
übertragbaren Flags/Keywords per `APPEND` eingespielt (`\Recent` und `\Deleted` nie). Nennt
`PERMANENTFLAGS` des Zielordners kein `\*`, werden Keywords, die er nicht hält, nicht gesendet
(strenge Server lehnen sonst die ganze Mail ab); ihre Zahl steht schon im Testlauf. Vor jedem
`APPEND` liest er neue UIDs des Zielordners (`UID SEARCH UID n:*`, Größe aus `EXAMINE`); fehlt
dabei eine geplante UID, endet der Lauf. Vorhandene Mails werden nie gelöscht oder überschrieben.

**Journal.** Das private Journal (0600, `<Mail-Daten>/restore-journals/`) heißt nach dem SHA-256
von Konto und **rohem** `manifest.json` aus dem ZIP (`RestoreArchive::manifest_sha256`), nicht
nach einem neu serialisierten Manifest; so bleibt der Name über Versionen stabil. Journale der
älteren Form (Hash des neu serialisierten Manifests) werden nicht übernommen; das Format war noch
nicht veröffentlicht, eine Migration gibt es nicht. Vor jedem `APPEND` hält es den Archiveintrag
und die Zahl gleicher Rohmails im Zielordner fest. Danach gilt:

- `APPEND` mit `OK`: Der Ordner wird nachgelesen. Mit `APPENDUID` entscheidet die genannte UID,
  ohne die Zahl der Kopien (genau eine mehr als vorher). Passt, ist der Eintrag erledigt.
- Abgelehnt (`NO`/`BAD`) oder gescheitert, bevor die ganze Mail gesendet war: Sie ist sicher nicht
  gespeichert, der Eintrag wird sofort entfernt. Eine abgelehnte Mail wird übersprungen und
  gezählt, der Lauf geht weiter (eine Ablehnung gilt meist nur dieser Mail, etwa ein zu großer
  Anhang); ein Fehler der Verbindung beendet ihn. Ein späterer Restore versucht sie wieder.
- Mail gesendet, aber keine Antwort: Der Eintrag bleibt, der Lauf endet. Der nächste Testlauf
  gleicht ab: eine Kopie mehr als vorher gilt als vorhanden; gleich viele oder weniger (jemand hat
  inzwischen eine gleiche Kopie gelöscht) als **`unconfirmed`** (nur mit `retry_unconfirmed`
  erneut hochgeladen, genau diese); mehr als eine Kopie mehr als unklar. Als gesendet gilt eine
  Mail, sobald ihr letztes Stück an die Verbindung ging: Scheitert erst das Schreiben dieses
  Stücks oder des abschließenden Zeilenumbruchs, kann der Server sie trotzdem haben, sie ist
  unbestätigt; nur ein Fehler bei früheren Stücken heißt sicher nicht gesendet.
- Der Server meldet `OK`, aber im Ordner liegt keine bytegleiche Kopie („restored mail differs“,
  etwa weil er Kopfzeilen umschreibt): Mit `APPENDUID` merkt das Journal die UID; die geänderte
  Kopie zählt als wiederhergestellt, solange sie dort liegt, und wird nie ein zweites Mal
  hochgeladen (Warnung im Ergebnis). Ohne `APPENDUID` lässt sich nicht sagen, welche Mail es war:
  Der Eintrag bleibt offen, der Lauf endet mit einem Hinweis, dass die Mail vielleicht verändert
  angekommen ist und der Ordner vor einem neuen Upload zu prüfen ist; erneut hochgeladen wird sie
  nur mit `retry_unconfirmed`, also nach der Entscheidung eines Menschen. Oberfläche und CLI sagen
  bei unbestätigten Uploads ebenso, dass sie nicht oder verändert angekommen sein können.
- Lehnt der Server das Öffnen eines Zielordners mitten im Lauf ab (`NO` auf `EXAMINE`), endet
  der Lauf mit diesem Grund, nicht als verlorene Verbindung.

Ein leeres Journal wird gelöscht. Liefert ein anderer Client genau zwischen Prüfung und `APPEND`
dieselbe Nachricht ein, kann IMAP keine atomare Dublettenprüfung garantieren; mehrere neue gleiche
Kopien beenden den Lauf als unklar. Für eine eindeutige Wiederherstellung sollte währenddessen
kein zweiter Client in die Zielordner schreiben. `Message-ID` allein ist kein Duplikatschlüssel.

**Ergebnis.** Sobald ein Schreibbefehl gesendet war, antwortet der Restore immer mit dem bisherigen
Ergebnis; ein späterer Fehler beendet den Lauf mit einer Warnung, die den Grund nennt.
`added` zählt hochgeladene Mails, `skipped` vorhandene und abgedeckte, `uncertain` alle übrigen
Archiveinträge, die danach nicht als vorhanden bestätigt sind (abgelehnt, unbestätigt oder nach
einem Abbruch nicht mehr versucht). Fehler vor dem ersten Schreibbefehl sind Fehlercodes
(`conflict`, `offline`, `auth`, `not-found`, `error`).

**Pläne.** Ein Plan gehört dem Aufrufer, der ihn erstellt hat: ein Client sieht und nutzt nur
seine eigenen, die App nur ihre; andere gelten als nicht vorhanden (`not-found`). Ein Client hält
höchstens vier Pläne, die App sechzehn; ein neuer verdrängt den ältesten desselben Aufrufers.
Laufende Testläufe zählen mit: Ihren Platz belegen sie, bevor das Archiv geöffnet wird, und geben
ihn zurück, wenn der Lauf scheitert oder die Anfrage abbricht. Laufen schon so viele Testläufe
eines Aufrufers, wie er Pläne halten darf, antwortet der nächste sofort `busy`. Sind Pläne und
laufende Testläufe aller Aufrufer zusammen 128 und hat der Aufrufer keinen eigenen Plan, den der
neue verdrängen könnte, ebenfalls `busy`. Laufen zwei Restores, wartet der zweite.

**Ordnernamen.** Jede Ordnerzeile eines Plans nennt den Ordner zweimal: `archive_path` und
`target_path` sind die genauen IMAP-Namen (modified UTF-7, z. B. `Entw&APw-rfe`), mit denen der
Ordner gemeint ist; `archive_display` und `target_display` sind dieselben Namen für Menschen
(`Entwürfe`). Oberfläche, CLI und Freigabe zeigen nur die lesbaren Namen; ältere Kerne ohne diese
Felder zeigen den IMAP-Namen. Die Freigabe eines Restores nennt das Konto mit der Bezeichnung, die
der Kern dafür führt (wie beim Backup), nie mit der Kennung aus dem Text des Clients.

## Oberfläche, CLI und Rechte

Unter **Mail-Fenster → Einstellungen → Konten → Konto** stehen Quelle, inklusives Datum,
Archivname, Start, Fortschritt/Abbruch, Archivprüfung, Restore-Testlauf und explizit
bestätigter Restore. Die Seite schickt nur `mail.backup_*`- und `mail.restore_*`-Requests;
gelesen, geschrieben und geprüft wird im Dienst.

**Speicherort und Archiv.** Ohne Auswahl ist der Backup-Ordner
`<Torro-Datenverzeichnis>/mail/backups/` (Platzhalter im Feld: „Managed folder“). „Choose…“
neben „Backup folder“ öffnet den Ordnerdialog des Systems, „Choose…“ neben „Archive to inspect“
den Dateidialog mit Filter auf `.zip`; beide Felder bleiben als Anzeige und zum Eintippen. Für
einen eigenen Zielordner braucht es einen absoluten, bereits vorhandenen Ordner und einen
Archivnamen; der Name folgt derselben Regel wie im Dienst (einfacher `.zip`-Name ohne Ordner).
Die zuletzt verwendete Quelle, Datumsgrenze und der Ordner werden mit dem Mail-Konto
gespeichert (`MailSettings.backup`). Enter in einem Feld startet die Aktion seiner Gruppe
(„Create backup“ bzw. „Verify archive“).

**Job.** Der Fortschritt steht als Statuszeile mit Phase und „Messages · Folders · Size“, darunter
Archivname und Warnungen; „Refresh progress“ und „Cancel backup“ stehen in einer Zeile. Ein
laufender Auftrag wird alle zehn Sekunden aktualisiert. Nach „Cancel backup“ zeigt der Job
„Cancelling…“, bis die Aufgabe wirklich steht; bis dahin bleibt „Create backup“ aus. Kennt der
Dienst den Job nicht mehr (`not-found`, etwa nach einem Neustart), lässt die Seite ihn los,
gibt „Create backup“ frei und sagt das unter dem Knopf. Dafür reicht das Mail-Fenster die
Fehlercodes der Backup-Requests bis zur Seite durch.

**Testlauf und Restore.** Während Prüfung oder Testlauf ist das Archivfeld gesperrt; eine
Antwort, die nicht zur angefragten Datei passt, wird verworfen. Der Plan nennt sein Archiv,
die Zähler, je Ordner Ziel, „neu“ oder „vorhanden“ und Zähler. Ein Sammelordner wie `\All`
(`aggregate`) sagt, dass dort nur Mails ergänzt werden, die in keinem anderen Ordner liegen,
und wie viele Kopien (`covered`) deshalb nicht hochgeladen werden. „Restore mail“ bleibt aus,
bis „I reviewed the plan“ gewählt ist, keine Mail unklar ist, ein unvollständiges Archiv mit
„Accept incomplete archive“ und unbestätigte Uploads eines früheren Restores (`unconfirmed`)
mit „Upload unconfirmed mail again“ (`retry_unconfirmed`) eigens bestätigt sind. Ein neuer
Testlauf setzt alle drei Bestätigungen zurück; ein fertiger Restore ebenso, und er verwirft den
verbrauchten Plan, sodass „Restore mail“ erst nach einem neuen Testlauf wieder geht. Status, Warnungen und Fehler tragen ein Icon
(fertig: Häkchen in OK-Grün, Warnung und unvollständig: Hinweis in Attention, Fehler: Hinweis in
der Fehlerfarbe) und brechen in schmalen Fenstern um. Fehler erscheinen bei der Gruppe, deren
Knopf sie ausgelöst hat; das Ändern des Archivfelds löscht keine Fehler des Backups.

**Sichtprüfung.** `torroinbox --demo --view mail --open <zustand>` öffnet die Einstellungen bei
Accounts auf der Seite des ersten Kontos: `backup` (laufendes Backup), `backup-plan` (Testlauf
eines unvollständigen Archivs mit Ordnertabelle, Sammelordner und unbestätigten Uploads),
`backup-error` (verlorener Job) und `backup-done` (fertiges Backup, geprüftes Archiv, Restore-
Ergebnis). Die Demo fragt keinen Job ab und beantwortet Backup-Knöpfe mit „The demo makes no
backups and restores nothing.“

CLI-Form, konsistent mit bestehenden `ctl`-Befehlen (`--json` darf auch vor `mail` stehen):

```text
torroworkplace ctl mail backup create --account mail:anna@example.org --source server --since 2020-01-01 --file anna.zip --wait --json
torroworkplace ctl mail backup create --account mail:anna@example.org --source local --file /mein/backups/anna.zip --wait --json
torroworkplace ctl mail backup status --account mail:anna@example.org --job <job-id> --json
torroworkplace ctl mail backup cancel --account mail:anna@example.org --job <job-id> --json
torroworkplace ctl mail backup verify --file anna.zip --account mail:anna@example.org --json
torroworkplace ctl mail restore --file ./backup.zip --account mail:anna@example.org --dry-run --json
torroworkplace ctl mail restore --file /erlaubter/ordner/backup.zip --account mail:anna@example.org --apply --json
```

Ohne `--apply` ist `restore` ein reiner Testlauf. Mit `--apply` plant die CLI zunächst neu,
prüft auf unklare Nachrichten und wendet genau diesen Plan an. Bei einem unvollständigen
Archiv ist zusätzlich `--allow-incomplete` nötig, bei unbestätigten Uploads eines früheren
Restores `--retry-unconfirmed` (beide nur mit `--apply`). Fehlt eine nötige Freigabe, wendet
die CLI nichts an, gibt den Plan aus und nennt den Grund auf stderr.

`--file`: Ein reiner Name (`anna.zip`) bleibt ein Name im verwalteten Ordner. Ein Pfad mit
Trennzeichen oder führendem `.` (`./anna.zip`, `sub/anna.zip`) ist relativ zum aktuellen
Verzeichnis und wird vor dem Senden absolut gemacht, weil der Dienst dieses Verzeichnis nicht
kennt. Beim App-Schlüssel darf `--file` ein absoluter Pfad sein; bei Client-Schlüsseln bleibt es
ein einfacher ZIP-Name im verwalteten Ordner.

`--wait` fragt den Backup-Status alle zwei Sekunden bis zu einem Endzustand ab
(`complete`, `incomplete`, `cancelled`; `cancelling` wartet weiter). Es hat **kein Zeitlimit**,
und **Strg-C beendet nur die CLI, nicht den Job**; abbrechen geht mit `ctl mail backup cancel`.

Ausgabe: Ohne `--json` stehen Job, Prüfung, Plan (mit einer Zeile je Ordner) und Ergebnis als
Zeilen für Menschen auf stdout, mit `--json` die strukturierte Endantwort. Fehlermeldungen des
Dienstes gehen auf stderr.

Exit-Codes von `ctl mail`:

| Code | Bedeutung |
|---|---|
| 0 | erledigt: Job läuft oder ist vollständig, erfolgreicher `cancel` (auch noch `cancelling`), vollständiges Archiv, Testlauf ohne Befund, Restore ohne unklare Mail |
| 1 | Fehler des Dienstes oder keine Verbindung (Dienst läuft nicht) |
| 2 | Ergebnis braucht eine Prüfung durch Menschen: Backup oder Archiv unvollständig, Testlauf mit unklaren oder unbestätigten Mails oder unvollständigem Archiv ohne passende Freigabe-Flags, Restore mit unklaren Mails |
| 3 | Backup abgebrochen (`create --wait`, `status`) |
| 64 | Syntaxfehler in den Argumenten |

Kontozugang kommt aus dem Schlüsselbund des laufenden Dienstes, nie aus CLI-Argumenten oder
Logs. Die bestehenden `mail`-Rechte gelten: Erstellen/Abbrechen benötigen `write`,
Status/Prüfen/Testlauf `read`, echtes Wiederherstellen `admin` samt Personenfreigabe bei
MCP-Nutzung. MCP-Clients dürfen nur ZIP-Basisnamen aus dem verwalteten Backup-Ordner
angeben; freie Pfade bleiben ihnen versperrt.

## Abnahme

- Anonymes Testkonto mit verschachtelten/Unicode-Ordnern, doppelter Ordnerkopie,
  Flags/Keywords, großer Mail, MIME-Anhang, Datum vor/nach Grenze und paralleler Änderung.
- ZIP mit Systemwerkzeug öffnen, `.eml` unabhängig lesen, `SHA256SUMS` prüfen;
  manipuliertes Archiv wird vor jedem Restore abgewiesen.
- Lokal-Modus ohne Original zählt fehlende Inhalte und markiert sich nicht als 100 %:
  Archiv und Job `incomplete`, Zahl der ausgelassenen Header als Warnung (englisch im
  Archiv, übersetzt im Job). Lokal-Modus öffnet keine Verbindung (Test mit einem Port, der
  jede Verbindung zählt); Datum, Ordnername, Trennzeichen und Rolle stimmen im Archiv.
- Nach Verschieben und UIDVALIDITY-Wechsel enthält ein lokales Backup jede Mail einmal;
  Originale nicht mehr vorhandener Mails werden ausgelassen, gezählt und nach 30 Tagen
  gelöscht. Ein Original, das nicht erfasst werden kann, hält Sync und Suchindex nicht auf.
- Testlauf gegen instrumentierten IMAP-Testserver enthält **keinen Schreibbefehl**.
- Restore in leeres und teilweise gefülltes Ziel, Unterbrechung direkt nach `APPEND`,
  Wiederaufnahme und Abgleich; keine stille Doppelung.
- UI in allen Größenklassen und hell/dunkel sichten; CLI-JSON/Exit-Codes und
  Rechtefälle mit Tests prüfen.

Spätere Ausbaustufen: inkrementelle Sicherungen, Rotation/Aufbewahrung und Zeitplan.
