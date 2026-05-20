.PHONY: build test check install clean

build:
	cargo build --release

test:
	cargo test

check: test
	cargo clippy --all-targets -- -D warnings
	scripts/check-line-lengths.sh
	scripts/check-coverage.sh

install: build
	install -d ~/.local/bin
	install -m 0755 target/release/balls-plugin-github ~/.local/bin/balls-plugin-github

clean:
	cargo clean
