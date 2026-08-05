# Issue #12 — Case-insensitive uniqueness for `environments.name`

Status: plan (no code written)
Scope: `src-tauri/src/db/mod.rs`, `src-tauri/src/project/mod.rs`, `src-tauri/src/api/mod.rs`, `src-tauri/Cargo.toml` (dev-deps), new `src-tauri/tests/environment_naming.rs`
Related: #7 (path traversal via environment name), #11 (test coverage / HTTP harness), prior "Bug 2" fix (`idx_projects_name_nocase`)

---

## 1. Objective

Definition of done — every item below is independently checkable.

1. **Index exists.** `idx_environments_name_nocase` on `environments(project_id, name COLLATE NOCASE)`, created as an additive, idempotent migration during `VaultDb::init_schema`. Verifiable with `SELECT sql FROM sqlite_master WHERE name='idx_environments_name_nocase'`.
2. **Migration is non-bricking.** Opening a vault that already contains a case-colliding environment pair succeeds. Colliding rows are deterministically renamed (lowest `id` keeps the name; the rest get `-2`, `-3`, … suffixes) *before* the index is created. A machine-readable report of every rename is persisted under the settings key `env_name_dedup_v1`. `init_schema` still returns `Err` if the index cannot be created for any other reason — it is never swallowed with `let _ =`.
3. **Same fix retrofitted to `projects`.** `idx_projects_name_nocase` gets the same pre-check. Today it is a bare statement with no audit, so any install predating that fix that already holds `MyApp` + `myapp` **cannot open its vault at all** (`init_schema` → `Err` → `VaultDb::open` → `Err`). See step 0.
4. **Write path returns 409, not 500.** `POST /environments` with a name that case-insensitively collides with an existing sibling returns `409 CONFLICT` with error code `CONFLICT`. Response body contains no SQL text — no `UNIQUE`, no `sqlite`, no table/column/index names. Success codes are unchanged: `201` when `id == 0`, `200` otherwise.
5. **Ambiguous scope resolution is rejected, not guessed.** `project::resolve_environment` returns an error listing the colliding candidates instead of silently taking the first match. HTTP maps it to `409` with error code `AMBIGUOUS_SCOPE`. This is required, not cosmetic — see §4, Decision D5 (SQLite `NOCASE` folds ASCII only; the Rust resolver folds full Unicode, so the index alone does **not** close the bug for non-ASCII names).
6. **Tests pass.** New file `src-tauri/tests/environment_naming.rs`, cases T1–T8 and T11 in §5. HTTP-level cases (T9, T10) are specified here but implemented under #11's harness.

---

## 2. What is being mitigated

**Checkable statement of the removed risk:**

> After this change it is impossible for a single project to hold two environments whose names differ only by ASCII case, and impossible for any name-based scope lookup (`?project=X&environment=Y`, CLI `--env`, MCP resolver) to silently resolve to one of several candidates. Every ambiguous lookup fails loudly and names the candidates.

Concretely, this closes:

| Failure | Before | After |
|---|---|---|
| Two `POST /environments` create `production` and `Production` in one project | Both persist | Second → `409 CONFLICT` |
| GUI creates `Production`, CLI auto-create races and creates `production` | Both persist | Loser → `409`, CLI can re-fetch and reuse |
| `--env production` with both rows present | Silently picks `ORDER BY id ASC` first row | Cannot happen (index); if it somehow does (non-ASCII), hard error listing candidates |
| Legacy install already holding a colliding pair | Would brick `VaultDb::open` once the index lands | Deterministic rename + persisted report |

Severity rationale: this is secret-disclosure-adjacent. The scope contract's entire job is guaranteeing you read from and write to the environment you asked for. A silent wrong-environment resolution means production values injected into a local `.env`, or a local value overwriting a production secret. No log, no error, no signal.

**Explicitly NOT mitigated by this change** (stated so nobody assumes otherwise):
- Non-ASCII case collisions at the *DB* level. `NOCASE` folds `A–Z` only, so `PRODUCCIÓN` and `producción` remain storable as two rows. They are caught by the app-level pre-check (write path) and the ambiguity rejection (read path) — layers 2 and 3 in §4/D5, not by the index.
- Homoglyph / Unicode-normalization collisions (`prоd` with a Cyrillic `о`). Out of scope; note it in the issue for a follow-up if it matters.

