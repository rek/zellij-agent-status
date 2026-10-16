//! Pure agent-state logic. No zellij types here, so it is unit-testable on the host.

use std::collections::{BTreeMap, HashMap, HashSet};

pub type PaneNum = u32;
pub type TabId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AgentState {
    Working,
    Waiting,
    Done,
    Idle,
}

impl AgentState {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "working" => Some(Self::Working),
            "waiting" => Some(Self::Waiting),
            "done" | "completed" => Some(Self::Done),
            "idle" | "init" => Some(Self::Idle),
            _ => None,
        }
    }

    /// States that ask for the user's attention and clear once the pane is focused.
    pub fn is_alert(self) -> bool {
        matches!(self, Self::Waiting | Self::Done)
    }
}

/// What a hook sends: `<pane_id>:<state>` or `<pane_id>:end`.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Set(PaneNum, AgentState),
    End(PaneNum),
}

pub fn parse_command(payload: &str) -> Option<Command> {
    let (pane, word) = payload.split_once(':')?;
    let pane: PaneNum = pane.trim().trim_start_matches("terminal_").parse().ok()?;
    if word.trim() == "end" || word.trim() == "cleanup" {
        return Some(Command::End(pane));
    }
    AgentState::parse(word).map(|s| Command::Set(pane, s))
}

#[derive(Default)]
pub struct Tracker {
    panes: BTreeMap<PaneNum, AgentState>,
}

impl Tracker {
    /// Returns true if the state actually changed.
    pub fn set(&mut self, pane: PaneNum, state: AgentState) -> bool {
        self.panes.insert(pane, state) != Some(state)
    }

    pub fn remove(&mut self, pane: PaneNum) -> bool {
        self.panes.remove(&pane).is_some()
    }

    pub fn get(&self, pane: PaneNum) -> Option<AgentState> {
        self.panes.get(&pane).copied()
    }

    /// Drop panes that no longer exist. Returns true if anything was removed.
    pub fn retain_alive(&mut self, alive: &HashSet<PaneNum>) -> bool {
        let before = self.panes.len();
        self.panes.retain(|p, _| alive.contains(p));
        self.panes.len() != before
    }

