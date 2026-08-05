# Issue #4 — Share a whole project (all environments) via relay

**Label:** enhancement (not a bug)
**Branch base:** `main` (the projects/environments migration is merged — `projects`, `environments`, `environment_vars`, `item_projects` all exist)
**Related:** #11 (test harness, in flight — this plan *consumes* it), #12 (`idx_projects_name_nocase` semantics, in flight — this plan *depends on* its collision error mapping)

---

## 0. Read this first — the value gate

The issue asks to build a feature. Before the implementation steps, here is the honest case for and against, because a cheaper action closes the issue's stated complaint.

### 0.1 What the investigation actually found

The dead-protocol claim in the issue is **true but understated**. `handle_workspace_relay_send` / `handle_workspace_relay_receive` in `/home/maosuarez/Programas/crypt-env/src-tauri/src/api/mod.rs` do not merely lack a UI — they read and write the **legacy `workspaces` / `workspace_vars` tables**, which post-migration no product surface writes to and no product surface reads from:

- `handle_workspace_relay_send` (L2662, L2676) calls `db.list_workspaces()` / `db.get_workspace_vars(id)`. Those tables are now **frozen backfill sources only** — `db/mod.rs` L209-212 drains them once into each project's default environment. Sending from them ships a snapshot of pre-migration state, not the user's current project.
- `handle_workspace_relay_receive` (L3004, L3026) calls `db.upsert_workspace()` / `db.set_workspace_vars()`. It writes rows into `workspaces` that **nothing can display**: the GUI, the CLI (`crypt-env project`) and the MCP project tools all read `projects`/`environments`. The one-time backfill is gated by a settings flag and will never re-run. The received secrets *are* created as real `items` rows, but they land with **zero owners** and are not linked into any environment, so they surface only in Global Secrets, unlabelled.

`/home/maosuarez/Programas/crypt-env/docs/reference.md` L45-46 already documents both endpoints as "Legacy… items imported this way are NOT linked into any project/environment and are invisible to the scoped endpoints above". So this is not undiscovered — it is **known-broken code with a live MCP surface** (`crypt_env_share_workspace_send` / `_receive`, `crypt-env-mcp.rs` L2077 / L2112) that an LLM agent can call today and get a silent data black hole.

That is a stronger reason to act than "unused code exists".

### 0.2 What already covers the user workflow

Project-level sharing is **not** absent today. Two surfaces already exist:

| Surface | Carries structure | Carries values | Where |
|---|---|---|---|
| `project_export` / `project_import` | yes (env names, `isDefault`, var **keys**) | **no** — `literal: None` is hard-coded, deliberately | `project/mod.rs` L458-525, wired in `ProjectManager.tsx` L676/L685 |
| `crypt-env relay receive --project X --env local` | no | yes | `commands/relay.rs` L23-36 |

So the concrete gap is: **one operation that carries structure *and* values for N environments at once.** Today the same outcome takes: export template → send template file → receiver imports → sender selects the items in ShareModal → relay-send → receiver relay-receives into each environment, once per environment.

That is a real ergonomic win for the "onboard a teammate onto a project" workflow, but it is an ergonomic win, not a new capability. Do not oversell it.

### 0.3 Recommendation

**Do both, in this order, as separable PRs:**

1. **Phase 0 — delete the workspace relay surface. Unconditional, ships on its own.** This alone closes the issue's stated complaint ("finished protocol with no product surface") and removes a broken, agent-reachable code path. If the feature below is never built, the repo is still strictly better. ~1 day.
2. **Phase 1-3 — build the project relay.** Recommended, with a **reduced surface**: protocol + Tauri + GUI + CLI. **Drop the MCP tool** (see D9). ~3-4 days.

**Why not delete-only:** the receive side is where the value concentrates. A teammate reconstructing a runnable three-environment project in one step is materially better than seven manual steps, and the safety default the issue proposes (non-default environments unchecked) makes the values-carrying version *safer* than the current "select the items by hand in ShareModal" flow, which has no environment awareness at all.

**Why not build-only:** deleting the legacy handlers is a prerequisite, not an afterthought. Leaving `/workspaces/*/relay/*` alive next to `/projects/*/relay/*` guarantees someone (or some agent) picks the wrong one.

**If the maintainer disagrees on value:** ship Phase 0, close #4 with the deletion, and open a new issue for the feature. That is a legitimate outcome of this plan and it is cheap.

---

## 1. Objective

Definition of done. Every item below is independently checkable.

### 1.1 Deleted

