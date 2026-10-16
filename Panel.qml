import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui
import "Model.js" as Model

// Theme/shader picker for emrgim.ShaderDesk. Choosing a shader writes
// ~/.config/shader-desk/theme-shaders.conf and installs the theme-set hook.
Panel {
  id: root
  moduleName: "emrgim.ShaderDesk"
  manageIpc: false

  property var anchorItem: null
  property var hostWidget: null
  property var conf: ({})
  property var shaders: []
  property var themes: []
  property string currentTheme: ""
  property string currentShader: ""
  property string shaderDeskBin: ""
  property string selectedTheme: ""
  property string status: ""
  property string pendingConf: ""
  property string pendingPath: ""
  property bool pendingApply: false

  readonly property string home: Quickshell.env("HOME")
  readonly property var shaderChoices: {
    var rows = [{ label: "Off", path: "" }]
    var list = root.shaders || []
    for (var i = 0; i < list.length; i++)
      rows.push({ label: Model.shaderLabel(list[i]), path: list[i] })
    return rows
  }

  function open() {
    refresh()
    root.controller.show()
  }

  function close() {
    root.controller.hide()
  }

  function toggle() {
    if (root.opened) root.close()
    else root.open()
  }

  function switchPanel(direction) {
    if (root.bar && typeof root.bar.switchPanelFrom === "function")
      return root.bar.switchPanelFrom(root.hostWidget || root, direction)
    return false
  }

  function refresh() {
    confFile.reload()
    scanProc.command = ["bash", Model.localPath(Qt.resolvedUrl("emrgim-shader-desk-scan.sh"))]
    scanProc.running = false
    scanProc.running = true
  }

  function applyScan(raw) {
    var scan = Model.parseScan(raw)
    root.shaders = scan.shaders
    root.themes = scan.themes
    root.currentTheme = scan.currentTheme
    root.currentShader = scan.currentShader
    root.shaderDeskBin = scan.shaderDeskBin
    if (!root.selectedTheme || root.themes.indexOf(root.selectedTheme) < 0)
      root.selectedTheme = root.currentTheme || (root.themes.length ? root.themes[0] : "")
  }

  function savedPath(theme) {
    if (!theme || !root.conf)
      return null
    var value = root.conf[theme]
    if (value === undefined || value === null)
      return null
    return value
  }

  function choose(theme, path) {
    if (!theme) return
    var next = {}
    var key
    for (key in root.conf)
      next[key] = root.conf[key]
    next[theme] = path
    root.conf = next
    root.pendingConf = Model.serializeConf(next)
    root.pendingPath = path
    root.pendingApply = theme === root.currentTheme
    root.status = "Saving " + theme
    prepareProc.running = false
    prepareProc.running = true
  }

  function launchNative() {
    if (!root.shaderDeskBin || launchProc.running) return
    launchProc.command = [root.shaderDeskBin]
    launchProc.running = true
  }

  FileView {
    id: confFile
    path: root.home + "/.config/shader-desk/theme-shaders.conf"
    watchChanges: true
    atomicWrites: true
    printErrors: false
    onLoaded: root.conf = Model.parseConf(text())
    onLoadFailed: root.conf = ({})
    onFileChanged: reload()
  }

  Process {
    id: scanProc
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.applyScan(text)
    }
  }

  Process {
    id: prepareProc
    command: ["bash", "-c", "mkdir -p \"$HOME/.config/shader-desk\" \"$HOME/.config/omarchy/hooks/theme-set.d\" && rm -f \"$HOME/.config/shader-desk/manual-off\""]
    onExited: {
      confFile.setText(root.pendingConf)
      installHookProc.command = ["cp", "--", Model.localPath(Qt.resolvedUrl("emrgim-shader-desk-theme-set.sh")), root.home + "/.config/omarchy/hooks/theme-set.d/emrgim-shader-desk.sh"]
      installHookProc.running = false
      installHookProc.running = true
      if (root.pendingApply) {
        applyProc.shaderPath = root.pendingPath
        applyProc.running = false
        applyProc.running = true
      } else {
        root.status = "Saved " + root.selectedTheme
      }
    }
  }

  Process {
    id: installHookProc
  }

  Process {
    id: applyProc
    property string shaderPath: ""
    command: ["hyprctl", "keyword", "decoration:screen_shader", shaderPath]
    onExited: function(code) {
      root.status = code === 0 ? "Applied to the current theme" : "Saved, but hyprctl did not apply it"
      root.refresh()
    }
  }

  Process {
    id: launchProc
  }

  KeyboardPanel {
    id: panel
    anchorItem: root.anchorItem
    owner: root.hostWidget || root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(280))
    contentHeight: panel.fittedContentHeight(Style.space(420))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }

      Flickable {
        id: scroller
        anchors.fill: parent
        contentWidth: width
        contentHeight: content.implicitHeight
        clip: true

        Column {
          id: content
          width: scroller.width
          spacing: Style.space(8)

          Text {
            width: parent.width
            text: "Shader Desk"
            color: root.barForeground
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.subtitle
            font.bold: true
            wrapMode: Text.WordWrap
          }

          Text {
            width: parent.width
            text: root.currentTheme ? ("Current theme: " + root.currentTheme) : "Current theme unknown"
            color: root.barForeground
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.body
            wrapMode: Text.WordWrap
          }

          PanelSectionHeader {
            text: "Theme"
            foreground: root.barForeground
            fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
          }

          Text {
            width: parent.width
            visible: root.themes.length === 0
            text: "No Omarchy themes found"
            color: root.barForeground
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.body
            wrapMode: Text.WordWrap
          }

          Repeater {
            model: root.themes
            delegate: Button {
              required property string modelData
              width: content.width
              text: modelData
              selected: modelData === root.selectedTheme
              onClicked: root.selectedTheme = modelData
            }
          }

          PanelSectionHeader {
            text: "Shader"
            foreground: root.barForeground
            fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
          }

          Text {
            width: parent.width
            visible: root.shaders.length === 0
            text: "No shaders in ~/.config/hypr/shaders"
            color: root.barForeground
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.body
            wrapMode: Text.WordWrap
          }

          Repeater {
            model: root.shaderChoices
            delegate: Button {
              required property var modelData
              width: content.width
              text: modelData.label
              tooltipText: modelData.path
              selected: root.savedPath(root.selectedTheme) === modelData.path
              onClicked: root.choose(root.selectedTheme, modelData.path)
            }
          }

          Button {
            width: parent.width
            visible: root.shaderDeskBin !== ""
            text: "Open Shader Desk"
            onClicked: root.launchNative()
          }

          Text {
            width: parent.width
            visible: root.status !== ""
            text: root.status
            color: root.barForeground
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.caption
            wrapMode: Text.WordWrap
          }
        }
      }
    }
  }
}
