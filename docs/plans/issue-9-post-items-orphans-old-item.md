# Issue #9 — `POST /items` orphans the old item on key collision

Status: plan only. No code written yet.
Scope: `src-tauri/src/api`, `src-tauri/src/vault`, `src-tauri/src/db`, CLI `add`, MCP tool
descriptions, `docs/reference.md`, `src-tauri/tests/vault_integration.rs`.

---

## 1. Objective

### 1.1 Current behaviour (the thing being changed)

`handle_create_item` (`src-tauri/src/api/mod.rs:572-648`) always calls
`vault::create_project_item`, which always `INSERT`s. `db::upsert_environment_var`
(`src-tauri/src/db/mod.rs:1152-1180`) then *repoints* the `UNIQUE(environment_id, key)`
row at the new item. The previously linked item survives with:

- its row in `items` (still encrypted with the live vault key),
- its rows in `item_projects` (still "owned"),
- zero rows in `environment_vars`,
- full reachability via `GET /items/:id`, `PUT /items/:id`, `POST /items/:id/reveal`,
- inclusion in `vault_export_backup` and every share/export path.

It is invisible in `GET /items?project=…&environment=…` because that endpoint filters by
`environment_vars`. Invisible and readable is the worst pair.

### 1.2 Target semantics

`POST /items` gains an optional query parameter `on_conflict` with three values.
A *collision* means: the resolved `key` (from `body.key`, else `body.item.name`) already has
an `environment_vars` row in the resolved environment.

| `on_conflict` | No collision | Collision, **exclusive** item | Collision, **shared** item |
|---|---|---|---|
| `update` (default) | create → `201` | re-encrypt onto the existing `items.id` → `200` | `409` `SHARED_ITEM_CONFLICT` |
| `replace` | create → `201` | create new, repoint link, delete old item + its `item_projects` rows → `201` | create new, repoint link, old item left intact (still referenced) → `201` |
| `error` | create → `201` | `409` `KEY_EXISTS` | `409` `KEY_EXISTS` |

**Exclusive** item (safe to mutate in place) is defined as *all three* holding for the
currently linked `item_id`:

1. `items.is_global = 0`,
2. exactly one row in `environment_vars` references it (the colliding one),
3. its `item_projects` owner set is exactly `{env.project_id}`.

Anything else is **shared**: mutating it in place would change the value seen by another
environment, another project, or the Global Secrets surface.

Additional guarantees:

- The whole "create-or-update item + own it + link it" sequence commits or rolls back as
  one SQLite transaction. There is no interleaving that leaves an item owned but unlinked.
- On the `update` branch the item keeps its `id`, its `created`, its `is_global`, and its
  full `item_projects` owner set. Only the encrypted payload and `items.updated` change.
- The response body stops hard-coding `is_global: false` (`api/mod.rs:646`) and reports the
  value actually persisted.
- No branch ever returns a plaintext secret; the response stays `redact_item`-filtered.

### 1.3 Definition of done — measurable

Done when all of the following are true and covered by a named test in
`src-tauri/tests/vault_integration.rs`:

| # | Test name | Assertion |
|---|---|---|
| 1 | `test_post_items_same_key_twice_updates_in_place` | Posting `API_KEY=first` then `API_KEY=second` to the same project+environment leaves **exactly 1** row in `items` and **exactly 1** row in `environment_vars` for that `(environment_id, key)`. Both responses carry the **same** `id`. First response is `201`, second is `200`. |
| 2 | `test_post_items_update_reveals_only_new_value` | After the two posts of test 1, `POST /items/<id>/reveal` returns `"second"` and no id in the vault reveals `"first"`. |
| 3 | `test_post_items_replace_deletes_superseded_item` | With `?on_conflict=replace`, the second post returns `201` with a **new** id. `GET /items/<first-id>` returns `404`, `POST /items/<first-id>/reveal` returns `404`, and `SELECT COUNT(*) FROM item_projects WHERE item_id = <first-id>` is `0`. |
| 4 | `test_post_items_replace_keeps_still_referenced_item` | Same key linked in two environments to one global item; `?on_conflict=replace` in env A leaves the original item alive and still linked in env B. |
| 5 | `test_post_items_conflict_on_global_item` | Default mode against a key linked to an `is_global = 1` item returns `409` with `error_code = "SHARED_ITEM_CONFLICT"`, and **nothing is written** — item count, link count, and the existing ciphertext are unchanged. |
| 6 | `test_post_items_conflict_on_multi_linked_item` | Same as 5 but for an item referenced by two `environment_vars` rows. |
| 7 | `test_post_items_conflict_on_multi_owned_item` | Same as 5 but for an item with two `item_projects` rows. |
| 8 | `test_post_items_on_conflict_error_rejects_any_collision` | `?on_conflict=error` returns `409` `KEY_EXISTS` even for an exclusive item; item count unchanged. |
| 9 | `test_post_items_new_key_returns_201` | No collision → `201`, item count `+1`, link count `+1`. Unchanged from today. |
| 10 | `test_post_items_invalid_on_conflict_is_422` | `?on_conflict=nonsense` → `422` `VALIDATION_ERROR`, nothing written. |
| 11 | `test_update_in_place_preserves_created_owners_and_global` | After an `update`-branch write, `items.created` equals the original, `items.is_global` is unchanged, and `list_owning_projects` returns the same set. |
| 12 | `test_link_item_rolls_back_when_link_fails` | Db-level: force the link step to fail inside `db::create_or_link_item` (invalid `environment_id`) → `items` gains **no** row and `item_projects` gains **no** row. |
| 13 | `test_list_orphan_items_finds_unlinked_nonglobal` | An item with no `environment_vars` row and `is_global = 0` appears in `db::list_orphan_item_ids()`. |
| 14 | `test_list_orphan_items_excludes_global_and_linked` | A global unlinked item and a linked non-global item both absent from the same query. |
| 15 | `test_prune_orphan_items_clears_item_projects` | `vault::prune_orphan_items(&[id])` deletes the `items` row **and** its `item_projects` rows, and leaves non-listed items untouched. |

Plus, non-test acceptance criteria:

- `cargo clippy -- -D warnings` clean; no `unwrap()` / `expect()` added on a production path.
- `docs/reference.md` rows 14, 72(3), 150 and the MCP tool table updated (§3.7).
- The reproduction loop from the issue, run twice against a fresh vault, produces exactly
  one `items` row.

---

## 2. What is being mitigated

Two distinct defects share one root cause. State each as a checkable proposition.

**M1 — Secret retention across rotation.**
Today, the natural rotation gesture (post the key again with a new value) leaves the old
ciphertext in the vault, decryptable with the live key by anyone holding the API/MCP token
who enumerates ids, and included in every backup and export. The vault silently retains
every superseded credential it has ever held.
*Checkable:* after rotating a key N times, `POST /items/:id/reveal` must not return any
superseded value for any id in the vault. Tests 2 and 3.

**M2 — Unbounded, invisible growth.**
Every repeated `crypt-env add KEY=…`, every re-run of an agent's "store the API key" step,
and every `crypt_env_import_env_file` over a changed `.env` appends a dead encrypted row
that no scoped listing shows. Growth is unbounded and the user has no surface on which to
notice it. `decrypt_all_items` runs a full decryption pass per authenticated request
(`docs/reference.md:50`), so dead rows are also a permanent per-request cost.
*Checkable:* the reproduction loop leaves exactly one `items` row. Test 1.

**M3 (secondary) — Non-atomic create+link.**
A failure between `create_project_item` and `upsert_environment_var` produces the same
orphan by a different route: owned, unlinked, invisible, readable. Today this window is
real — they are two independent awaits with no transaction.
*Checkable:* test 12.

**Not mitigated by this work** (stated so the boundary is explicit):

