#!/bin/bash
# emrgim.shader-desk: shaders, themes, current theme, and current screen shader.

set -u

home=${HOME:-}
shaders_dir="$home/.config/hypr/shaders"
theme_name_file="$home/.local/state/omarchy/current/theme.name"

json_escape() {
  local s=$1
  s=${s//\\/\\\\}
  s=${s//\"/\\\"}
  s=${s//$'\n'/}
  printf '%s' "$s"
}

slug() {
  local s=$1
  s=$(printf '%s' "$s" | tr '[:upper:]' '[:lower:]')
  s=${s#"${s%%[![:space:]]*}"}
  s=${s%"${s##*[![:space:]]}"}
  s=${s// /-}
  printf '%s' "$s"
}

shaders=()
if [[ -d $shaders_dir ]]; then
  while IFS= read -r file; do
    [[ -f $file ]] || continue
    shaders+=("$file")
  done < <(find "$shaders_dir" -maxdepth 1 -type f -name '*.glsl' | sort)
fi

declare -A seen=()
themes=()
add_theme() {
  local name
  name=$(slug "$1")
  [[ -n $name ]] || return 0
  [[ -z ${seen[$name]:-} ]] || return 0
  seen[$name]=1
  themes+=("$name")
}

for dir in "$home/.config/omarchy/themes" "$home/.local/share/omarchy/themes" /usr/share/omarchy/themes; do
  [[ -d $dir ]] || continue
  while IFS= read -r entry; do
    [[ -d $entry ]] || continue
    add_theme "$(basename "$entry")"
  done < <(find "$dir" -mindepth 1 -maxdepth 1 -type d | sort)
done

if command -v omarchy-theme-list >/dev/null 2>&1; then
  while IFS= read -r line; do
    [[ -n ${line//[[:space:]]/} ]] || continue
    add_theme "$line"
  done < <(omarchy-theme-list 2>/dev/null || true)
fi

current_theme=""
if [[ -f $theme_name_file ]]; then
  current_theme=$(slug "$(cat "$theme_name_file")")
  [[ -n $current_theme ]] && add_theme "$current_theme"
fi

current_shader=""
if command -v hyprctl >/dev/null 2>&1; then
  option=$(hyprctl getoption decoration:screen_shader 2>/dev/null || true)
  line=$(printf '%s\n' "$option" | sed -n 's/^str:[[:space:]]*//p' | head -n 1)
  if [[ -n $line && $line != "[[EMPTY]]" ]]; then
    current_shader=$line
  fi
fi

shader_desk_bin=""
if [[ -x $home/.local/bin/shader-desk ]]; then
  shader_desk_bin="$home/.local/bin/shader-desk"
elif command -v shader-desk >/dev/null 2>&1; then
  shader_desk_bin=$(command -v shader-desk)
fi

printf '{'
printf '"shaders":['
first=1
for shader in "${shaders[@]+"${shaders[@]}"}"; do
  [[ $first == 1 ]] || printf ','
  first=0
  printf '"%s"' "$(json_escape "$shader")"
done
printf '],"themes":['
first=1
for theme in "${themes[@]+"${themes[@]}"}"; do
  [[ $first == 1 ]] || printf ','
  first=0
  printf '"%s"' "$(json_escape "$theme")"
done
printf '],"currentTheme":"%s","currentShader":"%s","shaderDeskBin":"%s"}\n' \
  "$(json_escape "$current_theme")" \
  "$(json_escape "$current_shader")" \
  "$(json_escape "$shader_desk_bin")"
