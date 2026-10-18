#!/bin/bash
# emrgim.shader-desk theme-set hook.
# omarchy-hook theme-set passes the theme name as $1.
#
# A shader path is never interpolated into a shell command or a raw Lua string.
# Legacy Hyprland: `hyprctl keyword decoration:screen_shader <path>` (argv).
# Lua config (Omarchy Quattro rejects keyword): `hyprctl eval` with the path
# encoded as a Lua long-bracket literal, and only after the path is canonical
# and inside the shader directories.
#
# --check <path>   print status, do not call hyprctl
# --plan <path>    print the hyprctl argv shape, do not call hyprctl
# --apply <path>   validate and apply (empty path or "off" clears the shader)

set -u

safe_shader_name() {
  [[ $1 =~ ^[A-Za-z0-9._-]+\.glsl$ ]]
}

safe_theme_name() {
  [[ $1 =~ ^[a-z0-9][a-z0-9._-]*$ ]]
}

# Bash variables cannot hold NUL. A pattern built with $'\0' collapses and
# matches every string, so only newline, CR, and tab are tested here.
path_has_control() {
  local s=$1
  [[ $s == *$'\n'* || $s == *$'\r'* || $s == *$'\t'* ]]
}

# Long bracket. Quotes, backslashes, and newlines stay data. The '=' run is
# chosen so a ]] / ]=] / ]==] sequence inside the path cannot close it.
lua_long_bracket() {
  local s=$1 level=0 eq close candidate head i
  while (( level <= ${#s} + 16 )); do
    eq=
    for ((i = 0; i < level; i++)); do
      eq+='='
    done
    close="]${eq}]"
    candidate="${s}${close}"
    # Longest suffix that starts at a closer. What remains must be exactly s,
    # so the first closer is the one we appended (]=] glued to ]] is rejected).
    head=${candidate%%"$close"*}
    if [[ $head == "$s" ]]; then
      printf '[%s[%s%s' "$eq" "$s" "$close"
      return 0
    fi
    level=$((level + 1))
  done
  return 1
}

hypr_mode() {
  case ${SHADER_DESK_HYPR_MODE:-} in
    keyword|legacy) printf '%s\n' keyword; return ;;
    lua|eval) printf '%s\n' lua; return ;;
  esac
  local probe
  probe=$(hyprctl keyword decoration:screen_shader 2>&1 || true)
  if [[ $probe == *non-legacy* || $probe == *"Use eval"* ]]; then
    printf '%s\n' lua
  else
    printf '%s\n' keyword
  fi
}

# Prints the canonical path, or nothing for Off. Nonzero when refused.
validate_shader() {
  local path=$1
  if [[ -z $path || $path == off ]]; then
    printf '%s' ""
    return 0
  fi
  if path_has_control "$path"; then
    return 1
  fi
  case $path in
    *../*|../*|*/..|..) return 1 ;;
  esac
  local base=${path##*/}
  safe_shader_name "$base" || return 1
  [[ -f $path ]] || return 1
  local canon
  canon=$(readlink -e -- "$path") || return 1
  if path_has_control "$canon"; then
    return 1
  fi
  local cbase=${canon##*/}
  local cdir=${canon%/*}
  safe_shader_name "$cbase" || return 1
  local home=${HOME:-} root real
  for root in "$home/.config/hypr/shaders" "$home/.cache/shader-desk"; do
    [[ -d $root ]] || continue
    real=$(readlink -e -- "$root") || continue
    if [[ $cdir == "$real" ]]; then
      printf '%s' "$canon"
      return 0
    fi
  done
  return 1
}

set_screen_shader() {
  local path=$1 mode lit code
  mode=$(hypr_mode)
  if [[ $mode == lua ]]; then
    lit=$(lua_long_bracket "$path") || return 1
    printf -v code 'hl.config({ decoration = { screen_shader = %s } })' "$lit"
    hyprctl eval "$code"
    return
  fi
  hyprctl keyword decoration:screen_shader "$path"
}

dispatch() {
  local cmd=$1 path=${2:-} canon mode lit code
  if [[ $path == off ]]; then
    path=
  fi
  if ! canon=$(validate_shader "$path"); then
    printf 'status reject\n'
    return 1
  fi
  case $cmd in
    --check)
      printf 'status ok\npath %s\n' "$canon"
      ;;
    --plan)
      mode=$(hypr_mode)
      if [[ $mode == lua ]]; then
        lit=$(lua_long_bracket "$canon") || { printf 'status reject\n'; return 1; }
        printf -v code 'hl.config({ decoration = { screen_shader = %s } })' "$lit"
        printf 'status ok\nmode eval\ncode %s\n' "$code"
      else
        printf 'status ok\nmode keyword\npath %s\n' "$canon"
      fi
      ;;
    --apply)
      set_screen_shader "$canon"
      ;;
    *)
      printf 'status reject\n'
      return 1
      ;;
  esac
}

slug() {
  local s=$1
  s=$(printf '%s' "$s" | tr '[:upper:]' '[:lower:]')
  s=${s#"${s%%[![:space:]]*}"}
  s=${s%"${s##*[![:space:]]}"}
  s=${s// /-}
  printf '%s' "$s"
}

if [[ ${1:-} == --check || ${1:-} == --plan || ${1:-} == --apply ]]; then
  dispatch "$1" "${2:-}"
  exit $?
fi

home=${HOME:-}
theme=$(slug "${1:-}")
conf="$home/.config/shader-desk/theme-shaders.conf"
flag="$home/.config/shader-desk/manual-off"
shaders="$home/.config/hypr/shaders"

if [[ -f $flag ]]; then
  set_screen_shader ""
  exit $?
fi

if ! safe_theme_name "$theme"; then
  echo "shader-desk: ignored unsafe theme name" >&2
  set_screen_shader ""
  exit $?
fi

found=0
path=""
if [[ -f $conf ]]; then
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
  if [[ -z $path ]]; then
    set_screen_shader ""
    exit $?
  fi
  if canon=$(validate_shader "$path"); then
    set_screen_shader "$canon"
    exit $?
  fi
  echo "shader-desk: ignored unsafe shader path" >&2
  set_screen_shader ""
  exit $?
fi

fallback="$shaders/${theme}.glsl"
if [[ $theme == phosphor-green ]]; then
  fallback="$shaders/phosphor.glsl"
fi
if canon=$(validate_shader "$fallback"); then
  set_screen_shader "$canon"
  exit $?
fi

set_screen_shader ""
exit $?