- `WorkspaceBundle`, `WorkspaceBundleVar`, `WorkspaceBundle::KIND`, `encrypt_workspace`, `decrypt_workspace` are gone from `src-tauri/src/share/relay.rs`.
- Routes `POST /workspaces/:id/relay/send` and `POST /workspaces/relay/receive` are gone from the router in `api/mod.rs`; handlers `handle_workspace_relay_send`, `handle_workspace_relay_receive` and the response structs `WorkspaceRelaySendResponse` / `WorkspaceRelayReceiveResponse` are deleted.
- MCP tools `crypt_env_share_workspace_send` / `crypt_env_share_workspace_receive` and their handlers `tool_share_workspace_send` / `tool_share_workspace_receive` are gone from `crypt-env-mcp.rs`, and removed from the `tools/list` manifest.
- `grep -rn "WorkspaceBundle\|workspace_relay\|share_workspace" src-tauri/src src/` returns **zero hits**.
- The `workspaces` / `workspace_vars` / `workspace_paths` tables and their `db` accessors are **kept** (they are still the migration backfill source — see D10).

### 1.2 Protocol

- `ProjectBundle` in `share/relay.rs` with `kind: "project"`, `version: 1`, and `environments: Vec<EnvironmentBundle>`; items hoisted to the bundle root, referenced by name.
- `encrypt_project(&ProjectBundle, &[u8;32]) -> Result<String, ShareError>` and `decrypt_project(&str, &[u8;32]) -> Result<ProjectBundle, ShareError>`, the latter rejecting a wrong `kind` **and** an unknown `version` before returning.
- No `literal` field anywhere in the new format (D3). No `paths` field anywhere in the new format (D6).

### 1.3 Delivered surfaces

| Surface | Name | File |
|---|---|---|
| HTTP | `POST /projects/:id/relay/send` | `api/mod.rs` |
| HTTP | `POST /projects/relay/receive` | `api/mod.rs` |
| Tauri | `project_relay_send(project_id, environment_ids) -> RelayShareResult` | `project/relay_commands.rs` (new), registered in `lib.rs` |
| Tauri | `project_relay_receive(code, passphrase, project_name_override) -> ProjectReceiveResult` | same |
| CLI | `crypt-env project share --id N --envs a,b` | `bin/crypt-env/commands/project.rs` |
| CLI | `crypt-env project receive --code X --passphrase Y [--as NAME]` | same |
| GUI | "SHARE PROJECT" button on the project detail view, opening an environment checklist + key manifest + confirm, then the existing code/passphrase display | `ProjectManager.tsx` + new `ProjectShareModal.tsx` |
| GUI | "RECEIVE PROJECT" on the projects list view | same |

MCP: **no new tool** (D9).

### 1.4 Tests (all in `src-tauri`, no network)

Named cases, consuming the #11 harness (`test_support::{unlocked_vault, seed_project, seed_item, link_var, read_item}`):

**Pure, in `share/relay.rs` `#[cfg(test)] mod tests`:**
1. `project_bundle_roundtrip_preserves_structure` — encrypt → decrypt returns identical env names, `is_default` flags, var keys and item names.
2. `decrypt_project_rejects_items_payload` — a payload produced by `encrypt_items` fails with `ShareError::Protocol`, not a panic and not a successful parse.
3. `decrypt_project_rejects_unknown_version` — a bundle with `version: 99` is rejected with a message naming the supported version.
4. `decrypt_project_rejects_wrong_passphrase` — a different `derive_relay_key` input yields `ShareError::Crypto`/decrypt failure.

**Integration, in `src-tauri/tests/project_relay.rs` (`#[tokio::test]` + `tempfile`, transport stubbed — bundle handed directly to the receive-side function):**

5. **`share_three_env_project_dedups_reused_items`** — the round-trip case named in the objective. Seed a project with 3 environments (`local` default, `staging`, `production`) and 5 distinct items, 2 of which are linked into all 3 environments. Build the bundle for all 3 environments; assert `bundle.items.len() == 5` (not 11), and `bundle.environments.iter().map(|e| e.vars.len()).sum() == 11`. Receive into a fresh vault; assert the receiving vault contains **exactly 5 new `items` rows**, **11 `environment_vars` rows**, **3 `environments` rows**, **5 `item_projects` rows all pointing at the new project**, and that the two shared items appear as **one row each** (query `items` by decrypted name, assert count 1) — i.e. **0 duplicated ciphertext rows**.
6. `receive_refuses_case_insensitive_project_name_collision` — receiving `MyApp` into a vault that has `myapp` returns the typed conflict error, and the vault is **unchanged**: no new `items`, `environments`, `environment_vars` or `item_projects` rows (transaction rollback, D5/D7).
7. `receive_with_name_override_succeeds_after_collision` — same bundle, `project_name_override = "MyApp-received"`, completes and produces the row counts from case 5.
8. `send_excludes_unselected_environments` — selecting only the default environment from the 3-env project yields a bundle with 1 environment and only the items reachable from it; an item used *only* by `production` is absent from `bundle.items`.
9. `send_skips_dangling_item_reference` — an `environment_vars` row whose `item_id` no longer resolves is skipped without failing the send (mirrors the existing L2707 behaviour), and the resulting bundle's var count is one lower.
10. `received_items_are_project_owned_not_global` — every received item has `is_global = false` and exactly one `item_projects` row.
11. `bundle_never_contains_paths` — serialize a bundle built from a project whose environments have `paths` set; assert the JSON string contains none of those path substrings (D6 is enforced by the type, this test guards regressions).

