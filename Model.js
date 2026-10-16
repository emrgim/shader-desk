.pragma library

function slug(name) {
  return String(name || "").trim().toLowerCase().replace(/ /g, "-")
}

function localPath(url) {
  var s = String(url || "")
  if (s.indexOf("file://") === 0)
    s = decodeURIComponent(s.slice(7))
  return s
}

function parseConf(text) {
  var map = {}
  String(text || "").split("\n").forEach(function(line) {
    line = line.trim()
    if (!line || line.charAt(0) === "#")
      return
    var eq = line.indexOf("=")
    if (eq < 0)
      return
    map[line.slice(0, eq).trim()] = line.slice(eq + 1).trim()
  })
  return map
}

function serializeConf(map) {
  var keys = Object.keys(map || {}).sort()
  var body = "# Shader Desk: theme=shader path (empty = Off). No line = theme default.\n"
  keys.forEach(function(key) {
    body += key + "=" + (map[key] || "") + "\n"
  })
  return body
}

function shaderLabel(path) {
  var name = String(path || "").split("/").pop()
  if (name.slice(-5) === ".glsl")
    name = name.slice(0, -5)
  return name.replace(/-/g, " ")
}

function parseScan(raw) {
  try {
    var parsed = JSON.parse(String(raw || ""))
    return {
      shaders: parsed.shaders || [],
      themes: parsed.themes || [],
      currentTheme: parsed.currentTheme || "",
      currentShader: parsed.currentShader || "",
      shaderDeskBin: parsed.shaderDeskBin || ""
    }
  } catch (error) {
    return { shaders: [], themes: [], currentTheme: "", currentShader: "", shaderDeskBin: "" }
  }
}