---

## 3. Implementation steps

Ordered. Each step is independently compilable and testable.

### Step 0 — Retrofit the duplicate pre-check for `projects` (recommended, decide before starting)

`src-tauri/src/db/mod.rs` L246 today:

```rust
"CREATE UNIQUE INDEX IF NOT EXISTS idx_projects_name_nocase ON projects(name COLLATE NOCASE)",
```

sits inside the `migrations` array whose loop is `.map_err(|e| format!("migration: {e}"))?`. On an install that already holds `MyApp` + `myapp`, this fails, `init_schema` returns `Err`, `VaultDb::open` returns `Err`, and the vault is unopenable with a raw sqlx string as the only diagnostic.

**This is in scope** because the plan mirrors that statement; shipping the same latent brick for `environments` and leaving the `projects` one in place would be knowingly duplicating a defect. If the maintainer prefers to split it into its own issue, do that — but do not ship step 3 without step 2, and do not ship step 3 while leaving `projects` unaudited.

Work: move `idx_projects_name_nocase` out of the `migrations` array (delete L241–246 including the comment, keeping the comment text with the statement at its new site) and re-create it in the imperative block added in step 3, after `dedupe_project_names_nocase`.

### Step 1 — Detect unique-constraint violations without string matching

`src-tauri/src/db/mod.rs`, new private helper near `upsert_environment` (L1022):

- Stop using `.map_err(|e| e.to_string())` for the `INSERT`/`UPDATE` in `upsert_environment`.
- Match on `sqlx::Error::Database(dbe)` and use `dbe.is_unique_violation()` (sqlx 0.8 — already a direct dependency at `Cargo.toml` L34).
- On a unique violation, return the **stable sentinel string**:
  `"conflict: an environment with this name already exists in this project"`
- On any other error, keep `e.to_string()` — but see step 5 for the API-side leak fix.

Rationale for the sentinel living in `db`: `db` owns the schema, therefore owns the constraint's meaning. `project` propagates it unchanged. `api` translates it to a status code. This respects the `db` ↛ `api` decoupling rule in CLAUDE.md while removing the fragile `e.to_lowercase().contains("unique constraint")` pattern used at `api/mod.rs` L1961.

Apply the same treatment to `upsert_project` and change `handle_save_project` (L1961) to match the sentinel prefix `"conflict:"` instead of sqlx text.

### Step 2 — Deterministic dedup before the index

`src-tauri/src/db/mod.rs`, two new private async methods on `VaultDb`:

```
async fn dedupe_environment_names_nocase(&self) -> Result<Vec<RenameRecord>, String>
async fn dedupe_project_names_nocase(&self)     -> Result<Vec<RenameRecord>, String>
```

Algorithm (environments; projects identical minus the `project_id` grouping):

1. Find collision groups:
   `SELECT project_id, LOWER(name) AS k, COUNT(*) c FROM environments GROUP BY project_id, k HAVING c > 1`
   (`LOWER()` in SQLite is ASCII-only, which exactly matches `NOCASE` — deliberate; this must find precisely what the index would reject, no more, no less.)
2. For each group, `SELECT id, name FROM environments WHERE project_id=? AND LOWER(name)=? ORDER BY id ASC`.
3. **The first row (lowest `id`) keeps its name.** Every subsequent row `n` is renamed to `<name>-<n>`, incrementing the suffix until the candidate collides with nothing in that project (case-insensitively) — so `prod`, `Prod`, plus a pre-existing `prod-2` yields `prod`, `prod-3`.
4. `UPDATE environments SET name=?, updated=? WHERE id=?`. Only `name` changes.
5. Return the rename records.

**Why lowest `id` wins:** `db::list_environments` (L951) is `ORDER BY id ASC`, and `project::resolve_environment` takes the first `.find()` match. Lowest `id` is therefore the environment that name-based lookups resolve to *today*. Any other tiebreak (e.g. prefer `is_default`) would silently flip which environment `--env production` points at during an upgrade — the exact class of bug being fixed.

**Why rename and not merge:** the environment `id` is untouched, so `environment_vars` and `environment_paths` (both FK-on-`environment_id`, `ON DELETE CASCADE`) are structurally unaffected — zero rows move, zero rows are dropped, `item_projects` ownership is untouched. See §4/D3 for why merging was rejected.

