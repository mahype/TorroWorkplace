# Messung: Qwen3-TTS ohne Python

Messbericht zu Issue #764 (Stand 15. November 2026). Er vergleicht zwei Laufzeiten, die Qwen3-TTS
ohne Python ausführen, und bereitet die Entscheidung vor, welche davon der Assistent als lokale Stimme
neben Pocket TTS bekommt. Den Klang beurteilt ein Mensch beim Anhören der Proben; ein Agent kann eine
geklonte Stimme nicht verlässlich bewerten.

## Aufbau

- **Rechner:** Linux (Omarchy), NVIDIA GeForce RTX 4090 (24 GB), Treiber 615.78, CUDA 13.4, 32 Kerne.
- **Laufzeiten:**
  - [Crane](https://github.com/lucasjinreal/Crane) (Rust, candle 0.11.1), Commit `e736a214`, Cargo-Feature
    `cuda`, BF16, nativer Decoder (nicht die ONNX-Rückfallebene).
  - [qwen3-tts.cpp](https://github.com/predict-woo/qwen3-tts.cpp) (C++17, GGML), Commit `35d38549`,
    mitgeliefertes GGML `3af5f576`, gebaut mit `-DGGML_CUDA=ON`, Lauf mit `QWEN3_TTS_BACKEND=cuda`, F16-GGUF.
- **Modelle:** `Qwen/Qwen3-TTS-12Hz-0.6B-Base` (Revision `5d839924`) und `Qwen/Qwen3-TTS-12Hz-1.7B-Base`
  (Revision `fd4b2543`).
- **Stimmen:** die Vorlagen der App, `crates/torroassistant/assets/voices/warm_woman.wav` und `calm_man.wav`,
  geklont mit ihrem Transkript aus #745 (heute `TRANSCRIPTS` in `voice/qwen.rs`), also im ICL-Modus. Whisper war
  nicht nötig.
- **Texte (Deutsch):** kurz (61 Zeichen), mittel (184 Zeichen), lang (711 Zeichen). Jeder Text wird an
  Satzgrenzen in Stücke von höchstens 500 Zeichen geteilt (das `PIECE` für OpenAI); der lange Text ergibt zwei
  Stücke. Der Einbau teilt wie Pocket TTS in Stücke von höchstens 200 Zeichen, sein erster Ton bei langen Texten
  liegt deshalb eher bei der Spalte „mittel“ als bei „lang“.
- **Läufe:** je Laufzeit × Modell × Stimme × Text drei Läufe, dazu ein Lauf nur auf dem Prozessor (0.6B,
  kurz und mittel). Sampling wie in beiden Projekten voreingestellt (Temperatur 0,9, Wiederholungsstrafe 1,05).

### Was gemessen wird

- **Erster Ton:** Zeit, bis das erste Stück fertig berechnet ist, ohne das Laden des Modells. Keine der
  beiden Laufzeiten streamt beim Klonen, der erste Ton kommt also erst mit dem ganzen ersten Stück.
- **Echtzeitfaktor:** Rechenzeit geteilt durch Tondauer (kleiner ist besser, unter 1 schneller als Abspielen).
- **Grafikspeicher:** Spitze über `nvidia-smi` (alle 100 ms) abzüglich des Grundwerts vor dem Lauf.
- Werte sind Mediane über beide Stimmen und drei Läufe.

Messprogramme: für Crane ein eigenes Binary im Beispiel-Paket (`torro_bench`), für qwen3-tts.cpp ein
Treiber um `qwen3-tts-cli`. Beide liegen mit den Rohdaten (`crane.jsonl`, `cpp.jsonl`) unter
`~/.cache/torro-qwen-messung/run/`, nicht im Repo.

## Ergebnisse

### Grafikkarte (RTX 4090)

| Laufzeit | Modell | Erster Ton kurz | Erster Ton mittel | Erster Ton lang | Echtzeitfaktor lang | Grafikspeicher |
|---|---|---|---|---|---|---|
| Crane | 0.6B | **0,87 s** | **2,5 s** | **3,8 s** | **0,17** | +8,0 GB |
| Crane | 1.7B | 1,2 s | 3,5 s | 4,4 s | 0,21 | +10,1 GB |
| qwen3-tts.cpp | 0.6B | 2,7 s | 6,4 s | 8,3 s | 0,39 | **+3,4 GB** |
| qwen3-tts.cpp | 1.7B | 3,1 s | 5,9 s | 9,9 s | 0,47 | +5,8 GB |

- Crane ist beim ersten Ton etwa dreimal so schnell wie qwen3-tts.cpp; qwen3-tts.cpp braucht dafür
  weniger als die Hälfte des Grafikspeichers.
- **Laden:** Crane 0,8 bis 2 s (Dateien im Speicher). qwen3-tts.cpp lädt bei jedem Aufruf der
  Kommandozeile neu, etwa 9 bis 10 s; als eingebundene Bibliothek fiele das nur einmal an.
- Bei qwen3-tts.cpp kostet das Dekodieren den größten Teil: Die Vorlage wird bei jedem Stück mitdekodiert
  (kurzer Satz: 0,7 s Erzeugen, 1,8 s Dekodieren).
- Bei Crane 1.7B mit der Männerstimme schwankten die Zeiten (Echtzeitfaktor 0,33 bis 0,71). Während der
  Crane-Messung lief auf derselben Karte der Python-Qwen-Worker des installierten Assistenten (5,8 GB),
  das erklärt vermutlich die Ausreißer; die Mediane oben sind davon kaum betroffen.

### Nur Prozessor (0.6B)

| Laufzeit | Erster Ton kurz | Erster Ton mittel | Echtzeitfaktor |
|---|---|---|---|
| Crane (F32) | 69 s | 171 s | 15 bis 18 |
| qwen3-tts.cpp (16 Threads) | 16 s | 32 s | 3 bis 5 |

Ohne Grafikkarte ist keine der beiden brauchbar; qwen3-tts.cpp ist dort deutlich weniger schlecht. Das
bestätigt den Plan: Qwen3-TTS nur mit Grafikkarte, sonst Pocket TTS.

### Auffälligkeiten im Ton

- **Stille am Anfang:** Fast jede Probe von Crane beginnt mit etwa 0,56 s Stille; einmal (0.6B,
  Männerstimme, mittlerer Text) waren es 8,4 s vor dem eigentlichen Satz. Die Probe ist dadurch 19,3 s
  statt etwa 11 s lang, und der erste hörbare Ton käme 8 s später, als die Tabelle sagt. Bei qwen3-tts.cpp
  traten vereinzelt 1,2 bis 1,4 s auf. Ein Einbau muss Stille am Anfang jedes Stücks abschneiden.
- **Nicht endende Ausgabe:** qwen3-tts.cpp vermerkt im Code, dass das 1.7B-Modell im ICL-Modus oft kein
  Ende findet, und versucht es dann bis zu viermal neu. In der Messung brauchten drei Stücke (0.6B,
  Männerstimme) einen zweiten Versuch.
- **Encoder der Vorlage:** qwen3-tts.cpp meldet beim Einlesen der Vorlage Werte, die deutlich von der
  Python-Referenz abweichen (Mittelwert 0,60 statt −0,06, Streuung 15,2 statt 4,9). Ob das die Ähnlichkeit
  der Stimme mindert, zeigt nur das Anhören.

## Klang

Beurteilt von einem Menschen durch Anhören der Proben (kurzer und mittlerer Satz, beide Stimmen) und Vergleich mit
den Vorlagen, 15. November 2026:

| Laufzeit | Modell | Klang |
|---|---|---|
| Crane | 0.6B | trifft beide Stimmen der App („Warm woman“, „Calm man“) erkennbar |
| Crane | 1.7B | kein hörbarer Gewinn gegenüber 0.6B |
| qwen3-tts.cpp | 0.6B | etwa gleich wie Crane |
| qwen3-tts.cpp | 1.7B | nicht eigens angehört |

Da der Klang gleich ausfällt, entscheiden Tempo und Einbau: Die Wahl fiel auf **Crane mit 0.6B** (Kommentar im
Issue #764). Die messbaren Auffälligkeiten im Ton stehen oben. Proben (je Laufzeit, Modell, Stimme und Text
der erste Lauf; bei qwen3-tts.cpp je Stück eine Datei `-p0`, `-p1`):

- Grafikkarte: `~/.cache/torro-qwen-messung/run/proben/` (28 Dateien)
- Nur Prozessor: `~/.cache/torro-qwen-messung/run/proben-cpu/`
- Vorlagen zum Vergleich: `crates/torroassistant/assets/voices/`

Ein früherer Bericht nannte für eine Rust-Portierung (vermutlich `qwen_tts`/qwen3-tts-rs, nicht Crane),
dass die Stimme der Vorlage nicht getroffen wurde; Crane ist eine eigene Umsetzung.

## Bauaufwand

| | Crane | qwen3-tts.cpp |
|---|---|---|
| Werkzeuge | Rust, CUDA-Toolkit (`nvcc`) | C++17-Compiler, `cmake`, CUDA-Toolkit; zum Umwandeln des Modells einmalig Python mit `torch`, `gguf`, `safetensors` |
| Bauzeit sauber (32 Kerne) | 2 min 7 s | 2 min 24 s (GGML mit CUDA und Programm) |
| Größe | Binary 69 MB (CUDA-Kernel eingebettet) | Programm 0,4 MB, `libqwen3tts.so` plus `libggml-cuda.so` 44 MB, `libggml-base`/`-cpu` je ~1 MB |
| Modell | Safetensors direkt von Hugging Face (0.6B: 1,8 GB + Tokenizer 0,7 GB; 1.7B: 3,9 GB + 0,7 GB) | GGUF, Umwandlung mit Python (0.6B: 1,8 GB, 1.7B: 3,9 GB, Vocoder 0,3 GB); fertige GGUFs nur von Dritten (z. B. `OpenVoiceOS/qwen3-tts-0.6b-f16`) |
| Anbindung an Rust | `crane-core` 26.9.0 auf crates.io, kein eigenes `unsafe` | keine Anbindung auf crates.io; eigenes FFI (`unsafe`) oder das Programm als Kindprozess mit mitgelieferten Bibliotheken |
| macOS | Metal über candle, CI auf `macos-latest` (nicht gemessen) | Metal über GGML laut README (nicht gemessen), keine CI |
| Windows | nicht in der CI, candle baut dort mit CUDA (nicht gemessen) | nicht erwähnt (nicht gemessen) |

Weitere Hürden bei qwen3-tts.cpp: Der Dateiname des Modells ist fest auf `qwen3-tts-0.6b-f16.gguf`
gesetzt (1.7B lief nur über einen Verweis unter diesem Namen), die README ruft die Umwandlungsskripte mit
Positionsargumenten auf, die Skripte verlangen aber `-i`/`-o`, und die Kommandozeile gibt umfangreiche
Debug-Ausgaben aus. Ein Einbau als Kindprozess müsste je Plattform C++-Bibliotheken mitliefern.

Andere Laufzeiten wurden nicht gemessen: `wavekat-tts` und `rightkit-tts-qwen` binden eine eigene
ONNX-Runtime ein (neben sherpa-onnx ausgeschlossen), `qwen_tts` (qwen3-tts-rs) ist der Kandidat des
Berichts mit nicht getroffener Stimme.

## Empfehlung

**Crane mit dem 0.6B-Modell**, beim Anhören bestätigt und eingebaut (Issue #764):

- Schnellster erster Ton (0,9 s für einen kurzen Satz), Echtzeitfaktor um 0,2.
- Reines Rust über crates.io, ohne eigenes `unsafe`, ohne `cmake` und ohne Python, auch nicht für das
  Modell: die Safetensors kommen unverändert von Hugging Face.
- 1.7B ist bei Crane langsamer und braucht 2 GB mehr Grafikspeicher; es lohnt nur, wenn es hörbar besser
  klingt.

Kosten und Pflichten beim Einbau: rund 8 GB Grafikspeicher (also nur für Karten mit genug Speicher, der
Worker endet beim Wechsel des Anbieters), Stille am Anfang jedes Stücks abschneiden, ohne CUDA-Gerät eine
Fehlermeldung statt des Prozessors.

qwen3-tts.cpp wäre die Wahl, wenn Grafikspeicher knapp ist (3,4 GB). Es braucht aber eigenes FFI oder
einen Kindprozess mit C++-Bibliotheken, Python zum Umwandeln des Modells oder GGUFs von Dritten, und es
ist beim ersten Ton etwa dreimal langsamer.
