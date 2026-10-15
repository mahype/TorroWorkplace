# Abgleich über Nextcloud: Assistent und Einstellungen auf mehreren Rechnern

Stand: 2026-10-14. **Diskussionsgrundlage.** Wunsch: Wer sich auf einem zweiten Rechner bei Nextcloud
anmeldet, hat dort denselben Assistenten (Gedächtnis, Persönlichkeit, Bots, Unterhaltungen) und
dieselben Einstellungen. Beide Rechner dürfen gleichzeitig benutzt werden, auch ohne Netz, ohne dass
eine Änderung stillschweigend verloren geht.

Baut auf [api.md](api.md) (Befehle, Rechte, Freigaben), [assistent.md](assistent.md) (Gedächtnis,
Bots, Unterhaltungen) und [settings.md](settings.md) (Ablage) auf. Aufgaben: [tasks/sync.md](tasks/sync.md).

## Die Frage, die die Funktion beantwortet

**„Ist mein Assistent auf diesem Rechner derselbe wie auf dem anderen?“** Ja: Was ich ihm auf dem
MacBook beigebracht habe, weiß er auch auf dem Linux-PC. Habe ich auf beiden gleichzeitig etwas
geändert, sagt er mir, wo, und ich entscheide.

## Was mitgeht und was nicht

| Daten | Heute | Stufe 1 |
|---|---|---|
| Gedächtnis (`memory.md`) | `<config>/assistant/` | **mit**, jede Notiz einzeln; Notizen „Nur auf diesem Computer“ bleiben hier |
| Profil und Persönlichkeit (`profile.json`, `persona.md`) | `<config>/assistant/` | **mit** |
| Bots (`bots/<id>/`: Profil, Persönlichkeit, Anweisungen, Skills, Antrieb, Modell) | `<config>/assistant/bots/` | **mit**, **ohne** Client und Rechte (siehe „Sicherheit“) |
| Unterhaltungen (`<id>.jsonl`) | `<data>/assistant/` | **mit**, mit Titel und Anheften |
| Einstellungen: `language`, `appearance`, `design`, `night`, `quietWhenBusy`, `assistantRetention`, `assistantDefault`, `listOrders`, `hiddenCollections` | `settings.json` | **mit**, jeder Schlüssel einzeln |
| Einstellungen: `startAtLogin`, `zoom`, `voice` | `settings.json` | **bleiben** auf dem Rechner (Bildschirm, Mikrofon, Autostart sind Sache des Geräts) |
| GitHub-Filterlisten, eigene Designs, Regeln des Eingangs | | Stufe 2 |
| Workspace-Vorlagen | `templates/` | Stufe 2, getrennt je Fenstermanager |
| Konten, Geheimnisse, Clients und Rechte, Freigaben, Protokoll, Hooks, Caches | | **nie** |

Antriebe haben auf jedem Rechner dieselbe Kennung (`ai:ollama@localhost:11434`, `claude-code`). Fehlt
ein Antrieb auf diesem Rechner, zeigt der Bot „Not available on this computer“ und bietet einen
anderen an. Die Wahl gilt dann für alle Rechner (Offen, Punkt 3).

## Grundidee: Jeder Rechner schreibt nur in sein eigenes Protokoll

Überschreiben zwei Rechner dieselbe Datei, gewinnt einer, und die Änderung des anderen ist weg.
ETags mit `If-Match` würden das nur bemerken, nicht lösen. Darum gilt:

1. **Kein Rechner überschreibt eine Datei, die ein anderer schreibt.** Jeder Rechner hat in Nextcloud
   einen eigenen Ordner und schreibt nur dorthin.
2. Er schreibt dort keine fertigen Dateien, sondern ein **Protokoll seiner Änderungen**: „Notiz
   1a2b angelegt“, „Persönlichkeit geändert“, „Bot 4c5d gelöscht“, „`design` = nord“, „Eintrag in
   Unterhaltung 17d2 angehängt“.
3. Jeder Rechner liest die Protokolle **aller** Rechner und setzt daraus denselben Stand zusammen,
   egal in welcher Reihenfolge er sie liest.

