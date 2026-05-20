.PHONY: build test check install clean

# Workspace targets. `--workspace` covers every member crate; adding
# another binary requires one new line in `install`. `make check` is
# the pre-commit gate: tests + workspace clippy + line-length cap +
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
	install -m 0755 target/release/balls-plugin-github-issues ~/.local/bin/balls-plugin-github-issues

clean:
	cargo clean
