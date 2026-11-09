# Beispielmails zum Messen der Erkennung von Vorschlägen

Issue #603, Konzept [vorschlaege.md](../../../../../docs/vorschlaege.md).

Je Datei eine anonymisierte Mail (Betreff, Absender, Anfang des Textes) und was ein Mensch darin
erwartet (`expect`): ob die Mail einen Termin (`event`) und ob sie eine Aufgabe (`task`) für den
Empfänger enthält. `fields` nennt, was das Auslesen ungefähr finden sollte; geprüft wird es nur von
Hand. Keine echten Adressen, keine echten Namen, keine Anhänge.

Dabei sind Termine, Aufträge, beides, ein Newsletter und eine Werbung mit Datum, eine Rechnung, eine
Versandmeldung und Mails ohne alles: Gerade die mit Datum, aber ohne Termin für den Empfänger,
zeigen die Fehlalarme.

Messen mit einem Entscheidermodell (Ollama mit `/v1/systemone`):

```bash
TORRO_EVAL_BASE=http://localhost:11434/v1 TORRO_EVAL_MODEL=nimble \
  cargo test --release -p torroworkplace -- --ignored action_detection_quality --nocapture
```

Die Ausgabe nennt je Art Treffer, Fehlalarme und Verpasste und die Zeit je Mail. Die Werte stehen
in [vorschlaege.md](../../../../../docs/vorschlaege.md), Abschnitt „Messung“.

Format (JSON):

```json
{
  "expect": { "event": true, "task": false },
  "subject": "…",
  "from": "Name <name@example.org>",
  "list": null,
  "bulk": false,
  "text": "…",
  "fields": { "title": "…", "start": "…" }
}
```
