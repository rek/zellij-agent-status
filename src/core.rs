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

/// Grok fires Stop a second time when the session tears down (`/exit`).
/// Those reasons clear the pane; they are not a completed turn.
pub fn stop_reason_is_teardown(reason: &str) -> bool {
    matches!(reason.trim(), "shutdown" | "channel_closed")
}

pub fn parse_command(payload: &str) -> Option<Command> {
    let (pane, word) = payload.split_once(':')?;
    let pane: PaneNum = pane.trim().trim_start_matches("terminal_").parse().ok()?;
    match word.trim() {
        "end" | "cleanup" | "exit" | "shutdown" | "channel_closed" => Some(Command::End(pane)),
        other => AgentState::parse(other).map(|s| Command::Set(pane, s)),
    }
}

#[derive(Default)]
pub struct Tracker {
    panes: BTreeMap<PaneNum, AgentState>,
    /// Panes that received `end`. A later `done` from Grok's teardown Stop
    /// must not resurrect the ✅ after SessionEnd already cleared it.
    ended: HashSet<PaneNum>,
}

impl Tracker {
    /// Returns true if the state actually changed.
    pub fn set(&mut self, pane: PaneNum, state: AgentState) -> bool {
        if state == AgentState::Done && self.ended.contains(&pane) {
            return false;
        }
        if !matches!(state, AgentState::Done) {
            self.ended.remove(&pane);
        }
        self.panes.insert(pane, state) != Some(state)
    }

    pub fn remove(&mut self, pane: PaneNum) -> bool {
        self.ended.insert(pane);
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
                if *s == AgentState::Done {
                    self.ended.insert(pane);
                }
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
        Self {
            waiting: "🔔".into(),
            working: "⏳".into(),
            done: "✅".into(),
        }
    }
}

fn badge(icon: &str, n: usize) -> String {
    match n {
        0 => String::new(),
        1 => format!(" {icon}"),
        _ => format!(" {icon}{n}"),
    }
}

/// Remove trailing badges in our icons: `dev ⏳ ⏳ 🔔2` -> `dev`. Labels painted
/// by another instance, or left over from before a restart, then can't stack.
pub fn strip_badges<'a>(name: &'a str, icons: &Icons) -> &'a str {
    let mut rest = name.trim_end();
    loop {
        let Some((head, last)) = rest.rsplit_once(' ') else {
            return rest;
        };
        let is_badge = [&icons.waiting, &icons.working, &icons.done]
            .iter()
            .any(|i| {
                last.strip_prefix(i.as_str())
                    .is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()))
            });
        if !is_badge {
            return rest;
        }
        rest = head.trim_end();
    }
}

/// `dev 🔔 ⏳2`: a count only when more than one agent shares the state.
pub fn compose_label(base: &str, c: &Counts, icons: &Icons) -> String {
    format!(
        "{base}{}{}{}",
        badge(&icons.waiting, c.waiting),
        badge(&icons.working, c.working),
        badge(&icons.done, c.done),
    )
}

struct Painted {
    base: String,
    /// every label we have painted over this base; renames land late, so a
    /// tab may still report an older one of these
    labels: HashSet<String>,
    last: String,
}

/// Remembers what name the user gave a tab versus what we painted on it, so
/// a manual rename is adopted as the new base and badges never stack, even
/// when zellij reports stale names while several renames are in flight.
#[derive(Default)]
pub struct TabNames {
    tabs: HashMap<TabId, Painted>,
}

impl TabNames {
    pub fn base_for(&self, tab: TabId, current: &str, icons: &Icons) -> String {
        match self.tabs.get(&tab) {
            Some(p) if p.base == current || p.labels.contains(current) => p.base.clone(),
            // zellij reports an empty name mid-rename; never adopt that as a base
            prev => match strip_badges(current, icons) {
                "" => prev.map_or_else(|| current.trim().to_string(), |p| p.base.clone()),
                base => base.to_string(),
            },
        }
    }

    /// True when `label` still needs to be sent to zellij.
    pub fn needs_rename(&self, tab: TabId, label: &str, current: &str) -> bool {
        label != current && self.tabs.get(&tab).is_none_or(|p| p.last != label)
    }

    pub fn painted(&mut self, tab: TabId, base: &str, label: &str) {
        let entry = self.tabs.entry(tab).or_insert_with(|| Painted {
            base: base.to_string(),
            labels: HashSet::new(),
            last: base.to_string(),
        });
        if entry.base != base {
            *entry = Painted {
                base: base.to_string(),
                labels: HashSet::new(),
                last: base.to_string(),
            };
        }
        if entry.labels.len() > 16 {
            entry.labels.clear();
        }
        entry.labels.insert(label.to_string());
        entry.last = label.to_string();
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
        let after = hits.iter().find(|h| {
            order
                .iter()
                .position(|x| x == *h)
                .is_some_and(|i| i >= start)
        });
        return after.or(hits.first()).copied();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        assert_eq!(
            parse_command("3:waiting"),
            Some(Command::Set(3, AgentState::Waiting))
        );
        assert_eq!(
            parse_command("terminal_7:completed"),
            Some(Command::Set(7, AgentState::Done))
        );
        assert_eq!(parse_command("3:end"), Some(Command::End(3)));
        assert_eq!(parse_command("terminal_7:exit"), Some(Command::End(7)));
        assert_eq!(parse_command("3:shutdown"), Some(Command::End(3)));
        assert_eq!(parse_command("x:working"), None);
        assert_eq!(parse_command("3:bogus"), None);
    }

