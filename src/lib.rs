//! zellij-agent-status: per-pane AI agent state, summarised on tab names.
//!
//! `core` is pure and unit-tested on the host; `plugin` is the zellij glue
//! and only exists on the wasm target.

pub mod core;

#[cfg(target_family = "wasm")]
mod plugin;
