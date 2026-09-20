.pragma library

function parseJson(raw) {
  var text = String(raw || "").trim()
  if (text === "") return null
  try {
    return JSON.parse(text)
  } catch (error) {
    // Long-running commands may emit JSON Lines. The final complete object is
    // the useful result, while status polling supplies live progress.
    var lines = text.split("\n")
    for (var i = lines.length - 1; i >= 0; i--) {
      try { return JSON.parse(lines[i]) } catch (ignored) {}
    }
  }
  return null
}

function asArray(value) {
  if (Array.isArray(value)) return value
  if (!value || typeof value !== "object") return []
  var result = []
  for (var key in value) result.push(value[key])
  return result
}

function firstDefined(object, keys, fallback) {
  if (!object) return fallback
  for (var i = 0; i < keys.length; i++) {
    var value = object[keys[i]]
    if (value !== undefined && value !== null) return value
  }
  return fallback
}

function count(value) {
  if (Array.isArray(value)) return value.length
  var number = Number(value)
  return isFinite(number) ? number : 0
}

function normalizeProfile(profile, index) {
  var source = profile || {}
  var name = String(firstDefined(source, ["name", "title", "id", "slug"], "Profile " + (index + 1)))
  return {
    id: String(firstDefined(source, ["id", "slug", "name"], name)),
    name: name,
    workspaces: count(firstDefined(source, ["workspaceCount", "workspaces"], 0)),
    windows: count(firstDefined(source, ["windowCount", "windows"], 0)),
    unrestorable: count(firstDefined(source, ["unrestorableCount", "unrestorable"], 0)),
    updatedAt: String(firstDefined(source, ["updatedAt", "modifiedAt", "updated"], "")),
    isDefault: source.isDefault === true || source.default === true || source.startup === true,
    isLast: source.isLast === true || source.last === true || source.lastUsed === true,
    raw: source
  }
}

function normalizeStartup(source) {
  var root = source || {}
  var config = root.config && typeof root.config === "object" ? root.config : root
  var startup = config.startup && typeof config.startup === "object" ? config.startup : config
  var mode = String(firstDefined(startup, ["mode", "startupMode", "startup"], "none")).toLowerCase()
  if (mode === "fixed") mode = "default"
  else if (mode === "ask") mode = "chooser"
  if (["none", "default", "last", "chooser"].indexOf(mode) < 0) mode = "none"
  return {
    mode: mode,
    defaultProfile: String(firstDefined(startup, ["name", "profile", "defaultProfile", "defaultProfileName", "startupProfile"],
      firstDefined(config, ["defaultProfile", "defaultProfileName", "startupProfile"], "")) || ""),
    lastProfile: String(firstDefined(startup, ["lastProfile", "lastProfileName"],
      firstDefined(config, ["lastProfile", "lastProfileName"], "")))
  }
}

function parseList(raw) {
  var data = parseJson(raw)
  if (data === null) return { ok: false, profiles: [], startup: normalizeStartup(null) }
  var values = Array.isArray(data) ? data : asArray(firstDefined(data, ["profiles", "setups", "items"], []))
  var profiles = []
  for (var i = 0; i < values.length; i++) profiles.push(normalizeProfile(values[i], i))
  profiles.sort(function(a, b) { return a.name.toLowerCase().localeCompare(b.name.toLowerCase()) })
  return { ok: true, profiles: profiles, startup: Array.isArray(data) ? null : normalizeStartup(data), raw: data }
}

function normalizeProgressRows(data) {
  var operation = data && data.restore && typeof data.restore === "object" ? data.restore : (data || {})
  var rows = asArray(firstDefined(operation, ["items", "windows", "progress", "results"], []))
  var result = []
  for (var i = 0; i < rows.length; i++) {
    var row = rows[i] || {}
    result.push({
      label: String(firstDefined(row, ["label", "name", "application", "title", "id"], "Window " + (i + 1))),
      detail: String(firstDefined(row, ["detail", "message", "error", "workspace"], "")),
      state: String(firstDefined(row, ["state", "status"], "pending")).toLowerCase()
    })
  }
  return result
}

function parseStatus(raw) {
  var data = parseJson(raw)
  if (data === null) return { ok: false }
  var operation = data.restore && typeof data.restore === "object" ? data.restore : data
  var phase = String(firstDefined(operation, ["phase", "state", "status"], "idle")).toLowerCase()
  var active = operation.state === "running"
    || firstDefined(operation, ["active", "running", "busy"], false) === true
    || ["loading", "restoring", "starting", "running", "preparing", "closing", "launching", "arranging", "canceling"].indexOf(phase) >= 0
  var completed = Number(firstDefined(operation, ["completed", "restored", "done"], 0))
  var total = Number(firstDefined(operation, ["total", "expected", "windowCount"], 0))
  var warnings = asArray(operation.monitorWarnings)
  var message = String(firstDefined(operation, ["message", "statusText", "error"], ""))
  if (message === "" && warnings.length > 0) message = warnings.join(" · ")
  return {
    ok: true,
    active: active,
    phase: phase,
    profile: String(firstDefined(operation, ["profile", "profileName", "name"], "")),
    message: message,
    completed: isFinite(completed) ? completed : 0,
    total: isFinite(total) ? total : 0,
    rows: normalizeProgressRows(data),
    startup: (data.config || data.startup || data.startupMode !== undefined) ? normalizeStartup(data) : null,
    raw: data
  }
}

function localPath(url) {
  var value = String(url || "")
  if (value.indexOf("file://") === 0) value = value.substring(7)
  try { return decodeURIComponent(value) } catch (error) { return value }
}

function compactError(text, fallback) {
  var value = String(text || "").replace(/\s+/g, " ").trim()
  if (value === "") value = fallback || "Backend command failed"
  return value.length > 180 ? value.substring(0, 177) + "..." : value
}

function profileLabel(profile) {
  if (!profile) return ""
  var parts = []
  parts.push(profile.workspaces + " workspace" + (profile.workspaces === 1 ? "" : "s"))
  parts.push(profile.windows + " window" + (profile.windows === 1 ? "" : "s"))
  if (profile.unrestorable > 0)
    parts.push(profile.unrestorable + " cannot be relaunched")
  return parts.join(" · ")
}

function progressGlyph(state) {
  var value = String(state || "").toLowerCase()
  if (["done", "complete", "completed", "success", "restored"].indexOf(value) >= 0) return "✓"
  if (["failed", "error", "timeout", "skipped"].indexOf(value) >= 0) return "!"
  if (["running", "launching", "matching", "placing"].indexOf(value) >= 0) return "●"
  return "·"
}

function snapshotWarnings(snapshot) {
  var windows = snapshot && Array.isArray(snapshot.windows) ? snapshot.windows : []
  var workspaces = snapshot && Array.isArray(snapshot.workspaces) ? snapshot.workspaces : []
  var unresolved = 0
  for (var i = 0; i < windows.length; i++)
    if (!Array.isArray(windows[i].command) || windows[i].command.length === 0) unresolved++

  function hasFallback(node) {
    if (!node || typeof node !== "object") return false
    if (Array.isArray(node.fallback)) return true
    return hasFallback(node.first) || hasFallback(node.second)
  }

  var fallback = 0
  for (var j = 0; j < workspaces.length; j++)
    if (hasFallback(workspaces[j].tree)) fallback++
  return { unresolved: unresolved, fallback: fallback }
}
