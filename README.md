# balls-github-plugin (workspace)

A cargo workspace shipping the GitHub-side [balls](https://github.com/mudbungie/balls)
plugins. Two binaries in one repo, sharing a small library crate for
auth/HTTP/config:

| Crate | What it is |
|---|---|
| [`balls-plugin-github`](crates/balls-plugin-github/) | **Forge plugin.** Drives the deferred-mode pull-request gate: `bl review` pushes the work branch and opens a gate child; this plugin opens the PR; once it merges on GitHub, sync closes the gate child so the parent's `bl close` unblocks. Implements `SPEC-forge-gated-delivery.md` §9. |
| `balls-plugin-github-issues` *(not yet shipped — Epic B in this workspace's bl store)* | **Issue-tracker plugin.** Bidirectional mirror between balls tasks and GitHub Issues. balls-side `create`/`update`/`close` mirror to GH issues; an external GH issue close/edit flows back via `sync`/SyncReport. |
| [`balls-github-shared`](crates/balls-github-shared/) | Library crate. Token I/O, the base `GithubClient` (auth + status mapping + `GET /user`), the shared `RepoConfig` (repo + api_base), and the plugin-protocol types (`Task`, `Link`, `SyncReport`, `SyncUpdate`). |

## Why two binaries, not one

balls's participant model is "**one participant = one name = one
projection**" (`SPEC-lifecycle-sync-participants.md` §3 and §5). A
forge plugin and an issue-tracker plugin are *different roles* with
disjoint authoritative projections:

- The forge plugin owns `task.external.github.*` (the PR ref) and
  participates in `review`/`sync` to drive the deferred-mode gate.
- The issues plugin owns `task.external.github_issues.*` (the issue
  ref) and participates in `create`/`update`/`close`/`sync` to keep
  GitHub Issues in sync with balls tasks.

They are independently configurable in `.balls/config.json`
(`plugins.github` vs `plugins.github-issues`), with **independent
per-event failure policies** (you almost certainly want forge-gate
sync to be reliable but issue-mirror best-effort), and they hold
**separate tokens** (`.balls/local/plugins/github/` vs
`.balls/local/plugins/github-issues/`) so you can rotate or
fine-grain them independently. None of that survives a merge into
one binary.

> **For future maintainers — including agents.** Don't merge these
> into one participant. The shared library crate already covers the
> "one token, less duplication" instinct without paying the
> projection-overlap, single-policy-knob, and stable-coupled-to-churny
> costs. If you find yourself wanting to merge them, re-read
> `SPEC-lifecycle-sync-participants.md` §3 (projection) and §5
> (Participant contract: one name).

The shared library crate has a load-bearing boundary invariant: it
has zero references to any per-plugin `external.<name>.*` literal,
enforced by a unit test
(`projection_boundary_test::shared_has_no_per_plugin_projection_refs`).
If you find yourself wanting to put a `external.github.…` or
`external.github_issues.…` reference into `balls-github-shared`, you
are about to violate the disjoint-projection rule. Push that code
into the plugin crate that owns the projection.

## Build, test, install

```sh
make build       # cargo build --release --workspace
make test        # cargo test --workspace
make check       # tests + workspace clippy (-D warnings) + 300-line cap + 100% coverage
make install     # builds and installs binaries to ~/.local/bin/
make clean
```

`make check` is the gate. The pre-commit hook (`scripts/install-hooks.sh`)
runs it before every commit. CI runs the same gate.

The 300-line-per-file cap and 100% line coverage are non-negotiable
balls conventions (`balls/AGENTS.md`); the workspace-aware versions
of those scripts live in `scripts/`.

## Per-plugin docs

Each binary keeps its own README under `crates/<name>/README.md`
with its configuration schema, the command/protocol surface, and a
worked end-to-end example:

- [`crates/balls-plugin-github/README.md`](crates/balls-plugin-github/README.md) — forge plugin.
- *(issues plugin README ships with Epic B child B7.)*

## License

MIT.
