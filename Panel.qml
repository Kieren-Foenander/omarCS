import QtQuick
import QtQuick.Controls
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
  property string bootstrapError: ""
  property int selectedIndex: 0
  property string page: "stats"
  property int sprayWeaponIndex: 0

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
  readonly property var comparisonMatch: selectedIndex < matchCount - 1
    ? recent[selectedIndex + 1]
    : null
  readonly property var comparisonStats: comparisonMatch && comparisonMatch.stats
    ? comparisonMatch.stats
    : null
  readonly property bool hasMechanics: !!stats && Number(stats.mechanicsEngagements || 0) > 0
  readonly property var trends: report && report.trends ? report.trends : ({ matches: 0, wins: 0, rating: 0, adr: 0, kast: 0 })
  readonly property var sprayControl: report && report.sprayControl ? report.sprayControl : ({ matches: 0, weapons: [] })
  readonly property var sprayWeapons: sprayControl && sprayControl.weapons ? sprayControl.weapons : []
  readonly property var sprayWeapon: sprayWeapons.length > 0
    ? sprayWeapons[Math.max(0, Math.min(sprayWeaponIndex, sprayWeapons.length - 1))]
    : null
  readonly property bool hasSprayData: sprayWeapons.some(function(weapon) { return Number(weapon.sprays || 0) > 0 })
  readonly property string status: report ? String(report.status || "empty") : "empty"
  readonly property bool busy: status === "analyzing" || refreshProcess.running || bootstrapProcess.running
  readonly property string pluginDir: manifest && manifest.__sourceDir
    ? String(manifest.__sourceDir)
    : Quickshell.env("HOME") + "/.config/omarchy/plugins/omarcs.stats"

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

  function metricDelta(current, previous) {
    var currentValue = Number(current)
    var previousValue = Number(previous)
    if (!isFinite(currentValue) || !isFinite(previousValue)) return null
    var delta = currentValue - previousValue
    return Math.abs(delta) < 0.0001 ? 0 : delta
  }

  function formatDelta(delta, decimals) {
    if (delta === null || delta === undefined || !isFinite(Number(delta))) return ""
    return (delta > 0 ? "+" : "") + Number(delta).toFixed(decimals)
  }

  function deltaColor(delta, higherIsBetter) {
    if (delta === null || delta === undefined || Number(delta) === 0) return dim
    return (Number(delta) > 0) === higherIsBetter ? winColor : lossColor
  }

  function killDeathDifference(matchStats) {
    if (!matchStats) return null
    var kills = Number(matchStats.kills)
    var deaths = Number(matchStats.deaths)
    return isFinite(kills) && isFinite(deaths) ? kills - deaths : null
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

  function selectSprayWeapon(index) {
    if (sprayWeapons.length < 1) return
    sprayWeaponIndex = Math.max(0, Math.min(Number(index), sprayWeapons.length - 1))
  }

  function cycleSprayWeapon(direction) {
    if (sprayWeapons.length < 1) return
    sprayWeaponIndex = (sprayWeaponIndex + direction + sprayWeapons.length) % sprayWeapons.length
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
    command: [root.pluginDir + "/omarcs-plugin", "refresh", "--quiet"]
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.refreshError = String(text || "").trim()
    }
    onExited: function(exitCode) {
      summaryFile.reload()
      if (exitCode !== 0 && root.refreshError === "") root.refreshError = "Demo scan failed"
    }
  }

  // This is the first-run path after a user enables the plugin. It configures
  // automatic match pickup and scans any demo files already on disk.
  Process {
    id: bootstrapProcess
    command: [root.pluginDir + "/omarcs-plugin", "bootstrap"]
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.bootstrapError = String(text || "").trim()
    }
    onExited: function(exitCode) {
      summaryFile.reload()
      if (exitCode !== 0 && root.bootstrapError === "") root.bootstrapError = "Automatic setup failed"
    }
  }

  Component.onCompleted: bootstrapProcess.running = true

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
    function spray(): string { root.page = "spray"; root.open(); return "ok" }
    function stats(): string { root.page = "stats"; root.open(); return "ok" }
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
    contentHeight: panel.fittedContentHeight(content.implicitHeight + Style.space(12) + footer.implicitHeight)

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      clip: true
      onMoveRequested: function(dx, dy) {
        if (dy !== 0 && panelFlick.contentHeight > panelFlick.height) {
          panelFlick.contentY = Math.max(0, Math.min(
            panelFlick.contentY + dy * Style.space(56),
            panelFlick.contentHeight - panelFlick.height
          ))
        }
        if (root.page === "spray") {
          if (dx < 0) root.cycleSprayWeapon(-1)
          else if (dx > 0) root.cycleSprayWeapon(1)
        } else {
          if (dx < 0) root.selectOlder()
          else if (dx > 0) root.selectNewer()
        }
      }
      onActivateRequested: root.refreshNow()
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }
      onTextKey: function(text) { if (text === "r" || text === "R") root.refreshNow() }

      Flickable {
        id: panelFlick
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: footer.top
        anchors.bottomMargin: Style.space(12)
        contentWidth: width
        contentHeight: content.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick
        interactive: contentHeight > height
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

        Column {
          id: content
          width: panelFlick.width
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
          visible: !!root.selectedMatch && root.hasSprayData
          width: parent.width
          spacing: Style.space(6)

          Button {
            width: (parent.width - Style.space(6)) / 2
            text: "MATCH STATS"
            bordered: true
            foreground: root.page === "stats" ? root.winColor : root.dim
            fontFamily: root.fontFamily
            fontSize: Style.font.caption
            verticalPadding: Style.space(5)
            onClicked: root.page = "stats"
          }

          Button {
            width: (parent.width - Style.space(6)) / 2
            text: "SPRAY CONTROL"
            bordered: true
            foreground: root.page === "spray" ? root.winColor : root.dim
            fontFamily: root.fontFamily
            fontSize: Style.font.caption
            verticalPadding: Style.space(5)
            onClicked: root.page = "spray"
          }
        }

        Row {
          visible: root.matchCount > 1 && root.page === "stats"
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
            : (root.loadError || root.refreshError || root.bootstrapError || (root.report ? root.report.message : "Looking for CS2 and demos…"))
          color: root.busy ? root.foreground : root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          horizontalAlignment: Text.AlignHCenter
          wrapMode: Text.WordWrap
          topPadding: Style.space(18)
          bottomPadding: Style.space(18)
        }

        Column {
          visible: !!root.selectedMatch && root.page === "stats"
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
                  delta: root.metricDelta(root.killDeathDifference(root.stats), root.killDeathDifference(root.comparisonStats)),
                  deltaDecimals: 0,
                  higherIsBetter: true,
                  title: "Kills–Deaths",
                  tooltip: "Kills–deaths for this match.\nMore kills than deaths is generally better.\nChange is compared with your previous game."
                },
                {
                  label: "ADR",
                  value: root.formatNumber(root.stats.adr, 1),
                  delta: root.metricDelta(root.stats.adr, root.comparisonStats ? root.comparisonStats.adr : null),
                  deltaDecimals: 1,
                  higherIsBetter: true,
                  title: "Average Damage per Round",
                  tooltip: "Average damage dealt per round.\nHigher is better.\nChange is compared with your previous game."
                },
                {
                  label: "KAST",
                  value: root.formatNumber(root.stats.kast, 0) + "%",
                  delta: root.metricDelta(root.stats.kast, root.comparisonStats ? root.comparisonStats.kast : null),
                  deltaDecimals: 0,
                  higherIsBetter: true,
                  title: "Kill, Assist, Survive, or Trade",
                  tooltip: "Rounds with a kill, assist, survival, or traded death.\nHigher is better.\nChange is compared with your previous game."
                },
                {
                  label: "RATING",
                  value: root.formatNumber(root.stats.rating, 2),
                  delta: root.metricDelta(root.stats.rating, root.comparisonStats ? root.comparisonStats.rating : null),
                  deltaDecimals: 2,
                  higherIsBetter: true,
                  title: "Rating",
                  tooltip: "Overall performance estimate from kills, deaths,\nassists, impact, KAST, and ADR. Around 1.00 is average.\nChange is compared with your previous game."
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

                  Row {
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: Style.space(3)

                    Text {
                      text: modelData !== null && modelData !== undefined ? modelData.value : ""
                      color: root.foreground
                      font.family: root.fontFamily
                      font.bold: true
                      font.pixelSize: Style.font.body
                    }

                    Text {
                      visible: modelData !== null && modelData !== undefined && modelData.delta !== null
                      anchors.verticalCenter: parent.verticalCenter
                      text: visible ? root.formatDelta(modelData.delta, modelData.deltaDecimals) : ""
                      color: root.deltaColor(modelData.delta, modelData.higherIsBetter)
                      font.family: root.fontFamily
                      font.bold: true
                      font.pixelSize: Style.font.caption
                    }
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
                  text: modelData !== null && modelData !== undefined ? modelData.title + "\n" + modelData.tooltip : ""
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
                  title: "Headshot Percentage",
                  tooltip: "Percentage of your kills that were headshots.\nHigher is generally better."
                },
                {
                  value: "OPENINGS " + root.stats.openingKills + "–" + root.stats.openingDeaths,
                  title: "Opening Kills–Opening Deaths",
                  tooltip: "Opening kills–opening deaths.\nThese are the first kills and deaths of each round."
                },
                {
                  value: "TRADES " + root.stats.tradeKills,
                  title: "Trade Kills",
                  tooltip: "Kills that traded a teammate shortly after their death.\nHigher usually indicates effective spacing."
                },
                {
                  value: "UTIL " + root.stats.utilityDamage,
                  title: "Utility Damage",
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
                    text: modelData !== null && modelData !== undefined ? modelData.title + "\n" + modelData.tooltip : ""
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
                  delta: root.metricDelta(root.stats.crosshairPlacement, root.comparisonStats ? root.comparisonStats.crosshairPlacement : null),
                  deltaDecimals: 1,
                  higherIsBetter: false,
                  title: "Crosshair Placement",
                  tooltip: "Median aim movement from first visibility to first damage.\nLower is better; 0° means already on target.\nBased on " + root.stats.mechanicsEngagements + " qualifying duels.\nChange is compared with your previous game."
                },
                {
                  label: "TTD",
                  value: root.formatNumber(root.stats.timeToDamageMs, 0) + "ms",
                  delta: root.metricDelta(root.stats.timeToDamageMs, root.comparisonStats ? root.comparisonStats.timeToDamageMs : null),
                  deltaDecimals: 0,
                  higherIsBetter: false,
                  title: "Time to Damage",
                  tooltip: "Median time from first visibility to first damage.\nLower is generally better; duels over one second are excluded.\nBased on " + root.stats.mechanicsEngagements + " qualifying duels.\nChange is compared with your previous game."
                },
                {
                  label: "SPOT ACC",
                  value: root.formatNumber(root.stats.spottedAccuracy, 0) + "%",
                  delta: root.metricDelta(root.stats.spottedAccuracy, root.comparisonStats ? root.comparisonStats.spottedAccuracy : null),
                  deltaDecimals: 0,
                  higherIsBetter: true,
                  title: "Spotted Accuracy",
                  tooltip: "Shots that hit ÷ shots fired while an enemy was visible.\nHigher is better. Based on " + root.stats.spottedShots + " shots.\nChange is compared with your previous game."
                },
                {
                  label: "COUNTER",
                  value: root.formatNumber(root.stats.counterStrafePercent, 0) + "%",
                  delta: root.metricDelta(root.stats.counterStrafePercent, root.comparisonStats ? root.comparisonStats.counterStrafePercent : null),
                  deltaDecimals: 0,
                  higherIsBetter: true,
                  title: "Counter-Strafe",
                  tooltip: "Uncrouched rifle shots fired below 34% max movement speed.\nHigher is better. Based on " + root.stats.counterStrafeShots + " shots.\nChange is compared with your previous game."
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

                  Row {
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: Style.space(3)

                    Text {
                      text: modelData !== null && modelData !== undefined ? modelData.value : ""
                      color: root.foreground
                      font.family: root.fontFamily
                      font.bold: true
                      font.pixelSize: Style.font.body
                    }

                    Text {
                      visible: modelData !== null && modelData !== undefined && modelData.delta !== null
                      anchors.verticalCenter: parent.verticalCenter
                      text: visible ? root.formatDelta(modelData.delta, modelData.deltaDecimals) : ""
                      color: root.deltaColor(modelData.delta, modelData.higherIsBetter)
                      font.family: root.fontFamily
                      font.bold: true
                      font.pixelSize: Style.font.caption
                    }
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
                  text: modelData !== null && modelData !== undefined ? modelData.title + "\n" + modelData.tooltip : ""
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
                  title: "Reaction Time",
                  tooltip: "Median time from first enemy visibility to your first shot.\nLower generally means a faster reaction."
                },
                {
                  value: "H " + root.formatNumber(root.stats.horizontalAdjustment, 1) + "°",
                  title: "Horizontal Adjustment",
                  tooltip: "Median horizontal aim correction before first damage.\nLower indicates better left–right pre-aim."
                },
                {
                  value: "V " + root.formatNumber(root.stats.verticalAdjustment, 1) + "°",
                  title: "Vertical Adjustment",
                  tooltip: "Median vertical aim correction before first damage.\nLower usually indicates better head-height placement."
                },
                {
                  value: root.stats.mechanicsEngagements + " DUELS",
                  title: "Qualifying Duels",
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
                    text: modelData !== null && modelData !== undefined ? modelData.title + "\n" + modelData.tooltip : ""
                    fontFamily: root.fontFamily
                  }
                }
              }
            }
          }
        }

        PanelSeparator {
          visible: !!root.selectedMatch && root.page === "stats"
          foreground: root.foreground
        }

        Column {
          visible: !!root.selectedMatch && root.page === "stats"
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
          visible: root.recent.length > 1 && root.page === "stats"
          foreground: root.foreground
        }

        Column {
          visible: root.recent.length > 1 && root.page === "stats"
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

        Column {
          visible: !!root.selectedMatch && root.page === "spray"
          width: parent.width
          spacing: Style.space(9)

          PanelSectionHeader {
            width: parent.width
            text: "SPRAY CONTROL  ·  LAST " + root.sprayControl.matches + " MATCHES"
            foreground: root.foreground
            fontFamily: root.fontFamily
          }

          Row {
            width: parent.width
            spacing: Style.space(6)

            Repeater {
              model: root.sprayWeapons

              Button {
                required property var modelData
                required property int index
                width: (content.width - Style.space(18)) / 4
                text: modelData.shortName + " · " + modelData.sprays
                bordered: true
                foreground: root.sprayWeaponIndex === index ? root.winColor : root.dim
                fontFamily: root.fontFamily
                fontSize: Style.font.caption
                verticalPadding: Style.space(4)
                tooltipText: modelData.name + " — " + modelData.sprays + " qualifying sprays"
                onClicked: root.selectSprayWeapon(index)
              }
            }
          }

          Text {
            width: parent.width
            text: root.sprayWeapon
              ? root.sprayWeapon.sprays + " SPRAYS   ·   BULLETS 1–10   ·   CONFIDENCE " + root.sprayWeapon.confidence
              : "NO SPRAY DATA"
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
            horizontalAlignment: Text.AlignHCenter
          }

          Canvas {
            id: sprayCanvas
            width: parent.width
            height: Style.space(224)

            property var weapon: root.sprayWeapon
            property color foreground: root.foreground
            property color dimColor: root.dim
            property color accent: root.winColor

            function clamp(value, minimum, maximum) {
              return Math.max(minimum, Math.min(maximum, value))
            }

            function drawTarget(ctx, centerX, centerY) {
              ctx.strokeStyle = Qt.rgba(foreground.r, foreground.g, foreground.b, 0.20)
              ctx.lineWidth = 1
              for (var radius = 22; radius <= 66; radius += 22) {
                ctx.beginPath()
                ctx.arc(centerX, centerY, radius, 0, Math.PI * 2)
                ctx.stroke()
              }
              ctx.setLineDash([3, 4])
              ctx.beginPath()
              ctx.moveTo(centerX - 72, centerY)
              ctx.lineTo(centerX + 72, centerY)
              ctx.moveTo(centerX, centerY - 72)
              ctx.lineTo(centerX, centerY + 72)
              ctx.stroke()
              ctx.setLineDash([])

              ctx.fillStyle = Qt.rgba(foreground.r, foreground.g, foreground.b, 0.05)
              ctx.strokeStyle = Qt.rgba(foreground.r, foreground.g, foreground.b, 0.24)
              ctx.beginPath()
              ctx.arc(centerX, centerY, 13, 0, Math.PI * 2)
              ctx.fill()
              ctx.stroke()
              ctx.beginPath()
              ctx.moveTo(centerX - 35, centerY + 66)
              ctx.lineTo(centerX - 31, centerY + 38)
              ctx.quadraticCurveTo(centerX, centerY + 22, centerX + 31, centerY + 38)
              ctx.lineTo(centerX + 35, centerY + 66)
              ctx.closePath()
              ctx.fill()
              ctx.stroke()
            }

            function drawDot(ctx, x, y, number, color, radius) {
              ctx.fillStyle = Qt.rgba(accent.r, accent.g, accent.b, 0.10)
              ctx.strokeStyle = Qt.rgba(accent.r, accent.g, accent.b, 0.40)
              ctx.beginPath()
              ctx.arc(x, y, radius, 0, Math.PI * 2)
              ctx.fill()
              ctx.stroke()

              ctx.fillStyle = Qt.rgba(0.04, 0.05, 0.08, 0.96)
              ctx.strokeStyle = color
              ctx.lineWidth = 1.5
              ctx.beginPath()
              ctx.arc(x, y, 6, 0, Math.PI * 2)
              ctx.fill()
              ctx.stroke()
              ctx.fillStyle = foreground
              ctx.font = "bold 9px " + root.fontFamily
              ctx.textAlign = "center"
              ctx.textBaseline = "middle"
              ctx.fillText(String(number), x, y + 0.5)
            }

            onWeaponChanged: requestPaint()
            onForegroundChanged: requestPaint()
            onWidthChanged: requestPaint()
            Component.onCompleted: requestPaint()

            onPaint: {
              var ctx = getContext("2d")
              ctx.clearRect(0, 0, width, height)
              var gap = Style.space(8)
              var cardWidth = (width - gap) / 2
              var centerY = height / 2 + Style.space(8)
              var perfectX = cardWidth / 2
              var actualX = cardWidth + gap + cardWidth / 2

              ctx.fillStyle = Qt.rgba(foreground.r, foreground.g, foreground.b, 0.035)
              ctx.strokeStyle = Qt.rgba(foreground.r, foreground.g, foreground.b, 0.15)
              ctx.lineWidth = 1
              ctx.fillRect(0, 0, cardWidth, height)
              ctx.strokeRect(0.5, 0.5, cardWidth - 1, height - 1)
              ctx.fillRect(cardWidth + gap, 0, cardWidth, height)
              ctx.strokeRect(cardWidth + gap + 0.5, 0.5, cardWidth - 1, height - 1)

              ctx.fillStyle = foreground
              ctx.font = "bold 10px " + root.fontFamily
              ctx.textAlign = "center"
              ctx.textBaseline = "middle"
              ctx.fillText("PERFECT CONTROL", perfectX, Style.space(13))
              ctx.fillText("YOUR MEDIAN", actualX, Style.space(13))
              drawTarget(ctx, perfectX, centerY)
              drawTarget(ctx, actualX, centerY)

              var ideal = [[0, 0], [8, -2], [-8, 2], [3, 8], [-3, -8], [10, 8], [-10, -8], [-10, 8], [10, -8], [0, 12]]
              for (var idealIndex = 0; idealIndex < ideal.length; idealIndex++) {
                drawDot(ctx, perfectX + ideal[idealIndex][0], centerY - ideal[idealIndex][1], idealIndex + 1, accent, 7)
              }

              var shots = weapon && weapon.shots ? weapon.shots : []
              if (shots.length > 0) {
                ctx.strokeStyle = Qt.rgba(0.21, 0.82, 0.73, 0.50)
                ctx.setLineDash([4, 4])
                ctx.beginPath()
                for (var pathIndex = 0; pathIndex < shots.length; pathIndex++) {
                  var pathShot = shots[pathIndex]
                  var pathX = actualX + clamp(Number(pathShot.x) * 0.8, -70, 70)
                  var pathY = centerY - clamp(Number(pathShot.y) * 0.8, -70, 70)
                  if (pathIndex === 0) ctx.moveTo(pathX, pathY)
                  else ctx.lineTo(pathX, pathY)
                }
                ctx.stroke()
                ctx.setLineDash([])
                for (var shotIndex = 0; shotIndex < shots.length; shotIndex++) {
                  var shot = shots[shotIndex]
                  var shotX = actualX + clamp(Number(shot.x) * 0.8, -70, 70)
                  var shotY = centerY - clamp(Number(shot.y) * 0.8, -70, 70)
                  var spread = clamp(Math.max(Number(shot.radiusX), Number(shot.radiusY)) * 0.5, 7, 19)
                  drawDot(ctx, shotX, shotY, shot.number, "#35d0ba", spread)
                }
              } else {
                ctx.fillStyle = dimColor
                ctx.font = "10px " + root.fontFamily
                ctx.fillText("MORE MATCHES NEEDED", actualX, centerY)
              }
            }

            MouseArea {
              id: sprayHover
              anchors.fill: parent
              hoverEnabled: true
              acceptedButtons: Qt.NoButton
            }

            PanelToolTip {
              visible: sprayHover.containsMouse
              text: "Each number is the median position of that bullet across qualifying sprays.\nThe halo is the middle 50% of results. Centre represents the enemy's head."
              fontFamily: root.fontFamily
            }
          }

          Text {
            width: parent.width
            text: "CENTRE = ENEMY HEAD   ·   NUMBER = BULLET ORDER   ·   HALO = MIDDLE 50%"
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
            horizontalAlignment: Text.AlignHCenter
          }

          Rectangle {
            width: parent.width
            height: sprayCoach.implicitHeight + Style.space(20)
            radius: Style.cornerRadius
            color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.05)
            border.color: Qt.rgba(root.winColor.r, root.winColor.g, root.winColor.b, 0.45)

            Text {
              id: sprayCoach
              anchors.left: parent.left
              anchors.right: parent.right
              anchors.verticalCenter: parent.verticalCenter
              anchors.margins: Style.space(10)
              text: root.sprayWeapon ? "COACH  ·  " + root.sprayWeapon.coach : "Keep collecting matches to build a spray profile."
              color: root.foreground
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
            }
          }
        }
        }
      }

      Column {
        id: footer
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        spacing: Style.space(8)

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
