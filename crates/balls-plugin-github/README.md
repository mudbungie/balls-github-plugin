# balls-plugin-github (forge delivery plugin)

A [balls](https://github.com/mudbungie/balls) **forge plugin** for
GitHub — the §11 *forge* variant of the delivery / worktree plugin. It
delivers a task's code through a **pull request** instead of a local
squash: at `claim` it opens an approval **gate child** (a normal
`--blocks close` close-blocker, §10); at `close` it pushes the
`work/<id>` branch and opens/updates the PR; once GitHub merges that PR,
`sync` closes the gate child so the parent task's next `bl close` is
unblocked. The forge produces the squash (the merge), so the plugin
**never squashes locally**.

It speaks the §6/§7 **subprocess protocol** (`<bin> <op> <phase>`, the
§7 wire on stdin, no return channel) — the same contract the shipped
`bl-delivery` and `tracker` plugins use. The lifecycle it implements is
specified in balls's `docs/architecture.md` §11 ("Delivery / worktree
plugin", FORGE variant) and §9/§10 (close-blocker gating).

This crate is one of two binaries in the
[`balls-github-plugin` workspace](../../README.md); the sibling
`balls-plugin-github-issues` is the GitHub-Issues mirror. Both share the
`balls-github-shared` library crate (auth, HTTP, config base) but ship as
**separate participants** — separate names, projections, and auth dirs.

> A forge plugin is **not** an issue-tracker plugin. It does not sync
> tasks to GitHub Issues; it drives pull requests for code delivery.

## How it is wired (opt-in)

The forge variant differs from the default DIRECT variant only in *what
is wired into the delivery hooks*. It runs ALONGSIDE `bl-delivery`, which
still owns the `work/<id>` code worktree (materialize / tear down /
re-materialize); the forge plugin only takes over the PR half. To opt a
landing into forge delivery, set `config/plugins.toml`'s `[hooks]` to:

```toml
[hooks]
"claim.post" = ["bl-delivery", "balls-plugin-github", "tracker"]  # worktree, then gate child
"close.pre"  = ["balls-plugin-github"]                            # push + PR, NOT a local squash
"close.post" = ["bl-delivery", "tracker"]                         # bl-delivery still tears the worktree down
"sync.post"  = ["balls-plugin-github"]                            # close the gate child on merge
"drop.post"  = ["bl-delivery", "balls-plugin-github", "tracker"]  # worktree + PR teardown
# claim.pre / unclaim.* / prime.post stay on bl-delivery (the worktree lifecycle)
```

`bl install` resolves each name to this box's binary via the local
`config/plugins/bin/<name>` symlink. `bl prime` prunes a hook entry whose
binary is not installed beside `bl`.

## Install

```sh
make install   # builds release, installs to ~/.local/bin/balls-plugin-github
```

`make check` is the gate: tests, clippy (`-D warnings`), the
300-line-per-file cap, and 100% line coverage (`cargo-tarpaulin`).
Install the matching pre-commit hook with `scripts/install-hooks.sh`.

## Configure

Git-tracked, non-secret, on the landing at
`config/plugins/<plugin-name>.json` (the bundle `bl install` carries;
`<plugin-name>` is the name balls invokes it under via
`BALLS_PLUGIN_NAME`, e.g. `balls-plugin-github`):

```json
{
  "repo": "owner/name",
  "target_branch": "main",
  "api_base": "https://api.github.com"
}
```

| Field | Required | Meaning |
|---|---|---|
| `repo` | yes | `owner/name` of the GitHub repository. |
| `target_branch` | no | Default PR base. A task's own `target_branch` (a preserved frontmatter key) overrides it. A forge PR needs a base *somewhere*: if both are unset, `close` errors rather than guessing `main`. |
| `api_base` | no | API root. Override for GitHub Enterprise. Defaults to `https://api.github.com`. |

The token is the only secret. It is read from **stdin** by `auth-setup`
(scriptable, no TTY prompt) and stored under the plugin's XDG territory
at `$XDG_STATE_HOME/balls/plugins/<plugin-name>/auth/token.json`, mode
`0600`. Core never reads it.

```sh
echo "$GITHUB_PAT" | balls-plugin-github auth-setup [api_base]
balls-plugin-github auth-check [api_base]   # re-validate the stored token
```

A classic PAT with `repo` scope (or a fine-grained token with
pull-request read/write **and** contents read/write, for the branch
push) is sufficient. The branch is pushed with the token in the URL, so
no ambient credential helper is needed.

## Protocol surface

`balls-plugin-github protocol` self-describes as
`{"protocol":[1],"ops":["claim","close","drop","sync"]}`. The hooks:

| Hook | Behaviour |
|---|---|
| `claim post` | `bl create` the approval gate child (`--parent <id> --blocks close -t forge-gate`), recording the `parent → gate` link in the plugin's territory. Idempotent. |
| `close pre` | Capture pending `work/<id>` work, then **push** it + open/update the PR (`"<title> [<id>]"`, base = per-task → config). If `work/<id>` has no changes (the empty deliverable), instead **close the gate child** so the close proceeds. Core's close-blocker guard (§10) keeps the close blocked while the PR is unmerged. Prints the PR URL (a §6 human hint). |
| `sync post` | For each remembered gate, poll its `work/<parent>` PR; when merged, `bl close` the gate child and forget the link → the parent's next `bl close` unblocks. |
| `drop post` | Close the PR, delete the remote `work/<id>` branch, and `bl drop` the orphaned gate child. |

**Rollback (§14):** rollback of `claim post` drops the just-opened gate
child; rollback of `close pre` is a **no-op** — a pushed branch + open PR
is the correct in-review state, never undone (abandon is `bl drop`).

The plugin never `bl close`s the *parent* — only the gate child. The
parent is closed by whoever runs `bl close` after the gate clears.

## End-to-end (forge delivery)

```sh
bl claim bl-1234                 # bl-delivery makes work/bl-1234; forge opens the gate child
# ... write code in the work/bl-1234 worktree, commit ...
bl close bl-1234                 # forge pushes work/bl-1234 + opens the PR; close stays BLOCKED on the gate
# ... reviewers approve and merge the PR on GitHub ...
bl sync                          # forge sees the merge → closes the gate child
bl close bl-1234                 # now unblocked; the task retires (delivery already landed via the merge)
```

## License

MIT.