    #[test]
    fn grok_exit_is_teardown_not_done() {
        assert!(stop_reason_is_teardown("shutdown"));
        assert!(stop_reason_is_teardown("channel_closed"));
        assert!(!stop_reason_is_teardown("end_turn"));
        assert!(!stop_reason_is_teardown(""));
    }

    #[test]
    fn end_wins_over_a_later_done() {
        let mut t = Tracker::default();
        t.set(1, AgentState::Working);
        assert!(t.remove(1));
        assert!(!t.set(1, AgentState::Done));
        assert_eq!(t.get(1), None);
        assert!(t.set(1, AgentState::Working));
        assert_eq!(t.get(1), Some(AgentState::Working));
    }

    #[test]
    fn label_single_and_multi() {
        let icons = Icons::default();
        let one = count([AgentState::Working].into_iter());
        assert_eq!(compose_label("dev", &one, &icons), "dev ⏳");
        let many = count(
            [
                AgentState::Waiting,
                AgentState::Working,
                AgentState::Working,
            ]
            .into_iter(),
        );
        assert_eq!(compose_label("dev", &many, &icons), "dev 🔔 ⏳2");
        let mixed = count([AgentState::Working, AgentState::Done].into_iter());
        assert_eq!(compose_label("dev", &mixed, &icons), "dev ⏳ ✅");
        assert_eq!(
            compose_label("dev", &count(std::iter::empty()), &icons),
            "dev"
        );
    }

    #[test]
    fn idle_agents_add_no_badge() {
        let c = count([AgentState::Idle, AgentState::Waiting].into_iter());
        assert_eq!(compose_label("t", &c, &Icons::default()), "t 🔔");
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
    fn acknowledging_done_ignores_a_later_done() {
        let mut t = Tracker::default();
        t.set(1, AgentState::Done);
        assert!(t.acknowledge(1));
        assert!(!t.set(1, AgentState::Done));
        assert_eq!(t.get(1), None);
    }

    #[test]
    fn manual_rename_becomes_new_base() {
        let mut n = TabNames::default();
        let i = Icons::default();
        n.painted(0, "dev", "dev 🔔");
        assert_eq!(n.base_for(0, "dev 🔔", &i), "dev");
        assert_eq!(n.base_for(0, "api", &i), "api");
    }

    #[test]
    fn stale_names_do_not_stack_badges() {
        // two renames in flight; zellij first reports the older label
        let mut n = TabNames::default();
        n.painted(1, "work", "work 🔔");
        n.painted(1, "work", "work 🔔1 ⏳1");
        let i = Icons::default();
        assert_eq!(n.base_for(1, "work 🔔", &i), "work");
        assert_eq!(n.base_for(1, "work 🔔1 ⏳1", &i), "work");
        assert_eq!(n.base_for(1, "work", &i), "work");
    }

    #[test]
    fn foreign_labels_do_not_stack_badges() {
        // another instance (or a previous run) painted these; we never saw them
        let n = TabNames::default();
        let i = Icons::default();
        assert_eq!(n.base_for(2, "valkeet ⏳ ⏳ ⏳ ⏳", &i), "valkeet");
        assert_eq!(n.base_for(2, "api 🔔1 ⏳2 ✅1", &i), "api");
        // an icon inside a word is kept
        assert_eq!(strip_badges("a ⏳x", &i), "a ⏳x");
    }

    #[test]
    fn empty_or_badge_only_names_keep_the_old_base() {
        // mid-rename zellij reports "", and we then see our own " ⏳" on it
        let mut n = TabNames::default();
        let i = Icons::default();
        n.painted(3, "valkeet", "valkeet ⏳");
        assert_eq!(n.base_for(3, "", &i), "valkeet");
        assert_eq!(n.base_for(3, " ⏳ ⏳", &i), "valkeet");
        // a tab we have never painted keeps whatever it is called
        assert_eq!(n.base_for(4, "⏳", &i), "⏳");
    }

    #[test]
    fn does_not_resend_a_rename_already_in_flight() {
        let mut n = TabNames::default();
        assert!(n.needs_rename(1, "work 🔔", "work"));
        n.painted(1, "work", "work 🔔");
        assert!(!n.needs_rename(1, "work 🔔", "work")); // not applied yet
        assert!(n.needs_rename(1, "work", "work 🔔")); // clearing the badge
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