**Blast radius of a rename** (must be in the release note):
- `crypt-env.json` files or `--env` flags naming the *renamed loser* now fail with `project/environment not found` — a loud, correct failure replacing a silent wrong-environment resolution. The user re-points them at the new name or renames the environment back in the GUI.
- `environment_paths` rows are unchanged: a renamed environment still injects to the same configured file paths. If the *path string* embeds the old environment name (e.g. `.env.production`), that string is data and is not rewritten — deliberate, since rewriting user-authored paths is a filesystem side effect a migration must never take.
- The GUI shows the new name on next load; no stale cache to invalidate (TanStack Query refetches on unlock).

### Step 3 — Wire dedup + index into `init_schema`

`src-tauri/src/db/mod.rs`, `init_schema` (L95).

- Add the `environments` index **outside** the declarative `migrations` array — it now has an imperative precondition and no longer belongs in a flat SQL list.
- Insert after the `for stmt in &migrations` loop (L248–250) and before the `backfilled_global_orphans_v1` block (L257):

```
1. let project_renames = self.dedupe_project_names_nocase().await?;
2. execute CREATE UNIQUE INDEX IF NOT EXISTS idx_projects_name_nocase
     ON projects(name COLLATE NOCASE)                       -- error mapped with `?`
3. let env_renames = self.dedupe_environment_names_nocase().await?;
4. execute CREATE UNIQUE INDEX IF NOT EXISTS idx_environments_name_nocase
     ON environments(project_id, name COLLATE NOCASE)       -- error mapped with `?`
5. if !renames.is_empty() { merge into settings['env_name_dedup_v1'] }
```

**Placement relative to `PRAGMA foreign_keys=ON`:** the pragma is executed in the first `stmts` array (L98) on the single pooled connection (`max_connections(1)`, L85), so it is already on. Nothing here depends on it — no FK references `environments.name` — but the ordering is stated so a future reader does not have to re-derive it.

**Ordering relative to the other backfills:** the dedup must run *after* the `INSERT INTO environments … 'default'` backfill (L205–208), because that backfill can itself introduce a `default` row into a project that already has a differently-cased `Default`. Placing the block after the whole `migrations` loop guarantees this.

**Idempotency:** not gated by a settings flag. With the index in place the collision query returns zero rows, so a re-run is a no-op and costs one grouped scan per open. A flag would only mask a regression. The *report* is gated implicitly — it is only written when a rename actually occurred, and is merged (append) with any existing report rather than overwritten.

**Fail-safe:** all four statements use `?`. A failed index creation aborts `VaultDb::open` with a message that names the remedy, e.g.
`"migration: could not enforce unique environment names (idx_environments_name_nocase): <cause>"`.
It is **never** `let _ = …` — silently skipping the index would leave the vault unprotected with no signal, which is the failure mode the issue is about.

**Report format** (settings value, JSON):
```json
[{"table":"environments","id":42,"projectId":7,"from":"Production","to":"Production-2","at":"2026-08-03T…Z"}]
```
Contains names and ids only. It must never contain variable keys or values — environment names are already exposed via `GET /projects`, secret material is not.

### Step 4 — App-level pre-check in `project::save_environment`

`src-tauri/src/project/mod.rs`, `save_environment` (L172), before `db.upsert_environment` (L173):

- List the project's existing environments, compare `input.name.to_lowercase()` against each sibling's, skipping the row whose `id == input.id` (so renaming an environment to a different case of its own name is allowed).
- On collision, return the same sentinel: `"conflict: an environment with this name already exists in this project"`.

This layer is Unicode-aware (Rust `to_lowercase`) and therefore catches the non-ASCII collisions the index cannot. It is TOCTOU-racy on its own — the DB index is the race backstop for the ASCII case. Neither layer is redundant; each covers the other's gap.

Also add the same check to `save_project`'s auto-created `"default"` environment (L153) — cheap, since a brand-new project has no siblings, but keeps the choke point single.

**Sequencing with issue #7:** #7 wants `validate_environment_name` at this exact site. Fixed contract, whichever lands first:

```rust
validate_environment_name(&input.name)?;      // #7 — shape/traversal
ensure_no_case_collision(db, &input)?;        // #12 — this plan
let env_id = db.upsert_environment(...)       // existing
```

Shape validation first (a traversal-unsafe name should be rejected before it is compared against anything). The two edits are adjacent lines in the same function — coordinate the merge order, but there is no logical conflict.

