#!/bin/sh
# Drive zellij-agent-status (tab badges) and Zellij's own inbox (OSC 7501)
# from Hermes Agent shell hooks.
#
# Hermes pipes one JSON payload per event on stdin. Register this script for
# pre_llm_call, on_human_input_request, on_human_input_resolved,
# on_session_end and on_session_finalize (see the README's "Hermes" section).
#
# Hermes often runs its agent in a gateway process outside the pane you type
# in, so ZELLIJ_PANE_ID is usually not in this script's environment. The pane
# is then found from Hermes' terminal breadcrumb for the session (the tty the
# CLI runs on) and the Zellij variables of a process on that tty. A session
# with no breadcrumb (a subagent, cron, a chat platform) is not in a pane:
# the script does nothing.
#
# Best-effort and quiet: it never prints to stdout (which Hermes would read as
# a directive), and every failure exits 0.
set -u

payload=$(cat 2>/dev/null) || exit 0
command -v jq >/dev/null 2>&1 || exit 0

field() {
  printf '%s' "$payload" | jq -r "$1 // empty" 2>/dev/null
}

event=$(field '.hook_event_name')
case "$event" in
  pre_llm_call | on_human_input_resolved)
    state=working
    osc="state=working"
    ;;
  on_human_input_request)
    state=waiting
    case "$(field '.extra.kind')" in
      sudo) osc="state=blocked:kind=auth" ;;
      clarify | elicitation) osc="state=blocked:kind=question" ;;
      *) osc="state=blocked:kind=permission" ;;
    esac
    ;;
  on_session_end)
    state=done
    if [ "$(field '.extra.failed')" = true ]; then
      osc="state=error"
    elif [ "$(field '.extra.interrupted')" = true ]; then
      osc="state=idle"
    else
      osc="state=done"
    fi
    ;;
  on_session_finalize)
    state=end
    osc="state=clear"
    ;;
  *) exit 0 ;;
esac

pane=${ZELLIJ_PANE_ID:-}
zsession=${ZELLIJ_SESSION_NAME:-}
zellij_var=${ZELLIJ:-}
tty=""

if [ -z "$pane" ]; then
  sid=$(field '.session_id')
  [ -n "$sid" ] || exit 0
  case "$sid" in *[!A-Za-z0-9._-]*) exit 0 ;; esac

  # Resolved pane per session, so the common case costs no /proc walk.
  cache_dir="${XDG_RUNTIME_DIR:-/tmp}/zellij-agent-status"
  cache="$cache_dir/hermes-$sid"
  if [ -r "$cache" ]; then
    # shellcheck disable=SC1090
    . "$cache"
  fi

  if [ -z "$pane" ] || [ ! -e "$tty" ]; then
    pane="" zsession="" zellij_var="" tty=""
    crumbs="${HERMES_HOME:-$HOME/.hermes}/terminal-sessions"
    crumb=$(grep -lF "\"session_id\": \"$sid\"" "$crumbs"/tty-* 2>/dev/null | head -n 1)
    [ -n "$crumb" ] || exit 0
    # tty-dev-pts-5 -> /dev/pts/5
    tty="/$(basename "$crumb" | sed 's/^tty-//; s#-#/#g')"
    [ -e "$tty" ] || exit 0
    for pid in $(ps -t "${tty#/dev/}" -o pid= 2>/dev/null); do
      env=$(tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null) || continue
      pane=$(printf '%s\n' "$env" | sed -n 's/^ZELLIJ_PANE_ID=//p' | head -n 1)
      [ -n "$pane" ] || continue
      zsession=$(printf '%s\n' "$env" | sed -n 's/^ZELLIJ_SESSION_NAME=//p' | head -n 1)
      zellij_var=$(printf '%s\n' "$env" | sed -n 's/^ZELLIJ=//p' | head -n 1)
      break
    done
    [ -n "$pane" ] || exit 0
    case "$pane$zsession" in *[!A-Za-z0-9._-]*) exit 0 ;; esac
    mkdir -p "$cache_dir" 2>/dev/null &&
      printf "pane='%s'\nzsession='%s'\nzellij_var='%s'\ntty='%s'\n" \
        "$pane" "$zsession" "${zellij_var:-0}" "$tty" > "$cache" 2>/dev/null
  fi
fi

[ -n "$pane" ] || exit 0

# Zellij's inbox (0.46+): OSC 7501 written to the pane's tty. One short write;
# set AGENT_STATUS_OSC7501=0 to leave the tty alone.
if [ -n "$tty" ] && [ "${AGENT_STATUS_OSC7501:-1}" != 0 ]; then
  printf '\033]7501;%s:app=hermes\033\\' "$osc" > "$tty" 2>/dev/null || true
fi

# Tab badges: the shared hook script pipes "<pane>:<state>" to the plugin.
hook="$HOME/.config/zellij/agent-status-hook.sh"
[ -x "$hook" ] || exit 0
[ "$state" = end ] && [ -n "${cache:-}" ] && rm -f "$cache" 2>/dev/null
ZELLIJ=${zellij_var:-0} ZELLIJ_PANE_ID=$pane ZELLIJ_SESSION_NAME=$zsession \
  exec "$hook" "$state" < /dev/null > /dev/null 2>&1
