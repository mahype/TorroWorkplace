# Send to: Inhalte zwischen Komponenten übergeben

Stand: 2026-10-20. **Vorschlag zur Abstimmung, noch keine Implementierung.** Umsetzungsfolge:
[tasks/send-to.md](tasks/send-to.md). Das bestehende [Teilen-Konzept](teilen.md) ist der Ausgangspunkt;
dieser Plan erweitert es um freie Texte, Mail, Anhänge, Assistenten, Bots und externe Clients.

## Ziel und Abgrenzung

Jede App zeigt dieselbe Aktion **„Send to…“**. Sie übergibt einen oder mehrere Inhalte an ein
Ziel der Suite. Derselbe Weg ist über die lokale API, `ctl` und MCP erreichbar. Beispiele:

- markierten Text an den Assistenten oder einen Bot geben;
- eine Mail mit oder ohne Anhänge als Kontext an einen Bot geben;
- einen Mailanhang an einen neuen Mailentwurf hängen;
- eine Datei oder einen Mailanhang an eine bestehende Aufgabe oder einen Termin hängen;
- einen anderen Suite-Eintrag mit einer Aufgabe oder einem Termin verknüpfen;
- eine Datei an einen Entwurf hängen oder später in Talk senden.

„Send to“ ist **Routing**, keine allgemeine Aufforderung zum sofortigen Versand. Das Ziel legt die
Wirkung fest: `mail.compose` erstellt einen Entwurf, `assistant.ask` startet eine Antwort,
`task.attach` ergänzt eine Aufgabe, `event.link` verknüpft einen Termin und `talk.send` sendet nach
außen. „Senden“ nach außen bleibt eine ausdrücklich benannte Zielaktion mit den bestehenden
Freigaben. Eine Übergabe an einen Cloud-Assistenten kann ebenfalls Daten nach außen geben und
wird vor dem Start anhand der Datenregel angezeigt und geprüft.

## Vorhandene Grundlage

- Alle Apps, externe Clients und MCP nutzen `torro_ipc::Request` über den Kern, mit Katalog,
  Schlüsseln, Rechten und Freigaben ([api.md](api.md)). Es entsteht kein zweiter Transport.
- [teilen.md](teilen.md) plant `ShareItem` und `ShareCard`, aber die TL-Aufgaben sind noch offen.
  Deren Quellreferenzen und sparsame Feldauswahl bleiben sinnvoll.
- `assistant.send_message` nimmt bisher nur `text` an; Bots werden über dasselbe Request-Feld `bot`
  oder `bots.ask_bot` angesprochen. Die Übergabe von Inhalt braucht daher eine Erweiterung des
  Assistenten-Verlaufs und seiner Modellaufbereitung.
- Mailentwürfe kennen `DraftAttachment::MailPart` und `DraftAttachment::File`. Die lokale IPC-
  Anfrage ist auf 1 MiB begrenzt; Binärdaten gehören nicht als große Base64-Felder in `send.route`.
- `Todo` und `Event` haben derzeit **keine Anhangsliste**. Aufgaben besitzen ein einzelnes `url`
  und einen `link` zu einem anderen Eintrag. Die CalDAV-Schreibschicht erhält fremde `ATTACH`-
  Eigenschaften, kann neue Anhänge aber nicht als eigene Funktion anlegen oder anzeigen.

### Zusammenführung mit „Teilen“

Das geplante `ShareItem` wird zur referenzierten Form von `SendContent::Item` mit typabhängiger
Feldauswahl; `ShareCard` bleibt die vom Kern erzeugte Darstellung für die Vorschau. Für den
eigentlichen Dateiinhalt liefert der Kern ein verwaltetes Handle statt Base64 im JSON: Die in
[teilen.md](teilen.md) vorgesehenen 5 MiB Base64 wären für den JSON-Zeilentransport unhandlich
und überschritten als Eingabe die heutige 1-MiB-Grenze. `contacts.share_card` kann eine kleine
Text-/Metadatenvorschau liefern; „Copy“,
„Save as file“ und QR bleiben Oberflächenaktionen auf dieser Quelle. Das gemeinsame
`torro-gui`-Blatt ersetzt ein zweites, getrenntes Teilen-Blatt. Quellmaterialisierung für
`mail.save_draft` und `send.route` nutzt denselben Kernbaustein.

