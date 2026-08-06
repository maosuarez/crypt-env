# Issue #13 — Global items invisible through scoped list/search surfaces

Status: plan only (no code written).
Branch target: a dedicated `fix/global-items-scoped-visibility` branch off `main`.
Related: issue #11 (test-coverage plan — this plan depends on it for the HTTP harness, see §5).

---

## 0. Correction to the issue text

The issue claims `is_global` "isn't even in scope at the point of filtering (already
stripped by `decrypt_all_items`'s tuple shape)". **That is not accurate against current
code.**

`src-tauri/src/api/mod.rs:145-166` (`decrypt_all_items`) maps raw rows
`(id, _, data, _, is_global)` through `crate::vault::decrypt_item(&key, id, &data, is_global)`,
and `VaultItem.is_global: Option<bool>` (`src-tauri/src/vault/mod.rs:55-56`) is therefore
populated on every item reaching the filter site at `src-tauri/src/api/mod.rs:480`.

Consequence for scoping this work: the Rust change is small (a filter predicate plus a
query-param type). The bulk of the effort is **contract definition, response shape,
downstream surfaces (CLI/MCP), documentation, and tests** — not the filter itself.

---

## 1. Objective (definition of done)

### 1.1 Contract to be implemented

Introduce an explicit, documented tri-state query parameter on the two **discovery**
endpoints, and leave every **materialization** endpoint untouched.

| Surface | Kind | Change |
|---|---|---|
| `GET /items` | discovery | accepts `include_global=true\|false\|only`, **default `true`** |
| `GET /commands` | discovery | accepts `include_global=true\|false\|only`, **default `true`** |
| `POST /fill` | materialization | **unchanged** — resolves strictly through `environment_vars` |
| `POST /environments/:id/inject` | materialization | **unchanged** |
| `POST /environments/:id/example` | materialization | **unchanged** |
| `POST /share/listen` | action | **unchanged** — sender may still only share items linked into the resolved environment |
| `POST /items`, `/share/connect`, `/share/import`, `/relay/receive` | write/scoped | **unchanged** |

Semantics:

- `include_global=true` (default): returned set = (items linked in the resolved
  environment) ∪ (all items with `is_global = 1`), deduplicated by item id.
- `include_global=false`: returned set = items linked in the resolved environment only —
  byte-for-byte the current behaviour, i.e. "what `/fill` and `/inject` will materialize".
- `include_global=only`: returned set = all `is_global = 1` items, ignoring linkage — the
  REST/CLI/MCP equivalent of the GUI's `GlobalSecrets.tsx` screen, but still requiring a
  valid scope so error handling stays uniform.
- Any other value → `422 VALIDATION_ERROR` with field `include_global`
  (reusing `err_validation`, `src-tauri/src/api/mod.rs:211`).
- Scope resolution is unchanged: `environment_id`, or `project`+`environment`, still
  required; still 422 when unresolvable.

### 1.2 Response shape

Every item returned by `GET /items` and `GET /commands` carries two discriminators:

- `isGlobal: bool` — already serialized today via `VaultItem` (`vault/mod.rs:55`).
- `linked: bool` — **new, API-response-only**: `true` iff the item id appears in the
  resolved environment's `environment_vars`.

`linked` MUST NOT be added to `VaultItem`. `VaultItem` is the struct that gets serialized
into the AES-GCM ciphertext (`vault::encrypt_item`), so a view-only field on it risks being
persisted into the encrypted blob by any round-trip write path. Instead add an API-layer
wrapper in `src-tauri/src/api/mod.rs`:

```rust
#[derive(Serialize)]
struct ScopedItem {
    #[serde(flatten)]
    item: VaultItem,
    linked: bool,
}
```

Redaction is unchanged: `redact_item` (`api/mod.rs:169-174`) still strips
`value`/`password`/`content` before the item is wrapped. **No plaintext secret is added to
any response by this change** — the union widens *metadata* visibility only.

