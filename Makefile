.PHONY: build test check install clean

# Workspace targets. `--workspace` covers every member crate — adding
# another binary (e.g. balls-plugin-github-issues, Epic B) requires no
# Makefile change beyond a new line in `install` to copy its binary
# out of `target/release/` (B7 adds that line). `make check` is the
# pre-commit gate: tests + workspace clippy + line-length cap +
# 100% line coverage (cargo-tarpaulin).

build:
	cargo build --release --workspace

test:
	cargo test --workspace

check: test
	cargo clippy --workspace --all-targets -- -D warnings
	scripts/check-line-lengths.sh
	scripts/check-coverage.sh

install: build
	install -d ~/.local/bin
	install -m 0755 target/release/balls-plugin-github ~/.local/bin/balls-plugin-github

clean:
	cargo clean
