# Mail-Köpfe zum Nachjustieren der Konfidenzschwelle

Aufgabe E12.6, Konzept [kategorien.md](../../../../docs/kategorien.md).

Die Schwelle `categorize::CONFIDENCE_MIN` steht auf **0,6 und ist nicht gemessen** – beim Planen
gab es weder ein eingerichtetes Entscheidermodell noch ein echtes Postfach. Diese Dateien sind die
Grundlage, sie später zu messen.

Je Datei ein anonymisierter Mail-Kopf mit dem Anfang des Textes und der Kategorie, die ein Mensch
für richtig hält (`expect`). Keine echten Adressen, keine echten Namen, keine Anhänge.

Messen: Für jede Datei einen Aufruf `categorize::request` an ein laufendes Entscheidermodell
schicken und `choice` und `confidence` mit `expect` vergleichen. Gesucht ist der Wert, bei dem
möglichst wenige richtige Kategorien unter die Schwelle fallen und möglichst wenige falsche
darüber bleiben.

Format (JSON):

```json
{
  "expect": "invoice",
  "subject": "…",
  "from": "Name <name@example.org>",
  "list": null,
  "bulk": false,
  "text": "…"
}
```