- Pre-existing orphans already in users' vaults. Addressed by the report/prune path in
  §3.6, which is deliberately **not** automatic.
- Orphans produced by *other* routes: `project::save_environment` →
  `db::set_environment_vars` (`db/mod.rs:1122`) deletes and re-inserts the whole var set, so
  removing a variable in the GUI unlinks its item without deleting it. Scoped out on
  purpose — see §4.5.
- `POST /share/import`, `POST /relay/receive` and the GUI backup-import path can create
  unlinked items (`docs/reference.md:35, 46`). Also not orphan-cleanup targets; see §3.6's
  warning about the prune predicate.

---

## 3. Implementation steps

Ordered. Each step compiles and is independently reviewable.

### 3.0 Architectural placement (decide before writing anything)

Three constraints collide and the resolution is forced:

1. `VaultDb.pool` is private (`db/mod.rs:74`). Neither `api` nor `vault` can call
   `pool.begin()`. A real transaction can only be opened **inside** `db`.
2. CLAUDE.md: `db` must not know about `api`. So `db` cannot return HTTP status codes; it
   returns a plain Rust enum and `api` maps it.
3. The vault key and `encrypt_item` live in `vault` (`vault/mod.rs:121`, `pub(crate)`).
   `db` must never see a `CryptoKey`.

Therefore:

- **`db`** gains one read-only inspection method and one transactional mutation method.
  The mutation method receives an *already-encrypted* blob and a mode enum, performs the
  whole read-classify-write inside a single `sqlx::Transaction`, and returns an outcome
  enum. It knows nothing about HTTP, nothing about crypto.
- **`vault`** gains the orchestrator: inspect → decide → encrypt (preserving `created` on
  the update branch) → call the single db method. It is the only layer holding the key.
- **`api`** parses `on_conflict`, calls the vault orchestrator, maps the outcome enum to
  `200` / `201` / `409`, and redacts. It contains no SQL.

This is the only split that satisfies all three constraints. The tempting shortcut —
ad-hoc `sqlx` statements in the handler — is blocked by the private pool *and* by the
module rule, which is a good sign the rule is doing its job.

### 3.1 `db` — read-only conflict inspection

New in `src-tauri/src/db/mod.rs`, near `upsert_environment_var` (~L1150):

```rust
pub struct EnvKeyConflict {
    pub item_id: i64,
    pub created: String,
    pub is_global: bool,
    pub link_count: i64,   // rows in environment_vars pointing at item_id
    pub owner_ids: Vec<i64>,
}

pub async fn inspect_env_key(
    &self,
    environment_id: i64,
    key: &str,
) -> Result<Option<EnvKeyConflict>, String>
```

One `SELECT` joining `environment_vars` → `items`, plus the link/owner counts. Returns
`None` when the key is free, or when the existing row is a legacy `literal`-only var with
`item_id IS NULL` (treat legacy literals as "no item to update" → the create path runs and
`upsert_environment_var`'s existing repoint replaces the literal, which is the correct
upgrade and matches `migrate_literal_vars_to_items`).

Never selects `items.data`. No ciphertext leaves this call.

### 3.2 `db` — the single transactional mutation

Also in `src-tauri/src/db/mod.rs`:

```rust
pub enum LinkMode { Update, Replace, Error }

pub enum LinkOutcome {
    Created { item_id: i64, is_global: bool },
    Updated { item_id: i64, is_global: bool },
    Conflict { item_id: i64, reason: ConflictReason },
}

pub enum ConflictReason { Shared, KeyExists, StateChanged }

pub async fn create_or_link_item(
    &self,
    environment_id: i64,
    project_id: i64,
    key: &str,
    item_type: &str,
    encrypted: &str,   // produced by vault::encrypt_item
    created: &str,
    mode: LinkMode,
    expected: Option<i64>,  // item_id vault saw during inspect; None = expected free
) -> Result<LinkOutcome, String>
```

