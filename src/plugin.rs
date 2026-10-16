use std::collections::{BTreeMap, HashMap, HashSet};

use zellij_tile::prelude::*;

use crate::core::*;

/// `zellij pipe --name agent-status -- "<pane_id>:<state>"`
const PIPE_STATE: &str = "agent-status";
/// `zellij pipe --name agent-status-jump` (bind to a key with MessagePlugin)
const PIPE_JUMP: &str = "agent-status-jump";

#[derive(Default)]
struct State {
    tracker: Tracker,
    names: TabNames,
    icons: Icons,
    notify_on: HashSet<AgentState>,
    permitted: bool,
    renaming: bool,
    tabs: Vec<TabInfo>,
    /// terminal pane id -> tab id
    pane_tab: HashMap<PaneNum, TabId>,
    /// every terminal pane in (tab position, pane id) order
    order: Vec<(TabId, PaneNum)>,
    /// focused terminal pane per tab id
    focused: HashMap<TabId, PaneNum>,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, config: BTreeMap<String, String>) {
        let icon = |key: &str, default: &str| config.get(key).cloned().unwrap_or(default.into());
        self.icons = Icons {
            waiting: icon("waiting_icon", "🔔"),
            working: icon("working_icon", "⏳"),
            done: icon("done_icon", "✅"),
        };
        let notify = config.get("notify").map_or("waiting done", String::as_str);
        self.notify_on = notify.split_whitespace().filter_map(AgentState::parse).collect();

        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::RunCommands,
        ]);
        subscribe(&[
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::ModeUpdate,
            EventType::PermissionRequestResult,
        ]);
        set_selectable(false);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                self.permitted = true;
                self.sync();
            }
            Event::ModeUpdate(info) => {
                let was = self.renaming;
                self.renaming = matches!(info.mode, InputMode::RenameTab | InputMode::RenamePane);
                if was && !self.renaming {
                    self.sync();
                }
            }
            Event::TabUpdate(tabs) => {
                self.tabs = tabs;
                self.sync();
            }
            Event::PaneUpdate(manifest) => {
                self.ingest_panes(&manifest);
                self.sync();
            }
            _ => {}
        }
        false
    }

    fn pipe(&mut self, msg: PipeMessage) -> bool {
        match msg.name.as_str() {
            PIPE_JUMP => self.jump(),
            PIPE_STATE => {
                if let Some(cmd) = msg.payload.as_deref().and_then(parse_command) {
                    self.apply(cmd);
                }
            }
            _ => {}
        }
        false
    }
}

impl State {
    fn ingest_panes(&mut self, manifest: &PaneManifest) {
        let tab_by_pos: HashMap<usize, TabId> =
            self.tabs.iter().map(|t| (t.position, t.tab_id)).collect();
        self.pane_tab.clear();
        self.order.clear();
        self.focused.clear();
        let mut positions: Vec<_> = manifest.panes.keys().copied().collect();
        positions.sort();
        for pos in positions {
            let Some(&tab) = tab_by_pos.get(&pos) else { continue };
            let mut panes: Vec<&PaneInfo> = manifest.panes[&pos]
                .iter()
                .filter(|p| !p.is_plugin && !p.is_suppressed)
                .collect();
            panes.sort_by_key(|p| p.id);
            for p in panes {
                self.pane_tab.insert(p.id, tab);
                self.order.push((tab, p.id));
                if p.is_focused {
                    self.focused.insert(tab, p.id);
                }
            }
        }
    }

    fn active_tab(&self) -> Option<TabId> {
        self.tabs.iter().find(|t| t.active).map(|t| t.tab_id)
    }

    fn visible_pane(&self) -> Option<PaneNum> {
        self.active_tab().and_then(|t| self.focused.get(&t).copied())
    }

    fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::End(p) => {
                self.tracker.remove(p);
            }
            Command::Set(p, s) => {
                let changed = self.tracker.set(p, s);
                // you are already looking at it: no alert, no toast
                if changed && self.visible_pane() == Some(p) && s.is_alert() {
                    self.tracker.acknowledge(p);
                } else if changed && self.notify_on.contains(&s) {
                    self.notify(p, s);
                }
            }
        }
        self.sync();
    }

    fn notify(&self, pane: PaneNum, state: AgentState) {
        let tab = self
            .pane_tab
            .get(&pane)
            .and_then(|t| self.tabs.iter().find(|x| x.tab_id == *t))
            .map(|t| self.names.base_for(t.tab_id, &t.name))
            .unwrap_or_else(|| "tab".into());
        let body = if state == AgentState::Waiting { "needs your input" } else { "finished" };
        run_command(
            &["notify-send", "-a", "zellij", &format!("Agent: {tab}"), body],
            BTreeMap::new(),
        );
    }

    fn jump(&self) {
        let current = self
            .visible_pane()
            .and_then(|p| self.pane_tab.get(&p).map(|t| (*t, p)));
        if let Some((_, pane)) = jump_target(&self.order, &self.tracker, current) {
            focus_pane_with_id(PaneId::Terminal(pane), false, false);
        }
    }

    /// Reconcile tracker with reality, then repaint tab names.
    fn sync(&mut self) {
        if !self.permitted {
            return;
        }
        let alive: HashSet<PaneNum> = self.pane_tab.keys().copied().collect();
        if !alive.is_empty() {
            self.tracker.retain_alive(&alive);
        }
        if let Some(p) = self.visible_pane() {
            self.tracker.acknowledge(p);
        }
        if self.renaming {
            return; // don't clobber a rename in progress
        }
        for tab in &self.tabs {
            let states = self
                .tracker
                .iter()
                .filter(|(p, _)| self.pane_tab.get(p) == Some(&tab.tab_id))
                .map(|(_, s)| s);
            let counts = count(states);
            let base = self.names.base_for(tab.tab_id, &tab.name);
            let label = compose_label(&base, &counts, &self.icons);
            if label != tab.name {
                rename_tab_with_id(tab.tab_id as u64, &label);
            }
            self.names.painted(tab.tab_id, &base, &label);
        }
    }
}