So gibt es keinen Schreibkonflikt auf Dateiebene, auch nicht offline. Konflikte gibt es nur noch
inhaltlich, und für sie gelten feste Regeln (unten).

Lokal ändert sich nichts: Die Dateien unter `<config>/assistant/` und `settings.json` bleiben, wie sie
sind, und der Kern arbeitet weiter mit ihnen. Der Abgleich läuft daneben. Er schreibt eigene
Änderungen ins Protokoll und trägt fremde in die lokalen Dateien ein.

## Ablage in Nextcloud

```
Torro Workplace/Sync/                 (in den Dateien des gewählten Nextcloud-Kontos)
  README.md                           was der Ordner ist; „nicht von Hand ändern, nicht teilen“
  format.json                         {"format": 1}
  devices/
    7f3a9c21e04b5d16/                 ein Ordner je Rechner, Name = Geräte-Kennung
      device.json                     Name, System, Version, öffentlicher Schlüssel
      snapshot.jsonl                  verdichtete eigene Einträge (optional)
      ops/
        1760440105322-7f3a9c21e04b5d16.jsonl   ein Abschnitt, nach dem Hochladen unveränderlich
        1760440170011-7f3a9c21e04b5d16.jsonl
```

- **Geräte-Kennung:** 16 Hex-Ziffern, zufällig beim ersten Einschalten, liegt in `<config>/sync.json`.
  Der Name ist zuerst der Rechnername und lässt sich ändern.
- **Abschnitte:** Ein Hochladen schreibt einen neuen Abschnitt mit allen Änderungen seit dem letzten.
  WebDAV kann nicht anhängen, darum gibt es neue Dateien statt wachsender. Ein hochgeladener Abschnitt
  ändert sich nie mehr. Andere Rechner lesen ihn genau einmal.
- **Verdichten:** Hat ein Rechner mehr als 50 Abschnitte oder 5 MB, schreibt er `snapshot.jsonl`
  (nur seine eigenen Einträge, je Schlüssel der letzte, dazu alle Einträge der Unterhaltungen) und
  löscht danach die eingeschlossenen Abschnitte. Er verdichtet nur seinen eigenen Ordner.
- **Hochladen sicher:** zuerst unter einem Namen mit `.part`, dann `MOVE` auf den endgültigen Namen.
  Ein abgebrochenes Hochladen hinterlässt nie einen halben Abschnitt.

## Einträge im Protokoll

Eine Zeile je Änderung, JSON:

```json
{"at":"1760440105322-0003-7f3a9c21e04b5d16","key":"memory/1a2b3c4d","base":["1760439001200-0000-a91c…"],"value":{"text":"Anna works in sales.","created":"2026-10-14T09:30:00Z"}}
{"at":"1760440105400-0000-7f3a9c21e04b5d16","key":"memory/9e8f7a6b","base":["…"],"deleted":true}
{"at":"1760440106000-0001-7f3a9c21e04b5d16","key":"conversation/17d285ae7c94092d/entry/7f3a9c21e04b5d16.42","value":{"time":"…","kind":"answer","text":"…"}}
```

- **`at`, die Uhr:** Eine hybride logische Uhr aus Millisekunden, Zähler und Geräte-Kennung. Sie
  ist nach Text sortierbar. Sieht ein Rechner einen Eintrag aus der Zukunft (Uhr des anderen geht
  vor), zählt er von dort weiter. So ist „neuer“ auf allen Rechnern dasselbe, und bei Gleichstand
  entscheidet die Geräte-Kennung.
- **`key`:** was geändert wurde, immer so klein wie möglich:

  | Schlüssel | Wert |
  |---|---|
  | `memory/<note-id>` | Text, Zeitpunkt |
  | `profile/name`, `profile/description`, `profile/avatar`, `persona` | Text |
  | `bot/<id>/<feld>` für `name`, `description`, `avatar`, `engine`, `model`, `persona`, `instructions`, `skills` | Text bzw. Liste |
  | `bot/<id>` | nur `deleted` |
  | `setting/<name>` | Wert wie in `settings.json` |
  | `conversation/<id>/meta/{started,title,pinned}` | wie im `.jsonl` |
  | `conversation/<id>/entry/<gerät>.<n>` | ein Eintrag ohne `seq` |
  | `conversation/<id>` | nur `deleted` |