Transaction body (`let mut tx = self.pool.begin().await?` … `tx.commit().await?`):

1. Re-run the inspection **inside** the transaction. If the current `item_id` differs from
   `expected`, roll back and return `Conflict { StateChanged }`. This is optimistic
   concurrency: cheap, and it makes the classification decision (taken in `vault`) provably
   still valid at write time. (In practice all writers serialize on the process-wide
   `SharedState` mutex — `ApiState.vault` is the same `SharedState` the Tauri commands use,
   `api/mod.rs:33` — so this guard is defence in depth, not the primary mechanism.)
2. Branch:
   - free key → `INSERT INTO items`, `INSERT OR IGNORE INTO item_projects`,
     `INSERT INTO environment_vars` → `Created`.
   - collision + `Error` → rollback, `Conflict { KeyExists }`.
   - collision + `Update` + exclusive → `UPDATE items SET data, updated WHERE id`
     (`created` and `is_global` untouched) → `Updated`.
   - collision + `Update` + shared → rollback, `Conflict { Shared }`.
   - collision + `Replace` → `INSERT` new item, own it, `UPDATE environment_vars SET
     item_id = new` for that row, then **conditionally** delete the old item:
     `DELETE FROM items WHERE id = old AND is_global = 0 AND NOT EXISTS (SELECT 1 FROM
     environment_vars WHERE item_id = old)`, followed by
     `DELETE FROM item_projects WHERE item_id = old AND NOT EXISTS (SELECT 1 FROM items
     WHERE id = old)`. The `NOT EXISTS` guards make "delete only if now unreachable"
     a property of the statement, not of application logic that could drift → `Created`.

Note the transaction is what makes the `Replace` delete safe: repoint and delete are one
atomic step, so there is no instant where the old item is both unlinked and undeleted.

Important detail: `encrypt_item` strips `is_global` before encrypting (`vault/mod.rs:123`)
and `decrypt_item(&key, id, &data, is_global)` re-injects `id` and `is_global` from the row
(`api/mod.rs:162`). The ciphertext is therefore id-independent and is-global-independent —
a blob built for a create can be written onto an existing row without corruption. This is
what makes the `Update` branch possible without decrypting anything.

The `created` field, however, **is** inside the blob and is *not* rewritten by the update
branch of `upsert_item` (`db/mod.rs:324`). `vault` must therefore stamp the existing
`created` into the item before encrypting on the `Update` branch (§3.3), or the row column
and the blob will disagree.

### 3.3 `vault` — the orchestrator

In `src-tauri/src/vault/mod.rs`, next to `create_project_item` (L286-302), which stays as-is
for the GUI `vault_create_project_item` path (it does not link into an environment and
therefore cannot collide on an env key):

```rust
pub enum UpsertOutcome {
    Created(VaultItem),
    Updated(VaultItem),
    Conflict { item_id: i64, reason: ConflictReason },
}

pub async fn create_or_update_env_item(
    db: &VaultDb,
    key: &CryptoKey,
    item: &VaultItem,
    project_id: i64,
    environment_id: i64,
    env_key: &str,
    mode: LinkMode,
) -> Result<UpsertOutcome, String>
```

Body:

1. `let conflict = db.inspect_env_key(environment_id, env_key).await?;`
2. Classify exclusive vs shared from `conflict` (`!is_global && link_count == 1 &&
   owner_ids == [project_id]`). Short-circuit the `Error` mode and the shared-under-`Update`
   case here without touching the key — no need to encrypt to return a conflict.
3. Build the item to persist. On the `Update` branch, override `created` with
   `conflict.created` so the blob matches the row. On create/replace, keep the caller's
   `created` (already defaulted to `now_ts_str()` by the handler).
4. `let encrypted = encrypt_item(key, &to_persist)?;`
5. `db.create_or_link_item(..., expected = conflict.map(|c| c.item_id)).await`
6. Map `LinkOutcome` → `UpsertOutcome`, filling `id` and `is_global` from the outcome.

