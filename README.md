# balls-github-plugin (workspace)

A cargo workspace shipping the GitHub-side [balls](https://github.com/mudbungie/balls)
plugins. Two binaries in one repo, sharing a small library crate for
auth/HTTP/config:

| Crate | What it is |
|---|---|
| [`balls-plugin-github`](crates/balls-plugin-github/) | **Forge delivery plugin.** The §11 *forge* variant of the delivery plugin: at `claim` it opens an approval gate child (`--blocks close`); at `close` it pushes `work/<id>` and opens/updates the PR (no local squash); `sync` closes the gate child once the PR merges, unblocking the parent's `bl close`. Speaks the §6/§7 subprocess protocol. Implements `docs/architecture.md` §11 (FORGE) + §9/§10. |
| [`balls-plugin-github-issues`](crates/balls-plugin-github-issues/) | **Issue-tracker plugin.** Bidirectional mirror between balls tasks and GitHub Issues. balls-side `create`/`update`/`close` mirror to GH issues; an external GH issue close/edit flows back via `sync`/SyncReport. |
| [`balls-github-shared`](crates/balls-github-shared/) | Library crate. Token I/O, the base `GithubClient` (auth + status mapping + `GET /user`), the shared `RepoConfig` (repo + api_base), and the plugin-protocol types (`Task`, `Link`, `SyncReport`, `SyncUpdate`). |

## Why two binaries, not one

balls's participant model is "**one participant = one name**". A forge
*delivery* plugin and an issue-tracker plugin are *different roles*:

- The forge plugin is a **delivery** variant (§11): it drives the
  `work/<id>` pull request and an approval gate child. It is STATELESS —
  it keeps no `task.external.*` projection, re-finding the PR each time by
  its head branch (`work/<id>`) and tracking only the `parent → gate`
  link in its own XDG territory.
- The issues plugin owns `task.external.github-issues.*` (the issue ref)
  and participates in `create`/`update`/`close`/`sync` to keep GitHub
  Issues in sync with balls tasks.

They are independently configured (separate `config/plugins/<name>.json`
on the landing) and hold **separate tokens** (separate plugin
territories) so you can rotate or fine-grain them independently, and they
are wired into different op-phase hooks (`[hooks]`, §6). None of that
survives a merge into one binary.

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
If you find yourself wanting to put a `external.github-issues.…`
reference (the issues plugin's projection) into `balls-github-shared`,
you are about to violate the disjoint-projection rule. Push that code
into the plugin crate that owns the projection. (The forge plugin keeps
no `external.*` projection at all — it is stateless across ops, §11.)

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
- [`crates/balls-plugin-github-issues/README.md`](crates/balls-plugin-github-issues/README.md) — issue-tracker plugin (bidirectional GH Issues mirror).

## License

MIT.