**Test gate:** `cargo test` green; case 5 and case 6 are the two that must not be weakened during review.

### 1.5 Docs

`docs/reference.md` rows L45-46 replaced with the two `/projects/...` routes; MCP table rows L251-252 and the L282 backward-compatibility paragraph deleted; `CHANGELOG.md` entry under Unreleased noting the **breaking** removal of the workspace relay endpoints and MCP tools.

---

## 2. What is being mitigated

This is an enhancement. Stated as two checkable claims, without inflation.

**(a) Dead-and-broken protocol code with zero product surface is resolved — either shipped or deleted.**

| Check | Today | After |
|---|---|---|
| `grep -c "WorkspaceBundle" src-tauri/src` | 12 | 0 |
| Endpoints backed by frozen legacy tables | 2 | 0 |
| MCP tools that write rows no surface can read | 2 | 0 |
| Protocol structs with no consumer | `WorkspaceBundle`, `WorkspaceBundleVar` | none |

The specific defect removed: an agent calling `crypt_env_share_workspace_receive` today decrypts real secrets, writes them into the vault as **ownerless items**, and rebuilds the project into a table the application no longer reads. The user sees loose secrets in Global Secrets and no project. Nobody has reported it because the tool is undiscoverable — that is the *reason* it is worth removing rather than an argument that it is harmless.

**(b) The user workflow unlocked.**

> "Give my teammate everything they need to run this project" becomes one send + one receive, instead of: export template file → transfer it → import → open ShareModal → hand-select the items belonging to each environment → relay-send → relay-receive with `--env`, repeated per environment.

Honest bound on the value: the *capability* mostly exists (see §0.2). What is new is (i) doing it in one step, (ii) preserving multi-environment structure and values together, and (iii) the sender seeing an explicit, per-environment list of exactly which keys are about to leave the machine — which the current item-picker flow does not provide, because it has no idea which environment an item belongs to. Item (iii) is the strongest security argument for building the feature rather than deleting only.

**Not claimed:** this does not make sharing more secure at the transport layer (same relay, same AES-256-GCM, same Argon2id, same 24h TTL, same burn-after-read), does not add access control, and does not reduce the trust placed in the Supabase relay operator.

---

## 3. Decisions (with trade-offs)

### D1 — `ProjectBundle` shape: nested environments, root-level deduped items

```
ProjectBundle {
  kind: "project",          // discriminator, checked on decrypt
  version: 1,               // numeric, checked on decrypt
  name, description?, template,
  environments: [ EnvironmentBundle { name, is_default, vars: [ { key, item_name } ] } ],
  items: [ PlainItem ]      // deduped by name, referenced from vars by item_name
}
```

Rejected: (i) items nested inside each environment — simplest to build, but duplicates the ciphertext-bearing payload once per environment and makes "same item in 3 envs" indistinguishable from "3 items with the same name" on receive, which is exactly the bug the issue asks to avoid; (ii) reusing `ExportedProject` from `project/mod.rs` by adding an optional values field — attractive (one format for file export and relay) but it would make it possible to *accidentally* write values into a `.cryptenv-proj` file on disk, and that file format's whole point is that it never carries values. Keeping the two formats separate is the safer default. Cost accepted: two similar structs to maintain.

**Reference key is `item_name`, not an id.** Ids are meaningless across vaults. Cost: two items with the same name in the sender's vault collapse into one in the bundle. That is already the behaviour of the existing workspace bundle (`bundled.entry(item_name)` at L2711) and of `import_plain_items_into_vault`. Accepted, but the send-side preview (D4) must show the deduped list so the sender sees the collapse.

### D2 — `version` field, checked

`decrypt_project` rejects `version != 1` with `ShareError::Protocol("this package was created by a newer version of CryptEnv (format v{n}); update to receive it")`. Without this, the next format change repeats today's problem: a second discriminator swap. Cost: one field, one branch. Do this now — it is free before the first release and impossible after.

### D3 — `literal` does **not** survive into the new format

