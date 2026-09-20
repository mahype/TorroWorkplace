import QtQuick
import Quickshell
import Quickshell.Io
import "Model.js" as Model

Item {
  id: root

  property var shell: null
  property var manifest: null
  readonly property string pluginId: "io.github.mahype.workspace-setups"
  readonly property string backendPath: Model.localPath(Qt.resolvedUrl("bin/workspace-setups"))
  readonly property string configHome: Quickshell.env("XDG_CONFIG_HOME") || (Quickshell.env("HOME") + "/.config")
  readonly property string configPath: configHome + "/workspace-setups/config.json"

  property var profiles: []
  property string startupMode: "none"
  property string defaultProfile: ""
  property string lastProfile: ""
  property bool configurationLoaded: false
  property bool refreshing: false
  property bool actionRunning: actionProcess.running
  property string currentAction: ""
  property string inspectionName: ""
  property string inspectionExisting: ""
  property bool restoreActive: false
  property string restorePhase: "idle"
  property string restoreProfile: ""
  property int restoreCompleted: 0
  property int restoreTotal: 0
  property var progressRows: []
  property string statusMessage: ""
  property string lastError: ""

  property bool startupHandled: false
  property int readinessAttempts: 0
  property int readinessStableTicks: 0

  readonly property bool busy: actionRunning || restoreActive || inspectProcess.running
  readonly property bool backendAvailable: backendPath !== "" && backendPath !== "bin/workspace-setups"

  signal inspectionReady(string requestedName, string existingName, var snapshot)

  function applyStartup(startup) {
    if (!startup) return
    startupMode = startup.mode || "none"
    defaultProfile = startup.defaultProfile || ""
    lastProfile = startup.lastProfile || ""
  }

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

  function runAction(args, description) {
    if (!backendAvailable || actionProcess.running || (restoreActive && args[0] !== "cancel")) return false
    lastError = ""
    statusMessage = description || "Working..."
    currentAction = String(args[0] || "")
    actionProcess.command = [backendPath].concat(args)
    actionProcess.running = true
    return true
  }

  function save(name) {
    var value = String(name || "").trim()
    return value !== "" && runAction(["save", value], "Saving " + value + "...")
  }

  function inspectForSave(name, existingName) {
    var value = String(name || "").trim()
    if (value === "" || busy || inspectProcess.running) return false
    inspectionName = value
    inspectionExisting = String(existingName || "")
    lastError = ""
    statusMessage = "Checking current workspace..."
    inspectProcess.running = true
    return true
  }

  function updateProfile(name) {
    var value = String(name || "").trim()
    return value !== "" && runAction(["update", value], "Updating " + value + "...")
  }

  function loadProfile(name, mode, confirmReplace) {
    var value = String(name || "").trim()
    var selectedMode = String(mode || "empty")
    if (["empty", "add", "replace"].indexOf(selectedMode) < 0) return false
    var args = ["load", value, "--mode", selectedMode]
    if (selectedMode === "replace" && confirmReplace === true) args.push("--confirm-replace")
    return value !== "" && runAction(args, "Loading " + value + "...")
  }

  function renameProfile(oldName, newName) {
    var oldValue = String(oldName || "").trim()
    var newValue = String(newName || "").trim()
    return oldValue !== "" && newValue !== ""
      && runAction(["rename", oldValue, newValue], "Renaming " + oldValue + "...")
  }

  function deleteProfile(name) {
    var value = String(name || "").trim()
    return value !== "" && runAction(["delete", value], "Deleting " + value + "...")
  }

  function setStartup(mode, name) {
    var normalized = String(mode || "none")
    if (["none", "default", "last", "chooser"].indexOf(normalized) < 0) return false
    var backendMode = normalized === "default" ? "fixed" : (normalized === "chooser" ? "ask" : normalized)
    var args = ["set-startup", backendMode]
    if (normalized === "default") {
      var value = String(name || "").trim()
      if (value === "") return false
      args.push(value)
    }
    return runAction(args, "Updating startup behavior...")
  }

  function cancelRestore() {
    if (cancelProcess.running || (!restoreActive && !actionProcess.running)) return false
    statusMessage = "Canceling restore..."
    cancelProcess.running = true
    return true
  }

  function handleStartup() {
    if (startupHandled || !configurationLoaded) return
    if (startupMode === "none") {
      startupHandled = true
      return
    }
    readinessTimer.start()
  }

  function triggerStartup() {
    if (startupHandled) return true
    if (restoreActive || actionProcess.running) return false
    startupHandled = true
    if (startupMode === "chooser") {
      if (shell && typeof shell.summon === "function") shell.summon(pluginId, "")
      return true
    }
    var target = startupMode === "default" ? defaultProfile : lastProfile
    if (target === "") {
      lastError = startupMode === "default"
        ? "No default workspace profile is configured"
        : "No previously loaded workspace profile is available"
      return true
    }
    loadProfile(target, "empty", false)
    return true
  }

  Process {
    id: listProcess
    command: [root.backendPath, "list", "--json"]
    stdout: StdioCollector { id: listStdout; waitForEnd: true }
    stderr: StdioCollector { id: listStderr; waitForEnd: true }
    onExited: function(exitCode) {
      root.refreshing = false
      if (exitCode !== 0) {
        root.lastError = Model.compactError(listStderr.text || listStdout.text, "Could not list workspace profiles")
        return
      }
      var result = Model.parseList(listStdout.text)
      if (!result.ok) {
        root.lastError = "The backend returned invalid profile JSON"
        return
      }
      root.profiles = result.profiles
      root.applyStartup(result.startup)
      root.lastError = ""
      root.handleStartup()
    }
  }

  Process {
    id: statusProcess
    command: [root.backendPath, "status", "--json"]
    stdout: StdioCollector { id: statusStdout; waitForEnd: true }
    stderr: StdioCollector { id: statusStderr; waitForEnd: true }
    onExited: function(exitCode) {
      if (exitCode !== 0) return
      var result = Model.parseStatus(statusStdout.text)
      if (!result.ok) return
      root.restoreActive = result.active
      root.restorePhase = result.phase
      root.restoreProfile = result.profile
      root.restoreCompleted = result.completed
      root.restoreTotal = result.total
      root.progressRows = result.rows
      root.applyStartup(result.startup)
      if (result.message !== "") root.statusMessage = result.message
      root.handleStartup()
    }
  }

  Process {
    id: actionProcess
    command: []
    stdout: StdioCollector { id: actionStdout; waitForEnd: true }
    stderr: StdioCollector { id: actionStderr; waitForEnd: true }
    onExited: function(exitCode) {
      if (exitCode === 0) {
        var result = Model.parseJson(actionStdout.text)
        if (result && result.startupMode !== undefined) root.applyStartup(Model.normalizeStartup(result))
        if ((root.currentAction === "save" || root.currentAction === "update") && result && Array.isArray(result.windows)) {
          var unresolved = result.windows.filter(function(window) { return !window.command || window.command.length === 0 }).length
          root.statusMessage = "Saved " + String(result.name || "workspace profile")
            + (unresolved > 0 ? " · " + unresolved + " window" + (unresolved === 1 ? "" : "s") + " cannot be relaunched" : "")
        } else {
          root.statusMessage = root.currentAction === "load" && result
            ? "Restore " + String(result.state || "completed")
            : (result && result.message ? String(result.message) : "Workspace profiles updated")
        }
        root.lastError = ""
      } else {
        root.lastError = Model.compactError(actionStderr.text || actionStdout.text)
        root.statusMessage = ""
      }
      root.currentAction = ""
      configFile.reload()
      root.refresh()
    }
  }

  Process {
    id: inspectProcess
    command: [root.backendPath, "inspect", "--json"]
    stdout: StdioCollector { id: inspectStdout; waitForEnd: true }
    stderr: StdioCollector { id: inspectStderr; waitForEnd: true }
    onExited: function(exitCode) {
      var name = root.inspectionName
      var existing = root.inspectionExisting
      root.inspectionName = ""
      root.inspectionExisting = ""
      if (exitCode !== 0) {
        root.lastError = Model.compactError(inspectStderr.text || inspectStdout.text, "Could not inspect current workspace")
        root.statusMessage = ""
        return
      }
      var snapshot = Model.parseJson(inspectStdout.text)
      if (!snapshot || !Array.isArray(snapshot.windows)) {
        root.lastError = "The backend returned an invalid workspace inspection"
        root.statusMessage = ""
        return
      }
      root.statusMessage = ""
      root.inspectionReady(name, existing, snapshot)
    }
  }

  Process {
    id: cancelProcess
    command: [root.backendPath, "cancel"]
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { id: cancelStderr; waitForEnd: true }
    onExited: function(exitCode) {
      if (exitCode !== 0)
        root.lastError = Model.compactError(cancelStderr.text, "Could not cancel restore")
      root.refreshStatus()
    }
  }

  Process {
    id: readinessProcess
    command: ["hyprctl", "-j", "monitors"]
    stdout: StdioCollector { id: readinessStdout; waitForEnd: true }
    onExited: function(exitCode) {
      var monitors = exitCode === 0 ? Model.parseJson(readinessStdout.text) : null
      if (Array.isArray(monitors) && monitors.length > 0) {
        root.readinessStableTicks += 1
        if (root.readinessStableTicks >= 3 && root.triggerStartup()) readinessTimer.stop()
      } else if (root.readinessAttempts >= 60) {
        readinessTimer.stop()
        root.startupHandled = true
        root.lastError = "Startup restore skipped because Hyprland outputs were not ready"
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

  FileView {
    id: configFile
    path: root.configPath
    watchChanges: true
    printErrors: false
    onLoaded: {
      var data = Model.parseJson(text())
      if (data) root.applyStartup(Model.normalizeStartup(data))
      root.configurationLoaded = true
      root.handleStartup()
    }
    onLoadFailed: {
      root.applyStartup(Model.normalizeStartup(null))
      root.configurationLoaded = true
      root.handleStartup()
    }
    onFileChanged: reload()
  }

  Component.onCompleted: refresh()
}
