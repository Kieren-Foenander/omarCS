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

  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property color urgent: bar ? bar.urgent : Color.urgent
  readonly property color dim: Qt.darker(foreground, 1.55)
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family
  readonly property var latest: report && report.latest ? report.latest : null
  readonly property var stats: latest && latest.stats ? latest.stats : null
  readonly property var recent: report && report.recent ? report.recent : []
  readonly property var trends: report && report.trends ? report.trends : ({ matches: 0, wins: 0, rating: 0, adr: 0, kast: 0 })
  readonly property string status: report ? String(report.status || "empty") : "empty"
  readonly property bool busy: status === "analyzing" || refreshProcess.running

  visible: true
  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  function resultColor(result) {
    if (result === "W") return Color.accent
    if (result === "L") return urgent
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

  function score(match) {
    if (!match || !match.stats) return "—"
    return match.stats.roundsFor + "–" + match.stats.roundsAgainst
  }

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
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: root.latest && root.stats
      ? root.stats.result + " " + root.formatNumber(root.stats.rating, 2)
      : "CS2"
    active: root.busy
    tooltipText: root.latest ? root.latest.map + "  " + root.score(root.latest) : "omarCS — import a demo"
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
          title: root.latest ? root.latest.map.replace("de_", "").toUpperCase() : "omarCS"
          meta: root.latest
            ? root.stats.result + "  " + root.score(root.latest) + "  ·  " + root.latest.player.name
            : "LOCAL CS2 MATCH ANALYSIS"
          foreground: root.foreground
          fontFamily: root.fontFamily
          iconComponent: Component {
            Text {
              text: root.latest && root.stats ? root.stats.result : "2"
              color: root.latest && root.stats ? root.resultColor(root.stats.result) : root.foreground
              font.family: root.fontFamily
              font.bold: true
              font.pixelSize: Style.font.display
              horizontalAlignment: Text.AlignHCenter
              verticalAlignment: Text.AlignVCenter
            }
          }
        }

        Text {
          visible: !root.latest
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
          visible: !!root.latest
          width: parent.width
          spacing: Style.space(9)

          PanelSectionHeader {
            width: parent.width
            text: "LATEST MATCH"
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
          visible: !!root.latest
          foreground: root.foreground
        }

        Column {
          visible: !!root.latest
          width: parent.width
          spacing: Style.space(8)

          PanelSectionHeader {
            width: parent.width
            text: "COACH NOTES"
            foreground: root.foreground
            fontFamily: root.fontFamily
          }

          Repeater {
            model: root.latest ? root.latest.insights : []
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
            Item {
              required property var modelData
              width: content.width
              height: Style.space(24)
              Text {
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                text: modelData.stats.result
                color: root.resultColor(modelData.stats.result)
                font.family: root.fontFamily
                font.bold: true
                font.pixelSize: Style.font.body
              }
              Text {
                anchors.left: parent.left
                anchors.leftMargin: Style.space(28)
                anchors.verticalCenter: parent.verticalCenter
                text: modelData.map.replace("de_", "") + "  " + root.score(modelData)
                color: root.foreground
                font.family: root.fontFamily
                font.pixelSize: Style.font.body
              }
              Text {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                text: modelData.stats.kills + "–" + modelData.stats.deaths + "  ·  " + root.formatNumber(modelData.stats.rating, 2)
                color: root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.caption
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