Errors stay `Result<_, String>` to match the module's existing convention. No secret value
is ever formatted into an error string — the only identifiers in play are ids and key names.

### 3.4 `api` — handler rewrite

`src-tauri/src/api/mod.rs`, `handle_create_item` (L572-648):

- Extend the query extractor. Today it is `Query(scope): Query<EnvScopeQuery>`; add
  `on_conflict: Option<String>` to `EnvScopeQuery` (it is only used by the scoped
  endpoints, all of which tolerate an extra optional field) **or**, cleaner, add a second
  `Query<ConflictQuery>` extractor. Prefer extending `EnvScopeQuery` only if no other
  endpoint would then silently accept a meaningless `on_conflict`; otherwise use the second
  extractor. Parse to `LinkMode` with a `422 VALIDATION_ERROR` on an unknown value
  (test 10), default `LinkMode::Update`.
- Keep the existing order: `verify_token` → `resolve_scope` → `validate_create` → key
  resolution (L603-611) → `created` defaulting.
- Replace the L618-644 block with a single `crate::vault::create_or_update_env_item(...)`
  call under the existing `state.vault.lock().await`.
- Map outcomes:
  - `Created(item)` → `(StatusCode::CREATED, Json(redact_item(item)))`
  - `Updated(item)` → `(StatusCode::OK, Json(redact_item(item)))`
  - `Conflict { item_id, Shared }` → `409`, `err_code = "SHARED_ITEM_CONFLICT"`, message
    naming the *key* and the *item id* and the remedy (`?on_conflict=replace`, or
    `PUT /items/:id` to change the shared value everywhere). Never the value.
  - `Conflict { item_id, KeyExists }` → `409`, `err_code = "KEY_EXISTS"`.
  - `Conflict { StateChanged }` → `409`, `err_code = "CONFLICT_RETRY"`.
  - `Err(e)` → `500 INTERNAL_ERROR`, unchanged.
- Delete the `body.item.is_global = Some(false)` hard-code at L646; take the value from the
  outcome. (Under the new rules an `Updated` item is always non-global anyway — the global
  case 409s — but the hard-code is a latent lie and should not survive the rewrite.)
- `err_json` needs no change; `409` is expressible today.

`ConflictReason` must be re-exported or mirrored so `api` does not import from `db`
directly if that would violate the layering intent. Prefer: `vault` re-exports its own
`ConflictReason` and `api` matches on the `vault` type only.

### 3.5 Callers

- **CLI `add`** (`src-tauri/src/bin/crypt-env/commands/add.rs:130-150`): the POST loop
  currently treats any non-2xx as a generic failure. With the default `update` mode the
  existing "The following keys already exist / Update all conflicting keys?" prompt (L118-127)
  becomes *truthful* — today it says "update" and performs an orphaning create. Changes
  needed: (a) when the user declines the update prompt, send `?on_conflict=error` so a
  racing collision is rejected rather than silently created; (b) surface `409` distinctly —
  print the key name and the conflict reason, not `HTTP 409`. `--force` keeps the default
  `update`. No new flag unless a user asks for `replace`; do not add speculative surface.
- **CLI `set`** (`commands/set.rs`, 27 lines): confirm it routes through `PUT /items/:id`;
  if it posts, apply the same treatment.
- **MCP `crypt_env_add_item`** (`src-tauri/src/bin/crypt-env-mcp.rs:278` schema,
  `append_scope_params` at :798): add an optional `on_conflict` property and, more
  importantly, rewrite the tool **description** — it is the only thing the model reads.
  It must state: "If the key already exists in the target environment, the existing item is
  updated in place (its previous value is destroyed). If that item is shared with other
  environments or projects, the call fails instead of silently changing it elsewhere."
