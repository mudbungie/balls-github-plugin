# balls-plugin-github (forge plugin)

A [balls](https://github.com/mudbungie/balls) **forge plugin** for
GitHub, on the **subtask model** (balls `docs/architecture.md` §11,
bl-7bfe): the review gate is an ordinary close-blocker **gate child**
(§10), and the plugin is NOT a delivery variant — it never pushes,
never opens a PR, and never hooks `close.pre`. It does exactly two
things:

- **`claim.post`** — mint the review gate child of the claimed task
  (one `bl create --parent <id> --blocks close` — an explicit
  close-gate edge), stamped with a plugin-namespaced preserved
  key that joins gate → parent.
- **`sync.post`** — for each open gate child, check the parent's PR by
  its `work/<parent>` head branch; merged ⇒ `bl close` the gate child,
  unblocking the parent's `bl close`.

**Submission is git-native work**: the worker pushes `work/<id>` and
opens the PR themselves, with the `[bl-id]` tag in the PR title. The
squash-merge GitHub produces is what core delivery's tag-scan (bl-430e)
recognizes at the parent's close, so the local squash is skipped — one
delivery path, kind-blind.

It speaks the §6/§7 **subprocess protocol** (`<bin> <op> <phase>`, the
§7 wire on stdin, no return channel) — the same contract the shipped
`bl-delivery` and `bl-tracker` plugins use.

This crate is one of two binaries in the
[`balls-github-plugin` workspace](../../README.md); the sibling
`balls-plugin-github-issues` is the GitHub-Issues mirror. Both share the
`balls-github-shared` library crate (auth, HTTP, config base) but ship as
**separate participants** — separate names, projections, and auth dirs.

> A forge plugin is **not** an issue-tracker plugin. It does not sync
> tasks to GitHub Issues; it gates code delivery on pull-request review.

## How it is wired (opt-in)

`bl-delivery` keeps the WHOLE delivery lifecycle (worktree materialize /
squash-deliver / tear down) — forge changes *who merges*, never the
delivery path. To opt a landing into forge review, add the plugin to two
hooks in `config/plugins.toml`'s `[hooks]`:

```toml
[hooks]
"claim.post" = ["bl-delivery", "balls-plugin-github", "bl-tracker"]  # worktree, then mint the review gate child
"sync.post"  = ["balls-plugin-github"]                            # close the gate child on PR merge
# every other hook keeps the default schedule — there is no forge close.pre
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
`config/plugins/<plugin-name>/config.json` — the plugin's own territory
subdir (the bundle `bl install` carries; `<plugin-name>` is the name
balls invokes it under via `BALLS_PLUGIN_NAME`, e.g.
`balls-plugin-github`, matching the issues plugin and bl-chore):

```json
{
  "repo": "owner/name",
  "api_base": "https://api.github.com"
}
```

| Field | Required | Meaning |
|---|---|---|
| `repo` | yes | `owner/name` of the GitHub repository the PRs live in. |
| `api_base` | no | API root. Override for GitHub Enterprise. Defaults to `https://api.github.com`. Must be `https://` (`http://` is allowed only on loopback); a non-default base is warned on stderr. |

There is no `target_branch`: the plugin opens no PRs, so it has no base
to name — the worker picks the base when they open the PR.

The token is the only secret. It is read from **stdin** by `auth-setup`
(scriptable, no TTY prompt) and stored under the plugin's XDG territory
at `$XDG_STATE_HOME/balls/plugins/<plugin-name>/auth/token.json`, mode
`0600`. Core never reads it.

```sh
echo "$GITHUB_PAT" | balls-plugin-github auth-setup [api_base]
balls-plugin-github auth-check [api_base]   # re-validate the stored token
```

A read-only token suffices: the plugin only **reads** pull requests
(a classic PAT with `repo` scope, or a fine-grained token with
pull-request read).

## Protocol surface

`balls-plugin-github protocol` self-describes as
`{"protocol":[1],"ops":["claim","sync"]}`. The hooks:

| Hook | Behaviour |
|---|---|
| `claim post` | Mint the review gate child: `bl create --parent <id> --blocks close -- "Review gate: <title>"` (an explicit close-gate edge — since bl-5d9a `--subtask-of` gates the parent's *claim*, not its close, so bl-788e's one-word sugar was superseded; the spelling matches bl-chore), then stamp the join key `bl update <gate> <plugin-name>=<id>`. Prints the minted id (the §6 stdout product). **Skips** when the claimed task itself carries the plugin's key (it IS a gate child — no gates-for-gates) and when an open gate for this parent already exists (an unclaim-and-reclaim reuses it). |
| `sync post` | Scan `bl list --json` for open tasks carrying the plugin's key; for each, poll the PR whose head is `work/<parent>`; when merged, `bl close` the gate child with the PR URL in the note → the parent's next `bl close` unblocks. Prints one line per resolved gate. |

**Rollback (§14):** rollback of `claim post` deletes (closes) the
just-minted gate child, re-derived by the same key scan — the plugin
keeps no scratch at all: the gate is the key, the PR is the branch name.

**The join key.** Each gate child carries one preserved frontmatter
extra (§3), `<plugin-name> = "<parent-id>"` — plugin-namespaced, so two
differently-named forge wirings never claim each other's gates. It is
the single machine marker; everything else (the PR, the parent's title)
is derived.

What the plugin deliberately does NOT do (skill-doc lines, bl-7bfe):

- An **empty deliverable**'s gate has no auto-resolve moment — its
  claimant closes the gate by hand ("nothing to review").
- **Abandoning** a forge-gated task (`bl unclaim`, then `bl close`)
  stays blocked by the open gate: close it or `--no-needs`-unlink it
  first.
- It never `bl close`s the *parent* — only the gate child. The parent is
  closed by whoever runs `bl close` after the gate clears.

## End-to-end (forge review)

```sh
bl claim bl-1234                 # bl-delivery makes work/bl-1234; forge mints the review gate child
# ... write code in the work/bl-1234 worktree, commit ...
git push origin work/bl-1234     # git-native submission: push the branch...
gh pr create --head work/bl-1234 --title "Add the thing [bl-1234]"   # ...and open the PR ([bl-id] in the title)
# ... reviewers approve; GitHub squash-merges the PR ...
bl sync                          # forge sees the merge → closes the gate child
bl close bl-1234                 # unblocked; the tag-scan sees [bl-1234] already on main and skips the local squash
```

## License

MIT.