- **`base`:** die Fassungen (ihre `at`), die der Rechner beim Ändern vor sich hatte. Daran erkennt
  jeder Rechner, ob zwei Änderungen voneinander wussten.
- **Gelöscht** heißt `"deleted": true` (Grabstein). Ohne Grabstein käme eine gelöschte Notiz vom
  anderen Rechner zurück. Grabsteine sind klein und bleiben.
- **Einträge der Unterhaltungen** bekommen ihre Kennung vom schreibenden Rechner (`<gerät>.<n>`). Die
  laufende Nummer `seq` vergibt jeder Rechner beim Eintragen selbst, nur aufsteigend. So bleibt
  `assistant.get_conversation { after }` richtig, auch wenn Einträge von woanders nachkommen.

## Zusammenführen und Konfliktregeln

Für jeden Schlüssel hebt der Rechner die Fassungen auf, die keine Nachfolger haben, die **Köpfe**
(wie bei Git). Ein Kopf ist eine Fassung, auf die keine andere Fassung über `base` verweist.

- **Ein Kopf:** Alles ist klar, er gilt.
- **Mehrere Köpfe:** Zwei Rechner haben geändert, ohne voneinander zu wissen. Es gilt sofort der
  neueste Kopf (größtes `at`), damit nichts blockiert. Die anderen bleiben erhalten, und je nach Art
  meldet der Rechner einen **Konflikt**.
- **Jede neue Änderung** an dem Schlüssel nennt alle Köpfe in `base` und löst den Konflikt damit auf.
  Wer die Persönlichkeit nach einem Konflikt bearbeitet und speichert, hat ihn also gelöst.

| Daten | Fall | Regel |
|---|---|---|
| Gedächtnis | neue Notizen auf beiden Rechnern | beide bleiben (eigene Kennung je Notiz) |
| | dieselbe Notiz auf beiden geändert | neuere gilt, die andere als **Konflikt** |
| | auf A gelöscht, auf B geändert | **Löschen gewinnt**, die Änderung von B steht als Konflikt „Restore?“ da |
| Persönlichkeit, Profil, Bot-Texte | auf beiden geändert | neuere gilt, die andere als **Konflikt**; die Oberfläche zeigt beide Fassungen, wählen oder von Hand zusammenführen. Prosa wird nie automatisch gemischt |
| Bots | auf A gelöscht, auf B geändert | Löschen gewinnt, Konflikt „Restore?“; die Unterhaltungen mit dem Bot bleiben lesbar |
| | neuer Bot auf beiden | beide bleiben (eigene Kennung) |
| Einstellungen | verschiedene Schlüssel geändert | beide gelten |
| | derselbe Schlüssel | neuere gilt, **ohne** Konflikt-Meldung (ein Klick stellt es zurück) |
| Unterhaltungen | neue Unterhaltung auf beiden | beide bleiben (eigene Kennung) |
| | dieselbe Unterhaltung auf beiden fortgesetzt | alle Einträge bleiben. Fremde Einträge werden hinten angehängt und zeigen, von welchem Rechner sie kommen. Kein Konflikt |
| | Titel oder Anheften auf beiden geändert | neuere gilt |
| | auf A gelöscht, auf B fortgesetzt | Löschen gewinnt. Später ankommende Einträge werden verworfen |

**Aufbewahrung:** Jeder Rechner löscht alte Unterhaltungen nach derselben (abgeglichenen) Einstellung
und schreibt dafür einen Grabstein. Gelöscht wird nur, was nach dem letzten Abgleich nicht angeheftet
ist.

**Laufende Antworten:** Antwortet der Assistent auf A, sieht B die Einträge beim nächsten Abgleich
(höchstens eine Minute später). Schreibt man auf B gleichzeitig in dieselbe Unterhaltung, kennt B die
laufende Antwort von A noch nicht. Das ist gewollt und wird nicht gesperrt.

## Ablauf

