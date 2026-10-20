//! Private glslang compile check for Preview and Save.
//!
//! The check used to write `shader-desk-check-<pid>.frag` in the shared temp
//! directory. Another local user can plant a directory, file, or symlink at
//! that predictable name. The validator now reads an exclusively created
//! `.frag` file under `$XDG_RUNTIME_DIR/shader-desk` (mode 0700), or under
//! `~/.cache/shader-desk/compile-check` when the runtime directory is missing
//! or not private. The `.frag` suffix is what glslangValidator uses for stage
//! detection; `-S frag` selects the same stage when the suffix is ignored.
//! The file is removed when the check finishes. If it cannot be created, the
//! error is returned to Preview and Save.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Linux `O_NOFOLLOW`: do not open a trailing symlink.
#[cfg(target_os = "linux")]
const O_NOFOLLOW: i32 = 0o400000;

const APP_DIR_MODE: u32 = 0o700;
const CHECK_FILE_MODE: u32 = 0o600;

fn current_user_id() -> u32 {
    // `geteuid` reads the process credential and does not touch memory we own.
    unsafe { geteuid() }
}

unsafe extern "C" {
    fn geteuid() -> u32;
}

pub fn compile_ok(src: &str) -> Result<(), String> {
    compile_in(&compile_check_dir()?, src)
}

fn compile_in(dir: &Path, src: &str) -> Result<(), String> {
    let mut check = create_compile_check(dir)?;
    check.write_shader(src)?;
    let output = run_validator(&check)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(validator_failure(&output))
    }
}

fn compile_check_dir() -> Result<PathBuf, String> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    let runtime = runtime.as_deref().filter(|p| !p.as_os_str().is_empty());
    compile_check_dir_from(runtime, Path::new(&crate::home()))
}

fn compile_check_dir_from(runtime: Option<&Path>, home: &Path) -> Result<PathBuf, String> {
    let runtime_err = match runtime {
        Some(dir) => match runtime_compile_dir(dir) {
            Ok(path) => return Ok(path),
            Err(err) => Some(err),
        },
        None => None,
    };
    match home_compile_dir_from(home) {
        Ok(path) => Ok(path),
        Err(err) => Err(match runtime_err {
            Some(runtime) => {
                format!("could not create a private compile check file: {runtime}; {err}")
            }
            None => format!("could not create a private compile check file: {err}"),
        }),
    }
}

fn runtime_compile_dir(runtime: &Path) -> Result<PathBuf, String> {
    accept_anchor_dir(runtime)?;
    let child = runtime.join("shader-desk");
    ensure_app_dir(&child)?;
    Ok(child)
}

fn home_compile_dir_from(home: &Path) -> Result<PathBuf, String> {
    let home = resolve_existing_dir(home)?;
    accept_anchor_dir(&home)?;
    let cache = ensure_anchor_dir(&home.join(".cache"))?;
    let app = cache.join("shader-desk");
    ensure_app_dir(&app)?;
    let leaf = app.join("compile-check");
    ensure_app_dir(&leaf)?;
    Ok(leaf)
}

fn resolve_existing_dir(path: &Path) -> Result<PathBuf, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.file_type().is_symlink() {
        fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))
    } else if meta.is_dir() {
        Ok(path.to_path_buf())
    } else {
        Err(format!("{} is not a directory", path.display()))
    }
}

/// A directory this process did not create. It is usable only when another
/// local user cannot create entries in it. Symlinks are followed once, because
/// `~/.cache` is sometimes a link, and the target has to pass the same check.
fn ensure_anchor_dir(path: &Path) -> Result<PathBuf, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            create_mode_700(path)?;
            restrict_dir(path, APP_DIR_MODE)?;
            accept_anchor_dir(path)?;
            Ok(path.to_path_buf())
        }
        Err(e) => Err(format!("{}: {e}", path.display())),
        Ok(meta) if meta.file_type().is_symlink() => {
            let real = fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
            accept_anchor_dir(&real)?;
            Ok(real)
        }
        Ok(_) => {
            accept_anchor_dir(path)?;
            Ok(path.to_path_buf())
        }
    }
}

fn accept_anchor_dir(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!("{} is not an absolute path", path.display()));
    }
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!("{} is a symlink", path.display()));
    }
    if !meta.is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }
    if meta.uid() != current_user_id() {
        return Err(format!("{} is not owned by this user", path.display()));
    }
    if meta.mode() & 0o022 != 0 {
        return Err(format!("{} is writable by other users", path.display()));
    }
    Ok(())
}

