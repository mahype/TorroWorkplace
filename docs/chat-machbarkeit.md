# Chat auf Nextcloud Talk: Machbarkeit

Stand: 2026-11-06 (#470). Prüft, bevor gebaut wird, ob ein schlichter Chat wie ICQ mit Videoanrufen auf
Nextcloud Talk machbar ist, und mit welchem Aufwand. Das Konzept dazu: [chat.md](chat.md), die Entwürfe:
[prototypes/chat/](prototypes/chat/index.html).

**Wie geprüft:** nur gelesen, kein Versuchscode. Quelltext von Talk im Stand unseres Servers (Tag `v25.0.6` von
`nextcloud/spreed`), dazu `nextcloud-spreed-signaling`, `nextcloud-talk-recording`, `talk-desktop`, Teile von
`talk-android` und `talk-ios`, die Doku der Rust-Crates (Versionen über crates.io, Stand dieses Tages) und die
öffentlichen Capabilities des eigenen Servers (ohne Anmeldung). Was nur aus Erfahrung oder als Schluss folgt, ist
als *vermutet* markiert.

## Kurz

- **Text-Chat, Online-Status und Klingeln sind machbar** über die dokumentierte REST-API von Talk und die App User
  status. Das meiste davon kann `torro-nextcloud` heute schon halb (Unterhaltungen, Lesestand).
- **Videoanrufe sind nativ machbar, aber teuer:** etwa 11 bis 20 Personenwochen mit Googles libwebrtc (über die
  Crates von LiveKit), weil das Signaling von Talk ohne HPB nicht dokumentiert ist und Echo-Unterdrückung,
  Codecs, Kamera und Bild in Iced dazukommen. Mit webrtc-rs statt libwebrtc eher 18 bis 32, mit GStreamer 13 bis 21.
- **Eine eingebettete Webansicht scheidet unter Linux aus:** WebKitGTK kann WebRTC in den Paketen der
  Distributionen praktisch nicht, und unter Wayland ließe sie sich ohnehin nur als eigenes Fenster zeigen.
- **Den Anruf im Browser öffnen kostet fast nichts** (etwa eine halbe Personenwoche) und geht auf jedem System.
  Talk Desktop hat dafür keinen Link.
- **Empfehlung:** Stufe 5 öffnet Anrufe im Browser; Text, Status und Klingeln laufen in der Suite. Ein nativer
  Anruf wird ein eigener Versuch, wenn Text und Status stehen (Abschnitt [Empfehlung](#empfehlung)).
- **Entschieden (2026-11-06): Anrufe werden selbst gebaut**, gegen die Empfehlung; Stufe 5 beginnt mit einem Versuch
  (Abschnitt [Entscheidung](#entscheidung)).

## Der eigene Server

Öffentlich abgefragt (`GET https://cloud.imt24.de/ocs/v2.php/cloud/capabilities?format=json`, Header
`OCS-APIRequest: true`, ohne Anmeldung):

| Was | Wert |
|---|---|
| Nextcloud / Talk | 35.0.2 / 25.0.6 |
| Signaling | `config.signaling.mode: internal`, also **kein High-Performance-Backend (HPB)** |
| Anrufe | `config.call.enabled: true`, Features `audio`, `video`, `in-call-flags`, `start-call-flag`, `silent-call`, `send-call-notification`, `notification-calls`, `call-notification-state-api`, `signaling-v3`, `recording-consent`; `config.call.recording: false` |
| Chat | `chat-v2`, `chat-read-marker`, `chat-unread`, `chat-keep-notifications`, `chat-reference-id`, `typing-privacy`, `silent-send`, `edit-messages`, `delete-messages`, `reactions`, `media-caption`; `config.chat.max-length: 32000` |
| Nicht öffentlich | `user_status`, `notifications`, `notify_push` und STUN/TURN (`signaling/settings`) zeigt nur eine angemeldete Abfrage. Ob TURN eingerichtet ist, ist darum offen. |

Der zweite Server (`nextcloud.rheinschmiede.net`, Nextcloud 33.0.10) hat kein Talk; dort gäbe es nur den Status.

## Urteil je Funktion

Urteile: **machbar** (dokumentiert, überschaubar), **machbar mit Aufwand** (geht, kostet aber deutlich oder hängt an
Undokumentiertem), **nicht machbar** (auf einem der drei Systeme nicht mit vertretbarem Aufwand). Die letzte Spalte
sagt, was auf einem Server ohne HPB (wie dem eigenen) gilt.

| Funktion | Urteil | Begründung | Quelle | Ohne HPB |
|---|---|---|---|---|
| Unterhaltungen und Nachrichten lesen, senden, gelesen | **machbar** | Dokumentierte OCS-API; `rooms` und `mark_read` gibt es schon in `crates/torro-nextcloud/src/talk.rs`. Neue Nachrichten über Long Polling (`lookIntoFuture=1`, bis 60 s). | [Chat-API](https://nextcloud-talk.readthedocs.io/en/latest/chat/), [openapi.json](https://github.com/nextcloud/spreed/blob/v25.0.6/openapi.json) | gleich |
| Online-Status anderer | **machbar** | App User status: `/statuses` (seitenweise) oder einzeln `/statuses/{userId}`; kein Push, Abfrage im Minutentakt. Bei beschränkter Sichtbarkeit ist die Liste leer, dann einzeln. Zusätzlich liefert `GET /room?includeStatus=true` den Status in 1:1-Unterhaltungen. | [praesenz.md](praesenz.md), [Conversation-API](https://nextcloud-talk.readthedocs.io/en/latest/conversation/) | gleich |
| Eigener Status, Heartbeat | **machbar** | `PUT /user_status/status`, `…/message/custom`, `PUT /heartbeat` alle 5 Minuten. Achtung: Jede Abfrage der Raumliste ohne `noStatusUpdate=1` setzt „online“. | [HeartbeatController](https://github.com/nextcloud/server/blob/master/apps/user_status/lib/Controller/HeartbeatController.php) | gleich |
| Tipp-Anzeige | **machbar mit Aufwand** | Läuft über Signaling-Nachrichten `startedTyping`/`stoppedTyping`, gesteuert über `typing-privacy`. Der Web-Client sendet sie auch mit internem Signaling; dafür muss man der Unterhaltung mit einer Sitzung (Cookie) beitreten und je offenem Chat ein Long Polling auf `signaling/{token}` halten. Talk Android kann es nur mit HPB. Die Annahme aus dem Issue („nur mit HPB“) stimmt also nur für die Mobil-Apps. | [SignalingTypingHandler.js](https://github.com/nextcloud/spreed/blob/v25.0.6/src/utils/SignalingTypingHandler.js), [webrtc/index.js](https://github.com/nextcloud/spreed/blob/v25.0.6/src/utils/webrtc/index.js#L42) | geht, 1–2 s verzögert (*vermutet*) |
| Klingeln (eingehender Anruf) | **machbar** | Talk legt je Teilnehmer eine Benachrichtigung `object_type: call` an und nimmt sie beim Beitreten zurück; `notification-state` sagt, ob weiter geklingelt oder als verpasst gezeigt wird. Talk Desktop fragt alle 30 s ab oder nutzt `notify_push`. Die Suite fragt alle 10 s ab, auch ohne offenes Fenster ([chat.md](chat.md), Entscheidung 9). | [Listener.php](https://github.com/nextcloud/spreed/blob/v25.0.6/lib/Notification/Listener.php#L257), [CallNotificationController](https://github.com/nextcloud/spreed/blob/v25.0.6/lib/Controller/CallNotificationController.php), [Talk Desktop](https://github.com/nextcloud/talk-desktop/blob/main/src/talk/renderer/notifications/notifications.store.js) | gleich |
| Anruf starten, beitreten, verlassen | **machbar** | `POST`/`PUT`/`DELETE /api/v4/call/{token}` mit `flags` (1 im Anruf, 2 Ton, 4 Bild), erneut klingeln mit `…/ring/{attendeeId}`. Vorher der Unterhaltung beitreten (`POST /room/{token}/participants/active`), laut Doku nur mit Cookie-Sitzung; ein zweiter Login kann die Sitzung verdrängen (409). | [Call-API](https://nextcloud-talk.readthedocs.io/en/latest/call/), [docs/call.md](https://github.com/nextcloud/spreed/blob/v25.0.6/docs/call.md) | gleich |
| Internes Signaling | **machbar mit Aufwand** | **Nicht dokumentiert** (der Abschnitt in der Doku heißt nur „Todo“), Protokoll nur im PHP-Controller und im JS-Client: `POST signaling/{token}` mit gebündelten Nachrichten (`offer`, `answer`, `candidate`, `mute` …, alle 500 ms), `GET` als Long Polling bis 30 s, der Server prüft jede Sekunde. Mesh: jeder mit jedem, die größere Session-ID ruft an. Kann sich mit jeder Talk-Version ändern. | [internal-signaling](https://nextcloud-talk.readthedocs.io/en/latest/internal-signaling/), [SignalingController](https://github.com/nextcloud/spreed/blob/v25.0.6/lib/Controller/SignalingController.php), [signaling.js](https://github.com/nextcloud/spreed/blob/v25.0.6/src/utils/signaling.js#L372), [webrtc.js](https://github.com/nextcloud/spreed/blob/v25.0.6/src/utils/webrtc/webrtc.js#L295) | das ist der Fall des eigenen Servers |
| HPB-Signaling | **machbar mit Aufwand** | Gut dokumentiert (WebSocket, `hello` v2 mit JWT, Janus-SFU, `requestoffer`), aber ein zweites Protokoll neben dem internen. Für Gruppenanrufe ab drei Personen nötig. | [standalone-signaling-api-v1](https://nextcloud-spreed-signaling.readthedocs.io/en/latest/standalone-signaling-api-v1/) | entfällt |
| STUN/TURN | **machbar** | `GET /api/v3/signaling/settings` liefert `stunservers` und `turnservers` mit `username`/`credential`, 24 h gültig, ohne Angabe der Laufzeit. Diese Zugangsdaten sind Geheimnisse. Ob der eigene Server TURN hat, ist offen; ohne TURN scheitern Anrufe hinter strenger NAT. | [Config.php](https://github.com/nextcloud/spreed/blob/v25.0.6/lib/Config.php#L515) | gleich |
| WebRTC in Rust | **machbar mit Aufwand** | Drei Wege, Tabelle unten. Nur Googles libwebrtc bringt Echo-Unterdrückung, Codecs und Hardware-Encoder mit; so machen es Talk Android und Talk iOS. | [LiveKit rust-sdks](https://github.com/livekit/rust-sdks), [webrtc-rs](https://github.com/webrtc-rs/webrtc), [talk-android](https://github.com/nextcloud/talk-android), Abschnitt [WebRTC in Rust](#webrtc-in-rust) | – |
| Kamera wählen und nutzen | **machbar mit Aufwand** | `nokhwa` 0.10.11 (V4L2, AVFoundation, Media Foundation) ist schwach gepflegt und baut auf macOS gerade nicht (Issue #245); kein PipeWire-Portal. libwebrtc und GStreamer bringen eigene Kameraquellen mit. Unter Flatpak braucht Linux das Kameraportal (`ashpd`). | [nokhwa](https://github.com/l1npengtul/nokhwa), [#245](https://github.com/l1npengtul/nokhwa/issues/245), [Camera-Portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Camera.html) | – |
| Mikrofon und Lautsprecher | **machbar** | `cpal` 0.18 listet und wählt Geräte (ALSA/PulseAudio/PipeWire, CoreAudio ab macOS 14.2, WASAPI); libwebrtc hat eine eigene Geräteverwaltung. Schwierig ist nicht das Gerät, sondern die Echo-Unterdrückung zwischen zwei Strömen mit eigener Uhr. | [cpal](https://github.com/RustAudio/cpal), [webrtc-audio-processing](https://github.com/tonarino/webrtc-audio-processing) | – |
| Video in Iced zeigen | **machbar** | Je Bild `image::Handle::from_rgba` (so zeigt `torro-htmlview` schon Bilder) reicht für ein bis drei Bilder; besser ein `shader`-Widget mit YUV-Texturen auf derselben wgpu 27 wie Iced (Vorbild `iced_video_player`). Ohne GPU nur der erste Weg. | [iced_video_player](https://github.com/jazzfool/iced_video_player), [research.md](research.md), Abschnitt 8 | – |
| Webansicht (wry) | **nicht machbar** (Linux), machbar mit Aufwand (macOS, Windows) | WebKitGTK hat WebRTC hinter einem experimentellen Bau-Schalter, in Distributionen aus (wry #85, Tauri #13143). WKWebView kann es ab macOS 12, wry hat dort offene Fehler bei der Rechte-Abfrage (#1195, #1918); WebView2 ist Chromium. Unter Wayland nur als eigenes Fenster. Dazu bräuchte die Webansicht eine eigene Anmeldung an Nextcloud, weil das App-Passwort den Dienst nicht verlassen darf. Die Suite hat die Webansicht beim Mail-Fenster bewusst entfernt ([mail.md](mail.md), Entscheidungen 8 und 9; zu Blitz statt Webansicht [research.md](research.md), Abschnitt 8). | [wry #85](https://github.com/tauri-apps/wry/issues/85), [Tauri #13143](https://github.com/tauri-apps/tauri/issues/13143), [wry #1918](https://github.com/tauri-apps/wry/issues/1918), [wry README](https://github.com/tauri-apps/wry) | – |
| Anruf im Browser öffnen | **machbar** | `https://<server>/call/<token>` öffnet die Unterhaltung samt Anruf im Standard-Browser, mit dessen Anmeldung. Der Link wird aus dem Ursprung des Kontos und dem geprüften Token gebaut (`RoomToken`). | [PageController](https://github.com/nextcloud/spreed/blob/v25.0.6/lib/Controller/PageController.php) | gleich |
| Anruf in Talk Desktop öffnen | **nicht machbar** | Talk Desktop registriert kein URL-Schema; der Wunsch ist offen (spreed #16354). Es lässt sich nur starten, nicht an eine Unterhaltung schicken. | [talk-desktop #1634](https://github.com/nextcloud/talk-desktop/issues/1634), [spreed #16354](https://github.com/nextcloud/spreed/issues/16354) | – |
| Bildschirm teilen (Ausblick) | **machbar mit Aufwand** | Eine eigene PeerConnection je Freigabe (`roomType: screen`); Aufnahme des Bildschirms je System (PipeWire-Portal, ScreenCaptureKit, Windows Graphics Capture). Nicht in der ersten Stufe. | [webrtc.js](https://github.com/nextcloud/spreed/blob/v25.0.6/src/utils/webrtc/webrtc.js) | – |
| Rechte für Kamera und Mikrofon (macOS) | **machbar** | `NSCameraUsageDescription` und `NSMicrophoneUsageDescription` fehlen heute in `packaging/macos/Info.plist.in`; ohne sie beendet macOS die App beim ersten Zugriff. Mit Hardened Runtime kommen die Berechtigungen `com.apple.security.device.camera` und `…audio-input` dazu. Windows hat nur einen Schalter für alle Desktop-Apps. | [Apple: Requesting authorization](https://developer.apple.com/documentation/avfoundation/requesting-authorization-to-capture-and-save-media), [Microsoft](https://support.microsoft.com/en-us/windows/manage-app-permissions-for-a-camera-in-windows-87ebc757-1f87-7bbf-84b5-0686afb6ca6b) | – |

## WebRTC in Rust

| Baustein | Stand | Echo-Unterdrückung | Codecs | Bau auf allen drei Systemen | Lizenz, Größe |
|---|---|---|---|---|---|
| **LiveKit [`libwebrtc`](https://crates.io/crates/libwebrtc) / `webrtc-sys`** (Googles libwebrtc, [rust-sdks](https://github.com/livekit/rust-sdks)) | 0.4.0 / 0.3.53 (2026-11-04), etwa wöchentlich; produktiv in Zed | ja (`AudioProcessingModule`) | VP8, VP9, H.264, AV1, Opus, Hardware-Encoder | vorgebaute Bibliotheken; Linux braucht clang ≥ 21, macOS `-ObjC`, Windows `+crt-static`; der Bau lädt 100–260 MB | BSD-3 bzw. Apache-2.0; +15–30 MB im Programm (*vermutet*) |
| **[webrtc-rs](https://github.com/webrtc-rs/webrtc)** ([`webrtc`](https://crates.io/crates/webrtc), sans-IO-Kern `rtc`, [Umbau](https://webrtc.rs/blog/2026/01/31/async-friendly-webrtc-architecture.html)) | 0.21.1 (2026-10-11), Weg zu 1.0, Browser-Tests zuletzt „Partial“ | keine | nur RTP-Verpackung, Encoder extern (libvpx, `openh264`) | reines Rust | MIT/Apache-2.0, klein |
| **GStreamer [`webrtcbin`](https://gstreamer.freedesktop.org/documentation/webrtc/index.html)** ([`gstreamer-rs`](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs) 0.25, [`webrtcdsp`](https://gstreamer.freedesktop.org/documentation/webrtcdsp/webrtcdsp.html)) | GStreamer 1.28.8, ausgereift | ja (`webrtcdsp`), wenn der Ton über GStreamer läuft | alles Installierte | Linux Systempakete, macOS/Windows Installer, Laufzeit mitliefern | LGPL, 50–200 MB Laufzeit (*vermutet*) |
| **[str0m](https://github.com/algesten/str0m)** | 0.25.0 (2026-11-03), eher für Server | keine | nur Verpackung, kein TURN-Client | reines Rust | MIT/Apache-2.0, klein |

Ohne libwebrtc fehlen eine gepflegte Echo-Unterdrückung (`webrtc-audio-processing` 2.1 bindet die
PulseAudio-Fassung, `aec3` ist „work in progress“) und gepflegte VP8-Encoder (die libvpx-Bindungen sind alt).
`openh264` ist gepflegt, aber nur Ciscos heruntergeladene Binärdatei ist von den Patentgebühren befreit
([FAQ](https://www.openh264.org/faq.html)).

**Belege, dass es nativ geht:** Talk Android und Talk iOS nehmen mit eigenen libwebrtc-Builds an Anrufen des
Web-Clients teil. Es gibt einen ersten Rust-Client, [`nextcloud-modern-talk`](https://github.com/adapt2move/nextcloud-modern-talk)
(GPUI, webrtc-rs, GStreamer, EUPL-1.2), der laut eigener Aussage gegen genau Nextcloud 35.0.2 und Talk 25.0.6 mit
internem Signaling läuft, aber erst drei Wochen alt ist (kein TURN über TCP/TLS, keine erneuerten
TURN-Zugangsdaten). Er taugt als Nachschlagewerk für das Signaling, nicht als Abhängigkeit (Lizenz, Reife). Der
Recording-Server von Talk dagegen steuert einen echten Browser (Selenium) und braucht HPB.

**Was ein Client mit internem Signaling können muss:** Unterhaltung mit Cookie-Sitzung betreten, Anruf beitreten,
`signaling/{token}` pollen und Nachrichten bündeln, je Gegenstelle eine PeerConnection (Rolle nach Session-ID),
den Datenkanal `status` für Ton an/aus, Bild an/aus und „spricht“, `nick` in `offer`/`answer`, und VP8 mit Opus
(der Web-Client erzwingt keine Codecs, *vermutet:* der Browser bietet VP8 und meist H.264 an).

## Aufwand je Weg

Grob in Personenwochen, für Anrufe zu zweit auf Linux, macOS und Windows, mit Oberfläche, API, MCP, Tests und
Paketierung. Text-Chat und Status (Stufen 1 bis 4 in [chat.md](chat.md)) sind in keinem Weg enthalten und
kommen in jedem.

| Weg | Aufwand | Was der Mensch bekommt | Risiken |
|---|---|---|---|
| **Selbst gebaut** (libwebrtc über LiveKit, eigenes Talk-Signaling, Bild in Iced) | **11–20** (mit webrtc-rs 18–32, mit GStreamer 13–21) | Anruffenster und Gerätewahl wie in den Entwürfen, im Torro-Design, auf allen Systemen | undokumentiertes Signaling, das sich ändern kann; Bau und Größe von libwebrtc; schwache Kamera-Crates; Mesh ab drei Personen; eigene Plattform-Bindungen bräuchten `unsafe` (heute nur in `torro-appkit` und nur für macOS erlaubt, für Linux und Windows eine neue Ausnahme) |
| **Eingebettete Webansicht** (wry, Talk-Web im eigenen Fenster) | 3–5 auf macOS und Windows | Talk-Web in einem Torro-Fenster, Design von Nextcloud | **unter Linux kein WebRTC**; eigene Anmeldung in der Webansicht; WebKitGTK käme zurück in die Suite |
| **Im Browser öffnen** (`<server>/call/<token>`) | **etwa 0,5** | „Audio call“/„Video call“ öffnen Talk im Standard-Browser; Klingeln, „Accept“ und „Decline“ bleiben in der Suite | Anruf sieht aus wie Talk-Web, nicht wie Torro; der Browser braucht eine eigene Anmeldung an Nextcloud |
| **Vorerst keine Anrufe** | 0 | Text und Status; ein Anruf anderer erscheint als „Missed video call“ mit „Open in Talk“ | Anrufe bleiben außerhalb der Suite |

## Empfehlung

**Anrufe zuerst im Browser öffnen, den nativen Weg später als eigenen Versuch prüfen.**

- Der Browser-Weg kostet fast nichts, geht auf allen drei Systemen und mit jedem Server, auch mit HPB, und
  lässt keine Geheimnisse aus dem Dienst. Klingeln, Annehmen und Ablehnen bleiben in der Suite, so wirkt der
  Chat trotzdem wie aus einem Guss.
- Der native Weg ist machbar, aber er ist das größte Stück des ganzen Chats und hängt an einem Protokoll, das
  Talk nicht als API verspricht. Er lohnt sich erst, wenn Text und Status im Alltag laufen. Dann klärt ein Versuch
  (eigenes Issue) mit libwebrtc und `nextcloud-modern-talk` als Nachschlagewerk die offenen Punkte: Signaling gegen
  den eigenen Server, TURN, Echo, Bau auf allen drei Systemen.
- Die Webansicht scheidet aus: Unter Linux, dem Hauptsystem der Suite, geht WebRTC dort nicht.
- Die Entwürfe für Anruffenster und Gerätewahl bleiben das Ziel für den nativen Weg.

## Entscheidung

**Entschieden am 2026-11-06 vom Menschen: Anrufe werden selbst gebaut** (Weg „Selbst gebaut“, gegen die
Empfehlung oben). Folgen, aufgenommen in [chat.md](chat.md) (Entscheidung 13, Abschnitt [Anrufe](chat.md#anrufe-stufe-5)):

- Der Weg ist Googles libwebrtc über die Crates `libwebrtc`/`webrtc-sys` von LiveKit, ohne LiveKit-Server, mit
  eigenem Talk-Signaling (zuerst intern, HPB später) und dem Bild in Iced über ein `shader`-Widget.
- Stufe 5 beginnt mit einem Versuch als eigenem Issue, der die Risiken oben gegen den eigenen Server prüft, bevor
  das Anruffenster gebaut wird.
- Bis das Anruffenster auf einem System läuft, öffnet „Accept“ den Anruf im Browser; „Open in Talk“ bleibt auch danach im Menü.
