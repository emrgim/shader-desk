//! Shader Desk: pick, create and edit Hyprland screen shaders.
//! `shader-desk` (GUI) | `--editor` | `hook <theme>` | `apply <path|off>` | `current` | `gen-presets`
//!
//! Screen-shader paths are argv to `hyprctl`, never a shell string. Legacy
//! configs use `keyword decoration:screen_shader`. Omarchy Quattro's Lua
//! config rejects keyword, so that case uses `eval` only with a canonical
//! path from an allowed directory, encoded as a Lua literal (`shader_path`).
use adw::prelude::*;
use gtk::{gdk, glib};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::BTreeMap, fs, path::{Path, PathBuf}, process::Command, rc::Rc, sync::OnceLock, time::Duration};

mod shader_path;

fn home() -> String { std::env::var("HOME").unwrap_or_else(|_| "/home/emiliano".into()) }
fn sd() -> String { format!("{}/.config/hypr/shaders", home()) }
fn shader_dir() -> PathBuf {
    let raw = PathBuf::from(sd());
    fs::canonicalize(&raw).unwrap_or(raw)
}
fn roots() -> Vec<PathBuf> {
    vec![shader_dir(), PathBuf::from(format!("{}/.cache/shader-desk", home()))]
}
fn conf_path() -> String { format!("{}/.config/shader-desk/theme-shaders.conf", home()) }
fn flag_path() -> String { format!("{}/.config/shader-desk/manual-off", home()) }
const DEFAULT: &str = "__default__";
const FILTERS: [&str; 4] = ["CRT", "NTSC", "Game Boy", "LCD"];
const GRILLES: [&str; 3] = ["aperture", "slot mask", "shadow mask"];
const PHOS: [&str; 5] = ["none", "green", "orange", "amber", "white"];

fn defaults() -> Value {
    json!({"name":"","filter":"CRT","curvature":true,"scanlines":true,"grille":true,"bloom":true,"vignette":true,
           "grille_type":"aperture","phosphor":"none","scan_strength":0.35,"scan_spacing":3.0,"curv":0.03,
           "bloom_amt":0.06,"vig":0.22,"grille_strength":0.10,"noise":0.05})
}
/// Overlay preset fields onto the defaults.
/// A non-object, or a known field of an unexpected JSON type, is ignored so
/// metadata from a shader file cannot panic the editor. Valid fields are kept.
fn preset(over: Value) -> Value {
    let mut d = defaults();
    let Some(obj) = over.as_object() else {
        eprintln!("shader-desk: ignored non-object shader metadata");
        return d;
    };
    let mut ignored = false;
    for (k, v) in obj {
        match accepted_meta_field(k, v) {
            Some(clean) => d[k] = clean,
            None => ignored = true,
        }
    }
    if ignored {
        eprintln!("shader-desk: ignored shader metadata field with an unexpected type");
    }
    d
}
fn accepted_meta_field(key: &str, value: &Value) -> Option<Value> {
    let ok = match key {
        "name" | "filter" | "grille_type" | "phosphor" => value.is_string(),
        "curvature" | "scanlines" | "grille" | "bloom" | "vignette" => value.is_boolean(),
        "scan_strength" | "scan_spacing" | "curv" | "bloom_amt" | "vig" | "grille_strength" | "noise" => value.is_number(),
        _ => false,
    };
    ok.then(|| value.clone())
}
struct LoadedPreset { params: Value, overwrite: bool }
/// `// shader-desk:` metadata from a shader file. Only a JSON object is a preset.
/// Anything else is ignored and does not replace the file on Save.
fn load_shader_preset(src: &str) -> LoadedPreset {
    let Some(line) = shader_meta_line(src) else {
        return LoadedPreset { params: defaults(), overwrite: false };
    };
    match serde_json::from_str::<Value>(line) {
        Ok(value) if value.is_object() => LoadedPreset { params: preset(value), overwrite: true },
        Ok(_) => {
            eprintln!("shader-desk: ignored non-object shader metadata");
            LoadedPreset { params: defaults(), overwrite: false }
        }
        Err(_) => {
            eprintln!("shader-desk: ignored malformed shader metadata");
            LoadedPreset { params: defaults(), overwrite: false }
        }
    }
}
/// Params and overwrite flag used when Edit opens a user shader.
/// `friendly_name` is the label used when the file has no usable preset.
fn user_shader_edit(src: &str, friendly_name: &str) -> (Value, bool) {
    let loaded = load_shader_preset(src);
    if loaded.overwrite {
        (loaded.params, true)
    } else {
        let mut params = defaults();
        params["name"] = json!(format!("{friendly_name} custom"));
        (params, false)
    }
}
/// Built-in presets: (file, name, params used when Edit makes a copy). Protected from Delete.
fn builtins() -> Vec<(String, &'static str, Value)> {
    let d = shader_dir();
    let p = |n: &str| d.join(n).to_string_lossy().into_owned();
    vec![
        (p("phosphor-orange.glsl"), "Phosphor Orange", preset(json!({"phosphor":"orange","curvature":false,"grille":false,"bloom_amt":0.05}))),
        (p("phosphor.glsl"), "Phosphor Green", preset(json!({"phosphor":"green","curvature":false,"grille":false,"bloom_amt":0.05}))),
        (p("ntsc.glsl"), "NTSC", preset(json!({"name":"NTSC","filter":"NTSC","curv":0.02,"scan_strength":0.25,"grille":false,"bloom_amt":0.08,"vig":0.2,"noise":0.06}))),
        (p("game-boy.glsl"), "Game Boy", preset(json!({"name":"Game Boy","filter":"Game Boy","curvature":false,"scanlines":false,"grille_strength":0.25,"scan_spacing":3.0,"bloom":false,"vig":0.1}))),
        (p("aperture-grille.glsl"), "Aperture Grille", preset(json!({"scan_strength":0.55,"scan_spacing":2.0,"curv":0.035,"grille_strength":0.10,"bloom_amt":0.06}))),
        (p("lcd.glsl"), "LCD", preset(json!({"name":"LCD","filter":"LCD","curvature":false,"scanlines":false,"grille":false,"vignette":false,"bloom_amt":0.03}))),
    ]
}
fn is_builtin(p: &str) -> bool { builtins().iter().any(|b| b.0 == p) }