### 1.3 Downstream surfaces

- CLI `crypt-env search` (`src-tauri/src/bin/crypt-env/commands/search.rs`): new
  `--scope-globals <with|without|only>` flag (default `with`), plus a `SCOPE` column
  rendering `linked` / `global` / `global+linked`.
- CLI `crypt-env list` (`.../commands/list.rs`, consumes `/commands`): same flag, same column.
- CLI `crypt-env cmd` / `crypt-env exec` (`.../commands/cmd.rs`, `.../commands/exec.rs`):
  on name collision between a linked command and a global command, **prefer the linked one**
  and print a one-line stderr warning naming the shadowed global id.
- MCP `crypt_env_list_items` and `crypt_env_search_items`
  (`src-tauri/src/bin/crypt-env-mcp.rs:205`, `:229`): add `include_global` to both
  `inputSchema`s with the enum and a description that states the linked/global distinction,
  and forward it in the URL builder next to `append_scope_params` (`:798`).
- GUI: **no change**. `src/components/GlobalSecrets.tsx:31` filters `allItems` client-side
  off the unscoped Tauri command and continues to work. This keeps the diff off the
  frontend entirely and respects the `invoke()`-only rule.

### 1.4 Verifiable acceptance criteria

Done when all of the following hold:

1. `GET /items?project=X&environment=Y` returns a global item that is linked into **no**
   environment, with `isGlobal: true, linked: false`.
2. The same request with `&include_global=false` returns exactly the pre-change set.
3. `&include_global=only` returns exactly the set the GUI's Global Secrets screen shows.
4. `&include_global=bogus` returns 422 with `include_global` in the message.
5. An item that is both global and linked appears **once**, with `linked: true`.
6. `POST /fill`, `/environments/:id/inject`, `/environments/:id/example` produce
   byte-identical output before and after the change for the same environment.
7. `POST /share/listen` still 422s when asked to share an unlinked global item.
8. **9 automated tests pass** (7 unit + 2 integration; enumerated in §5), plus the
   4-step manual repro from the issue no longer reproduces.
9. `docs/reference.md` states the contract, and the stale Notes paragraph at
   `docs/reference.md:280` ("Global items … are invisible to …") is replaced.

---

## 2. What is being mitigated

**Concrete bug.** `environment_item_ids` (`src-tauri/src/api/mod.rs:438-440`) builds the
allowed-id set purely from `env.vars`, and `handle_list_items` (`:467`, `:480`) filters on
it with no `is_global` branch. A global item that is not linked into the queried environment
is therefore absent from the response — **indistinguishable, from the caller's side, from an
item that does not exist**.

Checkable statement of the defect:

> With a vault containing exactly one item, created global and linked to no environment,
> `GET /items?project=X&environment=Y` returns `[]` for every existing project/environment,
> while the GUI Global Secrets screen lists it.

**Risks this reduces:**

1. **Contract divergence between GUI and headless surfaces.** The documented mental model in
   `CLAUDE.md` and `docs/reference.md` is "create a value once, mark it global, reference it
   from any environment without re-entering it". Today only the GUI honours that; REST, CLI
   and MCP do not. `docs/reference.md:280` already admits this in a Notes paragraph, which
   makes the divergence a *known, undocumented-in-the-contract-table* behaviour — the worst
   of both.
2. **Duplicate-secret proliferation.** An agent or script that cannot see a global secret has
   exactly one recovery path: create a second copy of the same credential inside the project
   scope. That multiplies the number of places a rotation must reach, which is a security
   regression, not just an ergonomics one.
3. **Unrecoverable dead end for MCP agents.** There is **no** unscoped or global-only route
   today — `/globals` does not exist and `/items` has no `global` filter (route table,
   `api/mod.rs:3055-3090`). An LLM agent has no tool call that can answer "does a reusable
   global secret for this service already exist?".