    /// The user is looking at this pane: waiting/done alerts are acknowledged.
    pub fn acknowledge(&mut self, pane: PaneNum) -> bool {
        match self.panes.get(&pane) {
            Some(s) if s.is_alert() => {
                self.panes.remove(&pane);
                true
            }
            _ => false,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (PaneNum, AgentState)> + '_ {
        self.panes.iter().map(|(p, s)| (*p, *s))
    }
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Counts {
    pub waiting: usize,
    pub working: usize,
    pub done: usize,
    pub total: usize,
}

pub fn count(states: impl Iterator<Item = AgentState>) -> Counts {
    let mut c = Counts::default();
    for s in states {
        match s {
            AgentState::Waiting => c.waiting += 1,
            AgentState::Working => c.working += 1,
            AgentState::Done => c.done += 1,
            AgentState::Idle => {}
        }
        c.total += 1;
    }
    c
}

pub struct Icons {
    pub waiting: String,
    pub working: String,
    pub done: String,
}

impl Default for Icons {
    fn default() -> Self {
        Self { waiting: "🔔".into(), working: "⏳".into(), done: "✅".into() }
    }
}

fn badge(icon: &str, n: usize, total: usize) -> String {
    match (n, total) {
        (0, _) => String::new(),
        (_, t) if t > 1 => format!(" {icon}{n}"),
        _ => format!(" {icon}"),
    }
}

/// `dev 🔔1 ⏳2`; a lone agent gets no count: `dev ⏳`.
pub fn compose_label(base: &str, c: &Counts, icons: &Icons) -> String {
    format!(
        "{base}{}{}{}",
        badge(&icons.waiting, c.waiting, c.total),
        badge(&icons.working, c.working, c.total),
        badge(&icons.done, c.done, c.total),
    )
}

/// Remembers what name the user gave a tab versus what we painted on it, so
/// a manual rename is adopted as the new base and badges never stack.
#[derive(Default)]
pub struct TabNames {
    painted: HashMap<TabId, (String, String)>, // tab -> (base, label)
}

impl TabNames {
    pub fn base_for(&self, tab: TabId, current: &str) -> String {
        match self.painted.get(&tab) {
            Some((base, label)) if label == current => base.clone(),
            _ => current.to_string(),
        }
    }

    pub fn painted(&mut self, tab: TabId, base: &str, label: &str) {
        if base == label {
            self.painted.remove(&tab);
        } else {
            self.painted.insert(tab, (base.to_string(), label.to_string()));
        }
    }
}

/// Next pane to jump to: waiting first, then done; after `current` in
/// (tab position, pane id) order, wrapping. `order` is the full pane order.
pub fn jump_target(
    order: &[(TabId, PaneNum)],
    tracker: &Tracker,
    current: Option<(TabId, PaneNum)>,
) -> Option<(TabId, PaneNum)> {
    for want in [AgentState::Waiting, AgentState::Done] {
        let hits: Vec<(TabId, PaneNum)> = order
            .iter()
            .copied()
            .filter(|(_, p)| tracker.get(*p) == Some(want))
            .collect();
        if hits.is_empty() {
            continue;
        }
        let start = current
            .and_then(|c| order.iter().position(|x| *x == c))
            .map_or(0, |i| i + 1);
        let after = hits
            .iter()
            .find(|h| order.iter().position(|x| x == *h).is_some_and(|i| i >= start));
        return after.or(hits.first()).copied();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        assert_eq!(parse_command("3:waiting"), Some(Command::Set(3, AgentState::Waiting)));
        assert_eq!(parse_command("terminal_7:completed"), Some(Command::Set(7, AgentState::Done)));
        assert_eq!(parse_command("3:end"), Some(Command::End(3)));
        assert_eq!(parse_command("x:working"), None);
        assert_eq!(parse_command("3:bogus"), None);
    }

    #[test]
    fn label_single_and_multi() {
        let icons = Icons::default();
        let one = count([AgentState::Working].into_iter());
        assert_eq!(compose_label("dev", &one, &icons), "dev ⏳");
        let many = count([AgentState::Waiting, AgentState::Working, AgentState::Working].into_iter());
        assert_eq!(compose_label("dev", &many, &icons), "dev 🔔1 ⏳2");
        assert_eq!(compose_label("dev", &count(std::iter::empty()), &icons), "dev");
    }

    #[test]
    fn idle_agents_add_no_badge_but_count_toward_total() {
        let c = count([AgentState::Idle, AgentState::Waiting].into_iter());
        assert_eq!(compose_label("t", &c, &Icons::default()), "t 🔔1");
    }

    #[test]
    fn acknowledge_clears_alerts_not_work() {
        let mut t = Tracker::default();
        t.set(1, AgentState::Waiting);
        t.set(2, AgentState::Working);
        assert!(t.acknowledge(1));
        assert!(!t.acknowledge(2));
        assert_eq!(t.get(1), None);
        assert_eq!(t.get(2), Some(AgentState::Working));
    }

    #[test]
    fn manual_rename_becomes_new_base() {
        let mut n = TabNames::default();
        n.painted(0, "dev", "dev 🔔");
        assert_eq!(n.base_for(0, "dev 🔔"), "dev");
        assert_eq!(n.base_for(0, "api"), "api");
    }

    #[test]
    fn jump_prefers_waiting_then_wraps() {
        let order = [(0, 1), (0, 2), (1, 3), (2, 4)];
        let mut t = Tracker::default();
        t.set(2, AgentState::Waiting);
        t.set(4, AgentState::Waiting);
        t.set(3, AgentState::Done);
        assert_eq!(jump_target(&order, &t, Some((0, 1))), Some((0, 2)));
        assert_eq!(jump_target(&order, &t, Some((0, 2))), Some((2, 4)));
        assert_eq!(jump_target(&order, &t, Some((2, 4))), Some((0, 2)));
        t.remove(2);
        t.remove(4);
        assert_eq!(jump_target(&order, &t, None), Some((1, 3)));
        t.remove(3);
        assert_eq!(jump_target(&order, &t, None), None);
    }
}
