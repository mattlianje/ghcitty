SHELL := /bin/zsh
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

clean:
	cargo clean
