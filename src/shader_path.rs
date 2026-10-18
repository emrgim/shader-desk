//! Shader paths that reach Hyprland.
//!
//! A screen-shader path is a filesystem path, never a snippet of Lua or shell.
//! Callers pass it to `hyprctl` as an argv element. On a legacy config that is
//! `hyprctl keyword decoration:screen_shader <path>`. Omarchy Quattro's Lua
//! config rejects `keyword` (`keyword can't work with non-legacy parsers`),
//! so that case uses `hyprctl eval` only after the path has been canonicalized
//! into an allowed directory and the filename has passed the safe charset.
//! The eval payload encodes that validated path as a Lua literal; it does not
//! interpolate the original string.

use std::fs;
use std::path::{Component, Path, PathBuf};

/// Filename charset from the review: `[A-Za-z0-9._-]+\.glsl`.
pub fn safe_shader_filename(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".glsl") else { return false };
    !stem.is_empty()
        && stem.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderPathError {
    UnsafeName,
    Outside,
    NotFound,
    Control,
}

impl std::fmt::Display for ShaderPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::UnsafeName => "filename is not a safe .glsl name",
            Self::Outside => "path is outside the shader directories",
            Self::NotFound => "shader file was not found",
            Self::Control => "path contains control characters",
        })
    }
}

/// Refuse an imported shader whose filename is outside the safe charset.
/// Shader Desk does not rename the file; the name is not offered and not applied.
pub fn accept_shader_import(file_name: &str) -> Result<(), ShaderPathError> {
    if file_name.is_empty()
        || file_name.len() > 255
        || file_name.contains('/')
        || file_name.contains('\\')
        || file_name.contains('\0')
        || file_name_has_control(file_name)
        || !safe_shader_filename(file_name)
    {
        return Err(ShaderPathError::UnsafeName);
    }
    Ok(())
}

fn file_name_has_control(name: &str) -> bool {
    name.bytes().any(|b| b < 0x20 || b == 0x7f)
}

fn path_has_control(path: &str) -> bool {
    path.bytes().any(|b| b < 0x20 || b == 0x7f)
}

/// Canonicalize `path` and require a direct child of one of `roots`.
/// An empty path is Off and is allowed.
pub fn validate_shader_path(path: &str, roots: &[PathBuf]) -> Result<PathBuf, ShaderPathError> {
    if path.is_empty() {
        return Ok(PathBuf::new());
    }
    if path_has_control(path) {
        return Err(ShaderPathError::Control);
    }
    let raw = Path::new(path);
    if raw.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(ShaderPathError::Outside);
    }
    let name = raw.file_name().and_then(|s| s.to_str()).ok_or(ShaderPathError::UnsafeName)?;
    accept_shader_import(name)?;
    let canon = fs::canonicalize(raw).map_err(|_| ShaderPathError::NotFound)?;
    if !canon.is_file() {
        return Err(ShaderPathError::NotFound);
    }
    let canon_str = canon.to_str().ok_or(ShaderPathError::UnsafeName)?;
    if path_has_control(canon_str) {
        return Err(ShaderPathError::Control);
    }
    let canon_name = canon.file_name().and_then(|s| s.to_str()).ok_or(ShaderPathError::UnsafeName)?;
    accept_shader_import(canon_name)?;
    let parent = canon.parent().ok_or(ShaderPathError::Outside)?;
    for root in roots {
        let Ok(root_canon) = fs::canonicalize(root) else { continue };
        if parent == root_canon {
            return Ok(canon);
        }
    }
    Err(ShaderPathError::Outside)
}

/// Shaders already on disk. Unsafe filenames and paths that leave `dir` are omitted.
pub fn list_imported_shaders(dir: &Path) -> Vec<PathBuf> {
    let Ok(root) = fs::canonicalize(dir) else { return Vec::new() };
    let Ok(entries) = fs::read_dir(&root) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else { continue };
        if accept_shader_import(name).is_err() {
            continue;
        }
        let Some(text) = path.to_str() else { continue };
        if let Ok(canon) = validate_shader_path(text, &[root.clone()]) {
            out.push(canon);
        }
    }
    out.sort();
    out
}

