# Erinnerungen: Termine, die man nicht verpassen kann

Stand: 2026-10-15. **Konzept, Entscheidungen vom 2026-10-15 eingearbeitet.** Aufgaben: [tasks/erinnerungen.md](tasks/erinnerungen.md).
Screens: [prototypes/calendar/erinnerung.html](prototypes/calendar/erinnerung.html) (Bilder daneben,
`erinnerung-*.png`).

Beantwortet die Frage: **Was fängt gleich an, und was brauche ich dafür?** Eine Erinnerung zeigt einen
Termin rechtzeitig an, als leise Benachrichtigung oder, wenn man das will, **auf dem ganzen Bildschirm**,
mit allem, was man in dem Moment braucht: Beginn, Ort, Link zum Videocall, Teilnehmer, Notizen.

## Vorbild: In Your Face

[In Your Face](https://apps.apple.com/us/app/meeting-reminder-in-your-face/id1476964367) (Martin
Höller, Mac/iOS) ersetzt die kleinen Kalender-Benachrichtigungen, die man im Arbeitsfluss übersieht,
durch einen Hinweis über den ganzen Bildschirm. Recherchiert am 2026-10-15 (App Store, Macworld, iMore,
TidBITS):

| Was | Wie In Your Face es macht | Für Torro |
|---|---|---|
| Vollbild | Überdeckt alle Bildschirme, bleibt, bis man klickt; Standard 1 Minute vor Beginn | übernehmen, Vorlauf einstellbar |
| Inhalt | „Don't forget your appointment!“, darunter Titel, Zeit, Ort; Notizen hinter einem Knopf, mit Links | übernehmen, aber Titel statt Floskel und mehr Inhalt (Teilnehmer, Kalender, was danach kommt) |
| Videocall | erkennt Links von über 60 Diensten, Beitreten mit einem Klick aus dem Hinweis | übernehmen: `torro_gui::links::meeting` kennt Zoom, Teams, Meet, Webex, Jitsi, Talk |
| Schlummern | Knöpfe unten: 1 oder 5 Minuten, „bis zum Beginn“ | übernehmen, dazu 10 Minuten |
| Je Termin | „Customize This Event“: ausnehmen oder eigene Zeit | übernehmen, aber gespeichert im Termin (`VALARM`), nicht nur in der App |
| Mehrere Hinweise | seit 1.7 mehrere je Termin (früh ein Vorab-Hinweis, kurz vorher der letzte) | übernehmen: mehrere `VALARM`, jede mit eigener Art |
| Filter | Kalender wählen, „nur zugesagte“, ganztägige aus, Arbeitszeit, Ausnahmeregeln, Pause | übernehmen; Arbeitszeit sobald K.33 da ist |
| Reisezeit | Hinweis rückt nach vorn, wenn der Termin eine Reisezeit hat | später (Apple-Feld `X-APPLE-TRAVEL-DURATION`) |
| Menüleiste | Übersicht der nächsten Termine, verworfene fehlen dort | Tray: „Next: … in 12 min“ und „Pause reminders“ |
| Töne | Auswahl, auch ohne Ton | übernehmen, Standard leiser Ton |
| Eigene Erinnerungen | ohne Kalendertermin, nur in der App gespeichert | bei uns sind das Aufgaben mit Fälligkeit (später, ER4) |
| Kritik | Kalenderliste bei vielen Kalendern unhandlich; beim Bildschirm-Teilen sehen andere den Hinweis | Kalender in der Steuerzentrale als Gruppe; beim Teilen leise (offene Frage 3) |

Was wir besser machen wollen:
- **Gespeichert am Termin.** Eigene Erinnerungen eines Termins sind `VALARM` im Kalender, kommen also auch
  auf Handy und Nextcloud an. Nur „ganzer Bildschirm ja/nein“ ist Torro-eigen (`X-TORRO-STYLE`).
- **Ein Ort für alles.** Jede Erinnerung ist auch ein Eintrag im Eingang („Erinnerung aus Kalender“, schon in
  [eingang.md](eingang.md) vorgesehen): ✕ · Open · In 5 min. Der Vollbild-Hinweis ist dieselbe Erinnerung,
  nur lauter.
