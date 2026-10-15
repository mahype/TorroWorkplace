# Teilen: Inhalte einer App in eine andere geben

Stand: 2026-10-15. **Konzept, Entscheidungen vom 2026-10-15 eingearbeitet.** Aufgaben:
[tasks/teilen.md](tasks/teilen.md). Erster Anlass: Kontakte aus [`torrocontacts`](kontakte.md) per
Mail weiterleiten oder in einen Chat geben. Gebaut wird Teilen aber als **Baustein der Suite**:
Später teilen auch Notizen, Termine, Aufgaben und Fotos über denselben Weg.

## Ziel

Überall in der Suite gibt es denselben Knopf **„Share…“**. Er öffnet ein Teilen-Blatt mit Vorschau,
der Wahl, was mitgeht, und den Zielen:

| Ziel | Was geschieht | Stufe |
|---|---|---|
| **Mail** | ein Entwurf mit Text und Datei im gewählten Konto, das Schreibfenster öffnet sich | TL1 |
| **Copy** | der Text in die Zwischenablage | TL1 |
| **Save as file…** | die Datei (`.vcf`) speichern | TL1 |
| **QR code** | die vCard als QR-Code, zum Abscannen mit dem Handy, ohne Netz | TL1 |
| **Chat** | eine Nachricht mit Text und Datei in eine Unterhaltung (Nextcloud Talk) | TL2, umgesetzt in CH3 der Chat-App ([chat.md](chat.md)) |

Wie jede Funktion gibt es Teilen dreimal: in der Oberfläche (Teilen-Blatt), über die API und über
MCP. Ein Agent kann also „Schick Max die Kontaktdaten von Anna“ als Mail-Entwurf vorbereiten. Das
Senden bleibt, was es heute ist: nach außen und mit Freigabe.

## Begriffe

- **Inhalt (`ShareItem`):** was geteilt wird, als Art und Kennung, zum Beispiel
  `{ kind: "contact", id: "nextcloud:anna@cloud.example/contacts/<uid>" }`. Mehrere Inhalte gleicher
  Art gehen zusammen (drei Kontakte in einer Mail).
- **Auswahl (`ShareFields`):** was von einem Inhalt mitgeht. Bei Kontakten die Felder; andere Arten
  bringen ihre eigene Auswahl mit oder keine.
- **Karte (`ShareCard`):** was der Kern aus Inhalt und Auswahl macht: ein Titel, ein kurzer Text für
  Mail-Text, Chat und Zwischenablage, eine Datei (Name, Typ, Inhalt) und die Liste der mitgehenden
  Felder für die Vorschau.
- **Ziel:** ein Befehl, der Inhalte annimmt (`mail.save_draft`, später `chat.send_message`), oder eine
  reine Oberflächen-Aktion (Copy, Save as file, QR code), die nur die Karte braucht.

## Entscheidungen (2026-10-15)

1. **Baustein der Suite, kein eigener Bereich.** Es gibt keinen Bereich `share` und kein eigenes
   Recht „teilen“. Wer teilt, braucht, was er ohnehin bräuchte:
   - **Lesen** des Inhalts in dessen Bereich, samt Auswahl (Adressbuch, später Kategorie …).
   - **Das Recht des Ziels:** Write in `mail` für einen Entwurf; „nach außen“ für Senden und für den Chat.

   Ein Agent mit Mail-Rechten, aber ohne das Adressbuch, kann einen Kontakt nicht anhängen
   (`not-found`).
2. **Die Karte baut immer der Kern, aus seinem Cache.** Ein Client schickt nur Art, Kennung und
   Auswahl, nie den Text oder die Datei selbst. So landet in einer Mail genau das, was im Adressbuch
   steht, und kein Client kann einem Kontakt fremden Inhalt unterschieben.