## Gemeinsamer Vertrag

Der Kern bekommt einen **Umschlag**, nicht eine fertige Zielnachricht. Quellen und Ziele melden
ihre Fähigkeiten an. Die Oberfläche fragt kompatible Ziele ab und zeigt vor der Übergabe die
konkrete Aufbereitung.

```json
{
  "method": "send.route",
  "target": { "kind": "assistant", "id": "default", "action": "ask" },
  "destination": { "conversation": null },
  "content": [
    { "kind": "text", "text": "Bitte fasse das zusammen." },
    { "kind": "mail_message", "id": "m:...", "include": ["body"] }
  ],
  "options": { "format": "auto" },
  "preview_id": "optional",
  "idempotency_key": "<einmalige Kennung dieses Versuchs>"
}
```

Eine Datei an eine bestehende Aufgabe hängen benutzt denselben Befehl:

```json
{
  "method": "send.route",
  "target": { "kind": "task", "id": "<opake Aufgabenkennung>", "action": "attach" },
  "content": [{ "kind": "file", "id": "<verwaltetes Datei-Handle>" }],
  "idempotency_key": "<einmalige Kennung dieses Versuchs>"
}
```

Ein Request hat **genau ein Ziel und eine Aktion**, kann aber mehrere Inhalte enthalten. Der
Kern liefert als Ergebnis die Kennung des neuen/änderten Zielobjekts, die ausgeführten Schritte
und gegebenenfalls eine Freigabe oder einen konkreten Fehler. Die für `send.route`
**erforderliche** `idempotency_key` bindet einen Wiederholungsversuch an denselben Aufrufer und
dieselbe Eingabe; derselbe Schlüssel mit anderer Eingabe ergibt `conflict`. Der Kern bewahrt das
Ergebnis für einen begrenzten Zeitraum auf und führt eine bereits bestätigte Änderung nicht
zweimal aus. Bei unbekanntem Status eines entfernten Versands wird nicht automatisch erneut
gesendet. Diese Kennung ist gerade für Skripte und große Uploads wichtig.

Die Implementierung hat drei Teile im Kern: Ein **Quelladapter** löst eine Referenz mit den
Rechten des Aufrufers auf; ein **Formatplaner** wählt eine für das Ziel erlaubte Darstellung und
meldet Verluste; ein **Zieladapter** führt die Änderung aus. Neue Apps registrieren Quell- und
Zieladapter im Kern, statt selbst Dateien zwischen Prozessen zu kopieren. `torro-ipc` enthält
nur die versionierten, providerunabhängigen Request- und Antworttypen. Unbekannte Inhaltsarten
und Zielaktionen sind ein Fehler, keine Aufforderung zum Raten.

`content` ist eine geordnete Liste. Die Grundformen sind:

| Form | Bedeutung | Auflösung |
|---|---|---|
| `text` | Vom aufrufenden Client selbst verfasster oder markierter Text | als fremde Eingabe markiert; kein behaupteter Suite-Ursprung |
| `item` | Bestehender Suite-Eintrag, etwa Kontakt, Termin, Aufgabe oder Notiz | Kern lädt ihn mit den Leserechten des Aufrufers und der gewählten Feldauswahl |
| `mail_message` | Eine vorhandene Mail | Kern liest Kopf/Text und nur ausdrücklich gewählte Anhänge |
| `mail_part` | Ein Anhang einer vorhandenen Mail (`message`, `part`) | Kern holt genau diesen MIME-Teil; Dateiname ist nur Anzeige, kein Pfad |
| `file` | Eine verwaltete Datei mit opakem Handle | Kern prüft Eigentümer, Gültigkeit, Größe und unveränderten Inhalt |

