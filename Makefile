.PHONY: build test check install clean

# Workspace-wide targets. Adding a second binary (Epic B) requires no
# Makefile change — `--workspace` covers it. A3 will fully rework this
# along with the README; this revision is the minimal change needed to
# keep `make check` green across the workspace.

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
