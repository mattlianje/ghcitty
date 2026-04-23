SHELL := /bin/zsh
# Snapshot the original PATH before we prepend, so `install` can check what
# the user's shell would actually pick up.
ORIG_PATH := $(PATH)
export PATH := $(HOME)/.cargo/bin:$(HOME)/.ghcup/bin:$(HOME)/.local/bin:$(PATH)

.PHONY: build run test clean install

build:
	cargo build --release

run: build
	./target/release/ghcitty

test:
	cargo test

install:
	cargo install --path .
	codesign --force --sign - $(HOME)/.cargo/bin/ghcitty
	@active="$$(PATH='$(ORIG_PATH)' command -v ghcitty 2>/dev/null)"; \
	expected="$(HOME)/.cargo/bin/ghcitty"; \
	if [ -n "$$active" ] && [ "$$active" != "$$expected" ]; then \
		echo ""; \
		echo "warning: another 'ghcitty' is shadowing the one we just installed:"; \
		echo "    on PATH:        $$active"; \
		echo "    just installed: $$expected"; \
		echo "remove the stale binary (rm $$active) or put $(HOME)/.cargo/bin earlier in PATH."; \
	fi

clean:
	cargo clean