// ---------- hyprland ----------
fn current() -> String {
    let out = Command::new("hyprctl").args(["getoption", "decoration:screen_shader"]).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    let v = out.lines().find_map(|l| l.strip_prefix("str:")).map(|s| s.trim().to_string()).unwrap_or_default();
    if v == "[[EMPTY]]" { String::new() } else { v }
}
fn hypr_mode() -> shader_path::HyprConfigMode {
    static MODE: OnceLock<shader_path::HyprConfigMode> = OnceLock::new();
    *MODE.get_or_init(|| {
        if let Ok(forced) = std::env::var("SHADER_DESK_HYPR_MODE") {
            return match forced.as_str() {
                "lua" | "eval" => shader_path::HyprConfigMode::Lua,
                _ => shader_path::HyprConfigMode::Legacy,
            };
        }
        // No value: Hyprland returns before parseKeyword, so the probe does not change the shader.
        let out = Command::new("hyprctl").args(["keyword", "decoration:screen_shader"]).output();
        let Ok(out) = out else { return shader_path::HyprConfigMode::Legacy };
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        shader_path::mode_from_probe(&text)
    })
}
/// Off sets the sticky manual-off flag; any accepted shader clears it.
/// Refused paths are not sent to hyprctl and do not change the flag.
fn try_apply(path: &str) -> Result<String, String> {
    let args = shader_path::screen_shader_argv(path, &roots(), hypr_mode()).map_err(|e| e.to_string())?;
    let _ = fs::create_dir_all(format!("{}/.config/shader-desk", home()));
    if path.is_empty() { let _ = fs::write(flag_path(), ""); } else { let _ = fs::remove_file(flag_path()); }
    // argv only. `hyprctl` is not reached through a shell.
    let out = Command::new("hyprctl").args(&args).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("hyprctl exited {}", out.status));
    }
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if text.contains("non-legacy") || text.contains("Use eval") {
        return Err("hyprctl keyword was rejected by the Lua config".into());
    }
    Ok(current())
}
fn apply(path: &str) -> String {
    try_apply(path).unwrap_or_else(|e| { eprintln!("shader-desk: {e}"); current() })
}

// ---------- conf ----------
fn load_conf() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    if let Ok(s) = fs::read_to_string(conf_path()) {
        for l in s.lines().map(str::trim) {
            if l.is_empty() || l.starts_with('#') { continue; }
            if let Some((k, v)) = l.split_once('=') { m.insert(k.trim().to_string(), v.trim().to_string()); }
        }
    }
    m
}
fn save_conf(m: &BTreeMap<String, String>) {
    let _ = fs::create_dir_all(format!("{}/.config/shader-desk", home()));
    let mut s = String::from("# Shader Desk: theme=shader path (empty = Off). No line = theme default.\n");
    for (k, v) in m {
        let Some(k) = shader_path::safe_theme_key(k) else { continue };
        if !v.is_empty() && shader_path::validate_shader_path(v, &roots()).is_err() { continue; }
        s += &format!("{k}={v}\n");
    }
    let _ = fs::write(conf_path(), s);
}
fn slugify(s: &str) -> String { regex::Regex::new("[^a-z0-9]+").unwrap().replace_all(&s.to_lowercase(), "-").trim_matches('-').to_string() }

/// Theme-set hook: manual-off wins, then theme-shaders.conf, then the theme's own shader, else Off.
/// Theme keys and shader paths that fail the safe-path gate clear the shader instead of being applied.
fn hook(theme: &str) {
    if Path::new(&flag_path()).exists() { apply(""); return; }
    let Some(theme) = shader_path::safe_theme_key(theme) else { apply(""); return; };
    let conf = load_conf();
    if let Some(v) = conf.get(&theme).cloned() {
        if v.is_empty() { apply(""); return; }
        if shader_path::validate_shader_path(&v, &roots()).is_ok() { apply(&v); return; }
        eprintln!("shader-desk: ignored unsafe shader path");
        apply("");
        return;
    }
    let f = if theme == "phosphor-green" { shader_dir().join("phosphor.glsl") } else { shader_dir().join(format!("{theme}.glsl")) };
    let f = f.to_string_lossy().into_owned();
    if shader_path::validate_shader_path(&f, &roots()).is_ok() { apply(&f); } else { apply(""); }
}

