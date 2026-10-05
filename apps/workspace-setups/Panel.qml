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
  property string expandedId: ""
  property int openPopups: 0
  readonly property var barIdentity: hostWidget || root
  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property color urgent: bar ? bar.urgent : Color.urgent
  readonly property color dim: Qt.darker(foreground, 1.55)
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family
  readonly property var templates: backend ? backend.templates : []
  readonly property var selectedTemplate: templates.length > 0
    ? templates[Math.max(0, Math.min(selectedIndex, templates.length - 1))] : null
  readonly property bool busy: !backend || backend.busy
  readonly property bool bindingsActive: backend !== null && backend.bindingsInstalled
  readonly property bool choiceOpen: backend !== null && backend.pendingChoice !== null
  property int focusedFields: 0
  readonly property bool editing: saveField.activeFocus || renameField.activeFocus || focusedFields > 0 || openPopups > 0

  function t(key, args) { return backend ? backend.tr(key, args) : key }

  function open() {
    root.controller.show()
    if (backend) backend.refresh()
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  function close() {
    renameTarget = ""
    if (backend) backend.pendingChoice = null
    root.controller.hide()
  }

  function moveSelection(delta) {
    if (templates.length === 0) return
    cursorActive = true
    selectedIndex = Math.max(0, Math.min(templates.length - 1, selectedIndex + delta))
    scrollSelectedIntoView()
  }

  function selectTemplate(index) {
    cursorActive = true
    selectedIndex = index
  }

  function scrollSelectedIntoView() {
    Qt.callLater(function() {
      if (!templateColumn || selectedIndex < 0 || selectedIndex >= templateColumn.children.length) return
      var item = templateColumn.children[selectedIndex]
      var top = item.mapToItem(panelFlick.contentItem, 0, 0).y
      var bottom = top + item.height
      if (top < panelFlick.contentY) panelFlick.contentY = top
      else if (bottom > panelFlick.contentY + panelFlick.height)
        panelFlick.contentY = Math.min(panelFlick.contentHeight - panelFlick.height, bottom - panelFlick.height)
    })
  }

  function beginRename(template) {
    if (!template || busy) return
    renameTarget = template.name
    renameField.text = template.name
    Qt.callLater(function() { renameField.selectAll(); renameField.forceActiveFocus() })
  }

  function commitRename() {
    if (renameTarget !== "" && backend) backend.renameTemplate(renameTarget, renameField.text)
    renameTarget = ""
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  function save() {
    if (busy || saveField.text.trim() === "") return
    if (backend.saveNew(saveField.text)) saveField.text = ""
    keyCatcher.forceActiveFocus()
  }

  function load(template) {
    if (template && !busy) backend.loadTemplate(template.name, "ask")
  }

  function choose(id) {
    if (backend) backend.resolveChoice(id)
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  onTemplatesChanged: {
    // The list is rebuilt after every change, destroying any open dropdowns.
    openPopups = 0
    if (selectedIndex >= templates.length) selectedIndex = Math.max(0, templates.length - 1)
  }

  onChoiceOpenChanged: if (choiceOpen) Qt.callLater(function() { choiceFocus.forceActiveFocus() })

  onOpenedChanged: if (opened) {
    cursorActive = false
    panelFlick.contentY = 0
    if (backend) backend.refresh()
    Qt.callLater(function() { (root.choiceOpen ? choiceFocus : keyCatcher).forceActiveFocus() })
  }

  KeyboardPanel {
    id: panel
    anchorItem: root.anchorItem
    owner: root.barIdentity
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(540))
    contentHeight: panel.fittedContentHeight(content.implicitHeight, Style.space(760))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      blocked: root.editing || root.choiceOpen
      onMoveRequested: function(dx, dy) { if (dy !== 0) root.moveSelection(dy) }
      onActivateRequested: root.load(root.selectedTemplate)
      onCloseRequested: root.close()
      onDeleteRequested: if (root.selectedTemplate && !root.busy) root.backend.requestDelete(root.selectedTemplate.name)
      onTabRequested: function(direction) {
        if (root.bar && typeof root.bar.switchPanelFrom === "function")
          root.bar.switchPanelFrom(root.barIdentity, direction)
      }
      onTextKey: function(text) {
        var key = text.toLowerCase()
        if (key === "r" && root.backend) root.backend.refresh()
        else if (key === "s") saveField.forceActiveFocus()
        else if (key === "u" && root.selectedTemplate && !root.busy) root.backend.updateTemplate(root.selectedTemplate.name)
        else if (key === "n") root.beginRename(root.selectedTemplate)
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
            title: root.t("title")
            meta: root.backend && root.backend.restoreActive
              ? root.backend.restoreTemplate + " · " + root.t("phase." + root.backend.restorePhase)
              : root.t("templates.count", { count: root.templates.length })
            detail: root.backend && root.backend.restoreActive && root.backend.restoreTotal > 0
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
            PanelSectionHeader { text: root.t("section.progress"); foreground: root.foreground; fontFamily: root.fontFamily }
            Repeater {
              model: root.backend ? root.backend.progressRows : []
              Row {
                required property var modelData
                width: parent.width
                spacing: Style.space(8)
                Text {
                  width: Style.space(18)
                  text: Model.progressGlyph(modelData.state)
                  color: modelData.state === "failed" ? root.urgent : root.foreground
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
              text: root.t("action.cancelRestore")
              iconText: "󰜺"
              bordered: true
              focusable: true
              foreground: root.urgent
              fontFamily: root.fontFamily
              onClicked: root.backend.cancelRestore()
            }
          }

          Row {
            width: parent.width
            spacing: Style.space(8)
            TextField {
              id: saveField
              width: parent.width - saveButton.width - parent.spacing
              placeholderText: root.t("save.placeholder")
              foreground: root.foreground
              font.family: root.fontFamily
              enabled: !root.busy
              onAccepted: root.save()
              Keys.onEscapePressed: { text = ""; keyCatcher.forceActiveFocus() }
            }
            Button {
              id: saveButton
              text: root.t("save.button")
              iconText: "󰆓"
              bordered: true
              focusable: true
              enabled: saveField.text.trim() !== "" && !root.busy
              foreground: root.foreground
              fontFamily: root.fontFamily
              onClicked: root.save()
            }
          }

          Text {
            visible: root.templates.length === 0 && !(root.backend && root.backend.refreshing)
            width: parent.width
            text: root.t("templates.empty")
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            topPadding: Style.space(12)
            bottomPadding: Style.space(12)
          }

          Column {
            id: templateColumn
            width: parent.width
            spacing: Style.space(8)
            Repeater {
              model: root.templates
              BorderSurface {
                id: card
                required property var modelData
                required property int index
                readonly property bool expanded: root.expandedId === modelData.id
                width: templateColumn.width
                implicitHeight: cardBody.implicitHeight + Style.space(20)
                color: root.cursorActive && root.selectedIndex === index
                  ? Style.hoverFillFor(root.foreground, Color.accent) : "transparent"
                borderSpec: Border.controlSpec(root.cursorActive && root.selectedIndex === index
                  ? "hover-cursor" : "normal", root.foreground, Color.accent)
                radius: Style.cornerRadius

                Column {
                  id: cardBody
                  anchors.left: parent.left
                  anchors.right: parent.right
                  anchors.verticalCenter: parent.verticalCenter
                  anchors.margins: Style.space(10)
                  spacing: Style.space(8)
                  Item {
                    width: parent.width
                    implicitHeight: Math.max(templateName.implicitHeight, badges.implicitHeight)
                    Text {
                      id: templateName
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
                      spacing: Style.space(8)
                      Text {
                        visible: modelData.restoreOnStartup
                        anchors.verticalCenter: parent.verticalCenter
                        text: root.t("badge.startup", { workspace: modelData.startupWorkspace })
                        color: root.dim; font.family: root.fontFamily; font.pixelSize: Style.font.caption; font.bold: true
                      }
                      Dropdown {
                        anchors.verticalCenter: parent.verticalCenter
                        width: Style.space(240)
                        showLabel: false
                        enabled: !root.busy
                        value: String(modelData.shortcut)
                        options: Model.slotOptions(root.t("shortcuts.chipNone"), root.templates, modelData.name)
                        foreground: modelData.shortcut > 0 ? Color.accent : root.dim
                        fontFamily: root.fontFamily
                        onPopupOpenChanged: root.openPopups += popupOpen ? 1 : -1
                        onChanged: function(value) { root.backend.setShortcut(modelData.name, Number(value)) }
                      }
                    }
                  }
                  Text {
                    width: parent.width
                    text: [root.t("label.windows", { count: modelData.windowCount })]
                      .concat(modelData.sourceWorkspace ? [root.t("label.source", { workspace: modelData.sourceWorkspace })] : [])
                      .concat(modelData.unrestorable > 0 ? [root.t("label.unrestorable", { count: modelData.unrestorable })] : [])
                      .join(" · ")
                    color: root.dim
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.caption
                    wrapMode: Text.WordWrap
                  }
                  Flow {
                    width: parent.width
                    spacing: Style.space(6)
                    Button { text: root.t("action.load"); bordered: true; focusable: true; enabled: !root.busy; foreground: root.foreground; fontFamily: root.fontFamily; onClicked: root.load(modelData) }
                    Button { text: root.t("action.update"); bordered: true; focusable: true; enabled: !root.busy; foreground: root.foreground; fontFamily: root.fontFamily; onClicked: root.backend.updateTemplate(modelData.name) }
                    Button { text: root.t("action.rename"); bordered: true; focusable: true; enabled: !root.busy; foreground: root.foreground; fontFamily: root.fontFamily; onClicked: root.beginRename(modelData) }
                    Button { text: root.t("action.settings"); iconText: card.expanded ? "󰅃" : "󰅀"; bordered: true; focusable: true; selected: card.expanded; foreground: root.foreground; fontFamily: root.fontFamily; onClicked: root.expandedId = card.expanded ? "" : modelData.id }
                    Button { text: root.t("action.delete"); bordered: true; focusable: true; enabled: !root.busy; foreground: root.urgent; fontFamily: root.fontFamily; onClicked: root.backend.requestDelete(modelData.name) }
                  }

                  Column {
                    visible: card.expanded
                    width: parent.width
                    spacing: Style.space(10)
                    topPadding: Style.space(4)

                    Toggle {
                      width: parent.width
                      label: root.t("settings.startup")
                      description: root.t("settings.startupDescription", { workspace: modelData.startupWorkspace || "1" })
                      checked: modelData.restoreOnStartup
                      foreground: root.foreground
                      fontFamily: root.fontFamily
                      onClicked: root.backend.setStartup(modelData.name, !modelData.restoreOnStartup, modelData.startupWorkspace || "1")
                    }
                    Dropdown {
                      visible: modelData.restoreOnStartup
                      width: parent.width
                      label: root.t("settings.startupWorkspace")
                      value: modelData.startupWorkspace
                      options: Model.workspaceOptions()
                      foreground: root.foreground
                      fontFamily: root.fontFamily
                      onPopupOpenChanged: root.openPopups += popupOpen ? 1 : -1
                      onChanged: function(value) { root.backend.setStartup(modelData.name, true, value) }
                    }
                    PanelSectionHeader { text: root.t("settings.windows"); foreground: root.foreground; fontFamily: root.fontFamily }
                    Repeater {
                      model: modelData.windows
                      Column {
                        id: windowRow
                        required property var modelData
                        width: parent.width
                        spacing: Style.space(4)
                        Item {
                          property var modelData: windowRow.modelData
                          width: parent.width
                          implicitHeight: strategy.implicitHeight
                          Text {
                            anchors.left: parent.left
                            anchors.right: strategy.left
                            anchors.rightMargin: Style.space(8)
                            anchors.verticalCenter: parent.verticalCenter
                            text: modelData.application + (modelData.restorable ? "" : " (!)")
                            color: modelData.restorable ? root.foreground : root.urgent
                            font.family: root.fontFamily
                            font.pixelSize: Style.font.bodySmall
                            elide: Text.ElideRight
                          }
                          Dropdown {
                            id: strategy
                            anchors.right: parent.right
                            width: parent.width * 0.58
                            showLabel: false
                            value: modelData.strategy
                            options: [
                              { value: "auto", label: root.t("strategy.auto") },
                              { value: "new", label: root.t("strategy.new") },
                              { value: "move", label: root.t("strategy.move") }
                            ]
                            foreground: root.foreground
                            fontFamily: root.fontFamily
                            onPopupOpenChanged: root.openPopups += popupOpen ? 1 : -1
                            onChanged: function(value) { root.backend.setStrategy(card.modelData.name, modelData.id, value) }
                          }
                        }
                        TextField {
                          visible: windowRow.modelData.browser
                          width: parent.width
                          text: windowRow.modelData.urls.join(" ")
                          placeholderText: root.t("settings.urlsPlaceholder")
                          foreground: root.foreground
                          font.family: root.fontFamily
                          font.pixelSize: Style.font.caption
                          enabled: !root.busy
                          onActiveFocusChanged: root.focusedFields += activeFocus ? 1 : -1
                          Component.onDestruction: if (activeFocus) root.focusedFields = Math.max(0, root.focusedFields - 1)
                          onAccepted: { root.backend.setUrls(card.modelData.name, windowRow.modelData.id, text); keyCatcher.forceActiveFocus() }
                          Keys.onEscapePressed: { text = windowRow.modelData.urls.join(" "); keyCatcher.forceActiveFocus() }
                        }
                        Text {
                          visible: windowRow.modelData.browser
                          width: parent.width
                          text: root.t("settings.urlsHint", { count: windowRow.modelData.urls.length })
                          color: root.dim
                          font.family: root.fontFamily
                          font.pixelSize: Style.font.caption
                          wrapMode: Text.WordWrap
                        }
                      }
                    }
                  }
                }
                MouseArea {
                  anchors.fill: parent
                  z: -1
                  acceptedButtons: Qt.NoButton
                  hoverEnabled: true
                  onEntered: root.selectTemplate(index)
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
            Button { id: renameButton; text: root.t("action.rename"); bordered: true; focusable: true; enabled: renameField.text.trim() !== ""; foreground: root.foreground; fontFamily: root.fontFamily; onClicked: root.commitRename() }
          }

          PanelSeparator { foreground: root.foreground }

          Column {
            width: parent.width
            spacing: Style.space(8)
            Item {
              width: parent.width
              implicitHeight: Math.max(shortcutsHeader.implicitHeight, shortcutsState.implicitHeight)
              PanelSectionHeader {
                id: shortcutsHeader
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                text: root.t("section.shortcuts")
                foreground: root.foreground
                fontFamily: root.fontFamily
              }
              Text {
                id: shortcutsState
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                text: root.t(root.bindingsActive ? "shortcuts.active" : "shortcuts.inactive")
                color: root.bindingsActive ? Color.accent : root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.caption
                font.bold: true
              }
            }
            Repeater {
              model: Model.assignedSlots(root.templates)
              Item {
                required property var modelData
                width: parent.width
                implicitHeight: removeShortcut.implicitHeight
                Text {
                  id: slotKey
                  anchors.left: parent.left
                  anchors.verticalCenter: parent.verticalCenter
                  width: Style.space(160)
                  text: Model.slotLabel(modelData.slot)
                  color: Color.accent
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.bodySmall
                  font.bold: true
                }
                Text {
                  anchors.left: slotKey.right
                  anchors.right: removeShortcut.left
                  anchors.rightMargin: Style.space(8)
                  anchors.verticalCenter: parent.verticalCenter
                  text: modelData.name
                  color: root.foreground
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.bodySmall
                  elide: Text.ElideRight
                }
                Button {
                  id: removeShortcut
                  anchors.right: parent.right
                  anchors.verticalCenter: parent.verticalCenter
                  text: root.t("shortcuts.remove")
                  bordered: true
                  focusable: true
                  enabled: !root.busy
                  foreground: root.foreground
                  fontFamily: root.fontFamily
                  fontSize: Style.font.caption
                  onClicked: root.backend.setShortcut(modelData.name, 0)
                }
              }
            }
            Text {
              visible: Model.assignedSlots(root.templates).length === 0
              width: parent.width
              text: root.t("shortcuts.none", { slot: Model.firstFreeSlot(root.templates) || 1 })
              color: root.foreground
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
            }
            Text {
              width: parent.width
              text: root.t("shortcuts.help") + (root.bindingsActive ? "" : " " + root.t("shortcuts.inactiveDetail"))
              color: root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
            }
            Button {
              visible: !root.bindingsActive
              text: root.t("bindings.install")
              iconText: "󰌌"
              bordered: true
              focusable: true
              enabled: !root.busy
              foreground: root.foreground
              fontFamily: root.fontFamily
              onClicked: root.backend.installBindings()
            }
          }

          PanelSeparator { foreground: root.foreground }

          Column {
            width: parent.width
            spacing: Style.space(8)
            PanelSectionHeader { text: root.t("section.language"); foreground: root.foreground; fontFamily: root.fontFamily }
            Dropdown {
              width: parent.width
              showLabel: false
              value: root.backend ? root.backend.languageSetting : "auto"
              options: [
                { value: "auto", label: root.t("language.auto") },
                { value: "en", label: "English" },
                { value: "de", label: "Deutsch" }
              ]
              foreground: root.foreground
              fontFamily: root.fontFamily
              onPopupOpenChanged: root.openPopups += popupOpen ? 1 : -1
              onChanged: function(value) { if (root.backend) root.backend.setLanguage(value) }
            }
          }

          Text {
            width: parent.width
            text: root.t("hint.keys")
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
          }
        }
      }

      FocusScope {
        id: choiceFocus
        anchors.fill: parent
        visible: root.choiceOpen
        z: 20
        Keys.onPressed: function(event) {
          if (choiceDialog.handleKey(event)) event.accepted = true
        }
        ChoiceDialog {
          id: choiceDialog
          anchors.fill: parent
          opened: root.choiceOpen
          message: root.choiceOpen ? root.backend.pendingChoice.message : ""
          options: root.choiceOpen ? root.backend.pendingChoice.options : []
          cancelText: root.t("action.cancel")
          background: Color.popups.background
          foreground: root.foreground
          fontFamily: root.fontFamily
          onChosen: function(id) { root.choose(id) }
        }
      }
    }
  }
}
