// Aufruf: node --test tests/Scripts/skill-phase.test.mjs
import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { mkdtempSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import { test } from "node:test"
import { fileURLToPath } from "node:url"
import { hookAusgabe, phaseAusPrompt, setzeHerdr } from "../skill-phase.mjs"

const skript = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../skill-phase.mjs")

// Ohne Herdr-Variablen, damit ein Test nie das echte Herdr anspricht.
function ohneHerdr() {
  const env = { ...process.env }
  for (const k of Object.keys(env)) if (k.startsWith("HERDR_")) delete env[k]
  return env
}

function cli(args, eingabe) {
  return spawnSync(process.execPath, [skript, ...args], { input: eingabe, encoding: "utf8", env: ohneHerdr() })
}

test("Slash-Befehle der Phasen werden erkannt, sonst nichts", () => {
  assert.deepEqual(phaseAusPrompt("/issue-plan 2996"), { phase: "plan", nr: "2996" })
  assert.deepEqual(phaseAusPrompt("  /issue-approve #12"), { phase: "approve", nr: "12" })
  assert.deepEqual(phaseAusPrompt("/issue-implement 7 bitte mit Opus"), { phase: "implement", nr: "7" })
  assert.deepEqual(phaseAusPrompt("/issue-small 3085"), { phase: "small", nr: "3085" })
  assert.deepEqual(phaseAusPrompt("/quick-fix Tippfehler im Impressum"), { phase: "quick", nr: null })
  assert.deepEqual(phaseAusPrompt("  /quick-fix"), { phase: "quick", nr: null })
  assert.equal(phaseAusPrompt("/quick-fixes"), null)
  assert.equal(phaseAusPrompt("/quick-fix-alt"), null)
  assert.equal(phaseAusPrompt("bitte /quick-fix"), null)
  assert.equal(phaseAusPrompt("/issue-small"), null)
  assert.equal(phaseAusPrompt("setz #123 um"), null)
  assert.equal(phaseAusPrompt("bitte /issue-plan 5"), null)
  assert.equal(phaseAusPrompt("/issue-create 1"), null)
  assert.equal(phaseAusPrompt("/issue-plan"), null)
  assert.equal(phaseAusPrompt("/issue-plan 12a"), null)
  assert.equal(phaseAusPrompt(undefined), null)
})

test("Hook-Ausgabe setzt sessionTitle nur bei Treffer, auch über einen eigenen Titel", () => {
  const aus = JSON.parse(hookAusgabe(JSON.stringify({ prompt: "/issue-plan 42", session_title: "Mein Titel" })))
  assert.deepEqual(aus, { hookSpecificOutput: { hookEventName: "UserPromptSubmit", sessionTitle: "📋 Planning #42" } })
  assert.equal(hookAusgabe(JSON.stringify({ prompt: "hallo" })), "")
  assert.equal(hookAusgabe("{kaputt"), "")
  assert.equal(hookAusgabe(""), "")

  const quick = JSON.parse(hookAusgabe(JSON.stringify({ prompt: "/quick-fix Link im Footer" })))
  assert.equal(quick.hookSpecificOutput.sessionTitle, "⚡ Quick-Fix")
})

test("CLI hook: Treffer, kein Treffer, kaputtes JSON enden mit Exit 0", () => {
  const treffer = cli(["hook"], JSON.stringify({ prompt: "/issue-implement 7" }))
  assert.equal(treffer.status, 0)
  assert.equal(JSON.parse(treffer.stdout).hookSpecificOutput.sessionTitle, "⚙️ Implementation #7")

  for (const eingabe of [JSON.stringify({ prompt: "hallo" }), "{kaputt", ""]) {
    const r = cli(["hook"], eingabe)
    assert.equal(r.status, 0)
    assert.equal(r.stdout, "")
  }
})

test("CLI herdr: ohne Herdr Exit 0, falscher Aufruf Exit 2", () => {
  const r = cli(["herdr", "5", "plan", "planning"])
  assert.equal(r.status, 0)
  assert.equal(r.stdout + r.stderr, "")
  assert.equal(cli(["herdr", "5", "review"]).status, 2)
  assert.equal(cli(["herdr", "5", "plan", "fertig"]).status, 2)
  assert.equal(cli(["herdr", "x", "plan"]).status, 2)
  assert.equal(cli(["herdr", "-", "plan"]).status, 2)
})

test("CLI herdr: quick nur mit `-` und ohne Status", () => {
  const r = cli(["herdr", "-", "quick"])
  assert.equal(r.status, 0)
  assert.equal(r.stdout + r.stderr, "")
  assert.equal(cli(["herdr", "5", "quick"]).status, 2)
  assert.equal(cli(["herdr", "-", "quick", "implementing"]).status, 2)
})

test("CLI herdr: in Herdr, aber herdr nicht im PATH: Exit 0, still", () => {
  // PATH nur mit einem leeren Verzeichnis: der echte spawnSync-Aufruf findet kein herdr.
  const leer = mkdtempSync(path.join(tmpdir(), "skill-phase-"))
  const env = { ...ohneHerdr(), HERDR_ENV: "1", HERDR_PANE_ID: "w1:p2", PATH: leer }
  delete env.Path
  try {
    const r = spawnSync(process.execPath, [skript, "herdr", "5", "plan", "planning"], { encoding: "utf8", env, cwd: leer })
    assert.equal(r.status, 0)
    assert.equal(r.stdout + r.stderr, "")
  } finally {
    rmSync(leer, { recursive: true, force: true })
  }
})

test("herdr: Fehler ohne stderr nennt stdout", () => {
  const { ausfuehren } = attrappe([], { pane: { status: 1, stderr: "", stdout: '{"error":"kaputt"}' } })
  const w = setzeHerdr({ nr: "5", phase: "plan", env: herdrEnv, cwd: "/repo" }, ausfuehren)
  assert.match(w[0], /kaputt/)
})

// Attrappe fuer herdr: protokolliert die Aufrufe, `worktree list` liefert `worktrees`.
function attrappe(worktrees, fehler = {}) {
  const aufrufe = []
  const ausfuehren = (befehl, args) => {
    aufrufe.push([befehl, ...args].join(" "))
    if (fehler[args[0]]) return fehler[args[0]]
    if (args[0] === "worktree") return { status: 0, stdout: JSON.stringify({ result: { worktrees } }) }
    return { status: 0, stdout: "{}" }
  }
  return { aufrufe, ausfuehren }
}

const herdrEnv = { HERDR_ENV: "1", HERDR_PANE_ID: "w1:p2" }
const worktrees = [
  { branch: "main", open_workspace_id: "wA" },
  { branch: "task/5-skill-phase", open_workspace_id: "wB" },
  { branch: "task/55-anderes", open_workspace_id: "wC" },
]

test("herdr: Pane-Anzeige und Status am Workspace des Issue-Branches", () => {
  const { aufrufe, ausfuehren } = attrappe(worktrees)
  const w = setzeHerdr({ nr: "5", phase: "plan", status: "awaiting-approval", env: herdrEnv, cwd: "/repo" }, ausfuehren)
  assert.deepEqual(w, [])
  assert.deepEqual(aufrufe, [
    "herdr pane report-metadata w1:p2 --source torro-skill-phase --display-agent 📋 Planning #5",
    "herdr worktree list --cwd /repo",
    "herdr workspace report-metadata wB --source torro-skill-phase --token status=awaiting approval",
  ])
})

test("herdr: ohne Status nur die Pane-Anzeige", () => {
  const { aufrufe, ausfuehren } = attrappe(worktrees)
  setzeHerdr({ nr: "5", phase: "approve", env: herdrEnv, cwd: "/repo" }, ausfuehren)
  assert.deepEqual(aufrufe, ["herdr pane report-metadata w1:p2 --source torro-skill-phase --display-agent 🚦 Approval #5"])
})

test("herdr: Phase small zeigt 🎯 Small", () => {
  const { aufrufe, ausfuehren } = attrappe(worktrees)
  setzeHerdr({ nr: "5", phase: "small", env: herdrEnv, cwd: "/repo" }, ausfuehren)
  assert.deepEqual(aufrufe, ["herdr pane report-metadata w1:p2 --source torro-skill-phase --display-agent 🎯 Small #5"])
})

test("herdr: Phase quick zeigt ⚡ Quick-Fix ohne Nummer und ohne Workspace-Suche", () => {
  const { aufrufe, ausfuehren } = attrappe(worktrees)
  setzeHerdr({ nr: null, phase: "quick", status: "implementing", env: herdrEnv, cwd: "/repo" }, ausfuehren)
  assert.deepEqual(aufrufe, ["herdr pane report-metadata w1:p2 --source torro-skill-phase --display-agent ⚡ Quick-Fix"])
})

test("herdr: kein offener Workspace fuer den Branch, kein Status", () => {
  const { aufrufe, ausfuehren } = attrappe([
    { branch: "task/55-anderes", open_workspace_id: "wC" },
    { branch: "task/5-skill-phase" },
  ])
  const w = setzeHerdr({ nr: "5", phase: "implement", status: "implementing", env: herdrEnv, cwd: "/repo" }, ausfuehren)
  assert.deepEqual(w, [])
  assert.equal(aufrufe.some((a) => a.startsWith("herdr workspace")), false)
})

test("herdr: ohne HERDR_ENV kein Aufruf, ohne herdr im PATH still", () => {
  const ohne = attrappe(worktrees)
  assert.deepEqual(setzeHerdr({ nr: "5", phase: "plan", status: "planning", env: {}, cwd: "/repo" }, ohne.ausfuehren), [])
  assert.deepEqual(ohne.aufrufe, [])

  const fehlt = attrappe(worktrees, { pane: { error: { code: "ENOENT", message: "spawn herdr ENOENT" } } })
  assert.deepEqual(setzeHerdr({ nr: "5", phase: "plan", status: "planning", env: herdrEnv, cwd: "/repo" }, fehlt.ausfuehren), [])
  assert.equal(fehlt.aufrufe.length, 1)
})

test("herdr: Fehler werden Warnungen, kein Abbruch", () => {
  const { ausfuehren } = attrappe(worktrees, {
    pane: { status: 1, stderr: "pane not found" },
    worktree: { status: 0, stdout: "kein json" },
  })
  const w = setzeHerdr({ nr: "5", phase: "plan", status: "planning", env: herdrEnv, cwd: "/repo" }, ausfuehren)
  assert.equal(w.length, 2)
  assert.match(w[0], /pane not found/)
  assert.match(w[1], /ungueltige Antwort/)
})
