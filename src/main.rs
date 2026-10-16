//! The zellij plugin. Zellij runs plugins as wasm programs, so this is a binary.

#[cfg(target_family = "wasm")]
mod plugin;

#[cfg(target_family = "wasm")]
use zellij_tile::prelude::*;

#[cfg(target_family = "wasm")]
register_plugin!(plugin::State);

#[cfg(not(target_family = "wasm"))]
fn main() {
    eprintln!("zellij-agent-status is a zellij plugin: build for wasm32-wasip1");
}
