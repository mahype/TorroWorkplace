# GitHub in der Mitteilungszentrale: private Repos und Auswahl der Repos (#433)

Stand: 2026-11-21. Konzept zu Issue #433 („Mitteilungszentrale: nur public repos?“). Die Abfrage selbst ist in
[eingang.md](eingang.md) beschrieben (Quelle GitHub, E5.4), die Rechte des Tokens in [settings.md](settings.md)
(„GitHub: Rechte“) und [github.md](github.md).

## Was gemeldet wurde

In der Mitteilungszentrale erscheinen unter der Quelle „GitHub“ nur zwei Einträge, beide aus dem öffentlichen
Repo `pluginkollektiv/antispam-bee`, Grund „Abonniert“. Benachrichtigungen aus den privaten Repos von Immoticket24
fehlen. Gewünscht: in den Einstellungen festlegen, welche Repos dort erscheinen, ohne dafür das GitHub-Token zu
ändern.

## Ursache

### Die Abfrage lässt private Repos nicht aus

Der Kern fragt GitHubs Benachrichtigungen über REST ab, ohne Filter nach Sichtbarkeit:

- `crates/torro-pim-github/src/notifications.rs:505` (`first_page`): `GET /notifications?all=false&per_page=50`,
  bei Bedarf mit `since`. Kein `is:public`, keine Suche, kein Filter nach Repo oder Besitzer.
- `notifications.rs:420–470` (`Notifications::list`): folgt `Link: rel="next"` bis zu 10 Seiten zu je 50
  (`MAX_PAGES`, `PER_PAGE`, Zeile 45/49). Bei zwei Einträgen spielt die Paginierung keine Rolle.
- `crates/torroworkplace/src/sources/github.rs:476` (`file`): legt jeden Thread ab, der seit `reach.since`
  geändert wurde. `reach.since` ist der Zeitpunkt der ersten Abfrage des Kontos minus drei Tage (`FIRST_DAYS`,
  Zeile 80; Zeile 292). Das betrifft alle Repos gleich, öffentliche wie private.
- `read_threads` prüft nur Form und Herkunft der Felder; ein Feld `private` wird gar nicht gelesen.

Gegenprobe mit einem Token mit `repo` (Anmeldung der GitHub CLI, Scopes `repo, read:org, …`): derselbe Endpunkt
liefert von 50 Threads 48 aus privaten Repos. Der Code holt private Repos also, sobald das Token sie erreicht.

### Was dann bleibt: das Token erreicht die privaten Repos nicht

Fine-grained Tokens scheiden aus: Sie bekommen auf `/notifications` ein `403`, die Quelle stünde dann auf
`missing_scope` und brächte gar nichts. Es ist also ein klassisches Token oder die Anmeldung der GitHub CLI.
Mögliche Gründe, warum GitHub damit die privaten Threads weglässt, vom wahrscheinlichsten an:

1. **Klassisches Token nur mit `notifications`, ohne `repo`.** Unsere eigenen Texte haben genau dazu geraten:
   der Fehler `NeedsScope` („Use a classic token with the notifications scope“, `notifications.rs:388`, vor
   diesem Fix), der Hinweis der Quelle in der Steuerzentrale („… with notifications or repo …“,
   `crates/torro-settings-ui/src/inbox.rs:273`, vor diesem Fix) und [eingang.md](eingang.md) („braucht klassisches Token mit
   `notifications`“). GitHubs Doku verlangt für den Endpunkt nur „`notifications` or `repo`“, sagt aber nicht,
   was ohne `repo` mit privaten Repos geschieht; Clients wie Gitify raten zu `repo` für private Repos.
   Nachweisbar ist es nur an Frank Staudes Token: Steuerzentrale › Accounts › GitHub-Konto › Gruppe
   „Permissions“, Zeile „Private repositories“. Steht dort „Private repositories need the repo scope“, ist das
   die Ursache.
2. **Die Organisation sperrt den Zugang.** Organisationen können klassische Tokens ganz sperren, Tokens nur nach
   SAML-SSO-Freigabe zulassen oder OAuth-Apps (auch die GitHub CLI) erst nach Freigabe durch einen Owner Zugriff
   geben. Dann reicht auch `repo` nicht. Prüfen: Organisation Immoticket24 › Settings › „Third-party access“
   bzw. „Personal access tokens“; beim Token auf github.com „Configure SSO“.
