WASM := target/wasm32-wasip1/release/zellij_agent_status.wasm
DEST := $(HOME)/.config/zellij/plugins/zellij-agent-status.wasm

.PHONY: build test install
build:
	cargo build --release
test:
	cargo test --lib --target $$(rustc -vV | sed -n 's/host: //p')
install: build
	mkdir -p $(dir $(DEST)) && cp $(WASM) $(DEST)
