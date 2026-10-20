# Shader Desk

Version 1.0.3.

Shader Desk is a native desktop app for [Omarchy](https://omarchy.org) that matches a Hyprland screen shader (CRT, phosphor and more) to each Omarchy theme. Pick a shader once per theme, and every time the theme changes, the right shader is applied automatically.

The same matching is available as the Omarchy bar plugin `emrgim.shader-desk`.

## Plugin

Click **Shader** on the bar, choose a theme, then choose a `.glsl` file from `~/.config/hypr/shaders`. **Off** clears that theme. The choice is saved only when you pick one.

Saved lines go to `~/.config/shader-desk/theme-shaders.conf` as `theme=path` (an empty path is Off). The first save also installs `~/.config/omarchy/hooks/theme-set.d/emrgim-shader-desk.sh`. On the next theme change that hook applies the theme's shader. The current theme is applied immediately. **Open Shader Desk** launches `~/.local/bin/shader-desk` when that binary is installed.

Only a direct `.glsl` file under `~/.config/hypr/shaders` (or a Shader Desk preview under `~/.cache/shader-desk`) is applied, and the filename has to match `[A-Za-z0-9._-]+.glsl`. Anything else is ignored. On a legacy Hyprland config the path is passed as an argument to `hyprctl keyword decoration:screen_shader`. Omarchy Quattro's Lua config rejects `keyword`, so the same checked path is sent with `hyprctl eval` as a Lua long-bracket literal. It is not pasted into a shell command.

### Install

```sh
omarchy plugin add https://github.com/emrgim/shader-desk.git
omarchy plugin enable emrgim.shader-desk
```

### Remove

```sh
omarchy plugin remove emrgim.shader-desk
rm -f ~/.config/omarchy/hooks/theme-set.d/emrgim-shader-desk.sh
```

`~/.config/shader-desk/theme-shaders.conf` is left in place.

### Dependencies

- Omarchy shell and Hyprland (`hyprctl`)
- Optional native `shader-desk` binary from the Build section below

The plugin runs inside the Omarchy shell with your user permissions. It writes the theme-shader file and the theme-set hook only after you pick a shader.

## How it's built

- Rust, 2024 edition
- GTK4 with libadwaita for a native GNOME-style interface
- `regex` and `serde_json`
- Quickshell QML for the `emrgim.shader-desk` bar plugin

Native app ID: `emrgim.ShaderDesk`. Plugin ID: `emrgim.shader-desk` (the marketplace requires lowercase plugin IDs). All of Emiliano's projects use the `emrgim.*` ID prefix.

## Where it lives

| What | Path |
| --- | --- |
| Binary | `~/.local/bin/shader-desk` |
| Desktop entry | `~/.local/share/applications/shader-desk.desktop` |
| Omarchy menu entry | `~/.config/omarchy/extensions/omarchy-menu.jsonc` (`shader-desk`) |
| Plugin | repository root, id `emrgim.shader-desk` |
| Theme-change hook | `~/.config/omarchy/hooks/theme-set.d/emrgim-shader-desk.sh` |
| GLSL shaders | `~/.config/hypr/shaders` |
| Per-theme config | `~/.config/shader-desk/theme-shaders.conf` |

## Build

```sh
cargo build --release
install -Dm755 target/release/shader-desk ~/.local/bin/shader-desk
install -Dm644 data/shader-desk.desktop ~/.local/share/applications/shader-desk.desktop
```

## Changelog

### 1.0.3

The native compile check no longer writes `shader-desk-check-<pid>.frag` in the shared temp directory. Preview and Save run `glslangValidator` on an exclusively created `.frag` file in a per-user directory (`$XDG_RUNTIME_DIR/shader-desk`, or `~/.cache/shader-desk/compile-check` when that runtime directory is missing or not private). The file is removed when the check finishes. If it cannot be created, Preview and Save show the error. Valid and invalid shaders are still accepted and rejected as before. The plugin manifest and the `shader-desk` crate are both 1.0.3.

### 1.0.2

A shader file's `// shader-desk:` comment is ignored when it is not a JSON object, is malformed, or has fields of an unexpected type. Edit falls back to the built-in defaults instead of panicking. Valid presets are unchanged. The plugin manifest and the `shader-desk` crate are both 1.0.2.

### 1.0.1

Shader paths are no longer interpolated into `hyprctl eval`. A path is applied only when it canonicalizes to a file directly inside the shader directories and its filename matches `[A-Za-z0-9._-]+.glsl`. Legacy compositors receive `hyprctl keyword decoration:screen_shader <path>` as separate arguments. Omarchy Quattro (Lua config) receives `hyprctl eval` with that validated path encoded as a Lua long-bracket literal. Imported filenames outside the charset are not listed and not applied. The plugin manifest and the `shader-desk` crate are both 1.0.1.

### 1.0.0

First marketplace listing of the `emrgim.shader-desk` bar plugin.

## License

MIT. See [LICENSE](LICENSE).

## Preview

`preview.png` is captured by `demo/run`: the real Omarchy shell (Quickshell) in a headless sway session, with a throwaway HOME of fictional fixtures from `demo/fixtures` (three themes, three placeholder shaders, saved choices, retro-82 as the current theme). The script enables the plugin, opens its panel and saves a 1680x1050 crop. It never touches your own configuration or running shell.

```sh
OMARCHY_PATH=/path/to/omarchy DEMO_FONT_DIR=/path/to/JetBrainsMonoNerdFont demo/run preview.png
```