// ---------- shaders ----------
const META_PREFIX: &str = "// shader-desk: ";
fn shader_meta_line(src: &str) -> Option<&str> {
    src.lines().take(5).find_map(|l| l.strip_prefix(META_PREFIX))
}
fn read_params(path: &str) -> Option<Value> {
    let s = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(shader_meta_line(&s)?).ok()?;
    if value.is_object() { Some(value) } else { None }
}
fn fname(p: &str) -> String { Path::new(p).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default() }
fn friendly(path: &str) -> String {
    if let Some(b) = builtins().into_iter().find(|b| b.0 == path) { return b.1.to_string(); }
    if let Some(n) = read_params(path).and_then(|p| p["name"].as_str().map(String::from)).filter(|n| !n.is_empty()) { return n; }
    Path::new(path).file_stem().map(|s| s.to_string_lossy().replace('-', " ")).unwrap_or_default()
}
fn user_shaders() -> Vec<String> {
    shader_path::list_imported_shaders(&shader_dir()).into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|p| !is_builtin(p))
        .collect()
}
fn presets_present() -> Vec<String> {
    builtins().into_iter().map(|b| b.0).filter(|p| shader_path::validate_shader_path(p, &roots()).is_ok()).collect()
}
fn fnum(v: &Value, k: &str) -> f64 { v[k].as_f64().unwrap_or(0.0) }
fn fb(v: &Value, k: &str) -> bool { v[k].as_bool().unwrap_or(false) }
fn generate(p: &Value) -> String {
    // Sanitize again so the embedded comment stays a JSON object of known types.
    let sanitized = preset(p.clone());
    let p = &sanitized;
    let f = |x: f64| format!("{x:.4}");
    let on = |b: &str, k: &str| if fb(p, b) { fnum(p, k) } else { 0.0 };
    let rgb = match p["phosphor"].as_str().unwrap_or("none") { "green" => Some((0.2, 1.0, 0.35)), "orange" => Some((1.0, 0.55, 0.12)), "amber" => Some((1.0, 0.75, 0.2)), "white" => Some((0.92, 0.95, 1.0)), _ => None };
    let (r, g, b) = rgb.unwrap_or((1.0, 1.0, 1.0));
    let grille = GRILLES.iter().position(|x| Some(*x) == p["grille_type"].as_str()).unwrap_or(0);
    let filter = FILTERS.iter().position(|x| Some(*x) == p["filter"].as_str()).unwrap_or(0);
    format!(r#"#version 300 es
// shader-desk: {params}
// Generated by Shader Desk. Edit it in Shader Desk to regenerate.
precision highp float;
in vec2 v_texcoord;
layout(location = 0) out vec4 fragColor;
uniform sampler2D tex;
const int FILTER = {filter};
const float CURV = {curv};
const float SCAN = {scan};
const float SPACING = {spacing};
const float MASK = {mask};
const int GRILLE = {grille};
const float BLOOM = {bloom};
const float VIG = {vig};
const float NOISE = {noise};
const float PHOS_ON = {phon};
const vec3 PHOS = vec3({r}, {g}, {b});
float hash(vec2 p) {{ return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453); }}
vec2 warp(vec2 uv) {{ vec2 c = uv * 2.0 - 1.0; c *= 1.0 + CURV * dot(c.yx, c.yx); return c * 0.5 + 0.5; }}
void main() {{
    vec2 uv = warp(v_texcoord);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {{ fragColor = vec4(0.0, 0.0, 0.0, 1.0); return; }}
    vec2 res = vec2(textureSize(tex, 0));
    vec2 px = 1.0 / res;
    vec3 col = texture(tex, uv).rgb;
    if (FILTER == 1) {{
        // NTSC: horizontal chroma bleed, colour fringe, static analog noise
        vec3 a = texture(tex, uv - vec2(2.0 * px.x, 0.0)).rgb;
        vec3 c = texture(tex, uv + vec2(2.0 * px.x, 0.0)).rgb;
        vec3 avg = (a + col + c) / 3.0;
        float y = dot(col, vec3(0.299, 0.587, 0.114));
        col = vec3(y) + (avg - vec3(dot(avg, vec3(0.299, 0.587, 0.114))));
        col.r = mix(col.r, texture(tex, uv + vec2(px.x, 0.0)).r, 0.5);
        col += (hash(floor(gl_FragCoord.xy)) - 0.5) * NOISE;
    }}
    if (BLOOM > 0.0) {{
        col += BLOOM * (texture(tex, uv + vec2(px.x, 0.0)).rgb + texture(tex, uv - vec2(px.x, 0.0)).rgb
                      + texture(tex, uv + vec2(0.0, px.y)).rgb + texture(tex, uv - vec2(0.0, px.y)).rgb);
        col /= 1.0 + 4.0 * BLOOM;
    }}
    if (FILTER == 2) {{
        // Game Boy: 4-shade green palette and a dot-matrix grid
        float l = dot(col, vec3(0.299, 0.587, 0.114));
        vec3 p0 = vec3(0.059, 0.220, 0.059), p1 = vec3(0.188, 0.384, 0.188), p2 = vec3(0.545, 0.675, 0.059), p3 = vec3(0.608, 0.737, 0.059);
        col = l < 0.25 ? p0 : (l < 0.5 ? p1 : (l < 0.75 ? p2 : p3));
        if (MASK > 0.0) {{ vec2 gg = mod(gl_FragCoord.xy, SPACING); if (gg.x < 1.0 || gg.y < 1.0) col *= 1.0 - MASK; }}
    }}
    if (FILTER == 3) {{
        // LCD: smooth, no mask; a touch of softness
        col = mix(col, (texture(tex, uv + vec2(px.x, 0.0)).rgb + texture(tex, uv - vec2(px.x, 0.0)).rgb) * 0.5, 0.15);
    }}
    if (PHOS_ON > 0.5) {{ col = PHOS * dot(col, vec3(0.299, 0.587, 0.114)); }}
    if (SCAN > 0.0) {{ float s = sin(uv.y * res.y * 6.28318 / SPACING); col *= 1.0 - SCAN * (0.5 - 0.5 * s); }}
    if (MASK > 0.0 && FILTER < 2) {{
        vec3 mask = vec3(1.0 - MASK);
        float x = gl_FragCoord.x;
        if (GRILLE == 1) x += mod(floor(gl_FragCoord.y / 2.0), 2.0) * 1.5;
        if (GRILLE == 2) x += mod(floor(gl_FragCoord.y), 2.0) * 1.5;
        float m = mod(x, 3.0);
        if (m < 1.0) mask.r = 1.0; else if (m < 2.0) mask.g = 1.0; else mask.b = 1.0;
        if (GRILLE == 1 && mod(gl_FragCoord.y, 4.0) < 1.0) mask *= 1.0 - MASK;
        col *= mask;
    }}
    if (VIG > 0.0) {{ vec2 v = uv * (1.0 - uv.yx); col *= pow(clamp(v.x * v.y * 18.0, 0.0, 1.0), VIG); }}
    fragColor = vec4(clamp(col, 0.0, 1.0), 1.0);
}}
"#, params = p, filter = filter, curv = f(on("curvature", "curv")), scan = f(on("scanlines", "scan_strength")),
        spacing = f(fnum(p, "scan_spacing").max(1.0)), mask = f(on("grille", "grille_strength")), grille = grille,
        bloom = f(on("bloom", "bloom_amt")), vig = f(on("vignette", "vig")), noise = f(fnum(p, "noise")),
        phon = if rgb.is_some() { "1.0" } else { "0.0" }, r = f(r), g = f(g), b = f(b))
}
fn compile_ok(src: &str) -> Result<(), String> {
    let t = format!("{}/shader-desk-check-{}.frag", std::env::temp_dir().display(), std::process::id());
    fs::write(&t, src).map_err(|e| e.to_string())?;
    let o = Command::new("glslangValidator").args(["-S", "frag", &t]).output().map_err(|e| e.to_string())?;
    let _ = fs::remove_file(&t);
    if o.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stdout).to_string()) }
}
fn themes() -> Vec<String> {
    let mut s = std::collections::BTreeSet::new();
    let add = |s: &mut std::collections::BTreeSet<String>, name: &str| {
        if let Some(t) = shader_path::safe_theme_key(name) { s.insert(t); }
    };
    for d in [format!("{}/.config/omarchy/themes", home()), format!("{}/.local/share/omarchy/themes", home()), "/usr/share/omarchy/themes".into()] {
        if let Ok(r) = fs::read_dir(d) { for e in r.flatten() { if e.path().is_dir() { add(&mut s, &e.file_name().to_string_lossy()); } } }
    }
    if let Ok(o) = Command::new("omarchy-theme-list").output() { for l in String::from_utf8_lossy(&o.stdout).lines() { if !l.trim().is_empty() { add(&mut s, l); } } }
    s.into_iter().collect()
}