Ein externer Client kann eigene Dateien über einen **Staging-Weg** anmelden: `send.stage_begin`,
`send.stage_chunk`, `send.stage_finish` liefern nach begrenztem, stückweisem Upload ein Handle.
Staging ist pro Schlüssel getrennt, zeitlich befristet und auf Dateigröße/Gesamtvolumen begrenzt;
abgebrochene Uploads werden entfernt. Direkte lokale Pfade sind für Client-Schlüssel keine
Dateireferenz. Die eigenen Apps dürfen einen lokalen Pfad nur über einen eigenen, vom Kern
geprüften Importweg übergeben. URLs werden nicht automatisch geladen.

Ziele besitzen stabile Kennungen und Aktionen, z. B. `assistant/default/ask`, `bot/<id>/ask`,
`mail/<konto>/compose` und später `talk/<unterhaltung>/send`. Ein Ziel meldet akzeptierte
Inhaltsformen, Größen, Formate und mögliche Wirkungen. `send.list_targets { content }` liefert
**Zielarten und Aktionen**, nicht alle Aufgaben und Termine des Menschen. Das konkrete Objekt
wählt die Oberfläche über die bestehenden Such- und Listenbefehle des Zielbereichs; externe
Clients benutzen deren Ergebniskennungen. Verborgene Objekte und Konten werden nicht aufgelistet.
`send.preview` bestätigt dann die Verfügbarkeit und Verträglichkeit genau dieses Objekts.

Ein Ziel kann auch ein **bestehendes Objekt** sein. Für „Datei an Aufgabe hängen“ ist zum Beispiel
die Zieladresse `{ "kind": "task", "id": "<opake Aufgabenkennung>", "action": "attach" }` und der Inhalt
ein `file`- oder `mail_part`-Handle. `task.create` legt dagegen eine neue Aufgabe mit gewähltem
Inhalt an. Weitere Aktionen sind `link` (eine Referenz auf einen anderen Eintrag), `append_text`
(Text in Beschreibung oder Kommentar) und, nur wo vorhanden, `send`. Der Adapter des konkreten
Objekttyps und Providers bestimmt, welche Aktionen möglich sind. Ein Dateianhang ist nicht
gleichbedeutend mit einem Link auf eine Datei oder mit Text in der Beschreibung.

Die Objektkennung wird ausschließlich vom Kern ausgegeben und steht für Art, Konto, Sammlung und
UID; Clients bauen sie weder aus Dateipfaden noch aus CalDAV-URLs. Eine verschobene Aufgabe kann
so auf dieselbe interne Identität abgebildet werden. Zu einer konkreten Objektversion gehört eine
Revision (`etag` oder entsprechende Provider-Version). `send.route` schreibt bei einer seit der
Vorschau geänderten Version nicht blind darüber, sondern antwortet mit `conflict`.

Mehrere solche Beziehungen brauchen eine eigene Liste `related_items`. Das vorhandene einzelne
`Todo.link` repräsentiert bisher eine besondere Aufgabe, die für eine Mail oder ein GitHub-Issue
steht; `send.route` darf es nicht überschreiben. Für jede Beziehung wird festgehalten, ob sie
auf anderen Geräten verfügbar ist oder nur im lokalen Torro-Kern besteht. Auch ein Termin kann
so auf eine Mail, Aufgabe oder Notiz verweisen, ohne deren Inhalt als Datei zu duplizieren.

## Dauerhafte Anhänge an Aufgaben und Termine

