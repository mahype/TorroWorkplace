# Aufgaben: Send to

Konzept: [send-to.md](../send-to.md). **Planungsstand 2026-10-20; keine Aufgabe begonnen.**
Die Reihenfolge vermeidet, dass Apps verschiedene Übergabewege erfinden. Jede Aufgabe mit
Request-Änderung umfasst Katalog, API-Referenz, MCP und Rechteprüfung; jede Oberfläche nutzt
dieselben Requests.

## ST1 Vertrag und Rechte

- [ ] `SendContent`, `SendTarget`, `SendOptions`, Vorschau und Fehler in `torro-ipc` definieren;
  `send.list_targets`, `send.preview`, `send.route` samt Katalog und MCP aufnehmen. Der Request
  hat genau ein Ziel, eine Aktion, mehrere Inhalte und eine erforderliche `idempotency_key`.
- [ ] Zielregistrierung im Kern mit Fähigkeiten und opaken Objektkennungen; Quelladapter für `text`,
  `mail_message`, `mail_part` und bereits verwaltete Dateien. Quellrechte, Zielrechte,
  Sammlungsgrenzen, `not-found`, Freigaben, Revisionsprüfung und Audit zentral prüfen.
- [ ] Quelladapter, Formatplaner und Zieladapter getrennt halten. `send.preview` meldet Format,
  Verluste, Speicherkonto und Größen ohne Binärdaten; `send.route` materialisiert erst alle
  Quellen und ändert dann genau ein Ziel. Antwort nennt Ziel, Sync-Status und auch bei einem
  Fehler bereits abgeschlossene Schritte.
- [ ] [teilen.md](../teilen.md), [api.md](../api.md), Rechtevorlagen und API-Referenz auf die
  Entscheidung zu `send` abstimmen; geplante `ShareItem`/`ShareCard` als Quelladapter weiterführen,
  keine zweite Share-Engine bauen.
- **Abnahme:** Vertragstest Katalog ↔ MCP; API-Tests für fehlende Quellrechte, fehlende
  Zielrechte, fremde Mailkonten, veränderte Vorschau, verborgene Ziele und eine wiederholte
  `idempotency_key`, die keine zweite Änderung erzeugt.

## ST2 Assistent und Bots als Ziele

- [ ] `assistant.send_message` um strukturierte Kontext-Schnappschüsse erweitern; im Verlauf
  Herkunft, Revision, Hash und gewählte Inhalte speichern. Vor einer erneuten Modellübergabe
  Rechte und Datenregel prüfen; Gesprächslöschung entfernt die Schnappschüsse. Gleiches Modell
  für Bot-Chats.
- [ ] `assistant.ask` und `bot.ask` als Zieladapter; `conversation` und `bot` explizit zuordnen.
  `without`, Bot-Rechteschnittmenge, Datenregel und Schleifenschutz gelten auch für `send.route`;
  Assistent/Bot können sich darüber nicht selbst oder gegenseitig beauftragen.
- [ ] Zuerst Text und Mailtext; nicht lesbare Dateiformate mit `unsupported` beantworten und in
  der Vorschau zeigen. Kein stilles Weglassen von Anhängen.
- **Abnahme:** App- und Client-Schlüssel können erlaubten Text und Mail an Assistent/Bot geben;
  Cloud-Regel, Mail ohne Leserecht des Assistenten/Bots, gelöschte Quelle und nicht unterstützter
  Anhang sind geprüft.

## ST3 Mailentwurf als Ziel

- [ ] `mail.compose` übernimmt Text, Mailanhänge und verwaltete Dateien über den bestehenden
  Entwurfsweg. Original-Mail und Teil werden getrennt behandelt; Anhänge sind explizit wählbar.
  `send.route` liefert die Entwurfskennung, die Oberfläche öffnet ihn separat.
- [ ] Bestehende `DraftAttachment`-Varianten wiederverwenden; einen gemeinsamen internen
  Materialisierer für späteres `ShareItem`/`.ics`/vCard vorsehen.
- **Abnahme:** Mailanhang landet unverändert im Entwurf; falsches Konto, fremdes Handle und
  zu große Datei scheitern; `mail.send_draft` behält die bisherige Freigabe.

## ST4 Dateien von außen

- [ ] Begrenztes Staging per `send.stage_begin/chunk/finish` über IPC/API/MCP; Handle an Client
  und Integritätswert binden, Ablauf und Aufräumen. Chunks bleiben deutlich unter der 1-MiB-
  Grenze einer IPC-Anfrage. CLI-Komfortbefehl liest eine Datei und lädt sie in Stücken hoch.
- [ ] Dateityp anhand von Inhalt und Metadaten prüfen; Name nur als Anzeige behandeln, keine
  symbolischen Links oder beliebigen Client-Pfade öffnen. Größen je Quelle und Ziel begrenzen.
- **Abnahme:** Test für fremdes Handle, unterbrochenen Upload, Größenlimit, manipulierte Datei,
  Wiederverwendung und Ablauf; ein externes Script kann eine Datei in einen Entwurf übergeben.

