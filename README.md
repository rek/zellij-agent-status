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

> **Status: alpha.** The core logic is unit-tested and the plugin compiles to
> wasm, but it has not yet had a full run in a live session. Expect rough edges
> and please open issues.

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

```sh
git clone https://github.com/rek/zellij-agent-status
cd zellij-agent-status
make install          # builds and copies the wasm to ~/.config/zellij/plugins/
```

Prebuilt `.wasm` files will be attached to tagged releases.

Load it in the background (`~/.config/zellij/config.kdl`; zellij does not
expand `~` in some plugin paths, so use an absolute path if it fails to load):

```kdl
load_plugins {
    "file:~/.config/zellij/plugins/zellij-agent-status.wasm" {
        notify "waiting done"   // states that raise a desktop notification
        waiting_icon "🔔"
        working_icon "⏳"
        done_icon "✅"
    }
}
```

Bind the jump key (inside `shared_except "locked"`):

```kdl
bind "Alt a" {
    MessagePlugin "file:~/.config/zellij/plugins/zellij-agent-status.wasm" {
        name "agent-status-jump"
    }
}
```

Approve the permission prompt on first load (read and change application
state, run commands for notifications).

## Wire up Claude Code

Add hooks to `~/.claude/settings.json`. Each one runs the same command with a
different state:

| Hook | State |
|---|---|
| `UserPromptSubmit`, `PostToolUse` | `working` |
| `Notification` | `waiting` |
| `Stop` | `done` |
| `SessionEnd` | `end` |

```sh
zellij pipe --plugin "file:$HOME/.config/zellij/plugins/zellij-agent-status.wasm" \
  --name agent-status -- "$ZELLIJ_PANE_ID:waiting"
```

A hook-installer subcommand is on the roadmap.

## Protocol

| Pipe name | Payload | Effect |
|---|---|---|
| `agent-status` | `<pane_id>:<working\|waiting\|done\|idle\|end>` | set a pane's state |
| `agent-status-jump` | none | focus the next waiting (then done) pane |

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

- [ ] Live-session verification, and a comparison with the shell-script version
- [ ] Hook-installer subcommand
- [ ] Test seam over the zellij calls so `plugin.rs` is covered
- [ ] Optional own tab-bar rendering (badges without renaming)
- [ ] Tagged releases with prebuilt wasm

## Open questions

- Whether `zellij pipe --plugin file:…` reaches the instance started by
  `load_plugins` or starts a second one.
- Hook latency of `zellij pipe` startup on busy sessions.

## License

MIT
