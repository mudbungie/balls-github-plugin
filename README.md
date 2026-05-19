# balls-plugin-github

A [balls](https://github.com/mudbungie/balls) **forge plugin** for
GitHub. It automates the external-merge half of balls's *deferred*
delivery mode: `bl review` pushes a work branch and opens a gate child;
this plugin opens the pull request and, once GitHub merges it, closes
that gate child so the parent task's `bl close` is unblocked.

It is a standalone project, intentionally **not** part of the balls
core repo — balls stays small and forge-agnostic; each forge gets its
own plugin speaking the same protocol. The lifecycle contract it
implements is specified in balls's
`docs/SPEC-forge-gated-delivery.md` (§9, "Forge plugin contract").

> A forge plugin is **not** an issue-tracker plugin. It does not sync
> tasks to GitHub Issues; it drives pull requests for code delivery.
> Issue-tracker plugins (Jira, Linear, GitHub Issues) are a different
> family.

## Install

```sh
make install   # builds release, installs to ~/.local/bin/balls-plugin-github
```

`make check` runs the full gate: tests, clippy (`-D warnings`), the
300-line-per-file cap, and 100% line coverage (`cargo-tarpaulin`).
Install the matching pre-commit hook with `scripts/install-hooks.sh`.

## Configure

Git-tracked, non-secret, at `.balls/plugins/github.json`:

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
| `target_branch` | no | Default PR base. A task's own `target_branch` overrides it. Deferred-mode review needs a base *somewhere*: if both are unset, `push` errors rather than guessing `main`. |
| `api_base` | no | API root. Override for GitHub Enterprise. Defaults to `https://api.github.com`. |

The token is the only secret. It is read from **stdin** by
`auth-setup` (no TTY prompt, so the flow is scriptable) and stored at
`<auth-dir>/token.json`, mode `0600`. Core never reads it.

```sh
echo "$GITHUB_PAT" | balls-plugin-github auth-setup \
    --config .balls/plugins/github.json \
    --auth-dir .balls/local/plugins/github/
```

A classic PAT with `repo` scope (or a fine-grained token with
pull-request read/write) is sufficient.

## Commands

Implements the balls plugin protocol (README §Plugin System):

| Command | Behaviour |
|---|---|
| `auth-setup` | Read a token from stdin, validate it via `GET /user`, store it. |
| `auth-check` | Re-validate the stored token. Exit 0 if valid, non-zero otherwise. |
| `push --task ID` | For a `review` task: open the PR for `work/ID` against the effective target branch (per-task → config), title `"<title> [ID]"`. Idempotent — an existing PR (by stored number or head branch) is reused. Prints `{"pull_request":{number,url,head_sha,target_branch}}`, which core stores into `task.external.github`. |
| `sync [--task ID]` | For each `review` task with a recorded PR, poll it. When merged, emit a sync-report `updated` entry that closes the task's `gates` child, with the merge SHA in `add_note`. Core closes the gate child; the parent's `bl close` unblocks. |

The plugin never calls `bl close` on the parent itself — it only
closes the gate child. The operator (or other automation) closes the
parent. This keeps the plugin from owning the whole lifecycle.

## End-to-end (deferred mode)

```sh
# repo configured with delivery.mode = "deferred"
bl review bl-1234 -m "Add the thing"      # pushes work/bl-1234, opens gate child
bl sync                                    # core calls: push --task bl-1234  → PR opened
# ... reviewers approve and merge the PR on GitHub ...
bl sync                                    # core calls: sync → PR merged → gate child closed
bl close bl-1234 -m "Shipped"             # now unblocked; delivered_in resolved by tag-scan
```

## License

MIT.