## ST5 Aufgaben und Termine als Ziele

### ST5.1 Lesen und Fähigkeiten

- [ ] `Todo` und `Event` um lesbare Anhangsmetadaten ergänzen. Vorhandene CalDAV-`ATTACH`-
  Eigenschaften lesen und bei anderen Änderungen erhalten. Provider melden je Sammlung:
  verwalteter Anhang, geprüfter Datei-Link oder keine Dateiaktion.
- [ ] Objektkennungen aus Konto, Sammlung und UID nur im Kern auflösen; Schreibaktionen prüfen
  die Revision (`etag`). Fremde `ATTACH`-URIs nur als Metadaten zeigen, bis ein sicherer Abrufweg
  existiert.
- **Abnahme:** Bereits vorhandene Anhänge sind sichtbar und überstehen das Ändern anderer
  Aufgaben- oder Terminfelder; fremde oder nicht erreichbare Sammlungen bleiben verborgen.

### ST5.2 Beziehungen und Text

- [ ] `task.link`, `event.link` und `append_text` als Zielaktionen, dazu gezielte Befehle zum
  Entfernen einer Beziehung. Mehrere `related_items` separat speichern, ohne das bestehende
  einzelne `Todo.link` zu überschreiben; lokale und synchronisierte Beziehungen unterscheiden.
- [ ] Beschreibung nur mit Revisionsprüfung ergänzen, niemals eine veraltete Fassung blind
  ersetzen. Für neue Objekte `task.create` und `event.create` mit Inhalt planen und in der
  Vorschau zeigen, welche Felder übernommen werden.
- **Abnahme:** Mail oder Aufgabe kann mit einem vorhandenen Task/Termin verknüpft werden; zwei
  Beziehungen bleiben unabhängig, und ein alter `etag` ergibt `conflict`.

### ST5.3 Dauerhafte Dateien

- [ ] `task.attach` und `event.attach` als Zielaktionen; `tasks.get_attachment`,
  `calendar.get_attachment`, `tasks.remove_attachment` und `calendar.remove_attachment` als
  API/MCP-Befehle für den Lebenszyklus. Oberflächen zeigen und öffnen dieselben Anhänge.
- [ ] Für unterstützende CalDAV-Server verwaltete Anhänge nach RFC 8607 prüfen und anbinden;
  Nextcloud-WebDAV mit `ATTACH`-URI nur nach Interoperabilitäts- und Rechteprüfung anbieten.
  Kurzlebige Mail- und Staging-Dateien vor dem Verknüpfen dauerhaft speichern, Offline- und
  Fehlerfälle mit Aufräumen behandeln. Zuerst Pilot: ein Mailanhang an eine bestehende
  CalDAV-Aufgabe; danach eine externe Datei und ein Termin.
- **Abnahme:** Datei und Mailanhang bleiben nach Cache-Ablauf, Neustart und Sync am Ziel
  erreichbar. Nicht unterstützende Provider melden `unsupported` oder bieten einen klar
  benannten Link. Fremde Liste/Kalender, stale `etag`, Serientermin und Uploadfehler erzeugen
  weder doppelten Anhang noch eine unbemerkte Teiländerung.

### ST5.4 Neue Objekte mit Inhalt

- [ ] `task.create` und `event.create` als Zielaktionen ausführen: Ziel-Sammlung und benötigte
  Grundfelder ausdrücklich angeben, dann gewählten Text, Beziehungen und unterstützte
  Anhänge hinzufügen. Eine nicht fertiggestellte Anlage wird als solche gemeldet und kann
  gezielt fortgesetzt oder aufgeräumt werden.
- **Abnahme:** „Mail in neue Aufgabe“ und „Datei in neuen Termin“ ergeben je ein Objekt mit
  genau den gewählten Inhalten; derselbe Versuch erstellt nicht unbemerkt ein zweites Objekt.

## ST6 Gemeinsame Oberfläche und Ausbau

- [ ] `torro-gui`-Blatt „Send to…“ mit Zielsuche, Objektwahl, Aktion „anhängen“/„verknüpfen“/
  „als Text einfügen“, Feld-/Anhangsauswahl und verlustgenauer Vorschau. Inbox, Kalender,
  Aufgaben und Assistent öffnen dasselbe Blatt; Aufgaben- und Termindetails zeigen die Anhänge.
- [ ] Quellenadapter für Kontakte, Termine, Aufgaben und Notizen aus dem Teilen-Konzept ergänzen;
  später Talk und weitere Ziele nur durch registrierte Adapter hinzufügen.
- [ ] Für PDF, Bilder, HTML und `.eml` einzeln festlegen: nativ, sicher extrahiert oder
  `unsupported`; je Format Tests für Inhalt, Größen und Cloud-Datenregel.
- **Abnahme:** Derselbe Inhalt ergibt über Oberfläche, API und MCP dieselbe Zielwirkung;
  unbekannte Kombinationen sind sichtbar und werden nicht still verändert.