`environment_vars.item_id` is mandatory post-migration and `vault::migrate_literal_vars_to_items` (gated by `settings['migrated_literals_v1']`) converted legacy literals into real items. Carrying `literal` forward would re-introduce a plaintext-value-in-a-var path that the data model deliberately removed, and would give the receive side two code paths where one suffices.

**Legacy rows that still hold a literal:** they exist only if the migration failed or was skipped. The send side reads through `project::list_projects` → `EnvironmentVar { item_id: i64 }`, which already cannot represent a literal — such rows are invisible to it. Explicit behaviour: **they are silently absent from the bundle**, exactly as they are already absent from every other project-scoped read path. No new handling, no new error. Documented here so nobody "fixes" it later by re-adding the field.

### D4 — Sender sees the keys, never the values, and confirms

**This is the highest-value safety affordance in the feature and it is not in the issue's checklist.** Before upload the GUI shows, per selected environment, the list of `KEY → item name` pairs that will leave the machine, with the count, and a confirm button. No values, ever — the manifest is built from data the frontend already holds via `project_list`, so **no new backend command and no new decryption is required**. The CLI prints the same manifest and requires `--yes` or an interactive `y/N` confirmation.

Rejected: a `project_relay_preview` Tauri command. Rejected because it adds a command and a decryption pass for information the frontend already has.

**Environment selection default:** all non-default environments **unchecked**. Extended reasoning beyond the issue's: the failure mode is asymmetric and unrecoverable. Under-sharing costs one extra round trip; over-sharing puts production credentials in a third-party relay row and in a teammate's vault, and the only remediation is rotating every leaked secret. Additionally, any environment whose name matches `prod|production|live|release` (case-insensitive) renders with a distinct warning treatment and requires the confirm step to name it explicitly. Cost: a heuristic on names, which is inexact — accepted, because it only *adds* friction, never removes it.

### D5 — Receive creates a **new** project; a name collision is a hard error

Rejected: (i) **merge by name** — silently mutating an existing project's environments and repointing vars is destructive, invisible, and would let a sender overwrite a receiver's production values by choosing a matching project name. Rejected on security grounds. (ii) **auto-rename to `MyApp (2)`** — non-destructive, but produces confusing duplicates and hides the collision from the user.

Behaviour: the receive path checks for a case-insensitive project-name match **before writing anything**. On collision it returns a typed error carrying the colliding name, and the caller may retry with `project_name_override` (Tauri) / `--as NAME` (CLI). The GUI catches the error and shows a rename field pre-filled with `"{name}-received"`.

**Interaction with #12:** `idx_projects_name_nocase` already exists on `projects`. Relying on the index alone would surface a raw SQLite `UNIQUE constraint failed` string through the error path — which #12's plan explicitly forbids (no SQL text in responses) and which SQLite's ASCII-only `NOCASE` would miss for non-ASCII names anyway. So the check is an **application-level Unicode-aware pre-check** in the receive function, with the index as the backstop. HTTP maps it to `409 CONFLICT`, error code `CONFLICT`, consistent with #12 step 5. **Sequencing note:** if #12 lands first, reuse its `is_unique_violation` helper rather than duplicating it.

### D6 — Environment `paths` are stripped, not shipped

`paths` are absolute filesystem paths from the sender's machine: `C:\Users\maosuarez\dev\myapp\.env.production`. They are meaningless on the receiver's machine (inject would write to a path that does not exist, or worse, one that does), and they leak the sender's OS, username and directory layout to both the receiver and — as ciphertext the relay operator can size-analyse, though not read — the relay.

Decision: **the field does not exist in `EnvironmentBundle`.** Not "included but ignored", not "offered as a review step" — absent from the type, so it cannot be leaked by a future code path. This matches the existing precedent twice over: `project_export` drops paths deliberately (L431-433) and `handle_workspace_relay_receive` already notes "no paths — receiver sets their own .env targets" (L3002).

Cost: the receiver must set paths per environment before the first inject. Mitigation: the GUI's post-receive toast links straight into the environment editor. Accepted — this is a one-time step and the alternative is a leak.

Rejected: "offer paths as a review step so the receiver can adapt them." Rejected because it makes the leak the default and the redaction opt-out.

### D7 — Item ownership and the transaction boundary

On receive, for a bundle with `I` unique items and `V` total var links across `E` environments:

