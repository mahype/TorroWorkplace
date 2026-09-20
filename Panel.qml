import QtQuick
import QtQuick.Controls
import Quickshell
import qs.Commons
import qs.Ui
import "Model.js" as Model

Panel {
  id: root
  moduleName: "io.github.mahype.workspace-setups"
  ipcTarget: moduleName
  manageIpc: false

  property var anchorItem: null
  property var hostWidget: null
  property var backend: null
  property int selectedIndex: 0
  property bool cursorActive: false
  property string renameTarget: ""
  property string deleteTarget: ""
  property string overwriteTarget: ""
  property string replaceTarget: ""
  property string saveWarningTarget: ""
  property string saveWarningExisting: ""
  property int saveWarningUnresolved: 0
  property int saveWarningFallback: 0
  property string loadMode: "empty"
  readonly property var barIdentity: hostWidget || root
  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property color urgent: bar ? bar.urgent : Color.urgent
  readonly property color dim: Qt.darker(foreground, 1.55)
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family
  readonly property var profiles: backend ? backend.profiles : []
  readonly property var selectedProfile: profiles.length > 0
    ? profiles[Math.max(0, Math.min(selectedIndex, profiles.length - 1))] : null
  readonly property bool editing: saveField.activeFocus || renameField.activeFocus
    || startupMode.popupOpen || defaultProfile.popupOpen

  function open() {
    root.controller.show()
    if (backend) backend.refresh()
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  function close() {
    renameTarget = ""
    deleteTarget = ""
    overwriteTarget = ""
    replaceTarget = ""
    saveWarningTarget = ""
    saveWarningExisting = ""
    root.controller.hide()
  }

  function moveSelection(delta) {
    if (profiles.length === 0) return
    cursorActive = true
    selectedIndex = Math.max(0, Math.min(profiles.length - 1, selectedIndex + delta))
    scrollSelectedIntoView()
  }

  function selectProfile(index) {
    cursorActive = true
    selectedIndex = index
    scrollSelectedIntoView()
  }

  function scrollSelectedIntoView() {
    Qt.callLater(function() {
      if (!profileColumn || selectedIndex < 0 || selectedIndex >= profileColumn.children.length) return
      var item = profileColumn.children[selectedIndex]
      var point = item.mapToItem(panelFlick.contentItem, 0, 0)
      var top = point.y
      var bottom = top + item.height
      if (top < panelFlick.contentY) panelFlick.contentY = top
      else if (bottom > panelFlick.contentY + panelFlick.height)
        panelFlick.contentY = Math.min(panelFlick.contentHeight - panelFlick.height, bottom - panelFlick.height)
    })
  }

  function beginRename(profile) {
    if (!profile || !backend || backend.busy) return
    renameTarget = profile.name
    renameField.text = profile.name
    Qt.callLater(function() { renameField.selectAll(); renameField.forceActiveFocus() })
  }

  function commitRename() {
    var next = renameField.text.trim()
    if (renameTarget !== "" && next !== "" && next !== renameTarget && backend)
      backend.renameProfile(renameTarget, next)
    renameTarget = ""
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  function requestDelete(profile) {
    if (!profile || !backend || backend.busy) return
    deleteTarget = profile.name
    deleteDialog.selectedIndex = 0
    Qt.callLater(function() { deleteFocus.forceActiveFocus() })
  }

  function requestLoad(profile) {
    if (!profile || !backend || backend.busy) return
    if (loadMode === "replace") {
      replaceTarget = profile.name
      deleteDialog.selectedIndex = 0
      Qt.callLater(function() { deleteFocus.forceActiveFocus() })
      return
    }
    backend.loadProfile(profile.name, loadMode, false)
  }

  function requestSave(name) {
    var value = String(name || "").trim()
    if (value === "" || !backend || backend.busy) return
    var existing = ""
    for (var i = 0; i < profiles.length; i++) {
      if (profiles[i].name.toLowerCase() === value.toLowerCase()
          || profiles[i].id.toLowerCase() === value.toLowerCase()) {
        existing = profiles[i].name
        break
      }
    }
    backend.inspectForSave(value, existing)
  }

  function completeInspectedSave(name, existing, snapshot) {
    var warnings = Model.snapshotWarnings(snapshot)
    if (existing !== "" || warnings.unresolved > 0 || warnings.fallback > 0) {
      saveWarningTarget = name
      saveWarningExisting = existing
      saveWarningUnresolved = warnings.unresolved
      saveWarningFallback = warnings.fallback
      deleteDialog.selectedIndex = 0
      Qt.callLater(function() { deleteFocus.forceActiveFocus() })
      return
    }
    if (backend && backend.save(name)) saveField.text = ""
  }

  function finishDelete(confirm) {
    var target = deleteTarget
    deleteTarget = ""
    if (confirm && backend) backend.deleteProfile(target)
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  function finishConfirmation(confirm) {
    if (saveWarningTarget !== "") {
      var saveName = saveWarningTarget
      var existingName = saveWarningExisting
      saveWarningTarget = ""
      saveWarningExisting = ""
      if (confirm && backend) {
        if (existingName !== "") backend.updateProfile(existingName)
        else backend.save(saveName)
        saveField.text = ""
      }
      Qt.callLater(function() { keyCatcher.forceActiveFocus() })
      return
    }
    if (replaceTarget !== "") {
      var replaceName = replaceTarget
      replaceTarget = ""
      if (confirm && backend) backend.loadProfile(replaceName, "replace", true)
      Qt.callLater(function() { keyCatcher.forceActiveFocus() })
      return
    }
    if (overwriteTarget !== "") {
      var target = overwriteTarget
      overwriteTarget = ""
      if (confirm && backend) {
        backend.updateProfile(target)
        saveField.text = ""
      }
      Qt.callLater(function() { keyCatcher.forceActiveFocus() })
      return
    }
    finishDelete(confirm)
  }

  function startupChanged(mode) {
    if (!backend || backend.busy) return
    if (mode === "default" && backend.defaultProfile === "") {
      if (selectedProfile) backend.setStartup("default", selectedProfile.id)
      return
    }
    backend.setStartup(mode, backend.defaultProfile)
  }

  onProfilesChanged: {
    if (selectedIndex >= profiles.length) selectedIndex = Math.max(0, profiles.length - 1)
  }

  Connections {
    target: root.backend
    function onInspectionReady(requestedName, existingName, snapshot) {
      root.completeInspectedSave(requestedName, existingName, snapshot)
    }
  }
  onOpenedChanged: if (opened) {
    cursorActive = false
    panelFlick.contentY = 0
    if (backend) backend.refresh()
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  KeyboardPanel {
    id: panel
    anchorItem: root.anchorItem
    owner: root.barIdentity
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(520))
    contentHeight: panel.fittedContentHeight(content.implicitHeight, Style.space(720))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      blocked: root.editing || root.deleteTarget !== "" || root.overwriteTarget !== "" || root.replaceTarget !== "" || root.saveWarningTarget !== ""
      onMoveRequested: function(dx, dy) { if (dy !== 0) root.moveSelection(dy) }
      onActivateRequested: root.requestLoad(root.selectedProfile)
      onCloseRequested: root.close()
      onDeleteRequested: root.requestDelete(root.selectedProfile)
      onTabRequested: function(direction) {
        if (root.bar && typeof root.bar.switchPanelFrom === "function")
          root.bar.switchPanelFrom(root.barIdentity, direction)
      }
      onTextKey: function(text) {
        var key = text.toLowerCase()
        if (key === "r" && root.backend) root.backend.refresh()
        else if (key === "s") saveField.forceActiveFocus()
        else if (key === "u" && root.selectedProfile && root.backend) root.requestSave(root.selectedProfile.name)
        else if (key === "n") root.beginRename(root.selectedProfile)
        else if (key === "c" && root.backend && root.backend.busy) root.backend.cancelRestore()
      }

      Flickable {
        id: panelFlick
        anchors.fill: parent
        contentWidth: width
        contentHeight: content.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        interactive: contentHeight > height
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

        Column {
          id: content
          width: panelFlick.width
          spacing: Style.space(14)

          PanelHero {
            width: parent.width
            title: "Workspace Setups"
            meta: root.backend && root.backend.restoreActive
              ? ((root.backend.restoreProfile || "Profile") + " · " + root.backend.restorePhase)
              : (root.profiles.length + " saved profile" + (root.profiles.length === 1 ? "" : "s"))
            detail: root.backend && root.backend.restoreTotal > 0
              ? root.backend.restoreCompleted + "/" + root.backend.restoreTotal : ""
            foreground: root.foreground
            fontFamily: root.fontFamily
            iconComponent: Component {
              Text {
                text: "󰆍"
                color: root.foreground
                font.family: root.fontFamily
                font.pixelSize: Style.font.display
              }
            }
          }

          BorderSurface {
            visible: root.backend && (root.backend.lastError !== "" || root.backend.statusMessage !== "")
            width: parent.width
            implicitHeight: feedback.implicitHeight + Style.space(18)
            color: "transparent"
            borderSpec: Border.flat(root.backend && root.backend.lastError !== "" ? root.urgent : root.dim, 1)
            radius: Style.cornerRadius
            Text {
              id: feedback
              anchors.left: parent.left
              anchors.right: parent.right
              anchors.verticalCenter: parent.verticalCenter
              anchors.margins: Style.space(9)
              text: root.backend ? (root.backend.lastError || root.backend.statusMessage) : ""
              color: root.backend && root.backend.lastError !== "" ? root.urgent : root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
            }
          }

          Column {
            visible: root.backend && root.backend.progressRows.length > 0
            width: parent.width
            spacing: Style.space(6)
            PanelSectionHeader { text: "RESTORE PROGRESS"; foreground: root.foreground; fontFamily: root.fontFamily }
            Repeater {
              model: root.backend ? root.backend.progressRows : []
              Row {
                required property var modelData
                width: parent.width
                spacing: Style.space(8)
                Text {
                  width: Style.space(18)
                  text: Model.progressGlyph(modelData.state)
                  color: modelData.state === "failed" || modelData.state === "error" ? root.urgent : root.foreground
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.body
                }
                Text {
                  width: parent.width - Style.space(26)
                  text: modelData.label + (modelData.detail ? " · " + modelData.detail : "")
                  color: root.foreground
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.bodySmall
                  elide: Text.ElideRight
                }
              }
            }
            Button {
              visible: root.backend && root.backend.restoreActive
              text: "Cancel restore"
              iconText: "󰜺"
              bordered: true
              focusable: true
              foreground: root.urgent
              fontFamily: root.fontFamily
              onClicked: root.backend.cancelRestore()
            }
          }

          PanelSeparator { foreground: root.foreground }

          Column {
            width: parent.width
            spacing: Style.space(8)
            PanelSectionHeader { text: "LOAD BEHAVIOR"; foreground: root.foreground; fontFamily: root.fontFamily }
            Dropdown {
              width: parent.width
              showLabel: false
              value: root.loadMode
              options: [
                { value: "empty", label: "Require an empty session" },
                { value: "add", label: "Add to open windows" },
                { value: "replace", label: "Close open windows and replace" }
              ]
              foreground: root.foreground
              fontFamily: root.fontFamily
              onChanged: function(value) { root.loadMode = value }
            }
            Text {
              width: parent.width
              text: root.loadMode === "empty"
                ? "Safest and most exact. Loading stops if application windows are already open."
                : (root.loadMode === "add"
                  ? "Keeps existing windows and adds the saved setup."
                  : "Closes normal application windows after an explicit confirmation.")
              color: root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
            }
          }

          PanelSeparator { foreground: root.foreground }

          Row {
            width: parent.width
            spacing: Style.space(8)
            TextField {
              id: saveField
              width: parent.width - saveButton.width - parent.spacing
              placeholderText: "New profile name"
              foreground: root.foreground
              font.family: root.fontFamily
              enabled: !root.backend || !root.backend.busy
              onAccepted: {
                root.requestSave(text)
                if (root.overwriteTarget === "") keyCatcher.forceActiveFocus()
              }
              Keys.onEscapePressed: { text = ""; keyCatcher.forceActiveFocus() }
            }
            Button {
              id: saveButton
              text: "Save current"
              iconText: "󰆓"
              bordered: true
              focusable: true
              enabled: saveField.text.trim() !== "" && root.backend && !root.backend.busy
              foreground: root.foreground
              fontFamily: root.fontFamily
              onClicked: root.requestSave(saveField.text)
            }
          }

          Text {
            visible: root.profiles.length === 0 && !(root.backend && root.backend.refreshing)
            width: parent.width
            text: "No saved workspace profiles yet.\nName the current desktop above to create one."
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            topPadding: Style.space(12)
            bottomPadding: Style.space(12)
          }

          Column {
            id: profileColumn
            width: parent.width
            spacing: Style.space(8)
            Repeater {
              model: root.profiles
              BorderSurface {
                required property var modelData
                required property int index
                width: profileColumn.width
                implicitHeight: profileBody.implicitHeight + Style.space(20)
                color: root.cursorActive && root.selectedIndex === index
                  ? Style.hoverFillFor(root.foreground, Color.accent) : "transparent"
                borderSpec: Border.controlSpec(root.cursorActive && root.selectedIndex === index
                  ? "hover-cursor" : "normal", root.foreground, Color.accent)
                radius: Style.cornerRadius

                Column {
                  id: profileBody
                  anchors.left: parent.left
                  anchors.right: parent.right
                  anchors.verticalCenter: parent.verticalCenter
                  anchors.margins: Style.space(10)
                  spacing: Style.space(8)
                  Item {
                    width: parent.width
                    implicitHeight: Math.max(profileName.implicitHeight, badges.implicitHeight)
                    Text {
                      id: profileName
                      anchors.left: parent.left
                      anchors.right: badges.left
                      anchors.rightMargin: Style.space(8)
                      text: modelData.name
                      color: root.foreground
                      font.family: root.fontFamily
                      font.pixelSize: Style.font.title
                      font.bold: true
                      elide: Text.ElideRight
                    }
                    Row {
                      id: badges
                      anchors.right: parent.right
                      spacing: Style.space(6)
                      Text { visible: modelData.isDefault || (root.backend && root.backend.defaultProfile === modelData.id); text: "DEFAULT"; color: root.dim; font.family: root.fontFamily; font.pixelSize: Style.font.caption; font.bold: true }
                      Text { visible: modelData.isLast || (root.backend && root.backend.lastProfile === modelData.id); text: "LAST"; color: root.dim; font.family: root.fontFamily; font.pixelSize: Style.font.caption; font.bold: true }
                    }
                  }
                  Text {
                    text: Model.profileLabel(modelData)
                    color: root.dim
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.caption
                  }
                  Row {
                    width: parent.width
                    spacing: Style.space(6)
                    Button { text: "Load"; bordered: true; focusable: true; enabled: root.backend && !root.backend.busy; foreground: root.foreground; fontFamily: root.fontFamily; onClicked: root.requestLoad(modelData) }
                    Button { text: "Update"; bordered: true; focusable: true; enabled: root.backend && !root.backend.busy; foreground: root.foreground; fontFamily: root.fontFamily; onClicked: root.requestSave(modelData.name) }
                    Button { text: "Rename"; bordered: true; focusable: true; enabled: root.backend && !root.backend.busy; foreground: root.foreground; fontFamily: root.fontFamily; onClicked: root.beginRename(modelData) }
                    Button { text: "Delete"; bordered: true; focusable: true; enabled: root.backend && !root.backend.busy; foreground: root.urgent; fontFamily: root.fontFamily; onClicked: root.requestDelete(modelData) }
                  }
                }
                MouseArea {
                  anchors.fill: parent
                  z: -1
                  acceptedButtons: Qt.NoButton
                  hoverEnabled: true
                  onEntered: root.selectProfile(index)
                }
              }
            }
          }

          Row {
            visible: root.renameTarget !== ""
            width: parent.width
            spacing: Style.space(8)
            TextField {
              id: renameField
              width: parent.width - renameButton.width - parent.spacing
              foreground: root.foreground
              font.family: root.fontFamily
              onAccepted: root.commitRename()
              Keys.onEscapePressed: { root.renameTarget = ""; keyCatcher.forceActiveFocus() }
            }
            Button { id: renameButton; text: "Rename"; bordered: true; focusable: true; enabled: renameField.text.trim() !== ""; foreground: root.foreground; fontFamily: root.fontFamily; onClicked: root.commitRename() }
          }

          PanelSeparator { foreground: root.foreground }

          Column {
            width: parent.width
            spacing: Style.space(10)
            PanelSectionHeader { text: "AT LOGIN"; foreground: root.foreground; fontFamily: root.fontFamily }
            Dropdown {
              id: startupMode
              width: parent.width
              showLabel: false
              value: root.backend ? root.backend.startupMode : "none"
              options: [
                { value: "none", label: "Do nothing" },
                { value: "default", label: "Load default profile" },
                { value: "last", label: "Load last profile" },
                { value: "chooser", label: "Open profile chooser" }
              ]
              foreground: root.foreground
              fontFamily: root.fontFamily
              onChanged: function(value) { root.startupChanged(value) }
            }
            Dropdown {
              id: defaultProfile
              visible: root.backend && root.backend.startupMode === "default"
              width: parent.width
              label: "Default profile"
              value: root.backend ? root.backend.defaultProfile : ""
              options: root.profiles.map(function(profile) { return { value: profile.id, label: profile.name } })
              foreground: root.foreground
              fontFamily: root.fontFamily
              onChanged: function(value) { if (root.backend) root.backend.setStartup("default", value) }
            }
          }

          Text {
            width: parent.width
            text: "↑/↓ select · Enter load · S save · U update · N rename · X delete · R refresh"
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
          }
        }
      }

      FocusScope {
        id: deleteFocus
        anchors.fill: parent
        visible: root.deleteTarget !== "" || root.overwriteTarget !== "" || root.replaceTarget !== "" || root.saveWarningTarget !== ""
        z: 20
        Keys.onPressed: function(event) {
          if (deleteDialog.handleKey(event)) event.accepted = true
        }
        ConfirmDialog {
          id: deleteDialog
          anchors.fill: parent
          opened: deleteFocus.visible
          message: root.saveWarningTarget !== ""
            ? ((root.saveWarningExisting !== "" ? "Update existing profile '" + root.saveWarningExisting + "'? " : "Save profile '" + root.saveWarningTarget + "'? ")
              + (root.saveWarningUnresolved > 0 ? root.saveWarningUnresolved + " window(s) cannot be relaunched. " : "")
              + (root.saveWarningFallback > 0 ? root.saveWarningFallback + " workspace layout(s) can only be restored approximately." : ""))
            : (root.replaceTarget !== ""
            ? "Close all normal application windows and load workspace profile '" + root.replaceTarget + "'? Unsaved work may be lost."
            : (root.overwriteTarget !== ""
              ? "A workspace profile named '" + root.overwriteTarget + "' already exists. Update it from the current desktop?"
              : "Delete workspace profile '" + root.deleteTarget + "'? This cannot be undone."))
          confirmText: root.saveWarningTarget !== "" ? (root.saveWarningExisting !== "" ? "Update" : "Save")
            : (root.replaceTarget !== "" ? "Replace" : (root.overwriteTarget !== "" ? "Update" : "Delete"))
          background: Color.popups.background
          foreground: root.foreground
          fontFamily: root.fontFamily
          onCanceled: root.finishConfirmation(false)
          onConfirmed: root.finishConfirmation(true)
        }
      }
    }
  }
}