/// Theme keys become a single path component (`{shaders}/{theme}.glsl`).
/// `../../tmp` and anything outside `[a-z0-9][a-z0-9._-]*` is refused.
pub fn safe_theme_key(raw: &str) -> Option<String> {
    let s = raw.trim().to_lowercase().replace(' ', "-");
    let bytes = s.as_bytes();
    if bytes.is_empty() || !(bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit()) {
        return None;
    }
    if bytes.iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')) {
        Some(s)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HyprConfigMode {
    /// `hyprctl keyword` applies the path as a config value.
    Legacy,
    /// Lua config. `keyword` is rejected, so a validated path is sent with `eval`.
    Lua,
}

/// Map a no-value `hyprctl keyword decoration:screen_shader` probe to a mode.
/// That probe returns before a value is parsed, so it does not change the shader.
pub fn mode_from_probe(text: &str) -> HyprConfigMode {
    if text.contains("non-legacy") || text.contains("Use eval") {
        HyprConfigMode::Lua
    } else {
        HyprConfigMode::Legacy
    }
}

/// Encode `s` as a Lua literal.
///
/// Long brackets keep quotes, backslashes, and embedded newlines as data.
/// The `=` run is long enough that a `]]` / `]=]` / `]==]` sequence inside `s`
/// cannot close the literal. A leading newline would be stripped by Lua, so
/// that one case uses a quoted string with escapes instead.
pub fn lua_encode(s: &str) -> String {
    if s.starts_with('\n') { lua_quoted(s) } else { lua_long_bracket(s) }
}

pub fn lua_long_bracket(s: &str) -> String {
    let mut level = 0usize;
    loop {
        let eq = "=".repeat(level);
        let close = format!("]{eq}]");
        // The closer is the first `]` + equals + `]` Lua sees. A suffix of `s`
        // can form that sequence together with the closer (`]=]` + `]]`), so
        // containment in `s` alone is not enough: the first match in `s+close`
        // must start exactly where `s` ends.
        let candidate = format!("{s}{close}");
        if candidate.find(&close) == Some(s.len()) {
            return format!("[{eq}[{s}{close}");
        }
        level += 1;
    }
}

/// Quoted Lua string. Covers quotes, backslashes, newlines, and `]]` (data).
pub fn lua_quoted(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                let mut buf = [0u8; 4];
                for b in c.encode_utf8(&mut buf).bytes() {
                    out.push_str(&format!("\\{b:03}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn shader_eval_code(path: &str) -> String {
    format!("hl.config({{ decoration = {{ screen_shader = {} }} }})", lua_encode(path))
}

/// Argv for `hyprctl`, with no shell. `eval` is used only for a path that
/// [`validate_shader_path`] has already accepted (or for Off).
pub fn screen_shader_argv(path: &str, roots: &[PathBuf], mode: HyprConfigMode) -> Result<Vec<String>, ShaderPathError> {
    let canon = validate_shader_path(path, roots)?;
    let text = canon.to_str().ok_or(ShaderPathError::UnsafeName)?.to_string();
    Ok(match mode {
        HyprConfigMode::Legacy => vec!["keyword".into(), "decoration:screen_shader".into(), text],
        HyprConfigMode::Lua => vec!["eval".into(), shader_eval_code(&text)],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::symlink;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static N: AtomicU64 = AtomicU64::new(0);

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let n = N.fetch_add(1, Ordering::Relaxed);
            let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let p = std::env::temp_dir().join(format!("shader-desk-{t}-{n}-{}", std::process::id()));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> &Path { &self.0 }
    }
    impl Drop for Scratch {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    fn write_shader(dir: &Path, name: &str) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let p = dir.join(name);
        fs::write(&p, b"#version 300 es\nvoid main(){}\n").unwrap();
        p
    }

    fn decode_long_bracket(lit: &str) -> Result<String, ()> {
        let b = lit.as_bytes();
        if b.first() != Some(&b'[') { return Err(()); }
        let mut i = 1usize;
        let mut level = 0usize;
        while i < b.len() && b[i] == b'=' { level += 1; i += 1; }
        if i >= b.len() || b[i] != b'[' { return Err(()); }
        i += 1;
        let stripped_nl = if i < b.len() && b[i] == b'\n' { i += 1; true } else { false };
        let close = format!("]{}]", "=".repeat(level));
        let rest = lit.get(i..).ok_or(())?;
        let pos = rest.find(&close).ok_or(())?;
        if pos + close.len() != rest.len() { return Err(()); }
        let mut body = rest[..pos].to_string();
        if stripped_nl { body.insert(0, '\n'); }
        Ok(body)
    }

    fn decode_quoted(lit: &str) -> Result<String, ()> {
        let b = lit.as_bytes();
        if b.first() != Some(&b'"') || b.last() != Some(&b'"') || b.len() < 2 { return Err(()); }
        let mut out = Vec::new();
        let mut i = 1usize;
        while i < b.len() - 1 {
            if b[i] != b'\\' {
                out.push(b[i]);
                i += 1;
                continue;
            }
            i += 1;
            if i >= b.len() - 1 { return Err(()); }
            match b[i] {
                b'\\' => { out.push(b'\\'); i += 1; }
                b'"' => { out.push(b'"'); i += 1; }
                b'n' => { out.push(b'\n'); i += 1; }
                b'r' => { out.push(b'\r'); i += 1; }
                d if d.is_ascii_digit() => {
                    let mut n = 0u16;
                    let mut k = 0;
                    while k < 3 && i < b.len() - 1 && b[i].is_ascii_digit() {
                        n = n * 10 + u16::from(b[i] - b'0');
                        i += 1;
                        k += 1;
                    }
                    if n > 255 { return Err(()); }
                    out.push(n as u8);
                }
                _ => return Err(()),
            }
        }
        String::from_utf8(out).map_err(|_| ())
    }

    fn decode_literal(lit: &str) -> Result<String, ()> {
        if lit.starts_with('[') { decode_long_bracket(lit) } else { decode_quoted(lit) }
    }

    fn eval_literal(code: &str) -> &str {
        let prefix = "hl.config({ decoration = { screen_shader = ";
        let suffix = " } })";
        assert!(code.starts_with(prefix) && code.ends_with(suffix), "{code}");
        &code[prefix.len()..code.len() - suffix.len()]
    }

    fn lua_roundtrip(lit: &str, expected: &str) -> bool {
        let Some(bin) = ["lua5.4", "lua5.3", "lua"].into_iter().find(|b| Command::new(b).arg("-v").output().is_ok()) else {
            return true;
        };
        let script = r#"
local lit = os.getenv("LIT")
local chunk, err = load("return " .. lit, "lit", "t")
if not chunk then io.stderr:write(err or "load", "\n"); os.exit(2) end
if chunk() ~= os.getenv("EXPECTED") then os.exit(1) end
"#;
        let out = Command::new(bin).env("LIT", lit).env("EXPECTED", expected).args(["-e", script]).output().unwrap();
        if !out.status.success() {
            eprintln!("lua: {}", String::from_utf8_lossy(&out.stderr));
        }
        out.status.success()
    }

    #[test]
    fn plain_names_and_off_stay_keyword_arguments() {
        let dir = Scratch::new();
        let file = write_shader(dir.path(), "crt-2.glsl");
        let argv = screen_shader_argv(file.to_str().unwrap(), &[dir.path().to_path_buf()], HyprConfigMode::Legacy).unwrap();
        let canon = fs::canonicalize(&file).unwrap();
        assert_eq!(argv, vec![
            "keyword".to_string(),
            "decoration:screen_shader".to_string(),
            canon.to_str().unwrap().to_string(),
        ]);
        assert!(argv.iter().all(|a| a != "eval" && a != "sh" && a != "bash" && !a.contains("hl.config")));
        let off = screen_shader_argv("", &[dir.path().to_path_buf()], HyprConfigMode::Legacy).unwrap();
        assert_eq!(off, vec!["keyword".to_string(), "decoration:screen_shader".to_string(), String::new()]);
    }

    #[test]
    fn lua_mode_encodes_only_a_validated_path() {
        let scratch = Scratch::new();
        let dir = scratch.path().join("home\"]]");
        let file = write_shader(&dir, "crt.glsl");
        let argv = screen_shader_argv(file.to_str().unwrap(), &[dir.clone()], HyprConfigMode::Lua).unwrap();
        assert_eq!(argv[0], "eval");
        assert_eq!(argv.len(), 2);
        let canon = fs::canonicalize(&file).unwrap().to_string_lossy().into_owned();
        assert!(canon.contains('"') && canon.contains("]]"));
        let lit = eval_literal(&argv[1]);
        assert_eq!(decode_literal(lit).unwrap(), canon);
        assert!(lua_roundtrip(lit, &canon));
        assert!(!argv.iter().any(|a| a == "sh" || a == "bash" || a.contains(" -c")));
    }

    #[test]
    fn malicious_filenames_never_become_a_hyprctl_command() {
        let dir = Scratch::new();
        fs::create_dir_all(dir.path()).unwrap();
        let samples = [
            r#"x"); os.execute("touch /tmp/pwn")--.glsl"#,
            "quote\".glsl",
            "a\n.glsl",
            "a\r.glsl",
            "has]].glsl",
            "semi;.glsl",
            "space name.glsl",
            "../x.glsl",
            "sub/ok.glsl",
            ".glsl",
            "noext",
            "back\\slash.glsl",
        ];
        for name in samples {
            assert!(accept_shader_import(name).is_err(), "{name}");
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() { let _ = fs::create_dir_all(parent); }
            if !name.contains('/') && !name.contains('\0') {
                let _ = fs::File::create(&path);
            }
            let text = path.to_str().unwrap_or(name);
            assert!(screen_shader_argv(text, &[dir.path().to_path_buf()], HyprConfigMode::Lua).is_err(), "{name}");
            assert!(screen_shader_argv(text, &[dir.path().to_path_buf()], HyprConfigMode::Legacy).is_err(), "{name}");
        }
    }

    #[test]
    fn traversal_symlink_and_nested_paths_are_outside() {
        let scratch = Scratch::new();
        let root = scratch.path().join("shaders");
        let outside = scratch.path().join("outside");
        let inside = write_shader(&root, "ok.glsl");
        let _ = write_shader(&outside, "crt.glsl");
        let nested = write_shader(&root.join("nested"), "crt.glsl");
        assert!(validate_shader_path(inside.to_str().unwrap(), &[root.clone()]).is_ok());
        assert_eq!(
            validate_shader_path(nested.to_str().unwrap(), &[root.clone()]).unwrap_err(),
            ShaderPathError::Outside
        );
        let escaped = root.join("../outside/crt.glsl");
        assert_eq!(validate_shader_path(escaped.to_str().unwrap(), &[root.clone()]).unwrap_err(), ShaderPathError::Outside);
        symlink(outside.join("crt.glsl"), root.join("link.glsl")).unwrap();
        assert!(validate_shader_path(root.join("link.glsl").to_str().unwrap(), &[root.clone()]).is_err());
    }

    #[test]
    fn import_listing_drops_unsafe_names() {
        let dir = Scratch::new();
        write_shader(dir.path(), "ok.glsl");
        write_shader(dir.path(), "also-ok_1.glsl");
        let evil = dir.path().join("a\"].glsl");
        fs::write(&evil, b"x").unwrap();
        let listed = list_imported_shaders(dir.path());
        let names: Vec<_> = listed.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["also-ok_1.glsl".to_string(), "ok.glsl".to_string()]);
    }

    #[test]
    fn theme_keys_cannot_escape_the_shader_directory() {
        assert_eq!(safe_theme_key("Phosphor Green").as_deref(), Some("phosphor-green"));
        assert_eq!(safe_theme_key("retro-82").as_deref(), Some("retro-82"));
        assert_eq!(safe_theme_key("../../tmp/pwn"), None);
        assert_eq!(safe_theme_key("foo;rm"), None);
        assert_eq!(safe_theme_key("a\nb"), None);
        assert_eq!(safe_theme_key("--apply"), None);
        assert_eq!(safe_theme_key(""), None);
    }

    #[test]
    fn encoder_covers_quotes_backslashes_newlines_and_bracket_closes() {
        let samples = [
            r#"x" }) os.execute("touch /tmp/pwn") --"#,
            "a\\b\"c",
            "line\nnext",
            "\nleading",
            "]]",
            "]=]",
            "]==]",
            "pre]]mid]=]end",
            "",
            "plain/crt.glsl",
        ];
        for s in samples {
            let lit = lua_encode(s);
            assert_eq!(decode_literal(&lit).unwrap(), s, "{lit}");
            assert!(lua_roundtrip(&lit, s), "{lit}");
            if !s.starts_with('\n') {
                assert!(lit.starts_with('['), "{lit}");
            }
        }
        assert_eq!(lua_quoted("a b\\'\n]]"), "\"a b\\\\'\\n]]\"");
    }

    #[test]
    fn probe_text_selects_keyword_or_eval() {
        assert_eq!(
            mode_from_probe("keyword can't work with non-legacy parsers. Use eval."),
            HyprConfigMode::Lua
        );
        assert_eq!(
            mode_from_probe("Invalid input: command and value not properly formatted"),
            HyprConfigMode::Legacy
        );
    }

    #[test]
    fn shell_hook_matches_the_rust_gate() {
        let scratch = Scratch::new();
        let home = scratch.path().join("home");
        let shaders = home.join(".config/hypr/shaders");
        let cache = home.join(".cache/shader-desk");
        write_shader(&shaders, "crt.glsl");
        write_shader(&cache, "preview.glsl");
        let evil = shaders.join("x\"); os.execute(\"id\")--.glsl");
        let _ = fs::File::create(&evil);
        fs::write(shaders.join("has]].glsl"), b"x").unwrap();
        let outside = scratch.path().join("outside.glsl");
        fs::write(&outside, b"x").unwrap();
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("emrgim-shader-desk-theme-set.sh");

        let plan = |mode: &str, path: &str| {
            Command::new("bash")
                .arg(&script)
                .arg("--plan")
                .arg(path)
                .env("HOME", &home)
                .env("SHADER_DESK_HYPR_MODE", mode)
                .output()
                .unwrap()
        };

        let ok = plan("keyword", shaders.join("crt.glsl").to_str().unwrap());
        assert!(ok.status.success(), "{}", String::from_utf8_lossy(&ok.stdout));
        let text = String::from_utf8_lossy(&ok.stdout);
        assert!(text.contains("mode keyword\n"));
        let canon = fs::canonicalize(shaders.join("crt.glsl")).unwrap();
        assert!(text.contains(&format!("path {}\n", canon.display())));

        let lua = plan("lua", cache.join("preview.glsl").to_str().unwrap());
        assert!(lua.status.success(), "{}", String::from_utf8_lossy(&lua.stderr));
        let lua_text = String::from_utf8_lossy(&lua.stdout);
        assert!(lua_text.starts_with("status ok\nmode eval\ncode "));
        let code = lua_text.lines().find_map(|l| l.strip_prefix("code ")).unwrap();
        let canon = fs::canonicalize(cache.join("preview.glsl")).unwrap().to_string_lossy().into_owned();
        assert_eq!(decode_literal(eval_literal(code)).unwrap(), canon);

        for bad in [evil.to_str().unwrap(), shaders.join("has]].glsl").to_str().unwrap(), outside.to_str().unwrap(), shaders.join("../outside.glsl").to_str().unwrap_or("")] {
            if bad.is_empty() { continue; }
            let rejected = plan("lua", bad);
            assert!(!rejected.status.success(), "{bad}: {}", String::from_utf8_lossy(&rejected.stdout));
            let body = String::from_utf8_lossy(&rejected.stdout);
            assert!(body.contains("status reject"), "{bad}: {body}");
            assert!(!body.contains("os.execute") && !body.contains("hl.config"), "{body}");
        }

        let bin = scratch.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        let log = scratch.path().join("hypr.log");
        let mut fake = fs::File::create(bin.join("hyprctl")).unwrap();
        writeln!(fake, "#!/bin/bash\nprintf '%s\\n' \"$@\" >> \"$HYPRCTL_LOG\"\n").unwrap();
        drop(fake);
        let _ = Command::new("chmod").args(["+x", bin.join("hyprctl").to_str().unwrap()]).status();

        let conf = home.join(".config/shader-desk");
        fs::create_dir_all(&conf).unwrap();
        fs::write(conf.join("theme-shaders.conf"), format!("retro-82={}\n", evil.display())).unwrap();
        let hook = Command::new("bash")
            .arg(&script)
            .arg("retro-82")
            .env("HOME", &home)
            .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()))
            .env("SHADER_DESK_HYPR_MODE", "keyword")
            .env("HYPRCTL_LOG", &log)
            .output()
            .unwrap();
        assert!(hook.status.success(), "{}", String::from_utf8_lossy(&hook.stderr));
        let recorded = fs::read_to_string(&log).unwrap();
        assert!(!recorded.contains("os.execute"), "{recorded}");
        assert_eq!(recorded, "keyword\ndecoration:screen_shader\n\n");

        fs::write(conf.join("theme-shaders.conf"), format!("retro-82={}\n", canon_display(&shaders.join("crt.glsl")))).unwrap();
        fs::write(&log, "").unwrap();
        let hook = Command::new("bash").arg(&script).arg("retro-82")
            .env("HOME", &home)
            .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()))
            .env("SHADER_DESK_HYPR_MODE", "lua")
            .env("HYPRCTL_LOG", &log)
            .output().unwrap();
        assert!(hook.status.success(), "{}", String::from_utf8_lossy(&hook.stderr));
        let recorded = fs::read_to_string(&log).unwrap();
        assert!(recorded.starts_with("eval\n"), "{recorded}");
        assert!(!recorded.contains("os.execute"));
        let code = recorded.lines().nth(1).unwrap();
        let expected = fs::canonicalize(shaders.join("crt.glsl")).unwrap().to_string_lossy().into_owned();
        assert_eq!(decode_literal(eval_literal(code)).unwrap(), expected);
    }

    fn canon_display(path: &Path) -> String {
        fs::canonicalize(path).unwrap().to_string_lossy().into_owned()
    }
}
