# Entscheidungsregeln

Nach diesen Regeln entscheiden der Projekt-Bot und die T3-Threads Detailfragen in Issues und PRs selbst, damit
Sven möglichst selten gefragt wird.

## Ablauf

1. Bei einer Detailfrage zuerst hier eine passende Regel suchen und sie anwenden.
2. Die Entscheidung im PR unter **„Entschieden durch Projekt-Bot (delegiert von Sven)“** vermerken, mit
   Regelnummer, z. B. „Client-ID fehlt: Funktion bleibt mit Hinweis aus (Regel 3)“.
3. Passt keine Regel, fragt der Bot Sven und schlägt gleich eine neue Regel vor. Sagt Sven Ja, wird die Regel
   hier unten mit der nächsten Nummer ergänzt.

## Regeln

1. **Was offensichtlich sein muss, wird gemacht.** Ohne Rückfrage erledigt wird, was in jedem Fall nötig ist,
   z. B. Build- und Testfehler beheben, kaputte Installer reparieren, `main` in einen PR holen und Konflikte
   lösen.
2. **Empfehlung vor Vorsicht.** Gibt es eine empfohlene Variante, wird sie genommen; ohne Empfehlung die
   vorsichtigere.
3. **Fehlt etwas, bleibt die Funktion aus.** Fehlt einer Funktion, was sie braucht (Schlüssel, Client-ID,
   Zugang), bleibt sie deaktiviert und sagt klar, was fehlt, statt halb zu laufen oder unklar zu scheitern.
   Beispiel: #942, Sign in with ChatGPT ohne Client-ID.
4. **Geschmacksfragen werden einstellbar.** Mit sinnvollem Standard und festen Grenzen. Beispiel: #945,
   `postpone_retention` 30–1095 Tage, Standard 365.
5. **Oberfläche ist ein eigenes Issue.** Oberflächen-Arbeit geht an den Designer und läuft nicht nebenbei in
   Backend-PRs mit.
6. **Der Dienst arbeitet, nicht der Nutzer.** Was der Dienst selbst erledigen kann, erledigt er. Beispiel: #941,
   der Dienst schreibt die Client-Konfiguration selbst, die App übergibt nur `PATH`.
7. **Automatische Zugänge bekommen nur das Nötigste.** Skript-Schlüssel ohne vorgewählte Rechte oder
   Adressbücher; Änderungen an Admin-Rechten brauchen die Freigabe eines Menschen.
8. **Bildschirm-Zugriff vorsichtig.** Für Computer-Use (#939, `docs/bildschirm.md`): vor Enter immer fragen,
   Fenstertext nur an lokale Modelle, Browser gesperrt.
9. **Sven nur bei Grundsatzfragen.** Gefragt wird nur zu Sicherheit, Datenschutz, Architektur und neuen
   Abhängigkeiten, gesammelt und einmalig mit Hintergrund.
10. **Merge und main bleiben bei Sven.** Ohne Svens ausdrückliches OK kein Merge, kein Push auf `main`, kein
    Issue schließen. Die Merge-Reihenfolge freigegebener PRs legt der Bot selbst fest.
11. **Offensichtliche Kleinigkeiten werden selbstständig erledigt.** Einmal pro Stunde sucht der Projekt-Bot
    unter den offenen Issues nach Bugs und kleinen Aufgaben, die nach Regel 1 offensichtlich nötig, jetzt
    umsetzbar und sinnvoll sind. „Umsetzbar“ heißt umsetzungsreif nach „Umsetzungsreife (KI)“ in
    [issue-workflow.md](issue-workflow.md): `aufwand:klein`, vollständige und objektiv prüfbare Kriterien, keine
    offene Entscheidung, Protokollkommentar am Issue. Er setzt sie ohne Rückfrage in einem eigenen T3-Thread mit
    eigenem Branch und einem Draft-PR mit Präfix `[auto]` um. Ausgeschlossen sind Issues mit offenen Grundsatzfragen
    (Regel 9), reine UI-Themen (Regel 5) und Issues, zu denen es schon einen PR oder Thread gibt. Pro Lauf startet er
    höchstens ein Issue; liegen schon drei PRs mit `[auto]` ohne Review offen, startet er keines. Dazu gilt das
    Limit von höchstens drei gleichzeitig arbeitenden Coding-Agenten über alle T3-Threads und Dispatcher; es zählt
    das strengere von beiden. Mergen, Issues schließen und Pushen auf `main` bleiben nach Regel 10 bei Sven, nie per
    Auto-Merge.