**Explicitly *not* mitigated by this change** (stated so nobody mistakes the scope):
this is a visibility/discovery fix. It does not make globals participate in `/fill` or
`/inject`. Linking a global into an environment remains a deliberate act.

---

## 3. Implementation steps

Ordered. Each step is independently compilable; steps 1–3 are the behavioural core, 4–7 are
surface propagation, 8–9 are docs and tests.

### Step 1 — Query-param type and parser (`src-tauri/src/api/mod.rs`)

- Add near `EnvScopeQuery` (`:227`):
  ```rust
  #[derive(Clone, Copy, PartialEq)]
  enum IncludeGlobal { With, Without, Only }
  ```
  with `fn parse(raw: Option<&str>) -> Result<IncludeGlobal, axum::response::Response>`
  mapping `None|"true"|"with" → With`, `"false"|"without" → Without`, `"only" → Only`,
  and anything else → `err_validation("include_global", "must be one of: true, false, only")`.
  No `unwrap()`; `Result` per CLAUDE.md.
- Add `include_global: Option<String>` to `ItemsQuery` (`:425-434`) and to the
  `EnvScopeQuery` extractor used by `handle_list_commands` — or, cleaner, add it to
  `EnvScopeQuery` alone and have `ItemsQuery` keep its own copy, matching the existing
  duplication style rather than refactoring the extractor in a bug-fix PR.

### Step 2 — Replace the scope predicate (`src-tauri/src/api/mod.rs`)

- Replace the helper at `:438-440` with a pair:
  ```rust
  fn environment_item_ids(env: &project::Environment) -> HashSet<i64>   // keep, unchanged
  fn scope_items(items: Vec<VaultItem>, linked: &HashSet<i64>, mode: IncludeGlobal) -> Vec<ScopedItem>
  ```
  `scope_items` is a **pure function** — no `ApiState`, no lock, no crypto — so it is unit
  testable without a vault or an HTTP server. It performs the union/dedup and stamps
  `linked`.
- `handle_list_items` (`:442-481`): replace `.filter(|item| allowed_ids.contains(&item.id))`
  with `scope_items(items, &allowed_ids, mode)`, then apply the existing type/category/search
  filters over `ScopedItem` (matching on `s.item.*`). Order matters: union first, filters
  second, so `search` also searches globals.

### Step 3 — Apply to `/commands` (`src-tauri/src/api/mod.rs:964-1022`)

- `handle_list_commands` uses the same helper at `:989`; route it through `scope_items` with
  the same parsed mode. The placeholder-extraction logic is untouched.

### Step 4 — Confirm the untouched sites stay untouched

Audit, do not edit, the remaining `resolve_scope` callers so the split between discovery and
materialization is deliberate and reviewable:

| Line | Handler | Uses `environment_item_ids`? | Action |
|---|---|---|---|
| `:587` | `handle_create_item` | no | unchanged |
| `:978` | `handle_list_commands` | yes (`:989`) | **changed** (step 3) |
| `:1329` | `handle_fill` | no — matches `env.vars.key` directly | unchanged |
| `:1527` | `handle_share_listen` | yes (`:1538`) | **unchanged on purpose** — see §4.3 |
| `:1617` | `handle_share_connect` | no | unchanged |
| `:1877` | `handle_share_import` | no | unchanged |
| `:2448` | `handle_relay_receive` | no | unchanged |

Also note but do **not** change `handle_create_item`'s hardcoded
`body.item.is_global = Some(false);` (`:647`). It is documented behaviour
(`docs/reference.md:14`) and changing item-creation semantics does not belong in a
visibility fix; it is called out in §4.5 as follow-up.

### Step 5 — CLI (`src-tauri/src/bin/crypt-env/`)

- `client.rs:54-64` — add `#[serde(default, rename = "isGlobal")] pub is_global: bool` and
  `#[serde(default)] pub linked: bool` to `ItemSummary`; add the same two fields to
  `CommandDetail` (`:66+`).