/// Directory used for the check. Never a symlink. A new directory is mode
/// 0700. An existing directory is left alone when other users cannot write
/// to it, and restricted to 0700 when they can.
fn ensure_app_dir(path: &Path) -> Result<(), String> {
    if let Err(e) = fs::symlink_metadata(path) {
        if e.kind() != io::ErrorKind::NotFound {
            return Err(format!("{}: {e}", path.display()));
        }
        create_mode_700(path)?;
        restrict_dir(path, APP_DIR_MODE)?;
    }
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!("{} is a symlink", path.display()));
    }
    if !meta.is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }
    if meta.uid() != current_user_id() {
        return Err(format!("{} is not owned by this user", path.display()));
    }
    if meta.mode() & 0o022 != 0 {
        restrict_dir(path, APP_DIR_MODE)?;
    }
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.file_type().is_symlink()
        || !meta.is_dir()
        || meta.uid() != current_user_id()
        || meta.mode() & 0o022 != 0
    {
        return Err(format!("{} is not a private directory", path.display()));
    }
    Ok(())
}

fn create_mode_700(path: &Path) -> Result<(), String> {
    match fs::DirBuilder::new().mode(APP_DIR_MODE).create(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(format!("could not create {}: {e}", path.display())),
    }
}

fn restrict_dir(path: &Path, mode: u32) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!("{} is a symlink", path.display()));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|e| format!("could not restrict {}: {e}", path.display()))
}

#[derive(Debug)]
struct CompileCheck {
    path: PathBuf,
    file: Option<File>,
}

impl Drop for CompileCheck {
    fn drop(&mut self) {
        self.file.take();
        let _ = fs::remove_file(&self.path);
    }
}

fn create_compile_check(dir: &Path) -> Result<CompileCheck, String> {
    ensure_app_dir(dir)
        .map_err(|e| format!("could not create a private compile check file: {e}"))?;
    let mut last = None;
    for _ in 0..16 {
        let name = format!("check-{}.frag", random_hex(16)?);
        let path = dir.join(name);
        match open_exclusive(&path) {
            Ok(file) => {
                if let Err(e) = file.set_permissions(fs::Permissions::from_mode(CHECK_FILE_MODE)) {
                    drop(file);
                    let _ = fs::remove_file(&path);
                    return Err(format!(
                        "could not create a private compile check file: {e}"
                    ));
                }
                return Ok(CompileCheck {
                    path,
                    file: Some(file),
                });
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = Some(e.to_string()),
            Err(e) => {
                return Err(format!(
                    "could not create a private compile check file: {e}"
                ));
            }
        }
    }
    Err(format!(
        "could not create a private compile check file: {}",
        last.unwrap_or_else(|| "name collision".to_string())
    ))
}

fn open_exclusive(path: &Path) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true).mode(CHECK_FILE_MODE);
    #[cfg(target_os = "linux")]
    opts.custom_flags(O_NOFOLLOW);
    opts.open(path)
}

impl CompileCheck {
    fn write_shader(&mut self, src: &str) -> Result<(), String> {
        let mut file = self
            .file
            .take()
            .ok_or_else(|| "compile check file is not open".to_string())?;
        let write = (|| {
            file.write_all(src.as_bytes())?;
            file.flush()?;
            Ok(())
        })();
        drop(file);
        write.map_err(|e: io::Error| format!("could not write the compile check file: {e}"))?;
        let meta = fs::symlink_metadata(&self.path)
            .map_err(|e| format!("could not read the compile check file: {e}"))?;
        if !meta.file_type().is_file() || meta.file_type().is_symlink() {
            return Err("compile check path is not a regular file".to_string());
        }
        if meta.mode() & 0o077 != 0 {
            return Err("compile check file is readable by other users".to_string());
        }
        Ok(())
    }

    fn path_str(&self) -> Result<&str, String> {
        self.path
            .to_str()
            .ok_or_else(|| "compile check path is not valid UTF-8".to_string())
    }
}

fn run_validator(check: &CompileCheck) -> Result<Output, String> {
    let path = check.path_str()?;
    // The `.frag` suffix is the stage glslangValidator detects from the name.
    // `-S frag` selects that same stage. `--stdin` is not used: glslang 15
    // replaces the stage with the following argument when it sees `--stdin`.
    Command::new("glslangValidator")
        .args(["-S", "frag", path])
        .output()
        .map_err(|e| format!("could not run glslangValidator: {e}"))
}

fn validator_failure(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.trim().is_empty() {
        return stdout.into_owned();
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        return stderr.into_owned();
    }
    format!("glslangValidator failed ({})", output.status)
}

