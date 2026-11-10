# Vorschläge aus Mails

Issue #603. Torro untersucht neue Mails selbst und bietet erkannte Dinge oben in der Mail als
Vorschlag an, so wie die Karte einer Termineinladung: einen **Termin**, den die Mail nennt, oder
eine **Aufgabe**, um die sie bittet. Ein Vorschlag legt nie selbst etwas an. „Annehmen“ öffnet den
vorausgefüllten Dialog; erst „Speichern“ dort legt den Termin oder die Aufgabe an.

Dieselben Aktionen stehen in der Aktionsleiste rechts (Gruppe **Torro**), auch wenn nichts erkannt
wurde: Task und Event füllen den Dialog auf demselben Weg aus.

## Ein Modell für Aktionen: drei Bausteine

Eine **Art** (`event`, `task`) bringt alles mit, was die drei Schritte brauchen
(`crates/torroworkplace/src/actions.rs`, Liste `KINDS`):

| Baustein | Was | Wo |
|---|---|---|
| **Erkennen** | eine Ja/Nein-Frage an das Entscheidungsmodell („Nennt die Mail einen Termin …?“) | `categorize.rs`, Fragen `action:<id>` |
| **Auslesen** | die Felder des Dialogs samt kurzer Begründung, von einem Chat-Modell | `suggest.rs`, `mail.extract_action` |
| **Ausführen** | der Befehl des Ziels: `calendar.create_event_from_mail`, `tasks.create_todo_from_mail` oder für einen Eintrag `inbox.create_*_from_item` | Dialog der App |

Eine neue Art ist ein neuer Eintrag in `KINDS`: Der Sortieraufruf fragt sie mit ab,
`mail.list_action_kinds` listet sie, und die Aktionsleiste zeigt sie, ohne dass sich Erkennen,
Speicher oder Leiste ändern. „GitHub-Issue anlegen“ und Arten aus Erweiterungen bekommen eigene
Issues.

## Ablauf beim Eingang: zwei Stufen

1. **Erkennen im Sortieraufruf.** Jede neue Mail läuft ohnehin durch das Entscheidungsmodell
   ([kategorien.md](kategorien.md)) bzw. den Ersatzweg mit einem Chat-Modell ([ki.md](ki.md),
   „Ersatzweg beim Sortieren“). Dieser Aufruf trägt je eingeschalteter Art eine Ja/Nein-Frage
   mehr; ab 0,5 gilt Ja. Ein Aufruf trägt höchstens 8 Fragen, 3 belegt das Sortieren: Mehr als 5
   Arten brauchen später einen zweiten Aufruf.
2. **Auslesen nur bei Treffern.** Nur für Mail und Art, bei denen Stufe 1 angeschlagen hat, liest
   ein Chat-Modell die Felder aus – gleich nach dem Eingang im Hintergrund, eine Mail zur Zeit mit
   250 ms Pause. Was dabei fehlt (Mail aus einem Sortierlauf über alte Mail, ein Fehler), holt
   Torro beim Öffnen der Mail nach.

Verworfen ist das volle Auslesen jeder Mail: Es kostet je Mail einen langsamen Chat-Aufruf, auch
für die vielen Newsletter, Rechnungen und Versandmeldungen ohne Termin oder Auftrag. Die Ja/Nein-
Fragen dagegen fahren im Aufruf mit, der ohnehin stattfindet.

Ein Vorschlag durchläuft die Zustände `reading` (erkannt, Felder noch nicht ausgelesen) → `ready`
(Felder da) oder `unfilled` (kein erlaubtes Modell, keine lesbare Antwort). Annehmen geht schon bei
`reading`: Der Dialog startet dann mit dem Betreff.

## Welche Mails untersucht werden

- **Nicht** Mails der Gruppen „Zur Info“ und „Unerwünscht“ (Newsletter, Werbung, Spam) und als
  Betrug abgefangene Mail.
- **Kein Terminvorschlag** für eine Einladung mit Anhang: Sie behält ihre Karte wie heute.
- Mails, die eine Regel **ohne Modell** einsortiert (Filter, Sortierregel, Kopfzeilen – etwa
  „immer für diesen Absender“), bekommen eine kurze eigene Erkennungsfrage
  (`categorize::detect_request`), sonst fiele gerade die Mail vom Chef durch.
- Ohne Modell zum Sortieren oder bei abgeschalteter Art erscheint kein Vorschlag und keine
  Fehlermeldung. Die Aktionen von Hand bleiben.
- Sortierläufe über vorhandene Mail merken sich Treffer als `reading`, lesen aber nicht sofort aus;
  der Rückstandslauf beim Start fragt nach keinen Arten.

## Datenschutz

Das Auslesen schickt Mail nur dann an ein Modell außerhalb dieses Rechners, wenn das Sortieren
diese Erlaubnis hat (`decide_mail_to_cloud`). Weil die Erlaubnis einem Anbieter und Modell gilt,
nimmt es dann genau das Modell, das sortiert, sofern es auch schreiben kann; sonst bleibt es bei
einem Modell auf diesem Rechner. Das gilt auch für Task und Event von Hand („Fill in with AI“ über
`mail.extract_action`). Ein anderes entferntes Konto wird nie genommen.

