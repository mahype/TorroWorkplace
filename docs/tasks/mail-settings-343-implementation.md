# Mail-Einstellungen: Umsetzung von Issue #343

Quelle: https://github.com/mahype/TorroWorkplace/issues/343 und `docs/prototypes/mail-settings-refresh/`.

Diese Liste bildet die Produktanforderungen ab. Ein Punkt gilt erst als fertig, wenn native Bedienung, Speicherung und ein passender Laufzeit- oder Testnachweis vorliegen. Die HTML-Mockups selbst sind keine Implementierung.

## Stand nach #397

Die Umsetzung ist mit Issue #397 (PR #407) auf `main` gekommen. Dabei gilt, was `main` inzwischen hatte: fünf eingebaute Hauptkategorien mit „Unsorted“ (#338), eigene ab `custom_group_6` mit fünf Farben, Filter statt Verstecken als fünfter Abschnitt (#339), der Platz *Sortieren* auch im Einrichtungsassistenten (#325), Laden Antwort für Antwort (#394/#395) und Kontakte als Sortierstufe (#390). Nur ein geöffnetes Konto ist breiter als die normale Lesebreite. Der Zwischenstand unten beschreibt den Branch vor dieser Übernahme.

## Zwischenstand im Worktree (vor #397)

- Kontenliste und Kontodetail verwenden jetzt vergleichbare Mailgrößen (RFC822.SIZE), einen schraffierten Soll-Anteil und den angrenzenden Indexstreifen. Fehlende Größen verhindern eine irreführende Verhältnisgrafik. Die tatsächliche SQLite-Größe steht gesondert.
- Die allgemeine Speicher-Seite ist aus der Navigation entfernt; Offline-Auswahl und Anhang-Aufbewahrung stehen im Konto-Reiter „Speicher & Suche“. Die Mail-Seiten durften auf breiten Fenstern bis 1200 Pixel nutzen; seit #397 gilt das nur für ein geöffnetes Konto (zwei Formulare nebeneinander).
- Datum und maximale Größe sind als gespeicherte Konto-Strategien ergänzt. Die Synchronisierung und der Cache-Job verwenden denselben Auswahlbereich; eine bestehende 90-Tage-Strategie erscheint als entsprechendes Datum. Der Maximalwert verwendet bislang die gemeldete RFC822.SIZE-Summe und ist noch keine harte Grenze für Anhang-Cache, Index und Datenbankdatei.
- Die Kategorisierung hat die Reihenfolge Automatische Einsortierung → Kategorien; Regel-Editoren stehen innerhalb ihrer Regelkarte.
- Hauptkategorien haben nun stabile eigene IDs, Namen, sichtbare Icons und eine wählbare Designfarbe. Die eingebauten Gruppen werden bei der Datenbankmigration als bearbeitbare Hauptkategorien angelegt (seit #397 fünf, mit „Unsorted“, Schema 18). Neue Hauptkategorien können leer beginnen; vorhandene Unterkategorien behalten bei einer Umbenennung ihre Schlüssel. Die Kategorienseite zeigt offene Gruppen und direkt bearbeitbare Beschreibungen der Unterkategorien.
- Neue Regeln werden an Position 0 gespeichert. Die Modellauswahl zeigt nur aktivierte Entscheidungsmodelle und gruppiert mehrere Anbieter. Kategorie-Icons erscheinen als sichtbares Raster.
- „Lesen & Schreiben“ enthält die Standard-Absenderadresse, das Schreibformat und eine hohe Signaturfläche mit Formatierungsaktionen. Formatierte Signaturen werden sicher in HTML und als Textkopie versendet. Der Konto-Reiter zeigt Identität und Verbindung, ohne die Signatur zu duplizieren.
- Die frühere Anhang-Vorabladeoption ist auch für alte gespeicherte Policies deaktiviert. Alte „Nur Kopfzeilen“-Konten werden ausdrücklich zur Auswahl einer der drei neuen Strategien aufgefordert.
- Das Backup-Formular öffnet als Overlay; Quelle, Stichtag mit Kalenderwahl, Archivname und Ziel stehen zusammen. Fortschritt und Wiederherstellung bleiben im Konto-Reiter sichtbar.
- Der Konto-Reiter enthält nun Benutzername und maskiertes Passwort direkt neben Name und Adresse. IMAP und SMTP lassen sich manuell mit getrennten Host-, Port- und TLS/STARTTLS-Feldern bearbeiten. „Zugangsdaten testen“ prüft die eingegebenen Werte ohne Speicherung; bei leerem Passwortfeld verwendet der Kern das vorhandene Keychain-Geheimnis. „Verbindung speichern“ prüft und speichert die Änderungen nach erfolgreicher Anmeldung. Der Test-/Speicherbefehl ist auf den App-Schlüssel beschränkt.
- Die MB-Auswahl berechnet ihre Größenkante beim Synchronisieren nur einmal pro Stapel und zieht bereits gespeicherte Anhänge von der Grenze ab. Beim Ändern der Auswahl und im Cache-Job werden ältere Anhangdateien nötigenfalls entfernt und die Offline-Texte neu zugeschnitten. Die Auswahl bleibt dennoch eine RFC822-Größenschätzung; Index und Datenbankdatei sind noch keine hart begrenzten physischen Bytes.
- Die elf Prüffelder des Mockups mit passenden Vergleichen sind im nativen Regeleditor und in der Regelengine vorhanden. Eingang und erneutes Einsortieren liefern Antwortadresse, Empfänger, Anhangdaten, Nachrichtengröße, List-ID und Konto. Fehlende Metadaten erfüllen keine negativen Bedingungen; eine negative Textregel wartet auf den vollständigen Nachrichtentext. Gespeicherte Regeln ohne Vergleich behalten ihre bisherige Bedeutung. Die Bestätigung nennt auch Negationen und Größenvergleiche ausdrücklich.
- Bei der nativen Sichtprüfung aus `torro-inbox-mail --demo` wurde eine zu kurze Kontozeile gefunden: Größen und Balken waren abgeschnitten. Die Zeilenhöhe richtet sich nun nach dem Inhalt. Während die Mail-Einstellungen offen sind, nutzt die App links die schmale Symbolleiste statt der vollen Mail-Seitenleiste; die Kontenübersicht zeigt damit die Größen-Spalte und beide Balken auf einer normalen Fensterbreite. Regelkarten haben einen sichtbaren Stift statt eines Navigationspfeils. Die seltene Massenaktion „Bestehende E-Mails neu einsortieren“ zeigt zunächst eine kompakte Zeile; Konten, Ordner und Zeitraum erscheinen erst nach „Einrichten…“.
- Die Unterkategorie-Beschreibungen stehen jetzt in offenen Karten ohne feste Zeilenhöhe. Bei normaler Inhaltsbreite stehen zwei Karten nebeneinander, im schmalen Fenster eine. Die native Demo zeigt die zweispaltigen Karten mit sichtbarem Stift und vollständig lesbarem Text; der Fenstergrößentest baut alle Mail-Einstellungsseiten auch bei 360 Pixeln Breite.
- Der KI-Schritt des Einrichtungsassistenten lädt aktivierte Entscheidungsmodelle der verbundenen Konten einmal pro Kontobestand. Er empfiehlt die Modellsortierung mit dem ersten geeigneten Modell, lässt die Wahl und den Schalter ändern und speichert die Zuordnung beim Weitergehen. Ohne geeignetes Modell wird nichts stillschweigend zugeordnet. Die Auswahl respektiert die Mail-Cloud-Freigabe; der Weiter-Knopf wartet auf die Modellliste.
- Die Aufbewahrungsdauer bereits geladener Anhänge steht nun in derselben Karte wie die drei Offline-Strategien. Beide Werte werden erst mit „Offline-Auswahl speichern“ als eine Policy übernommen; ein gezielter UI-Test prüft die gemeinsame Änderung. Der Hinweis zur MB-Grenze benennt die zusätzlich benötigten Bytes für Kopfzeilen und Suchindex ausdrücklich.
- Die Kontoeinstellungen zeigen nun die sechs IMAP-Systemordner direkt unter den Serverangaben. Erkannte Zuordnungen stehen sichtbar in der rechten Spalte; offene Zuordnungen führen direkt zur vorhandenen Ordnerauswahl. Bis zum ersten Ordnerabgleich wird kein Ordnername geraten.
- Die native Demo des Konto-Reiters „Speicher & Suche“ zeigt den Größenvergleich, die drei Radio-Optionen und die Anhang-Aufbewahrung in einer Karte. Für schmale Karten stehen Datumsfeld, MB-Feld und Anhang-Auswahl unter ihren Beschriftungen; Warnungen und Fehler dürfen umbrechen, statt in eine feste Zeilenhöhe gezwungen zu werden.
- Die Sichtprüfung von `--demo --open account-writing` im 1100-Punkt-Fenster zeigte zunächst untereinander gestapelte Kontodaten und Server. Die Umschaltbreite ist korrigiert: jetzt stehen die vier Zugangsfelder links und Verbindung plus offene Systemordner rechts. Die beiden Verbindungsaktionen stehen in der schmalen rechten Spalte untereinander.
- `--demo --settings reading` zeigt Leseformat, Farben, Standard-Absender, Schreibformat und die acht Zeilen hohe Signatur mit Formatierungsleiste ohne Vorschau. Ein langer Erklärungstext unter den zwei Leseoptionen ist entfernt, damit die Signatur früher sichtbar wird.
- Der vollständige Push-Gate-Abgleich hat nicht mehr verwendete Statistik-Karten und deren Tests aus dem Code entfernt. Clippy für alle Workspace-Ziele und `cargo test --workspace` bestehen. Der Erstabgleich-Test hat jetzt eine eindeutig ältere Nachricht statt eines sekundengenauen 90-Tage-Grenzfalls; ein unter Vollast einmal abgelaufener Erweiterungs-Test bestand isoliert und im zweiten vollständigen Lauf.
- Build und gezielte Tests liefen; die native Sichtprüfung in allen Größenklassen folgte mit #397.

## Navigation und Oberfläche

- [x] Kontenübersicht C mit einer Größen-Spalte, Ist/Soll/Online-Balken, dichtem Indexstreifen und gemeinsamer Legende.
- [x] Kontodetail mit Einstellungen und Speicher & Suche; alle Kontenfelder und Serverangaben in den vorgesehenen Spalten.
- [x] Allgemein mit Lesen & Schreiben sowie Anhängen; allgemeine Speicher-&-Index-Seite entfällt.
- [x] Kategorisierung mit Automatische Einsortierung vor Kategorien, breiten Karten und schmalem responsivem Layout.

## Konten und Verbindung

- [x] Anzeigename, Adresse, Benutzername und Passwort zusammen; Zugangsdaten testen.
- [x] IMAP/SMTP Host, Port und TLS/STARTTLS getrennt bearbeiten, automatische Erkennung und manuelle Angaben.
- [x] POP3 bleibt ein Mockup und wird ohne Konto-, Synchronisations- und Ordnerlogik nicht angeboten.
- [x] Systemordner bei IMAP sichtbar; Backup als Overlay mit Quelle, Zeitraum, Ziel, Archivname und Kalenderwahl.

## Speicher und Suche

- [x] Genau eine Offline-Strategie: alle, ab Datum, lokale MiB-Grenze (neueste zuerst); Kopfzeilen aller bekannten Nachrichten bleiben synchronisiert.
- [x] Anhänge: keine, 1/7/30 Tage oder alle behalten; Speichergrenze zählt Anhänge mit und hat Vorrang.
- [x] Vergleichbare lokale/Online-Byte-Werte, Soll-Schätzung auf derselben Größenskala, keine Kontingent-Verwechslung; fehlende Werte offen kennzeichnen.
- [x] Gelber Indexstreifen innerhalb des lokalen Segments und ohne redundante Quote; geladene Texte werden für die Volltextsuche nachgezogen.
- [x] Redundante Statistik- und Wartungsboxen entfernen.

## Lesen, Schreiben und Anhänge

- [x] Leseformat und Farben, Standard-Absenderadresse, Schreibformat und große formatierbare Signatur zusammenführen.
- [x] Signatur sicher speichern und beim Verfassen/Antworten als formatierten Inhalt verwenden.
- [x] Anhang-Standardaktion, Downloads-Ordner und Speicherziele übersichtlich darstellen.

## Kategorisierung

- [x] Allgemeine Regeln vor Modell, erste passende Regel gewinnt; Regeln in Reihenfolge und direkt in ihrer eigenen Karte bearbeiten.
- [x] Regel-Prüffelder und passende Vergleiche gemäß Mockup, Ziel-Unterkategorie, Ausnahmen und Datenverfügbarkeit korrekt behandeln.
- [x] Nur aktivierte Entscheidungsmodelle nach Anbieter anbieten; beim empfohlenen Wizard-Weg aktiviert und vorbelegt, separat abschaltbar.
- [x] Bestehende E-Mails mit klarer Option „Alle E-Mails“ erneut einsortieren; der Zeitraumfilter ist dann leer.
- [x] Bearbeitbare Hauptkategorien mit stabilen IDs, Name, Icon und Farbe; Unterkategorien mit Name, Icon, Beschreibung und Zuordnung. Bestandsdaten migrieren. Neue Gruppen bleiben beim Umbenennen und nach Neustart stabil; Löschen von Hauptkategorien ist nicht Teil des Mockups.
- [x] Sichtbares Icon-Raster und Stift-Button, offene Unterkategorie-Karten mit direkt bearbeitbarer Beschreibung.

## Abnahme

- [x] Schmale Fensterbreiten visuell prüfen: mit #397 in allen sechs Größenklassen (`--demo`), Muss-Funde behoben.
- [x] Gezielte Daten-/UI-Tests und Repository-Prüfungen ausführen.
- [ ] Letzte Laufzeitabnahme mit echtem IMAP-Konto und KI-Entscheidungsmodell; die Demo und Unit-Tests decken die Bedienung und Datenpfade ab.
