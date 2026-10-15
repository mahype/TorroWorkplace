# Kontakte: App `torrocontacts` auf CardDAV, mit Karte

Stand: 2026-10-15. **Konzept, Entscheidungen vom 2026-10-15 eingearbeitet.** Aufgaben:
[tasks/kontakte.md](tasks/kontakte.md). Beantwortet die Frage: **Wer ist das, und wie erreiche ich
ihn?** Dazu: **Wo ist das?**, mit einer Karte ohne Lizenzschlüssel.

## Ziel

Ein modernes Adressbuch der Suite: schnell durchsuchen, ansehen, bearbeiten, offline benutzbar.
Kontakte kommen per **CardDAV**, Nextcloud zuerst, danach jeder CardDAV-Server (iCloud, Fastmail,
mailbox.org, Radicale). Wie jede Funktion gibt es das Adressbuch dreimal: in der Oberfläche
(`torrocontacts`), über die API (Bereich `contacts`) und über MCP (`contacts_*`), also auch für den
Assistenten („Wie ist die Adresse von Anna?“, „Leg Max als Kontakt an“).

Adressen lassen sich auf einer **Karte** zeigen. Kacheln und Ortssuche kommen von OpenStreetMap,
ohne Schlüssel, ohne Konto, ohne Gebühren, unter deren Nutzungsregeln.

**Was „modern“ hier heißt:**
- Suche über alles (Name, Firma, Mail, Telefon, Ort, Notiz) beim Tippen, aus dem lokalen Index.
- Schnellaktionen in jeder Zeile: Mail schreiben, anrufen (`tel:`), Adresse kopieren, auf der Karte zeigen.
- Favoriten, zuletzt benutzte Kontakte, anstehende Geburtstage.
- Gruppen, Fotos, mehrere Adressbücher und Konten in einer Liste.
- Bearbeiten im Kontakt selbst, ohne eigenen Dialog; jedes Feld mit Typ (privat, Arbeit, mobil …).
- Offline lesen und bearbeiten, Konflikte werden gezeigt, nie still überschrieben.
- Dubletten erkennen und zusammenführen, vCard importieren und exportieren (Stufe KO3).

## Was CardDAV und Nextcloud bieten

**CardDAV** (RFC 6352) legt jeden Kontakt als vCard-Datei in eine Sammlung (Adressbuch). Der Weg ist
derselbe wie bei CalDAV, und vieles davon steht schon in `torro-pim-caldav`:
- **Entdecken:** `/.well-known/carddav` → `current-user-principal` → `addressbook-home-set`. Nie Pfade raten.
- **Abgleich:** `sync-collection` (RFC 6578) mit Sync-Token. Server ohne sync-collection: `getetag`
  aller Einträge vergleichen und nur geänderte holen (`addressbook-multiget`).
- **Schreiben:** neu mit `If-None-Match: *`, ändern und löschen mit `If-Match: <etag>`. 412 heißt:
  jemand war schneller. Dann zusammenführen (siehe Offline), nie überschreiben.
- **Rechte je Adressbuch:** `current-user-privilege-set`; ohne `write` ist das Buch schreibgeschützt.

**Nextcloud** (`/remote.php/dav/addressbooks/users/<uid>/`):

| Adressbuch | Was | Wie wir es behandeln |
|---|---|---|
| `contacts` (und weitere eigene) | die Kontakte des Nutzers | lesen und schreiben |
| geteilte Bücher anderer | je nach Freigabe | Schreibrecht aus `current-user-privilege-set` |
| `z-server-generated--system` | alle Nutzer der Instanz | nur lesen, ab Werk ausgeblendet |
| `z-app-generated--contactsinteraction--recent` | „Recently contacted“ der App Contacts Interaction | nur lesen, ausgeblendet; speist Mail-Vorschläge (KO3) |

Eigenheiten, auf die wir achten:
- **vCard 3.0 und 4.0:** Nextcloud speichert, was der Client schickt, meist 3.0. Wir lesen beide und
  schreiben in der Fassung, in der der Kontakt liegt.