- `commands/search.rs` — add the `--scope-globals` flag, append `include_global=` to the URL
  built at `:22-26`, add the `SCOPE` column to the `println!` table.
- `commands/list.rs` — same flag, appended to the `/commands` URL at `:29`, extra table column.
- `commands/cmd.rs` (`:42`, `:75`, `:124`) and `commands/exec.rs` (`:31`) — these resolve a
  command **by name**; add linked-wins tie-breaking plus the stderr shadow warning. They do
  not get the new flag (they are execution paths, not listings) and should send
  `include_global=true` implicitly so a global command is runnable from any project.

### Step 6 — MCP (`src-tauri/src/bin/crypt-env-mcp.rs`)

- Add `include_global` (`"type": "string"`, `"enum": ["true","false","only"]`) to the
  `inputSchema` of `crypt_env_list_items` (`:205`) and `crypt_env_search_items` (`:229`).
  Descriptions must state: *"true (default) also lists reusable global secrets not yet linked
  into this environment — these appear with `linked: false` and will NOT be written by
  generate/inject/fill until linked."*
- Forward the value in the two URL builders that call `append_scope_params` (`:798`) for
  those tools. Do **not** touch `crypt_env_generate_env` / `crypt_env_inject_env` /
  `crypt_env_fill_env`.

### Step 7 — Tauri commands

No change. The GUI reaches items through the unscoped `vault_get_items` path, and
`GlobalSecrets.tsx:31` filters on `isGlobal` client-side. Registering a new `module_action`
command is unnecessary and would duplicate contract surface.

### Step 8 — Documentation (`docs/reference.md`)

1. Rewrite the `GET /items` row (`:14`) and the `GET /commands` row to describe
   `include_global`, its default, and the `linked` field.
2. Add a short **"Global items and scope"** subsection under the REST table stating the
   contract in one paragraph: *discovery surfaces union globals; materialization surfaces
   never do; `linked` is the discriminator.*
3. Replace the stale Notes paragraph at `:280` with a description of the new behaviour and
   the remaining gap (globals still require an explicit link before `/fill`/`inject` uses them).
4. Update the MCP tool table rows for `crypt_env_list_items` / `crypt_env_search_items`.
5. Update the CLI table rows for `list` / `search` with the new flag.

### Step 9 — Tests (see §5).

---

## 4. Trade-offs and alternatives considered

### 4.1 DECISION — tri-state `include_global`, defaulting to `true`, on discovery endpoints only

Union globals into `GET /items` and `GET /commands` by default, mark every returned item with
`isGlobal` + `linked`, and leave `/fill`, `/inject`, `/example` and `/share/listen` strictly
linkage-based. This is the smallest change that makes the documented mental model true for
headless callers, while keeping the "what will actually be written" question answerable via
`include_global=false`.

### 4.2 ALTERNATIVE A — unconditional union, no parameter (the issue's option (a))

Strengths: one-line diff, zero new surface, nothing to document beyond a sentence, no risk of
callers passing a wrong enum value.

Rejected because it **destroys an answer that currently exists**. Today `GET /items?project=X&environment=Y`
is the only way to ask "what will `/fill` and `/inject` write for this environment". An
unconditional union removes that with no replacement, so any consumer doing a pre-inject
diff, a CI drift check, or a "which keys are missing" report silently starts reporting the
whole global set as present. `include_global=false` preserves it at the cost of one enum.

### 4.3 ALTERNATIVE B — document the current behaviour as intended, add a `/globals` route (the issue's option (b))

Strengths: intellectually the cleanest split — "global means *available to link*, not
*implicitly present*" is a defensible model, and a dedicated route keeps each endpoint's
answer unambiguous. It is also the least likely to surprise an existing script.