- **Drei Wege** wie alles in der Suite: Oberfläche, API, MCP. Ein Agent kann fragen „Was steht als Nächstes
  an?“, eine Erinnerung schlummern lassen oder Erinnerungen während einer Präsentation pausieren.

## Was der Vollbild-Hinweis zeigt

Von oben nach unten, alles in einer Spalte (Lesbreite `size::content_max()`, mittig):

1. **Kopfzeile** leise: Kalender (Punkt in seiner Farbe und Name), Art des Termins („Video call · Zoom“),
   rechts die Uhrzeit groß genug, um sie aus der Distanz zu lesen.
2. **Titel** sehr groß (neue Stufe `typo::hero()`, 48/56, 600), bis zwei Zeilen, sonst „…“.
3. **Countdown** „Starts in 4:32“, sekundengenau, tabellarisch. Nach dem Beginn „Started 2 min ago“ mit
   Uhr-Icon in Attention-Orange, nie Rot.
4. **Zeit** „10:00 – 10:45 · 45 min · Thu, 15 Oct“, bei Serien das `Repeat`-Icon.
5. **Knöpfe**: „Join Zoom“ (Primärknopf, rot, nur wenn ein Link erkannt ist; sonst „Open in calendar“ als
   `secondary`), „Snooze ▾“ (Menü: 1 min, 5 min, 10 min, Until it starts), „Dismiss“ (`ghost`). Tasten
   stehen als `kbd` daneben: `Enter`, `S`, `Esc`.
6. **Eigenschaften** wie im Termin-Paneel: Ort (mit „Open in Maps“), Teilnehmer (Avatare, Zusagen; braucht
   K.26), Organisator, Links (aus Ort und Notizen, Videocall zuerst), Notizen (drei Zeilen, „Show all“).
7. **Fußzeile**: „Next: 11:30 Lunch with Ben“ und, wenn gleichzeitig mehrere fällig sind, „Also now:
   Deploy window“ mit Umschalten; „Dismiss all“ nur dann.

**Größen** (logische Pixel des Bildschirms, nicht des Fensters):
- **Normal** (bis 1599 breit): eine Spalte wie oben, Notizen eingeklappt.
- **Breit** (ab 1600 breit, z. B. 1920×1080 und 32:9 mit 3840×1080): zwei Spalten. Links Kopf, Titel,
  Countdown, Knöpfe; rechts Eigenschaften und die Notizen offen. Beide Spalten zusammen höchstens 1280
  breit und mittig: Groß heißt nicht gestreckt.
- **Andere Bildschirme**: eine ruhige Fassung mit Kalender, Titel, Countdown und den drei Knöpfen. Ein Klick
  dort wirkt wie auf dem Hauptbildschirm, alle Fenster gehen zusammen.
- **Klein** (unter 1280 breit oder 760 hoch, etwa ein kleines Laptop oder ein Beamer mit 1280×720): Eigenschaften als Chips
  unter dem Titel, Notizen nur über „Show notes“.

**Aussehen**: Fläche `BG` über den ganzen Bildschirm, oben ein weicher Schein in der Kalenderfarbe
(Tönung 18 %, hell 14 %, wie bei Terminen, nie volle Farbe als Fläche). Rot nur für „Join“. Hell und
dunkel wie die Suite. Keine Animation außer dem Countdown; mit „Reduce motion“ ohne Einblenden.

**Bedienung**: `Enter` tritt bei (oder öffnet den Termin), `S` öffnet das Schlummer-Menü, darin `1`, `5`,
`0` (10 min) und `B` (bis zum Beginn), `Esc` verwirft, `Tab` geht durch alle Knöpfe, `←`/`→` wechseln
zwischen gleichzeitigen Erinnerungen. Der Hinweis nimmt den Fokus nur einmal beim Erscheinen; wer gerade
tippt, verliert keinen Tastendruck an „Dismiss“: Die Tasten wirken erst nach 600 ms.

**Screens** im Entwurf (`erinnerung.html?screen=…`, Bilder `prototypes/calendar/erinnerung-*.png`):