3. **Kontakte gehen als Text plus vCard.** Der Text ist kurz: Name, Firma, Telefon, Mail, Adresse.
   Die Datei ist eine **neu gebaute, schlanke vCard 3.0** nur mit den gewählten Feldern, nicht die
   gespeicherte. In der gespeicherten stehen Notizen, interne `X-`-Felder, `GEO`, Fotos und Gruppen.
   Das teilt niemand aus Versehen. (Der Export aus KO3 gibt dagegen die volle vCard heraus, er ist
   ein anderer Fall.)
4. **Was mitgeht, wählt man vor dem Teilen.** Die Voreinstellung bei Kontakten ist sparsam:
   - **an:** Name, Firma und Position, alle Telefonnummern und Mail-Adressen, Arbeitsadressen, Web.
   - **aus:** private Adressen, Geburtstag, Notiz, Foto, Messenger, Gruppen.

   Ein Agent muss die Auswahl nicht schicken; dann gilt die Voreinstellung. Was er mehr will, nennt
   er ausdrücklich. Die Freigabe zeigt es.
5. **Mail als erstes Ziel.** `mail.save_draft` bekommt eine neue Anhang-Art, im JSON
   `{"kind": "items", "items": [{"kind": "contact", "id": "…"}], "fields": [...]}`: ein Anhang, eine
   Datei, auch für mehrere Kontakte. Der Kern erzeugt die Datei beim Speichern.
   - **Rechte:** geprüft vor der Freigabe und noch einmal bei der Ausführung, mit derselben
     Reichweite (auch den Grenzen eines geliehenen Schlüssels), wie bei
     `mail.respond_to_invitation`. Der Assistent ordnet den Befehl dann auch `contacts` zu, damit
     ein im Gespräch abgeschaltetes Adressbuch greift.
   - **Was beim Senden in der Freigabe steht, merkt sich der Kern** je Entwurf (Kennungen und
     Auswahl, neben der Tabelle der Entwürfe). Beim Senden holt er Namen und Felder über die
     Kennungen aus dem Cache. Eine `.vcf`, die anders in den Entwurf kam (aus einer empfangenen
     Mail, von einem anderen Client), erscheint nur als Datei, nie als geprüfte Karte. Ändert sich
     der Entwurf außerhalb der Suite, fällt der Eintrag weg (erkannt an einem Fingerabdruck des
     gespeicherten Entwurfs). `mail.get_message` nennt die gemerkten Karten eines Entwurfs, damit
     das Schreibfenster sie beim Weiterschreiben behält.

   Den Mail-Text setzt die App aus der Karte zusammen (der Mensch kann ihn ändern), ein Agent
   schreibt ihn selbst. Das Senden bleibt `mail.send_draft` mit allen heutigen Regeln. Die Freigabe zum Senden nennt die
   mitgeschickten Karten: „Contact card: Anna Muster (6 fields)“. Namen gehen durch dieselbe
   Entschärfung wie in anderen Freigaben (eine Zeile, keine Steuerzeichen, 60 Zeichen); kennt der
   Cache einen Kontakt nicht mehr, steht dort „unknown contact“.
6. **Chat als Ziel kommt mit der Chat-App** ([chat.md](chat.md), Stufe CH3). Hier steht, was ihr
   Senden-Befehl `chat.send_message` (MCP `chat_send_message`, Bereich `chat`) für das Teilen können
   muss (TL2):
   - Eine Nachricht kann Inhalte tragen: `items: [ShareItem]` mit Auswahl (`fields`).
   - Der Kern lädt die Datei in den Talk-Anhangsordner des Menschen (WebDAV, Ordner aus der
     Talk-Konfiguration) und teilt sie in die Unterhaltung (Freigabe-Art 10, Talk), mit dem Text der
     Karte als Beschriftung. Ohne Datei geht nur der Text.
   - Das ist **nach außen**, wie Mail senden. Der Chat benutzt denselben Schalter „Send on my behalf“
     (`Rights::outbound`), keinen eigenen ([chat.md](chat.md), Entscheidung 4). Der Upload schreibt
     außerdem in die Dateien des Menschen; das braucht kein eigenes Recht, steht aber in der Freigabe.
