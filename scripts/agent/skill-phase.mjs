// Phasen der Workflow-Skills sichtbar machen (uebernommen aus enon, docs/issue-workflow.md).
//
// Eine Quelle fuer Icon, Anzeigename und Herdr-Status der Phasen von
// issue-plan, issue-approve, issue-implement, issue-small und quick-fix. Zwei
// Adapter:
//
//   node skill-phase.mjs hook
//     UserPromptSubmit-Hook (.claude/settings.json): erkennt `/issue-plan <nr>`
//     usw. und setzt den Sitzungstitel, z.B. "📋 Planning #188". `/quick-fix`
//     hat keine Issue-Nummer und setzt "⚡ Quick-Fix".
//   node skill-phase.mjs herdr <nr> <phase> [<status>]
//     Aus den Skills: setzt in Herdr die Agent-Anzeige des eigenen Panes und
//     optional den Status-Token am Workspace des Issue-Worktrees. Fuer die
//     Phase `quick` steht `-` statt der Nummer und es gibt keinen Status, weil
//     der Workspace ueber die Issue-Nummer gefunden wird.
//
// Beides ist Komfort: Ein Fehler darf weder einen Prompt blockieren noch einen
// Skill abbrechen. Der Hook schweigt im Fehlerfall, `herdr` warnt auf stderr
// und endet mit Exit 0. Nur ein falscher Aufruf (unbekannte Phase) endet mit 2.

import { spawnSync } from "node:child_process"
import { readFileSync } from "node:fs"
import { pathToFileURL } from "node:url"

export const PHASEN = {
  plan: { icon: "📋", name: "Planning" },
  approve: { icon: "🚦", name: "Approval" },
  implement: { icon: "⚙️", name: "Implementation" },
  small: { icon: "🎯", name: "Small" },
  quick: { icon: "⚡", name: "Quick-Fix" },
}

// Schluessel: was die Skills uebergeben. Wert: was Herdr anzeigt.
export const STATUS = {
  planning: "planning",
  "awaiting-approval": "awaiting approval",
  approved: "approved",
  implementing: "implementing",
  "in-review": "in review",
}

const QUELLE = "torro-skill-phase"
const TIMEOUT_MS = 5_000

export function phaseAusPrompt(prompt) {
  const text = prompt ?? ""
  if (/^\s*\/quick-fix(?![\w-])/.test(text)) return { phase: "quick", nr: null }
  const treffer = /^\s*\/issue-(plan|approve|implement|small)\s+#?(\d+)\b/.exec(text)
  return treffer ? { phase: treffer[1], nr: treffer[2] } : null
}

export function anzeige(phase, nr) {
  const { icon, name } = PHASEN[phase]
  return nr ? `${icon} ${name} #${nr}` : `${icon} ${name}`
}

export function hookAusgabe(eingabe) {
  try {
    const daten = JSON.parse(eingabe)
    const treffer = phaseAusPrompt(daten.prompt)
    if (!treffer) return ""
    const sessionTitle = anzeige(treffer.phase, treffer.nr)
    return JSON.stringify({ hookSpecificOutput: { hookEventName: "UserPromptSubmit", sessionTitle } })
  } catch {
    return ""
  }
}

function starte(befehl, args) {
  return spawnSync(befehl, args, { encoding: "utf8", timeout: TIMEOUT_MS, windowsHide: true })
}

// Liefert die Warnungen; leer heisst: alles gesetzt oder nichts zu tun.
export function setzeHerdr({ nr, phase, status, env, cwd }, ausfuehren = starte) {
  if (env.HERDR_ENV !== "1") return []
  const warnungen = []
  const herdr = (args) => {
    const r = ausfuehren("herdr", args)
    if (r.error?.code === "ENOENT") return { fehlt: true }
    if (r.error || r.status !== 0) {
      warnungen.push(`herdr ${args.slice(0, 2).join(" ")}: ${r.error?.message ?? (r.stderr?.trim() || r.stdout?.trim() || `Exit ${r.status}`).slice(0, 200)}`)
      return null
    }
    return r
  }

  if (env.HERDR_PANE_ID) {
    const r = herdr(["pane", "report-metadata", env.HERDR_PANE_ID, "--source", QUELLE, "--display-agent", anzeige(phase, nr)])
    if (r?.fehlt) return []
  }
  if (!status || !nr) return warnungen

  const liste = herdr(["worktree", "list", "--cwd", cwd])
  if (!liste || liste.fehlt) return warnungen
  let workspace
  try {
    const muster = new RegExp(`^[a-z]+/${nr}-`)
    workspace = (JSON.parse(liste.stdout).result?.worktrees ?? [])
      .find((w) => muster.test(w.branch ?? "") && w.open_workspace_id)?.open_workspace_id
  } catch (fehler) {
    warnungen.push(`herdr worktree list: ungueltige Antwort (${fehler.message})`)
    return warnungen
  }
  // Worktree in Herdr nicht offen: kein Workspace oeffnen, Status entfaellt still.
  if (!workspace) return warnungen
  herdr(["workspace", "report-metadata", workspace, "--source", QUELLE, "--token", `status=${STATUS[status]}`])
  return warnungen
}

function herdrCli([nr, phase, status]) {
  // `quick` hat keine Issue-Nummer (`-`) und damit auch keinen Workspace-Status.
  const direkt = phase === "quick"
  const nrOk = direkt ? nr === "-" && status === undefined : /^\d+$/.test(nr ?? "")
  if (!nrOk || !PHASEN[phase] || (status !== undefined && !STATUS[status])) {
    console.error(
      `Aufruf: node skill-phase.mjs herdr <nr> <${Object.keys(PHASEN).join("|")}> [<${Object.keys(STATUS).join("|")}>] (quick: \`-\` statt <nr>, ohne Status)`,
    )
    return 2
  }
  for (const w of setzeHerdr({ nr: direkt ? null : nr, phase, status, env: process.env, cwd: process.cwd() })) {
    console.error(`Warnung: ${w}`)
  }
  return 0
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [befehl, ...args] = process.argv.slice(2)
  if (befehl === "hook") {
    let eingabe = ""
    try {
      eingabe = readFileSync(0, "utf8")
    } catch {}
    const text = hookAusgabe(eingabe)
    if (text) process.stdout.write(text)
  } else if (befehl === "herdr") {
    process.exitCode = herdrCli(args)
  } else {
    console.error("Aufruf: node skill-phase.mjs hook | herdr <nr> <phase> [<status>]")
    process.exitCode = 2
  }
}
