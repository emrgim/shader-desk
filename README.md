# Shader Desk

Shader Desk is a native desktop app for [Omarchy](https://omarchy.org) that matches a Hyprland screen shader (CRT, phosphor and more) to each Omarchy theme. Pick a shader once per theme, and every time the theme changes, the right shader is applied automatically.

The same matching is available as the Omarchy bar plugin `emrgim.shader-desk`.

## Plugin

Click **Shader** on the bar, choose a theme, then choose a `.glsl` file from `~/.config/hypr/shaders`. **Off** clears that theme. The choice is saved only when you pick one.

Saved lines go to `~/.config/shader-desk/theme-shaders.conf` as `theme=path` (an empty path is Off). The first save also installs `~/.config/omarchy/hooks/theme-set.d/emrgim-shader-desk.sh`. On the next theme change that hook runs `hyprctl keyword decoration:screen_shader` for the theme's shader. The current theme is applied immediately. **Open Shader Desk** launches `~/.local/bin/shader-desk` when that binary is installed.

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

## License

MIT. See [LICENSE](LICENSE).

## Preview

`preview.png` is captured by `demo/run`: the real Omarchy shell (Quickshell) in a headless sway session, with a throwaway HOME of fictional fixtures from `demo/fixtures` (three themes, three placeholder shaders, saved choices, retro-82 as the current theme). The script enables the plugin, opens its panel and saves a 1680x1050 crop. It never touches your own configuration or running shell.

```sh
OMARCHY_PATH=/path/to/omarchy DEMO_FONT_DIR=/path/to/JetBrainsMonoNerdFont demo/run preview.png
```