### Step 5 — HTTP mapping

`src-tauri/src/api/mod.rs`, `handle_save_environment` (L2022–2063). Replace the catch-all at L2061:

```rust
Err(e) if e.starts_with("conflict:") =>
    err_json(StatusCode::CONFLICT,
             "an environment with this name already exists in this project",
             "CONFLICT").into_response(),
Err(_) => err_json(StatusCode::INTERNAL_SERVER_ERROR,
                   "internal error", "INTERNAL_ERROR").into_response(),
```

Two things happen here. The conflict gets its own 409, and — separately — the fallback stops echoing `&e` into the response body. Today an unexpected sqlx failure returns raw SQL text (table, column, index names) to the client, which violates the CLAUDE.md rule on what may appear in API responses. Log the detail to stderr instead (never the input `name`, and never any var value).

Mirror the same fallback hardening in `handle_save_project` (L1965).

### Step 6 — Ambiguity rejection in `resolve_environment`

`src-tauri/src/project/mod.rs`, `resolve_environment` (L214–237). Replace both `.find()` calls with collect-and-count:

- Project match: >1 candidate → `Err("ambiguous match for project '<p>': <name> (id N), <name> (id M). Pass environment_id instead.")`
- Environment match within the resolved project: same shape.

Message format deliberately mirrors the existing MCP precedent at `src-tauri/src/bin/crypt-env-mcp.rs` :2725–2740 so users see one vocabulary across GUI, HTTP, CLI and MCP.

API side: map an error starting with `"ambiguous match"` to `409 CONFLICT`, code `AMBIGUOUS_SCOPE`, in every handler that calls `resolve_environment`. 409 rather than 422 because the request is well-formed; it is the vault's state that prevents an unambiguous answer, and the client's fix is to pass `environment_id`.

Leave `src-tauri/src/bin/crypt-env/commands/scope.rs` and the MCP resolver alone in this change — MCP already handles ambiguity, and CLI's resolver goes through the HTTP API, so it inherits the new 409. Note it and move on.

### Step 7 — Frontend (optional, low priority)

`src/` `ProjectManager.tsx` environment editor: client-side case-insensitive check against the already-loaded environment list, to give immediate feedback before `invoke('environment_save')`. Surface the backend sentinel verbatim on failure.

This is UX only. The client check is **not** enforcement — the DB index and the server pre-check are. Do not let it grow into the only guard.

Tauri path: `environment_save` (`project/mod.rs` L393) already returns `Result<i64, String>` and will carry the sentinel unchanged, so the GUI gets a distinguishable, stable string without a new command or signature change.

---

## 4. Trade-offs / alternatives considered

### D1 — Index shape: `(project_id, name COLLATE NOCASE)`

**Chosen.** `project_id` must stay as the leading column: environments are scoped per project, and `production` in project A and `production` in project B is not merely legal, it is the normal case. A global `UNIQUE(name COLLATE NOCASE)` would break every multi-project vault on first open.

`project_id` is an INTEGER, so its collation is irrelevant — only `name` carries `COLLATE NOCASE`.

*Rejected:* changing the column declaration to `name TEXT COLLATE NOCASE` in the `CREATE TABLE`. It reads cleaner, but SQLite does not apply it to existing tables, so it would require a table rebuild (`ALTER TABLE … RENAME` + recreate + copy + FK dance) on every existing install. A separate unique index is additive, `IF NOT EXISTS`-idempotent, and instantly droppable — a far cheaper rollback.

*Rejected:* a `name_lower` generated/materialized column with a plain unique index. More explicit, and it would let us store a Unicode-folded key — but it changes the table shape, needs a backfill, and adds a column every read path must ignore. Deferred; revisit only if non-ASCII collisions become a real complaint.

### D2 — Where the case-folding contract lives

Chosen: SQLite `NOCASE` (ASCII) at the DB, Rust `to_lowercase()` (Unicode) in the app. These two disagree, and the plan treats that disagreement as a documented, tested fact rather than pretending it away — that disagreement is precisely why step 6 exists.

*Rejected:* switching the Rust resolvers to ASCII-only folding to match SQLite. It would make the two layers agree, but by making resolution *less* strict — `PRODUCCIÓN` and `producción` would then resolve as different environments, which is defensible but changes existing lookup behaviour for anyone relying on it. Loosening a security-relevant comparison to simplify an invariant is the wrong direction.