3. **GitHub benachrichtigt gar nicht.** Ohne „Watching“ oder Beteiligung (zugewiesen, erwähnt, Review) gibt es
   für ein Repo keine Threads. Das zeigt github.com/notifications im Browser genauso.
4. **Schon gelesen oder zu alt.** Der Kern holt nur ungelesene Threads (`all=false`) und nur ab drei Tagen vor
   der ersten Abfrage. Wer die IMT24-Benachrichtigungen im Browser oder per Mail-Link liest, sieht sie hier nicht.

Die ersten beiden Gründe sehen für die Mitteilungszentrale gleich aus: Die Quelle ist gesund, bringt aber nur
öffentliche Repos. Das ist die eigentliche Lücke: **niemand sagt dem Menschen, dass private Repos fehlen.**

## Lösung für private Repos

**In diesem PR (klein, nur Texte):**

- Der Rat lautet überall `repo` statt `notifications`: Fehlertext `NeedsScope` in
  `crates/torro-pim-github/src/notifications.rs`, Hinweis der Quelle in `crates/torro-settings-ui/src/inbox.rs`
  und `crates/torro-settings-ui/src/accounts/mod.rs` (mit Übersetzung in `crates/torro-core/src/i18n/de.rs`),
  [eingang.md](eingang.md), [settings.md](settings.md), [github.md](github.md).
- Kein neuer Request, keine neue Oberfläche.