// ---------- GUI (text/glyph buttons only: some symbolic icons crash GTK on this machine) ----------
struct State { active: String, pending: Option<String>, timer: Option<glib::SourceId>, rows: Vec<(adw::ActionRow, String)>, populating: bool, show_presets: bool }
struct App { app: adw::Application, win: adw::ApplicationWindow, holder: gtk::Box, st: RefCell<State> }

impl App {
    fn rebuild(self: &Rc<Self>) {
        self.st.borrow_mut().populating = true;
        while let Some(c) = self.holder.first_child() { self.holder.remove(&c); }
        let page = adw::PreferencesPage::new(); page.set_vexpand(true);
        let gv = adw::PreferencesGroup::new();
        let sp = adw::SwitchRow::builder().title("Show presets").subtitle("Orange, Green, NTSC, Game Boy, Aperture Grille, LCD").active(self.st.borrow().show_presets).build();
        let me = self.clone();
        sp.connect_active_notify(move |r| {
            if me.st.borrow().populating { return; }
            me.st.borrow_mut().show_presets = r.is_active();
            let m = me.clone(); glib::idle_add_local_once(move || m.rebuild());
        });
        gv.add(&sp); page.add(&gv);
        let g = adw::PreferencesGroup::builder().title("Shaders").description("Click to apply to the whole desktop").build();
        let mut items: Vec<String> = vec![String::new()];
        if self.st.borrow().show_presets { items.extend(presets_present()); }
        items.extend(user_shaders());
        let mut rows = vec![];
        for path in &items {
            let title = if path.is_empty() { "Off".to_string() } else { friendly(path) };
            let sub = if path.is_empty() { "No shader".to_string() } else if is_builtin(path) { format!("{} · preset", fname(path)) } else { fname(path) };
            let r = adw::ActionRow::builder().title(title.as_str()).subtitle(sub.as_str()).activatable(true).build();
            if !path.is_empty() {
                let e = gtk::Button::builder().label("󰏫 Edit").valign(gtk::Align::Center).build(); e.add_css_class("flat");
                let (me, p) = (self.clone(), path.clone()); e.connect_clicked(move |_| me.open_editor(Some(p.clone())));
                r.add_suffix(&e);
                if !is_builtin(path) {
                    let d = gtk::Button::builder().label("󰆴").tooltip_text("Delete").valign(gtk::Align::Center).build(); d.add_css_class("flat");
                    let (me, p) = (self.clone(), path.clone()); d.connect_clicked(move |_| me.ask_delete(p.clone()));
                    r.add_suffix(&d);
                }
            }
            let (me, p) = (self.clone(), path.clone()); r.connect_activated(move |_| me.pick(p.clone()));
            g.add(&r); rows.push((r, path.clone()));
        }
        page.add(&g);
        let tg = adw::PreferencesGroup::builder().title("Themes").description("Shader applied when switching to each theme").build();
        let mut values: Vec<String> = vec![DEFAULT.into(), String::new()];
        values.extend(presets_present()); values.extend(user_shaders());
        let labels: Vec<String> = values.iter().map(|v| if v == DEFAULT { "Theme default".into() } else if v.is_empty() { "Off".into() } else { friendly(v) }).collect();
        let lref: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
        let conf = load_conf();
        for t in themes() {
            let r = adw::ComboRow::builder().title(t.replace('-', " ").as_str()).model(&gtk::StringList::new(&lref)).build();
            let cur = conf.get(&t).cloned().unwrap_or(DEFAULT.into());
            r.set_selected(values.iter().position(|v| *v == cur).unwrap_or(0) as u32);
            let (me, vals, th) = (self.clone(), values.clone(), t.clone());
            r.connect_selected_notify(move |r| {
                if me.st.borrow().populating { return; } // only writes the conf; never applies or runs the hook
                let mut m = load_conf(); let v = &vals[r.selected() as usize];
                if v == DEFAULT { m.remove(&th); } else { m.insert(th.clone(), v.clone()); }
                save_conf(&m);
            });
            tg.add(&r);
        }
        page.add(&tg);
        self.holder.append(&page);
        { let mut s = self.st.borrow_mut(); s.rows = rows; s.populating = false; }
        self.refresh(None);
    }
    fn refresh(&self, cur: Option<String>) {
        let mut s = self.st.borrow_mut();
        s.active = cur.unwrap_or_else(current);
        for (r, p) in &s.rows { if *p == s.active { r.add_css_class("active") } else { r.remove_css_class("active") } }
    }
    fn pick(self: &Rc<Self>, path: String) {
        let mut s = self.st.borrow_mut();
        if s.populating || path == s.active { return; }
        s.pending = Some(path);
        if let Some(t) = s.timer.take() { t.remove(); }
        let me = self.clone();
        s.timer = Some(glib::timeout_add_local_once(Duration::from_millis(250), move || {
            let p = { let mut s = me.st.borrow_mut(); s.timer = None; s.pending.take() };
            if let Some(p) = p { if p != me.st.borrow().active { let c = apply(&p); me.refresh(Some(c)); } }
        }));
    }
    fn ask_delete(self: &Rc<Self>, path: String) {
        if is_builtin(&path) { return; }
        let d = adw::AlertDialog::new(Some(&format!("Delete “{}”?", friendly(&path))), Some(&format!("{} will be removed. Themes using it fall back to Off.", fname(&path))));
        d.add_response("cancel", "Cancel"); d.add_response("delete", "Delete");
        d.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        let me = self.clone();
        d.connect_response(None, move |_, r| { if r == "delete" {
            if me.st.borrow().active == path { let c = apply(""); me.refresh(Some(c)); }
            let mut m = load_conf(); let mut ch = false;
            for v in m.values_mut() { if *v == path { v.clear(); ch = true; } }
            if ch { save_conf(&m); }
            let _ = fs::remove_file(&path);
            me.rebuild();
        }});
        d.present(Some(&self.win));
    }
    fn open_editor(self: &Rc<Self>, base: Option<String>) {
        let mut p = defaults();
        let mut target: Option<String> = None; // only user shaders are overwritten; presets always save a copy
        if let Some(b) = &base {
            if let Some(bi) = builtins().into_iter().find(|x| x.0 == *b) { p = bi.2.clone(); p["name"] = json!(format!("{} custom", bi.1)); }
            else {
                let src = fs::read_to_string(b).unwrap_or_default();
                let (params, overwrite) = user_shader_edit(&src, &friendly(b));
                p = params;
                if overwrite { target = Some(b.clone()); }
            }
        }
        let w = adw::Window::builder().application(&self.app).title("Shader Desk · Editor").default_width(440).default_height(720).build();
        let tv = adw::ToolbarView::new(); let hb = adw::HeaderBar::new();
        let sub = base.as_ref().map(|b| format!("Edit {}", friendly(b))).unwrap_or("New shader".into());
        hb.set_title_widget(Some(&adw::WindowTitle::new("Shader Editor", &sub)));
        tv.add_top_bar(&hb);
        let page = adw::PreferencesPage::new(); page.set_vexpand(true);
        let g0 = adw::PreferencesGroup::new();
        let nr = adw::ActionRow::builder().title("Name").build();
        let name = gtk::Entry::builder().hexpand(true).valign(gtk::Align::Center).placeholder_text("My CRT").text(p["name"].as_str().unwrap_or("")).build();
        nr.add_suffix(&name); g0.add(&nr);
        let combo = |t: &str, opts: &[&str], cur: &str| { let r = adw::ComboRow::builder().title(t).model(&gtk::StringList::new(opts)).build(); r.set_selected(opts.iter().position(|o| *o == cur).unwrap_or(0) as u32); r };
        let filt = combo("Filter type", &FILTERS, p["filter"].as_str().unwrap_or("CRT")); g0.add(&filt);
        page.add(&g0);
        let ge = adw::PreferencesGroup::builder().title("Effects").build();
        let mut sw = vec![];
        for (k, t) in [("curvature", "Curvature"), ("scanlines", "Scanlines"), ("grille", "RGB grille / dot grid"), ("bloom", "Bloom"), ("vignette", "Vignette")] {
            let r = adw::SwitchRow::builder().title(t).active(fb(&p, k)).build(); ge.add(&r); sw.push((k, r));
        }
        let gt = combo("Grille type", &GRILLES, p["grille_type"].as_str().unwrap_or("aperture")); ge.add(&gt);
        let ph = combo("Phosphor colour", &PHOS, p["phosphor"].as_str().unwrap_or("none")); ge.add(&ph);
        page.add(&ge);
        let gs = adw::PreferencesGroup::builder().title("Strengths").build();
        let mut sl = vec![];
        for (k, t, lo, hi, st) in [("scan_strength", "Scanline strength", 0.0, 1.0, 0.01), ("scan_spacing", "Scanline spacing (px)", 1.0, 8.0, 0.5),
            ("curv", "Curvature", 0.0, 0.15, 0.005), ("bloom_amt", "Bloom", 0.0, 0.3, 0.01), ("vig", "Vignette", 0.0, 1.0, 0.01),
            ("grille_strength", "Grille strength", 0.0, 0.5, 0.01), ("noise", "Noise (NTSC)", 0.0, 0.3, 0.01)] {
            let row = adw::ActionRow::builder().title(t).build();
            let s = gtk::Scale::with_range(gtk::Orientation::Horizontal, lo, hi, st);
            s.set_value(fnum(&p, k)); s.set_hexpand(true); s.set_size_request(190, -1); s.set_draw_value(true); s.set_digits(if hi <= 1.0 { 3 } else { 2 });
            row.add_suffix(&s); gs.add(&row); sl.push((k, s));
        }
        page.add(&gs);
        let msg = gtk::Label::builder().wrap(true).xalign(0.0).margin_start(12).margin_end(12).build(); msg.add_css_class("dim-label");
        let bb = gtk::Box::builder().spacing(8).margin_top(8).margin_bottom(12).margin_end(12).halign(gtk::Align::End).build();
        let pv = gtk::Button::with_label("Preview"); let sv = gtk::Button::with_label("Save"); sv.add_css_class("suggested-action");
        bb.append(&pv); bb.append(&sv);
        let bx = gtk::Box::new(gtk::Orientation::Vertical, 0); bx.append(&page); bx.append(&msg); bx.append(&bb);
        tv.set_content(Some(&bx)); w.set_content(Some(&tv));
        let collect: Rc<dyn Fn() -> Value> = Rc::new(move || {
            let mut v = defaults();
            v["name"] = json!(name.text().trim());
            v["filter"] = json!(FILTERS[filt.selected() as usize]);
            for (k, r) in &sw { v[*k] = json!(r.is_active()); }
            v["grille_type"] = json!(GRILLES[gt.selected() as usize]); v["phosphor"] = json!(PHOS[ph.selected() as usize]);
            for (k, s) in &sl { v[*k] = json!((s.value() * 10000.0).round() / 10000.0); }
            v
        });
        let (c, m, me) = (collect.clone(), msg.clone(), self.clone());
        pv.connect_clicked(move |_| {
            let src = generate(&c());
            if let Err(e) = compile_ok(&src) { m.set_text(&format!("Compile error:\n{}", e.chars().take(400).collect::<String>())); return; }
            let dir = format!("{}/.cache/shader-desk", home()); let _ = fs::create_dir_all(&dir);
            let a = format!("{dir}/preview.glsl"); let b = format!("{dir}/preview-b.glsl");
            let path = if current() == a { b } else { a }; // alternate names so Hyprland reloads the new source
            let _ = fs::write(&path, src); let cur = apply(&path); me.refresh(Some(cur));
            m.set_text("Previewing (not saved).");
        });
        let (c, m, me) = (collect, msg, self.clone());
        sv.connect_clicked(move |_| {
            let p = c(); let n = p["name"].as_str().unwrap_or("").to_string();
            if n.is_empty() { m.set_text("Give it a name first."); return; }
            let safe_name = format!("{}.glsl", slugify(&n));
            if shader_path::accept_shader_import(&safe_name).is_err() { m.set_text("Name must contain letters or numbers."); return; }
            let src = generate(&p);
            if let Err(e) = compile_ok(&src) { m.set_text(&format!("Compile error, not saved:\n{}", e.chars().take(400).collect::<String>())); return; }
            let path = target.clone().unwrap_or_else(|| {
                let base = format!("{}/{}", sd(), slugify(&n));
                let mut cand = format!("{base}.glsl"); let mut i = 2;
                while Path::new(&cand).exists() || is_builtin(&cand) { cand = format!("{base}-{i}.glsl"); i += 1; }
                cand
            });
            if fs::write(&path, &src).is_err() { m.set_text("Could not write the file."); return; }
            m.set_text(&format!("Saved {} (compiles cleanly).", fname(&path)));
            if me.st.borrow().active == path { apply(&path); }
            me.rebuild();
        });
        w.present();
    }
}

