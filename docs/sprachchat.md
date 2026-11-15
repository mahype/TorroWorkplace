# Sprachchat: ein Gespräch, das sich echt anfühlt

Stand: 2026-11-15, Issue [#825](https://github.com/mahype/TorroWorkplace/issues/825). **Konzept und gemeinsamer
Rahmen** für alle Issues zum Sprachchat. Der Sprachchat soll das Herz von Torro werden: Man redet mit dem Assistenten
wie mit einem Menschen, so wie im Live-Sprachmodus von ChatGPT. Die heutige Technik (Weckwort, Aufnahme, Vorlesen)
steht in [assistent.md](assistent.md#sprache-per-weckwort-mit-dem-assistenten-reden); dieses Dokument legt fest, wohin
sie sich entwickelt. Es beschreibt nur das Ziel, die Umsetzung steht in den Issues der [Wellen](#wellen).

## Heute

Der Sprachchat läuft streng nacheinander (`crates/torroassistant/src/voice/listen.rs`):

1. **Weckwort** „Hey Torro“ → **Ton** → **Aufnahme** bis eine Sekunde Stille (nach Lautstärke, `Limits::default` in
   `voice/audio.rs`: 5 s bis zum Anfang, 1 s Pause, höchstens 15 s).
2. **Whisper** erkennt den ganzen Satz erst nach dem Satzende.
3. Die **KI** bekommt den vollen Prompt und antwortet.
4. **Vorlesen** Satz für Satz (`assistant.speak_answer`, Pocket, Qwen oder OpenAI).

Beim Vorlesen ist das Mikrofon zu, danach folgen 300 ms Pause (`Listener::quiet_now`); das Gespräch endet nach 8 s
Schweigen (`TALK_WAIT_SECONDS`). Das ist sicher, der Assistent hört sich nicht selbst, hat aber drei Nachteile: Es dauert
lange bis zur Antwort, man kann ihn nicht unterbrechen, und eine Denkpause („ähm“, „warte mal kurz“) beendet den Satz.

## Zielbild

| Punkt | Wie es sich anfühlt | Issues |
|---|---|---|
| **Mikrofon immer offen** | Es bleibt auch beim Vorlesen offen, die eigene Stimme wird herausgerechnet | [#834](https://github.com/mahype/TorroWorkplace/issues/834) |
| **Unterbrechen jederzeit** | Wer zu sprechen anfängt, hat sofort das Wort; im Verlauf steht nur, was tatsächlich gesagt wurde | [#835](https://github.com/mahype/TorroWorkplace/issues/835) |
| **Satzende nach Sinn** | Ein fertiger Satz wird nach etwa 0,5 s beantwortet, „ähm“ und „warte mal“ halten den Turn | [#833](https://github.com/mahype/TorroWorkplace/issues/833), [#836](https://github.com/mahype/TorroWorkplace/issues/836) |
| **Nichts geht verloren** | Weder der Anfang der Person noch der Anfang der Antwort wird abgeschnitten | [#829](https://github.com/mahype/TorroWorkplace/issues/829) |
| **Zustände sichtbar** | Zuhören, Erkennen, Überlegen, Werkzeug (welches), Sprechen; dazu der Füllstand des Kontexts | [#830](https://github.com/mahype/TorroWorkplace/issues/830), [#831](https://github.com/mahype/TorroWorkplace/issues/831) |
| **Keine stillen Pausen** | Bei langen Schritten sagt er kurz, was er tut („Moment, ich schaue in deinen Kalender“) | [#837](https://github.com/mahype/TorroWorkplace/issues/837) |
| **Gesprächsstil** | Kurze Sätze, das Wichtigste zuerst, Rückfragen statt Vorträge; gesprochener Teil als eigene Sprechblase, saubere Trennung von gesprochen und gezeigt | [#826](https://github.com/mahype/TorroWorkplace/issues/826), [#827](https://github.com/mahype/TorroWorkplace/issues/827), [#832](https://github.com/mahype/TorroWorkplace/issues/832) |

Langfristiges Maß ist der Live-Modus von ChatGPT: Duplex, Unterbrechen, natürliches Abwechseln beim Sprechen.

## Zeitbudget

Gemessen wird vom **Satzende der Person bis zum ersten Ton der Antwort**. Ziel: unter einer Sekunde. Die Zielwerte
legen fest, wofür die Issues der Stufe 2 optimieren; die gemessenen Werte trägt die Zeitmessung
([#828](https://github.com/mahype/TorroWorkplace/issues/828)) nach, erst danach gibt es ein „Heute“.

| Schritt | Ziel | Heute | Issues |
|---|---|---|---|
| Satzende erkannt | ≤ 350 ms | offen (#828) | [#833](https://github.com/mahype/TorroWorkplace/issues/833), [#836](https://github.com/mahype/TorroWorkplace/issues/836) |
| Text erkannt (nach dem Satzende) | ≤ 100 ms | offen (#828) | [#833](https://github.com/mahype/TorroWorkplace/issues/833) |
| Erstes Wort der KI | ≤ 250 ms | offen (#828) | [#831](https://github.com/mahype/TorroWorkplace/issues/831) |
| Erster Ton | ≤ 250 ms | offen (#828) | [#829](https://github.com/mahype/TorroWorkplace/issues/829) |
| **Summe** | **< 1 s** | offen (#828) | |

Dauert ein Schritt trotzdem länger (Werkzeug, langsames Modell), füllt die Ansage aus
[#837](https://github.com/mahype/TorroWorkplace/issues/837) die Pause.

## Zwei Wege

### Weg A: lokale Kette (Standard)

Erkennen → KI → Vorlesen, wie heute, aber dicht verzahnt. Läuft auf dem Rechner, das Mikrofon verlässt ihn nicht
(Open First). Vorbild für die Verzahnung ist Kyutai „Unmute“.

- **Stufe 1: messbar, schneller, sichtbar** ([#826](https://github.com/mahype/TorroWorkplace/issues/826) bis
  [#832](https://github.com/mahype/TorroWorkplace/issues/832)): Zeitmessung, kein abgehackter Anfang, Zustände,
  Füllstand, Skills je App und für den Gesprächsstil, Sprechblase. Die Kette bleibt nacheinander.
- **Stufe 2: Mikrofon offen und dichter Ablauf** ([#833](https://github.com/mahype/TorroWorkplace/issues/833) bis
  [#837](https://github.com/mahype/TorroWorkplace/issues/837), Vorbild Kyutai „Unmute“): Sprache per Modell (Silero VAD)
  statt Lautstärke, Text schon beim Sprechen, Echo-Unterdrückung, Unterbrechen, Satzende nach Sinn, Ansagen beim
  Nachdenken.
- **Stufe 3: Ausblick, noch ohne Issues:** Die KI beginnt zu antworten, bevor der Satz zu Ende ist, und das Vorlesen
  beginnt mit dem ersten Wort statt dem ersten Satz.

Zwei Entscheidungen lässt das Konzept bewusst den Plänen:

- **Echo-Unterdrückung (#834).** Optionen: das PipeWire-Modul `echo-cancel` (nur Linux, ohne neue Abhängigkeit) oder
  `webrtc-audio-processing` mit dem vorgelesenen Ton als Referenz (alle Systeme, neue Abhängigkeit). Maßstab: Lautes
  Vorlesen über Lautsprecher löst weder Weckwort noch Satz aus (Test mit Tondateien); sie muss auf Linux, macOS und
  Windows bauen; ohne laufende Unterdrückung bleibt das Mikrofon beim Vorlesen zu wie heute.
- **Satzende nach Sinn (#836).** Optionen: Regeln auf dem laufenden Text für den Anfang, oder ein kleines Modell
  (Turn Detector von LiveKit, Smart Turn von Pipecat). Maßstab: Deutsch und Englisch, kürzere Zeit bis zur Antwort und
  weniger abgeschnittene Sätze (gemessen mit #828). Die Grenze von 15 s je Satz (`Limits::most`) ist mit „warte mal“
  neu zu bedenken, sonst bricht ein ausdrücklich gehaltener Turn nach 15 s ab.

### Weg B: Sprache-zu-Sprache-Engine (Alternative, danach)

Ein Modell, das Ton direkt hört und spricht, zuerst die OpenAI Realtime API
([#838](https://github.com/mahype/TorroWorkplace/issues/838)). Es versteht Tonfall und Zögern und antwortet in wenigen
hundert Millisekunden, aber jedes Wort geht in die Cloud und kostet je Minute. Darum:

- **Nur ein Mensch wählt Weg B**, so wie heute OpenAI beim Vorlesen (`speech_provider` `"openai"`, siehe
  [assistent.md](assistent.md#vorlesen-issue-745)). Agenten dürfen die Wahl lesen, nicht setzen. Weg A bleibt Standard;
  Weg B als Vorgabe ist verworfen.
- **Der Dienst hält die Verbindung und den Schlüssel.** Der Zuhörer schickt Ton über den Dienst; der Schlüssel
  verlässt ihn nie.
- **Werkzeuge laufen als Funktionsaufrufe durch den Kern**, mit denselben Rechten, Freigaben und Protokoll wie bei
  Weg A. Echo-Unterdrückung und Unterbrechen gelten auch hier; Verlauf und Anzeige wie bei Weg A.
- **Offen für #838:** Der Ton des Mikrofons läuft heute nicht über `core.sock`; der Plan legt den Weg fest.

Weg B startet erst nach Stufe 2 von Weg A, weil er dieselbe Echo-Unterdrückung und dasselbe Unterbrechen braucht und
sein Gewinn nur an Weg A gemessen werden kann (Vergleich über die Zeitmessung #828).

## Regeln

Sie gelten für jedes Issue dieses Konzepts.

1. **Schlüssel bleiben im Dienst**, auf jedem Weg (`docs/api.md`, Punkt 1); bei Weg B hält der Dienst die Verbindung.
2. **Keine Freigabe per Stimme.** Eine Freigabe erteilt nur ein Mensch im Fenster, auch nicht durch Unterbrechen oder
   ein „Ja“ im Gespräch; der Assistent sagt, dass sie wartet (wie heute, [Gespräch](assistent.md#gespräch-issue-747)).
3. **Kein Ton auf der Platte.** Gesprochenes steht nur als Nachricht der Unterhaltung, nie im Protokoll, so wie heute.
   Die Zeitmessung schreibt Dauern, nie Text oder Ton.
4. **Das Mikrofon schaltet nur ein Mensch ein** (`docs/api.md`, Punkt 6): `assistant.start_talk` hat kein MCP-Tool.
5. **Oberfläche, API, MCP für jede neue Funktion** (`docs/api.md`): Request und Katalogeintrag kommen in den ersten
   Schritt des Plans, sonst begründet der Plan, warum ein Weg fehlt. Wer einen Request anfasst, lädt `torro-api` und
   prüft danach mit `torro-api-review`.
6. **Texte englisch über `tr()`.** Die deutschen Übersetzungen trägt nur die Koordination in
   `crates/torro-core/src/i18n/de.rs` ein; jedes Issue nennt seine Texte im Bericht.
7. **Kein Selbstgespräch** (heute: Mikrofon zu beim Vorlesen). Die Regel gilt, bis die Echo-Unterdrückung
   (#834) sie ablöst; danach gilt: Seine eigene Stimme löst nie einen Satz aus.

## Wellen

Die Issues sind so geschnitten, dass mehrere Agenten parallel arbeiten, ohne sich in denselben Dateien zu stören.

| Welle | Issues | Hängt ab von | Berührt vor allem |
|---|---|---|---|
| 0, sofort | [#825](https://github.com/mahype/TorroWorkplace/issues/825) Konzept (dieses) | | `docs/` |
| 0 | [#826](https://github.com/mahype/TorroWorkplace/issues/826) Marke `---shown---` tolerant | | `torro-ipc` (`split_spoken`), Prompt |
| 0 | [#827](https://github.com/mahype/TorroWorkplace/issues/827) Sprechblase | | Chatfenster (`view.rs`) |
| 0 | [#828](https://github.com/mahype/TorroWorkplace/issues/828) Zeitmessung | | `voice/listen.rs`, Kern, Einstellungen |
| 1 | [#829](https://github.com/mahype/TorroWorkplace/issues/829) abgehackter Anfang | #828 | `voice/listen.rs`, `voice/audio.rs` |
| 1 | [#830](https://github.com/mahype/TorroWorkplace/issues/830) Zustände | | `torro-ipc`, Kern, Chatfenster, Tray |
| 1 | [#831](https://github.com/mahype/TorroWorkplace/issues/831) Kontext-Füllstand | | Kern (`compact.rs`), Chatfenster |
| 1 | [#832](https://github.com/mahype/TorroWorkplace/issues/832) Skills je App | | Kern (Skills, Prompt) |
| 2 | [#833](https://github.com/mahype/TorroWorkplace/issues/833) Silero VAD | Konzept | `voice/audio.rs`, `voice/listen.rs`, Modelle |
| 2 | [#834](https://github.com/mahype/TorroWorkplace/issues/834) Echo-Unterdrückung | Konzept | `voice/listen.rs`, Tonausgabe |
| 2 | [#837](https://github.com/mahype/TorroWorkplace/issues/837) Ansagen beim Nachdenken | #830 | Kern, Vorlesen |
| 3 | [#835](https://github.com/mahype/TorroWorkplace/issues/835) Unterbrechen | #833, #834 | `voice/listen.rs`, Kern |
| 3 | [#836](https://github.com/mahype/TorroWorkplace/issues/836) Satzende nach Sinn | #833 | `voice/audio.rs`, `voice/listen.rs` |
| 4 | [#838](https://github.com/mahype/TorroWorkplace/issues/838) Realtime-Engine (Weg B) | #834, #835 | Kern, Zuhörer, Einstellungen |

- **Issues an `voice/listen.rs` laufen nie parallel in einer Welle:** #828 und #829 nacheinander, #833 und #834 in
  getrennten Abschnitten der Datei oder nacheinander, #835 und #836 ebenso. Wer ein Issue beginnt, prüft vorher, ob ein
  anderes dieselbe Datei offen hat.
- **Modelle:** Dieses Konzept mit Fable. Pläne für die Issues der Stufe 2 und Weg B empfohlen mit Fable, Umsetzung aller
  Issues mit Opus (`strong-model-recommended`).
- **Übersetzungen:** Laufen Issues parallel, trägt nur die Koordination in `de.rs` ein (Regel 6).