- `I` rows in `items` — one per unique `item_name`, encrypted with the **receiver's** vault key. `is_global = false`.
- 1 row in `projects`.
- `E` rows in `environments`. Exactly one has `is_default = true` — if the bundle's selected set contains no default (because the sender deselected it), the **first** environment in the bundle is promoted, so the receiver never ends up with a project that has no default environment.
- `I` rows in `item_projects` — every received item is owned by the new project, **not global**. Rationale: a received item's provenance is one project; promoting it to global would make it appear in Global Secrets and be reusable across the receiver's unrelated projects, which is a scope decision only the receiver should make (they can toggle it afterwards with `vault_set_item_global`).
- `V` rows in `environment_vars` — an item linked into 3 environments produces 3 rows pointing at **one** `items` row. This is the dedup assertion in test case 5.
- 0 rows in `project_categories` — categories are the receiver's taxonomy; the bundle does not carry them.

**Transaction boundary: the entire receive is one SQLite transaction.** Today's code is not transactional — `handle_workspace_relay_receive` upserts items one at a time and a failure midway leaves orphans. With a project bundle the orphan blast radius is much larger, and the burn-after-read delete has already fired by then, so the payload is **unrecoverable**. All-or-nothing is not optional here.

**Module decoupling (CLAUDE.md: `db` must not know about `api`; `vault` orchestrates).** The `db` layer must not decrypt or encrypt. Therefore:

1. `vault`/`project` layer encrypts each `PlainItem` with the vault key, producing opaque ciphertext strings.
2. It hands `db` a single plain-data struct (project row + environment rows + `(item_name → ciphertext, item_type, created)` + var links by name) via one new function, e.g. `db::insert_received_project(...) -> Result<InsertedProject, String>`, which opens one transaction, does the name pre-check, inserts everything, and commits.
3. `db` sees ciphertext strings and never a key. `api` and the Tauri command both call the `project`-layer orchestrator, never `db` directly.

**Do not reuse `share::import_plain_items_into_vault` for the item writes.** It is the right helper for the *item* relay path and it owns the collision/skip behaviour there (the code path where #11 found a prior bug), but its contract is "import loose items, optionally linking into one existing environment" — it takes `link: Option<(i64, i64)>`, a single pair. A project bundle needs N environments, a new project, and one transaction spanning all of it. Forcing it through that signature would mean N calls, N implicit transactions, and no rollback. **Reuse its per-item encrypt-and-upsert body by extracting it into a shared private helper**; do not reuse the outer function. Note this explicitly in the PR so a reviewer does not read it as reinvention.

### D8 — Payload size: cap it client-side before the relay rejects it

A whole-project bundle is much larger than a few items. Rough sizing: a `PlainItem` serialises to ~150-600 bytes; AES-256-GCM adds nonce+tag (~28 bytes) and base64 inflates by 4/3. A 200-item project lands around 100 KB plaintext → ~140 KB of base64 in one `text` column and one PostgREST request body. That is comfortably fine. A 5000-item project is ~3.5 MB, which is where Supabase's gateway request-size limit becomes a real risk.

**The exact relay limit is not verifiable from this repo** — there is no `relay_packages` schema in `docs/`, and the setup SQL is user-provided. Therefore do not guess it: **cap on the client**. Refuse to send when the pre-encryption bundle JSON exceeds **1 MiB**, with an actionable error naming the size and suggesting fewer environments. A deterministic local error beats an opaque `relay upload failed (413)` from a third party. Revisit the constant if a real limit is ever documented.

### D9 — No MCP tool for project relay

The issue does not ask for one; the current MCP workspace tools are being deleted. Adding a project equivalent would let an LLM agent push an entire project's decrypted secrets to a third-party relay from a single tool call, with the confirmation affordance from D4 unavailable (an agent cannot meaningfully "confirm" on the user's behalf). CLAUDE.md's MCP rule — *the MCP server does not return secret values* — is about the return direction, but the spirit is that MCP is the lowest-trust surface. Sending is worse than returning.

Decision: **MCP loses two tools and gains none.** If it is wanted later, it is a separate issue with its own consent design.

### D10 — Keep the legacy `workspaces` tables, delete only the relay code

The tables are still the source for the one-time backfill in `db/mod.rs` L209-212, which runs for any user upgrading from a pre-migration install. Dropping them would break that upgrade path. Deleting the *relay handlers* does not touch them. The `db` accessors `list_workspaces` / `get_workspace_vars` become unused by production code after Phase 0 — they remain referenced by the existing migration test at `db/mod.rs` L1325, so they stay, and no `#[allow(dead_code)]` is needed. Verify this with `cargo check` after Phase 0; if a warning does appear, keep the function and annotate rather than delete.

### D11 — Breaking REST change: clean break, no aliases

`POST /workspaces/:id/relay/send` and `POST /workspaces/relay/receive` are **deleted**, not aliased. The issue's "never reached users" claim was verified: `docs/reference.md` L45-46 documents them as legacy and explicitly warns their imports are invisible to the scoped endpoints; `CHANGELOG.md` L34 documents the MCP tools as "unchanged (out of scope, workspace-table-backed)". They are documented, so the change is breaking and belongs in the CHANGELOG — but they are documented *as broken*, and the only shipped consumer is the MCP server in the same repo, updated in the same PR. Aliasing would preserve a data black hole for the sake of a contract nobody can be depending on correctly.

