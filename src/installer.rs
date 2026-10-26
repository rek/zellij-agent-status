//! First-load Claude/Grok hook install. Pure std, so it is tested on the host.
//! The wasm plugin runs the generated shell via `run_command`.

/// Marker written into the hook script and checked before rewriting.
pub const HOOK_VERSION_TAG: &str = concat!("# zellij-agent-status v", env!("CARGO_PKG_VERSION"));

/// Portable command prefix stored in `~/.claude/settings.json`.
/// Claude Code and Grok expand `${HOME}` when they run hooks.
pub const HOOK_CMD: &str = "${HOME}/.config/zellij/agent-status-hook.sh";

/// Host path the installer writes (relative to `$HOME`).
pub const HOOK_REL_PATH: &str = ".config/zellij/agent-status-hook.sh";

/// Hook script with a version tag inserted after the shebang.
pub fn hook_script_content() -> String {
    let original = include_str!("../scripts/agent-status-hook.sh");
    if let Some(pos) = original.find('\n') {
        let (shebang, rest) = original.split_at(pos);
        format!("{shebang}\n{HOOK_VERSION_TAG}{rest}")
    } else {
        original.to_string()
    }
}

/// Idempotent `sh -c` body: write the hook script, then merge Claude/Grok hooks.
pub fn install_script() -> String {
    INSTALL_TEMPLATE
        .replace("__VERSION_TAG__", HOOK_VERSION_TAG)
        .replace("__HOOK_CMD__", HOOK_CMD)
        .replace("__HOOK_REL_PATH__", HOOK_REL_PATH)
        .replace("__HOOK_SCRIPT__", &hook_script_content())
}

const INSTALL_TEMPLATE: &str = r##"set -e
HOOK_PATH="$HOME/__HOOK_REL_PATH__"
HOOK_CMD='__HOOK_CMD__'
SETTINGS="$HOME/.claude/settings.json"

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

# Resolve symlink so mv does not replace the link with a regular file
if [ -L "$SETTINGS" ]; then
  SETTINGS="$(resolve_file_symlink "$SETTINGS")"
fi

# Already current: same plugin version and our portable command is registered
if grep -qF '__VERSION_TAG__' "$HOOK_PATH" 2>/dev/null; then
  if [ -f "$SETTINGS" ] && grep -qF "$HOOK_CMD" "$SETTINGS" 2>/dev/null; then
    echo "current"
    exit 0
  fi
fi

mkdir -p "$(dirname "$HOOK_PATH")"
cat > "$HOOK_PATH" << 'AGENT_STATUS_HOOK_EOF'
__HOOK_SCRIPT__
AGENT_STATUS_HOOK_EOF
chmod +x "$HOOK_PATH"

if ! command -v jq >/dev/null 2>&1; then
  echo "no_jq"
  exit 0
fi

if [ ! -f "$SETTINGS" ]; then
  mkdir -p "$(dirname "$SETTINGS")"
  echo '{}' > "$SETTINGS"
fi

cp "$SETTINGS" "$SETTINGS.bak"