- **MCP `crypt_env_import_env_file`** (:690, handler ~:2810-2916): it already has an
  `overwrite` boolean whose current implementation skips or updates by *name*. Map
  `overwrite: true` → `on_conflict=update` and `overwrite: false` → `on_conflict=error`,
  and treat a `409` as a skip in the report. This removes the bulk-import path's ability to
  mass-produce orphans, which is where the worst growth comes from.
- **GUI**: `EditItem.tsx` / `ProjectManager.tsx` use the Tauri command
  `vault_create_project_item`, which does not link into an environment and cannot collide on
  an env key. No GUI change is required for issue #9 itself. The GUI's own orphan route is
  `environment_save` → `set_environment_vars`; see §4.5.

### 3.6 One-off orphan cleanup — report by default, delete only on explicit confirmation

**Recommendation: report everywhere, delete only from the GUI.** Justification below.

- `db::list_orphan_item_ids()` — the issue's query, verbatim:
  ```sql
  SELECT i.id FROM items i
  LEFT JOIN environment_vars ev ON ev.item_id = i.id
  WHERE ev.id IS NULL AND i.is_global = 0
  ```
  Returns ids only. A companion `db::delete_items_cascade(&[i64])` runs one transaction
  deleting from `item_projects` then `items` for the given ids, re-checking the orphan
  predicate per id inside the transaction (so a concurrently re-linked item is skipped).
- `vault::list_orphan_items(db, key) -> Vec<VaultItem>` decrypts *only* those ids and
  returns them `redact`-shaped (id, type, name, created) so the user can judge what they are
  about to delete. Values never leave the process.
- **Tauri commands** `vault_list_orphan_items` and `vault_prune_orphan_items(ids: Vec<i64>)`,
  registered in `lib.rs`, surfaced in Settings next to the existing destructive operations
  (backup / wipe), with a per-item checklist and a typed confirmation.
- **REST**: add `GET /maintenance/orphans` (token, read-only, redacted) so
  `crypt-env doctor` can print `[!!] Orphaned items    3 unreferenced items (Settings →
  Maintenance to review)`. **Do not** add a REST prune endpoint. A static MCP token should
  not be able to bulk-delete vault rows; the destructive half stays behind the GUI's
  confirmation, which is where every other destructive vault operation already lives.
- **No startup migration.** Deleting user data on unlock, before the user has seen what is
  being deleted, is exactly the class of action CLAUDE.md's autonomy rules reserve for
  explicit consent. It is also unsafe on its own terms: the predicate matches items produced
  by `POST /share/import`, `POST /relay/receive`, and the GUI backup-import path, which land
  unlinked and non-global (`docs/reference.md:35, 46`) — a freshly received share would be
  silently destroyed. Report-then-confirm is not caution theatre here; it prevents a real
  data-loss bug.

**Secret hygiene on delete.** Rather than the ad-hoc zero-then-delete used for files
(`TempEnvFile`, `db::wipe_and_reset`), add `PRAGMA secure_delete=ON` to the pragma list at
`db/mod.rs:97-98`. SQLite then zeroes freed page content on every delete, which covers
`delete_item`, `delete_project`'s cascades, the `Replace` branch, and the prune path
uniformly. A per-call `UPDATE items SET data = <zeros>` before `DELETE` is strictly worse:
it only covers the call sites someone remembered, and SQLite may relocate the row anyway.
Write cost is negligible at this vault's scale. Note this changes behaviour for *all*
deletes — call it out in the commit and in `docs/reference.md`.

### 3.7 Documentation

`docs/reference.md`:

- Row 14 (`POST /items`): document `on_conflict`, the `200` vs `201` distinction, the
  exclusive/shared classification, and `409` `SHARED_ITEM_CONFLICT` / `KEY_EXISTS`.
- "Known, deferred issues" item (3) at L72: remove — it describes exactly this bug.
- CLI note at L150: rewrite to describe update-in-place.
- MCP tool table (L219-224): add the `crypt_env_add_item` conflict behaviour and the
  `crypt_env_import_env_file` `overwrite` → `on_conflict` mapping.
