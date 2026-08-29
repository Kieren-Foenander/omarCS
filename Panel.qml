import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

Panel {
  id: root
  moduleName: "omarcs.stats"
  ipcTarget: "omarcs.stats"
  manageIpc: false

  property var report: null
  property string loadError: ""
  property string refreshError: ""
  property int selectedIndex: 0

  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property color urgent: bar ? bar.urgent : Color.urgent
  readonly property color winColor: "#4ade80"
  readonly property color lossColor: "#f87171"
  readonly property color dim: Qt.darker(foreground, 1.55)
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family
  readonly property var recent: report && report.recent ? report.recent : []
  readonly property int matchCount: Math.min(5, recent.length)
  readonly property var selectedMatch: matchCount > 0
    ? recent[Math.max(0, Math.min(selectedIndex, matchCount - 1))]
    : (report && report.latest ? report.latest : null)
  readonly property var stats: selectedMatch && selectedMatch.stats ? selectedMatch.stats : null
  readonly property var trends: report && report.trends ? report.trends : ({ matches: 0, wins: 0, rating: 0, adr: 0, kast: 0 })
  readonly property string status: report ? String(report.status || "empty") : "empty"
  readonly property bool busy: status === "analyzing" || refreshProcess.running

  visible: true
  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  function resultColor(result) {
    if (result === "W") return winColor
    if (result === "L") return lossColor
    return foreground
  }

  function formatNumber(raw, decimals) {
    var value = Number(raw)
    return isFinite(value) ? value.toFixed(decimals) : "—"
  }

  function refreshNow() {
    if (refreshProcess.running) return
    refreshError = ""
    refreshProcess.running = true
  }

  function selectMatch(index) {
    if (matchCount < 1) return
    selectedIndex = Math.max(0, Math.min(Number(index), matchCount - 1))
  }

  function selectOlder() {
    selectMatch(selectedIndex + 1)
  }

  function selectNewer() {
    selectMatch(selectedIndex - 1)
  }

  function score(match) {
    if (!match || !match.stats) return "—"
    return match.stats.roundsFor + "–" + match.stats.roundsAgainst
  }

  onRecentChanged: if (selectedIndex >= matchCount) selectedIndex = Math.max(0, matchCount - 1)

  FileView {
    id: summaryFile
    path: (Quickshell.env("XDG_STATE_HOME") || Quickshell.env("HOME") + "/.local/state") + "/omarcs/summary.json"
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: {
      try {
        root.report = JSON.parse(text())
        root.loadError = ""
      } catch (error) {
        root.loadError = "Could not read omarCS data"
      }
    }
    onLoadFailed: {
      root.report = null
      root.loadError = ""
    }
  }

  Process {
    id: refreshProcess
    command: ["omarcs", "refresh", "--quiet"]
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.refreshError = String(text || "").trim()
    }
    onExited: function(exitCode) {
      summaryFile.reload()
      if (exitCode !== 0 && root.refreshError === "") root.refreshError = "Demo scan failed"
    }
  }

  Timer {
    interval: Math.max(5, Number(root.setting("refreshMinutes", 30))) * 60 * 1000
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: root.refreshNow()
  }

  onOpenedChanged: if (opened) {
    summaryFile.reload()
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  IpcHandler {
    target: root.ipcTarget
    function open() { root.open() }
    function close() { root.close() }
    function show() { root.open() }
    function hide() { root.close() }
    function toggle() { root.toggle() }
    function refresh(): string { root.refreshNow(); return "ok" }
    function older(): string { root.selectOlder(); return "ok" }
    function newer(): string { root.selectNewer(); return "ok" }
  }

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: root.selectedMatch && root.stats
      ? root.stats.result + "·" + root.formatNumber(root.stats.rating, 2)
      : "CS2"
    fontSize: Style.font.caption
    horizontalMargin: Style.space(5)
    foreground: root.selectedMatch && root.stats ? root.resultColor(root.stats.result) : root.foreground
    active: root.busy
    tooltipText: root.selectedMatch ? root.selectedMatch.map + "  " + root.score(root.selectedMatch) : "omarCS — import a demo"
    onPressed: function(buttonCode) {
      if (buttonCode === Qt.MiddleButton) root.refreshNow()
      else root.toggle()
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(420))
    contentHeight: panel.fittedContentHeight(content.implicitHeight, Style.space(640))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onMoveRequested: function(dx, dy) {
        if (dx < 0) root.selectOlder()
        else if (dx > 0) root.selectNewer()
      }
      onActivateRequested: root.refreshNow()
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }
      onTextKey: function(text) { if (text === "r" || text === "R") root.refreshNow() }

      Column {
        id: content
        width: parent.width
        spacing: Style.space(12)

        PanelHero {
          width: parent.width
          title: root.selectedMatch ? root.selectedMatch.map.replace("de_", "").toUpperCase() : "omarCS"
          meta: root.selectedMatch
            ? root.stats.result + "  " + root.score(root.selectedMatch) + "  ·  " + root.selectedMatch.player.name
            : "LOCAL CS2 MATCH ANALYSIS"
          foreground: root.foreground
          fontFamily: root.fontFamily
          iconComponent: Component {
            Text {
              text: root.selectedMatch && root.stats ? root.stats.result : "2"
              color: root.selectedMatch && root.stats ? root.resultColor(root.stats.result) : root.foreground
              font.family: root.fontFamily
              font.bold: true
              font.pixelSize: Style.font.display
              horizontalAlignment: Text.AlignHCenter
              verticalAlignment: Text.AlignVCenter
            }
          }
        }

        Row {
          visible: root.matchCount > 1
          width: parent.width
          spacing: Style.space(8)

          Button {
            width: Style.space(104)
            text: "‹  OLDER"
            enabled: root.selectedIndex < root.matchCount - 1
            bordered: true
            foreground: root.foreground
            fontFamily: root.fontFamily
            fontSize: Style.font.caption
            verticalPadding: Style.space(5)
            onClicked: root.selectOlder()
          }

          Text {
            width: parent.width - Style.space(224)
            anchors.verticalCenter: parent.verticalCenter
            text: (root.selectedIndex + 1) + " / " + root.matchCount + (root.selectedIndex === 0 ? "  ·  NEWEST" : "")
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
            horizontalAlignment: Text.AlignHCenter
          }

          Button {
            width: Style.space(104)
            text: "NEWER  ›"
            enabled: root.selectedIndex > 0
            bordered: true
            foreground: root.foreground
            fontFamily: root.fontFamily
            fontSize: Style.font.caption
            verticalPadding: Style.space(5)
            onClicked: root.selectNewer()
          }
        }

        Text {
          visible: !root.selectedMatch
          width: parent.width
          text: root.busy
            ? "Scanning your demo folders…"
            : (root.loadError || root.refreshError || (root.report ? root.report.message : "Import a demo to begin:\nomarcs import ~/Downloads/match.dem"))
          color: root.busy ? root.foreground : root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          horizontalAlignment: Text.AlignHCenter
          wrapMode: Text.WordWrap
          topPadding: Style.space(18)
          bottomPadding: Style.space(18)
        }

        Column {
          visible: !!root.selectedMatch
          width: parent.width
          spacing: Style.space(9)

          PanelSectionHeader {
            width: parent.width
            text: "MATCH " + (root.selectedIndex + 1) + " OF " + root.matchCount
            foreground: root.foreground
            fontFamily: root.fontFamily
          }

          Row {
            width: parent.width
            spacing: Style.space(6)

            Repeater {
              model: root.stats ? [
                { label: "K–D", value: root.stats.kills + "–" + root.stats.deaths },
                { label: "ADR", value: root.formatNumber(root.stats.adr, 1) },
                { label: "KAST", value: root.formatNumber(root.stats.kast, 0) + "%" },
                { label: "RATING", value: root.formatNumber(root.stats.rating, 2) }
              ] : []

              Rectangle {
                required property var modelData
                width: (content.width - Style.space(18)) / 4
                height: Style.space(66)
                radius: Style.cornerRadius
                color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.06)
                border.color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.16)

                Column {
                  anchors.centerIn: parent
                  spacing: Style.space(3)
                  Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: modelData.value
                    color: root.foreground
                    font.family: root.fontFamily
                    font.bold: true
                    font.pixelSize: Style.font.body
                  }
                  Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: modelData.label
                    color: root.dim
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.caption
                  }
                }
              }
            }
          }

          Text {
            width: parent.width
            text: root.stats
              ? "HS " + root.formatNumber(root.stats.headshotPercent, 0) + "%   ·   OPENINGS " + root.stats.openingKills + "–" + root.stats.openingDeaths + "   ·   TRADES " + root.stats.tradeKills + "   ·   UTIL " + root.stats.utilityDamage
              : ""
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
            horizontalAlignment: Text.AlignHCenter
          }
        }

        PanelSeparator {
          visible: !!root.selectedMatch
          foreground: root.foreground
        }

        Column {
          visible: !!root.selectedMatch
          width: parent.width
          spacing: Style.space(8)

          PanelSectionHeader {
            width: parent.width
            text: "COACH NOTES"
            foreground: root.foreground
            fontFamily: root.fontFamily
          }

          Repeater {
            model: root.selectedMatch ? root.selectedMatch.insights : []
            Text {
              required property var modelData
              width: content.width
              text: "•  " + modelData
              color: root.foreground
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
            }
          }
        }

        PanelSeparator {
          visible: root.recent.length > 1
          foreground: root.foreground
        }

        Column {
          visible: root.recent.length > 1
          width: parent.width
          spacing: Style.space(7)

          PanelSectionHeader {
            width: parent.width
            text: "RECENT  ·  " + root.trends.wins + "W / " + root.trends.matches + "  ·  AVG " + root.formatNumber(root.trends.rating, 2)
            foreground: root.foreground
            fontFamily: root.fontFamily
          }

          Repeater {
            model: root.recent
            Rectangle {
              required property var modelData
              required property int index
              width: content.width
              height: Style.space(28)
              radius: Style.cornerRadius
              color: root.selectedIndex === index
                ? Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.08)
                : "transparent"
              Text {
                anchors.left: parent.left
                anchors.leftMargin: Style.space(6)
                anchors.verticalCenter: parent.verticalCenter
                text: modelData.stats.result
                color: root.resultColor(modelData.stats.result)
                font.family: root.fontFamily
                font.bold: true
                font.pixelSize: Style.font.body
              }
              Text {
                anchors.left: parent.left
                anchors.leftMargin: Style.space(34)
                anchors.verticalCenter: parent.verticalCenter
                text: modelData.map.replace("de_", "") + "  " + root.score(modelData)
                color: root.foreground
                font.family: root.fontFamily
                font.pixelSize: Style.font.body
                font.bold: root.selectedIndex === index
              }
              Text {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                text: modelData.stats.kills + "–" + modelData.stats.deaths + "  ·  " + root.formatNumber(modelData.stats.rating, 2)
                color: root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.caption
              }

              MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: root.selectMatch(index)
              }
            }
          }
        }

        Button {
          width: parent.width
          text: root.busy ? "SCANNING…" : "REFRESH DEMOS  [R]"
          enabled: !root.busy
          bordered: true
          foreground: root.foreground
          fontFamily: root.fontFamily
          onClicked: root.refreshNow()
        }

        Text {
          visible: root.refreshError !== ""
          width: parent.width
          text: root.refreshError
          color: root.urgent
          font.family: root.fontFamily
          font.pixelSize: Style.font.caption
          horizontalAlignment: Text.AlignHCenter
          wrapMode: Text.WordWrap
        }
      }
    }
  }
}