- **Gruppen:** Nextcloud Contacts nutzt `CATEGORIES`. iCloud nutzt eigene Gruppen-vCards
  (`X-ADDRESSBOOKSERVER-KIND:group` mit `X-ADDRESSBOOKSERVER-MEMBER`). Beides wird gelesen;
  geschrieben wird im Stil des Servers (Entscheidung 6).
- **Fotos** stehen meist eingebettet (`PHOTO`, Base64) in der vCard. Der Kern hält verkleinerte
  Vorschaubilder im Cache, damit die Liste nicht Megabytes über den Socket schiebt.
- **Geburtstage:** Nextcloud legt selbst den Kalender „Contact birthdays“ an, der erscheint schon in
  `torrocalendar`. Die App zeigt zusätzlich „Upcoming birthdays“ aus dem eigenen Cache, für alle Server.
- **Unbekannte Felder bleiben erhalten.** Andere Clients schreiben Eigenschaften, die wir nicht
  kennen (`X-…`, `IMPP`, `RELATED` …). Der Kern ändert nur die Felder, die bearbeitet wurden, und
  lässt den Rest der vCard Byte für Byte stehen. Das ist die wichtigste Regel des ganzen Plans.

## Karten ohne Lizenzschlüssel

Recherche vom 2026-10-15 (Quellen unter der Tabelle):

| Baustein | Dienst | Bedingungen |
|---|---|---|
| Kacheln | `tile.openstreetmap.org` | erlaubt für Desktop-Apps: eigener User-Agent mit Kontakt, Cache-Header beachten (sonst mindestens 7 Tage), nur der sichtbare Ausschnitt, kein Vorab-Laden, kein „offline speichern“, sichtbare Nennung „© OpenStreetMap contributors“. URL einstellbar, nicht fest im Code. Keine Garantie. |
| Kacheln, Alternativen | OpenTopoMap (Raster, weltweit), basemap.de (BKG, Raster, nur Deutschland, CC BY 4.0) | ohne Schlüssel, eigene Nennung. Wikimedia ist für Dritte gesperrt. |
| Ortssuche (Adresse → Koordinaten) | Nominatim | höchstens 1 Anfrage pro Sekunde, eigener User-Agent, keine Vorschläge beim Tippen, Ergebnisse cachen. Ein paar hundert Adressen einmal umwandeln und speichern ist vertretbar. |
| Übersicht offline | Protomaps-PMTiles | Welt bis Zoom 6 etwa 60 MB, ODbL. Kein Netz nötig. |
| „In Karten öffnen“ | Apple Maps (`https://maps.apple.com/…`), `geo:` (Linux, wenn ein Programm dafür da ist), sonst `https://www.openstreetmap.org/search?query=…` | ohne Schlüssel. Windows-Karten wurde 2025 eingestellt, dort gilt die OSM-Adresse. |

