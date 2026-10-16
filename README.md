# zellij-agent-status

See which AI agent needs you, from the tab bar. Each agent pane reports
`working` / `waiting` / `done`; every tab shows a summary of its panes, e.g.
`dev 🔔1 ⏳2`. Built for Claude Code, driven by plain `zellij pipe` so any
agent can use it.

**Status: prep.** The pure core (state, labels, jump order) is unit-tested and
the plugin compiles to wasm. Not yet run inside a live session.

## Features

| | |
|---|---|
| Per-pane state, per-tab summary | 1–4 agents in a tab; counts only when more than one |
| Alerts clear on focus | acknowledged the moment you look at that pane, not just the tab |
| Jump key | `agent-status-jump` focuses the next waiting pane (then done), wrapping across tabs |
| Desktop notification | `notify-send` for waiting/done on panes you are not looking at |
| Keeps your tab names | remembers the base name; a manual rename becomes the new base |
| Rename-safe | pauses repainting while you are in rename mode |
| No polling, no files | state in memory, driven by zellij's tab/pane events (no `list-panes` calls) |
| Uses stable tab ids | `rename_tab_with_id`, so tab reordering cannot mislabel |

Borrowed from existing plugins: per-pane icons and rename deferral
(zellij-claude-status), focus-clear and the pipe protocol (zellij-attention),
the click/jump idea (zellaude, zj-radar).

## Protocol

```sh
zellij pipe --plugin file:~/.config/zellij/plugins/zellij-agent-status.wasm \
  --name agent-status -- "$ZELLIJ_PANE_ID:working"   # working|waiting|done|idle|end
```

Jump key (`config.kdl`):

```kdl
bind "Alt a" {
  MessagePlugin "file:~/.config/zellij/plugins/zellij-agent-status.wasm" {
    name "agent-status-jump"
  }
}
```

Load in the background:

```kdl
load_plugins {
  "file:~/.config/zellij/plugins/zellij-agent-status.wasm" {
    notify "waiting done"      // which states raise a desktop notification
    waiting_icon "🔔"
    working_icon "⏳"
    done_icon "✅"
  }
}
```

## Layout

- `src/core.rs` — pure logic + tests (`make test`)
- `src/plugin.rs` — zellij glue (wasm only)
- `make build` / `make install`

## Roadmap

1. Run it in a live session, replace `devconfig/bin/zellij-agent` with it.
2. `claude-hooks` installer subcommand (merge hooks into `~/.claude/settings.json`).
3. Tests for `plugin.rs` logic via a small trait seam over the zellij calls.
4. Optional: own tab-bar rendering (no renames), elapsed time, idle-since.
5. Release `.wasm` via GitHub Actions; document the zellij version matrix.

## Open questions

- Does `zellij pipe --plugin <file:…>` reach the instance started by
  `load_plugins`, or spawn a second one? (zellij-claude-status relies on it working.)
- `PaneUpdate` fires on focus changes, but does not fire for title-only changes
  (a known 0.45.1 limitation); fine here because state comes from hooks.
- Hook latency: `zellij pipe` startup vs. the current bash script.
