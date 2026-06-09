# balls-plugin-github-issues (issue-tracker plugin)

A [balls](https://github.com/mudbungie/balls) **issue-tracker plugin** for
GitHub Issues, on the §6/§7 subprocess protocol (bl-613d). Bidirectional mirror:
a balls task create/update/close propagates to a GitHub issue (push, on the
verb's `post`); GitHub issues flow back into balls on `sync` (pull). The sibling
[`balls-plugin-github`](../balls-plugin-github/) is the forge plugin — different
role, different state; see the [workspace README](../../README.md).

## How it speaks to balls

balls dispatches the binary as `balls-plugin-github-issues <op> <phase>` with the
§7 payload on stdin, and reads `balls-plugin-github-issues protocol` once at
install to validate the binding. **There is no return channel** (§7): the plugin
never prints state for balls to merge. Instead:

- **push** (`create`/`update`/`close`/`drop` `post`) calls the GitHub API
  directly from the sealed task state.
- **pull** (`sync`) drives the public verb surface — it shells `bl create` /
  `bl update` / `bl close` for each inward change (the §6 "plugin shells back").

### Where state lives

No GitHub data is stored on the ball. The durable task↔issue join is the
`[bl-xxxx]` marker the plugin appends to the **issue title** (GitHub is
authoritative for issue identity). A small reconciliation base (last-synced
title/body-hash/state per issue, plus the GitHub token) lives in the plugin's
own territory, `$XDG_STATE_HOME/balls/plugins/github-issues/<project>/` — machine-
local, derived, rebuildable from GitHub. Uninstalling is `rm -rf` of that
directory: zero ball edits, zero core changes.

## Authority model — read this before enabling

balls owns task content; GitHub owns only the close transition inward. Concretely:

- **Content is balls-authoritative.** A balls title/body edit is mirrored OUT to
  the issue. If an issue's title/body drifts on GitHub, the next `bl sync`
  re-asserts the ball's content back onto it. (The greenfield `bl update` verb
  cannot set a ball's title/body, so there is no inward content mirror — this is
  the authority model enforced by the platform, not a missing feature.)
- **`close_mirror: "authoritative"`** (default). A GitHub-side close runs
  `bl close` on the mapped task. `"best_effort"` behaves the same here;
  `"off"` makes GitHub never close a balls task.
- **Auto-create.** An in-scope, open GitHub issue with no `[bl-xxxx]` marker and
  no known link becomes a new task (`bl create`); the marker is then stamped back
  onto the issue. Scope with `target_label`. Already-closed external issues are
  not imported.
- **`on_external_delete: "deferred"`** (default). A previously-mirrored issue
  that vanishes from the repo tags the task `deferred` (operator decides).
  `"closed"` closes it; `"noop"` ignores it.

## Configure

Committed, non-secret, on the landing at
`config/plugins/github-issues/config.json`:

```json
{
  "repo": "owner/name",
  "api_base": "https://api.github.com",
  "target_label": "balls:track",
  "on_external_delete": "deferred",
  "close_mirror": "authoritative"
}
```

| Field | Required | Default | Meaning |
|---|---|---|---|
| `repo` | yes | — | `owner/name` of the GitHub repo. |
| `api_base` | no | `https://api.github.com` | API root. Override for GitHub Enterprise. |
| `target_label` | no | unset | If set, only issues carrying this label are in sync scope. |
| `on_external_delete` | no | `deferred` | `deferred` \| `closed` \| `noop`. |
| `close_mirror` | no | `authoritative` | `authoritative` \| `best_effort` \| `off`. |

## Auth

The token is the only secret. Run the human-facing subcommand from the project
directory; it validates the token and stores it in the plugin's territory
(mode `0600`):

```sh
echo "$GITHUB_PAT" | balls-plugin-github-issues auth-setup
balls-plugin-github-issues auth-check   # exit 0 if the stored token is valid
```

## Wiring

Add the plugin to the landing's `config/plugins.toml` `[hooks]` — push on the
verb posts, pull on `sync.post` (after the tracker has imported the store):

```toml
[hooks]
"create.post" = ["tracker", "github-issues"]
"update.post" = ["tracker", "github-issues"]
"close.post"  = ["bl-delivery", "tracker", "github-issues"]
"sync.post"   = ["github-issues"]
```

## Migrating from legacy balls

Legacy (pre-greenfield) balls kept the task↔issue join inline on the task as
`external.github-issues.issue.number`. The greenfield join is the `[bl-xxxx]`
title marker plus the territory base; legacy issues carry no marker, so without
a hint the first `sync` would auto-create a duplicate for every mirrored issue.

`adopt` is the one-time cutover step that seeds the base's number→id fallback
(`Base::id_for_number`) from a legacy task store, so the first `sync` re-adopts
each existing issue with **zero dups**. It runs **offline** — it reads the
legacy JSON only, never GitHub — and keys the territory on the cwd, like the
auth subcommands, so run it from the project directory:

```sh
# extract the legacy task JSON (e.g. from the pre-cutover store branch)
git archive balls-archive .balls/tasks | tar -x -C /tmp/legacy
balls-plugin-github-issues adopt /tmp/legacy/.balls/tasks
```

Per the §16 *migrate-clean-or-delink* rule it seeds only the one fact the legacy
store proves — the issue number; the agreed title/body/state are left as
force-refresh sentinels the first `sync` overwrites from GitHub. It is
idempotent and non-clobbering (an id the base already knows is left untouched),
so a re-run is safe. Closed and never-mirrored legacy tasks are skipped.

Run order: `bl prime` (brings up XDG/config) → `adopt` → first `bl sync`.

## Install

The workspace `make install` builds and installs the binary to `~/.local/bin/`.