Quellen: [OSM-Kachelregeln](https://operations.osmfoundation.org/policies/tiles/),
[Nominatim-Regeln](https://operations.osmfoundation.org/policies/nominatim/),
[OpenTopoMap](https://opentopomap.org/about),
[basemap.de-Nutzungsbedingungen](https://sgx.geodatenzentrum.de/public/gdz/lizenz/deu/nutzungsbedingungen_basemapde.pdf),
[Protomaps-Downloads](https://docs.protomaps.com/basemaps/downloads),
[Apple Map Links](https://developer.apple.com/documentation/mapkit/unified-map-urls),
[Nextcloud Maps](https://github.com/nextcloud/maps) (geocodiert selbst serverseitig über Nominatim).

**Ein fertiges Karten-Widget für Iced gibt es nicht.** Wir bauen ein kleines selbst nach dem Muster
von `walkers` (egui): Kachel-Mathematik, Laden mit Cache, Widget aus `canvas` und Bildern.

**Wer was erfährt** (darum die Entscheidungen 7 und 8):
- **Kachelserver:** IP-Adresse und die angezeigten Kacheln. Bei hohem Zoom grenzt das den Ort auf
  einige hundert Meter ein. Der Cache verringert das.
- **Nominatim:** die **ganze Adresse im Klartext** und die IP. Das ist der heikelste Abfluss.
- **„In Karten öffnen“:** Apple oder OSM bekommen genau diese eine Adresse, ausgelöst durch einen Klick.
- **PMTiles offline:** nichts.

## Entscheidungen (2026-10-15)

1. **Name:** App `torrocontacts`, Bereich `contacts`, MCP-Tools `contacts_*`, Fensterklasse
   `torro-contacts`. Karten-Befehle im Bereich `maps`, MCP-Tools `maps_*`.
2. **Quelle:** CardDAV, Nextcloud zuerst, dann jeder CardDAV-Server. Google und Microsoft bleiben
   draußen, weil sie registrierte OAuth-Client-IDs brauchen. Ein Adressbuch nur auf dem Rechner ist
   nicht Teil dieses Plans.
3. **Crates:**
   - `torro-contacts` (neu): neutrales Modell, vCard lesen und schreiben (mit Erhalt unbekannter
     Felder), Anbieter-Trait, Zusammenführen, Dubletten-Regeln, Demo. Ohne Ein-/Ausgabe, Vorbild
     `torro-pim`.
   - CardDAV kommt als `carddav.rs` in das bestehende `torro-pim-caldav`. Entdeckung, Anmeldung,
     HTTP-Client und DNS-Schutz liegen dort schon. Ein eigenes DAV-Crate wäre nur Umbau.
   - Das Karten-Widget kommt nach `torro-gui` (`map.rs`), weil später auch der Kalender Orte zeigt.
   - Der Kern-Teil kommt nach `crates/torroworkplace/src/contacts/` und `…/src/maps/`.
   - `torrocontacts` (neu) ist die App.
4. **Kontaktfelder sind immer `untrusted: true`.** In Adressbüchern steht Text von anderen: geteilte
   Bücher, das Systemadressbuch, vCards aus Mails. Ein Feld „Notiz“ mit einer eingeschleusten
   Anweisung bliebe sonst dauerhaft wirksam. Katalogbeschreibungen und MCP-Tools sagen das, wie bei
   Mail, Eingang und Notizen.
5. **Rechte je Adressbuch**, wie heute je Kalender: `Rights.address_books: Selection`, fehlt in alten
   Rechten = nichts.
   - **Die Rechte-Vorlagen öffnen kein Adressbuch.** Sie setzen nur die Stufe in `contacts`. Beim
     Verbinden eines Agenten sind die bestehenden Bücher vorgewählt, und der Mensch bestätigt sie.
   - **Was ein Client nicht sehen darf, gibt es für ihn nicht:** Kontakte aus anderen Büchern sind
     `not-found` und werden vor dem Lesen ausgefiltert, auch bei Suche, Zählern, Geburtstagen,
     Dubletten und Mail-Vorschlägen.
   - **Ausgeblendete Bücher** (Systemadressbuch, „Recently contacted“) erreicht ein Client nur, wenn
     sie einzeln in seiner Auswahl stehen, auch bei `All`. Das gilt auch für Mail-Vorschläge.
   - **Verschieben** zwischen Büchern braucht Quelle und Ziel.
   - **Zusammenführen** braucht alle beteiligten Bücher.
   - **Der Assistent darf ohne Rückfrage lesen, suchen und anlegen. Ändern und Löschen wartet auf
     eine Freigabe.** Umgesetzt mit den vorhandenen Mitteln:
     - `contacts.update_contact` überschreibt Felder des Menschen und ist `Delete`.
     - `contacts.add_to_contact` fügt nur hinzu (eine Nummer, eine Adresse, eine Zeile Notiz) und ist
       `Write`. So kann ein Agent ergänzen, ohne eine Freigabe zu brauchen. Ergänzte Werte kommen
       **hinter** die bestehenden und bekommen nie `pref`. Sonst könnte ein Agent bei „Bank“ still eine
       fremde Mail-Adresse zur ersten machen.
     - Gruppen: `add_to_group` ist `Write`, `remove_from_group` ist `Delete`.
     - `Rights::assistant()` bekommt `contacts` mit Stufe Voll, `confirm: OnDelete` (wie bisher) und
       eine leere Auswahl. In „What the assistant may use“ wählt der Mensch die Bücher.
   - `Area::Contacts` kommt **nicht** in `NEVER_TO_AGENTS`. Was ein Agent sieht, regelt allein die
     Auswahl. Wer ein Buch nicht bei einem KI-Anbieter haben will, lässt es dort weg.
6. **vCard schreiben:** Der Kern ändert nur bearbeitete Eigenschaften, alles andere bleibt, wie es
   war. Neue Kontakte bekommen die vCard-Fassung, die im Buch überwiegt (sonst 3.0, die versteht
   jeder Server). Gruppen werden im Stil des Servers geschrieben: `CATEGORIES`, oder bei iCloud die
   Gruppen-vCard.
7. **Karte stufenweise:**
   - **KO2:** „In Karten öffnen“ und eine kleine Karte im Kontakt.
   - **KO4:** eine Karte aller Kontakte, die Übersicht offline aus mitgelieferten PMTiles, die Details online.
   - Kacheln holt **der Kern**, nie die App (`maps.get_tile`). Er cacht sie auf der Platte nach den
     Regeln des Servers, sendet den User-Agent `Torro Workplace/<version> (+<projekt-url>)` und zeigt
     die Nennung. Der Kachelserver ist einstellbar: OpenStreetMap (Standard), OpenTopoMap,
     basemap.de oder eine eigene URL.
8. **Ortssuche nur mit Zustimmung:**
   - **Ab Werk aus.** Eine Einstellung „Look up addresses with OpenStreetMap (Nominatim)“ schaltet sie ein.
     Ein Text erklärt, dass dabei Adressen an die OpenStreetMap Foundation gehen.
   - **Eingeschaltet:** Der Kern wandelt Adressen nacheinander um, höchstens eine pro Sekunde, nur
     aus Büchern, die in der App sichtbar sind. Das Systemadressbuch (alle Nutzer der Instanz) geht
     also nur an die OSMF, wenn der Mensch ausgeblendete Bücher einblendet. Er
     speichert das Ergebnis lokal, verknüpft mit einer Prüfsumme der Adresse. Ändert sich die
     Adresse, wird neu gesucht.
   - **`GEO` aus der vCard wird gelesen und hat Vorrang.**
   - **Zurückgeschrieben wird `GEO` nie still.** Es landet nur auf dem Server, wenn der Mensch einen
     Pin von Hand setzt (KO4).
   - **Einschalten darf nur ein Mensch.** `maps.geocoding` auf „an“ ist wie das Mikrofon für Menschen
     vorbehalten (`Request::kept_for_people`); ausschalten darf jeder mit Admin-Recht.
   - **Ins Netz gehen nur der Kern im Hintergrund und die Apps** (App-Schlüssel). Für Clients
     antworten `contacts.locate_contact` und `maps.get_tile` nur aus `GEO` und Cache. Der Kern
     nimmt keinen Freitext an, nur Adressfelder gespeicherter Kontakte.
   - **Restweg, ehrlich benannt:** Ein Agent mit Schreibrecht kann einen Kontakt mit erfundener
     „Adresse“ anlegen, die der Hintergrund dann sucht. Das ist ein langsamer Kanal (1/s, nur
     Adressfelder, höchstens 200 Zeichen) zu einem festen Empfänger, den der Mensch eingeschaltet
     hat. Wir nehmen ihn hin. Eine eigene Kachel-URL kostet eine Admin-Freigabe, die die URL zeigt.
9. **Offline bearbeiten** wie bei Notizen: SQLite mit Basisfassung je Kontakt und Warteschlange.
   Auf 412 folgt eine Dreiwege-Zusammenführung **je Eigenschaft**. Mehrfachfelder (Mail, Telefon,
   Adresse) werden als Mengen zusammengeführt. Echte Konflikte entscheidet der Mensch.
10. **Stufen:**
    - **KO1:** Adressbuch mit Kern, Abgleich, API, MCP und App.
    - **KO2:** Karte im Kontakt.
    - **KO3:** Querverbindungen und Pflege: Mail-Vorschläge, Absender speichern, vCard importieren
      und exportieren, Dubletten.
    - **KO4:** Karte aller Kontakte.

    Jede Stufe ist für sich nutzbar und über alle drei Wege erreichbar. KO3 ersetzt die Aufgabe
    M7.6 aus [tasks/mail.md](tasks/mail.md).

## Aufbau

```
torrocontacts (App)  ── IPC ──▶  torroworkplace (Kern)
  └─ torro_gui::map (Widget)      ├─ contacts/service.rs  Befehle, Rechte-Filter, Freigabe-Fakten
                                  ├─ contacts/store.rs    SQLite: vCards, Basis, Warteschlange, FTS5, Fotos
                                  ├─ contacts/sync.rs     Abgleich je Buch, Zusammenführen bei 412
                                  ├─ maps/tiles.rs        Kacheln laden, Platten-Cache, Server-Regeln
                                  └─ maps/geocode.rs      Nominatim-Warteschlange, 1/s, Ergebnis-Cache
                                           │
                        torro_contacts::ContactsProvider (Trait, Modell, vCard, merge)
                           └─ torro-pim-caldav/carddav.rs   CardDAV (Nextcloud, iCloud, Fastmail …)
```

**Kennungen:**
- **Adressbuch:** `<konto-id>/<name>`, zum Beispiel `nextcloud:anna@cloud.example/contacts`. Der Name
  ist das letzte Pfadsegment der Sammlung, getrennt am ersten `/` wie die Kalender.
- **Kontakt:** `<buch-id>/<uid>`. Die `UID` der vCard ist stabil und vom Dateinamen unabhängig. Den
  Pfad (`href`) merkt sich der Kern. Ein offline angelegter Kontakt bekommt sofort eine echte UUID als
  `UID`, deshalb muss sich die Kennung später nicht ändern.

**Abgleich:** beim Start, alle 15 Minuten, nach `accounts.sync` und nach jedem Schreiben. Später
kann `notify_push` einen Abgleich sofort auslösen. Ob ein Konto Kontakte hat, steht in
`Services::contacts` (neues Feld) und in `accounts.list_accounts`.

**Fehler** (feste Codes, wie bei Kalender und Notizen): `offline` (Änderungen bleiben in der
Warteschlange), `auth`, `conflict`, `unsupported` („This address book is read-only.“), `not-found`,
`error`. Die Ortssuche ohne Zustimmung antwortet mit der Antwortart `no_consent` und dem Hinweis auf die
Einstellung, nicht mit `forbidden`: Das hieße für Agenten „Rechte fehlen“.

**Sicherheit:**
- **Kein Inhalt im Protokoll:** nur Befehl, Konto und Prüfsummen der Kennungen (`audit::ids`). Namen,
  Adressen, Nummern, Suchbegriffe und Koordinaten stehen dort nie. Datenbank und Foto-Cache
  sind 0600 und verschwinden mit dem Konto oder bei einem Remote Wipe. Der Kachel-Cache (0600) ist an
  kein Konto gebunden, verrät aber, wo Kontakte wohnen. Er geht bei jedem Remote Wipe und per „Clear“
  in den Einstellungen, nicht beim Entfernen eines Kontos. Die gefundenen Orte liegen im
  Kontakt-Speicher und gehen mit dem Konto.
- **Freigaben** zeigen, was wirklich geschieht, aus dem Cache des Kerns über die Kennung
  (`approvals::Facts`), für **jeden** schreibenden Befehl, weil „Freigabe bei jeder Änderung“ auch
  `Write` anhält: Name, Buch, was dazukommt, je geändertes Feld „alt → neu“, Quelle und Ziel beim
  Verschieben, Zahl und Buch beim Import, beim Zusammenführen welche Kontakte wegfallen. Bei
  `maps.tile_url` die URL, bei `maps.geocoding` „Address lookup off“ (einschalten kann ein Agent sie nicht).
- **Fotos:** höchstens 5 MB beim Hochladen, nur PNG und JPEG, der Kern rechnet sie auf 512 px herunter
  und entfernt Metadaten (EXIF mit Ort).
- **Links in Kontakten** (`URL`, `IMPP`, `SOCIALPROFILE`) öffnen sich nur nach einem Klick und nur
  mit `https`, `mailto`, `tel`, `sms`, `xmpp`, `matrix`. Andere Schemata werden als Text gezeigt.
- **Import:** eine vCard-Datei höchstens 20 MB und 10.000 Kontakte. Der Import zeigt vorher, was
  entsteht und was eine Dublette wäre.

## Befehle

Bereich `contacts`:

| Stufe | Befehl | Zugriff | MCP |
|---|---|---|---|
| KO1 | `contacts.list_address_books { account? }` (mit Zahlen, Schreibrecht) | Read | `contacts_list_address_books` |
| KO1 | `contacts.list_contacts { book?, group?, favorites?, recent?, query?, limit?, cursor? }` (Zusammenfassung, Vorschaubild-Kennung) | Read | `contacts_list_contacts` |
| KO1 | `contacts.get_contact { id }` (alle Felder, `etag`; bei `conflict` auch die Serverfassung) | Read | `contacts_get_contact` |
| KO1 | `contacts.get_photo { id, size }` | Read | `contacts_get_photo` |
| KO1 | `contacts.list_groups { book? }` | Read | `contacts_list_groups` |
| KO1 | `contacts.upcoming_birthdays { days? }` | Read | `contacts_upcoming_birthdays` |
| KO1 | `contacts.create_contact { book, fields }` | Write | `contacts_create_contact` |
| KO1 | `contacts.add_to_contact { id, fields }` (nur hinzufügen) | Write | `contacts_add_to_contact` |
| KO1 | `contacts.update_contact { id, etag, fields }` | **Delete** | `contacts_update_contact` |
| KO1 | `contacts.set_favorite { id, favorite }` | Write | `contacts_set_favorite` |
| KO1 | `contacts.add_to_group { id, etag, group }` | Write | `contacts_add_to_group` |
| KO1 | `contacts.remove_from_group { id, etag, group }` | **Delete** | `contacts_remove_from_group` |
| KO1 | `contacts.mark_used { id }` (für „Recent“, nur lokal) | Write | `contacts_mark_used` |
| KO1 | `contacts.set_photo { id, etag, data? }` (`None` entfernt) | **Delete** | `contacts_set_photo` |
| KO1 | `contacts.move_contact { id, etag, to_book }` | Write | `contacts_move_contact` |
| KO1 | `contacts.delete_contact { id, etag }` | Delete | `contacts_delete_contact` |
| KO1 | `contacts.resolve_conflict { id, keep: mine\|theirs\|both, etag }` | Write, mit `mine`/`theirs` **Delete** | `contacts_resolve_conflict` |
| KO2 | `contacts.locate_contact { id, address: usize }` (Antwort `found \| not_found \| queued \| no_consent`; Netz nur für den App-Schlüssel) | Read | `contacts_locate_contact` |
| KO2 | `contacts.open_in_maps { id, address: usize }` (öffnet die Karten-App oder den Browser) | Write | `contacts_open_in_maps` |
| KO3 | `contacts.suggest_recipients { query, limit? }` (Mail-Adressen aus Kontakten und „Recently contacted“) | Read | `contacts_suggest_recipients` |
| KO3 | `contacts.import_vcards { book, data, dry_run }` | Write | `contacts_import_vcards` |
| KO3 | `contacts.export_vcards { book?, ids? }` | Read | `contacts_export_vcards` |
| KO3 | `contacts.find_duplicates { book? }` | Read | `contacts_find_duplicates` |
| KO3 | `contacts.merge_contacts { ids, etags, keep }` | **Delete** | `contacts_merge_contacts` |
| KO3 | `inbox.create_contact_from_item { id, book }` (Absender, vCard-Anhang) | Write (`inbox`) + Write in `contacts` | `inbox_create_contact_from_item` |
| KO4 | `contacts.list_locations { book?, group?, bbox? }` (Pins: Kennung, Name, Koordinaten) | Read | `contacts_list_locations` |
| KO4 | `contacts.set_location { id, etag, address: usize, lat, lon, replace }` (Pin von Hand, schreibt `GEO`) | Write, mit `replace` **Delete** | `contacts_set_location` |

Bereich `maps` (neu):

| Stufe | Befehl | Zugriff | MCP |
|---|---|---|---|
| KO2 | `maps.get_tile { z, x, y }` (PNG aus dem Cache oder vom eingestellten Server; für Clients nur aus dem Cache) | Read | `maps_get_tile` |
| KO2 | `maps.get_status` (Server, Nennung, Ortssuche an/aus mit Fortschritt, Größe des Caches) | Read | `maps_get_status` |
| KO2 | `maps.clear_cache` | Write | `maps_clear_cache` |
| KO4 | `maps.get_offline_tile { z, x, y }` (aus den mitgelieferten PMTiles) | Read | `maps_get_offline_tile` |

Einstellungen über das vorhandene `settings.set_setting` (Admin, für Agenten also mit Freigabe):
`maps.tile_server` (`osm`, `opentopomap`, `basemap-de`, `custom`), `maps.tile_url` (bei `custom`),
`maps.geocoding` (aus/an; „an“ nur für Menschen), `contacts.show_hidden_books` (aus/an, nur die Anzeige
in der App, die Rechte bleiben).

Erläuterungen:
- **Kein Befehl steht auf `Never`.** Einzig das Einschalten der Ortssuche (`maps.geocoding = true`)
  ist nach seinem Wert Menschen vorbehalten, wie das Mikrofon. Das wird ein eigener Punkt in
  [„Was nie nach außen geht“](api.md#was-nie-nach-außen-geht) (KO2.1).
- **`update_contact` ist `Delete`,** weil es Felder des Menschen überschreibt; `add_to_contact` nicht.
  Was ein Agent ergänzen will, geht ohne Freigabe. Wer etwas ändern will, wartet.
- **`set_location` ist `Write`,** solange es kein `GEO` ersetzt. Mit `replace: true` ist es `Delete`
  (`Request::operation()` sieht nur den Request). Ohne `replace` und mit vorhandenem `GEO` antwortet
  der Kern mit `conflict`.
- **`locate_contact` ist `Read`:** Für Clients liest es nur `GEO` und den Cache. Nominatim fragt nur
  der Kern selbst, im Hintergrund oder für die App, und nur mit Zustimmung.
- **Kein `locate_all`:** Mit Zustimmung sucht der Hintergrund alle Adressen von selbst. Ausschalten
  hält ihn an, `maps.get_status` zeigt den Fortschritt.
- **`open_in_maps` ist `Write`** wie `apps.open_app`: Es öffnet etwas auf dem Bildschirm des Menschen.
- **`maps_get_tile` für Agenten:** Ein Agent kann mit einem Kachelbild wenig anfangen. Es gibt das
  Tool trotzdem, weil die Regel „jede Funktion dreimal“ gilt. Es liest nur den Cache, damit ein
  Agent über frei gewählte Kachelnummern nichts nach draußen trägt.

## Oberfläche

**`torrocontacts`** nutzt `torro_gui::shell`:
- **Seitenleiste:**
  - **All contacts**, **Favorites**, **Recent**, **Birthdays** und **Map** (ab KO4).
  - Darunter die Adressbücher mit ihren Gruppen, ab zwei Konten je Konto gruppiert.
  - Systemadressbuch und „Recently contacted“ sind ausgeblendet; ein Schalter in den Einstellungen zeigt sie.
- **Liste:** Foto oder Initialen, Name, Firma, ein Hauptkontakt. Alphabetische Sprungleiste.
  Suchfeld oben; `/` und `Ctrl+F` springen hinein.
- **Kontakt rechts:**
  - **Kopf:** großes Foto, Name, Firma und Position, darunter Schnellknöpfe Mail, Call, Map, Copy.
  - **Abschnitte:** Telefon, Mail, Adressen (je mit kleiner Karte, KO2), Web und Messenger,
    Geburtstag und Jahrestage, Notiz, Gruppen.
  - **„Edit“** schaltet alle Felder im Kontakt selbst um. „+ Add field“ fügt eine Art hinzu.
  - **Schmal** zeigt das Fenster entweder die Liste oder den Kontakt.
- **Mini-Kachel:** Suchfeld und Favoriten mit Schnellknöpfen.
- **Steuerzentrale:**
  - Kontoseite mit Dienst „Contacts“.
  - Rechte-Zeile `contacts` mit Adressbuch-Auswahl.
  - Abschnitt „Maps“: Kachelserver, Ortssuche an/aus mit Erklärung, Cache-Größe und „Clear“.
- **Eingang und Mail (KO3):** Aktion „Save as contact“ bei Mails und vCard-Anhängen,
  Empfänger-Vorschläge im Schreibfenster.

**Karte im Kontakt (KO2):** ein fester Ausschnitt (Zoom 15, 3×3 Kacheln) mit Pin, unten rechts
„© OpenStreetMap contributors“. Ein Klick öffnet die Karten-App. Ohne Koordinaten und ohne
Zustimmung zeigt die Stelle nur „Open in Maps“ und den Hinweis „Turn on address lookup to see a
map“, mit Link zur Einstellung.

**Karte aller Kontakte (KO4):** verschiebbar und zoombar. Pins werden zu Gruppen zusammengefasst,
ein Klick öffnet den Kontakt. Bis Zoom 6 kommt die Karte offline aus PMTiles, darüber vom
Kachelserver. Gibt es keine Zustimmung zur Ortssuche, zeigt die Karte nur Kontakte mit `GEO` und
sagt, wie viele fehlen. Mit Zustimmung zeigt sie den Fortschritt der Suche im Hintergrund.

## Offen (in der jeweiligen Aufgabe klären)

- **vCard-Bibliothek:** `calcard` (Stalwart, vCard 3/4 und iCalendar) oder ein eigener kleiner
  Leser. Bedingung: unbekannte Eigenschaften und Parameter überleben eine Rundreise unverändert.
  Entscheidet KO1.1a nach einem Test mit echten vCards von Nextcloud, iCloud und Google.
- **iCloud-Gruppen schreiben** braucht eine zweite Datei je Gruppe. Gegen einen echten Server prüfen,
  sonst zunächst nur lesen.
- **PMTiles ins Paket?** 60 MB machen den Installer deutlich größer. Alternativ lädt die App die
  Datei einmal nach Zustimmung. Das klärt KO4.1 mit dem Release-Plan ([release.md](release.md)).
- **Vektorkacheln** (OpenFreeMap, OSMF Shortbread) sähen schärfer aus. Dafür bräuchte es aber einen
  eigenen MVT-Renderer in Iced. Das ist nicht Teil dieses Plans.
- **Orte im Kalender:** `LOCATION` von Terminen auf derselben Karte und mit „In Karten öffnen“ liegt
  nahe. Das wird ein eigener kleiner Plan, sobald das Widget steht.
