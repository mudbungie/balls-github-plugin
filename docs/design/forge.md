# forge — the GitHub PR review gate (balls-plugin-github)

**Living design record (bl-42c1 modernization).** This is the first tracked
design doc in this repo — it establishes `docs/design/` as the home for the
forge plugin's reasoning, mirroring balls core's `docs/design/bl-chore.md`. It
describes the plugin AS IT EXISTS in this repo (read `forge.rs`, `bl_ops.rs`,
`project.rs`, `pr_api.rs`); the per-crate `README.md` is the operator-facing
behaviour, this file holds the *why* at full length. Amend it like code — the
ball records the work, this file is the artifact.

The forge plugin (`balls-plugin-github`) is balls core's `bl-chore` sibling: both
mint a tagged **close-gate child at `claim.post`**. bl-chore mints declarative
chores from config; forge mints ONE review gate per claimed task and — unlike
bl-chore — also **resolves** it (on PR merge). The mint spelling is identical
(`bl create --parent <id> --blocks close`); the difference is the resolution
half and the GitHub join.

## What it is

A balls **forge plugin** for GitHub on the **subtask model** (balls
`docs/architecture.md` §11, bl-7bfe): the review gate is an ordinary
close-blocker **gate child** (§10), NOT a delivery variant. It never pushes,
never opens a PR, never hooks `close.pre`. `bl-delivery` keeps the WHOLE delivery
lifecycle (worktree materialize / squash-deliver / tear down); forge changes
*who merges*, never the delivery path. It touches exactly two moments:

- **`claim.post`** — for the just-claimed task, mint one review **gate child**:
  `bl create --parent <id> --blocks close -- "Review gate: <title>"`, then stamp
  a plugin-namespaced preserved key joining gate → parent. The minted id is the
  hook's §6 stdout product. Minting SKIPS when the claimed task is itself one of
  the plugin's gate children (no gates-for-gates) or when a standing open gate
  for this parent already exists (an unclaim-and-reclaim reuses it).
- **`sync.post`** — for each open gate child (the preserved-key scan over `bl
  list --json`), find the parent's PR by its `work/<parent>` head branch; if
  merged, `bl close` the gate child, unblocking the parent's `bl close`. One
  human-readable line per resolved gate.

## Why `--parent X --blocks close`, not `--subtask-of`

The decisive design fact (and the heart of the bl-42c1 modernization):

- **bl-788e** once gave `--subtask-of X` a one-word meaning: parent pointer +
  reciprocal close-gate on X. Forge's original `claim.post` mint used it.
- **bl-5d9a (2026-06-17) flipped that.** `bl create --subtask-of X` now gates
  X's **CLAIM** (it lands `X.blockers += {child, claim}`), NOT its close. A
  close gate must now be spelled **explicitly**: `bl create --parent X --blocks
  close`. bl-788e's close-gate sugar was superseded.