| Bild | Zeigt |
|---|---|
| `wide`, `wide-light` | 1920×1080, zwei Spalten, dunkel und hell |
| `ultrawide` | 3840×1080 (32:9): gleich, mittig, nicht gestreckt |
| `snooze` | 1440×900, eine Spalte, Schlummer-Menü offen |
| `small` | 1280×720, Eigenschaften als Chips |
| `started` | nach Beginn: „Started 2 min ago“, „Join now“ |
| `many` | zwei Termine zugleich, „Also now“, „Dismiss all“ |
| `onsite` | vor Ort ohne Videolink: „Dismiss“ ist die Hauptaktion, „Open in Maps“ |
| `second` | ruhige Fassung für die anderen Bildschirme |
| `editor` | Termin-Paneel mit „Reminders“ und dem Menü „Add reminder“ |
| `settings` | Steuerzentrale › Calendar: Reminders, Which events, Calendars |
| `quiet` | leise Benachrichtigung und Tray mit „Next“ und „Pause reminders“ |

## Leise Benachrichtigung

Für Kalender oder Termine ohne Vollbild: die normale Desktop-Benachrichtigung der Suite (`notify.rs`) mit
„Weekly product sync · in 5 min“, Zeit und Ort, Knöpfe „Join“ (falls Link), „In 5 min“, „Dismiss“. Dazu
der Eintrag im Eingang. „Quiet during busy events“ (E3.4) hält Erinnerungen **nicht** zurück: Gerade
während eines Termins will man den nächsten nicht verpassen.

## Wann eine Erinnerung kommt

- **Erinnerungen des Termins** (`VALARM` mit `ACTION:DISPLAY` oder `AUDIO`, relativ zum Beginn oder Ende
  oder zu einer festen Zeit) gelten zuerst. `ACTION:EMAIL` ignoriert der Kern; das macht der Server.
- **Standard der Suite** für Termine ohne eigene Erinnerung: „1 min before“ (wie In Your Face),
  einstellbar, oder „None“.
- **Art je Erinnerung**: `X-TORRO-STYLE:FULLSCREEN` bzw. `NOTIFICATION` in der `VALARM`; fehlt es, gilt die
  Art des Kalenders (Off, Notification, Full screen; ohne Wahl Notification, Vollbild schaltet man je Kalender ein).
- **Filter** (Einstellungen): nur zugesagte Termine (sobald K.26 den Status liest; abgesagte und
  abgelehnte nie), ganztägige aus (Standard), freie (`TRANSP:TRANSPARENT`) aus, außerhalb der Arbeitszeit
  aus (nach K.33), ausgeblendete Kalender nie.
- **Pause**: „Pause reminders“ für 1 Stunde, bis morgen früh oder bis zum Fortsetzen. In der Pause fällige
  Erinnerungen werden leise Einträge im Eingang, kein Vollbild.

## Wie es gebaut ist

- **Der Kern plant** (`crates/torroworkplace/src/reminders.rs`, neu): Aus dem Zwischenspeicher der Termine
  (schon aufgelöste Wiederholungen) berechnet er die Erinnerungen der nächsten 36 Stunden, neu bei jedem
  Sync und jeder Änderung, sonst im Minutentakt. Eine fällige Erinnerung wird ausgelöst: Eingangseintrag,
  dann Benachrichtigung oder Vollbild. Nach dem Aufwachen aus dem Ruhezustand zeigt er verpasste
  Erinnerungen, deren Termin noch läuft („Started 3 min ago“); vorbei ist vorbei, nur der Eingang behält sie.
- **Zustand nur lokal** (`<state>/reminders.json`, 0600): verworfen, geschlummert bis, angezeigt. Er geht
  nicht auf den Server (kein `ACKNOWLEDGED` schreiben), weil jedes Verwerfen sonst den Termin ändern, das
  ETag verschieben und bei Serien den Hauptsatz anfassen würde (Entscheidung 1).
- **Kennung** einer Erinnerung: Prüfsumme aus Kalender, UID, Beginn des Vorkommens und Auslöser. Ändert sich
  die Zeit des Termins, ist es eine neue Erinnerung; eine verworfene kommt dann wieder.
