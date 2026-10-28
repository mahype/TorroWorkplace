# Mail-Einstellungen: Umsetzung von Issue #343

Quelle: https://github.com/mahype/TorroWorkplace/issues/343 und `docs/prototypes/mail-settings-refresh/`.

Diese Liste bildet die Produktanforderungen ab. Ein Punkt gilt erst als fertig, wenn native Bedienung, Speicherung und ein passender Laufzeit- oder Testnachweis vorliegen. Die HTML-Mockups selbst sind keine Implementierung.

## Zwischenstand im Worktree

- Kontenliste und Kontodetail verwenden jetzt vergleichbare Mailgrößen (RFC822.SIZE), einen schraffierten Soll-Anteil und den angrenzenden Indexstreifen. Fehlende Größen verhindern eine irreführende Verhältnisgrafik. Die tatsächliche SQLite-Größe steht gesondert.
- Die allgemeine Speicher-Seite ist aus der Navigation entfernt; Offline-Auswahl und Anhang-Aufbewahrung stehen im Konto-Reiter „Speicher & Suche“. Die Mail-Seiten dürfen auf breiten Fenstern bis 1200 Pixel nutzen.
- Datum und maximale Größe sind als gespeicherte Konto-Strategien ergänzt. Die Synchronisierung und der Cache-Job verwenden denselben Auswahlbereich; eine bestehende 90-Tage-Strategie erscheint als entsprechendes Datum. Der Maximalwert verwendet bislang die gemeldete RFC822.SIZE-Summe und ist noch keine harte Grenze für Anhang-Cache, Index und Datenbankdatei.
- Die Kategorisierung hat die Reihenfolge Automatische Einsortierung → Kategorien; Regel-Editoren stehen innerhalb ihrer Regelkarte.
- Hauptkategorien haben nun stabile eigene IDs, Namen, sichtbare Icons und eine wählbare Designfarbe. Die vier bisherigen Gruppen werden bei der Datenbankmigration als bearbeitbare Hauptkategorien angelegt. Neue Hauptkategorien können leer beginnen; vorhandene Unterkategorien behalten bei einer Umbenennung ihre Schlüssel. Die Kategorienseite zeigt offene Gruppen und direkt bearbeitbare Beschreibungen der Unterkategorien.
- Neue Regeln werden an Position 0 gespeichert. Die Modellauswahl zeigt nur aktivierte Entscheidungsmodelle und gruppiert mehrere Anbieter. Kategorie-Icons erscheinen als sichtbares Raster.
- „Lesen & Schreiben“ enthält die Standard-Absenderadresse, das Schreibformat und eine hohe Signaturfläche mit Formatierungsaktionen. Formatierte Signaturen werden sicher in HTML und als Textkopie versendet. Der Konto-Reiter zeigt Identität und Verbindung, ohne die Signatur zu duplizieren.
- Die frühere Anhang-Vorabladeoption ist auch für alte gespeicherte Policies deaktiviert. Alte „Nur Kopfzeilen“-Konten werden ausdrücklich zur Auswahl einer der drei neuen Strategien aufgefordert.
- Das Backup-Formular öffnet als Overlay; Quelle, Stichtag mit Kalenderwahl, Archivname und Ziel stehen zusammen. Fortschritt und Wiederherstellung bleiben im Konto-Reiter sichtbar.
- Der Konto-Reiter enthält nun Benutzername und maskiertes Passwort direkt neben Name und Adresse. IMAP und SMTP lassen sich manuell mit getrennten Host-, Port- und TLS/STARTTLS-Feldern bearbeiten. „Zugangsdaten testen“ prüft die eingegebenen Werte ohne Speicherung; bei leerem Passwortfeld verwendet der Kern das vorhandene Keychain-Geheimnis. „Verbindung speichern“ prüft und speichert die Änderungen nach erfolgreicher Anmeldung. Der Test-/Speicherbefehl ist auf den App-Schlüssel beschränkt.
- Die MB-Auswahl berechnet ihre Größenkante beim Synchronisieren nur einmal pro Stapel und zieht bereits gespeicherte Anhänge von der Grenze ab. Beim Ändern der Auswahl und im Cache-Job werden ältere Anhangdateien nötigenfalls entfernt und die Offline-Texte neu zugeschnitten. Die Auswahl bleibt dennoch eine RFC822-Größenschätzung; Index und Datenbankdatei sind noch keine hart begrenzten physischen Bytes.
- Die elf Prüffelder des Mockups mit passenden Vergleichen sind im nativen Regeleditor und in der Regelengine vorhanden. Eingang und erneutes Einsortieren liefern Antwortadresse, Empfänger, Anhangdaten, Nachrichtengröße, List-ID und Konto. Fehlende Metadaten erfüllen keine negativen Bedingungen; eine negative Textregel wartet auf den vollständigen Nachrichtentext. Gespeicherte Regeln ohne Vergleich behalten ihre bisherige Bedeutung. Die Bestätigung nennt auch Negationen und Größenvergleiche ausdrücklich.
- Bei der nativen Sichtprüfung aus `torro-inbox-mail --demo` wurde eine zu kurze Kontozeile gefunden: Größen und Balken waren abgeschnitten. Die Zeilenhöhe richtet sich nun nach dem Inhalt. Während die Mail-Einstellungen offen sind, nutzt die App links die schmale Symbolleiste statt der vollen Mail-Seitenleiste; die Kontenübersicht zeigt damit die Größen-Spalte und beide Balken auf einer normalen Fensterbreite. Regelkarten haben einen sichtbaren Stift statt eines Navigationspfeils. Die seltene Massenaktion „Bestehende E-Mails neu einsortieren“ zeigt zunächst eine kompakte Zeile; Konten, Ordner und Zeitraum erscheinen erst nach „Einrichten…“.
- Die Unterkategorie-Beschreibungen stehen jetzt in offenen Karten ohne feste Zeilenhöhe. Bei normaler Inhaltsbreite stehen zwei Karten nebeneinander, im schmalen Fenster eine. Die native Demo zeigt die zweispaltigen Karten mit sichtbarem Stift und vollständig lesbarem Text; der Fenstergrößentest baut alle Mail-Einstellungsseiten auch bei 360 Pixeln Breite.
- Build und gezielte Tests laufen; native Sichtprüfung und die übrigen Anforderungen stehen noch aus.

