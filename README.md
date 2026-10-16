# Shader Desk

Shader Desk is a native desktop app for [Omarchy](https://omarchy.org) that matches a Hyprland screen shader (CRT, phosphor and more) to each Omarchy theme. Pick a shader once per theme, and every time the theme changes, the right shader is applied automatically.

## How it's built

- Rust, 2024 edition
- GTK4 with libadwaita for a native GNOME-style interface
- `regex` and `serde_json`

App ID: `emrgim.ShaderDesk`. All of Emiliano's projects use the `emrgim.*` ID prefix.

## Where it lives

| What | Path |
| --- | --- |
| Binary | `~/.local/bin/shader-desk` |
| Desktop entry | `~/.local/share/applications/shader-desk.desktop` |
| Omarchy menu entry | `~/.config/omarchy/extensions/omarchy-menu.jsonc` (`shader-desk`) |
| Theme-change hook | `~/.config/omarchy/hooks/theme-set.d/shader.sh` |
| GLSL shaders | `~/.config/hypr/shaders` |
| Per-theme config | `~/.config/shader-desk/theme-shaders.conf` |

## Build

```sh
cargo build --release
install -Dm755 target/release/shader-desk ~/.local/bin/shader-desk
```
