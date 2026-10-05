.pragma library

function parseJson(raw) {
  var text = String(raw || "").trim()
  if (text === "") return null
  try {
    return JSON.parse(text)
  } catch (error) {
    var lines = text.split("\n")
    for (var i = lines.length - 1; i >= 0; i--) {
      try { return JSON.parse(lines[i]) } catch (ignored) {}
    }
  }
  return null
}

function normalizeTemplate(source) {
  var value = source || {}
  var windows = Array.isArray(value.windows) ? value.windows : []
  return {
    id: String(value.id || ""),
    name: String(value.name || value.id || ""),
    windowCount: Number(value.windowCount || windows.length || 0),
    unrestorable: Number(value.unrestorableCount || 0),
    shortcut: value.shortcut === null || value.shortcut === undefined ? 0 : Number(value.shortcut),
    restoreOnStartup: value.restoreOnStartup === true,
    startupWorkspace: String(value.startupWorkspace || ""),
    sourceWorkspace: String(value.sourceWorkspace || ""),
    windows: windows.map(function(window) {
      return {
        id: String(window.id || ""),
        application: String(window.application || window.id || ""),
        strategy: String(window.strategy || "auto"),
        restorable: window.restorable !== false,
        browser: window.browser === true,
        urls: Array.isArray(window.urls) ? window.urls.map(String) : []
      }
    })
  }
}

function parseList(raw) {
  var data = parseJson(raw)
  if (!data || !Array.isArray(data.templates)) return { ok: false }
  var templates = data.templates.map(normalizeTemplate)
  templates.sort(function(a, b) {
    if ((a.shortcut || 99) !== (b.shortcut || 99)) return (a.shortcut || 99) - (b.shortcut || 99)
    return a.name.toLowerCase().localeCompare(b.name.toLowerCase())
  })
  return {
    ok: true, templates: templates,
    language: String(data.language || "auto"),
    bindingsInstalled: data.bindingsInstalled === true
  }
}

function parseStatus(raw) {
  var data = parseJson(raw)
  if (data === null) return { ok: false }
  var rows = Array.isArray(data.windows) ? data.windows : []
  var warnings = Array.isArray(data.warnings) ? data.warnings : []
  return {
    ok: true,
    active: data.state === "running",
    state: String(data.state || "idle"),
    phase: String(data.phase || data.state || "idle"),
    runId: String(data.runId || ""),
    template: String(data.template || ""),
    message: String(data.message || warnings.join(" · ")),
    completed: Number(data.completed || 0),
    total: Number(data.total || 0),
    rows: rows.map(function(row) {
      return {
        label: String(row.application || row.id || ""),
        detail: String(row.error || ""),
        state: String(row.state || "waiting")
      }
    })
  }
}

function localPath(url) {
  var value = String(url || "")
  if (value.indexOf("file://") === 0) value = value.substring(7)
  try { return decodeURIComponent(value) } catch (error) { return value }
}

function compactError(text, fallback) {
  var data = parseJson(text)
  var value = data && data.error ? String(data.error) : String(text || "").replace(/\s+/g, " ").trim()
  if (value === "") value = fallback || "Backend command failed"
  return value.length > 180 ? value.substring(0, 177) + "..." : value
}

function progressGlyph(state) {
  var value = String(state || "").toLowerCase()
  if (["restored", "moved", "completed"].indexOf(value) >= 0) return "✓"
  if (["failed", "error", "skipped"].indexOf(value) >= 0) return "!"
  if (value === "waiting") return "●"
  return "·"
}

// Fn is optional: many keyboards send media keys on the F-row unless it is held.
function slotLabel(slot) {
  return "SUPER + (Fn) + F" + slot
}

// Options for a template's shortcut chip; keys held by another template name
// their owner so moving a shortcut is never a surprise.
function slotOptions(noneLabel, templates, ownName) {
  var owners = {}
  ;(templates || []).forEach(function(template) {
    if (template.shortcut > 0) owners[template.shortcut] = template.name
  })
  var options = [{ value: "0", label: noneLabel }]
  for (var i = 1; i <= 10; i++) {
    var owner = owners[i] && owners[i] !== ownName ? " · " + owners[i] : ""
    options.push({ value: String(i), label: slotLabel(i) + owner })
  }
  return options
}

function assignedSlots(templates) {
  return (templates || [])
    .filter(function(template) { return template.shortcut > 0 })
    .map(function(template) { return { slot: template.shortcut, name: template.name } })
    .sort(function(a, b) { return a.slot - b.slot })
}

function firstFreeSlot(templates) {
  var taken = assignedSlots(templates).map(function(entry) { return entry.slot })
  for (var i = 1; i <= 10; i++) if (taken.indexOf(i) < 0) return i
  return 0
}

function workspaceOptions() {
  var options = []
  for (var i = 1; i <= 10; i++) options.push({ value: String(i), label: String(i) })
  return options
}