### D12 — Relay code entropy: adequate, but three notes (all out of scope)

The `XXXX-XXXX` code is a **lookup handle, not the confidentiality boundary**. Confidentiality rests on `generate_passphrase` (12 chars over a 62-symbol alphabet ≈ 71 bits) run through Argon2id (m=32 MiB, t=2, p=2). Guessing a code yields ciphertext only, and an offline attack on 71 bits behind that Argon2id cost is not practical. **A whole-project payload does not change this analysis** — the same key protects it.

Three observations, recorded so they are not rediscovered, and **explicitly out of scope for this issue**:

1. **Modulo bias.** Both `generate_share_code` (relay.rs L121) and `generate_passphrase` (`share/crypto.rs` L95) do `ALPHA[(byte as usize) % ALPHA.len()]` over a `u8`. `256 % 36 = 4` and `256 % 62 = 8`, so the first few symbols are marginally over-represented. The entropy loss is a fraction of a bit — cosmetic, not exploitable, but it is a crypto-hygiene defect worth a one-line fix (`rand::seq::IndexedRandom` / rejection sampling) in a separate PR.
2. **Code enumeration.** 36^8 ≈ 2.8 × 10^12 handles, but `relay_download` is an unauthenticated-ish PostgREST `GET` with the anon key. Nothing in this repo rate-limits it. An attacker who enumerates codes harvests ciphertexts for offline passphrase attack. Higher-value payloads make harvesting more attractive even though it does not become more feasible. **Mitigation belongs in the relay's RLS policy / rate limit, not in this codebase** — but the relay setup SQL documentation should say so.
3. **Burn-after-read is best-effort.** `relay_download` filters `retrieved=eq.false` but nothing ever sets `retrieved = true`; the burn is the subsequent `relay_delete`, whose result is discarded (`let _ = ...` at api/mod.rs L2933 and `share_commands.rs`). If the delete fails, the payload stays downloadable until TTL. Also `relay_download` does not filter on `expires_at`, so expiry depends entirely on a server-side purge job. Larger payloads raise the cost of this failure. Record it; do not fix it here.

---

## 4. Implementation steps

Ordered. Each phase is a reviewable PR.

### Phase 0 — Delete the workspace relay surface *(PR 1, independent, ships first)*

`fix(api,mcp,share): remove the workspace relay protocol and its endpoints`

1. `src-tauri/src/share/relay.rs` — delete L57-110: `WorkspaceBundleVar`, `WorkspaceBundle`, `impl WorkspaceBundle`, `encrypt_workspace`, `decrypt_workspace`, and the section comment. Keep everything else (`derive_relay_key`, `encrypt_items`, `decrypt_payload`, `generate_share_code`, the three transport fns, the ISO-8601 helpers).
2. `src-tauri/src/api/mod.rs` — delete `handle_workspace_relay_send`, `handle_workspace_relay_receive`, `WorkspaceRelaySendResponse`, `WorkspaceRelayReceiveResponse`, and router lines L3089-3090. Drop the now-unused `DbWorkspaceVar` import if `cargo check` flags it.
3. `src-tauri/src/bin/crypt-env-mcp.rs` — delete `tool_share_workspace_send` (L~2040-2100) and `tool_share_workspace_receive` (L~2101-2130), their dispatch arms, and their entries in the `tools/list` manifest.
4. `docs/reference.md` — delete rows L45-46, rows L251-252, and the L282 paragraph.
5. `CHANGELOG.md` — Unreleased → Removed: the two endpoints and the two MCP tools, marked **breaking**, with the reason (backed by frozen legacy tables; imports were invisible to every project-scoped surface).
6. `cargo check && cargo clippy` — confirm no orphaned imports and that `list_workspaces` / `get_workspace_vars` are still reachable from the migration test (D10).

**Gate:** `grep -rn "WorkspaceBundle\|workspace_relay\|share_workspace" src-tauri/src src/ docs/` → zero hits.

### Phase 1 — Protocol + core orchestration + tests *(PR 2)*

`feat(share,project,db): project relay bundle format and receive orchestration`

