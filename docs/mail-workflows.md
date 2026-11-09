# Mail-Abläufe

Verbindlicher Umfang von #556 gemäß [Originalplan](https://github.com/mahype/TorroWorkplace/issues/556#issuecomment-6713253989)
und [Ergänzung](https://github.com/mahype/TorroWorkplace/issues/556#issuecomment-6721119925).
Die folgenden Regeln beschreiben den vorgesehenen Gesamtumfang, nicht eine bereits ausgelieferte
oder vollständig geprüfte Mail-Automation; zum Entwicklungs- und Abnahmestand siehe unten.

## Zweck und getrennte Objekte

Ein Ablauf beschreibt einen begrenzten Weg von einer Mail zu einem Rechnungsdokument. Ein Ablageziel
ist ein bereits vom Menschen eingerichteter Ordner mit seinen Namensregeln. Eine Regel verbindet
einen Ablauf mit Konto, Auslöser und Bedingung. Ein Lauf hält den konkreten Versuch und sein Ergebnis
fest. Das Anlegen oder Speichern eines dieser Objekte erteilt keinen dauerhaften Ausführungsgrant.

Die erste Version kennt nur Mail-Eingaben und Rechnungs-PDFs. Ihr Paket enthält höchstens 16
typisierte Schritte: Mail-Link, GET, HTML-Link, den geschlossenen Stripe-Resolver, die geschlossene
Paddle-Rechnungskopie, PDF-Prüfung und Ablageziel. Es gibt keine Ausdrücke, Programme, Browser,
Anmeldungen, Zahlungsaktionen oder frei ausführbaren Skripte. Andere Fähigkeiten werden abgewiesen.
Der vorbereitete Mail-Auftrag ist nur ein Anschluss für #260, keine allgemeine Übergabe-Funktion.
Der Erweiterungsrahmen aus #79 wird hier nicht ausgebaut; es kommen keine neuen Abhängigkeiten hinzu.

## Einrichten und bewusst ausführen

Im Leser unter „Torro“ und im Rechtsklickmenü der Nachrichtenliste stehen dieselben Einstiege:
„Create workflow with AI…“ und „Run workflow ›“, mit passenden Abläufen zuerst und „All workflows…“.
Bestehende Torro-Aktionen bleiben davor. Die Einrichtung verwendet eine ausgewählte Beispielmail,
„Download document“, ein vorhandenes Ablageziel und optional eine Ergänzung. Nur der Dokumentdownload
ist aktiv; andere Aufträge sind kein versteckter allgemeiner Executor.

1. „AI draft“ oder „Bundled template“ wählen. Der KI-Weg benutzt ausschließlich das eingerichtete
   Chat-Modell unter der Mail-Datenregel, ohne Modell-/Anbieterwechsel oder Web-Rückfall. Ein fehlendes
   Modell wird erklärt (`not-connected`); der Vorlagenweg braucht keines. Das Modell kann nur lokal
   validierte, begrenzte Wege vorschlagen, keinen ausführbaren Code oder Ausführungsrechte.
2. Entwurf mit Schritten, exakten Domains, nötigen Zugriffen, Ziel und Grenzen ansehen.
   **Preview ist lokal:** kein DNS, GET, Konvertieren, Reservieren oder Schreiben einer Ergebnisdatei.
   Auch die Vorschau vorhandener Regel-Treffer führt nichts aus.
3. **Test ausdrücklich bestätigen:** Er ruft tatsächlich ein Dokument ab und schreibt es in das
   angezeigte Ablageziel. Er ist keine Simulation. Der einmalige Testgrant gilt nur für die gewählte
   Mail, Fassung, Domains und das Ziel; Client-Anfragen brauchen eine echte menschliche Freigabe.
   „Open“ und „Show in folder“ erlauben anschließend die Ergebnisprüfung.
4. Erst eine erfolgreich getestete exakte Fassung mit „Save workflow“ zur bewussten manuellen
   Wiederverwendung speichern. **Beim ersten Speichern** wird der Ablauf dafür eingeschaltet
   (`enabled=true`); erneutes Speichern hebt eine spätere Deaktivierung nicht auf. Das erteilt
   **keinen Dauergrant und aktiviert keine Regel**. Manuelle App-Klicks gelten jeweils als einmaliger
   Auftrag. Sämtliche clientseitigen `workflows.run`-Aufrufe brauchen eine konkrete menschliche
   Freigabe, auch normale manuelle Läufe; der Einmalgrant wird an den jeweiligen Job gebunden.
   Automatik braucht dagegen einen gesonderten Dauergrant und eine aktivierte Regel.
   „Set up automatic execution…“ ist ein eigener Schritt mit einer zunächst ausgeschalteten Regel.

Unter Mail › Settings › Automation stehen **Workflows**, **Rules** und **Runs**. Listen führen zu
Details mit Bearbeiten, Testen, Duplizieren, Deaktivieren und Entfernen; Läufe zeigen Fortschritt,
Abbruch, Gründe, Teilergebnisse und ausdrücklich bestätigte Wiederholung. Schmal stehen Liste und
Detail nacheinander mit Zurück; Tastatur und Fokus müssen erreichbar bleiben. Originalanhänge bleiben
im vorhandenen Anhangsblock. Zusätzlich geholte Dokumente stehen getrennt als „Retrieved documents“
mit Herkunft, Status, „Open“ und „Show in folder“.

## Gebündelte Vorlagen

`workflows.list_templates` und `workflows.get_template` liefern öffentliche Metadaten und Pakete.
Die Auswahl erfordert kein KI-Modell. `workflows.instantiate_template` kopiert eine ausdrücklich
benannte Revision und Prüfsumme für eine erreichbare Beispielmail und ein vorhandenes Ablageziel.
Es entstehen weder Downloads noch Tests, Berechtigungen oder Regeln.
Die eigene Kopie ist ein inaktiver, ungetesteter Entwurf mit eigener Kennung, kein freigeschalteter
Automatismus. Dieselbe Einrichtung ist unter Workflows über „New from template…“ erreichbar.

| Kennung | Ergebnis | Grenze |
|---|---|---|
| `direct_pdf` | Rechnungs-PDF eines direkten Links | Genau ein eindeutiger PDF-Link; dessen exakter Host wird lokal an den Entwurf gebunden. Der Beispielhost im öffentlichen Paket ist kein Abrufziel. |
| `stripe_invoice` | Originale Rechnungs-PDF | Nur der begrenzte Stripe-JSON-Resolver und sein gelieferter PDF-Link; keine Beleg-, Zahlungs-, Dashboard- oder Browserwege. |
| `paddle_setapp_invoice_copy` | Lokal erzeugte Rechnungskopie | Nur das eingeschränkte, ressourcenfreie Profil. Stylesheets und Skripte werden abgewiesen. Die echten Paddle-/Setapp-Beispiele sind noch nicht unterstützt; siehe #598. |

**Bewusst zurückgestellt:** Die beiden echten Paddle-/Setapp-Beispiele referenzieren veränderliche externe
CSS-/JavaScript-Dateien. Deren Darstellungseinfluss ist nicht nachgewiesen; bekannte URLs allein reichen
nicht aus. Nach menschlicher Entscheidung in #556 werden diese Seiten daher sicher als `unsupported`
abgewiesen, ohne Nachladen und ohne Erfolgs-PDF. Die vollständige reale Abnahme und ein sicherer
Folgeansatz werden in [#598](https://github.com/mahype/TorroWorkplace/issues/598) nachgeholt. Synthetische
ressourcenfreie Profiltests sind kein Nachweis für diese echten Anbieterfälle.

Jede Kopie behält `template_origin` mit Kennung, Revision und Prüfsumme. Spätere Änderungen an der
mitgelieferten Vorlage verändern keine bestehenden Abläufe. Bearbeiten oder Duplizieren übernimmt
keinen erfolgreichen Test und keine Autorisierung.

## Rechte und Daten

Der Bereich `workflows` ist in allen Rechtevorlagen zunächst aus. Mailabhängige Workflow-Befehle
brauchen zusätzlich `mail` zum Lesen und das betreffende Konto in der Auswahl; die beiden öffentlichen
Vorlagenabfragen lesen keine Mail. Für ein Ablageziel gelten
die bestehenden Mail-Ablagerechte; ein Zielbezeichner ist kein Ordnergrant. Nicht erreichbare oder
fremde Objekte verhalten sich wie unbekannte. Die Filterung erfolgt vor dem Lesen gespeicherter
Inhalte.

Der Dienst bewahrt den ursprünglichen Träger und seine Einschränkungen dauerhaft. Aktuelle Rechte
werden mit dieser Obergrenze geschnitten; eine spätere Freigabe oder Bearbeitung über die Oberfläche
macht aus einem Client-Ablauf keinen Auftrag mit App-Rechten. Autorisieren und Widerrufen stehen
nur Menschen mit dem App-Schlüssel offen und sind keine MCP-Werkzeuge. Eine Autorisierung muss an
einen erfolgreichen Test, die konkrete ausführbare Fassung und den unveränderten Ziel-Fingerprint
gebunden sein.
Der ausdrückliche Dauergrant bindet Träger, Konten, exakte Hosts, Wirkungen und ausführbares Paket
einschließlich Adapter-/Profil-/Parser-/Font-/Satzbindung. Er ist weder ein allgemeiner Ordnergrant
noch eine Erlaubnis für beliebige spätere Fassungen. Eine normale Client-Freigabe entscheidet nur
den konkreten Auftrag, nicht diese dauerhaften Rechte. Vor jedem Außen- und Schreibschritt werden
aktuelle Rechte, Freigaben und Bindungen erneut geprüft. Entzug, Deaktivierung, Entfernen oder eine
Zieländerung stoppen die folgenden Schritte; bereits abgelegte Dokumente bleiben erhalten.
Ein verschwundener oder nicht erreichbarer Zielordner wird als „Save target unavailable.“ gemeldet,
nicht als Entzug unveränderter Torro-Rechte. Andere Verzeichnisidentitäten und unsichere Pfade
bleiben gesperrt; diese Unterscheidung gilt auch für die Wiederaufnahme unterbrochener Läufe.
Deaktivierung und Widerruf erhöhen die Ausführungsepoche: Sie stoppen auch bereits laufende
manuelle Einmalaufträge, nicht nur künftige automatische Jobs. Ein bestehender Einmalgrant darf
diese Grenze nicht überleben.

Freigaben zeigen die dienstseitig ermittelte Mail (Kennung, Betreff, Absender und Datum),
Ablauffassung und Regelkategorie mit ihrem stabilen Schlüssel; ohne Einschränkung steht ausdrücklich
„Any category“/„Beliebige Kategorie“. Auch im Notification Hub bleiben Zusammenfassung und Details im
Eintrag zugänglich. Überschreiten sie dessen Textgrenze, gibt es dort keine direkten Allow-/Deny-
Aktionen; entschieden wird in der Steuerzentrale mit vollständigen Angaben.
Nur die Zusammenfassung wird gekürzt. Die aufgeklappten Ablaufdetails behalten Domains,
Regelbedingungen und Mail-/Zielmerkmale vollständig, bereinigt und bei langen Werten in zitierte
Abschnitte aufgeteilt; insbesondere bleiben Domainenden und der Schluss einer Bedingung sichtbar.
Der Job bewahrt zusätzlich einen Hash der freigegebenen Mailmerkmale. Diese werden vor dem Abruf
und nach dem Download anhand genau des für die Ablage verwendeten Headerstands geprüft. Änderungen
bei unveränderter Rohmail pausieren den Lauf, statt unbestätigte Pfadbestandteile zu verwenden.
Ältere wartende Jobs ohne diese Bindung pausieren ebenfalls und benötigen eine neue Bestätigung.

Entwürfe, Bedingungen und Ergebnisse sind nicht vertrauenswürdige, aus Mail abgeleitete Daten. Die
Mail-Datenregel des Assistenten gilt auch für Antworten und Gesprächsverlauf von `workflows.*`;
ausgenommen sind nur die beiden öffentlichen Vorlagenabfragen. Ein im Gespräch ausgeschalteter
Mail-Bereich sperrt auch mailabhängige Workflow-Werkzeuge.

Pakete enthalten keine Beispiel-URLs oder Anmeldedaten. Private Maillinks und signierte Downloadziele
existieren nur vorübergehend im laufenden Abruf. Diagnose und Audit enthalten keine URLs, Tokens,
Rechnungsinhalte, Bedingungen oder Dateipfade.

## Regeln, Ereignisse und Bestandsgrenze

Eine Regel verknüpft Konto, Auslöser und Suchbedingung mit einem Ablauf und dessen getrenntem
Ablageziel. Die Bedingung nutzt die bestehende `filter_query`-Suchsprache, nicht eine neue Ausdrucks-
oder Skriptsprache; `MailFilter` bleibt unverändert. Regeln werden neu, nach Bearbeitung und als
Duplikat ausgeschaltet gespeichert. Einschalten ist eine gesonderte Entscheidung und braucht den
getesteten, autorisierten und eingeschalteten Ablauf.

Es gibt zwei Auslöser: **Neue Mail** (`new_mail`) und **Kategorie zugewiesen** (`category_assigned`,
mit gewählter Kategorie). Die erste Einordnung einer neuen Mail oder ein tatsächlicher interaktiver
Kategorie-Wechsel kann ein Ereignis liefern, nicht bloß eine andere Konfidenz oder Quelle derselben
Kategorie. Bulk-Verarbeitung, Erst-/Resetbestand, Neukategorisierung und Automation erzeugen keine
solchen Auslöser und damit keine Automationsschleife.

Automatik verarbeitet nur passende Ereignisse ab Regelaktivierung, mit bestätigten exakten Domains
und geprüftem Absender (`SenderCheck::verified_domain`, DKIM-Bezug zur Absenderdomain). Diese Prüfung
ist unabhängig vom Schalter für Absenderlogos. Ungeprüfte Absender warten sichtbar auf manuelle
Prüfung, statt automatisch herunterzuladen.

Eine **persistente Baseline-Epoche mit Bestandsgrenze** bleibt bis zum vollständigen ersten Abgleich
beziehungsweise nach neuer UIDVALIDITY geschlossen. Auch nach einem Crash mitten im Import werden
alte oder neu nummerierte Ordnerbestände nicht zu neuen Zustellungen. Aktivieren allein enumeriert
und verarbeitet keinen Altbestand. Nachgeholt werden höchstens sieben Tage alte Zustellungen nach
Aktivierung; das Alter kommt aus IMAP `INTERNALDATE`, nicht aus dem vom Absender gesetzten `Date`.
Ältere oder zeitlich nicht belegbare Eingaben werden sichtbar übersprungen beziehungsweise manuell
geprüft. Vorhandene Mail wird nur nach Vorschau und ausdrücklicher Auswahl einzelner Mail-IDs gestartet.

Import beziehungsweise Kategorieänderung und Ereignis-Outbox werden in **einer Transaktion**
gespeichert. Der Runner übernimmt die stabile Ereignis-ID dedupliziert dauerhaft und bestätigt erst
danach den Empfang (Ack). Neustart, verpasste Live-Meldungen und ein erneutes Lesen erzeugen so keine
zweite Außenwirkung. Vorbereitung und Kategorisierung arbeiten unabhängig von `notify`, `\Seen` und
der Zustellung im Notification Hub: `waiting_body`, `waiting_category`, `ready` und `failed` bleiben
dauerhaft nachvollziehbar. Ohne eingerichtetes Sortiermodell geht eine Mail ohne Kategorie auf
`ready`; ein Modell-/Datenregel-/Vorbereitungsfehler wird nicht als Erfolg versteckt, sondern als
sichtbare Pause behandelt. Die gewöhnlichen Mail-Filter und Hub-Benachrichtigungen behalten ihre
eigenen Regeln; ein ausgeschaltetes „Show new mail“ schaltet keine Workflow-Regel aus.

Die normale Benachrichtigung wird nach Pflichtfiltern und Sicherheitsguard abgelegt, ohne auf das
optionale Sortiermodell zu warten. Zwei Pflichtpipeline- und zwei Modell-Worker koordinieren sich
mit Claims pro Mail statt einer globalen Batchsperre. Ein terminal fehlender oder zu großer Body
darf die Benachrichtigung nicht verlieren; das Workflow-Ereignis bleibt dabei fehlgeschlagen.
Ein separater persistenter Zustellbeleg überlebt Neustarts und die Bestätigung durch den Runner.
Lese-/Papierkorbfilter gelten auch dann, wenn die Person schon eine Kategorie gesetzt hat;
automatische Kategorieentscheidungen dürfen diese Personenauswahl nicht überschreiben.

Hierfür wird der Mail-Cache additiv auf **Schema 16** migriert. Ältere Binärdateien können diesen
Store nicht öffnen; sie dürfen ihn nicht still zurücksetzen oder als neuen leeren Bestand behandeln.

## Dauerhafte Läufe, Deduplizierung und Dateien

Das private SQLite-Journal hält Queue, Läufe, Autorisierungen und Deduplizierung über Neustarts
hinweg (Unix: Verzeichnis 0700, Dateien 0600). Fehlend bedeutet leer; beschädigte oder unbekannte
Speicherstände bleiben unverändert und sperren die Ausführung, statt still neu anzufangen.
Es laufen höchstens **zwei Worker**, bei höchstens **100 offenen Jobs** einschließlich wartender
Vorbereitung; eine volle Queue meldet `busy`. Netzwerkfehler, HTTP 429 und 5xx erlauben höchstens
zwei Wiederholungen nach fünf und 30 Sekunden, andere Fehler keine automatische Wiederholung.

Die atomare Deduplizierung bindet **Konto + Hash der vollständigen Rohmail mit CRLF-Zeilenenden +
Ablauf-ID + Dokumentrolle `invoice` + Ablageziel**. Sie gilt auch für Tests und mehrere passende
Regeln; Regel, Revision und Herkunft (`supplier_pdf` oder `html_copy`) sind kein neuer Schlüssel.
Erneutes Anfordern liefert den vorhandenen Lauf, statt erneut abzurufen. Nur ausdrücklich
bestätigtes „Repeat“ (`repeat: true`) eröffnet eine neue Generation; ein noch aktiver Lauf darf
nicht parallel als neue Generation wiederholt werden.

Vor dem Schreiben werden Dokumenthash und **genau reservierter relativer Pfad** journalisiert.
Commit verwendet diesen Pfad mit `create_new` und Sync, nicht eine zweite Namenswahl. Nach einem
Crash wird ausschließlich dieser Pfad mit dem gespeicherten Hash abgeglichen; unklarer Zustand
pausiert als `recovery_uncertain`, ohne heimlichen Neuabruf, neue Nummerierung oder Erfolgsbehauptung.
Laufende Downloads werden nach einem unklaren Abbruch nicht blind neu gestartet. Abbrechen stoppt
weitere Schritte, bewahrt aber schon entstandene Dateien und sichtbare Teilergebnisse.

Ziel-ID, Relativpfad, Größe und SHA-256 bleiben dauerhaft am Dokument. Lesen, Öffnen und „Show in
folder“ prüfen erneut aktuelle Rechte, Zielbindung, Pfad und Hash; `get_document` liefert höchstens
256 KiB je Stück und akzeptiert keinen freien Dateipfad. Fehlende oder geänderte Dateien werden
sichtbar markiert und nicht automatisch neu geladen; veränderte Dateien werden nicht als geprüft
geöffnet. Es gibt keine automatische Löschung: Auch das Entfernen eines Ablaufs oder einer Regel
behält Dokumente, Läufe und Deduplizierungsnachweise.

## Browserfreie Abrufbausteine

Ein GET darf nur zu einem ausdrücklich gebundenen HTTPS-Host auf Port 443 gehen. Benutzerangaben,
IP-Schreibweisen und lokale Namen werden abgewiesen. Der Verbindungsaufbau benutzt den bestehenden
`GuardedDns` ohne Proxy; Redirects werden einzeln erneut geprüft. Es werden keine Cookies,
Anmeldedaten oder Referer weitergegeben.

Ein Abrufbudget umfasst höchstens acht GETs, fünf Redirects, fünf Sekunden Verbindungsaufbau und
60 Sekunden Gesamtzeit. HTML ist auf 2 MiB, das Stripe-JSON auf 256 KiB und PDFs auf 50 MiB
begrenzt. Fehlermeldungen tragen keine Serveradresse oder Antwortinhalte zurück.

Stripe wird nur aus den letzten beiden validierten Segmenten des tatsächlich in der Mail
enthaltenen Hosted-Invoice-Links aufgelöst. Das JSON muss genau ein nichtleeres Stringfeld
`file_url` enthalten; doppelte Schlüssel werden vor der Mapbildung abgewiesen. Nur das gelieferte
Ziel auf dem ausdrücklich zugelassenen Datei-Host wird verwendet. Fehlende oder fremde Varianten
werden nicht durch erratene Pfade, Receipt-Links oder Browserbedienung ersetzt. Ein Ablauf wird nur
mit einem tatsächlichen Ablaufbeleg als `expired` bezeichnet.
Beim geschlossenen Resolver darf auch HTTP 400 einen ausdrücklichen Ablaufhinweis tragen:
höchstens 16 KiB JSON-/Klartextfehler, unter denselben Zeit-, GET- und aktuellen Rechteprüfungen.
Nur unterstützte explizite Ablaufcodes oder eindeutige Ablaufsätze werden erkannt; der Status allein,
beliebige Vorkommen von „expired“, HTML und unbekannte oder mehrdeutige Antworten reichen nicht.
Es folgt kein weiterer Abruf oder automatischer Retry, und keine Antwortinhalte werden ausgegeben.
Die Testfehler sind synthetisch, keine aufgezeichnete private Stripe-Payload oder reale Produktabnahme.

Die exakten Stripe-Hosts sind `invoice.stripe.com`, `invoicedata.stripe.com` und
`stripe-upload-api.s3.us-west-1.amazonaws.com`, ohne Wildcards. Der geschlossene Resolver kodiert
die letzten beiden validierten Linksegmente einzeln für `invoice_pdf_file_url`; er konstruiert keine
PDF-Zieladresse. `pay.stripe.com`, `account.mapbox.com`, Zahlungs- und Receipt-Aktionen gehören nicht
zum Rechnungsweg. Die Grenzen ergeben keine universelle Stripe-Kompatibilitätszusage.

**PDF-Prüfung:** Der größen- und strukturbegrenzte Containerparser akzeptiert PDFs mit klassischer
Xref-Tabelle. Er dekomprimiert keine Streams, Fonts oder Bilder; komprimierte Xref-Streams und
Objektstream-Varianten (`ObjStm`) werden als `unsupported` abgewiesen. Das ist keine vollständige
Inhalts-, Darstellungs- oder Malwareprüfung und kein Versprechen, jedes gültige Anbieter-PDF zu
akzeptieren. Eine nicht unterstützte Variante wird nicht durch einen unbeschränkten PDF-Lader ersetzt.

Gründe bleiben unterscheidbar, etwa `auth_required`, `not_found`, `unexpected_type`, `unsupported`,
`offline` und `target_unavailable`. Ein Anmeldehinweis erklärt die Grenze; Torro meldet sich nicht
am Portal an. „Later“ pausiert, statt Browser, Login oder Skript als Umweg auszuführen.

## Paddle: vollständige HTML-Kopie statt angebliches Original

Der Konverter dekodiert strikt ohne Ersatzzeichen. Unbekannte, widersprüchliche oder defekte
Kodierungen werden abgewiesen. Ein begrenzter Tokenizer und ein geschlossenes Profil extrahieren
Textblöcke und Tabellenzellen einschließlich ihrer Beziehungen. Es gibt keine Readability- oder
Sanitizer-Vorstufe. Nur die belegte Navigation und Bearbeitungsoberfläche außerhalb der Rechnung
beziehungsweise das bekannte Bearbeitungssteuerelement werden weggelassen.

Relevante Formwerte, Bilder, unbekannte Klassen oder Darstellungsweisen, versteckte Inhalte und
nicht belegte Zeichen werden nicht stillschweigend entfernt. Stattdessen endet die Konvertierung
mit `unsupported`. Weder HTML noch CSS, Bilder, SVG, Fonts, Skripte oder Linkziele des Anbieters
werden ausgeführt oder nachgeladen.
Das gilt auch für den Dokumentkopf und die Vorfahren des Rechnungsblocks: Stylesheets, Style-/Script-
Elemente, Ereignishandler und ungeprüfte Darstellungseinflüsse werden nicht als harmlose Metadaten
übersprungen. Die derzeitigen echten Paddle-Seiten fallen unter diese Sperre (Folge-Issue #598).

Die Kopie wird mit den eingebetteten Standardfonts auf A4, mit 15 mm Rand und 11 pt gesetzt. Der
Satz misst Umbrüche, Tabellenzellen und Seitengrenzen. Auf jeder Seite steht ausdrücklich, dass es
eine aus HTML erzeugte PDF-Kopie und nicht die Original-PDF des Anbieters ist. Das eigene erzeugte
PDF wird wieder eingelesen und mit sämtlichen Quellblöcken verglichen. Eine Differenz erlaubt keine
Ablage als Erfolg.

Grenzen: Token 16 KiB, 64 Attribute, Tiefe 64, 50.000 Knoten, 1 MiB Text, 50 Seiten, 50 MiB PDF und
ein kooperativ geprüftes Gesamtbudget von fünf Sekunden. Global darf höchstens ein Konverter
arbeiten. Seine Zulassung bleibt beim tatsächlichen Worker, auch wenn die wartende Zukunft
abgebrochen wird.

Adapter, Profil, Parser, konkrete Fontbytes und Satzversion sind Teil der ausführbaren Bindung.
Eine andere Bindung braucht einen neuen Test und für Automatik eine neue menschliche
Dauerautorisierung, nicht nur eine gleich gebliebene sichtbare Beschreibung. Manuelle Läufe
brauchen jeweils ihren konkret gebundenen Einmalauftrag.

## Lokale Verifikation ohne private Fixtures

Die eingecheckten August-/Oktober-Beispiele und Stripe-Daten sind vollständig synthetisch. Die
erwarteten vollständigen Texte stehen unabhängig neben den HTML-Fixtures. Die Tests prüfen unter
anderem Vollständigkeit, PDF-Rücklesen, mehrseitigen Satz, unsichere Eingaben, Kodierung und die
Konverterzulassung nach Abbruch.

Für die ausdrücklich bereitgestellten echten Paddle-Beispiele gibt es einen separat ignorierten
lokalen Abnahmetest. Er liest nur ein ausdrücklich gesetztes privates temporäres Verzeichnis mit
`august.html` und `october.html`; in Ausgabe und Repository gelangen keine Rechnungsinhalte oder
privaten Links. Die dort erzeugten PDFs brauchen zusätzlich einen unabhängigen vollständigen
Inhaltsvergleich sowie eine Prüfung jeder Seite. Ein erfolgreicher HTML-Abruf allein ist keine
Konvertierungsabnahme. Für die aktuelle sichere Ablehnung gibt es außerdem den lokalen Negativtest
`local_paddle_unproven_resources_are_rejected`; dessen Erfolg ist ausdrücklich keine positive
Paddle-Abnahme. Die positive Abnahme bleibt bis #598 offen.

## Aktueller Implementierungsstand

Verträge, Vorlagen, Entwürfe, Vorschau, Trägerfilterung, KI-Vorschläge, Runner und Mail-Outbox sind
mit der Einrichtungs- und Verwaltungsoberfläche verbunden. Die Implementierung ist weder ein
Auslieferungsnachweis noch der Nachweis einer erfolgreichen Gesamtprüfung. Nicht nachgewiesene
Anbietervarianten bleiben gesperrt; die reale Paddle-Unterstützung wird gemäß menschlicher
Scope-Entscheidung aus #556 in #598 nachgeholt.

Die browserfreien Stripe-Voruntersuchungen aus der Ergänzung ersetzen keine Abnahme des fertigen
Produktpfads mit zwei geeigneten Rechnungen einschließlich Rechnung statt Zahlungsbeleg und klarer
Meldung eines abgelaufenen Links. Auch die echte Paddle-Abnahme (10.08./10.10.2026: sämtliche
Rechnungstexte, Tabellenbeziehungen und jede PDF-Seite) sowie die DE/EN-Sicht-/Tastaturmatrix und
die vollständigen lokalen Prüfungen sind gesondert zu belegen. Dieses Dokument behauptet keinen
dieser Abschlüsse und keine echte Stripe-Produktabnahme.
