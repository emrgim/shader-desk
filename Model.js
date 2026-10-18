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

function isSafeTheme(name) {
  return /^[a-z0-9][a-z0-9._-]*$/.test(String(name || ""))
}

function isSafeShaderFileName(name) {
  return /^[A-Za-z0-9._-]+\.glsl$/.test(String(name || ""))
}

// Empty is Off. The filename must be a plain .glsl name, with no control
// characters and no "." or ".." path component. The scan script and the
// theme-set hook also require the canonical file to sit in a shader directory.
function isSafeShaderPath(path) {
  var s = String(path || "")
  if (!s)
    return true
  if (/[\u0000-\u001f\u007f]/.test(s))
    return false
  var parts = s.split("/")
  var name = parts[parts.length - 1]
  if (!isSafeShaderFileName(name))
    return false
  for (var i = 0; i < parts.length; i++) {
    if (parts[i] === "." || parts[i] === "..")
      return false
  }
  return true
}

function parseScan(raw) {
  try {
    var parsed = JSON.parse(String(raw || ""))
    return {
      shaders: (parsed.shaders || []).filter(isSafeShaderPath),
      themes: (parsed.themes || []).filter(isSafeTheme),
      currentTheme: parsed.currentTheme || "",
      currentShader: parsed.currentShader || "",
      shaderDeskBin: parsed.shaderDeskBin || ""
    }
  } catch (error) {
    return { shaders: [], themes: [], currentTheme: "", currentShader: "", shaderDeskBin: "" }
  }
}