- New rows for `GET /maintenance/orphans` and the two Tauri maintenance commands.
- Note the `secure_delete` pragma under the security notes.

### 3.8 Suggested commit sequence

1. `db`: `inspect_env_key` + `create_or_link_item` + `list_orphan_item_ids` +
   `delete_items_cascade` + `secure_delete` pragma, with tests 12-15.
2. `vault`: `create_or_update_env_item` + orphan helpers.
3. `api`: handler rewrite + `GET /maintenance/orphans`, with tests 1-11.
4. CLI `add` / `set` + MCP schemas and descriptions.
5. Tauri commands + Settings maintenance panel.
6. `docs/reference.md`.

Steps 1-3 are the fix. Steps 5-6 are the cleanup surface and can ship separately if the
Settings UI needs design time; step 4 must not lag behind step 3 by a release, or the MCP
descriptions will describe semantics the server no longer has.

### 3.9 Test harness

Tests 1-11 need an HTTP-level harness (bound router + token + unlocked temp vault).
**Issue #11's plan owns that harness — consume it, do not build a second one.** If it has
not landed when this work starts, sequence #11 first rather than forking a competing
fixture. Tests 12-15 are pure `VaultDb` tests and use the existing
`tempdir()` + `VaultDb::open` pattern already in `src-tauri/tests/vault_integration.rs`, so
they can proceed independently.

---

## 4. Trade-offs and alternatives considered

### 4.1 Default mode: `update` vs `error` vs `replace`

**Chosen: `update`.**

- The user-facing verb is already "update". CLI `add` literally prompts *"Update all
  conflicting keys?"* (`add.rs:124`) and then performs an orphaning create. The MCP import
  tool advertises `overwrite`. Making the server do what both surfaces already claim is a
  bug fix, not a semantic change.
- It is the only default that actually satisfies M1: rotation destroys the old ciphertext.

*Rejected: default `error`.* Strictly safest and the most honest reading of "POST means
create". Rejected because it breaks `crypt-env add KEY=newvalue` — today's most common
gesture — for every existing user, and pushes them toward a two-call
`GET /items` + `PUT /items/:id` dance that the CLI would then have to implement anyway. It
is available as an opt-in for scripts that want strict create semantics.

*Rejected: default `replace`.* Achieves M1 and M2 but churns the item id on every write.
Any external reference to the id (an MCP conversation, a script, the TUI's cached list)
silently goes stale. Update-in-place is the same outcome with a stable identity.

**Cost of `update`:** `POST` is no longer purely a create, which is a small REST purity
violation. Mitigated by the explicit `200`/`201` distinction and by keeping `error` available.

### 4.2 Dropping the issue's `replace`-always semantics

The issue proposed `replace` as "create a new row and delete the orphan". The plan keeps
`replace` but changes its delete to **conditional** (`NOT EXISTS` guards). Reason: an
unconditional delete would destroy a *shared* item that other environments still reference —
turning a fix for silent data retention into a cause of silent data loss. The conditional
form is strictly safer and is a no-op difference in the exclusive case.

### 4.3 The exclusive/shared split (the crux)

The blast radius of update-in-place is not uniform. `item_projects` is many-to-many and a
global item can be linked from several environments. Posting a new `API_KEY` value in
`production` must not silently rewrite the value that `staging` and every other project sees.

Three ways to handle the shared case:

1. **`409` and make the user choose** (chosen). Loud, reversible, teaches the model
   (`SHARED_ITEM_CONFLICT` is self-describing), never destroys anything.
2. *Auto-fork:* create a private copy and repoint the link. Rejected: it silently detaches
   the environment from the shared item. The next rotation of the global item stops
   propagating and nobody is told. Silent semantic divergence in a secrets manager is worse
   than a failed call. Available explicitly as `on_conflict=replace`.
3. *Update anyway:* rejected outright — it changes a value in projects the caller never
   named.

**Cost of the `409`:** a rougher first experience for the exact user who has been
consciously using global items, and one more failure mode an MCP agent must handle. Accepted:
the alternative is an unannounced cross-project write.

### 4.4 Full field replacement vs merge on the update branch

**Chosen: the POST body fully defines the item's fields** (with `id`, `created`,
`is_global`, and ownership preserved). `validate_create` already requires `name`, `type`,
and a non-empty `value`, so a POST always carries a complete item. `PUT /items/:id` is the
merge endpoint and stays the merge endpoint; having two endpoints with different, subtly
overlapping merge rules is a worse outcome than one clear rule per endpoint.

