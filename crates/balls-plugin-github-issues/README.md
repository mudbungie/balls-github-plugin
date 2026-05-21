# balls-plugin-github-issues (issue-tracker plugin)

A [balls](https://github.com/mudbungie/balls) **issue-tracker
plugin** for GitHub Issues. Bidirectional mirror: balls task
create/update/close propagates to a GH issue (push); external GH
issue state and content flow back into balls via `sync`/SyncReport
(pull). The sibling [`balls-plugin-github`](../balls-plugin-github/)
in this workspace is the forge plugin — different role, different
projection, different role; see the [workspace README](../../README.md)
for why they are separate participants.

## Authority model — read this before enabling

The defaults bake in policy decisions made when the plugin was
scoped. Each is overridable per config.

- **`close_mirror: "authoritative"`** (default). When a GH issue is
  closed externally (someone closes it on github.com), the next
  `bl sync` flips the mapped balls task to `status="closed"` via
  a `SyncReport.updated` entry. This is the strict default —
  external state can close balls work. Set `close_mirror: "off"`
  if you want GH to never own status; `"best_effort"` keeps the
  emission but downstream failure-policy treats it as best-effort
  (warns + records sync_status; does not abort the lifecycle event).
- **Auto-create from new GH issues**. Any GH issue without a stored
  `external.github_issues.issue.number` mapping and without a
  `[bl-xxxx]` tag in its title is treated as an external report and
  becomes a new balls task on the next `bl sync` (a
  `SyncReport.created` entry). Filter the scope via
  `target_label` if you only want labeled issues to flow through.
- **`on_external_delete: "deferred"`** (default). A previously-
  mirrored GH issue that vanishes from the API flips the balls
  task to `status="deferred"` rather than `closed` — operator
  decides whether to revive or close. Set `"closed"` or `"noop"`
  to change.

The asymmetric merge rule (locked by `src/merge.rs` and its
conformance tests): **GH-closed beats balls-open; balls-open
beats GH-open.** GH never re-opens a closed balls task; balls's
workflow direction is authoritative for everything except the
close transition.

## Install

The workspace `make install` installs both binaries to
`~/.local/bin/` (see the [workspace README](../../README.md)).

## Configure

Git-tracked, non-secret, at `.balls/plugins/github-issues.json`:

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
| `target_label` | no | unset | If set, sync only mirrors issues carrying this label. Unset = every issue in the repo is in scope. |
| `on_external_delete` | no | `deferred` | What to do when a previously-mirrored GH issue vanishes. One of `deferred`, `closed`, `noop`. |
| `close_mirror` | no | `authoritative` | Policy for GH-side closes flowing to balls. One of `authoritative`, `best_effort`, `off`. |

The token is the only secret. It is read from **stdin** by
`auth-setup` and stored at `<auth-dir>/token.json`, mode `0600`.
This plugin uses its own auth-dir (separate from the forge plugin)
so you can rotate or fine-grain each independently:

```sh
echo "$GITHUB_PAT" | balls-plugin-github-issues auth-setup \
    --config .balls/plugins/github-issues.json \
    --auth-dir .balls/local/plugins/github-issues/
```

A classic PAT with `repo` scope is sufficient; a fine-grained token
with `issues: read/write` works for read-only `repo` (public) repos.

## Commands

Implements the balls plugin protocol (README §Plugin System):

| Command | Behaviour |
|---|---|
| `auth-setup` | Read a token from stdin, validate it via `GET /user`, store it. |
| `auth-check` | Re-validate the stored token. Exit 0 if valid, non-zero otherwise. |
| `push --task ID` | For a task (any status): create/update/close the mapped GH issue with title `"<title> [ID]"` and body = description. Idempotent — a stored number is reused; status unchanged since last sync is a noop. Prints `{"issue":{number,url,state,source,synced_at,last_synced_status,last_synced_title,last_synced_body_hash}}` which core stores into `task.external.github-issues`. The `last_synced_*` triple is the *who-moved* oracle for the next sync's content mirror. |
| `sync` | Poll GH issues for the repo. For each matched issue, emit `updated` carrying close-mirror status + title/body content mirror (bl-4918, see below); for each unmatched untagged issue, emit `created` (auto-create with bl-4673-aligned defenses); for each balls task whose stored number is no longer in the GH list, emit `updated` per `on_external_delete`. Each emitted `updated` also rewrites the `external.github-issues.issue.*` projection so a subsequent sync against an unchanged GH state is a noop. Empty arrays are omitted from the report. |

### Content mirror (bl-4918)

Title and body changes flow GH → balls under the same asymmetric merge contract as status (`merge.rs`): **balls wins on conflict, GH wins when only GH moved.** The decision per field:

| Did GH move? | Did balls move? | Result |
|---|---|---|
| no | * | nothing — already converged, or balls's edit will sync out on the next push |
| yes | no | mirror GH's value to balls |
| yes | yes | leave balls, emit an `add_note` describing both views; the next push reasserts balls |

"Did X move?" is decided against the `last_synced_title` / `last_synced_body_hash` fields the push side records on every emit. The body hash is FNV-1a-64 hex (16 bytes), constant-size by construction so the projection doesn't grow with body length. A legacy task whose projection predates these fields skips the content mirror until the next push populates them; the lifecycle still converges, it just takes one more sync to do so.

## Ingest defenses (bl-4673, bl-2202)

When auto-creating balls tasks from GH issues (the
attacker-influenceable direction), the plugin refuses to ingest an
issue whose title carries any `[bl-xxxx]` marker. balls's
`store.all_tasks` (the task input handed to sync) is open-only, so
the matching ball may be archived and absent from the lookup; the
marker itself is sufficient evidence of prior mirroring and treating
it as a fresh report restarts the close-mirror-re-ingest loop
(bl-2202).

It also pre-truncates inputs so a pathological repo can't blow up
core's ingest:

- `MAX_BODY_BYTES` (64 KiB): oversized bodies are truncated at a
  UTF-8 char boundary; the description carries a marker line so the
  operator sees what happened.
- `MAX_LABELS` (100): label sets are capped, preserving GH order;
  the description carries a marker line.
- `MAX_CREATES_PER_SYNC` (500): the sync loop stops appending
  creates after the cap; remaining unmatched issues page to the
  next sync invocation.
- `MAX_DELETES_PER_SYNC` (500): the deleted-from-GH sweep is also
  bounded so a pagination-induced false-positive (B4a hasn't
  paginated yet — known limitation) can't cascade through every
  mirrored task in one go.

These plugin-side defenses sit on top of balls-core's own
`bl-b807` sanitizer and `bl-4673` ingest backstops; the
defense-in-depth is intentional.

## Live sandbox testing

`scripts/live_sandbox.sh` exercises auth-setup + auth-check + a
read-only sync against a real GH repo. Gated by
`BALLS_PLUGIN_LIVE_TEST=1`:

```sh
GITHUB_PAT=ghp_... GH_REPO=you/sandbox \
BALLS_PLUGIN_LIVE_TEST=1 \
scripts/live_sandbox.sh
```

The mockito-driven unit and integration tests
(`tests/{auth,push,sync,lifecycle}.rs` plus the in-source
`#[cfg(test)]` modules) prove the wire-protocol contract under
`make check`; this script proves real-API connectivity. The two
layers are complementary.

## License

MIT.