## Navigation und Oberfläche

- [ ] Kontenübersicht C mit einer Größen-Spalte, Ist/Soll/Online-Balken, dichtem Indexstreifen und gemeinsamer Legende.
- [ ] Kontodetail mit Einstellungen und Speicher & Suche; alle Kontenfelder und Serverangaben in den vorgesehenen Spalten.
- [ ] Allgemein mit Lesen & Schreiben sowie Anhängen; allgemeine Speicher-&-Index-Seite entfällt.
- [ ] Kategorisierung mit Automatische Einsortierung vor Kategorien, breiten Karten und schmalem responsivem Layout. Zwei Tabs und die Reihenfolge Regeln → Modell sind begonnen; visuelle Abnahme steht aus.

## Konten und Verbindung

- [x] Anzeigename, Adresse, Benutzername und Passwort zusammen; Zugangsdaten testen.
- [x] IMAP/SMTP Host, Port und TLS/STARTTLS getrennt bearbeiten, automatische Erkennung und manuelle Angaben.
- [ ] POP3 nur nach echter Unterstützung in Konto-, Synchronisations- und Ordnerlogik anbieten.
- [ ] Systemordner bei IMAP sichtbar, bei POP3 nicht; Backup als Overlay mit Quelle, Zeitraum, Ziel, Archivname und Kalenderwahl.

## Speicher und Suche

- [ ] Genau eine Offline-Strategie: alle, ab Datum, lokale MB-Grenze (neueste zuerst); Kopfzeilen aller bekannten Nachrichten bleiben synchronisiert.
- [ ] Anhänge: keine, 1/7/30 Tage oder alle behalten; Speichergrenze zählt Anhänge mit und hat Vorrang.
- [ ] Vergleichbare lokale/Online-Byte-Werte, belastbare Soll-Schätzung, keine Kontingent-Verwechslung; fehlende Werte offen kennzeichnen.
- [ ] Indizierung geladener Inhalte zuverlässig nachziehen; gelber Streifen innerhalb des lokalen Segments und ohne redundante Quote.
- [ ] Redundante Statistik- und Wartungsboxen entfernen.

## Lesen, Schreiben und Anhänge

- [ ] Leseformat und Farben, Standard-Absenderadresse, Schreibformat und große formatierbare Signatur zusammenführen.
- [ ] Signatur sicher speichern und beim Verfassen/Antworten als formatierten Inhalt verwenden.
- [ ] Anhang-Standardaktion, Downloads-Ordner und Speicherziele übersichtlich darstellen.

## Kategorisierung

- [ ] Allgemeine Regeln vor Modell, erste passende Regel gewinnt; Regeln in Reihenfolge und direkt in ihrer eigenen Karte bearbeiten. Inline-Bearbeitung ist begonnen, visuelle Abnahme steht aus.
- [x] Regel-Prüffelder und passende Vergleiche gemäß Mockup, Ziel-Unterkategorie, Ausnahmen und Datenverfügbarkeit korrekt behandeln.
- [ ] Geeignete, wirtschaftliche Kategorisierungsmodelle nach Anbieter anbieten; beim empfohlenen Wizard-Weg aktiviert und vorbelegt, separat abschaltbar.
- [ ] Bestehende E-Mails mit klarer Option „Alle E-Mails“ erneut einsortieren. Die Beschriftung ist begonnen; Laufzeitverhalten noch prüfen.
- [x] Bearbeitbare Hauptkategorien mit stabilen IDs, Name, Icon und Farbe; Unterkategorien mit Name, Icon, Beschreibung und Zuordnung. Bestandsdaten migrieren. Neue Gruppen bleiben beim Umbenennen und nach Neustart stabil; Löschen von Hauptkategorien ist nicht Teil des Mockups.
- [ ] Sichtbares Icon-Raster und Stift-Button, offene Unterkategorie-Karten mit direkt bearbeitbarer Beschreibung.

## Abnahme

- [ ] Reguläre und schmale Fensterbreiten in der nativen App prüfen, Tastaturbedienung und klare Beschriftungen.
- [ ] Gezielte Daten-/UI-Tests und Repository-Prüfungen ausführen.
- [ ] Jede Anforderung gegen aktuelle Implementierung und Laufzeitverhalten nachweisen.