- **Einschalten** (Steuerzentrale › Sync): Nextcloud-Konto wählen (nur verbundene), Bereiche
  wählen. Der Kern legt den Ordner an, prüft, dass er **nicht geteilt** ist, und schreibt den
  heutigen Stand dieses Rechners als ersten Abschnitt.
- **Zweiter Rechner:** Nextcloud anmelden, Sync einschalten. Er sieht die vorhandenen Rechner und
  fragt „Use data from *MacBook*?“ (siehe „Sicherheit“). Danach führt er seinen eigenen Stand mit den
  fremden zusammen. Was es auf beiden gibt und verschieden ist (z. B. die Persönlichkeit), wird ein
  Konflikt. Nichts wird überschrieben.
- **Im Betrieb:**
  - Eigene Änderungen gehen zuerst in einen lokalen Ausgang (`<state>/sync/outbox.jsonl`) und werden
    5 s nach der letzten Änderung hochgeladen.
  - Fremde holt der Kern beim Start, jede Minute und auf „Sync now“. Nextcloud ändert das ETag eines
    Ordners, wenn sich darin etwas ändert. Ein `PROPFIND` (Tiefe 0) auf `Sync/` genügt also, um zu
    sehen, ob überhaupt etwas neu ist. Erst dann folgt Tiefe 1 auf die geänderten `devices/<id>/ops/`
    und `GET` der neuen Abschnitte. Benachrichtigungen per `notify_push` folgen in Stufe 2.
- **Ohne Netz:** Alles geht weiter. Der Ausgang wächst und wird hochgeladen, sobald Nextcloud wieder
  erreichbar ist.
- **Von Hand bearbeitet:** Ändert jemand `memory.md` oder `persona.md` im Editor, bemerkt der Kern das
  beim nächsten Takt (Prüfsumme gegen den Stand des letzten Abgleichs) und macht daraus Einträge.
- **Rechner entfernen** (z. B. verkaufter Laptop): Der Kern übernimmt die Einträge des Rechners mit
  ihrer ursprünglichen Uhrzeit in das eigene Protokoll und löscht dann dessen Ordner. Nichts von ihm
  geht verloren, aber er schreibt nicht mehr mit. Einen **unbestätigten** Rechner entfernen heißt
  ablehnen: Seine Einträge werden nie übernommen, nur der Ordner gelöscht.
- **Ausschalten:** Der Abgleich hört auf, die Daten bleiben lokal und in Nextcloud. „Remove this
  computer from sync“ markiert zusätzlich den eigenen Ordner (`"leaving": true` in `device.json`). Der
  nächste andere Rechner übernimmt dann dessen Einträge wie oben und löscht den Ordner.

## Sicherheit

Was aus Nextcloud kommt, landet in jedem Prompt (Gedächtnis, Persönlichkeit, Anweisungen der Bots).
Darum gilt:

1. **Rechner bestätigen.** Jeder Rechner hat ein eigenes Schlüsselpaar (Ed25519). Der private
   Schlüssel liegt im Schlüsselbund, der öffentliche in `device.json`. Jeder Abschnitt ist
   signiert. Einen neuen Rechner bestätigt die Person einmal auf jedem anderen Rechner („New computer
   *Linux-PC* wants to sync. Allow?“, mit Kurzform des Schlüssels). Erst dann trägt der Kern dessen
   Einträge ein. Abschnitte ohne gültige Signatur werden nicht eingetragen und gemeldet. So kann auch
   jemand, der in den Ordner schreiben kann (eine andere App mit App-Passwort, ein Sync-Client),
   nichts in den Prompt schmuggeln. Bestätigen geht nur mit dem App-Schlüssel (neuer Punkt 9 in „Was
   nie nach außen geht“, wie das Bestätigen eines Bots).
2. **Rechte wandern nie mit.** Ein Bot kommt ohne seinen Client. Auf jedem Rechner bekommt er einen
   eigenen neuen Client mit `Rights::none`, und die Person vergibt die Rechte dort (Punkt 2). Der
   Eingang meldet: „Bot *Reiseplaner* came from *MacBook*. Grant access?“
3. **Nur erlaubte Einstellungen.** Der Kern trägt nur die Schlüssel der Liste oben ein. `voice` (das
   Mikrofon, Punkt 6) und `startAtLogin` kommen nie über den Abgleich.