So the gate is now minted as an explicit edge. This is the canonical spelling —
bl-chore's `render_create` mints the same shape (`["create","--parent",parent,
"--blocks","close","-t",tag,"--as",actor,"--",title]`). The forge mint differs
only in carrying NO `-t` chore tag (its join is a preserved key, not a tag) and
prefixing the subject with `Review gate: `.

The title is task-sourced (untrusted) and rides a positional behind the `--`
end-of-options separator, so a hostile `-`-leading title can never hijack a flag
(the bl-d31f core seam). The plugin owns the whole argv, exactly like bl-chore —
the flags are placed before `--` by the plugin, never spliced into a free-form
line.

## The stateless join — no projection, derive everything

The plugin is **STATELESS across ops** (§11/§14). It holds no id-keyed scratch
and no `task.external.github.*` projection — two representations of one fact
would drift (CLAUDE.md: single source of truth; delivery is the tag, readiness
is the query). Two facts, two derivations:

- **parent → gate** is the plugin-namespaced **preserved key** (§3 extras) the
  mint stamps on the gate child: `<plugin-name> = "<parent-id>"`. It is
  namespaced by the plugin's own `BALLS_PLUGIN_NAME`, so two differently-named
  forge wirings never claim each other's gate children. The open-gate set is
  re-derived every read by scanning `bl list --json` for rows carrying the key
  (`wire::open_gates`); a closed gate has no file, so absence = resolved.
- **parent → PR** is NEVER stored. Each `sync` re-finds the PR by its head
  branch `work/<parent>` (`pr_api::find_pr`, `state=all`). Merged-ness is
  `merged_at.is_some()` (the LIST endpoint returns `merged_at`, not the `merged`
  boolean that exists only on the single-PR GET).

So the gate is the key, the PR is the branch name, and nothing can fall out of
sync because nothing is held.

## Git-native submission + core's tag-scan delivery

Submission is **git-native work**, not a plugin step: the worker pushes
`work/<id>` and opens the PR themselves, with `[bl-id]` in the PR title. The
squash-merge GitHub produces is what core delivery's tag-scan (bl-430e)
recognizes at the parent's `bl close` — so the local squash is skipped. One
delivery path, kind-blind: forge does not re-implement delivery, it gates it.

Deliberate non-actions (skill-doc lines, bl-7bfe):

- An **empty deliverable**'s gate has no auto-resolve moment — its claimant
  closes the gate by hand ("nothing to review").
- **Abandoning** a forge-gated task (`bl unclaim`, then `bl close`) stays
  blocked by the open gate: close it or `--no-needs`-unlink it first.
- It never `bl close`s the *parent* — only the gate child. The parent is closed
  by whoever runs `bl close` after the gate clears.

## Rollback (§14) — close, the one retirement

`rollback claim.post` deletes the just-minted gate child, re-derived by the same
key scan — no scratch (§14). "Delete" is `bl close` (close is the one
retirement, §10). No open gate is a clean no-op (the mint never happened, or was
already undone). This DIFFERS from bl-chore, which ships a no-op rollback: a
bl-chore gate is independently sealed+pushed by its own `create.post`, whereas
forge's single mint is an unpushed edge the claim op can still take back — so
forge honours §11's "remove the just-minted gate child" teardown while bl-chore
deviates to no-op.

## Wiring & order

Opt-in (not a default schedule). Two hooks:

```toml
[hooks]
"claim.post" = ["bl-delivery", "balls-plugin-github", "bl-tracker"]  # worktree, then mint the review gate child
"sync.post"  = ["balls-plugin-github"]                               # close the gate child on PR merge
```

Order follows the §6 hook-list rule: **only the IRREVERSIBLE effect sorts
LAST** (bl-tracker's push, bl-delivery's squash). The reversible forge mint sits
*before* bl-tracker so an aborted claim stays local-reversible — its un-mint
(the rollback `bl close`) is not stranded behind a remote push. There is no
forge `close.pre`; every other hook keeps the default schedule.

## Severability + config territory

The plugin's committed, non-secret config lives in its **own territory subdir**
on the landing: `<landing>/config/plugins/<name>/config.json` (the bl-42c1
cutover from the flat `<name>.json`, matching the issues plugin and bl-chore's
`<landing>/config/plugins/bl-chore/chores.toml`). Core never reads it (§4
severability — policy lives in the capability, not the core). It holds `repo`
(required `owner/name`) and `api_base` (optional, GHE override). The token is the
only secret, read from stdin by `auth-setup` and stored under the plugin's XDG
territory, mode `0600`.

`bl` itself is resolved on `$PATH` (the bl-42c1 cutover): core spawns `<bin>
<op> <phase>` with ONLY `BALLS_PROTOCOL` / `BALLS_PLUGIN_NAME` /
`BALLS_PLUGIN_DEPTH` (§6/§7) — it sets no `BALLS_BL`. A plugin shells `bl` off
the triggering invocation's env, exactly like bl-chore (`Cli::at("bl")`) and
bl-tracker. The `bl` path stays a constructor field on the runner so unit tests
inject a fake without mutating global env; integration tests inject the fake by
prepending its directory to `$PATH`.

## The bl-chore sibling relationship

| | bl-chore (core) | forge (this plugin) |
|---|---|---|
| Mints at | `claim.post` | `claim.post` |
| Mint spelling | `create --parent X --blocks close -t bl-chore` | `create --parent X --blocks close` (no tag) |
| Join | the `bl-chore` tag | a plugin-namespaced preserved key |
| Count | N from config | 1 per claimed task |
| Resolves the gate? | NO (resolution is a separate plugin) | YES (on PR merge, `sync.post`) |
| Recursion break | tag-skip (claimed task carries the tag) | gate-of-skip (claimed task carries the join key) |
| Idempotent reclaim | epic-skip (live children exist) | standing-gate reuse (open gate for parent exists) |
| Rollback | no-op (independently sealed) | `bl close` the mint (unpushed edge) |

Both sit on the same §10 truth — **core enforces blocking but never mints
edges**; creation is left to plugins, and an explicit `bl create --blocks close`
is exactly that. forge earns its separate binary by owning the resolution half
and the GitHub join, which bl-chore (create-side only) deliberately does not.

## Where it ships

`balls-plugin-github`, one of two binaries in the `balls-github-plugin`
workspace (the sibling `balls-plugin-github-issues` is the GitHub-Issues
mirror). Third-party plugins keep their own non-`bl-` names (`bl-` is reserved
to first-party plugins, bl-27bf). Installed from source via `make install`;
gated by `make check` (tests, clippy `-D warnings`, 300-line-per-file cap, 100%
line coverage).
