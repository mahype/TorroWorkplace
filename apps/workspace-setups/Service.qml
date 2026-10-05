import QtQuick
import Quickshell
import Quickshell.Io
import "Model.js" as Model
import "I18n.js" as I18n

Item {
  id: root

  property var shell: null
  property var manifest: null
  readonly property string pluginId: "io.github.mahype.workspace-setups"
  readonly property string backendPath: Model.localPath(Qt.resolvedUrl("bin/workspace-setups"))
  readonly property bool backendAvailable: backendPath !== "" && backendPath !== "bin/workspace-setups"

  property var templates: []
  property string languageSetting: "auto"
  readonly property string language: I18n.resolve(languageSetting,
    Quickshell.env("LC_ALL") || Quickshell.env("LC_MESSAGES") || Quickshell.env("LANG") || Qt.locale().name)
  property bool bindingsInstalled: true
  property bool refreshing: false

  property bool actionRunning: actionProcess.running
  property var currentRequest: null
  property bool restoreActive: false
  property string restorePhase: "idle"
  property string restoreTemplate: ""
  property int restoreCompleted: 0
  property int restoreTotal: 0
  property var progressRows: []
  property string statusMessage: ""
  property string lastError: ""

  // A backend refusal the user resolves in the panel: { message, options, request, code }.
  property var pendingChoice: null

  property bool startupHandled: false
  property int readinessAttempts: 0
  property int readinessStableTicks: 0

  readonly property bool busy: actionRunning || restoreActive

  function tr(key, args) { return I18n.tr(language, key, args) }

  function refresh() {
    if (!listProcess.running) {
      refreshing = true
      listProcess.running = true
    }
    refreshStatus()
  }

  function refreshStatus() {
    if (!statusProcess.running) statusProcess.running = true
  }

  // `request.args` is the backend command; `label` names the template for
  // messages; `notify` reports the result as a desktop notification because
  // the action came from a shortcut rather than the open panel.
  function run(args, label, notify, message) {
    if (!backendAvailable || actionProcess.running || (restoreActive && args[0] !== "cancel")) return false
    lastError = ""
    statusMessage = message || tr("status.working")
    currentRequest = { args: args, label: String(label || ""), notify: notify === true }
    actionProcess.command = [backendPath].concat(args)
    actionProcess.running = true
    return true
  }

  function saveNew(name) {
    var value = String(name || "").trim()
    return value !== "" && run(["save", value], value, false, tr("status.saving", { name: value }))
  }

  function updateTemplate(name) {
    return run(["update", name], name, false, tr("status.saving", { name: name }))
  }

  function loadTemplate(name, mode) {
    return run(["load", name, "--mode", mode || "ask"], name, false, tr("status.loading", { name: name }))
  }

  function loadSlot(slot) {
    return run(["load-slot", String(slot), "--mode", "ask"], Model.slotLabel(slot), true, tr("status.loading", { name: Model.slotLabel(slot) }))
  }

  function saveSlot(slot) {
    return run(["save-slot", String(slot)], Model.slotLabel(slot), true, tr("status.saving", { name: Model.slotLabel(slot) }))
  }

  function renameTemplate(oldName, newName) {
    var value = String(newName || "").trim()
    return value !== "" && value !== oldName && run(["rename", oldName, value], value, false)
  }

  function requestDelete(name) {
    pendingChoice = {
      code: "delete", request: ["delete", name],
      message: tr("choice.delete", { template: name }),
      options: [{ id: "confirm", label: tr("action.delete"), destructive: true }]
    }
  }

  function setShortcut(name, slot) {
    return run(["set-shortcut", name, Number(slot) > 0 ? String(slot) : "none"], name, false)
  }

  function setStartup(name, enabled, workspace) {
    var args = ["set-startup", name, enabled ? "on" : "off"]
    if (workspace) args.push("--workspace", String(workspace))
    return run(args, name, false)
  }

  function setStrategy(name, windowId, strategy) {
    return run(["set-strategy", name, windowId, strategy], name, false)
  }

  function setUrls(name, windowId, text) {
    var urls = String(text || "").split(/\s+/).filter(function(url) { return url !== "" })
    return run(["set-urls", name, windowId, "--"].concat(urls), name, false)
  }

  function setLanguage(value) {
    languageSetting = value
    return run(["set-language", value], "", false)
  }

  function installBindings() {
    return run(["install-bindings"], "", false)
  }

  function cancelRestore() {
    if (cancelProcess.running || (!restoreActive && !actionProcess.running)) return false
    cancelProcess.running = true
    return true
  }

  function offerChoice(error, request) {
    var code = String(error.code || "")
    var args = request.args
    var choice = null
    if (code === "workspace_not_empty") {
      choice = {
        message: tr("choice.notEmpty", { workspace: error.workspace, windows: error.windows, template: error.template }),
        options: [
          { id: "replace", label: tr("choice.replace"), destructive: true },
          { id: "add", label: tr("choice.add"), destructive: false }
        ]
      }
    } else if (code === "slot_taken" && args[0] === "save-slot") {
      choice = {
        message: tr("choice.slotTaken", { slot: error.slot, template: error.template }),
        options: [{ id: "confirm", label: tr("choice.overwrite"), destructive: true }]
      }
    } else if (code === "slot_taken" && args[0] === "set-shortcut") {
      choice = {
        message: tr("choice.assignTaken", { slot: error.slot, template: error.template, target: args[1] }),
        options: [{ id: "confirm", label: tr("choice.move"), destructive: false }]
      }
    } else if (code === "exists") {
      choice = {
        message: tr("choice.exists", { template: error.template }),
        options: [{ id: "confirm", label: tr("choice.overwrite"), destructive: true }]
      }
    }
    if (!choice) return false
    choice.code = code
    choice.request = args
    choice.notify = request.notify
    pendingChoice = choice
    statusMessage = ""
    // Shortcut-triggered actions have no open panel yet.
    if (shell && typeof shell.summon === "function") shell.summon(pluginId, "")
    return true
  }

  function resolveChoice(optionId) {
    var choice = pendingChoice
    pendingChoice = null
    if (!choice || optionId === "cancel") return
    var args = choice.request.slice()
    if (choice.code === "workspace_not_empty") {
      var index = args.indexOf("--mode")
      args[index + 1] = optionId
    } else if (choice.code === "exists") {
      args = ["update", args[1]]
    } else if (choice.code === "slot_taken") {
      args.push("--force")
    }
    run(args, args[1], choice.notify === true)
  }

  function notify(text, urgent) {
    Quickshell.execDetached(["notify-send", "-a", tr("title"), "-u", urgent ? "critical" : "normal", "-t", "4000", text])
  }

  function finishAction(exitCode, stdout, stderr) {
    var request = currentRequest || { args: [], label: "", notify: false }
    currentRequest = null
    var command = String(request.args[0] || "")
    if (exitCode !== 0) {
      var error = Model.parseJson(stderr) || {}
      if (error.code && offerChoice(error, request)) return
      lastError = error.code === "slot_empty"
        ? tr("status.slotEmpty", { slot: error.slot })
        : Model.compactError(stderr || stdout)
      statusMessage = ""
      if (request.notify) notify(lastError, true)
      return
    }
    var result = Model.parseJson(stdout) || {}
    var message = tr("status.updated")
    if ((command === "save" || command === "update" || command === "save-slot") && Array.isArray(result.windows)) {
      var unresolved = result.windows.filter(function(window) { return !window.command || window.command.length === 0 }).length
      message = unresolved > 0
        ? tr("status.savedUnresolved", { name: result.name, count: unresolved })
        : tr("status.saved", { name: result.name })
    } else if (command === "load" || command === "load-slot") {
      message = tr("status.restore." + (result.state || "completed"), { name: result.template || request.label })
    } else if (command === "install-bindings") {
      message = tr("status.bindings", { slot: Model.firstFreeSlot(templates) || 1 })
    }
    // The silent login restore should not leave a message in the panel.
    statusMessage = command === "startup" ? "" : message
    lastError = ""
    if (request.notify) notify(message, result.state === "partial")
  }

  function handleStartup() {
    if (startupHandled || !backendAvailable) return
    readinessTimer.start()
  }

  IpcHandler {
    target: "workspace-setups"
    function loadSlot(slot: string): string { return root.loadSlot(Number(slot)) ? "ok" : "busy" }
    function saveSlot(slot: string): string { return root.saveSlot(Number(slot)) ? "ok" : "busy" }
    function refresh(): string { root.refresh(); return "ok" }
  }

  Process {
    id: listProcess
    command: [root.backendPath, "list", "--json"]
    stdout: StdioCollector { id: listStdout; waitForEnd: true }
    stderr: StdioCollector { id: listStderr; waitForEnd: true }
    onExited: function(exitCode) {
      root.refreshing = false
      if (exitCode !== 0) {
        root.lastError = Model.compactError(listStderr.text || listStdout.text, root.tr("error.list"))
        return
      }
      var result = Model.parseList(listStdout.text)
      if (!result.ok) {
        root.lastError = root.tr("error.invalid")
        return
      }
      root.templates = result.templates
      root.languageSetting = result.language
      root.bindingsInstalled = result.bindingsInstalled
    }
  }

  Process {
    id: statusProcess
    command: [root.backendPath, "status", "--json"]
    stdout: StdioCollector { id: statusStdout; waitForEnd: true }
    onExited: function(exitCode) {
      if (exitCode !== 0) return
      var result = Model.parseStatus(statusStdout.text)
      if (!result.ok) return
      root.restoreActive = result.active
      root.restorePhase = result.phase
      root.restoreTemplate = result.template
      root.restoreCompleted = result.completed
      root.restoreTotal = result.total
      root.progressRows = result.active || root.actionRunning ? result.rows : []
    }
  }

  Process {
    id: actionProcess
    command: []
    stdout: StdioCollector { id: actionStdout; waitForEnd: true }
    stderr: StdioCollector { id: actionStderr; waitForEnd: true }
    onExited: function(exitCode) {
      root.finishAction(exitCode, actionStdout.text, actionStderr.text)
      root.refresh()
    }
  }

  Process {
    id: cancelProcess
    command: [root.backendPath, "cancel"]
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { waitForEnd: true }
    onExited: root.refreshStatus()
  }

  // Startup templates wait until Hyprland reports stable outputs; the backend
  // itself makes sure this only happens once per Hyprland session.
  Process {
    id: readinessProcess
    command: ["hyprctl", "-j", "monitors"]
    stdout: StdioCollector { id: readinessStdout; waitForEnd: true }
    onExited: function(exitCode) {
      var monitors = exitCode === 0 ? Model.parseJson(readinessStdout.text) : null
      if (Array.isArray(monitors) && monitors.length > 0) {
        root.readinessStableTicks += 1
        if (root.readinessStableTicks >= 3 && !root.busy) {
          readinessTimer.stop()
          root.startupHandled = true
          root.run(["startup"], "", false)
        }
      } else if (root.readinessAttempts >= 60) {
        readinessTimer.stop()
        root.startupHandled = true
      } else root.readinessStableTicks = 0
    }
  }

  Timer {
    id: readinessTimer
    interval: 1000
    repeat: true
    onTriggered: {
      root.readinessAttempts += 1
      if (!readinessProcess.running) readinessProcess.running = true
    }
  }

  Timer {
    interval: root.busy ? 750 : 5000
    repeat: true
    running: true
    onTriggered: root.refreshStatus()
  }

  Component.onCompleted: {
    refresh()
    handleStartup()
  }
}