4. **Gleiche Grenzen wie bei Befehlen.** Jeder fremde Eintrag wird geprüft wie der passende Request:
   Längen (`MAX_NOTE_CHARS`, `profile_refusal`), Form der Kennungen (`is_id`), Steuerzeichen raus,
   Einstellungen wie bei `settings.set_setting` (vorhandenes Design, Wertebereich der Aufbewahrung,
   Form von `assistantDefault`). Was nicht passt, wird übersprungen und ohne Inhalt protokolliert.
   `hiddenCollections` passt, weil die Kennungen der Konten auf jedem Rechner gleich gebildet werden
   (`nextcloud:<login>@<host>`). Kennungen ohne Konto auf diesem Rechner stören nicht.
5. **Fremde Namen sind Daten.** Der Name eines Rechners kommt aus einem fremden `device.json`. In
   Freigaben, Eingang und Benachrichtigungen erscheint er gekürzt und ohne Steuerzeichen. Freigaben
   schlagen Rechner und Konflikte über ihre Kennung im Kern nach (`approvals::Facts`), nie aus dem
   Text des Clients. Die Freigabe für `sync.update_sync` nennt Konto und Bereiche: „Upload assistant
   and conversations to *account*“.
6. **Nicht geteilt.** Hat der Ordner `Sync/` oder einer darüber Freigaben (`oc:share-types`) oder ist
   er von jemand anderem geteilt (`nc:mount-type`), hält der Abgleich an und meldet es.
7. **„Nur auf diesem Computer“** bleibt, was es heißt: Solche Notizen werden nie Einträge.
8. **Protokoll ohne Inhalte** wie immer (`audit.jsonl`): Befehl, Kennungen, Zahl der Einträge. Keine
   Namen von Rechnern, keine Texte.
9. **Inhalt auf dem Server:** In Stufe 1 liegen die Einträge unverschlüsselt in den eigenen
   Nextcloud-Dateien, wie Kalender und Aufgaben auch. Nextcloud hält Versionen und Papierkorb, das ist
   ein Backup nebenbei. Ende-zu-Ende-Verschlüsselung mit einer Passphrase ist Stufe 2 (Offen, Punkt 2).

## Wo die Logik liegt

Wie überall: nur im Kern. Apps und Steuerzentrale schicken Requests.

- `crates/torro-nextcloud/src/files.rs` (neu): WebDAV auf die Dateien eines Nextcloud-Kontos
  (`PROPFIND` mit ETag, `oc:share-types`, `nc:mount-type`; `GET`, `PUT`, `MKCOL`, `MOVE`, `DELETE`).
  Gleiche Regeln wie OCS: App-Passwort nur im `Authorization`-Header, nur an den eigenen Ursprung.
- `crates/torroworkplace/src/sync/` (neu): Uhr, Einträge, Zusammenführen und Köpfe (ohne Netz,
  vollständig testbar), Anbindung an die lokalen Speicher, Ablauf mit Nextcloud, Geräte und
  Signaturen.
- Die vorhandenen Speicher (`assistant/profile.rs`, `assistant/store.rs`, `bots/store.rs`,
  `settings_service.rs`) bleiben die Wahrheit für diesen Rechner. Der Abgleich bekommt ihre Änderungen
  gemeldet und schreibt fremde über dieselben Funktionen ein. Es gibt keinen zweiten Speicher.
- Einstellungen des Abgleichs (an/aus, Konto, Bereiche, Geräte-Kennung, bestätigte Rechner) liegen in
  `<config>/sync.json`, **nicht** in `settings.json`. Sonst würde der Abgleich sich selbst abgleichen.

## Drei Wege

