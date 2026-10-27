# KI in der Suite: Konten, Modelle, Zuordnung

Stand: 2026-10-27. **Diskussionsgrundlage.** Entstanden aus den Entscheidungen 8 und 9 in
[eingang.md](eingang.md). Ergänzt den Eintrag „KI-Anbieter“ in [settings.md](settings.md).

## Zwei verschiedene Richtungen

Bisher kommt KI in der Suite nur von außen: **Agenten rufen die Suite** über MCP (Claude Code,
Codex …), Rechte in Steuerzentrale › MCP ([api.md](api.md), seit #231). Neu ist die umgekehrte Richtung:

| Richtung | Wer ruft wen | Wo eingestellt | Beispiel |
|---|---|---|---|
| **Agenten** | ein Agent ruft die Suite | Steuerzentrale › MCP (Skripte: › API) | Claude Code liest den Eingang |
| **KI-Konten** (neu) | die Suite ruft ein Modell | Steuerzentrale › AI (Anbieter einrichten; auf der Kontoseite Modelle an/aus, seit #231); welches Modell wofür: in den Einstellungen des Moduls | Der Eingang lässt eine Spur vorschlagen |

Beides bleibt getrennt. Ein Agent bekommt nie Zugriff auf die KI-Konten der Suite, und die Suite
benutzt nie den Schlüssel eines Agenten.

## Grundsätze

1. **Immer optional.** Ohne KI-Konto fehlen die KI-Funktionen einfach; kein Hinweis-Banner, kein
   „Upgrade“. Jede Funktion arbeitet vorher schon vollständig ohne KI.
2. **Open First: lokal und offen zuerst.** Die Suite ist Self-Hosting-First wie bei Nextcloud. Lokale Modelle sind
   gleichberechtigt und stehen im Assistenten oben, danach offene Modelle über OpenRouter, dann
   geschlossene Anbieter. Der Chat-Assistent ([assistent.md](assistent.md)) folgt derselben Reihenfolge.
3. **Je Funktion das passende Modell.** Spur vorschlagen ist eine kleine Aufgabe (kleines, schnelles,
   lokales Modell), eine Tageszusammenfassung eine größere. Darum wird ein Konto, ein Modell und ein
   Aufwand gewählt – seit #325 **je Platz** (Chat, Sortieren; Abschnitt „Plätze“). Eine Funktion kann
   davon eine eigene Zuordnung als Ausnahme haben.
4. **Daten nur dahin, wo du es erlaubt hast.** Je Quelle des Eingangs lässt sich festlegen, dass ihre
   Inhalte nur an **lokale** Modelle gehen (z. B. Mail und Messenger ja nur lokal, GitHub darf in die
   Cloud). Standard für Mail: nur lokal. Bis die Regeln je Quelle kommen (KI.1), gibt es dafür je Platz
   einen Schalter (Aufgabe C.17 und #325, Abschnitt „Erlaubnis je Platz“): `mail_to_cloud` für Chat,
   `decide_mail_to_cloud` für Sortieren, unter „Data rule“ in der Steuerzentrale › AI. Einschalten kann
   nur die Person selbst; dann lesen auch Modelle außerhalb der eigenen Rechner Mail, soweit die Rechte
   sie geben. Für Cloud-Modelle bleiben Einträge von Nextcloud und Talk weiter lokal.
   Der Schalter hieß bis Issue #219 „Mail an Claude Code und Cloud-Modelle“; Agenten als Antrieb
   (Antrieb B in [assistent.md](assistent.md)) werden nicht mehr angeboten, die Sperre gilt aber
   weiter für jeden Agenten-Antrieb, falls einer zurückkommt.
   Die Regel gilt ab der nächsten Antwort; eine laufende Antwort behält, was beim Start galt.
   **Lokal heißt: das Modell rechnet auf einem eigenen Rechner**, nicht „der Server ist lokal“. Modelle
   von Ollama mit `:cloud` laufen über das lokale Ollama auf `ollama.com` und gelten als entfernt
   (`remote` in `ai.list_models`, Aufgabe S.23 in [tasks/steuerzentrale.md](tasks/steuerzentrale.md)).
   Jede KI-Funktion bestimmt den Ort über Konto **und** Modell (`data_rule::place_of`), nachdem das
   Modell gewählt ist, auch bei „Automatic“.
5. **Vorschlag, nie Urteil.** KI-Ergebnisse sind gekennzeichnet, das Original bleibt sichtbar,
   nichts wird von einer KI versteckt, gelöscht oder verschickt.
6. **Schlüssel bleiben im Dienst** (Regel 1 und 5 in „Was nie nach außen geht“): API-Schlüssel im
   Schlüsselbund, eintragen nur über die Oberfläche, nie über MCP.

## Anbieter

| Art | Anbieter | Anbindung |
|---|---|---|
| **Lokal** | Ollama, LM Studio, [Strata](https://github.com/Niko1221/Strata), llama.cpp-Server, vLLM, LocalAI | OpenAI-kompatible Schnittstelle (`/v1/chat/completions`, `/v1/models`); Adresse, meist ohne Schlüssel |
| **Selbst gehostet** | eigener Server mit einem der obigen; Nextcloud Assistant (TaskProcessing-API, z. B. mit lokalem LLM auf dem Nextcloud-Server) | OpenAI-kompatibel bzw. über das vorhandene Nextcloud-Konto, ohne neuen Schlüssel |
| **Offene Modelle in der Cloud** | **OpenRouter** (Qwen, Llama, Mistral, DeepSeek … über einen Schlüssel; Filter „open weights“) | OpenAI-kompatibel (`https://openrouter.ai/api/v1`); API-Schlüssel |
| **Anbieter** | Anthropic, OpenAI, Mistral, Google | eigene Schnittstelle (Anthropic) bzw. OpenAI-kompatibel; API-Schlüssel |

**Strata** (MIT, Standardadresse `http://127.0.0.1:8080/v1`) ist ein lokaler Server für sehr große offene
Modelle auf einem PC mit RTX- oder Radeon-Karte. Es ist ein Katalogeintrag, kein neues Protokoll. Auf Port 8080
läuft auch der llama.cpp-Server und beide beantworten `/v1/models` gleich; die Suche erkennt Strata deshalb an
`GET /health` (`"service": "strata"`, nicht hinter dem API-Schlüssel). Dessen Modell-Liste ist leer, solange
kein Modell geladen ist. Strata sagt nicht, ob ein Modell Werkzeuge kann (`tools` bleibt unbekannt); seine Modelle
rechnen auf diesem Rechner (`remote: false`) und unterliegen der Datenregel wie die von Ollama.

Zwei Protokolle decken fast alles ab: **OpenAI-kompatibel** und **Anthropic**. Weitere kommen als
Eintrag im Katalog dazu, nie als Umbau. Die Modell-Liste holt der Kern vom Anbieter (`/v1/models`),
damit niemand Modellnamen tippen muss.

## Zuordnung je Funktion

**Umgeplant am 2026-10-15** ([settings.md › KI](settings.md#ki)): Die Steuerzentrale richtet nur Anbieter ein und
schaltet Modelle an und ab (seit 2026-10-20 im eigenen Abschnitt AI, auf der Kontoseite des KI-Kontos). **Welches Modell eine Funktion nimmt, stellt man im Modul ein**, zu dem sie gehört (die
Inbox für „suggest lane“ und „summarize item“, in den Einstellungen der Inbox), und zur Wahl stehen nur angeschaltete
Modelle. Die Tabelle unten bleibt die Übersicht aller Funktionen; die Befehle `ai.list_assignments` und
`ai.set_assignment` bleiben, nur ihre Oberfläche wandert in die Module.

**Umgesetzt am 2026-10-19** (Aufgabe E12.5, Issue #221): Die Befehle `ai.list_assignments` (Read) und
`ai.set_assignment` (Admin) stehen, gespeichert in `Settings::ai_assignments`. Es gibt vorerst **einen**
Funktionsschlüssel, `inbox.categorize` (Mail einordnen, [kategorien.md](kategorien.md)); eine neue Funktion ist
eine Variante in `torro_core::ai::AiFunction` und sonst nichts. `ai.set_assignment` prüft vor dem Speichern, dass
das Konto besteht, das Modell anbietet, dass es entscheidet, wo die Funktion das braucht, und – solange
`mail_to_cloud` aus ist – dass es auf diesem Rechner rechnet. Die Datenregel ist damit doppelt gesichert: beim
Setzen und beim Aufruf.

Ursprünglich geplant: Seite **„AI“** in der Steuerzentrale (erscheint erst mit einem KI-Konto). Eine Zeile je Funktion:

| Funktion | Konto | Modell | Aufwand | Daten |
|---|---|---|---|---|
| Inbox: suggest lane | Ollama (lokal) | qwen3:8b | low | – |
| Inbox: summarize item | Ollama (lokal) | qwen3:14b | medium | – |
| Inbox: daily summary | Anthropic | Sonnet | medium | GitHub, Agents (Mail nur lokal) |
| … später: draft reply, find dates in mail | | | | |

- **Aufwand** (low / medium / high) wird je Anbieter übersetzt: Denkaufwand bzw. Denkbudget, wo das
  Modell es kann, sonst Länge der Antwort. Kein Anbieter-Fachchinesisch in der Oberfläche.
- **Standard:** keine Funktion ist zugeordnet. Beim ersten KI-Konto schlägt die Seite eine Zuordnung
  vor (lokal für kleine Aufgaben), die man mit einem Klick übernimmt.
- **Kosten:** je Konto Zähler für Anfragen und Tokens im Monat, optional eine Obergrenze; ist sie
  erreicht, pausieren die Funktionen dieses Kontos mit einem Hinweis am Konto.
- **Protokoll** wie überall ohne Inhalte: Funktion, Konto, Modell, Tokens, Dauer.

Seit #325 ist die Zuordnung je Funktion der Ausnahmefall: Der Normalfall ist der Platz (Abschnitt „Plätze“).

## Plätze

Man wählt nicht mehr je Funktion ein Modell, sondern je **Platz** (`torro_core::ai::AiSlot`, Schlüssel `chat`
und `decide`, in der Oberfläche „Chat“ und „Sorting“):

| Platz | Wofür | Erlaubnis für Mail |
|---|---|---|
| **Chat** (`chat`) | Assistent und Bots; ein neuer Chat beginnt mit dessen Modell | `mail_to_cloud` |
| **Sortieren** (`decide`) | die kleinen Entscheidungen der Suite, heute das Einordnen der Mail (`inbox.categorize`, `AiFunction::slot()`), später weitere | `decide_mail_to_cloud` |

- **Gespeichert** als `aiSlots` in den Einstellungen (je Platz `engine`, `model`, `effort`). Gesetzt wird ein
  Platz mit `ai.set_slot` (Admin) oder in der Steuerzentrale › AI, Gruppe „Models“.
- **Zurückfallen:** Eine Funktion ohne eigene Zuordnung nimmt das Modell ihres Platzes. Eine eigene Zuordnung
  (`ai.set_assignment`) ist die **Ausnahme** und geht vor; sie bleibt unberührt, wenn sich der Platz ändert.
  Ist sie ausgeschaltet, sortiert die Funktion nicht; sie fällt dann nicht auf den Platz zurück.
- **Übernahme beim ersten Laden** einer Datei von vor #325, einmal beim Lesen:
  - `assistantDefault` wird zum Platz *Chat*.
  - Eine eingeschaltete Zuordnung `inbox.categorize` wird zum Platz *Sortieren* und ist danach keine Ausnahme
    mehr. Eine ausgeschaltete bleibt, Sortieren bleibt also aus.
  - `decideMailToCloud` fehlt → es wird aus `mailToCloud` übernommen, aber nur, wenn dabei ein Platz
    *Sortieren* übernommen wurde; sonst `false`.
  - Ein schon gesetzter Platz wird nie überschrieben. Eine Datei, die `decideMailToCloud` schon hat, übernimmt
    nichts mehr.
- **Alias:** `settings.set_setting assistant_default` bleibt für ältere Clients und schreibt den Platz *Chat*,
  ohne die Prüfung von `ai.set_slot` (auch nicht, ob das Modell eingeschaltet ist). Das Feld `assistantDefault`
  steht nicht mehr in `settings.get_settings`.

## Empfehlung je Rechner

Torro misst den Rechner und schlägt je Platz ein Modell vor; herunterladen kann es das auf Wunsch. Ein Vorschlag
setzt nie etwas von selbst.

- **Messen** (`account/hardware.rs`, einmal je Lauf des Kerns): Arbeitsspeicher und Prozessor über `sysinfo`.
  Grafikkarte: Apple Silicon mit gemeinsamem Speicher ohne eigenen VRAM, sonst `nvidia-smi` (Linux, Windows),
  `/sys/class/drm` (Linux, AMD) und die Gerätedaten der Registry (Windows ohne NVIDIA). Die größte Karte zählt.
  Was sich nicht messen lässt, bleibt leer; das Messen liefert nie einen Fehler.
- **Stufen** aus `crates/torroworkplace/assets/ai/recommendations.toml`, die erste zutreffende gilt:
  `mac_large` (gemeinsamer Speicher ab 64 GB), `gpu` (VRAM ab 12 GB und RAM ab 32 GB), `large` (gemeinsamer
  Speicher ab 32 GB oder RAM ab 16 GB), `medium` (RAM ab 8 GB), `small` (sonst, auch bei unbekannter Hardware).
  Der Kern lässt 10 % weniger gelten, weil Systeme etwas weniger melden, als verbaut ist.
- **Lokal zuerst:** Ein lokales Modell der Stufe für den Platz gewinnt. Ein Strata-Eintrag zählt nur, wenn Strata
  ein passendes Modell ausliefert; sonst gilt der Ollama-Eintrag derselben Stufe. Ohne verbundenes Ollama bleibt
  der lokale Vorschlag, mit dem Grund „Needs Ollama“.
- **Cloud nur als Ausweg:** Ein Cloud-Modell kommt nur, wenn die Stufe für den Platz kein lokales Modell hat
  oder kein lokaler Server läuft, in der Anbieter-Reihenfolge aus Grundsatz 2. *Sortieren* bekommt einen
  Vorschlag nur, wenn Mail verbunden ist.
- **Die Liste ist Daten, keine Abfrage:** `recommendations.toml` ist in den Kern eingebunden und ändert sich nur
  mit einer neuen Version von Torro; es gibt keinen Abruf aus dem Netz. Jedes lokale Modell hat offene Gewichte
  und eine Lizenz aus einer offenen Liste (`apache-2.0`, `mit`, `qwen`, `llama`, `gemma`). Jede Stufe hat ein
  Modell für *Sortieren*.
- **Herunterladen** (`ai.pull_model`): nur Modelle, die die Liste als lokale Ollama-Modelle führt, nur in Ollama
  auf diesem Rechner, mit der Größe aus dem Vorschlag (`download_bytes`). Ein freier Name, der einen fremden
  Registry-Host nennt, wird abgewiesen. Ollama lädt im Hintergrund; `ai.list_pulls` zeigt den Stand,
  `ai.cancel_pull` bricht ab.
- **Oberfläche:** Steuerzentrale › AI › Models, „Use suggestion“ bei jedem Platz, mit der Größe, wenn zuerst
  geladen werden muss. Der Knopf lädt bei Bedarf herunter und setzt dann den Platz.

## Erlaubnis je Platz

Der eine Schalter aus Grundsatz 4 gilt jetzt je Platz:

- `mail_to_cloud` ist die Erlaubnis für **Chat**, mit der Bedeutung von bisher.
- `decide_mail_to_cloud` ist die Erlaubnis für **Sortieren**. Sie folgt der Wahl: Setzt man für Sortieren ein
  Modell anderswo (Cloud, Server im Netz), wird sie eingeschaltet (`ai.set_slot` setzt sie mit); wählt man ein
  Modell hier, wird sie zurückgenommen. In der Oberfläche steht sie unter „Data rule“ als Zeile mit „Stop“,
  solange sie an ist.
- **Einschalten nur Menschen**; ausschalten darf jeder Client mit Rechten.
- **Ein Modell anderswo für Mail-Funktionen setzt nur ein Mensch**: als Platz *Sortieren* wie als Ausnahme
  `inbox.categorize` (`ai.set_assignment`), auch wenn die Erlaubnis schon an ist. Die Zustimmung gilt einem
  Anbieter; kein Agent leitet sie auf einen anderen um. Ein Client bekommt dafür `forbidden`, auch nach einer
  Freigabe.
- **Geprüft wird zweimal:** beim Setzen und beim Aufruf. Ist die Erlaubnis aus, fragt das Einordnen kein Modell
  anderswo; es bleibt bei `NotLocal` und ändert nichts.

## Ersatzweg beim Sortieren

Sortieren braucht kein Entscheidermodell. Ein gewöhnliches Chat-Modell, das Werkzeuge nutzen kann, tut es auch:
Die Auswahl (`categorize::pick_model`) nimmt erst ein Entscheidermodell, dann ein Chat-Modell.

- Die Fragen gehen als ein Werkzeug `answer` (`torro_core::ai::decide_chat`): die Kategorie als Auswahl der
  angebotenen Namen, ob eine Antwort erwartet wird, als Ja/Nein, die Dringlichkeit als Zahl.
- Die Antwort wird **streng gelesen**: die Argumente des Werkzeugaufrufs, sonst ein JSON-Objekt im Text. Eine
  nicht angebotene Option oder ein falscher Typ lässt die Frage weg, eine abgeschnittene Antwort ist ein Fehler.
  Nichts wird erfunden.
- Das Chat-Modell gibt **keine Konfidenz**. Seine Kategorie gilt ohne Schwellwert und ohne Zahl; was es nicht
  sinnvoll beantwortet, wird `other`.
- Der Text der Mail bleibt im Zaun `<message>` wie bisher; die Anweisungen stehen nur im System-Text.

## API und MCP

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| KI-Konto verbinden (lokal: Adresse; Anbieter: Schlüssel) | Accounts › Add › AI | `accounts.connect_ai` · Admin, nur App-Schlüssel | **Never** (Punkt 5: Geheimnisse nur über die Oberfläche) |
| Modelle eines Kontos | Accounts › KI-Konto › Models | `ai.list_models` · Read | `ai_list_models` |
| Modell an/aus (S.24) | Accounts › KI-Konto › Models, Schalter | `ai.set_model_enabled` · Admin (anschalten, was anderswo rechnet: nur Menschen) | `ai_set_model_enabled` |
| Plätze und Zuordnungen lesen | Steuerzentrale › AI (Gruppe „Models“), Einstellungen des Moduls | `ai.list_assignments` (Feld `slots`: beide Plätze mit Modell und Erlaubnis) · Read; `settings.get_settings` (`aiSlots`, `decideMailToCloud`) · Read | `ai_list_assignments`, `settings_get_settings` |
| Platz setzen oder leeren | Steuerzentrale › AI (Gruppe „Models“), Einstellungen des Moduls | `ai.set_slot` · Admin; ein Modell anderswo für *Sortieren* nur Menschen | `ai_set_slot` (Freigabe) |
| Zuordnung ändern (Ausnahme je Funktion) | Einstellungen des Moduls | `ai.set_assignment` · Admin | `ai_set_assignment` (Admin braucht beim Agenten immer Freigabe) |
| Empfehlung je Rechner | Steuerzentrale › AI (Gruppe „Models“) | `ai.recommend` · Read | `ai_recommend` |
| Herunterladen | Steuerzentrale › AI, „Use suggestion“ | `ai.pull_model` · Admin; nur Modelle der Liste, nur Ollama auf diesem Rechner. Die Antwort zeigt den neuen Download als `running`; lehnt Ollama ihn sofort ab, steht er dort als `failed`; ist Ollama nicht erreichbar, kommt `offline` und nichts wird angelegt | `ai_pull_model` (Freigabe) |
| Stand der Downloads | dieselben Orte | `ai.list_pulls` · Read | `ai_list_pulls` |
| Download abbrechen | „Cancel“ | `ai.cancel_pull` · Write | `ai_cancel_pull` |
| Erlaubnis *Sortieren* | Steuerzentrale › AI, „Data rule“ („Stop“) | `settings.set_setting decide_mail_to_cloud` · Admin, einschalten nur Menschen; `ai.set_slot` setzt sie mit | `settings_set_setting` nur `false` |
| Eintrag zusammenfassen | Paneel „Summarize“ | `inbox.summarize_item` · Write (löst einen Modellaufruf aus) | `inbox_summarize_item` |

Neuer Bereich `ai` im Katalog für die Einstellungen; die Funktionen selbst gehören zu dem Bereich, in
dem sie wirken (`inbox.*`), und prüfen dort die Rechte.

## Reihenfolge

Kommt **nach** E1–E3 des Eingangs. Aufgaben entstehen, wenn dieses Konzept freigegeben ist:

1. **KI.1 Kern:** KI-Konten (`AccountKind::Ai`), Anbieter-Trait mit OpenAI-kompatibel und Anthropic,
   Modell-Liste, Zuordnung je Funktion, Datenregel je Quelle, Zähler. Tests mit `httpmock`.
2. **KI.2 Steuerzentrale:** Anbieter „AI providers“ im Assistenten (die Zeile gibt es schon),
   lokale Anbieter oben, automatische Suche nach Ollama/LM Studio/Strata auf `localhost`; Modelle an/aus auf der Kontoseite
   unter Accounts (S.24, S.26, S.31 in [tasks/steuerzentrale.md](tasks/steuerzentrale.md); S.25 entfällt mit #231),
   die Zuordnung je Funktion in den Modulen.
3. **E6.x Eingang:** Spur vorschlagen, Eintrag zusammenfassen, Tageszusammenfassung.

## Offen

1. Datenregel: Reicht „nur lokal ja/nein“ je Quelle, oder braucht es mehr (z. B. „nur Betreff“)?
2. Kosten-Obergrenze je Konto: nötig im ersten Schritt?
3. Nextcloud Assistant als Anbieter gleich mit, weil das Konto schon da ist?