Dateien an Aufgaben und Termine brauchen einen **dauerhaften Speicher**. Das kurzlebige Staging-
Handle und der Mail-Anhangcache dürfen nach `send.route` verschwinden, ohne den Anhang am Ziel zu
brechen. Der Kern überträgt die Bytes deshalb einmal in den Speicher des Ziel-Providers und
speichert am Objekt eine Anhangsreferenz mit Name, Typ, Größe und stabiler Kennung. Löschen einer
Verknüpfung und Löschen der Datei sind getrennte Vorgänge; verwaiste Uploads nach einem Fehler
werden bereinigt. Offline wird die Änderung vorgemerkt und ihr Sync-Status angezeigt.

Bei CalDAV gibt es `ATTACH` für `VEVENT` und `VTODO` als URI oder kodierte Bytes
([RFC 5545](https://www.rfc-editor.org/rfc/rfc5545.html)). Für Server mit der Erweiterung für
verwaltete Anhänge nutzt der Provider deren Upload und Änderungsablauf
([RFC 8607](https://www.rfc-editor.org/rfc/rfc8607.html)). Bei einem Nextcloud-Konto kann ein
WebDAV-Upload plus `ATTACH`-URI ein weiterer Adapter sein, **wenn** Rechte, Abrufbarkeit auf
anderen Geräten und Roundtrip mit dem Server geprüft sind. Ein beliebiger CalDAV-Server muss
keinen dieser Wege unterstützen. Dann bietet die Oberfläche nur einen funktionierenden Link an
oder meldet `unsupported`; eine bloß lokale Datei wird nicht als synchronisierter Anhang
ausgegeben.

`Todo` und `Event` bekommen dafür eine Anhangsübersicht, die der Provider beim Lesen befüllt;
Anlegen und Entfernen laufen über gezielte Befehle, nicht über ein vollständiges Ersetzen des
Objekts. Ein bestehender `ATTACH`-Eintrag bleibt beim Speichern anderer Felder erhalten. Bei
Serienterminen muss die UI zwischen ganzer Serie und einzelner Instanz unterscheiden; solange
die Bearbeitung von Serien nicht unterstützt ist, wird `event.attach` dafür nicht angeboten.
Ein Dateianhang an einem Termin verschickt nicht automatisch eine Einladung oder die Datei an
Teilnehmer. Die Vorschau nennt, wo die Datei gespeichert wird und wer dort Zugriff hat.

Die Objektansicht braucht den ganzen Lebenszyklus: Anhangsmetadaten lesen, einen Anhang in den
verwalteten Cache holen/öffnen und ihn gezielt entfernen. Dafür werden `tasks.get_attachment`,
`calendar.get_attachment`, `tasks.remove_attachment` und `calendar.remove_attachment`
geplant; Listen können in `tasks.list_todos`/`calendar.list_events` stehen, sofern dadurch keine
zu großen Antworten entstehen. Entfernen einer Beziehung bekommt entsprechend
`tasks.remove_related_item` bzw. `calendar.remove_related_item`. Diese Zielbefehle prüfen
dieselben Objekt- und Sammlungsrechte wie `send.route`. Fremde `ATTACH`-URIs werden zunächst als
Metadaten angezeigt; Abrufen darf nur über einen gegen fremde Hosts abgesicherten Providerweg
geschehen, nicht als beliebiger URL-Download durch den Kern.

| Ziel | Erste sinnvolle Aktionen | Wenn der Provider keine echte Datei unterstützt |
|---|---|---|
| Aufgabe | Datei anhängen, Eintrag verknüpfen, Text ergänzen, neue Aufgabe anlegen | Link nur bei dauerhaft erreichbarer URL; sonst `unsupported` |
| Termin | Datei anhängen, Eintrag verknüpfen, Beschreibung ergänzen | wie bei Aufgaben; Serientermine vorerst nur lesen |
| Mailentwurf | Text/Datei übernehmen und Entwurf öffnen | keine stille Umwandlung in einen Link |
| Assistent/Bot | Inhalt als gekennzeichneten Kontext übergeben und fragen | unbekanntes Dateiformat ablehnen |
| Talk (später) | Nachricht und unterstützte Datei versenden | `unsupported`, wenn Upload oder Versand nicht möglich ist |

### Ablauf einer Übergabe

1. Die Quell-App liefert Kennungen und Auswahl, eine externe Anwendung ihren Client-Schlüssel
   und gegebenenfalls ein Datei-Handle aus Staging.
2. `send.list_targets` nennt passende Zielarten. Die konkrete Aufgabe oder den konkreten Termin
   sucht die Oberfläche über `tasks` bzw. `calendar`.
3. `send.preview` prüft Quell- und Zielrechte, Revisionsstand, Größe, Format und Datenregel;
   die Vorschau nennt ausdrücklich „Datei“, „Link“ oder „Text“ und das Speicherkonto.
4. `send.route` prüft erneut, erstellt den nötigen Schnappschuss bzw. dauerhaften Upload und
   führt den Zielbefehl aus. Das Ergebnis enthält die Zielkennung und den Sync-Status.
5. Die aufrufende Oberfläche aktualisiert die Zielansicht oder öffnet das Objekt über den
   vorhandenen `apps.open_app`- bzw. `assistant.open_window`-Befehl. `send.route` selbst öffnet
   kein Fenster; ein externer Client braucht dafür das gesonderte App-Recht.

## Formatbehandlung

Die Quellen liefern **semantische Inhalte und Originaldateien**, die Ziele wählen ihre Darstellung:

1. **Nativ, wenn möglich:** Ein Mailanhang bleibt als MIME-Teil/Datei erhalten; ein Termin als
   Kalenderreferenz oder `.ics`; ein Kontakt als sparsame vCard. Kein Umweg über kopierten Text.
2. **Lesbare Darstellung:** Für Assistent und Bot erzeugt der Kern einen gekennzeichneten
   Kontextblock mit Quelle, Titel und Text. Anhänge werden nur durch einen ausdrücklich
   vorgesehenen Extraktor oder ein vom gewählten Modell unterstütztes Dateiformat eingebunden.
3. **Ersatzformat:** Ist nur eine Datei oder nur Text möglich, zeigt die Vorschau genau diese
   Umwandlung. Verlust (zum Beispiel keine Bilder, kein HTML, ausgelassene Anhänge) ist sichtbar.
4. **Keine stille Umwandlung:** Kann das Ziel einen Inhalt nicht verarbeiten, wird es nicht
   angeboten oder `send.route` antwortet mit `unsupported`; es fällt nichts unbemerkt weg.

| Quelle | Mögliche Darstellung | Passende erste Aktionen |
|---|---|---|
| Freitext | Text mit Kennzeichnung „vom Absender“ | Assistent fragen, Mailentwurf füllen, Beschreibung ergänzen |
| Mail | gewählter Kopf/Text, Referenz auf die Mail | Assistent/Bot fragen, mit Aufgabe/Termin verknüpfen |
| Mailanhang oder Datei | Originalbytes plus Name/Typ/Größe | an Entwurf, Aufgabe oder Termin anhängen |
| Termin | strukturierte Daten; bei Mail als `.ics` | Assistent fragen, per Mail teilen, mit Aufgabe verknüpfen |
| Aufgabe | strukturierte Daten oder Referenz | Assistent fragen, mit Termin/anderer Aufgabe verknüpfen |
| Kontakt (später) | sparsame Textkarte und vCard nach Feldauswahl | Mailentwurf, Assistent, später Talk |

Eine Mail mit Anhängen ist daher keine einzige undurchsichtige Datei: Der Mensch wählt Text,
Referenz und jeden Anhang einzeln. Eine Datei wird für eine Aufgabe nicht ungefragt in deren
Beschreibung oder in einen Link umgewandelt. Das Ziel kann mehrere **ausdrücklich angebotene**
Aktionen haben, die in der Vorschau getrennt erscheinen.

`send.preview` liefert je Inhalt: Quellart, gewählte Felder, Zielaktion, verwendetes Format,
Größe, ausgelassene Teile, Warnungen und ob eine Freigabe ansteht. Es liefert eine kurze
`preview_id` mit Ablaufzeit und Fingerabdruck. Die Antwort enthält nur eine begrenzte Textprobe
und Metadaten, niemals große Binärdaten. `send.route` prüft Rechte, Datenregel und Quellzustand
erneut; bei Änderung seit der Vorschau antwortet es mit `conflict` und verlangt eine neue
Vorschau. API/MCP dürfen ohne Vorschau aufrufen; der Kern führt dieselben Prüfungen aus.

Für den Assistenten wird aus den gewählten Inhalten **bei der Übergabe ein unveränderlicher
Kontext-Schnappschuss** erstellt, strukturiert und mit Herkunft, Revision und Hash im Verlauf
gespeichert. So sieht das Modell genau die geprüfte Fassung, auch wenn die Ursprungsmail später
wechselt oder verschwindet. Vor jeder erneuten Übergabe dieses Kontexts an ein Modell prüft der
Kern die dann geltenden Rechte, `without`, Bot-Rechte und Datenregel; gesperrter Kontext wird dem
Modell nicht erneut gegeben und im Verlauf als nicht mehr verfügbar markiert. Die Aufbewahrung
folgt der Unterhaltung; deren Löschung entfernt auch ihre Schnappschüsse. Fremde Mailtexte,
Dateien und frei gelieferter Text bleiben `untrusted`.
Referenzierte Inhalte müssen auch in der Reichweite des Assistenten bzw. Bots liegen; ein
ausdrücklich gelieferter Freitext ist hingegen die Nachricht des Absenders. Ein Bot erhält für
weitere Werkzeugaufrufe nur die Schnittmenge aus Aufruferrechten und seinen eigenen Rechten; der
Assistent kann aus einer Übergabe heraus nicht seine eigenen `assistant.*`-Befehle aufrufen.

## Rechte und Freigaben

`send.*` ist ein **gemeinsamer Orchestrierungsbereich** im Befehlskatalog. Ein `send`-Recht allein
berechtigt weder zum Lesen einer Quelle noch zur Nutzung eines Ziels. Für externe Clients wird
der Bereich bewusst freigeschaltet; die eigenen Apps verwenden den App-Schlüssel. Zusätzlich
prüft der Kern vor Vorschau und Ausführung:

1. Leserecht samt Konto/Sammlung für jede Quellreferenz beim Aufrufer und bei Assistent/Bot,
   wenn sie das Ziel sind; unbekannt und außer Reichweite ergeben beide `not-found`.
2. Schreibrecht und Reichweite des Ziels (`assistant`, `bots`, `mail`, `tasks`, `calendar`, später
   `talk`), einschließlich der konkreten Liste oder des Kalenders. Eine Übergabe an einen Bot
   nutzt dessen eigene Rechte beim Modelllauf.
3. Zielabhängige Schranken: `mail.compose` speichert nur einen Entwurf; `mail.send` und `talk.send`
   brauchen Outbound-Recht und die bestehende Freigabe; Cloud-Modelle beachten die Datenregel.
4. Prüfung erneut nach einer Freigabe und unmittelbar vor der Ausführung. Protokolliert werden
   Quellarten, Zielart, Größen und Ergebnis, keine Inhalte oder Dateinamen.

`send.route` darf die bestehenden Schleifensperren nicht umgehen: Der Client des Assistenten
kann damit keine neue Anfrage an sich selbst oder einen Bot starten; Bot-Clients können weder
sich noch andere Bots fragen. Die ausdrücklich vom Menschen ausgelöste Delegation über
`bots.ask_bot` bleibt der einzige Assistent-zu-Bot-Weg.

Der Kern materialisiert alle Inhalte **vor** der Änderung eines Ziels und prüft Grenzen und
Kompatibilität als Ganzes. Danach führt ein Zieladapter die Änderung aus. Für einen einzelnen
Provider gilt dessen Konfliktschutz; bei einer Änderung über mehrere Dienste gibt es keine
vorgebliche globale Transaktion: Das Ergebnis nennt jeden abgeschlossenen Schritt. Ein Fehler
nach einem Upload löst Aufräumen oder einen sichtbar ausstehenden Reparaturvorgang aus.
Automatisches Wiederholen nach einem unklaren Versandstatus ist verboten; das lokale
Ausführungsjournal zur `idempotency_key` macht bestätigte Wiederholungen sicher und zeigt
unklare Fälle zur Klärung an.

Damit ändert sich eine frühere Entscheidung aus [teilen.md](teilen.md): Dort gibt es keinen eigenen
Bereich „share“. Die breitere, auch von externen Clients nutzbare Routing-Funktion bekommt hier
einen Bereich `send` als **zusätzliche Benutzungserlaubnis**, ohne Quell- oder Zielrechte zu
ersetzen. Vor Implementierung müssen die beiden Dokumente und die Rechtevorlagen angeglichen
werden. `send` ist für neue Client-Schlüssel standardmäßig aus.

## Drei Wege und erste Ziele

| Funktion | Oberfläche | API | MCP |
|---|---|---|---|
| Ziele finden | „Send to…“-Menü | `send.list_targets` · Read | `send_list_targets` |
| Darstellung prüfen | Vorschau im Blatt | `send.preview` · Read + Quellrechte | `send_preview` |
| Übergabe ausführen | „Send to…“ bestätigen | `send.route` · Write + Quell-/Zielrechte | `send_route` |
| Datei anmelden | Datei auswählen/ablegen | `send.stage_*` · Write | `send_stage_*` |
| Anhang ansehen/holen | Anhangsliste in Aufgabe/Termin | `tasks.get_attachment` bzw. `calendar.get_attachment` · Read | `tasks_get_attachment`, `calendar_get_attachment` |
| Anhang entfernen | „Remove attachment“ am Objekt | `tasks.remove_attachment` bzw. `calendar.remove_attachment` · Delete | `tasks_remove_attachment`, `calendar_remove_attachment` |
| Beziehung entfernen | „Remove link“ am Objekt | `tasks.remove_related_item` bzw. `calendar.remove_related_item` · Delete | `tasks_remove_related_item`, `calendar_remove_related_item` |

Erste Zieladapter: Assistent fragen, Bot fragen, Mailentwurf erstellen; anschließend vorhandene
Aufgaben und Termine ergänzen. Für Mail wird intern der vorhandene Entwurfsweg genutzt, für den
Assistenten der vorhandene Nachrichtenweg erweitert. Spätere Adapter dürfen direkt nach außen
senden. Gemeinsames Blatt in `torro-gui`, zuerst in Inbox, Kalender, Aufgaben und Assistent.
Kontextmenüs und Mehrfachauswahl benutzen dieselben Requests. Die API braucht eine
`ctl send …`-Fassade und Beispiele für JSON-Clients.

## Offene Produktentscheidungen vor dem Bau

- Soll eine Übergabe an den Assistenten immer einen neuen Chat öffnen oder standardmäßig die
  letzte Unterhaltung fortsetzen? Empfehlung: neue Unterhaltung, vorhandene nur bei expliziter ID.
- Soll `mail_message` standardmäßig Anhänge mitnehmen? Empfehlung: nein; in der Vorschau einzeln
  auswählbar, damit Größe und sensible Daten erkennbar bleiben.
- Für Cloud-Modelle: Welche Dateiarten darf der Kern extrahieren oder direkt an das Modell geben?
  Empfehlung für die erste Stufe: Nur Text/Markdown und ausdrücklich gewählter Mailtext; PDF und
  Bilder erst mit getesteten Extraktoren und sichtbarer Datenregel.