7. `src-tauri/src/share/relay.rs` — add the D1/D2 section: `ProjectBundleVar { key, item_name }`, `EnvironmentBundle { name, is_default, vars }`, `ProjectBundle { kind, version, name, description?, template, environments, items }`, `ProjectBundle::{KIND, VERSION}`, `encrypt_project`, `decrypt_project` (kind check **then** version check, mirroring the existing L104-108 guard, with a message that points at the items receive flow).
8. `src-tauri/src/db/mod.rs` — add `insert_received_project(...)` per D7: one `BEGIN`/`COMMIT`, Unicode-aware project-name pre-check first, then project → environments → items (ciphertext in, no key) → `item_projects` → `environment_vars`. Returns inserted ids and counts. **No crypto, no knowledge of `api` or `vault`.**
9. `src-tauri/src/project/mod.rs` (or a new `src-tauri/src/project/relay.rs` if `mod.rs` is getting long) — two orchestrators shared by the HTTP handler and the Tauri command:
   - `build_project_bundle(db, vault_key, project_id, environment_ids) -> Result<ProjectBundle, String>` — reads the project via `list_projects`, filters to the selected environments, decrypts each referenced item once, dedups by name (`HashMap<String, PlainItem>` entry-API, same shape as api/mod.rs L2711), skips dangling `item_id`s, enforces the D8 size cap.
   - `receive_project_bundle(db, vault_key, bundle, name_override) -> Result<ReceivedProject, String>` — default-environment promotion, encrypt each item with the receiver's key, single call into `insert_received_project`.
10. `src-tauri/src/share/mod.rs` — extract the per-item encrypt-and-upsert body out of `import_plain_items_into_vault` (L656+) into a private helper both paths share (D7). Do not change `import_plain_items_into_vault`'s public behaviour — #11's tests cover it.
11. Tests: `share/relay.rs` `mod tests` (cases 1-4) + `src-tauri/tests/project_relay.rs` (cases 5-11), on the #11 harness.

**Gate:** `cargo test` green, cases 5 and 6 passing.

### Phase 2 — HTTP + Tauri + GUI *(PR 3)*

`feat(api,project,ui): share a whole project via the encrypted relay`

12. `src-tauri/src/api/mod.rs` — `handle_project_relay_send` / `handle_project_relay_receive`, following the existing relay handlers' structure (settings lookup → `spawn_blocking` for the blocking reqwest calls → typed error json). Routes `POST /projects/:id/relay/send`, `POST /projects/relay/receive`. Collision → `409 CONFLICT`. Oversize → `413` or `422` with the D8 message. **No secret values in any response** — send returns `{code, passphrase, project, environment_count, item_count}`; receive returns `{project, environments: [names], item_count}`.
13. `src-tauri/src/project/relay_commands.rs` (new) — `project_relay_send`, `project_relay_receive`, modelled on `vault/share_commands.rs` L212/L283 including the `guard.touch()` calls and the `DEFAULT_RELAY_URL`/`DEFAULT_RELAY_ANON_KEY` fallbacks. **Note the existing inconsistency:** the Tauri commands fall back to bundled defaults while the HTTP handlers hard-fail with `NOT_CONFIGURED`. Match the Tauri side for the new Tauri commands and the HTTP side for the new handlers — do not "fix" the divergence here; that is its own issue.
14. `src-tauri/src/lib.rs` — register both in `invoke_handler` next to the existing project commands (~L211-213) and add the `use` entries (~L31-32).
15. `src/components/ProjectShareModal.tsx` (new) — environment checklist (non-default unchecked, prod-named highlighted), the D4 key manifest built from `useProjectStore` data, confirm step, then the code/passphrase display **extracted from `ShareModal.tsx`**. Receive side: code + passphrase inputs, and a rename field revealed on `CONFLICT`. `invoke()` only; Tailwind only.
16. `src/components/ShareModal.tsx` — extract the code/passphrase display and the relay security note (L980-1060 region) into a shared component so both modals use one implementation. Behaviour unchanged.
17. `src/components/ProjectManager.tsx` — "SHARE PROJECT" on the project detail view (next to the existing export button, L676), "RECEIVE PROJECT" on the projects list (next to import, L685). Refresh `projectStore` + vault items after a successful receive.

**Gate:** manual round trip between two vaults on Windows per `CLAUDE.local.md`; confirm the receiver's items are project-owned and absent from Global Secrets.

### Phase 3 — CLI + docs *(PR 4)*

`feat(cli,docs): crypt-env project share / receive`

18. `src-tauri/src/bin/crypt-env/commands/project.rs` — add `Share { id/name, envs: Option<String>, yes: bool }` and `Receive { code, passphrase, as_name: Option<String> }` to `ProjectCmd` (L16) and the `run` match. Follow `commands/relay.rs` for the `authenticated_post` calls and reuse its code/passphrase box output verbatim (L78-86). `--envs` omitted means **default environment only**, matching D4's GUI default; `--envs all` is the explicit opt-in. Print the D4 manifest and require `y/N` unless `--yes`.
19. `docs/reference.md` — new REST rows for the two `/projects/...` routes with request/response examples; CLI section updated.
20. `CHANGELOG.md` — Added entry for the feature.