*Rejected:* registering a custom Unicode collation with SQLite. Correct in principle, but it makes the index dependent on a runtime-registered function — a vault opened by any other SQLite client (backup tooling, `sqlite3` CLI, a future `crypt-env` build that forgets to register it) would fail to read the index. Not worth it for a desktop vault.

### D3 — Conflict resolution policy: rename losers

**Chosen:** lowest `id` keeps the name, others get `-2`, `-3`, … with collision-avoiding suffix search.

*Rejected — refuse to start.* Honest, but it bricks the vault for a condition the user never caused and cannot fix without a SQL client. The vault is the only place the secrets live. Unacceptable.

*Rejected — merge the colliding environments.* Superficially the "nicest" outcome, but merging two sets of `environment_vars` means deciding what happens when both define `DB_PASSWORD` with different `item_id`s. Any automatic answer picks one secret over another silently — which is exactly the failure class this issue is about. A migration must never silently choose between two secrets.

*Rejected — surface a repair command and skip the index until the user runs it.* Keeps the vault open and gives the user full control, but leaves the bug live for an unbounded window, and the "skip" branch is the branch nobody tests. Also needs new UI plus a new CLI subcommand — significantly more surface than a rename.

The rename's cost is real and stated: a name-based reference to the renamed loser breaks. That break is loud and locally fixable, and it replaces a silent wrong-environment read. Reversible by hand using the persisted `env_name_dedup_v1` report.

### D4 — Sentinel string vs. a typed error

**Chosen:** a stable sentinel prefix (`"conflict:"`) on the existing `Result<_, String>`, with the *detection* done properly via `sqlx`'s `is_unique_violation()` rather than substring-matching sqlx's text.

CLAUDE.md asks for custom error types, and a `VaultError` enum is the right long-term answer. It is not this change: `Result<_, String>` runs through `db`, `project`, every Tauri command (Tauri needs a serializable error) and every HTTP handler. Converting it is a large, mechanical, high-blast-radius refactor that should be its own issue, reviewed on its own merits. Bundling it here would make a security fix hard to review.

*Rejected:* keeping the existing `e.to_lowercase().contains("unique constraint")` pattern from `handle_save_project` L1961. It couples the API layer to sqlx's error prose, breaks on a sqlx upgrade or a locale change, and requires the raw SQL string to survive all the way to the HTTP layer — which is what causes the SQL leak in the 500 path.

**Regret check:** if the `VaultError` refactor lands later, every sentinel site is a single `grep "conflict:"` away. The sentinel is a deliberate placeholder, not a permanent design.

### D5 — Ambiguity rejection in `resolve_environment`: required, not belt-and-braces

The initial framing was "defence in depth for installs that predate the index". That framing is wrong, and the correct reason is stronger:

SQLite's `NOCASE` folds ASCII `A–Z` only. `project::resolve_environment` uses Rust `to_lowercase()`, which folds full Unicode. Therefore `PRODUCCIÓN` and `producción` **satisfy the new index** (SQLite sees two distinct names) while the resolver sees two matches and silently takes the first. The index alone does not close the reported bug for non-ASCII names.

Steps 4 and 6 are what actually close it for those names. Step 6 also covers the residual TOCTOU race in step 4's pre-check.

*Rejected:* relying on the index alone. Cheaper, and correct for the ASCII case that motivated the issue — but it would ship a fix that is advertised as complete and is not.

### D6 — `resolve_environment` ambiguity → 409, not 300/422

409 `CONFLICT` + code `AMBIGUOUS_SCOPE`. The request is syntactically valid, so not 422; the server refuses because vault state makes the answer non-unique, and the client's remedy (`environment_id`) is deterministic. 300 Multiple Choices is technically closest but is effectively unused in practice and would surprise every existing client.

### D7 — Test seeding requires `sqlx` in dev-dependencies

Test T3 must seed a DB that *already* contains a colliding pair — impossible through the public API once the index exists. Integration tests link the lib crate but not its dependencies, so they cannot `use sqlx` today.

**Chosen:** add `sqlx = { version = "0.8", features = ["sqlite", "runtime-tokio"] }` to `[dev-dependencies]` in `src-tauri/Cargo.toml` (L97–99). Zero extra compilation — the crate is already built as a normal dependency (L34).