*Rejected: merge like `handle_update_item` (L692-712).* It would require decrypting the
existing item on every collision — pulling the vault key and a `decrypt_all_items` pass into
what is currently a pure ciphertext-in operation, and dragging crypto into the transactional
path. The simplification is real and load-bearing for §3.2.

**Cost:** an agent that posts a partial item (name + value only) onto an existing item wipes
its `notes` / `categories`. This is the sharpest edge in the plan. Mitigations: document it
in the MCP tool description; consider a follow-up that returns the pre-update field names
that were cleared. If this bites in practice, switching to merge is a contained change
inside `create_or_update_env_item` and does not touch the db layer's contract.

### 4.5 GUI orphan route left out of scope

`project::save_environment` → `db::set_environment_vars` (`db/mod.rs:1122-1145`) deletes and
re-inserts an environment's entire var set, so removing a variable in `ProjectManager.tsx`
unlinks its item without deleting it — the same orphan class. Deliberately not fixed here:
whether "remove this variable from the environment" should also destroy the underlying
secret is a product decision, not an architectural one, and auto-deleting on unlink would be
a data-loss regression. The orphan report in §3.6 is the correct surface for it. Track
separately.

### 4.6 Coupling with issue #13 (global items invisible in scoped lists)

The orphan predicate `ev.id IS NULL AND i.is_global = 0` is correct **because** global items
have a reachable surface (Global Secrets) and non-global unlinked items have none. #13's fix
changes how global items are *listed*, not whether they are reachable, so the predicate
survives it.

The predicate does **not** survive a change that gives project-owned-but-unlinked items a
view of their own (e.g. "all items owned by this project", which is a plausible follow-up to
#13). If such a view lands, the predicate must additionally require
`NOT EXISTS (SELECT 1 FROM item_projects WHERE item_id = i.id)`, or the prune would delete
items the new view legitimately shows. **Re-verify the predicate against `main` immediately
before shipping step 5.** This is a hard gate, not a note.

### 4.7 Transaction placement

*Rejected: `sqlx` statements directly in `handle_create_item`.* Blocked twice over — the
pool is private, and it would put SQL in `api`.

*Rejected: making `VaultDb.pool` `pub(crate)` so `vault` can own the transaction.* It works
and keeps the "vault orchestrates" story tidier, but it opens the pool to every module in
the crate permanently in exchange for one call site. The private pool is currently the only
thing enforcing that all SQL lives in `db`; trading that away for convenience is a bad
long-term deal.

*Rejected: `Mutex`-only serialization with no transaction.* The `SharedState` mutex does
serialize all writers today, which would make the interleaving in M3 unreachable — but that
is an invariant nobody has written down, held together by every future caller remembering to
take the lock. A transaction makes the atomicity a property of the storage layer.

### 4.8 Rollback plan

The change is behavioural, not schema-changing: no migration, no new columns, no data
rewrite. Reverting steps 1-4 restores the previous behaviour exactly; vaults written under
the new semantics are indistinguishable at rest from vaults written under the old ones —
they simply contain fewer rows. The only non-reversible action in the whole plan is the
user-initiated orphan prune, which is why it sits behind an explicit confirmation and a
preview. `PRAGMA secure_delete=ON` is a connection-level setting with no on-disk format
impact and can be removed at any time.
