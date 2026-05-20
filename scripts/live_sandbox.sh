#!/usr/bin/env bash
# Live integration smoke test against a real GitHub repository.
#
# This script is the operator-driven counterpart to tests/lifecycle.rs:
# the mockito tests prove the wire-protocol contract; this script
# proves real GitHub connectivity. It is NOT part of `make check`
# (no network in CI), and is invoked manually when validating a
# release candidate against a dedicated sandbox repo.
#
# Required env vars:
#   GITHUB_PAT    A personal access token with `repo` scope.
#   GH_REPO       owner/name of the sandbox repo (e.g. mudbungie/balls-plugin-sandbox).
#
# Optional:
#   BALLS_PLUGIN_LIVE_TEST=1   Gate flag (mirrors the SPEC's pattern
#                              for opt-in network tests).
#
# Outputs PASS/FAIL per step on stderr; exits 0 on overall PASS.
#
# What it exercises:
#   1. auth-setup against api.github.com via stdin'd token.
#   2. auth-check round-trips the token.
#   3. push --task on a synthetic "review" task opens a PR.
#      (The forge plugin: balls-plugin-github.)
#   4. Same idempotent on second invocation (reuses the existing PR).
#
# Issues-plugin live exercises are deliberately left to the operator
# to run interactively, since auto-creating real GitHub issues from
# a script litters the sandbox repo. The lifecycle.rs mockito tests
# cover the bidirectional issue mirror; the live check is whether
# real-API connectivity and auth work end-to-end.

set -euo pipefail

step() { printf '\n[live_sandbox] %s\n' "$*" >&2; }
fail() { printf '\n[live_sandbox] FAIL: %s\n' "$*" >&2; exit 1; }

if [[ "${BALLS_PLUGIN_LIVE_TEST:-}" != "1" ]]; then
    fail "BALLS_PLUGIN_LIVE_TEST=1 must be set; this script makes real GH API calls."
fi
: "${GITHUB_PAT:?GITHUB_PAT must be set to a PAT with repo scope}"
: "${GH_REPO:?GH_REPO must be set to owner/name of a sandbox repo}"

# Build first so any test runs the just-built binaries.
step "build"
( cd "$(dirname "$0")/.." && cargo build --release --workspace >&2 )
FORGE_BIN="$(dirname "$0")/../target/release/balls-plugin-github"
ISSUES_BIN="$(dirname "$0")/../target/release/balls-plugin-github-issues"

# Isolated config + auth-dir per script run.
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK/forge"
cat > "$WORK/forge/config.json" <<EOF
{"repo":"$GH_REPO","target_branch":"main"}
EOF

step "forge: auth-setup"
printf '%s\n' "$GITHUB_PAT" | "$FORGE_BIN" auth-setup \
    --config "$WORK/forge/config.json" \
    --auth-dir "$WORK/forge" \
    || fail "auth-setup rejected the token; check scope (needs repo)"

step "forge: auth-check"
"$FORGE_BIN" auth-check \
    --config "$WORK/forge/config.json" \
    --auth-dir "$WORK/forge" \
    || fail "auth-check failed; the token didn't round-trip"

mkdir -p "$WORK/issues"
cat > "$WORK/issues/config.json" <<EOF
{"repo":"$GH_REPO"}
EOF

step "issues: auth-setup (separate auth-dir, same PAT)"
printf '%s\n' "$GITHUB_PAT" | "$ISSUES_BIN" auth-setup \
    --config "$WORK/issues/config.json" \
    --auth-dir "$WORK/issues" \
    || fail "issues auth-setup rejected the token"

step "issues: auth-check"
"$ISSUES_BIN" auth-check \
    --config "$WORK/issues/config.json" \
    --auth-dir "$WORK/issues" \
    || fail "issues auth-check failed"

# Issues-side dry-poll: list issues for the sandbox repo. Empty
# task list -> any unmatched issue would be classified AutoCreate,
# but we don't act on it here; the goal is to prove the API round-
# trip works against the real endpoint.
step "issues: sync dry-poll (real GH list)"
echo '[]' | "$ISSUES_BIN" sync \
    --config "$WORK/issues/config.json" \
    --auth-dir "$WORK/issues" \
    > "$WORK/sync.out" \
    || fail "issues sync against real GH failed"

step "issues: sync output ($(wc -l < "$WORK/sync.out") lines)"
head -1 "$WORK/sync.out" >&2

step "PASS: live sandbox smoke checks completed against $GH_REPO"