tmp=$(mktemp)
jq --arg hook "$HOOK_CMD" '
  def cmd($state):
    ($hook) + " " + $state + " # zellij-agent-status";
  def ours:
    ((.command // "")
     | contains("zellij-agent")
       or contains("agent-status-hook.sh")
       or contains("zellij pipe --name agent-status"));
  def add_hook($event; $state; $matcher):
    .hooks[$event] = (
      ((.hooks[$event] // [])
        | map(.hooks |= map(select(ours | not)))
        | map(select((.hooks | length) > 0)))
      + [{hooks: [{type: "command", command: cmd($state)}]}
         + (if $matcher == "" then {} else {matcher: $matcher} end)]);
  def add_hook($event; $state): add_hook($event; $state; "");
  .hooks = (.hooks // {})
  | add_hook("UserPromptSubmit"; "working")
  | add_hook("PostToolUse";      "working")
  | add_hook("Notification";     "waiting"; "permission_prompt|elicitation_dialog")
  | add_hook("Stop";             "stop")
  | add_hook("SessionEnd";       "end")
' "$SETTINGS" > "$tmp" && mv "$tmp" "$SETTINGS"

echo "installed"
"##;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_DIR_SEQ: AtomicU64 = AtomicU64::new(0);

    fn unique_home() -> PathBuf {
        let n = TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("agent-status-install-{nanos}-{n}"));
        fs::create_dir_all(dir.join(".claude")).unwrap();
        dir
    }

    fn run_install(home: &Path) -> String {
        let output = Command::new("sh")
            .arg("-c")
            .arg(install_script())
            .env("HOME", home)
            .output()
            .expect("sh");
        assert!(
            output.status.success(),
            "install failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn settings(home: &Path) -> Value {
        let raw = fs::read_to_string(home.join(".claude/settings.json")).unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    fn commands_for<'a>(hooks: &'a Value, event: &str) -> Vec<&'a str> {
        hooks["hooks"][event]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|g| g["hooks"].as_array().into_iter().flatten())
            .filter_map(|h| h["command"].as_str())
            .collect()
    }

    fn jq_available() -> bool {
        Command::new("jq")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn version_tag_follows_the_shebang() {
        let body = hook_script_content();
        let mut lines = body.lines();
        assert_eq!(lines.next(), Some("#!/bin/sh"));
        assert_eq!(lines.next(), Some(HOOK_VERSION_TAG));
        assert!(body.contains("zellij pipe --name agent-status"));
        assert!(body.contains("agent-status-plugin.url"));
        assert!(body.contains("shutdown|channel_closed"));
    }

    #[test]
    fn install_script_embeds_the_hook_and_events() {
        let sh = install_script();
        assert!(sh.contains(HOOK_VERSION_TAG));
        assert!(sh.contains(HOOK_CMD));
        assert!(sh.contains("UserPromptSubmit"));
        assert!(sh.contains("PostToolUse"));
        assert!(sh.contains("Notification"));
        assert!(sh.contains("permission_prompt|elicitation_dialog"));
        assert!(sh.contains("SessionEnd"));
        assert!(sh.contains(r#""stop""#));
        assert!(!sh.contains("__HOOK_SCRIPT__"));
        assert!(!sh.contains("__VERSION_TAG__"));
    }

    #[test]
    fn writes_hook_and_merges_settings() {
        if !jq_available() {
            return;
        }
        let home = unique_home();
        assert_eq!(run_install(&home), "installed");

        let hook = home.join(HOOK_REL_PATH);
        let body = fs::read_to_string(&hook).unwrap();
        assert!(body.contains(HOOK_VERSION_TAG));
        let mode = fs::metadata(&hook).unwrap().permissions();
        assert!(mode.mode() & 0o111 != 0);

        let s = settings(&home);
        let working = commands_for(&s, "UserPromptSubmit");
        assert_eq!(working.len(), 1);
        assert!(working[0].starts_with(HOOK_CMD));
        assert!(working[0].contains(" working "));

        let stop = commands_for(&s, "Stop");
        assert!(stop.iter().any(|c| c.contains(" stop ")));
        assert!(commands_for(&s, "SessionEnd")
            .iter()
            .any(|c| c.contains(" end ")));

        let notif = s["hooks"]["Notification"].as_array().unwrap();
        assert_eq!(
            notif[0]["matcher"].as_str(),
            Some("permission_prompt|elicitation_dialog")
        );
        assert!(commands_for(&s, "Notification")
            .iter()
            .any(|c| c.contains(" waiting ")));

        assert_eq!(run_install(&home), "current");
        fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn replaces_old_one_liners_and_keeps_foreign_hooks() {
        if !jq_available() {
            return;
        }
        let home = unique_home();
        let settings_path = home.join(".claude/settings.json");
        fs::write(
            &settings_path,
            r#"{
              "hooks": {
                "UserPromptSubmit": [{
                  "hooks": [
                    {"type": "command", "command": "echo keep-me"},
                    {"type": "command", "command": "timeout 2 zellij pipe --name agent-status -- \"$ZELLIJ_PANE_ID:working\""}
                  ]
                }],
                "Notification": [{
                  "matcher": "permission_prompt|elicitation_dialog",
                  "hooks": [{
                    "type": "command",
                    "command": "[ -z \"$ZELLIJ\" ] || timeout 2 zellij pipe --name agent-status -- \"$ZELLIJ_PANE_ID:waiting\""
                  }]
                }],
                "Stop": [{
                  "hooks": [{
                    "type": "command",
                    "command": "/home/you/.config/zellij/agent-status-hook.sh done # zellij-agent-status"
                  }]
                }]
              }
            }"#,
        )
        .unwrap();

        assert_eq!(run_install(&home), "installed");
        let s = settings(&home);

        let submit = commands_for(&s, "UserPromptSubmit");
        assert!(submit.contains(&"echo keep-me"));
        assert_eq!(
            submit
                .iter()
                .filter(|c| c.contains("zellij pipe --name agent-status"))
                .count(),
            0
        );
        assert_eq!(submit.iter().filter(|c| c.contains(HOOK_CMD)).count(), 1);

        let stop = commands_for(&s, "Stop");
        assert_eq!(stop.len(), 1);
        assert!(stop[0].contains(" stop "));
        assert!(!stop[0].contains(" done "));

        let notif_cmds = commands_for(&s, "Notification");
        assert_eq!(notif_cmds.len(), 1);
        assert!(notif_cmds[0].contains(HOOK_CMD));

        fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn rewrites_when_the_version_tag_changes() {
        if !jq_available() {
            return;
        }
        let home = unique_home();
        let hook = home.join(HOOK_REL_PATH);
        fs::create_dir_all(hook.parent().unwrap()).unwrap();
        fs::write(&hook, "#!/bin/sh\n# zellij-agent-status v0.0.0\nold\n").unwrap();
        fs::write(
            home.join(".claude/settings.json"),
            format!(
                r#"{{"hooks":{{"Stop":[{{"hooks":[{{"type":"command","command":"{} stop # zellij-agent-status"}}]}}]}}}}"#,
                HOOK_CMD
            ),
        )
        .unwrap();

        assert_eq!(run_install(&home), "installed");
        let body = fs::read_to_string(&hook).unwrap();
        assert!(body.contains(HOOK_VERSION_TAG));
        assert!(!body.contains("v0.0.0"));
        fs::remove_dir_all(&home).ok();
    }
}
