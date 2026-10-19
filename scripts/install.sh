#!/bin/sh
# One-liner install / update for zellij-agent-status.
#
#   curl -fsSL https://github.com/rek/zellij-agent-status/releases/latest/download/install.sh | sh
#   curl -fsSL https://github.com/rek/zellij-agent-status/releases/latest/download/install.sh | sh -s -- --update
set -eu

WASM_URL='https://github.com/rek/zellij-agent-status/releases/latest/download/zellij-agent-status.wasm'
CONFIG="${ZELLIJ_CONFIG:-$HOME/.config/zellij/config.kdl}"
MODE=install

usage() {
  echo "usage: $0 [--update] [--config PATH] [--print]" >&2
  exit 2
}

while [ $# -gt 0 ]; do
  case "$1" in
    --update) MODE=update ;;
    --print) MODE=print ;;
    --config)
      [ $# -ge 2 ] || usage
      CONFIG=$2
      shift
      ;;
    -h|--help) usage ;;
    *) usage ;;
  esac
  shift
done

resolve_file_symlink() {
  path=$1
  while [ -L "$path" ]; do
    dir=$(cd "$(dirname "$path")" && pwd -P)
    target=$(readlink "$path")
    case "$target" in
      /*) path=$target ;;
      *) path=$dir/$target ;;
    esac
  done
  dir=$(cd "$(dirname "$path")" && pwd -P)
  printf '%s/%s\n' "$dir" "$(basename "$path")"
}

print_kdl() {
  cat <<EOF
plugins {
    agent-status location="$WASM_URL"
}
load_plugins {
    agent-status
}
EOF
}

print_jump() {
  cat <<'EOF'
bind "Alt a" {
    MessagePlugin "agent-status" {
        name "agent-status-jump"
    }
}
EOF
}

if [ "$MODE" = print ]; then
  print_kdl
  echo
  echo "// jump key, inside shared_except \"locked\":"
  print_jump
  exit 0
fi

if [ "$MODE" = update ]; then
  if [ -d "$HOME/.cache/zellij" ]; then
    find "$HOME/.cache/zellij" -name '*zellij-agent-status*' -prune -exec rm -rf {} + 2>/dev/null || true
  fi
  if [ -n "${ZELLIJ:-}" ]; then
    # Reload the aliased instance. Same URL + config as load_plugins, so this
    # does not start a second copy.
    zellij action start-or-reload-plugin agent-status </dev/null >/dev/null 2>&1 || \
      zellij action start-or-reload-plugin "$WASM_URL" </dev/null >/dev/null 2>&1 || true
    echo "Updated agent-status. New hooks apply after /hooks in a running agent."
  else
    echo "Cleared the agent-status plugin cache. Start a new zellij session to load the latest wasm."
  fi
  exit 0
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 is required to edit $CONFIG." >&2
  echo "Paste this into that file, then start a new zellij session:" >&2
  print_kdl
  exit 1
fi

if [ -L "$CONFIG" ]; then
  CONFIG="$(resolve_file_symlink "$CONFIG")"
fi

if [ ! -f "$CONFIG" ]; then
  echo "No $CONFIG yet. Start zellij once so it writes a config, then re-run." >&2
  echo "Or paste this in yourself:" >&2
  print_kdl
  exit 1
fi

python3 - "$CONFIG" "$WASM_URL" <<'PY'
import pathlib, re, sys

path = pathlib.Path(sys.argv[1])
wasm = sys.argv[2]
alias_line = f'    agent-status location="{wasm}"'
text = path.read_text()
original = text

def block_span(src, name):
    m = re.search(r'(?m)^[ \t]*' + re.escape(name) + r'[ \t]*\{', src)
    if not m:
        return None
    i = src.find('{', m.start())
    depth = 0
    for j, ch in enumerate(src[i:], i):
        if ch == '{':
            depth += 1
        elif ch == '}':
            depth -= 1
            if depth == 0:
                return m.start(), j + 1
    return None

def insert_before_close(src, span, line):
    start, end = span
    block = src[start:end]
    if re.search(r'(?m)^[ \t]*' + re.escape(line.strip()), block):
        return src, False
    # already an agent-status node: replace that line (keeps file: → https)
    replaced, n = re.subn(
        r'(?m)^[ \t]*agent-status\b.*$',
        line,
        block,
        count=1,
    )
    if n:
        return src[:start] + replaced + src[end:], True
    close = block.rfind('}')
    inner = block[:close].rstrip()
    nl = '\n' if inner.endswith('\n') else '\n'
    new_block = inner + nl + line + '\n}'
    return src[:start] + new_block + src[end:], True

plugins = block_span(text, 'plugins')
if plugins:
    text, _ = insert_before_close(text, plugins, alias_line)
else:
    text = text.rstrip() + '\n\nplugins {\n' + alias_line + '\n}\n'

loads = block_span(text, 'load_plugins')
load_line = '    agent-status'
if loads:
    start, end = loads
    block = text[start:end]
    if re.search(r'(?m)^[ \t]*"?file:.*zellij-agent-status\.wasm"?', block):
        block, _ = re.subn(
            r'(?m)^[ \t]*"?file:.*zellij-agent-status\.wasm"?[ \t]*$',
            load_line,
            block,
            count=1,
        )
        text = text[:start] + block + text[end:]
        loads = block_span(text, 'load_plugins')
    if loads and not re.search(r'(?m)^[ \t]*agent-status\b', text[loads[0]:loads[1]]):
        text, _ = insert_before_close(text, loads, load_line)
else:
    text = text.rstrip() + '\n\nload_plugins {\n' + load_line + '\n}\n'

if text != original:
    path.write_text(text)
    print(f'updated {path}')
else:
    print(f'{path} already has agent-status')
PY

echo
echo "Start a new zellij session. On first load, approve permissions; the plugin"
echo "writes Claude/Grok hooks into ~/.claude/settings.json (needs jq)."
echo "Existing agent sessions: /hooks"
echo
echo "Optional jump key, inside shared_except \"locked\":"
print_jump
