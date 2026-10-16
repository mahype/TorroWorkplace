// Issue-Kontext fuer KI-Agenten (uebernommen aus enon, docs/issue-workflow.md).
//
// Liest die Issue-Nummer aus dem Branch-Namen (<type>/<nr>-<slug>), holt das
// Issue live per `gh` und liefert es als Kontext-Text fuer den
// SessionStart-Hook von Claude Code (`claude` als Argument).
//
// Ein Fehler hier darf eine Session nie blockieren: SessionStart-Hooks koennen
// das nicht, und der Kontext ist Komfort, kein Muss. Fehler werden als Warnung
// gemeldet, die Session laeuft ohne Issue-Kontext weiter.

import { spawnSync } from "node:child_process"
import { pathToFileURL } from "node:url"

const REPO = "mahype/TorroWorkplace"
const KOMMENTARE = 10
const MAX_ZEICHEN = 20_000 // je Beschreibung und je Kommentar
const TIMEOUT_MS = 15_000

export function issueNummerAusBranch(branch) {
  const treffer = /^[a-z]+\/(\d+)-/.exec(branch ?? "")
  return treffer ? treffer[1] : null
}

function kuerze(text, quelle) {
  const t = (text ?? "").trim()
  return t.length > MAX_ZEICHEN ? `${t.slice(0, MAX_ZEICHEN)}

[... gekuerzt, vollstaendig: ${quelle}]` : t
}

export function formatiereKontext(nummer, branch, daten) {
  const quelle = `https://github.com/${REPO}/issues/${nummer}`
  const zeilen = [
    `# Issue #${nummer}: ${(daten.title ?? "").trim()}`,
    "",
    "> Live von GitHub geladen (scripts/agent/issue-context.mjs), nicht committen.",
    `> Quelle: ${quelle}`,
    "",
    "## Branch",
    branch,
    "",
  ]

  const abschnitt = (titel, werte) => {
    if (werte.length) zeilen.push(`## ${titel}`, werte.join(", "), "")
  }
  abschnitt("Labels", (daten.labels ?? []).map((l) => l.name))
  abschnitt("Assignees", (daten.assignees ?? []).map((a) => a.login))
  abschnitt("Milestone", daten.milestone?.title ? [daten.milestone.title] : [])

  zeilen.push("## Description", "", kuerze(daten.body, quelle), "")

  const kommentare = (daten.comments ?? []).slice(-KOMMENTARE)
  if (kommentare.length) {
    zeilen.push("## Comments", "")
    for (const k of kommentare) {
      const autor = k.author?.login ?? "unknown"
      zeilen.push(`### @${autor} (${(k.createdAt ?? "").slice(0, 10)})`, "", kuerze(k.body, quelle), "")
    }
  }
  return zeilen.join("\n")
}

function starte(befehl, args, cwd) {
  return spawnSync(befehl, args, { cwd, encoding: "utf8", timeout: TIMEOUT_MS, windowsHide: true })
}

// Ergebnis: { kontext } bei Erfolg, { warnung } bei Fehler, {} wenn der Branch
// zu keinem Issue gehoert (z.B. main) — dann ist Schweigen richtig.
export function ladeKontext(cwd, ausfuehren = starte) {
  const git = ausfuehren("git", ["rev-parse", "--abbrev-ref", "HEAD"], cwd)
  if (git.error || git.status !== 0) {
    return { warnung: `Issue-Kontext: Branch nicht lesbar (${git.error?.message ?? git.stderr?.trim()})` }
  }
  const branch = git.stdout.trim()
  const nummer = issueNummerAusBranch(branch)
  if (!nummer) return {}

  const gh = ausfuehren(
    "gh",
    ["issue", "view", nummer, "--repo", REPO, "--json", "title,body,labels,assignees,milestone,comments"],
    cwd,
  )
  if (gh.error || gh.status !== 0) {
    const grund = gh.error?.message ?? gh.stderr?.trim().slice(0, 200) ?? "keine Fehlerausgabe"
    return { warnung: `Issue-Kontext für #${nummer} nicht geladen (${grund}). Session läuft ohne ihn weiter.` }
  }
  try {
    return { kontext: formatiereKontext(nummer, branch, JSON.parse(gh.stdout)) }
  } catch (fehler) {
    return { warnung: `Issue-Kontext für #${nummer}: ungültige Antwort von gh (${fehler.message})` }
  }
}

// Claude-Code-Adapter: SessionStart-Protokoll. `systemMessage` zeigt dem Nutzer
// die Warnung, `additionalContext` landet im Kontext des Modells.
function claudeAusgabe(cwd) {
  const { kontext, warnung } = ladeKontext(cwd)
  const ausgabe = {}
  if (warnung) ausgabe.systemMessage = warnung
  if (kontext) ausgabe.hookSpecificOutput = { hookEventName: "SessionStart", additionalContext: kontext }
  return Object.keys(ausgabe).length ? JSON.stringify(ausgabe) : ""
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv[2] === "claude") {
    const text = claudeAusgabe(process.env.CLAUDE_PROJECT_DIR || process.cwd())
    if (text) process.stdout.write(text)
  } else {
    console.error("Aufruf: node issue-context.mjs claude")
    process.exitCode = 2
  }
}
