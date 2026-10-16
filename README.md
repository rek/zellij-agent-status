<div align="center">

# zellij-agent-status

**See which AI agent needs you, from the tab bar.**

[![ci](https://github.com/rek/zellij-agent-status/actions/workflows/ci.yml/badge.svg)](https://github.com/rek/zellij-agent-status/actions/workflows/ci.yml)
![zellij 0.45+](https://img.shields.io/badge/zellij-0.45%2B-8A2BE2)
![license MIT](https://img.shields.io/badge/license-MIT-blue)
![status alpha](https://img.shields.io/badge/status-alpha-orange)

<img src="assets/tabbar.svg" alt="Tabs with agent badges: dev ⏳, api 🔔1 ⏳2, docs ✅, notes" width="760">

</div>

Run Claude Code (or any agent) in several panes across many tabs, and you lose
track of which one is stuck on a permission prompt. This plugin puts each
agent's state on its tab. Zellij pushes tab and pane events to the plugin, so
there is no polling, no state files and no shelling out.

> **Status: alpha.** In daily use on zellij 0.45.1 with real Claude Code hooks:
> per-pane states, tab summaries, notifications-on-unfocused, the jump and
> focus-clear all work. Expect rough edges and please open issues.

## What you get

- **Per-pane state, per-tab summary.** 1–4 agents in one tab: `api 🔔1 ⏳2`. A
  lone agent gets no count: `dev ⏳`.
- **Alerts clear when you look.** Focusing the pane acknowledges it, not just
  the tab.
- **Jump key.** One keypress focuses the next waiting pane (then finished
  ones), wrapping across tabs.
- **Desktop notifications** (`notify-send`) for waiting and done, skipped for
  the pane you are already looking at.
- **Your tab names stay yours.** A manual rename becomes the new base name, and
  repainting pauses while you type one.
- **Stable tab ids.** Reordering tabs can't mislabel them.
- **Agent-agnostic.** The protocol is one `zellij pipe` line. Claude Code hooks
  are the first-class example.

## Install

Download the prebuilt plugin from the latest release:

```sh
mkdir -p ~/.config/zellij/plugins
curl -fL -o ~/.config/zellij/plugins/zellij-agent-status.wasm \
  https://github.com/rek/zellij-agent-status/releases/latest/download/zellij-agent-status.wasm
```

Or build it yourself: `make install` (needs the `wasm32-wasip1` Rust target).

Load it in the background (`~/.config/zellij/config.kdl`). Zellij does not
expand `~` in some plugin paths, so use an absolute path if it fails to load:

```kdl
load_plugins {
    "file:/home/you/.config/zellij/plugins/zellij-agent-status.wasm"
}
```

Bind the jump key (inside `shared_except "locked"`):

```kdl
bind "Alt a" {
    MessagePlugin "file:/home/you/.config/zellij/plugins/zellij-agent-status.wasm" {
        name "agent-status-jump"
    }
}
```

> **Keep the two blocks' configuration identical.** Zellij finds the running
> instance by URL *and* configuration. If they differ, the keybind starts a
> second instance, and the two fight over tab names.

### Options

All optional. If you set any, put the same lines in both blocks above.

| Key | Default | Meaning |
|---|---|---|
| `notify` | `"waiting done"` | states that raise a desktop notification (`""` for none) |
| `waiting_icon` | `"🔔"` | badge for an agent waiting on you |
| `working_icon` | `"⏳"` | badge for an agent at work |
| `done_icon` | `"✅"` | badge for an agent that finished |

Approve the permission prompt on first load. It asks for: read application
state (tabs and panes), change application state (rename tabs, focus panes),
run commands (`notify-send`) and read CLI pipes (to release hook pipes).

## Wire up Claude Code

Add hooks to `~/.claude/settings.json`. Each one runs the same command with a
different state:

| Hook | Matcher | State |
|---|---|---|
| `UserPromptSubmit`, `PostToolUse` | | `working` |
| `Notification` | `permission_prompt\|elicitation_dialog` | `waiting` |
| `Stop` | | `done` |
| `SessionEnd` | | `end` |

The `Notification` matcher matters. Without it, Claude Code's "idle for a
while" reminder turns a finished ✅ back into 🔔 and sends a second toast.

```json
"Notification": [{
  "matcher": "permission_prompt|elicitation_dialog",
  "hooks": [{
    "type": "command",
    "command": "[ -z \"$ZELLIJ\" ] || timeout 2 zellij pipe --name agent-status -- \"$ZELLIJ_PANE_ID:waiting\" </dev/null >/dev/null 2>&1"
  }]
}]
```

The other hooks use the same command with their state. The guard makes it a
no-op outside zellij. `timeout` and the closed stdin mean a missing plugin can
never hang the agent.

No `--plugin` flag on purpose: it broadcasts to running plugins, so it reaches
the instance started by `load_plugins`. With `--plugin file:…` zellij matches
on the plugin's configuration too, and tries to start a second instance.

A hook-installer subcommand is on the roadmap.

## Protocol

| Pipe name | Payload | Effect |
|---|---|---|
| `agent-status` | `<pane_id>:<working\|waiting\|done\|idle\|end>` | set a pane's state |
| `agent-status-jump` | none | focus the next waiting (then done) pane |

From a shell, close stdin for the jump (`zellij pipe --name agent-status-jump </dev/null`),
or `zellij pipe` waits for input. The keybind does not have that problem.

`<pane_id>` is `$ZELLIJ_PANE_ID`; `terminal_7` is accepted too.

## How it works

`src/core.rs` is pure logic (state, label composition, base-name memory, jump
order) with no zellij types, so it is tested on the host. `src/plugin.rs` is the
thin zellij layer. It tracks panes from `PaneUpdate`, tabs from `TabUpdate`,
and repaints names with `rename_tab_with_id`.

```sh
make test      # core unit tests
make build     # wasm32-wasip1 release build
```

## Prior art

Built after surveying what exists, borrowing the good ideas:
[zellij-claude-status](https://github.com/KeAiMianYang/zellij-claude-status)
(per-pane icons, rename deferral),
[zellij-attention](https://github.com/KiryuuLight/zellij-attention) (pipe
protocol, focus-clear),
[zellaude](https://github.com/ishefi/zellaude) and
[zj-radar](https://github.com/marktoda/zj-radar) (richer UIs that replace the
bar or add a sidebar). This plugin is the small one: it only touches tab names,
and adds the jump key and per-pane focus clearing.

## Roadmap

- [x] Live-session verification of states, labels, jump and focus-clear
- [x] Real Claude Code hooks
- [x] Tagged releases with prebuilt wasm
- [ ] Hook-installer subcommand
- [ ] Test seam over the zellij calls so `plugin.rs` is covered
- [ ] Optional own tab-bar rendering (badges without renaming)

## Lessons from the first live run

- Zellij runs plugins as programs, so the crate is a **binary** (`main`), not a `cdylib`.
- A CLI pipe blocks its caller until the plugin releases it (`unblock_cli_pipe_input`,
  by pipe id, needs the `ReadCliPipes` permission). A hook would otherwise hang.
- Rename events land late, so zellij can report an older label; the plugin
  remembers every label it painted and never mistakes one for a manual rename.
- When a tab becomes active, zellij briefly reports the *previous* focused pane
  there. A jump claims its destination so it cannot acknowledge the wrong pane.
- Pane updates can arrive before tab updates, so the pane-to-tab map is rebuilt on both.
- A `MessagePlugin` keybind whose configuration differs from `load_plugins` starts a
  second instance. Each took the other's label for a manual rename and added a badge,
  forever. Since 0.1.1 the base name drops any trailing badges in the plugin's own
  icons, so instances (and leftovers from a restart) converge instead.
- Zellij reports a tab's name as empty for a moment during a rename. An empty or
  badge-only name is never adopted as the base; the previous base is kept.

## Open questions

- Hook latency of `zellij pipe` on busy sessions.
- Behavior with several clients attached to one session.

## License

MIT