**Gate:** `crypt-env project share` → `crypt-env project receive` round trip against a second vault.

---

## 5. Trade-offs and alternatives considered

### 5.1 The big one: build vs. delete-only

| Option | Cost | What you get | Verdict |
|---|---|---|---|
| **Delete only (Phase 0)** | ~1 day | Issue's stated complaint closed; broken agent-reachable path removed; ~250 lines gone | **Ships regardless.** Legitimate stopping point. |
| **Build it (Phases 0-3, no MCP)** | ~4-5 days, 6 files of new backend + 3 of frontend | One-step project onboarding, per-environment key manifest, safe defaults | **Recommended.** |
| Build it including MCP | +1 day | An agent can exfiltrate a whole project in one call | **Rejected (D9).** |
| Do nothing | 0 | Broken endpoints stay live | Rejected. |

The honest risk with "build it": this is a **six-surface feature for a workflow that already has a two-step workaround**. If the user base is one person (it currently is), the payback is thin. That is why Phase 0 is separable and why MCP is dropped — the plan is structured so that stopping after any phase leaves the repo coherent.

### 5.2 Format alternatives

- **Extend `ExportedProject` with optional values** — one format instead of two. Rejected (D1): risks writing values into a `.cryptenv-proj` file, whose entire contract is that it never carries them.
- **Keep `kind: "workspace"` and just add fields** — no new discriminator, smaller diff. Rejected: it is precisely the format-confusion the issue asks to avoid, and the old receiver would parse the new payload's shared fields and silently drop the environments.
- **Ship items nested per environment** — simplest builder. Rejected (D1): duplicated ciphertext, dedup information destroyed.

### 5.3 Receive-semantics alternatives

- **Merge into the existing project by name** — the most "convenient" option, and the most dangerous: a sender chooses the project name, so a sender chooses which of the receiver's projects to mutate. Rejected on security grounds (D5).
- **Auto-rename on collision** — non-destructive but confusing; hides the collision. Rejected in favour of an explicit error plus a caller-supplied override.
- **Receive items as global** — would make them immediately reusable. Rejected (D7): scope is the receiver's decision, and global items surface in Global Secrets where their provenance is invisible.

### 5.4 Transport alternatives

- **Chunk large bundles across multiple relay rows** — removes the size ceiling. Rejected: multi-row burn-after-read, partial-download and partial-burn semantics are a meaningful protocol expansion for a limit that a 1 MiB cap makes unreachable in practice (D8).
- **Compress before encrypting** — would raise the effective ceiling several-fold and is cheap. Rejected *for now*: compression before encryption leaks plaintext-size structure via ciphertext length, and the cap makes it unnecessary. Revisit only if real users hit the cap.

### 5.5 What breaks first, and what would make this decision wrong

- **Breaks first at scale:** the D8 size cap, on a project with thousands of variables. Warning sign: users reporting the cap error. Fix: compression (5.4) or chunking.
- **Breaks first in correctness:** name-based item matching (D1). Two distinct items sharing a name collapse into one. Warning sign: a receiver reporting a missing variable that "was in the send". Mitigation: the D4 manifest shows the deduped list, so the sender can see the collapse before uploading.
- **Would invalidate this plan:** if the `workspaces` tables turn out to still be written by some path this investigation missed, D10 and Phase 0 both need revisiting — `grep -rn "upsert_workspace\|set_workspace_vars"` was run and returned only `api/mod.rs`, but re-run it at implementation time.
- **Would invalidate D9:** an explicit product decision that MCP agents are trusted to initiate outbound secret transfer. That is a product decision, not an architectural one.
- **Reversibility:** Phase 0 is a pure deletion, recoverable from git. Phases 1-3 are additive — the new endpoints, commands and UI can be removed without touching the data model, since receiving only ever *creates* normal `projects`/`environments`/`items` rows that the rest of the app already owns. There is **no schema migration in this plan**, which is what makes it cheap to unwind.

---

## 6. Sequencing dependencies

| Depends on | Why | If it lands later |
|---|---|---|
| #11 (test harness) | Cases 5-11 use `test_support::{unlocked_vault, seed_project, seed_item, link_var}` | Phase 1 blocks on it, or writes a throwaway local fixture and refactors onto the harness afterwards — prefer blocking |
| #12 (`nocase` uniqueness) | D5's 409 mapping and the `is_unique_violation` helper | Implement the application-level pre-check regardless; adopt #12's helper as the backstop when it lands |

Phase 0 depends on nothing and should not wait for either.
