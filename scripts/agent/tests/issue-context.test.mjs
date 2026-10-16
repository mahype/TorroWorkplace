// Aufruf: node --test tests/Scripts/issue-context.test.mjs
import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { mkdtempSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import { test } from "node:test"
import { fileURLToPath } from "node:url"
import { formatiereKontext, issueNummerAusBranch, ladeKontext } from "../issue-context.mjs"

// Git-Hooks (pre-push) exportieren GIT_DIR und Co.; die Temp-Repos der Tests zeigten sonst auf das echte Repo.
for (const k of Object.keys(process.env)) if (k.startsWith("GIT_")) delete process.env[k]

const skript = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../issue-context.mjs")

test("Issue-Nummer kommt aus dem Branch-Namen", () => {
  assert.equal(issueNummerAusBranch("task/2917-session-start-hook"), "2917")
  assert.equal(issueNummerAusBranch("bug/12-x"), "12")
  assert.equal(issueNummerAusBranch("main"), null)
  assert.equal(issueNummerAusBranch("HEAD"), null)
  assert.equal(issueNummerAusBranch(undefined), null)
})

test("Titel mit Sonderzeichen bleibt unveraendert, nur die letzten 10 Kommentare", () => {
  const kommentare = Array.from({ length: 12 }, (_, i) => ({
    author: { login: "a" }, createdAt: "2026-10-08T10:00:00Z", body: `Kommentar ${i}`,
  }))
  const text = formatiereKontext("5", "task/5-x", {
    title: "Titel mit ' und \\ und \"", body: " Beschreibung ", labels: [{ name: "ai" }],
    assignees: [], milestone: { title: "KW 41/26" }, comments: kommentare,
  })
  assert.match(text, /^# Issue #5: Titel mit ' und \\ und "/)
  assert.match(text, /## Labels\nai/)
  assert.doesNotMatch(text, /## Assignees/)
  assert.match(text, /## Milestone\nKW 41\/26/)
  assert.match(text, /## Description\n\nBeschreibung\n/)
  assert.doesNotMatch(text, /Kommentar 1\n/)
  assert.match(text, /Kommentar 2\n/)
  assert.match(text, /Kommentar 11\n/)
})

function lauf(branch, pfad) {
  const repo = mkdtempSync(path.join(tmpdir(), "issue-context-"))
  try {
    for (const args of [["init", "-q", "-b", branch], ["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "x"]]) {
      assert.equal(spawnSync("git", args, { cwd: repo }).status, 0)
    }
    const env = { ...process.env, CLAUDE_PROJECT_DIR: repo }
    if (pfad !== undefined) {
      delete env.Path // Windows: sonst gewinnt die Gross-/Kleinschreibung unvorhersehbar
      env.PATH = pfad
    }
    return spawnSync(process.execPath, [skript, "claude"], { env, encoding: "utf8", cwd: repo, windowsHide: true })
  } finally {
    rmSync(repo, { recursive: true, force: true })
  }
}

test("Branch ohne Issue-Nummer: keine Ausgabe, Exit 0", () => {
  const r = lauf("main")
  assert.equal(r.status, 0)
  assert.equal(r.stdout, "")
})

test("gh nicht ausfuehrbar: Warnung als systemMessage, Exit 0, kein Kontext", () => {
  // git bleibt auffindbar, gh nicht: PATH nur mit dem Verzeichnis von git.
  const gitDir = path.dirname(spawnSync(process.platform === "win32" ? "where" : "which", ["git"], { encoding: "utf8" }).stdout.split(/\r?\n/)[0])
  const r = lauf("task/2917-x", gitDir)
  assert.equal(r.status, 0)
  const ausgabe = JSON.parse(r.stdout)
  assert.match(ausgabe.systemMessage, /#2917 nicht geladen/)
  assert.equal(ausgabe.hookSpecificOutput, undefined)
})

test("Sehr lange Beschreibung und Kommentare werden gekuerzt", () => {
  const lang = "x".repeat(25_000)
  const text = formatiereKontext("5", "task/5-x", { title: "T", body: lang, comments: [{ author: { login: "a" }, createdAt: "2026-10-08", body: lang }] })
  assert.equal(text.match(/gekuerzt, vollstaendig: https:\/\/github\.com\/mahype\/TorroWorkplace\/issues\/5/g).length, 2)
  assert.ok(text.length < 45_000)
})

test("Erfolgspfad: gh-Antwort wird zu Kontext", () => {
  const aufrufe = []
  const fake = (befehl, args) => {
    aufrufe.push([befehl, ...args])
    if (befehl === "git") return { status: 0, stdout: "task/2917-session-start-hook\n" }
    return { status: 0, stdout: JSON.stringify({ title: "Hook", body: "Text", labels: [{ name: "ai" }], comments: [] }) }
  }
  const { kontext, warnung } = ladeKontext("/irgendwo", fake)
  assert.equal(warnung, undefined)
  assert.match(kontext, /^# Issue #2917: Hook/)
  assert.deepEqual(aufrufe[1].slice(0, 4), ["gh", "issue", "view", "2917"])
})
