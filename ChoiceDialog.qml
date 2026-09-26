import QtQuick
import qs.Commons
import qs.Ui

// ConfirmDialog with any number of choices. `options` is a list of
// { id, label, destructive }; a leading cancel button is always added.
Item {
  id: root

  property bool opened: false
  property string message: ""
  property string cancelText: "Cancel"
  property var options: []
  property int selectedIndex: 0
  property color background: Color.background
  property color foreground: Color.foreground
  property color selectedText: Color.accent
  property string fontFamily: Style.font.family

  readonly property var buttons: [{ id: "cancel", label: cancelText, destructive: false }].concat(options || [])

  signal chosen(string id)

  function handleKey(event) {
    if (!opened) return false
    if (event.key === Qt.Key_Escape) {
      chosen("cancel")
    } else if (event.key === Qt.Key_Left || event.key === Qt.Key_Backtab) {
      selectedIndex = (selectedIndex + buttons.length - 1) % buttons.length
    } else if (event.key === Qt.Key_Right || event.key === Qt.Key_Tab) {
      selectedIndex = (selectedIndex + 1) % buttons.length
    } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
      chosen(buttons[selectedIndex].id)
    } else return false
    return true
  }

  onOpenedChanged: if (opened) selectedIndex = 0
  visible: opened

  Rectangle {
    anchors.fill: parent
    color: Util.alpha(Color.background, 0.7)

    MouseArea { anchors.fill: parent; onClicked: root.chosen("cancel") }

    BorderSurface {
      id: card
      width: Math.min(parent.width - Style.space(32), Style.space(420))
      height: card.contentTopInset + card.contentBottomInset + messageText.implicitHeight + buttonFlow.implicitHeight + Style.space(20)
      anchors.centerIn: parent
      color: root.background
      borderSpec: Border.flat(root.selectedText, Style.normalBorderWidth)
      padding: Style.space(18)
      radius: Style.cornerRadius

      MouseArea { anchors.fill: parent; onClicked: {} }

      Item {
        anchors.fill: parent
        anchors.topMargin: card.contentTopInset
        anchors.rightMargin: card.contentRightInset
        anchors.bottomMargin: card.contentBottomInset
        anchors.leftMargin: card.contentLeftInset

        Text {
          id: messageText
          textFormat: Text.PlainText
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.top: parent.top
          text: root.message
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.title
          wrapMode: Text.WordWrap
        }

        Flow {
          id: buttonFlow
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.bottom: parent.bottom
          layoutDirection: Qt.RightToLeft
          spacing: Style.space(10)

          Repeater {
            // Reversed so cancel sits on the left in right-to-left flow.
            model: root.buttons.slice().reverse()

            BorderSurface {
              required property int index
              required property var modelData

              readonly property int buttonIndex: root.buttons.length - 1 - index
              readonly property bool selected: root.selectedIndex === buttonIndex
              readonly property bool destructive: modelData.destructive === true

              width: Math.max(Style.space(88), label.implicitWidth + Style.space(24))
              height: Style.space(34)
              color: selected
                ? (destructive ? Util.alpha(Color.urgent, 0.22) : Util.alpha(root.foreground, 0.08))
                : "transparent"
              borderSpec: Border.flat(destructive
                ? (selected ? Color.urgent : Util.alpha(Color.urgent, 0.56))
                : (selected ? root.selectedText : Util.alpha(root.foreground, 0.38)), Style.normalBorderWidth)
              radius: 0

              Text {
                id: label
                textFormat: Text.PlainText
                anchors.centerIn: parent
                text: modelData.label
                color: destructive ? (selected ? Color.urgent : root.foreground) : (selected ? root.selectedText : root.foreground)
                font.family: root.fontFamily
                font.pixelSize: Style.font.caption
              }

              MouseArea {
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onEntered: root.selectedIndex = buttonIndex
                onClicked: root.chosen(modelData.id)
              }
            }
          }
        }
      }
    }
  }
}