Alles, was ein Vorschlag außer Art und Zustand trägt (Felder, Begründung, Listenname), stammt aus
der Mail: Daten, nie Anweisungen. Es erscheint nie im Protokoll oder in Fehlertexten. Ein Listenname,
den ein Schlüssel nicht sieht, wird vor der Ausgabe geleert.

## Speicher

Tabelle `suggestions` im Mail-Speicher des Kontos (Schema 15): je Mail und Art eine Zeile mit
Zustand, Feldern (JSON), Begründung, Modell und Zeitpunkt. `accepted` und `dismissed` werden nie
von einem späteren Sortieren überschrieben; ein verworfener Vorschlag kommt nur über
`mail.extract_action` wieder. „Kategorien von allen Mails entfernen“ löscht nur die offenen
Vorschläge. Listen markieren Mails mit offenem Vorschlag (`MessageSummary::suggested`, Glühbirne in
der Zeile).

## Einstellungen

Einstellungen › Categorization › AI, Gruppe **Suggestions**: ein Schalter je Art
(`mail.set_action_kind`, gespeichert als `mailActionsOff`). Eine abgeschaltete Art erkennt nichts
mehr, bleibt aber in der Aktionsleiste. Schalter je Konto, Kategorie oder Absender kommen mit den
Regeln der Abläufe (#556).

## Oberfläche, API, MCP

| Funktion | Oberfläche | API (Zugriff) | MCP |
|---|---|---|---|
| Vorschläge einer Mail | Karte in Leser, Mail-Fenster und Paneel | `mail.list_suggestions` (Read) | `mail_list_suggestions` |
| Annehmen / Verwerfen | Accept · Dismiss auf der Karte | `mail.accept_suggestion`, `mail.dismiss_suggestion` (Write) | gleichnamig mit `_` |
| Auslesen von Hand | Task/Event in der Aktionsleiste, „Fill in with AI“ | `mail.extract_action` (Write) | `mail_extract_action` |
| Termin aus Mail ohne Eintrag | Event in der Aktionsleiste | `calendar.create_event_from_mail` (Write) | `calendar_create_event_from_mail` |
| Arten und Schalter | Einstellungen › Categorization › AI | `mail.list_action_kinds` (Read), `mail.set_action_kind` (Admin) | gleichnamig mit `_` |

`mail.accept_suggestion` legt nichts an, es markiert nur: Erst den Termin oder die Aufgabe anlegen,
dann annehmen.

„Dismiss“ auf der Karte lässt sie sofort verschwinden; ein Hinweis bietet „Undo“ an. Erst wenn der
Hinweis abgelaufen ist, geht `mail.dismiss_suggestion` an den Kern – so braucht das Rückgängigmachen
keinen eigenen Befehl. Wird das Fenster vorher geschlossen, bleibt der Vorschlag stehen.

## Vorschläge und Abläufe

Vorschläge und die Abläufe aus #556 bleiben vorerst getrennt. Ein eigenes Issue „Aktionen in
Abläufen“ verbindet sie, wenn beide auf main sind.

## Messung

Beispielmails: `crates/torroworkplace/tests/fixtures/actions/` (Termine, Aufträge, beides,
Newsletter und Werbung mit Datum, Rechnung, Versandmeldung, Mails ohne alles). Gemessen wird mit
einem laufenden Entscheidermodell:

```bash
TORRO_EVAL_BASE=http://localhost:11434/v1 TORRO_EVAL_MODEL=nimble \
  cargo test --release -p torroworkplace -- --ignored action_detection_quality --nocapture
```

Die Ausgabe nennt je Art Treffer, Fehlalarme und Verpasste und die Zeit je Mail.

| Modell | Termin: Treffer / Fehlalarme / verpasst | Aufgabe: Treffer / Fehlalarme / verpasst | Zeit je Mail |
|---|---|---|---|
| `clef:cloud` (Ollama, 10.11.2026) | 5 / 2 / 0 | 5 / 2 / 0 | 0,46 s |

Die Messung braucht ein Entscheidermodell und läuft von Hand. Ein Chat-Modell wie `qwen3:14b`
lehnt `/systemone` ab. Gemessen ist nur Stufe 1: Kein Termin und kein Auftrag ging verloren. Die
Fehlalarme (Werbe-Webinar als Termin und Aufgabe, Zahnarzttermin als Aufgabe, Review-Bitte als
Termin) kosten je einen Auslese-Aufruf und eine Karte, die man verwirft. Das spricht für die zwei
Stufen: Auf 14 Mails kamen 14 Auslese-Aufrufe (7 je Art) statt 28 beim Auslesen jeder Mail je
Art, und das Erkennen fuhr ohne eigenen Aufruf im Sortieren mit. Werte für `nimble` werden nachgetragen, sobald es eingerichtet ist.