*Rejected:* a `pub` test-only hook on `VaultDb` (e.g. `insert_environment_unchecked`). It puts a constraint-bypassing method on the production API surface for test convenience — precisely the hidden complexity CLAUDE.md rejects, and a future caller will eventually use it.

*Rejected:* shipping a fixture `.db` file in the repo. Opaque binary, invisible to review, and rots the moment the schema changes.

---

## 5. Tests

New file: `src-tauri/tests/environment_naming.rs`. Same pattern as the existing `vault_integration.rs` — `#[tokio::test]` + `tempfile::tempdir()` + `VaultDb::open`. No new harness.

**Coordination with #11:** #11 owns the HTTP-level test harness (spawning the axum app, tokens). Cases T9 and T10 below are specified but belong to that harness — do not build a second one here. Cases T1–T8 and T11 need only a `VaultDb`, so they are self-contained and can land first.

| # | Case | Assertion |
|---|---|---|
| T1 | Fresh DB, `upsert_environment(0, p, "production")` then `"Production"` | Second returns `Err` starting with `"conflict:"` |
| T2 | `"production"` in project A and project B | Both succeed — proves `project_id` is in the index |
| T3 | Seed DB via raw sqlx: drop the index, insert `production` (id 1) + `Production` (id 2), close, reopen with `VaultDb::open` | Opens successfully; id 1 still `production`, id 2 now `production-2` |
| T4 | T3's DB seeded with vars + paths on both rows | `environment_vars` / `environment_paths` counts per `environment_id` unchanged; `item_projects` unchanged |
| T5 | Open T3's DB a second time | No further renames; `env_name_dedup_v1` report has exactly the T3 entries, not duplicated |
| T6 | Seed `prod`(1), `Prod`(2), `PROD`(3) | → `prod`, `prod-2`, `prod-3` |
| T7 | Seed `prod`(1), `Prod`(2), plus an existing `prod-2`(3) | Loser becomes `prod-3`, `prod-2` untouched |
| T8 | Seed `producción` + `PRODUCCIÓN` (both survive the index — ASCII-only `NOCASE`) | `project::resolve_environment(db, None, Some(p), Some("PRODUCCIÓN"))` returns `Err` containing `"ambiguous match"` and both ids |
| T9 | *(#11 harness)* `POST /environments` with a colliding name | `409`, code `CONFLICT`; body contains none of `UNIQUE`, `sqlite`, `environments`, `idx_` |
| T10 | *(#11 harness)* `POST /environments` with a non-ASCII case collision | `409` from the step-4 app pre-check, same body constraints |
| T11 | `save_project` auto-creates `default`; then save an environment named `Default` | `Err` with the `"conflict:"` sentinel |

Also extend the projects side minimally: one test that a DB seeded with `MyApp` + `myapp` now **opens** (today it would fail `init_schema`) and that `myapp` was renamed to `myapp-2`.

---

## 6. Rollback

- **Index:** `DROP INDEX idx_environments_name_nocase;` — instant, no data touched. Reverting the binary alone is not enough: the index persists in the file, so a downgrade must drop it or the old binary will start hitting constraint errors on write. State this in the release note.
- **Renames:** not automatically reversible. Mitigated by the persisted `env_name_dedup_v1` report, which contains every `(id, from, to)` needed to reverse them by hand or via a future repair command. This is the one irreversible part of the change and the reason the rename policy is deterministic and documented.
- **Sentinel / 409 mapping / ambiguity rejection:** pure code, revert with the commit.

---

## 7. Open questions for the maintainer

1. **Step 0 in or out?** Retrofitting the `projects` pre-check fixes a real vault-bricking path but widens the diff beyond issue #12's title. Recommendation: in, same PR, called out in the commit message.
2. **Suffix format.** `production-2` assumed. If environment names are ever used verbatim as filename fragments, confirm `-` is safe there — this overlaps with #7's `validate_environment_name`, and the generated suffix must satisfy whatever that validator ends up allowing.
3. **Surfacing the rename to the user.** The report lands in `settings`. Minimum viable surface is a line in `crypt-env doctor` (`src-tauri/src/bin/crypt-env/commands/doctor.rs` already reports on `crypt-env.json` and is the natural home). A GUI toast on first unlock after the migration is nicer but is a product decision, not an architectural one.
