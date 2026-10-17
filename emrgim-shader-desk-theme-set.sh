#!/bin/bash
# emrgim.shader-desk theme-set hook.
# omarchy-hook theme-set passes the theme name as $1.

set -u

theme=$(printf '%s' "${1:-}" | tr '[:upper:]' '[:lower:]')
theme=${theme#"${theme%%[![:space:]]*}"}
theme=${theme%"${theme##*[![:space:]]}"}
theme=${theme// /-}

conf="${HOME}/.config/shader-desk/theme-shaders.conf"
flag="${HOME}/.config/shader-desk/manual-off"
shaders="${HOME}/.config/hypr/shaders"

apply() {
  hyprctl keyword decoration:screen_shader "$1"
}

if [[ -f $flag ]]; then
  apply ""
  exit $?
fi

found=0
path=""
if [[ -n $theme && -f $conf ]]; then
  while IFS= read -r line || [[ -n $line ]]; do
    line=${line#"${line%%[![:space:]]*}"}
    line=${line%"${line##*[![:space:]]}"}
    [[ -z $line || $line == \#* ]] && continue
    key=${line%%=*}
    value=${line#*=}
    key=${key%"${key##*[![:space:]]}"}
    value=${value#"${value%%[![:space:]]*}"}
    if [[ $key == "$theme" ]]; then
      path=$value
      found=1
      break
    fi
  done <"$conf"
fi

if [[ $found == 1 ]]; then
  if [[ -z $path || -f $path ]]; then
    apply "$path"
    exit $?
  fi
fi

fallback="${shaders}/${theme}.glsl"
if [[ $theme == phosphor-green ]]; then
  fallback="${shaders}/phosphor.glsl"
fi
if [[ -n $theme && -f $fallback ]]; then
  apply "$fallback"
  exit $?
fi

apply ""
exit $?
