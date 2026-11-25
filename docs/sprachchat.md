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

Beim Vorlesen ist das Mikrofon zu, danach folgen 300 ms Pause (`Listener::quiet`, Feld `pause`); das Gespräch endet nach 8 s
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
| Erstes Wort der KI | ≤ 250 ms | offen (#828) | noch kein Issue, entscheidet sich nach der Messung (#828) |
| Erster Ton | ≤ 250 ms | offen (#828) | noch kein Issue, entscheidet sich nach der Messung (#828) |
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

- **Echo-Unterdrückung (#834), entschieden.** Die Unterdrückung von WebRTC (AEC3) als Nachbau in reinem Rust
  (`sonora`), mit dem vorgelesenen Ton als Referenz; sie baut auf allen drei Systemen. Verworfen: die C++-Bindung
  `webrtc-audio-processing` (baut unter Windows praktisch nicht) und das PipeWire-Modul `echo-cancel` (nur Linux,
  nicht mit Tondateien prüfbar). Der Vorlese-Helfer schreibt, was er spielt, als Zeilen `{"echo": …}` (16 kHz, mono,
  16 Bit, Base64) zwischen seine Antworten; der Dienst liest sie laufend und reicht sie über die Standardeingabe an
  den Hörer weiter, ohne neuen Zugang von außen und ohne Platte. Der Hörer rechnet den Ton aus jedem Mikrofonblock
  heraus (`voice/echo.rs`). Kommt eine Referenz (in den letzten 300 ms), bleibt das Mikrofon beim Vorlesen offen,
  auch während der eigenen Antwort (spricht die Person dabei, unterbricht sie ihn, siehe #835);
  sonst bleibt es zu wie bisher. Ohne Einstellung. Geprüft mit `hey_torro.wav` als Vorlesen in einem gerechneten
  Raum (Echo nach 30, 60 und 90 ms): Der Hörer hört darin keine Sprache, die Person darüber aber schon.
- **Satzende nach Sinn (#836), entschieden.** Smart Turn v3 von Pipecat (32 MB ungepackt: die gepackte Fassung mit 8,7 MB rechnet tract falsch, gegen onnxruntime geprüft; ONNX auf tract, Deutsch und Englisch)
  hört am Tonfall, ob ein Satz fertig ist, Regeln lesen den vorab erkannten Text; der Text gewinnt, wo er mehr sagt
  („und“, „ähm“, „Moment“). Verworfen: der Turn Detector von LiveKit (ein Sprachmodell auf Text, groß und langsamer)
  und Regeln allein (Whisper liefert den Text oft erst nach der kurzen Pause). Pausen 500 ms nach einem fertigen Satz,
  2 s nach einem unfertigen, 15 s Halten nach „warte mal“; alle einstellbar, Agenten dürfen sie ändern. Ein gehaltener
  Turn zählt nicht zur Grenze von 15 s je Satz (`Limits::most`). Gemessen wird mit #828, wie oft eine Pause ausgehalten
  wurde, die früher den Satz beendet hätte (`held`). Einzelheiten:
  [assistent.md](assistent.md#gespräch-issue-747).

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
- **Entschieden in #838:** Der Ton des Mikrofons läuft nicht über `core.sock`, sondern über die Leitung, die Dienst
  und Hörer schon verbindet (`{"mic": …}` auf der Ausgabe des Hörers); kein Programm erreicht ihn. Erste Module sind
  OpenAI Realtime (auch mini) und Grok Voice über dasselbe Protokoll; Gemini Live (#918, eigenes Protokoll, eigener
  Adapter `live/gemini.rs`) ist gefolgt, ebenso GPT-Live (#919, `live/gpt.rs`): dort hört und spricht das Modul nur,
  und der Torro-Assistent denkt dahinter (Chat-Modell, Skills, Werkzeuge, Rechte wie im Chat). Nova 2 Sonic (#920)
  folgt als weiterer Adapter. Das Modul denkt selbst und ruft die Werkzeuge auf; Satzende und
  Unterbrechen erkennt der Anbieter, Torro rechnet vorher die eigene Stimme heraus. Einzelheiten:
  [assistent.md](assistent.md), „Gesprächsmodul in der Cloud“.

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
7. **Kein Selbstgespräch:** Seine eigene Stimme löst nie einen Satz aus. Mit laufender Echo-Unterdrückung (#834) wird
   sie aus dem Mikrofon herausgerechnet, das offen bleibt; läuft sie nicht, ist das Mikrofon beim Vorlesen zu wie
   früher.

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
- **Modelle:** Pläne für die Issues der Stufe 2 und Weg B empfohlen mit Fable, Umsetzung aller
  Issues mit Opus (`strong-model-recommended`).
- **Übersetzungen:** Laufen Issues parallel, trägt nur die Koordination in `de.rs` ein (Regel 6).