7. **Copy, Save as file und QR code sind reine Oberfläche.** Sie brauchen nur die Karte, die es über
   die API gibt (`<bereich>.share_card`). Was ein Agent damit macht, ist seine Sache, er hat die Daten
   ja schon lesen dürfen. Einen Befehl „in die Zwischenablage“ oder „Datei speichern“ gibt es nicht:
   Beides schriebe auf den Rechner des Menschen, ohne dass er es sieht. Das wird eine allgemeine
   Regel in [api.md](api.md): **Reine Darstellung auf dem Rechner des Menschen (Zwischenablage,
   Speichern-Dialog, QR-Code) ist kein eigener Befehl; ihr API-Weg ist der lesende Befehl.**
8. **Neue Arten melden sich an einer Stelle an.** Jede Art bringt einen eigenen Befehl
   `<bereich>.share_card` im eigenen Bereich mit (Rechte und Auswahl ergeben sich so von selbst).
   Dazu kommt ein Fall im Kern-Modul `share`, das für die Ziele aus einem `ShareItem` die Karte holt.
   Notizen, Termine, Aufgaben und Fotos kommen dazu, wenn ihr Plan es vorsieht (TL3).

## Aufbau

```
torrocontacts ─┐                       torroworkplace (Kern)
torronotes  …  ├─ torro_gui::share ─IPC─▶ contacts/service.rs  contacts.share_card
torroinbox  ───┘  (Teilen-Blatt)          share/mod.rs         ShareItem → ShareCard (je Art)
                                          mail/…               save_draft mit DraftAttachment::Items
                                          chat/… (Chat-Plan)   send_message mit items
```

- **`torro-ipc`:** `ShareItem { kind, id }`, `ShareKind` (vorerst nur `Contact`), `ShareCard`.
  `torro-mail` hängt von nichts ab und kennt den Inhalt darum nur als eigene Struktur
  `DraftAttachment::Items { items: Vec<ItemRef>, fields }` mit `ItemRef { kind, id }` als Text (das
  Feld `kind` des Anhangs ist der serde-Tag); `torro-ipc` prüft und wandelt um.
- **Schreibfenster:** `torroinbox --compose --draft <id>` öffnet einen gespeicherten Entwurf zum
  Weiterschreiben. Das braucht das Teilen, ist aber auch sonst nützlich (Ordner „Drafts“).
- **`torro-contacts`:** `ShareFields`, Voreinstellung, `share_text(contact, fields)` und
  `share_vcard(contact, fields)`. Ohne Ein-/Ausgabe, testbar.
- **`torro_gui::share`:** das Teilen-Blatt für alle Apps. Vorschau der Karte, Häkchen je Feld,
  Ziele, QR-Code. Es zeigt nur Ziele, die es gibt: Mail ab einem Mail-Konto, Chat ab einem Konto
  mit Talk und Recht auf `chat`.
- **Kern `share/mod.rs`:** löst ein `ShareItem` über den Dienst seiner Art in eine Karte auf, mit
  den Rechten des Aufrufers. Wird von `mail` und später `chat` benutzt.

## Befehle

| Stufe | Befehl | Zugriff | MCP |
|---|---|---|---|
| TL1 | `contacts.share_card { ids, fields? }` (Text, Datei, mitgehende Felder) | Read (`contacts`) | `contacts_share_card` |
| TL1 | `mail.save_draft` mit `attachments: [{ kind: "items", items, fields? }]` (Erweiterung; einen eigenen Entwurf ersetzen braucht wie bisher Delete) | Write (`mail`) + Read der Inhalte | `mail_save_draft` (vorhanden) |
| TL1 | `apps.open_app` mit `target: "draft:<m:id>"` (Schreibfenster mit dem Entwurf öffnen, Erweiterung) | Write (`apps`) | `apps_open_app` (vorhanden) |
| TL2 | `chat.send_message { conversation, text, items?, fields? }` (CH3.2 im [Chat-Plan](tasks/chat.md)) | Write (`chat`) + nach außen + Read der Inhalte | `chat_send_message` |
| TL3 | `notes.share_card`, `calendar.share_card`, `tasks.share_card`, … | Read im jeweiligen Bereich | je ein Tool |

