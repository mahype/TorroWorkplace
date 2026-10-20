# KI in der Suite: Konten, Modelle, Zuordnung

Stand: 2026-10-20. **Diskussionsgrundlage.** Entstanden aus den Entscheidungen 8 und 9 in
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
   lokales Modell), eine Tageszusammenfassung eine größere. Darum wird **je Funktion** ein Konto, ein
   Modell und ein Aufwand gewählt.
4. **Daten nur dahin, wo du es erlaubt hast.** Je Quelle des Eingangs lässt sich festlegen, dass ihre
   Inhalte nur an **lokale** Modelle gehen (z. B. Mail und Messenger ja nur lokal, GitHub darf in die
   Cloud). Standard für Mail: nur lokal. Bis die Regeln je Quelle kommen (KI.1), gibt es dafür einen
   Schalter (Aufgabe C.17, Einstellung `mail_to_cloud`): „Mail to cloud models“ unter AI in der
   Steuerzentrale. Einschalten kann nur die Person selbst; dann lesen auch Modelle außerhalb der eigenen Rechner Mail, soweit
   die Rechte sie geben. Für Cloud-Modelle bleiben Einträge von Nextcloud und Talk weiter lokal.
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
| **Lokal** | Ollama, LM Studio, llama.cpp-Server, vLLM, LocalAI | OpenAI-kompatible Schnittstelle (`/v1/chat/completions`, `/v1/models`); Adresse, meist ohne Schlüssel |
| **Selbst gehostet** | eigener Server mit einem der obigen; Nextcloud Assistant (TaskProcessing-API, z. B. mit lokalem LLM auf dem Nextcloud-Server) | OpenAI-kompatibel bzw. über das vorhandene Nextcloud-Konto, ohne neuen Schlüssel |
| **Offene Modelle in der Cloud** | **OpenRouter** (Qwen, Llama, Mistral, DeepSeek … über einen Schlüssel; Filter „open weights“) | OpenAI-kompatibel (`https://openrouter.ai/api/v1`); API-Schlüssel |
| **Anbieter** | Anthropic, OpenAI, Mistral, Google | eigene Schnittstelle (Anthropic) bzw. OpenAI-kompatibel; API-Schlüssel |

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

## API und MCP

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| KI-Konto verbinden (lokal: Adresse; Anbieter: Schlüssel) | Accounts › Add › AI | `accounts.connect_ai` · Admin, nur App-Schlüssel | **Never** (Punkt 5: Geheimnisse nur über die Oberfläche) |
| Modelle eines Kontos | Accounts › KI-Konto › Models | `ai.list_models` · Read | `ai_list_models` |
| Modell an/aus (S.24) | Accounts › KI-Konto › Models, Schalter | `ai.set_model_enabled` · Admin (anschalten, was anderswo rechnet: nur Menschen) | `ai_set_model_enabled` |
| Zuordnungen lesen | Einstellungen des Moduls | `ai.list_assignments` · Read | `ai_list_assignments` |
| Zuordnung ändern | Einstellungen des Moduls | `ai.set_assignment` · Admin | `ai_set_assignment` (Admin braucht beim Agenten immer Freigabe) |
| Eintrag zusammenfassen | Paneel „Summarize“ | `inbox.summarize_item` · Write (löst einen Modellaufruf aus) | `inbox_summarize_item` |

Neuer Bereich `ai` im Katalog für die Einstellungen; die Funktionen selbst gehören zu dem Bereich, in
dem sie wirken (`inbox.*`), und prüfen dort die Rechte.

## Reihenfolge

Kommt **nach** E1–E3 des Eingangs. Aufgaben entstehen, wenn dieses Konzept freigegeben ist:

1. **KI.1 Kern:** KI-Konten (`AccountKind::Ai`), Anbieter-Trait mit OpenAI-kompatibel und Anthropic,
   Modell-Liste, Zuordnung je Funktion, Datenregel je Quelle, Zähler. Tests mit `httpmock`.
2. **KI.2 Steuerzentrale:** Anbieter „AI providers“ im Assistenten (die Zeile gibt es schon),
   lokale Anbieter oben, automatische Suche nach Ollama/LM Studio auf `localhost`; Modelle an/aus auf der Kontoseite
   unter Accounts (S.24, S.26, S.31 in [tasks/steuerzentrale.md](tasks/steuerzentrale.md); S.25 entfällt mit #231),
   die Zuordnung je Funktion in den Modulen.
3. **E6.x Eingang:** Spur vorschlagen, Eintrag zusammenfassen, Tageszusammenfassung.

## Offen

1. Datenregel: Reicht „nur lokal ja/nein“ je Quelle, oder braucht es mehr (z. B. „nur Betreff“)?
2. Kosten-Obergrenze je Konto: nötig im ersten Schritt?
3. Nextcloud Assistant als Anbieter gleich mit, weil das Konto schon da ist?