fn random_hex(bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; bytes];
    let mut file = fs::File::open("/dev/urandom").map_err(|e| {
        format!("could not create a private compile check file: could not read /dev/urandom: {e}")
    })?;
    io::Read::read_exact(&mut file, &mut buf).map_err(|e| {
        format!("could not create a private compile check file: could not read /dev/urandom: {e}")
    })?;
    let mut hex = String::with_capacity(bytes * 2);
    for byte in buf {
        hex.push_str(&format!("{byte:02x}"));
    }
    Ok(hex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
    use std::time::{SystemTime, UNIX_EPOCH};

    const VALID_FRAG: &str = "\
#version 300 es
precision highp float;
layout(location = 0) out vec4 fragColor;
void main() {
    fragColor = vec4(gl_FragCoord.xy, 0.0, 1.0);
}
";
    const INVALID_FRAG: &str = "this is not a shader\n";

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let n = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("shader-desk-cc-{n}-{}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn mode(path: &Path) -> u32 {
        fs::symlink_metadata(path).unwrap().mode() & 0o777
    }

    fn make_dir(path: &Path, permissions: u32) {
        fs::DirBuilder::new()
            .mode(permissions)
            .create(path)
            .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(permissions)).unwrap();
    }

    fn legacy_shared_path() -> PathBuf {
        PathBuf::from(format!(
            "{}/shader-desk-check-{}.frag",
            std::env::temp_dir().display(),
            std::process::id()
        ))
    }

    fn frag_names(dir: &Path) -> Vec<String> {
        let mut names = Vec::new();
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("check-") && name.ends_with(".frag") {
                names.push(name);
            }
        }
        names.sort();
        names
    }

    fn remove_any(path: &Path) {
        let Ok(meta) = fs::symlink_metadata(path) else {
            return;
        };
        if meta.is_dir() && !meta.file_type().is_symlink() {
            let _ = fs::remove_dir_all(path);
        } else {
            let _ = fs::remove_file(path);
        }
    }

    #[test]
    fn private_dir_prefers_runtime_and_falls_back() {
        let scratch = Scratch::new();
        let runtime = scratch.0.join("runtime");
        let home = scratch.0.join("home");
        make_dir(&runtime, 0o700);
        make_dir(&home, 0o700);

        let chosen = compile_check_dir_from(Some(&runtime), &home).unwrap();
        assert_eq!(chosen, runtime.join("shader-desk"));
        assert_eq!(mode(&chosen), 0o700);
        assert!(
            !fs::symlink_metadata(&chosen)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(!chosen.starts_with(&home));

        let open = scratch.0.join("open-runtime");
        make_dir(&open, 0o777);
        let chosen = compile_check_dir_from(Some(&open), &home).unwrap();
        assert_eq!(chosen, home.join(".cache/shader-desk/compile-check"));
        assert_eq!(mode(&chosen), 0o700);
        assert!(!chosen.starts_with(&open));

        let link = scratch.0.join("runtime-link");
        symlink(&runtime, &link).unwrap();
        let chosen = compile_check_dir_from(Some(&link), &home).unwrap();
        assert_eq!(chosen, home.join(".cache/shader-desk/compile-check"));

        let file = scratch.0.join("runtime-file");
        fs::write(&file, b"x").unwrap();
        let chosen = compile_check_dir_from(Some(&file), &home).unwrap();
        assert_eq!(chosen, home.join(".cache/shader-desk/compile-check"));

        let open_home = scratch.0.join("open-home");
        make_dir(&open_home, 0o777);
        let err = compile_check_dir_from(Some(&open), &open_home).unwrap_err();
        assert!(
            err.contains("could not create a private compile check file"),
            "{err}"
        );
        assert!(err.contains("writable by other users"), "{err}");

        let err = create_compile_check(Path::new("/dev/null")).unwrap_err();
        assert!(
            err.contains("could not create a private compile check file"),
            "{err}"
        );
        let missing = scratch.0.join("missing-parent/leaf");
        let err = create_compile_check(&missing).unwrap_err();
        assert!(
            err.contains("could not create a private compile check file"),
            "{err}"
        );

        let home_link = scratch.0.join("home-with-link");
        make_dir(&home_link, 0o700);
        let cache = home_link.join(".cache");
        make_dir(&cache, 0o755);
        let elsewhere = scratch.0.join("elsewhere");
        make_dir(&elsewhere, 0o700);
        symlink(&elsewhere, &cache.join("shader-desk")).unwrap();
        let err = home_compile_dir_from(&home_link).unwrap_err();
        assert!(err.contains("symlink"), "{err}");
        assert_eq!(mode(&elsewhere), 0o700);

        let home_open_app = scratch.0.join("home-open-app");
        make_dir(&home_open_app, 0o755);
        let open_app = home_open_app.join(".cache/shader-desk");
        make_dir(&home_open_app.join(".cache"), 0o755);
        make_dir(&open_app, 0o777);
        let leaf = home_compile_dir_from(&home_open_app).unwrap();
        assert_eq!(leaf, open_app.join("compile-check"));
        assert_eq!(mode(&open_app), 0o700);
        assert_eq!(mode(&leaf), 0o700);

        let home_shared_read = scratch.0.join("home-shared-read");
        make_dir(&home_shared_read, 0o755);
        let cache = home_shared_read.join(".cache");
        let app = cache.join("shader-desk");
        make_dir(&cache, 0o755);
        make_dir(&app, 0o755);
        let leaf = home_compile_dir_from(&home_shared_read).unwrap();
        assert_eq!(mode(&app), 0o755);
        assert_eq!(mode(&leaf), 0o700);
    }

    #[test]
    fn check_file_is_private_and_removed_without_the_validator() {
        let scratch = Scratch::new();
        let home = scratch.0.join("home");
        make_dir(&home, 0o755);
        let dir = home_compile_dir_from(&home).unwrap();
        assert_eq!(mode(&dir), 0o700);
        let mut check = create_compile_check(&dir).unwrap();
        let path = check.path.clone();
        check.write_shader(VALID_FRAG).unwrap();
        let meta = fs::symlink_metadata(&path).unwrap();
        assert!(meta.file_type().is_file());
        assert_eq!(meta.mode() & 0o777, 0o600);
        assert_eq!(fs::read(&path).unwrap(), VALID_FRAG.as_bytes());
        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("frag"));
        drop(check);
        assert!(!path.exists());
        assert!(frag_names(&dir).is_empty());

        assert!(compile_in(&dir, VALID_FRAG).is_ok());
        assert!(frag_names(&dir).is_empty());
        let invalid = compile_in(&dir, INVALID_FRAG).unwrap_err();
        assert!(invalid.contains("ERROR"), "{invalid}");
        assert!(
            !invalid.contains("could not create a private compile check file"),
            "{invalid}"
        );
        assert!(frag_names(&dir).is_empty());
    }

    #[test]
    fn precreated_shared_temp_names_do_not_block_or_change_the_check() {
        let legacy = legacy_shared_path();
        let sentinel =
            std::env::temp_dir().join(format!("shader-desk-sentinel-{}", std::process::id()));
        let _cleanup = scopeguard_paths(vec![legacy.clone(), sentinel.clone()]);

        remove_any(&legacy);
        assert!(compile_ok(VALID_FRAG).is_ok());
        assert!(fs::symlink_metadata(&legacy).is_err());

        fs::create_dir(&legacy).unwrap();
        assert!(compile_ok(VALID_FRAG).is_ok());
        let invalid = compile_ok(INVALID_FRAG).unwrap_err();
        assert!(invalid.contains("ERROR"), "{invalid}");
        assert!(
            !invalid.contains("could not create a private compile check file"),
            "{invalid}"
        );
        assert!(fs::symlink_metadata(&legacy).unwrap().is_dir());

        remove_any(&legacy);
        fs::write(&legacy, b"precreated-file").unwrap();
        assert!(compile_ok(VALID_FRAG).is_ok());
        assert_eq!(fs::read(&legacy).unwrap(), b"precreated-file");
        assert!(compile_ok(INVALID_FRAG).unwrap_err().contains("ERROR"));
        assert_eq!(fs::read(&legacy).unwrap(), b"precreated-file");

        remove_any(&legacy);
        fs::write(&sentinel, b"sentinel-contents").unwrap();
        symlink(&sentinel, &legacy).unwrap();
        assert!(compile_ok(VALID_FRAG).is_ok());
        assert_eq!(fs::read(&sentinel).unwrap(), b"sentinel-contents");
        assert!(
            fs::symlink_metadata(&legacy)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_link(&legacy).unwrap(), sentinel);
        assert!(compile_ok(INVALID_FRAG).unwrap_err().contains("ERROR"));
        assert_eq!(fs::read(&sentinel).unwrap(), b"sentinel-contents");
        assert!(
            fs::symlink_metadata(&legacy)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    struct PathCleanup(Vec<PathBuf>);
    impl Drop for PathCleanup {
        fn drop(&mut self) {
            for path in &self.0 {
                let meta = fs::symlink_metadata(path);
                let Ok(meta) = meta else { continue };
                if meta.is_dir() && !meta.file_type().is_symlink() {
                    let _ = fs::remove_dir_all(path);
                } else {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }
    fn scopeguard_paths(paths: Vec<PathBuf>) -> PathCleanup {
        PathCleanup(paths)
    }
}
