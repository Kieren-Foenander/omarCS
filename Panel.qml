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
  readonly property bool hasMechanics: !!stats && Number(stats.mechanicsEngagements || 0) > 0
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
    if (raw === null || raw === undefined || raw === "") return "—"
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
                {
                  label: "K–D",
                  value: root.stats.kills + "–" + root.stats.deaths,
                  tooltip: "Kills–deaths for this match.\nMore kills than deaths is generally better."
                },
                {
                  label: "ADR",
                  value: root.formatNumber(root.stats.adr, 1),
                  tooltip: "Average damage dealt per round.\nHigher is better."
                },
                {
                  label: "KAST",
                  value: root.formatNumber(root.stats.kast, 0) + "%",
                  tooltip: "Rounds with a kill, assist, survival, or traded death.\nHigher is better."
                },
                {
                  label: "RATING",
                  value: root.formatNumber(root.stats.rating, 2),
                  tooltip: "Overall performance estimate from kills, deaths,\nassists, impact, KAST, and ADR. Around 1.00 is average."
                }
              ] : []

              Rectangle {
                required property var modelData
                width: (content.width - Style.space(18)) / 4
                height: Style.space(66)
                radius: Style.cornerRadius
                color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, metricHover !== null && metricHover.containsMouse ? 0.10 : 0.06)
                border.color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, metricHover !== null && metricHover.containsMouse ? 0.30 : 0.16)

                Column {
                  anchors.centerIn: parent
                  spacing: Style.space(3)
                  Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: modelData !== null && modelData !== undefined ? modelData.value : ""
                    color: root.foreground
                    font.family: root.fontFamily
                    font.bold: true
                    font.pixelSize: Style.font.body
                  }
                  Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: modelData !== null && modelData !== undefined ? modelData.label : ""
                    color: root.dim
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.caption
                  }
                }

                MouseArea {
                  id: metricHover
                  anchors.fill: parent
                  hoverEnabled: true
                  acceptedButtons: Qt.NoButton
                }

                PanelToolTip {
                  visible: metricHover !== null && metricHover.containsMouse
                  text: modelData !== null && modelData !== undefined ? modelData.tooltip : ""
                  fontFamily: root.fontFamily
                }
              }
            }
          }

          Item {
            width: parent.width
            height: matchDetails.implicitHeight

            Row {
              id: matchDetails
              anchors.horizontalCenter: parent.horizontalCenter
              spacing: Style.space(7)

              Repeater {
                model: root.stats ? [
                  {
                    value: "HS " + root.formatNumber(root.stats.headshotPercent, 0) + "%",
                    tooltip: "Percentage of your kills that were headshots.\nHigher is generally better."
                  },
                  {
                    value: "OPENINGS " + root.stats.openingKills + "–" + root.stats.openingDeaths,
                    tooltip: "Opening kills–opening deaths.\nThese are the first kills and deaths of each round."
                  },
                  {
                    value: "TRADES " + root.stats.tradeKills,
                    tooltip: "Kills that traded a teammate shortly after their death.\nHigher usually indicates effective spacing."
                  },
                  {
                    value: "UTIL " + root.stats.utilityDamage,
                    tooltip: "Enemy damage dealt with HE grenades and fire.\nHigher means more damaging utility value."
                  }
                ] : []

                Item {
                  required property var modelData
                  required property int index
                  width: matchDetailLabel.implicitWidth
                  height: matchDetailLabel.implicitHeight

                  Text {
                    id: matchDetailLabel
                    text: modelData !== null && modelData !== undefined ? (index > 0 ? "·  " : "") + modelData.value : ""
                    color: matchDetailHover !== null && matchDetailHover.containsMouse ? root.foreground : root.dim
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.caption
                  }

                  MouseArea {
                    id: matchDetailHover
                    anchors.fill: parent
                    hoverEnabled: true
                    acceptedButtons: Qt.NoButton
                  }

                  PanelToolTip {
                    visible: matchDetailHover !== null && matchDetailHover.containsMouse
                    text: modelData !== null && modelData !== undefined ? modelData.tooltip : ""
                    fontFamily: root.fontFamily
                  }
                }
              }
            }
          }

          PanelSectionHeader {
            visible: root.hasMechanics
            width: parent.width
            text: "AIM MECHANICS  ·  " + (root.stats && root.stats.mechanicsQuality === "geometry" ? "MAP VISIBILITY" : "BETA")
            foreground: root.foreground
            fontFamily: root.fontFamily
          }

          Row {
            visible: root.hasMechanics
            width: parent.width
            spacing: Style.space(6)

            Repeater {
              model: root.hasMechanics ? [
                {
                  label: "XHAIR",
                  value: root.formatNumber(root.stats.crosshairPlacement, 1) + "°",
                  tooltip: "Median aim movement from first visibility to first damage.\nLower is better; 0° means already on target.\nBased on " + root.stats.mechanicsEngagements + " qualifying duels."
                },
                {
                  label: "TTD",
                  value: root.formatNumber(root.stats.timeToDamageMs, 0) + "ms",
                  tooltip: "Median time from first visibility to first damage.\nLower is generally better; duels over one second are excluded.\nBased on " + root.stats.mechanicsEngagements + " qualifying duels."
                },
                {
                  label: "SPOT ACC",
                  value: root.formatNumber(root.stats.spottedAccuracy, 0) + "%",
                  tooltip: "Shots that hit ÷ shots fired while an enemy was visible.\nHigher is better. Based on " + root.stats.spottedShots + " shots."
                },
                {
                  label: "COUNTER",
                  value: root.formatNumber(root.stats.counterStrafePercent, 0) + "%",
                  tooltip: "Uncrouched rifle shots fired below 34% max movement speed.\nHigher is better. Based on " + root.stats.counterStrafeShots + " shots."
                }
              ] : []

              Rectangle {
                required property var modelData
                width: (content.width - Style.space(18)) / 4
                height: Style.space(58)
                radius: Style.cornerRadius
                color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, mechanicsHover !== null && mechanicsHover.containsMouse ? 0.09 : 0.04)
                border.color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, mechanicsHover !== null && mechanicsHover.containsMouse ? 0.28 : 0.13)

                Column {
                  anchors.centerIn: parent
                  spacing: Style.space(2)
                  Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: modelData !== null && modelData !== undefined ? modelData.value : ""
                    color: root.foreground
                    font.family: root.fontFamily
                    font.bold: true
                    font.pixelSize: Style.font.body
                  }
                  Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: modelData !== null && modelData !== undefined ? modelData.label : ""
                    color: root.dim
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.caption
                  }
                }

                MouseArea {
                  id: mechanicsHover
                  anchors.fill: parent
                  hoverEnabled: true
                  acceptedButtons: Qt.NoButton
                }

                PanelToolTip {
                  visible: mechanicsHover !== null && mechanicsHover.containsMouse
                  text: modelData !== null && modelData !== undefined ? modelData.tooltip : ""
                  fontFamily: root.fontFamily
                }
              }
            }
          }

          Item {
            visible: root.hasMechanics
            width: parent.width
            height: mechanicsDetails.implicitHeight

            Row {
              id: mechanicsDetails
              anchors.horizontalCenter: parent.horizontalCenter
              spacing: Style.space(7)

              Repeater {
                model: root.hasMechanics ? [
                  {
                    value: "FIRST SHOT " + root.formatNumber(root.stats.reactionTimeMs, 0) + "ms",
                    tooltip: "Median time from first enemy visibility to your first shot.\nLower generally means a faster reaction."
                  },
                  {
                    value: "H " + root.formatNumber(root.stats.horizontalAdjustment, 1) + "°",
                    tooltip: "Median horizontal aim correction before first damage.\nLower indicates better left–right pre-aim."
                  },
                  {
                    value: "V " + root.formatNumber(root.stats.verticalAdjustment, 1) + "°",
                    tooltip: "Median vertical aim correction before first damage.\nLower usually indicates better head-height placement."
                  },
                  {
                    value: root.stats.mechanicsEngagements + " DUELS",
                    tooltip: "Engagements with reconstructed first visibility and\nfirst damage within one second, used for aim metrics."
                  }
                ] : []

                Item {
                  required property var modelData
                  required property int index
                  width: mechanicsDetailLabel.implicitWidth
                  height: mechanicsDetailLabel.implicitHeight

                  Text {
                    id: mechanicsDetailLabel
                    text: modelData !== null && modelData !== undefined ? (index > 0 ? "·  " : "") + modelData.value : ""
                    color: mechanicsDetailHover !== null && mechanicsDetailHover.containsMouse ? root.foreground : root.dim
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.caption
                  }

                  MouseArea {
                    id: mechanicsDetailHover
                    anchors.fill: parent
                    hoverEnabled: true
                    acceptedButtons: Qt.NoButton
                  }

                  PanelToolTip {
                    visible: mechanicsDetailHover !== null && mechanicsDetailHover.containsMouse
                    text: modelData !== null && modelData !== undefined ? modelData.tooltip : ""
                    fontFamily: root.fontFamily
                  }
                }
              }
            }
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
