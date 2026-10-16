//! Shader Desk: pick, create and edit Hyprland screen shaders (Lua config, via `hyprctl eval`).
//! `shader-desk` (GUI) | `--editor` | `hook <theme>` | `apply <path|off>` | `current` | `gen-presets`
use adw::prelude::*;
use gtk::{gdk, glib};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::BTreeMap, fs, path::Path, process::Command, rc::Rc, time::Duration};

fn home() -> String { std::env::var("HOME").unwrap_or_else(|_| "/home/emiliano".into()) }
fn sd() -> String { format!("{}/.config/hypr/shaders", home()) }
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
fn preset(over: Value) -> Value { let mut d = defaults(); for (k, v) in over.as_object().unwrap() { d[k] = v.clone(); } d }
/// Built-in presets: (file, name, params used when Edit makes a copy). Protected from Delete.
fn builtins() -> Vec<(String, &'static str, Value)> {
    let d = sd();
    vec![
        (format!("{d}/phosphor-orange.glsl"), "Phosphor Orange", preset(json!({"phosphor":"orange","curvature":false,"grille":false,"bloom_amt":0.05}))),
        (format!("{d}/phosphor.glsl"), "Phosphor Green", preset(json!({"phosphor":"green","curvature":false,"grille":false,"bloom_amt":0.05}))),
        (format!("{d}/ntsc.glsl"), "NTSC", preset(json!({"name":"NTSC","filter":"NTSC","curv":0.02,"scan_strength":0.25,"grille":false,"bloom_amt":0.08,"vig":0.2,"noise":0.06}))),
        (format!("{d}/game-boy.glsl"), "Game Boy", preset(json!({"name":"Game Boy","filter":"Game Boy","curvature":false,"scanlines":false,"grille_strength":0.25,"scan_spacing":3.0,"bloom":false,"vig":0.1}))),
        (format!("{d}/aperture-grille.glsl"), "Aperture Grille", preset(json!({"scan_strength":0.55,"scan_spacing":2.0,"curv":0.035,"grille_strength":0.10,"bloom_amt":0.06}))),
        (format!("{d}/lcd.glsl"), "LCD", preset(json!({"name":"LCD","filter":"LCD","curvature":false,"scanlines":false,"grille":false,"vignette":false,"bloom_amt":0.03}))),
    ]
}
fn is_builtin(p: &str) -> bool { builtins().iter().any(|b| b.0 == p) }

// ---------- hyprland ----------
fn current() -> String {
    let out = Command::new("hyprctl").args(["getoption", "decoration:screen_shader"]).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    let v = out.lines().find_map(|l| l.strip_prefix("str:")).map(|s| s.trim().to_string()).unwrap_or_default();
    if v == "[[EMPTY]]" { String::new() } else { v }
}
fn eval_shader(path: &str) {
    let _ = Command::new("hyprctl").args(["eval", &format!("hl.config({{ decoration = {{ screen_shader = \"{path}\" }} }})")]).output();
}
/// One eval per pick. Off sets the sticky manual-off flag; any shader clears it.
fn apply(path: &str) -> String {
    let _ = fs::create_dir_all(format!("{}/.config/shader-desk", home()));
    if path.is_empty() { let _ = fs::write(flag_path(), ""); } else { let _ = fs::remove_file(flag_path()); }
    eval_shader(path);
    current()
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
    for (k, v) in m { s += &format!("{k}={v}\n"); }
    let _ = fs::write(conf_path(), s);
}
fn slug(s: &str) -> String { s.trim().to_lowercase().replace(' ', "-") }
fn slugify(s: &str) -> String { regex::Regex::new("[^a-z0-9]+").unwrap().replace_all(&s.to_lowercase(), "-").trim_matches('-').to_string() }

/// Theme-set hook: manual-off wins, then theme-shaders.conf, then the theme's own shader, else Off.
fn hook(theme: &str) {
    let theme = slug(theme);
    if Path::new(&flag_path()).exists() { eval_shader(""); return; }
    if !theme.is_empty() {
        if let Some(v) = load_conf().get(&theme) {
            if v.is_empty() || Path::new(v).exists() { eval_shader(v); return; }
        }
    }
    let f = if theme == "phosphor-green" { format!("{}/phosphor.glsl", sd()) } else { format!("{}/{theme}.glsl", sd()) };
    if !theme.is_empty() && Path::new(&f).exists() { eval_shader(&f) } else { eval_shader("") }
}

// ---------- shaders ----------
fn read_params(path: &str) -> Option<Value> {
    let s = fs::read_to_string(path).ok()?;
    let line = s.lines().take(5).find_map(|l| l.strip_prefix("// shader-desk: "))?;
    serde_json::from_str(line).ok()
}
fn fname(p: &str) -> String { Path::new(p).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default() }
fn friendly(path: &str) -> String {
    if let Some(b) = builtins().into_iter().find(|b| b.0 == path) { return b.1.to_string(); }
    if let Some(n) = read_params(path).and_then(|p| p["name"].as_str().map(String::from)).filter(|n| !n.is_empty()) { return n; }
    Path::new(path).file_stem().map(|s| s.to_string_lossy().replace('-', " ")).unwrap_or_default()
}
fn user_shaders() -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(sd()).map(|r| r.filter_map(|e| e.ok()).map(|e| e.path().to_string_lossy().to_string())
        .filter(|p| p.ends_with(".glsl") && !is_builtin(p)).collect()).unwrap_or_default();
    v.sort(); v
}
fn presets_present() -> Vec<String> { builtins().into_iter().map(|b| b.0).filter(|p| Path::new(p).exists()).collect() }
fn fnum(v: &Value, k: &str) -> f64 { v[k].as_f64().unwrap_or(0.0) }
fn fb(v: &Value, k: &str) -> bool { v[k].as_bool().unwrap_or(false) }
fn generate(p: &Value) -> String {
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
    for d in [format!("{}/.config/omarchy/themes", home()), format!("{}/.local/share/omarchy/themes", home()), "/usr/share/omarchy/themes".into()] {
        if let Ok(r) = fs::read_dir(d) { for e in r.flatten() { if e.path().is_dir() { s.insert(slug(&e.file_name().to_string_lossy())); } } }
    }
    if let Ok(o) = Command::new("omarchy-theme-list").output() { for l in String::from_utf8_lossy(&o.stdout).lines() { if !l.trim().is_empty() { s.insert(slug(l)); } } }
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
            else if let Some(prm) = read_params(b) { p = preset(prm); target = Some(b.clone()); }
            else { p["name"] = json!(format!("{} custom", friendly(b))); }
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
            let path = if current() == a { b } else { a }; // alternate names so one eval loads the new source
            let _ = fs::write(&path, src); let cur = apply(&path); me.refresh(Some(cur));
            m.set_text("Previewing (not saved).");
        });
        let (c, m, me) = (collect, msg, self.clone());
        sv.connect_clicked(move |_| {
            let p = c(); let n = p["name"].as_str().unwrap_or("").to_string();
            if n.is_empty() { m.set_text("Give it a name first."); return; }
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
            if me.st.borrow().active == path { eval_shader(&path); }
            me.rebuild();
        });
        w.present();
    }
}

fn gui(editor: bool) {
    let app = adw::Application::builder().application_id("dev.emiliano.ShaderDesk").build();
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
        Some("apply") => { let p = a.get(2).map(String::as_str).unwrap_or("off"); println!("{}", apply(if p == "off" { "" } else { p })); }
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