Rejected on two grounds. First, it contradicts the redesign premise already written into
`CLAUDE.md` and `docs/reference.md` ("reference it from any environment without re-entering
it") — adopting it means editing the product's stated model to match an implementation
accident. Second, it costs *more* surface, not less: a new route, a new MCP tool, a new CLI
subcommand, all of which an LLM agent must be taught to call at the right moment. Agents
reliably call the obvious tool and stop; a discovery gap that requires knowing about a second
tool is a discovery gap.

Partially adopted anyway: `include_global=only` gives the same capability as a `/globals`
route without a new route, MCP tool, or auth surface.

### 4.4 ALTERNATIVE C — union into every scoped surface including `/fill` and `/share/listen`

Strengths: maximal consistency; one rule, no discovery/materialization distinction to explain.

Rejected as a security regression. `/fill` writing every global secret into a project's
`.env` because it happens to match a template key turns "reusable across projects" into
"leaked into every project's on-disk env file". `/share/listen` is worse: it would let a
sender transmit globals that were never associated with the project they claimed to be
sharing from — a one-call exfiltration widening, exactly the class of hazard already noted
for `crypt_env_inject_environment` in `docs/reference.md`. Materialization must stay an
explicit, per-link decision.

### 4.5 Sub-decisions

**Default `true` vs default `false`.** Default `false` is strictly backward compatible and
therefore tempting. It is rejected because it fixes nothing for existing callers: every MCP
agent and CLI script keeps seeing an empty result for globals until someone rewrites it to
pass a parameter it has no reason to know exists. The bug is precisely "callers cannot
discover that globals exist" — a fix gated behind an opt-in the caller cannot discover is
not a fix. The cost is a genuine behaviour change to `GET /items`, mitigated by `linked`,
by the unchanged materialization endpoints, and by the escape hatch.

**`linked` on a wrapper vs on `VaultItem`.** Adding `linked` to `VaultItem` is fewer lines
but `VaultItem` is the plaintext struct that gets encrypted; a per-request view flag on it
can be persisted into ciphertext by any read-modify-write path, permanently baking a
scope-relative boolean into an item's encrypted payload. The `ScopedItem` wrapper with
`#[serde(flatten)]` keeps `db`/`vault` unaware of API concerns, honouring the module
decoupling rule.

**Applying it to `/commands` too.** Considered leaving `/commands` alone (smaller blast
radius). Rejected: `crypt-env list`, `crypt-env cmd` and `crypt-env exec` all read
`/commands`, so a global command would remain invisible to the CLI while global secrets
became visible — a new inconsistency in place of the old one. The name-collision tie-break
(linked wins, warn on shadow) is the cost of that consistency and is cheap.

**Not fixing `handle_create_item`'s `is_global = Some(false)` here.** It is a separate
contract question ("can the REST API create a global item?") with its own ownership and
cascade implications. Bundling it would make this PR's diff span creation semantics as well
as read semantics. Follow-up issue.

### 4.6 Reversibility

High. The change is one enum, one pure function, one wrapper struct, plus additive fields on
CLI/MCP schemas. Reverting is a single `git revert` of the API commit; CLI and MCP additions
are backward-compatible (an unknown flag simply stops being sent) and can be left in place or
reverted independently. No database migration, no schema change, no persisted state — nothing
to roll forward or clean up.

---

## 5. Test plan

**Dependency on issue #11.** There is no HTTP-level test harness today:
`src-tauri/tests/vault_integration.rs` (106 lines) exercises `VaultDb` directly with
`tempfile::tempdir()` + `#[tokio::test]`, and never boots the axum router or unlocks a vault.
Building one (unlock, token, router, client) is issue #11's job. **This plan must not
duplicate that harness design.** It instead extracts the logic into a pure function so the
behaviour is fully covered without HTTP, and leaves exactly one end-to-end assertion to be
added once #11 lands.

### 5.1 Unit tests — 7, in `src-tauri/src/api/mod.rs` under `#[cfg(test)] mod scope_tests`

Fixtures are plain `VaultItem` values and a `project::Environment` with synthetic `vars`; no
database, no key, no async.

1. `global_unlinked_item_visible_by_default` — env links item A (non-global); vault also has
   item B (`is_global = true`, unlinked). Default mode returns both; B has `linked: false`.
2. `linked_item_reports_linked_true` — A comes back with `linked: true`, `isGlobal: false`.
3. `global_and_linked_item_appears_once` — item C is global **and** linked; returned exactly
   once with `linked: true` (dedup guard).
4. `include_global_false_matches_legacy_scope` — `Without` returns exactly the linked set,
   asserting parity with the pre-change filter.
5. `include_global_only_returns_globals_regardless_of_link` — `Only` returns B and C, not A.
6. `search_and_type_filters_apply_to_unioned_globals` — `search=` narrows within the union
   (guards the union-before-filter ordering from step 2).
7. `invalid_include_global_value_is_rejected` — `IncludeGlobal::parse(Some("bogus"))` is
   `Err`, and `parse(None)` is `Ok(With)`.

### 5.2 Integration tests — 2, appended to `src-tauri/tests/vault_integration.rs`

Same `tempdir()` + `#[tokio::test]` style as the existing 7 tests.

8. `test_db_list_items_preserves_is_global_flag` — `upsert_item(..., true)` and
   `upsert_item(..., false)`; assert tuple index 4 round-trips per row
   (`db/mod.rs:299-320`). Guards the data path the API filter now depends on.
9. `test_db_set_item_global_roundtrip` — `set_item_global(id, true)` (`db/mod.rs:374`) then
   `list_items()` reflects it; flip back to `false` and re-assert.

### 5.3 Deferred to issue #11's harness — 1

`http_list_items_includes_unlinked_global` — full `GET /items?project=…&environment=…`
against a booted router with an unlocked temp vault, asserting the JSON contains
`"isGlobal":true,"linked":false`. Track as a checklist item on #11 rather than a blocker here.

### 5.4 Manual verification (the issue's repro, inverted)

1. Create a global item, link it to nothing.
2. `curl 'https://127.0.0.1:47821/items?project=X&environment=Y'` → item present,
   `linked:false`.
3. Same with `&include_global=false` → absent.
4. `crypt-env search <name>` → present, `SCOPE = global`.
5. `POST /fill` with a template referencing that key → key **not** filled, reported as a
   warning (unchanged behaviour — proves the materialization split holds).

---

## 6. Risks and monitoring

| Risk | Early warning | Mitigation |
|---|---|---|
| A consumer treated `GET /items` as the materialization set and silently changes behaviour | Drift/diff scripts stop reporting missing keys | `linked` flag + `include_global=false` + explicit doc row; call it out in the PR description and release notes |
| Item-name collisions between a global and a linked item confuse name-based lookups (`crypt_env_generate_env`, `crypt-env cmd/exec`) | Wrong value injected, or "ambiguous name" reports | Linked-wins tie-break plus stderr warning (step 5); the existing name-vs-key mismatch note in `docs/reference.md` already flags this family of bugs |
| Larger `/items` responses in vaults with many globals | Noticeably slower CLI/MCP listings | `decrypt_all_items` already decrypts the whole vault on every call — the union adds no decryption work, only response size. Revisit only if measured |
| Perceived "secret leakage across projects" | User confusion in review | No plaintext is added: `redact_item` still strips `value`/`password`/`content`, and `GET /items/:id` is already unscoped — scope is documented as a display filter, not an access boundary |
| Scope creep into item-creation semantics | PR diff touching `handle_create_item` | Explicitly out of scope (§4.5); open a follow-up issue |

**Assumption that would invalidate this plan:** that `is_global` is intended as
"available to link", not "implicitly present". If the project owner confirms that reading,
Alternative B (§4.3) becomes the correct choice and this plan should be replaced — the
implementation cost is comparable, but the documentation and MCP-tool surface differ
substantially. **This is the one question worth confirming before writing code.**