**Als Folge-Issue [#947](https://github.com/mahype/TorroWorkplace/issues/947) (nicht umgesetzt):** Der Kern kennt die Scopes des Tokens schon
(`AccountInfo::scopes` aus `X-OAuth-Scopes`). Fehlt `repo`, sagt die Quelle GitHub das selbst, an zwei Stellen:

- in der Steuerzentrale bei der Quelle (Notification Hub › Settings › Sources, Gruppe des GitHub-Kontos) als
  Fußzeile, etwa „Only public repositories: the token has no repo scope.“, mit Verweis auf die Kontoseite;
- in `inbox.list_sources` als Hinweis an der Quelle, damit Agenten (MCP `inbox_list_sources`) dasselbe sehen.

Dafür bräuchte `SourceHealth` einen weiteren Wert (etwa `public_only`) oder `SourceInfo` ein eigenes Feld. Das
ist eine Änderung an einer festen Schnittstelle (`torro-ipc`) und gehört deshalb in ein eigenes, geplantes Issue.
Die Gründe 2–4 kann Torro nicht verlässlich erkennen; dort hilft nur der Text auf der Kontoseite.

## Auswahl der Repos

### Was es heute schon gibt

- **Ein Repo ausblenden:** Die Glocke an jedem GitHub-Eintrag bietet „Everything from {repo}“
  (`inbox.list_mute_options`, Merkmal `repo`), mit „Hide“ und „immer“ wird es eine Regel
  (`inbox.mute_item`, MCP `inbox_mute_item`). Alle Regeln stehen unter Notification Hub › Settings › Rules mit
  „Remove“ ([eingang.md](eingang.md), „Stummschalten nach Merkmalen“). Das ist eine Sperrliste und braucht einen
  Eintrag aus dem Repo, bevor man es ausblenden kann.
- **Die ganze Quelle aus:** Schalter „Notifications“ des GitHub-Kontos unter Sources (`inbox.mute_source`).
- **Repos eines Kontos auflisten:** `tasks.list_github_repos` (Read, MCP-Tool) liefert `owner/name` und ob es
  privat ist, bis 1.000 Repos.

Was fehlt, ist der umgekehrte Weg aus dem Issue: **nur bestimmte Repos zulassen**, ohne das Token anzufassen.

### Vorschlag

**Wo:** Notification Hub › Settings › Sources, in der Gruppe des GitHub-Kontos, unter dem Schalter
„Notifications“ eine Zeile „Repositories“. Dort und nicht in den Accounts, weil es nur die Mitteilungszentrale
betrifft (Regel aus [settings.md](settings.md): jede Einstellung an genau einer Stelle). Die Steuerzentrale zeigt
diese Seite über `crate::inbox` von `torro-settings-ui` ohnehin mit.

**Wie (nur beschrieben, das Design klärt Sven mit dem Designer):**

- Wahl zwischen „All repositories“ (Standard) und „Only selected“.
- Bei „Only selected“ eine Liste der Repos zum Abhaken, aus `tasks.list_github_repos`, private kenntlich gemacht,
  mit Suche, weil es Hunderte sein können. Oben die Repos, aus denen in letzter Zeit Benachrichtigungen kamen.
- Ganze Besitzer wählbar (`Immoticket24/*`), damit neue Repos einer Organisation von selbst dazukommen.
- Die Fußzeile sagt, was nicht kommt, etwa „Notifications from other repositories stay on GitHub.“ (F3).

**Standardverhalten:** „All repositories“. Ein neues Konto bringt alles, was das Token erreicht, wie heute.
Eine Auswahl wirkt nur auf neue Benachrichtigungen; offene Einträge aus abgewählten Repos werden erledigt wie
bei `mute_source` und nicht auf GitHub „done“ gesetzt (entschieden, F3).

**Wo gefiltert wird:** im Kern, in `sources/github.rs` beim Ablegen (`file`), bevor `inbox.file_remote` gerufen
wird. GitHubs `/notifications` kennt keinen Filter nach mehreren Repos (nur `/repos/{owner}/{repo}/notifications`
je Repo, das wären viele Abfragen und verlöre das billige `304`). Also weiter eine Abfrage je Konto, die Auswahl
filtert danach. Abgewählte Threads bleiben auf GitHub unberührt.

**Speicherung:** im Dienst, beim Zustand der Quelle, wie `muted` von `inbox.mute_source` (Inbox-Speicher, Schlüssel
ist die Quelle `github:<login>`). Wert: `null` für alle, sonst eine Liste aus `owner/name` und `owner/*`.
Entfernt man das Konto, geht die Auswahl mit (`Inbox::forget_source`).

**Drei Wege:**

| Funktion | Oberfläche | API | MCP |
|---|---|---|---|
| Auswahl lesen | Sources › GitHub-Konto › Repositories | `inbox.list_sources` liefert die Auswahl je Quelle mit (Read) | `inbox_list_sources` |
| Auswahl setzen | dieselbe Zeile | neu: `inbox.set_source_repos { source, repos: null \| [..] }`, Zugriff Admin (wie `mute_source`: dauerhafte Regel, Freigabe des Menschen) | `inbox_set_source_repos` |
| Repos zur Auswahl | Liste in der Zeile | vorhanden: `tasks.list_github_repos` | `tasks_list_github_repos` |

Kein Geheimnis berührt, also kein `Never`. Agenten-Schlüssel, die nur einige Repos sehen dürfen, bekommen in
`inbox.list_sources` nur die Namen, die sie erreichen (`Rights::reaches_repo`), damit die Auswahl keine privaten
Repo-Namen verrät.

**Aufwand:** ein Request mit Katalogeintrag, ein Feld in `SourceInfo`, Filter in `file`, eine Zeile mit Liste in
`torro-settings-ui`. Das ist mehr als klein und kommt nicht in diesen PR, sondern in #946.

## Entschieden durch Projekt-Bot (delegiert von Sven)

Stand: 2026-11-21.

- **F2, Zulassliste:** „Only selected“ kommt wie oben vorgeschlagen (Sources › GitHub-Konto, Standard „All
  repositories“, `owner/*` wählbar, Request `inbox.set_source_repos`, MCP-Tool, Filter im Kern) als eigenes
  Issue [#946](https://github.com/mahype/TorroWorkplace/issues/946). Die Gestaltung klärt der Designer.
- **F3, abgewählte Repos:** bleiben auf GitHub unberührt, kein „done“ dort.
- **F4, Hinweis „nur öffentliche Repos“:** Folge-Issue [#947](https://github.com/mahype/TorroWorkplace/issues/947).
  Ob ein neuer `SourceHealth`-Wert oder ein eigenes Feld, bleibt dort offen.

## Offene Fragen

1. Welcher der vier Gründe trifft bei Frank Staude zu? Die Zeile „Private repositories“ auf seiner Kontoseite und
   die Einstellungen der Organisation Immoticket24 (Third-party access, PAT-Richtlinie, SSO) entscheiden es.
   Geht an staude.
2. Gestaltung der Zeile „Repositories“ und der Liste: Sven mit dem Designer.
