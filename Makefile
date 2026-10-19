WASM := target/wasm32-wasip1/release/zellij-agent-status.wasm
DEST := $(HOME)/.config/zellij/plugins/zellij-agent-status.wasm
HOOK := $(HOME)/.config/zellij/agent-status-hook.sh

.PHONY: build test install
build:
	cargo build --release
test:
	cargo test --lib --target $$(rustc -vV | sed -n 's/host: //p')
install: build
	mkdir -p $(dir $(DEST)) && cp $(WASM) $(DEST)
	cp scripts/agent-status-hook.sh $(HOOK) && chmod +x $(HOOK)