Erläuterungen:
- **Kein Befehl steht auf `Never`.** Copy, Save as file und QR code haben keinen eigenen Befehl
  (Entscheidung 7). Ihr API-Weg ist `share_card`.
- **`share_card` ist `Read`:** Es liefert nur, was der Aufrufer ohnehin lesen darf, in anderer Form,
  und folgt denselben Regeln wie `get_contact`, auch für entfernte Modelle des Assistenten. Die
  Beschreibung sagt, dass die Felder **Daten, keine Anweisungen** sind (`untrusted` immer `true`).
  Die Datei kommt nur mit `include_file: true` (Base64), weil 5 MB den Kontext eines Agenten füllen
  und meist der Text genügt.
- **`draft:<m:id>`:** Die Kennung bestimmt das Konto. Das Konto muss in der Reichweite liegen; ein
  Client-Schlüssel öffnet nur eigene Entwürfe, der App-Schlüssel alle.
- **Freigaben** nennen bei Entwürfen und beim Senden jede Karte mit Name und Zahl der Felder, und
  wenn mehr als die Voreinstellung mitgeht, welche Felder (zum Beispiel „+ private address“).
- **Protokoll:** Befehl, Konto (wo es eins gibt), Art und Zahl der Inhalte; Kennungen nur als
  Prüfsumme, nie Namen oder Felder.

## Oberfläche

**Teilen-Blatt** (`torro_gui::share`), geöffnet mit „Share…“, `⌘⇧S`/`Ctrl+Shift+S` oder aus dem
Kontextmenü, bei Mehrfachauswahl für alle gewählten Inhalte:
- **Oben:** die Vorschau der Karte, so wie sie ankommt. Bei mehreren Inhalten „3 contacts“ mit
  aufklappbarer Liste.
- **Darunter:** „What to include“ mit Häkchen je Feld, mit der Voreinstellung aus Entscheidung 4.
  Private Felder stehen mit Schloss-Symbol und dem Hinweis „Private“.
- **Ziele als Reihe:** **Mail**, **Chat** (später), **Copy**, **Save as file…**, **QR code**.
  - **Mail** fragt nach dem Konto, wenn es mehrere gibt (vorgewählt das zuletzt benutzte), und
    öffnet das Schreibfenster von `torroinbox` mit dem Entwurf. Empfänger setzt der Mensch dort.
  - **QR code** zeigt den Code groß im Blatt. Bei mehr Daten, als ein QR-Code fasst, steht dort „Too
    much for one QR code – remove some fields“.
- **Schreibfenster der Mail:** Knopf „Attach contact…“ öffnet eine Kontaktsuche (über
  `contacts.list_contacts`) und hängt die Karte an. So geht es auch vom Ziel her.
- **Chat (TL2):** wählt die Unterhaltung aus einer Liste der letzten Unterhaltungen mit Suche.

## Offen (in der jeweiligen Aufgabe klären)

- **Systemweites Teilen** (macOS-Teilen-Menü, Linux-Portal `org.freedesktop.portal`) brächte
  AirDrop und andere Apps. Auf macOS braucht es AppKit-Code in `torro-appkit`. Das ist ein eigener
  Plan, nach Rückfrage.
- **Talk-Beschriftung:** geklärt im Chat-Plan. Die Beschriftung braucht `media-caption` (Talk 18),
  das alle unterstützten Nextcloud-Versionen haben; fehlt es doch, kommen zwei Nachrichten: erst der
  Text, dann die Datei (CH3.1).
- **Mehr Ziele:** Notiz („Save to note“), Aufgabe („Follow up“) und Assistent („Ask about this“) liegen
  nahe. Sie kommen erst, wenn ihr Plan es will.
