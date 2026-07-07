# balls-github-plugin (workspace)

A cargo workspace shipping the GitHub-side [balls](https://github.com/mudbungie/balls)
plugins. Two binaries in one repo, sharing a small library crate for
auth/HTTP/config:

| Crate | What it is |
|---|---|
| [`balls-plugin-github`](crates/balls-plugin-github/) | **Forge plugin** (subtask model, bl-7bfe — NOT a delivery variant). At `claim.post` it mints a review **gate child** (`bl create --parent <id> --blocks close`, an explicit §10 close-gate edge) carrying a plugin-namespaced join key; at `sync.post` it closes each gate child whose parent's `work/<id>` PR has merged. PR submission is git-native work (the worker pushes + opens the PR, `[bl-id]` in the title); there is no forge `close.pre`. Speaks the §6/§7 subprocess protocol. Implements `docs/architecture.md` §10/§11 (FORGE). |
| [`balls-plugin-github-issues`](crates/balls-plugin-github-issues/) | **Issue-tracker plugin.** Bidirectional mirror between balls tasks and GitHub Issues. balls-side `create`/`update`/`close` mirror to GH issues; an external GH issue close/edit flows back on `sync` by shelling the public verbs (`bl create`/`update`/`close` — there is no return channel, §7). |
| [`balls-github-shared`](crates/balls-github-shared/) | Library crate. Token I/O, the base `GithubClient` (auth + status mapping + `GET /user`), the shared `RepoConfig` (repo + api_base), and the shared §7 wire shapes (`Binding`, `Metadata`/`metadata_id`). |

## Why two binaries, not one

balls's participant model is "**one participant = one name**". A forge
*delivery* plugin and an issue-tracker plugin are *different roles*:

- The forge plugin gates delivery on PR review (§10/§11): it mints the
  review gate child and resolves it on merge. It is STATELESS — it keeps
  no `task.external.*` projection and no scratch: the `parent → gate`
  join is a preserved key on the gate child itself, and the PR is
  re-found each sync by its head branch (`work/<id>`).
- The issues plugin owns `task.external.github-issues.*` (the issue ref)
  and participates in `create`/`update`/`close`/`sync` to keep GitHub
  Issues in sync with balls tasks.

They are independently configured (separate
`config/plugins/<name>/config.json` on the landing) and hold
**separate tokens** (separate plugin
territories) so you can rotate or fine-grain them independently, and they
are wired into different op-phase hooks (`[hooks]`, §6). None of that
survives a merge into one binary.

> **For future maintainers — including agents.** Don't merge these
> into one participant. The shared library crate already covers the
> "one token, less duplication" instinct without paying the
> projection-overlap, single-policy-knob, and stable-coupled-to-churny
> costs. If you find yourself wanting to merge them, re-read
> `docs/architecture.md` §6 in the balls repo (one plugin = one NAME:
> the hook schedule, the `bin/<name>` symlink, and the per-name
> territory, §1, all key on it).

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
make install     # builds and installs binaries to ~/.local/bin/ (the issues
                 # binary lands as `github-issues` — its schedule name, so bl's
                 # by-name binding finds it beside `bl` with no --bin step)
make clean
```

`make check` is the gate. The pre-commit hook (`scripts/install-hooks.sh`)
runs it before every commit, and CI (`.github/workflows/ci.yml`) runs the
same gate on every push and pull request.

The 300-line-per-file cap and 100% line coverage are non-negotiable
balls conventions (`balls/AGENTS.md`); the workspace-aware versions
of those scripts live in `scripts/`.

These plugins are installed **from source** with `make install` (binaries
land beside `bl` in `~/.local/bin/`); they are not published to crates.io,
so the repo carries no release-plz automation. `[workspace.package]` pins
the MSRV (`rust-version`) and shared metadata for the day that changes.

## Per-plugin docs

Each binary keeps its own README under `crates/<name>/README.md`
with its configuration schema, the command/protocol surface, and a
worked end-to-end example:

- [`crates/balls-plugin-github/README.md`](crates/balls-plugin-github/README.md) — forge plugin.
- [`crates/balls-plugin-github-issues/README.md`](crates/balls-plugin-github-issues/README.md) — issue-tracker plugin (bidirectional GH Issues mirror).

## License

MIT.