Neuer Bereich `sync`. Er steht **nicht** in den Vorlagen der Clients (wie `assistant` und `bots`): Man
schaltet ihn je Agent bewusst an. Für den Client des Assistenten und für Bots ist alles außer Lesen
gesperrt (`loops_back`), sonst könnten sie über einen Konflikt ihre eigene Persönlichkeit ändern.

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Stand lesen (an/aus, Konto, Bereiche, letzter Abgleich, Fehler, Zahl der Konflikte) | Steuerzentrale › Sync | `sync.get_status` · Read | `sync_get_status` |
| Ein-/ausschalten, Konto und Bereiche wählen | ebenda | `sync.update_sync { enabled?, account?, areas? }` · Admin (Freigabe) | `sync_update_sync` |
| Jetzt abgleichen | „Sync now“, Tray | `sync.run_now` · Write | `sync_run_now` |
| Rechner auflisten (Name, System, zuletzt gesehen, bestätigt) | Sync › Computers | `sync.list_devices` · Read | `sync_list_devices` |
| Diesen Rechner umbenennen | ebenda | `sync.rename_device` · Write | `sync_rename_device` |
| Rechner bestätigen | ebenda, Eingang, Benachrichtigung | `sync.confirm_device` · Admin | **Never** (neuer Punkt 9: Daten in den Prompt lassen entscheidet nur ein Mensch) |
| Rechner entfernen oder ablehnen; mit der eigenen Kennung „Remove this computer from sync“ | ebenda | `sync.remove_device` · Admin (Freigabe; wie `accounts.remove_account`) | `sync_remove_device` |
| Konflikte auflisten | Sync › Conflicts, Hinweis auf Persona-Seite und im Bot-Editor | `sync.list_conflicts` · Read; die Fassungen nur mit Leserecht im Bereich des Schlüssels (`assistant`, `bots`, `settings`), sonst nur Kennung, Art und Rechner | `sync_list_conflicts` |
| Konflikt lösen (eine Fassung behalten, auch „Restore?“ = `keep: other`) | ebenda | `sync.resolve_conflict { id, keep }` · Admin (Freigabe), zusätzlich das Recht des passenden Fachbefehls (`assistant.update_profile`, `assistant.forget`, `bots.save_bot`, `settings.set_setting`) | `sync_resolve_conflict` |
| Von Hand zusammenführen | Persona-Seite, Bot-Editor | die vorhandenen `assistant.update_profile`, `bots.save_bot` | wie vorhanden |

Neue Konflikte und neue Rechner kommen außerdem als Eintrag in den Eingang (Spur *For me*).

## Reihenfolge

Stufe 1 ist der ganze Assistent plus Einstellungen: Aufgaben N.1–N.8 in
[tasks/sync.md](tasks/sync.md). Stufe 2 (später, eigene Planung): Filterlisten (Offener Punkt 3 in
[github.md](github.md)), eigene Designs, Regeln des Eingangs, Workspace-Vorlagen je Fenstermanager,
Ende-zu-Ende-Verschlüsselung, `notify_push`.

## Offen

1. **Schnittstellen:** neuer Bereich `sync`, Punkt 9 in „Was nie nach außen geht“ und das optionale
   Feld `device` an `Entry` (von welchem Rechner ein Eintrag kommt). Das ändert festgelegte
   Schnittstellen (`Area`, `api.md`, `torro-ipc`) und braucht eine Freigabe.
2. **Verschlüsselung:** Stufe 1 ohne (wie Kalender und Aufgaben)? Vorschlag: ja. Ende-zu-Ende mit
   Passphrase in Stufe 2.
3. **Antrieb fehlt auf einem Rechner:** Vorschlag Stufe 1: Antrieb und Modell gleichen sich ab, ein
   fehlender wird angezeigt, und die Auswahl eines anderen gilt überall. Ein Antrieb je Rechner
   („hier Ollama, dort OpenRouter“) wäre ein Feld, das nicht abgeglichen wird. Später, falls nötig.
4. **Mail-Inhalte in Unterhaltungen:** Werkzeug-Ergebnisse können Mail-Text enthalten. Vorschlag: Sie
   gehen mit (eigener Server, wie der Mail-Cache auf der Platte). Ein Schalter „Keep mail contents on
   this computer“ ersetzt sie in den Einträgen durch „withheld“.
5. **Ordner:** fest `Torro Workplace/Sync/` oder wählbar? Vorschlag: fest in Stufe 1.
6. **`zoom`** je Rechner (Vorschlag) oder abgleichen?
