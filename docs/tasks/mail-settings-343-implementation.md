# Mail-Einstellungen: Umsetzung von Issue #343

Quelle: https://github.com/mahype/TorroWorkplace/issues/343 und `docs/prototypes/mail-settings-refresh/`.

Diese Liste bildet die Produktanforderungen ab. Ein Punkt gilt erst als fertig, wenn native Bedienung, Speicherung und ein passender Laufzeit- oder Testnachweis vorliegen. Die HTML-Mockups selbst sind keine Implementierung.

## Zwischenstand im Worktree

- Kontenliste und Kontodetail verwenden jetzt vergleichbare Mailgrößen (RFC822.SIZE), einen schraffierten Soll-Anteil und den angrenzenden Indexstreifen. Fehlende Größen verhindern eine irreführende Verhältnisgrafik. Die tatsächliche SQLite-Größe steht gesondert.
- Die allgemeine Speicher-Seite ist aus der Navigation entfernt; Offline-Auswahl und Anhang-Aufbewahrung stehen im Konto-Reiter „Speicher & Suche“. Die Mail-Seiten dürfen auf breiten Fenstern bis 1200 Pixel nutzen.
- Datum und maximale Größe sind als gespeicherte Konto-Strategien ergänzt. Die Synchronisierung und der Cache-Job verwenden denselben Auswahlbereich; eine bestehende 90-Tage-Strategie erscheint als entsprechendes Datum. Der Maximalwert verwendet bislang die gemeldete RFC822.SIZE-Summe und ist noch keine harte Grenze für Anhang-Cache, Index und Datenbankdatei.
- Die Kategorisierung hat die Reihenfolge Automatische Einsortierung → Kategorien; Regel-Editoren stehen innerhalb ihrer Regelkarte.
- Neue Regeln werden an Position 0 gespeichert. Die Modellauswahl zeigt nur aktivierte Entscheidungsmodelle und gruppiert mehrere Anbieter. Kategorie-Icons erscheinen als sichtbares Raster.
- „Lesen & Schreiben“ enthält die Standard-Absenderadresse, das Schreibformat und eine hohe Signaturfläche mit Formatierungsaktionen. Formatierte Signaturen werden sicher in HTML und als Textkopie versendet. Der Konto-Reiter zeigt Identität und Verbindung, ohne die Signatur zu duplizieren.
- Die frühere Anhang-Vorabladeoption ist auch für alte gespeicherte Policies deaktiviert. Alte „Nur Kopfzeilen“-Konten werden ausdrücklich zur Auswahl einer der drei neuen Strategien aufgefordert.
- Das Backup-Formular öffnet als Overlay; Quelle, Stichtag mit Kalenderwahl, Archivname und Ziel stehen zusammen. Fortschritt und Wiederherstellung bleiben im Konto-Reiter sichtbar.
- Die MB-Auswahl berechnet ihre Größenkante beim Synchronisieren nur einmal pro Stapel und zieht bereits gespeicherte Anhänge von der Grenze ab. Beim Ändern der Auswahl und im Cache-Job werden ältere Anhangdateien nötigenfalls entfernt und die Offline-Texte neu zugeschnitten. Die Auswahl bleibt dennoch eine RFC822-Größenschätzung; Index und Datenbankdatei sind noch keine hart begrenzten physischen Bytes.
- Build und gezielte Tests laufen; native Sichtprüfung und die übrigen Anforderungen stehen noch aus.

## Navigation und Oberfläche

- [ ] Kontenübersicht C mit einer Größen-Spalte, Ist/Soll/Online-Balken, dichtem Indexstreifen und gemeinsamer Legende.
- [ ] Kontodetail mit Einstellungen und Speicher & Suche; alle Kontenfelder und Serverangaben in den vorgesehenen Spalten.
- [ ] Allgemein mit Lesen & Schreiben sowie Anhängen; allgemeine Speicher-&-Index-Seite entfällt.
- [ ] Kategorisierung mit Automatische Einsortierung vor Kategorien, breiten Karten und schmalem responsivem Layout. Zwei Tabs und die Reihenfolge Regeln → Modell sind begonnen; visuelle Abnahme steht aus.

## Konten und Verbindung

- [ ] Anzeigename, Adresse, Benutzername und Passwort zusammen; Zugangsdaten testen.
- [ ] IMAP/SMTP Host, Port und TLS/STARTTLS getrennt bearbeiten, automatische Erkennung und manuelle Angaben.
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
- [ ] Regel-Prüffelder und passende Vergleiche gemäß `regeln-und-kategorien.md`, Ziel-Unterkategorie, Ausnahmen und Datenverfügbarkeit korrekt behandeln.
- [ ] Geeignete, wirtschaftliche Kategorisierungsmodelle nach Anbieter anbieten; beim empfohlenen Wizard-Weg aktiviert und vorbelegt, separat abschaltbar.
- [ ] Bestehende E-Mails mit klarer Option „Alle E-Mails“ erneut einsortieren. Die Beschriftung ist begonnen; Laufzeitverhalten noch prüfen.
- [ ] Bearbeitbare Hauptkategorien mit stabilen IDs, Name, Icon und Farbe; Unterkategorien mit Name, Icon, Beschreibung und Zuordnung. Bestandsdaten migrieren.
- [ ] Sichtbares Icon-Raster und Stift-Button, offene Unterkategorie-Karten mit direkt bearbeitbarer Beschreibung.

## Abnahme

- [ ] Reguläre und schmale Fensterbreiten in der nativen App prüfen, Tastaturbedienung und klare Beschriftungen.
- [ ] Gezielte Daten-/UI-Tests und Repository-Prüfungen ausführen.
- [ ] Jede Anforderung gegen aktuelle Implementierung und Laufzeitverhalten nachweisen.