- **Das Vollbild ist ein Fenster von `torrocalendar`** (`torrocalendar --reminders`, Fensterklasse
  `torro-reminder`), vom Kern gestartet wie `apps.open_app`. Es liest nur `calendar.list_reminders` und
  schickt `calendar.snooze_reminder` / `calendar.dismiss_reminder`; ist nichts mehr fällig, schließt es
  sich. Je Bildschirm ein Fenster: Hyprland über `torro-wm` (Regeln `fullscreen`, `pin`, Monitor), macOS
  über `torro-appkit` (Fenster über allen Spaces, auch über Vollbild-Apps), Windows randlos und oben.
  Wie das je System sicher geht, klärt zuerst ein Versuch (ER0.1).
- **Doppelte Hinweise von Nextcloud**: Die Nextcloud-App `dav` schickt eigene Erinnerungen als
  Benachrichtigung (Quelle Nextcloud im Eingang, Regel `dav` → „Für mich“). Sind Torro-Erinnerungen an,
  gehen diese im Torro-Eintrag desselben Termins auf, statt ein zweites Mal zu kommen.

## Oberfläche, API, MCP

| Funktion | Oberfläche | API (`calendar.*`) | Zugriff | MCP |
|---|---|---|---|---|
| Fällige und nächste Erinnerungen | Vollbild, Tray „Next“, Eingang | `calendar.list_reminders` | Read | `calendar_list_reminders` |
| Schlummern | Vollbild, Benachrichtigung, Eingang „In 5 min“ | `calendar.snooze_reminder` | Write | `calendar_snooze_reminder` |
| Verwerfen | Vollbild, Benachrichtigung, Eingang ✕ | `calendar.dismiss_reminder` | Write | `calendar_dismiss_reminder` |
| Pausieren / fortsetzen | Tray, Steuerzentrale | `calendar.pause_reminders` | Write | `calendar_pause_reminders` |
| Vorschau eines Termins | Termin-Paneel „…“ › Preview reminder, Steuerzentrale „Preview“ | `calendar.preview_reminder` | Write | `calendar_preview_reminder` |
| Erinnerungen eines Termins | Termin-Editor „Reminders“ | `calendar.save_event` (neues Feld `alarms`) | Write | `calendar_save_event` (kurz: `reminder_minutes`) |
| Einstellungen | Steuerzentrale › Calendar › Reminders | `settings.set_setting` (Schlüssel `reminders`) | Admin | `settings_set_setting` |

- Die Vorschau von einem Agenten zeigt oben „Preview · sent by {client}“ und geht höchstens einmal je
  Minute und Client (sonst `busy`), damit niemand den Bildschirm zuspammt.
- `list_reminders` liefert nur Erinnerungen von Kalendern, die der Client sehen darf (vor dem Lesen
  gefiltert); Titel, Ort und Notizen sind `untrusted`, Teilnehmer sind Adressen Dritter (wie bei
  `list_events`, K.26).
- Ins Protokoll kommen Kennung und Befehl, nie Titel oder Zeit.

## Entscheidungen (2026-10-15)

1. **Verwerfen nur lokal.** Verworfen und geschlummert steht nur in `<state>/reminders.json`, nichts geht
   als `ACKNOWLEDGED` (RFC 9074) auf den Server. Ein Schalter für den Server kann später kommen.
2. **Vollbild je Kalender.** Ohne Wahl ist ein Kalender „Notification“; Vollbild schaltet man je Kalender
   ein (z. B. „Work“) oder je Erinnerung im Termin.
3. **Während eines Videocalls nie Vollbild.** Schalter „Never full screen during a video call“ (Standard
   an): Läuft gerade ein Termin mit Videolink, kommt die Erinnerung als Benachrichtigung.
4. **Schnittstellen freigegeben:** `Event.alarms`, `Settings.reminders` und die fünf Requests
   `calendar.list_reminders`, `snooze_reminder`, `dismiss_reminder`, `pause_reminders`, `preview_reminder`.

Noch offen: **Ton** – eigenen leisen Ton mitliefern (Lizenz klären) oder den Systemton nehmen? Bis zur
Entscheidung nimmt ER1.4 den Systemton.

## Später

- Reisezeit (`X-APPLE-TRAVEL-DURATION`) und „Leave now“ mit Ort.
- Aufgaben mit Fälligkeit und `VALARM` (Nextcloud Tasks, Apple Erinnerungen) erinnern genauso (ER4).
- Ausnahmeregeln nach Titel („Focus time“ nie erinnern).
- Erinnerungen auf das Handy über ntfy weiterreichen.