fn gui(editor: bool) {
    let app = adw::Application::builder().application_id("emrgim.ShaderDesk").build();
    app.connect_activate(move |app| {
        if let Some(w) = app.active_window() { w.present(); return; }
        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
        let css = format!("{}/.local/state/omarchy/current/theme/shader-desk.css", home());
        if Path::new(&css).exists() {
            let p = gtk::CssProvider::new(); p.load_from_path(&css);
            gtk::style_context_add_provider_for_display(&gdk::Display::default().unwrap(), &p, gtk::STYLE_PROVIDER_PRIORITY_USER);
        }
        let win = adw::ApplicationWindow::builder().application(app).title("Shader Desk").default_width(460).default_height(640).build();
        let tv = adw::ToolbarView::new(); let hb = adw::HeaderBar::new();
        hb.set_title_widget(Some(&adw::WindowTitle::new("Shader Desk", "Hyprland screen shader")));
        let add = gtk::Button::builder().label("+").tooltip_text("New shader").build(); hb.pack_start(&add); tv.add_top_bar(&hb);
        let holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
        tv.set_content(Some(&holder)); win.set_content(Some(&tv));
        let st = State { active: String::new(), pending: None, timer: None, rows: vec![], populating: true, show_presets: true };
        let me = Rc::new(App { app: app.clone(), win: win.clone(), holder, st: RefCell::new(st) });
        let m = me.clone(); add.connect_clicked(move |_| m.open_editor(None));
        me.rebuild(); win.present();
        if editor { me.open_editor(None); }
    });
    app.run_with_args::<&str>(&[]);
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a.get(1).map(String::as_str) {
        Some("hook") => hook(&a.get(2).cloned().unwrap_or_else(|| fs::read_to_string(format!("{}/.local/state/omarchy/current/theme.name", home())).unwrap_or_default())),
        Some("apply") => {
            let p = a.get(2).map(String::as_str).unwrap_or("off");
            match try_apply(if p == "off" { "" } else { p }) {
                Ok(c) => println!("{c}"),
                Err(e) => { eprintln!("shader-desk: {e}"); std::process::exit(1); }
            }
        }
        Some("current") => println!("{}", current()),
        Some("gen-presets") => {
            for (path, _, prm) in builtins() {
                if Path::new(&path).exists() || path.ends_with("aperture-grille.glsl") || path.contains("phosphor") { continue; }
                let src = generate(&prm);
                match compile_ok(&src) { Ok(_) => { let _ = fs::write(&path, src); println!("wrote {path}"); } Err(e) => eprintln!("{path}: {e}") }
            }
        }
        Some("--editor") => gui(true),
        _ => gui(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn generated_meta(src: &str) -> Value {
        let line = src.lines().nth(1).expect("shader meta line");
        let json_text = line.strip_prefix(META_PREFIX).expect("shader meta prefix");
        serde_json::from_str(json_text).expect("shader meta json")
    }

    /// Previous merge for a well-typed object: copy every field onto the defaults.
    fn legacy_merge(over: Value) -> Value {
        let mut d = defaults();
        for (k, v) in over.as_object().expect("legacy oracle") {
            d[k.as_str()] = v.clone();
        }
        d
    }

    fn assert_no_panic<T: std::panic::UnwindSafe>(label: &str, f: impl FnOnce() -> T + std::panic::UnwindSafe) -> T {
        match catch_unwind(f) {
            Ok(value) => value,
            Err(_) => panic!("{label} panicked"),
        }
    }

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let n = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
            let p = std::env::temp_dir().join(format!("shader-desk-meta-{n}-{}", std::process::id()));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    #[test]
    fn non_object_metadata_falls_back_to_defaults() {
        let cases = [
            ("array", "[]"),
            ("nested array", "[1, {\"a\": 1}]"),
            ("null", "null"),
            ("true", "true"),
            ("false", "false"),
            ("zero", "0"),
            ("int", "1"),
            ("negative", "-3"),
            ("float", "1.5"),
            ("string", "\"nope\""),
            ("whitespace array", "  []"),
        ];
        for (label, body) in cases {
            let src = format!("// shader-desk: {body}\nvoid main() {{}}\n");
            let parsed: Value = serde_json::from_str(body).unwrap();
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let direct = preset(parsed);
                let (params, overwrite) = user_shader_edit(&src, "evil");
                let generated = generate(&params);
                (direct, params, overwrite, generated)
            }));
            let (direct, params, overwrite, generated) = outcome.unwrap_or_else(|_| panic!("{label} panicked"));
            assert_eq!(direct, defaults(), "{label}");
            assert!(!overwrite, "{label}");
            let mut expect = defaults();
            expect["name"] = json!("evil custom");
            assert_eq!(params, expect, "{label}");
            assert_eq!(generated_meta(&generated), expect, "{label}");
            assert!(!generated.contains("// shader-desk: []"), "{label}");
        }
    }

    #[test]
    fn malformed_metadata_falls_back_to_defaults() {
        let bodies = ["", "{", "{]", "not-json", "{\"a\":}", "{\"name\":\"ok\"} trailing", "[] []"];
        for body in bodies {
            let src = format!("#version 300 es\n// shader-desk: {body}\n");
            let (params, overwrite) = assert_no_panic(body, || user_shader_edit(&src, "broken"));
            assert!(!overwrite, "{body}");
            assert_eq!(params["name"], json!("broken custom"), "{body}");
            assert_eq!(params["filter"], defaults()["filter"], "{body}");
            assert_eq!(params["curvature"], defaults()["curvature"], "{body}");
            assert!(generated_meta(&generate(&params)).is_object(), "{body}");
        }
    }

    #[test]
    fn unexpected_field_types_keep_defaults_and_valid_fields() {
        let input = json!({
            "name": "Keep",
            "filter": ["LCD"],
            "curvature": "yes",
            "scanlines": 1,
            "grille": false,
            "bloom": null,
            "vignette": false,
            "grille_type": {"t": "slot mask"},
            "phosphor": "green",
            "scan_strength": "0.9",
            "scan_spacing": true,
            "curv": 0.1,
            "bloom_amt": [],
            "vig": null,
            "grille_strength": 0,
            "noise": {},
            "extra": [1, 2, 3],
            "note": "hello"
        });
        let got = assert_no_panic("unexpected types", || preset(input));
        let mut expect = defaults();
        expect["name"] = json!("Keep");
        expect["grille"] = json!(false);
        expect["vignette"] = json!(false);
        expect["phosphor"] = json!("green");
        expect["curv"] = json!(0.1);
        expect["grille_strength"] = json!(0);
        assert_eq!(got, expect);
        let src = generate(&got);
        let meta = generated_meta(&src);
        assert_eq!(meta, expect);
        assert!(src.contains("const int FILTER = 0;"));
        assert!(src.contains("const float CURV = 0.1000;"));
        assert!(src.contains("const float SCAN = 0.3500;"));
        assert!(src.contains("const float MASK = 0.0000;"));
        assert!(src.contains("const float PHOS_ON = 1.0;"));
        assert!(src.contains("const vec3 PHOS = vec3(0.2000, 1.0000, 0.3500);"));
    }

    #[test]
    fn generate_of_non_object_embeds_defaults() {
        for value in [json!([]), json!(null), json!("x"), json!(1), json!(true), json!(false)] {
            let src = assert_no_panic("generate", AssertUnwindSafe(|| generate(&value)));
            assert_eq!(generated_meta(&src), defaults());
            assert!(src.starts_with("#version 300 es\n"));
        }
    }

    #[test]
    fn valid_presets_merge_and_generate_as_before() {
        let samples = [
            json!({"phosphor":"orange","curvature":false,"grille":false,"bloom_amt":0.05}),
            json!({"phosphor":"green","curvature":false,"grille":false,"bloom_amt":0.05}),
            json!({"name":"NTSC","filter":"NTSC","curv":0.02,"scan_strength":0.25,"grille":false,"bloom_amt":0.08,"vig":0.2,"noise":0.06}),
            json!({"name":"Game Boy","filter":"Game Boy","curvature":false,"scanlines":false,"grille_strength":0.25,"scan_spacing":3.0,"bloom":false,"vig":0.1}),
            json!({"scan_strength":0.55,"scan_spacing":2.0,"curv":0.035,"grille_strength":0.10,"bloom_amt":0.06}),
            json!({"name":"LCD","filter":"LCD","curvature":false,"scanlines":false,"grille":false,"vignette":false,"bloom_amt":0.03}),
            json!({"name":"","curvature":false,"scan_strength":0,"grille_strength":0}),
        ];
        for sample in samples {
            let merged = preset(sample.clone());
            assert_eq!(merged, legacy_merge(sample));
            assert_eq!(preset(merged.clone()), merged);
            let src = generate(&merged);
            assert_eq!(generated_meta(&src), merged);
            let again = user_shader_edit(&src, "ignored");
            assert!(again.1);
            assert_eq!(again.0, merged);
        }

        let ntsc = preset(json!({"name":"NTSC","filter":"NTSC","curv":0.02,"scan_strength":0.25,"grille":false,"bloom_amt":0.08,"vig":0.2,"noise":0.06}));
        let src = generate(&ntsc);
        assert!(src.contains("const int FILTER = 1;"));
        assert!(src.contains("const float CURV = 0.0200;"));
        assert!(src.contains("const float SCAN = 0.2500;"));
        assert!(src.contains("const float SPACING = 3.0000;"));
        assert!(src.contains("const float MASK = 0.0000;"));
        assert!(src.contains("const int GRILLE = 0;"));
        assert!(src.contains("const float BLOOM = 0.0800;"));
        assert!(src.contains("const float VIG = 0.2000;"));
        assert!(src.contains("const float NOISE = 0.0600;"));
        assert!(src.contains("const float PHOS_ON = 0.0;"));
        assert!(src.contains("const vec3 PHOS = vec3(1.0000, 1.0000, 1.0000);"));

        let orange = preset(json!({"phosphor":"orange","curvature":false,"grille":false,"bloom_amt":0.05}));
        let src = generate(&orange);
        assert!(src.contains("const int FILTER = 0;"));
        assert!(src.contains("const float CURV = 0.0000;"));
        assert!(src.contains("const float BLOOM = 0.0500;"));
        assert!(src.contains("const float PHOS_ON = 1.0;"));
        assert!(src.contains("const vec3 PHOS = vec3(1.0000, 0.5500, 0.1200);"));

        let gb = preset(json!({"name":"Game Boy","filter":"Game Boy","curvature":false,"scanlines":false,"grille_strength":0.25,"scan_spacing":3.0,"bloom":false,"vig":0.1}));
        let src = generate(&gb);
        assert!(src.contains("const int FILTER = 2;"));
        assert!(src.contains("const float SCAN = 0.0000;"));
        assert!(src.contains("const float MASK = 0.2500;"));
        assert!(src.contains("const float BLOOM = 0.0000;"));
        assert!(src.contains("const float VIG = 0.1000;"));
    }

    #[test]
    fn metadata_outside_the_first_five_lines_is_not_a_preset() {
        let late = "\n\n\n\n\n// shader-desk: []\n";
        let (params, overwrite) = assert_no_panic("line 6", || user_shader_edit(late, "late"));
        assert!(!overwrite);
        assert_eq!(params["name"], json!("late custom"));

        let fifth = "\n\n\n\n// shader-desk: []\n";
        let (params, overwrite) = user_shader_edit(fifth, "fifth");
        assert!(!overwrite);
        assert_eq!(params["filter"], defaults()["filter"]);
        assert_eq!(params["name"], json!("fifth custom"));
    }

    #[test]
    fn glsl_file_with_array_metadata_is_ignored_on_edit() {
        let dir = Scratch::new();
        let path = dir.0.join("bad-array.glsl");
        fs::write(&path, b"#version 300 es\n// shader-desk: []\nvoid main() {}\n").unwrap();
        let text = path.to_str().unwrap().to_string();
        let src = fs::read_to_string(&path).unwrap();
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let from_file = read_params(&text);
            let label = friendly(&text);
            let (params, overwrite) = user_shader_edit(&src, &label);
            let generated = generate(&params);
            (from_file, label, params, overwrite, generated)
        }));
        let (from_file, label, params, overwrite, generated) = outcome.expect("edit of // shader-desk: [] panicked");
        assert!(from_file.is_none());
        assert_eq!(label, "bad array");
        assert!(!overwrite);
        assert_eq!(params["name"], json!("bad array custom"));
        assert_eq!(params["curvature"], defaults()["curvature"]);
        assert_eq!(params["scan_strength"], defaults()["scan_strength"]);
        let meta = generated_meta(&generated);
        assert!(meta.is_object());
        assert_eq!(meta["name"], json!("bad array custom"));
        assert!(!generated.contains("// shader-desk: []"));

        let utf8 = dir.0.join("bad-utf8.glsl");
        fs::write(&utf8, b"// shader-desk: []\xff\n").unwrap();
        let utf8_text = utf8.to_str().unwrap().to_string();
        assert!(catch_unwind(AssertUnwindSafe(|| read_params(&utf8_text))).expect("invalid utf-8 panicked").is_none());
        let src = fs::read_to_string(&utf8).unwrap_or_default();
        let (params, overwrite) = user_shader_edit(&src, "broken");
        assert!(!overwrite);
        assert_eq!(params["name"], json!("broken custom"));

        let ok = dir.0.join("amber-tube.glsl");
        fs::write(&ok, b"// shader-desk: {\"name\":\"Amber Tube\",\"phosphor\":\"amber\",\"curvature\":false}\n").unwrap();
        let ok_text = ok.to_str().unwrap();
        let loaded = read_params(ok_text).expect("valid object");
        assert!(loaded.is_object());
        assert_eq!(friendly(ok_text), "Amber Tube");
        let src = fs::read_to_string(&ok).unwrap();
        let (params, overwrite) = user_shader_edit(&src, "Amber Tube");
        assert!(overwrite);
        assert_eq!(params["name"], json!("Amber Tube"));
        assert_eq!(params["phosphor"], json!("amber"));
        assert_eq!(params["curvature"], json!(false));
        assert_eq!(params["scanlines"], defaults()["scanlines"]);
        assert_eq!(params["bloom_amt"], defaults()["bloom_amt"]);
    }
}

