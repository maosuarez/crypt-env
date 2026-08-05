# Issue #11 — Test coverage baseline, shared test harness, and the stale Postman collection

**Label:** tech-debt
**Scope of this plan:** the *test harness*, the *coverage baseline*, and the *Postman decision*.
This plan deliberately does **not** define fix behaviour for issues #7, #8, #9, #10, #12, #13 — it
only provides the scaffolding their regression tests will consume. Where a sibling plan needs a
fixture, this document is the authority on its name and signature.

---

## 0. Verified starting state

Measured against `fix/updater-signing-pipeline` (parent commit `1641f97`), not assumed:

| Fact | Value |
|---|---|
| Test functions in the whole crate | **17** — `src/vault/import.rs` (5 `#[test]`), `src/crypto/mod.rs` (4 `#[test]`), `src/db/mod.rs` (1 `#[tokio::test]`, legacy-workspace backfill only), `tests/vault_integration.rs` (7 `#[tokio::test]`) |
| Issue's "9 `#[test]`" figure | Correct if you count only `#[test]` inside `src/` (5 + 4). The 8 `#[tokio::test]` cases were not counted. |
| `src/api/mod.rs` | 3122 lines, **36** `async fn handle_*`, 0 tests |
| `src/project/mod.rs` | 525 lines, 0 tests |
| `src/vault/mod.rs` | 1149 lines, 0 tests |
| `src/share/mod.rs` | 882 lines, 0 tests |
| `src/bin/crypt-env/` | 4037 lines across 22 files, 0 tests |
| `src/bin/crypt-env-mcp.rs` | 3136 lines, 0 tests |
| Coverage tooling | none installed, none configured |
| CI | `.github/workflows/release.yml` only — triggers on tag push / `workflow_dispatch`, builds Windows + macOS bundles. **There is no test job and no PR trigger anywhere in the repo.** |
| Local toolchain | `cargo 1.97.0` in WSL; `pkg-config --exists webkit2gtk-4.1` succeeds, so the Tauri lib compiles and tests run natively in WSL. `CARGO_TARGET_DIR` is *not* exported in non-login shells despite `CLAUDE.local.md` — see step 1.6. |
| Postman collection | `src-tauri/tests/crypt-env-api.postman_collection.json`, 81 KB. Asserts `item_count` on `GET /health` at lines 213–216 (field removed). 0 of 15 request URLs pass `environment_id`, `project`, or `environment`. |

Two structural facts drive every decision below:

1. **`ApiState` fields are private, every `handle_*` is a private `async fn`, and the router is built
   inline inside `start_server` (L3054–3091).** Nothing in `api/` is reachable from an external
   integration-test crate today.
2. **`vault::migrate_literal_vars_to_items` (L134), `vault::decrypt_item`/`encrypt_item`, and
   `share::import_plain_items_into_vault` (L656) are `pub(crate)`.** External integration tests in
   `src-tauri/tests/` *cannot* see them at any visibility short of `pub`.

---

## 1. Objective — measurable definition of done

### 1.1 Tooling

`cargo-llvm-cov` is installed, pinned in CI, and driven by a single committed command:

```
cargo llvm-cov --lib --bins --no-fail-fast --ignore-filename-regex '(^|/)(tests|test_support)/' --summary-only
```

Coverage is **line coverage of the lib + the two bin targets**. The frontend (`src/`, TypeScript) is
out of scope for this issue.

### 1.2 Per-module line-coverage targets

Baseline is recorded in Phase 0 by running the command above *before any test is written* and
committing the numbers into the PR description (not into a file — CLAUDE.md forbids extra `.md`s).
Expected baseline is ~0% for every row below; the target column is the merge gate.

| File | Baseline (expected) | Target | Why not higher |
|---|---|---|---|
| `src-tauri/src/api/mod.rs` | 0% | **≥ 55%** | ~1200 of 3122 lines are `handle_share_*` / `handle_relay_*` / `handle_workspace_relay_*` (L1503–1910, L2245–3040), which do mDNS discovery, x25519 pairing and live Supabase HTTPS calls. Testing those needs a network double, which is out of scope here. |
| — non-network subset of `api/mod.rs`: `resolve_scope`, `validate_create`, `validate_update`, `redact_item`, `environment_item_ids`, `verify_token`, `handle_health`, `handle_unlock`, `handle_list_items`, `handle_get_item`, `handle_create_item`, `handle_update_item`, `handle_delete_item`, `handle_reveal_item`, `handle_fill`, `handle_list_categories`..`handle_delete_category`, `handle_list_commands`, `handle_get_command`, `handle_get_settings`, `handle_put_settings`, `handle_list_projects`, `handle_save_project`, `handle_delete_project`, `handle_preview_delete_project`, `handle_save_environment`, `handle_delete_environment`, `handle_inject_environment`, `handle_environment_example` | 0% | **≥ 80%** | This is the real gate. The whole-file number is diluted by the network handlers. |
| `src-tauri/src/project/mod.rs` | 0% | **≥ 75%** whole file, **≥ 90%** over L95–L360 | L369–434 are `#[tauri::command]` wrappers needing a Tauri runtime; L416 and L459–525 use `rfd` native dialogs. Neither is unit-testable without a window. |
| `src-tauri/src/vault/mod.rs` | 0% | **≥ 50%** | Same reason — the file mixes pure logic with `#[tauri::command]` wrappers. `migrate_literal_vars_to_items`, `create_project_item`, `set_item_global` fork logic must each be ≥ 90%. |
| `src-tauri/src/share/mod.rs` | 0% | **≥ 25%** whole file, **≥ 85%** on `import_plain_items_into_vault` | The other 800 lines are mDNS + x25519 session handshakes. |
| `src-tauri/src/db/mod.rs` | ~8% | **≥ 60%** | |
| `src-tauri/src/bin/crypt-env/commands/scope.rs` | 0% | **≥ 70%** | `fetch_projects` (L118) does a blocking HTTP call to the local API; excluded. |
| `src-tauri/src/bin/crypt-env-mcp.rs` | 0% | **≥ 15%** whole file; **100%** on `append_scope_params` (L798) and `is_safe_env_key` (L816) | The 14 tool handlers are thin `reqwest::blocking` wrappers over the REST API already covered above; duplicating them here buys nothing. |

### 1.3 New test-case count

**115 new test functions**, taking the crate from 17 to 132. Distribution is fixed per phase:

| Phase | Location | Cases |
|---|---|---|
| 1 | `src/api/tests/units.rs` — `redact_item` 3, `validate_create` 8, `validate_update` 4, `environment_item_ids` 2 | 17 |
| 1 | `src/project/mod.rs` in-file — `resolve_environment` | 10 |
| 1 | `src/db/mod.rs` in-file — `upsert_environment_var` | 6 |
| 2 | `src/api/tests/scope.rs` | 12 |
| 2 | `src/api/tests/items.rs` | 10 |
| 2 | `src/api/tests/fill.rs` | 8 |
| 3 | `src/api/tests/projects.rs` | 8 |
| 3 | `src/project/mod.rs` in-file — `inject_environment` 6, `save_environment` multi-owner guard 4 | 10 |
| 3 | `src/vault/mod.rs` in-file — `migrate_literal_vars_to_items` 4, `create_project_item` 3, `set_item_global` fork 5 | 12 |
| 4 | `src/share/mod.rs` in-file — `import_plain_items_into_vault` | 6 |
| 4 | `src/bin/crypt-env/commands/scope.rs` in-file | 9 |
| 4 | `src/bin/crypt-env-mcp.rs` in-file | 7 |
| **Total** | | **115** |

### 1.4 Non-numeric completion criteria

- A shared harness exists at `src-tauri/src/test_support/mod.rs` with the exact API in §3.2, and at
  least one test in each of `api/`, `project/`, `vault/`, `share/` consumes it.
- `.github/workflows/test.yml` runs on every `push` and `pull_request` and fails the build on any
  failing test (Phase 1) and on any coverage regression (Phase 4).
- `src-tauri/tests/crypt-env-api.postman_collection.json` is deleted and `docs/reference.md` §REST API
  carries the replacement examples (§5).
- Zero production-code behaviour changes. The only non-test edits are the `start_server` extraction
  in §3.1 and the `resolve_environment_id` split in §3.6. Both are pure refactors; any behaviour
  delta is a bug in this PR.

---

## 2. What is being mitigated

Stated so each item is checkable by pointing at a named test.

**R1 — Silent scope-authorization drift.** `resolve_scope` (api/mod.rs L237) → `project::resolve_environment`
(project/mod.rs L214) is the single function deciding *which environment's secrets an authenticated
caller may read*. It has three input paths (`environment_id`, `project`+`environment`, project-only →
default env) and today nothing asserts that an unresolvable or mismatched scope produces 422 rather
than falling back to "all items". A regression here is not a 500 — it is a cross-project secret leak
that returns HTTP 200. Mitigated by `api/tests/scope.rs` (12 cases) + `project::resolve_environment`
(10 cases).

**R2 — `ON CONFLICT` semantics flipping from repoint to insert.** `db::upsert_environment_var`
(db/mod.rs L1152) carries `ON CONFLICT(environment_id, key) DO UPDATE SET item_id = excluded.item_id`.
If that clause is ever dropped or the unique index changes, an environment silently accumulates two
rows for the same key and injection becomes order-dependent. Mitigated by the 6 `upsert_environment_var`
cases, which assert row *count* as well as `item_id`.

**R3 — Regression of the four bugs already fixed in the projects/environments pass.**
Line-preservation in `handle_fill`, 409-on-duplicate-project, collision-skip in
`share::import_plain_items_into_vault`, and mandatory-scope enforcement all currently have zero
protection. Three of the four are shapes a `#[tokio::test]` against `handle_fill`, `resolve_scope`
and `import_plain_items_into_vault` catches directly. Mitigated by `api/tests/fill.rs`,
`api/tests/projects.rs`, and the `share` in-file cases.

**R4 — Data loss on the one-shot literal migration.** `migrate_literal_vars_to_items` runs once per
install, gated by `settings['migrated_literals_v1']`, and rewrites user data with the master key held
in memory. It cannot be re-run to recover from a bug. Mitigated by 4 cases including an
already-migrated no-op case and a zero-owner-promotes-to-global case.

**R5 — No PR gate.** Nothing runs `cargo test` before merge today. Even the 17 existing tests can rot
undetected. Mitigated by `.github/workflows/test.yml`.

**R6 — A contract artifact that lies.** The Postman collection asserts a field that no longer exists
and issues 15 requests that now all 422. Anyone using it to learn the API is actively misled.
Mitigated by §5.

---

## 3. Implementation steps

### 3.1 Decision: how the API layer is made testable

Three strategies were on the table. **Recommendation: (B), in-crate `#[cfg(test)]` tests driving the
plain `axum::Router` via `tower::ServiceExt::oneshot`.**

**(A) Widen visibility — `pub fn build_router(state: Arc<ApiState>) -> Router` + `pub fn ApiState::new` — and test from `src-tauri/tests/api_*.rs`.**
Matches the existing `tests/vault_integration.rs` convention. Rejected on three counts:
- It widens the crate's public API purely for test access, and the widened items are the *entire REST
  surface*. CLAUDE.md's decoupling rule exists to keep seams intentional; a `pub` router is a seam no
  production caller wants.
- It still cannot reach `redact_item`, `validate_create`, `environment_item_ids`,
  `vault::migrate_literal_vars_to_items` or `share::import_plain_items_into_vault` — all private or
  `pub(crate)`. Those would need in-file tests *anyway*, splitting the suite across two conventions.
- Every file under `tests/` becomes its own binary that links the whole crate — Tauri, `libsqlite3-sys`
  bundled C, rustls, ratatui. Eight such files means eight full links. Measured link cost dominates
  this crate's test time.

**(B) In-crate `#[cfg(test)] mod tests;` submodule under `src/api/tests/`, plus a `#[cfg(test)]`-gated
`src/test_support/mod.rs`. — CHOSEN.**
Zero visibility changes to production items. Full access to private handlers, private `ApiState`
fields and `pub(crate)` domain functions. One test binary for the whole lib. One import path
(`crate::test_support`) for every sibling issue's tests. The existing `tests/vault_integration.rs`
stays exactly as-is — it only touches genuinely public API and is still a useful smoke test that the
public surface compiles standalone.

**(C) Spin the real HTTPS server on `127.0.0.1:47821` and drive it with `reqwest`.** Rejected: the
port is a fixed constant, so tests cannot run in parallel or alongside a running app; it needs
`tls::ensure_tls_config` to mint a self-signed cert and the client to trust it; and it converts unit
failures into timeouts. It tests axum's TCP stack, not our logic.

**(D) Test only the pure helpers, skip handlers.** Rejected: R1 and R3 live *in* the handlers.

**The single required refactor.** `api::start_server` (L3042) is split so the router construction is
separable, with the socket address and TLS staying where they are:

```
async fn start_server(vault, app_data_dir)      // unchanged signature, still pub
  ├── fn build_router(state: Arc<ApiState>) -> Router   // private (not pub) — all 36 .route() calls + cors_guard layer
  └── (unchanged) const ADDR = "127.0.0.1:47821"; tls::ensure_tls_config; axum_server::bind_rustls
```

`build_router` stays **private**; `#[cfg(test)] mod tests;` inside the same module reaches it. **The
CLAUDE.md rule that the API listens only on `127.0.0.1:47821` is untouched**: the address literal and
the only call to `bind_rustls` remain inside `start_server`, no test binds a socket at all, and
nothing test-only is compiled into a release build (`#[cfg(test)]` is absent from `cargo build`).

`ApiState` gains a private `fn new(vault: SharedState) -> Self` so `start_server` and the harness
build it identically — no duplicated initialisation to drift.

### 3.2 The shared harness — `src-tauri/src/test_support/mod.rs`

**Sibling issue plans (#7, #8, #9, #10, #12, #13): this is the fixture API. Use it; do not invent
another.** Declared in `lib.rs` as `#[cfg(test)] mod test_support;`, so it never exists in a release
build. `unwrap()`/`expect()` are **permitted here and in all `#[cfg(test)]` modules** — CLAUDE.md's
ban applies to production code; a panicking fixture is a failing test, which is correct behaviour.

```rust
pub struct TestVault {
    pub dir: tempfile::TempDir,   // MUST stay alive: dropping it deletes the sqlite file
    pub state: crate::vault::SharedState,
    pub token: String,            // value for the `X-Vault-Token` header
    pub master_password: String,  // "test-master-password-1"
    pub project_id: i64,          // project "demo"
    pub env_id: i64,              // environment "production" of "demo", is_default = true
    pub item_ids: Vec<i64>,       // the seeded items, in insertion order
}

/// Tempdir + VaultDb::open + init_vault_crypto + key installed in VaultState +
/// project "demo" with environments "production" (default) and "local" +
/// 3 items (DB_HOST, DB_PASSWORD, API_KEY) linked into "production" +
/// 1 global item (SHARED_TOKEN, is_global = true, linked to nothing) +
/// settings['mcp_token'] seeded so `token` authenticates immediately.
pub async fn unlocked_vault() -> TestVault;

/// Same crypto setup, no projects / environments / items. For tests that need
/// to assert creation from empty, or 422-on-unresolvable-scope.
pub async fn unlocked_vault_empty() -> TestVault;

/// Locked vault (`state.key == None`) for 403 VAULT_LOCKED assertions.
pub async fn locked_vault() -> TestVault;

/// The plain axum Router with state — no TLS, no socket.
pub fn router(v: &TestVault) -> axum::Router;

/// One request through `ServiceExt::oneshot`. `token: None` omits the header.
/// Returns the status plus the parsed JSON body (`Value::Null` for empty bodies).
pub async fn req(
    app: &axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<serde_json::Value>,
) -> (axum::http::StatusCode, serde_json::Value);

/// Real POST /unlock round-trip returning a session token, for the tests that
/// need session semantics (expiry, rate limiting) rather than the static token.
pub async fn session_token(app: &axum::Router, v: &TestVault) -> String;

pub async fn seed_item(v: &TestVault, name: &str, value: &str, is_global: bool) -> i64;
pub async fn seed_project(v: &TestVault, name: &str, envs: &[&str]) -> (i64, Vec<i64>);
pub async fn link_var(v: &TestVault, env_id: i64, key: &str, item_id: i64) -> i64;

/// Reads an item straight out of the DB and decrypts it — for asserting what was
/// actually persisted, independent of what the handler echoed back.
pub async fn read_item(v: &TestVault, id: i64) -> crate::vault::VaultItem;
```

**Auth mechanism, and why.** `verify_token` (L110) falls back to the static `settings['mcp_token']`
when the session token doesn't match. Seeding that setting gives every test a working credential with
no `/unlock` round-trip, which keeps the unlock rate limiter (`unlock_rate`, 5-attempt window) out of
unrelated tests. Tests that specifically exercise session-token behaviour call `session_token()`.

**`cors_guard` note for consumers:** the middleware (L84) allows a request with no `Origin` header, so
`req()` sends none. A test asserting the CORS guard must set `Origin` explicitly.

**Dependency-direction note:** `test_support` sits *above* `db`, `vault`, `project` and `api` and is
imported by their test modules only. It does not create a `db → api` edge; `db` remains unaware of
`api` in all non-test builds, satisfying CLAUDE.md.

### 3.3 `Cargo.toml` changes

```toml
[dev-dependencies]
tempfile = "3"
tokio = { version = "1", features = ["rt", "macros"] }
tower = { version = "0.5", features = ["util"] }   # NEW — ServiceExt::oneshot
```

Only `tower` is added. Version skew against axum 0.7's internal tower is a non-issue: `ServiceExt` is
a blanket extension over `tower_service::Service`, which is `0.3` in both trees. `http-body-util` is
not needed — axum 0.7 provides `axum::body::to_bytes`. `serde_json` and `axum` are already normal
dependencies, so in-crate test modules use them without further dev-deps (another concrete win of
strategy B).

### 3.4 Files touched

Production edits (refactor only, no behaviour change):

- `src-tauri/src/api/mod.rs` — extract `build_router`, add `ApiState::new`, add `#[cfg(test)] mod tests;`
- `src-tauri/src/lib.rs` — add `#[cfg(test)] mod test_support;`
- `src-tauri/src/bin/crypt-env-mcp.rs` — split `resolve_environment_id` (§3.6)
- `src-tauri/Cargo.toml` — one dev-dependency

New test files:

- `src-tauri/src/test_support/mod.rs`
- `src-tauri/src/api/tests/mod.rs` (declares the four below)
- `src-tauri/src/api/tests/units.rs`, `scope.rs`, `items.rs`, `fill.rs`, `projects.rs`

In-file `#[cfg(test)] mod tests` appended to: `src/project/mod.rs`, `src/vault/mod.rs`,
`src/share/mod.rs`, `src/bin/crypt-env/commands/scope.rs`, `src/bin/crypt-env-mcp.rs`.
Extended: the existing `mod tests` in `src/db/mod.rs` (L1209).

Deleted: `src-tauri/tests/crypt-env-api.postman_collection.json`.
Untouched: `src-tauri/tests/vault_integration.rs`.

### 3.5 Test-case specifications

Behaviour is asserted **as it exists on `main` today**, except where a sibling issue is explicitly
fixing it — in those spots this plan writes the harness and the sibling plan writes the assertion.

`api/tests/scope.rs` (12): resolves by `environment_id`; resolves by `project`+`environment`;
resolves by `project` alone → default environment; case-insensitive project name;
case-insensitive environment name; no scope params at all → 422 `VALIDATION_ERROR`; unknown
`environment_id` → 422; unknown project name → 422; known project + unknown environment → 422;
`environment_id` present *and* `project`/`environment` present → documented precedence; missing token
→ 401 `UNAUTHORIZED`; locked vault + valid token → 403 `VAULT_LOCKED`.

`api/tests/items.rs` (10): `GET /items` returns only items linked into the scoped environment;
`GET /items` never returns `value`/`password`/`content` (asserts `redact_item` on the wire — this is
the CLAUDE.md "no plaintext in API responses" rule made executable); `GET /items?search=` filters
within scope; `GET /items/:id` for an out-of-scope id → 404; `POST /items` persists an encrypted blob
(assert via `read_item`, and assert the raw DB column is not the plaintext); `POST /items` with a
missing `name`/`value`/bad `type` → 422 with the offending field named; `PUT /items/:id` partial
update leaves other fields intact; `DELETE /items/:id` unlinks and removes;
`POST /items/:id/reveal` is the *only* endpoint returning a plaintext value; reveal on an
out-of-scope id → 404.

`api/tests/fill.rs` (8): a key present in the environment is substituted; a key **absent** from the
environment leaves that line byte-identical (the line-preservation bugfix — R3); comments and blank
lines preserved; trailing-newline presence preserved; CRLF input round-trips; duplicate keys in the
input; empty input; unresolvable scope → 422 before any file content is touched.

`api/tests/projects.rs` (8): `GET /projects` lists with nested environments; `POST /projects`
creates; `POST /projects` with an existing name → **409** (R3); `POST /projects` updating by id is
not a duplicate; `DELETE /projects/:id` cascade; `GET /projects/:id/preview-delete` counts match what
`DELETE` then does; `POST /environments` multi-owner grant guard; `DELETE /environments/:id`.

`project::resolve_environment` (10) and the other in-file suites mirror the same axes at the function
level, without HTTP — cheaper failures and clearer blame when both layers break.

`project::inject_environment` (6) writes to real paths under `tempfile::tempdir()`; it must assert
that the written file's parent stays inside the tempdir (the fixture #7 will build its traversal
assertions on).

### 3.6 Making the MCP binary's scope logic testable

`resolve_environment_id` (crypt-env-mcp.rs L1845) mixes argument parsing with a blocking
`fetch_projects` HTTP call, so it cannot be unit-tested. Split it:

```
fn resolve_environment_id(args, token) -> Result<i64, Value>     // unchanged signature; now = fetch + delegate
fn pick_environment_id(args: &Value, projects: &Value) -> Result<i64, Value>   // pure, testable
```

`append_scope_params` (L798) and `is_safe_env_key` (L816) are already pure — in-file tests only. Same
for `scope::parse_project_config` and `scope::resolve` in the CLI binary; only `fetch_projects`
(scope.rs L118) stays untested, and the `allow_create` gate at L158 is asserted through `resolve`
with both flag values. Bin-target tests run under `cargo test --bins`.

---

## 4. Phasing

The repo's convention is scoped PRs; this is four of them. Each is independently mergeable and green.

| Phase | PR title | Contents | Gate added |
|---|---|---|---|
| **0** | *(no PR)* | Install `cargo-llvm-cov`, run the baseline command, paste numbers into the Phase 1 PR body | — |
| **1** | `test(api,db,project): add shared harness and pure-function coverage` | `test_support/`, the `build_router` extraction, `Cargo.toml` dev-dep, 33 cases (`api/tests/units.rs`, `resolve_environment`, `upsert_environment_var`), `.github/workflows/test.yml` running `cargo test --lib --bins` | **CI fails on any test failure** |
| **2** | `test(api): router-level coverage for scope, items and fill` | 30 cases via `oneshot` | — |
| **3** | `test(api,project,vault): projects, environments and item-ownership coverage` | 30 cases | — |
| **4** | `test(share,cli,mcp): collision, scope-resolution and helper coverage; drop stale Postman collection` | 22 cases, the `pick_environment_id` split, collection deletion, `docs/reference.md` REST examples | **CI fails on coverage below the ratcheted floor** |

Phase 1 is the blocking dependency for issues #7–#13 — their plans should target `crate::test_support`
and can begin as soon as it lands, in parallel with Phases 2–4.

**CI job (`.github/workflows/test.yml`), new, `on: [push, pull_request]`, `ubuntu-latest`:**
checkout → `dtolnay/rust-toolchain@stable` → `Swatinem/rust-cache` → **apt install of the Tauri Linux
system deps** (`libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
libsoup-3.0-dev build-essential`) → `cargo test --lib --bins`. Phase 4 appends
`taiki-e/install-action@cargo-llvm-cov` and the coverage run with `--fail-under-lines` set to the
**achieved** figure minus 2 points, per-file thresholds enforced by a small `jq` step over
`--json --summary-only` output. The floor is a ratchet against regression, never an aspiration —
setting it above what the suite actually achieves makes the gate a nuisance and it gets disabled.

`release.yml` is not modified; it stays tag-triggered.

---

## 5. Decision on the Postman collection

**Delete `src-tauri/tests/crypt-env-api.postman_collection.json`. `docs/reference.md` becomes the
single documented contract; the `api/tests/*` suite becomes the executable one.**

Reasoning. The collection has two jobs — documenting the contract and verifying it — and it is now
failing both. It verifies nothing because no process ever runs it: there is no `newman` step in
`release.yml` and no other CI. It documents wrongly because it asserts a removed `item_count` field
and none of its 15 requests carry the now-mandatory scope parameter, so every one of them 422s. A
contract artifact that nothing executes will always drift; it just did, silently, across a whole
migration. Meanwhile `docs/reference.md` already has a REST API section, and after Phase 2 the router
tests assert the real status codes and payload shapes on every commit.

**Alternative considered — regenerate and wire `newman` into CI.** Rejected on cost and on security:
it needs a live server, which needs `tls::ensure_tls_config` to mint a self-signed cert plus a client
configured to trust it, plus a vault initialised with a master password held in CI — exactly the
shape CLAUDE.md's "master password only in memory, never persists" rule pushes back on. It would
duplicate coverage the `oneshot` tests already provide, at higher operational cost.

**Alternative considered — regenerate and leave it unexecuted.** Rejected: identical to the current
state, with the clock reset. It re-rots at the next contract change.

**The condition for ever bringing it back,** stated so the decision is reversible on purpose rather
than by accident: a Postman collection may return only together with a test that *executes* it —
parse the collection JSON in `api/tests/`, replay each request through the same `oneshot` router, and
assert no request returns 404 or 422. That makes staleness a build failure. Without that test,
re-adding the file recreates R6.

**Replacement content** (Phase 4, inside the existing `docs/reference.md`, no new file):
`curl` examples for `/unlock`, `/items` (GET + POST), `/items/:id/reveal`, `/fill`, `/projects`,
`/environments/:id/inject`, each showing the mandatory scope parameter in both forms
(`?environment_id=` and `?project=&environment=`), and a one-line note that the server is HTTPS on
`127.0.0.1:47821` with a self-signed certificate.

---

## 6. Trade-offs and risks

### 6.1 Accepted trade-offs

| Gaining | Losing |
|---|---|
| Zero widening of the crate's public API for test purposes | Test code lives inside `src/`, which conflicts with the existing `tests/vault_integration.rs` convention. Mitigated by keeping that file untouched — the two conventions coexist with a clear rule: *external tests for public API, in-crate tests for private/`pub(crate)` logic.* |
| One test binary instead of ~8 links of a Tauri-sized crate | `src/api/mod.rs` grows a `mod tests;` declaration; the api test code is ~1000 lines across 5 files under `src/`. `cargo build` is unaffected (`#[cfg(test)]`). |
| `oneshot` tests run in milliseconds, in parallel, with no port binding | The TLS layer, `axum_server::bind_rustls`, and `tls::ensure_tls_config` remain untested. Accepted: that is dependency code, and a manual smoke test against the running app covers it. |
| Static `mcp_token` auth in the fixture — no unlock round-trip | Rate-limiting and session expiry are only covered by the explicit `session_token()` tests, not incidentally by every test. Accepted: incidental coverage of a rate limiter makes suites flaky. |
| Coverage floor set as a ratchet on achieved numbers | It will not force coverage upward on its own. Accepted: aspirational gates get disabled. |
| Deleting the Postman collection | Loss of a click-to-explore artifact for manual API exploration. Mitigated by the `curl` examples; reversible under the §5 condition. |

### 6.2 Risks and early warning signs

**Risk: the CI test job fails on `ubuntu-latest` for missing Tauri system deps.** The lib depends on
`tauri = "2"`, which needs `libwebkit2gtk-4.1-dev` on Linux. Locally this is already satisfied
(verified). Warning sign: a `pkg-config` failure in the first CI run. Mitigation: the apt step is in
the Phase 1 job from the start. Fallback if it proves slow or brittle: `cargo test --lib --bins`
under `windows-latest`, matching the project's actual build target, at higher runner cost.

**Risk: the `build_router` extraction silently changes routing or middleware order.** It moves 36
`.route()` calls and one `.layer(middleware::from_fn(cors_guard))`. Warning sign: any test in
Phase 2 getting an unexpected 404 or 403. Mitigation: Phase 1 lands the extraction with the
handler-level tests already exercising every route in Phase 2 immediately after; review the diff for
`+`/`-` symmetry on the route list specifically.

**Risk: fixture coupling.** Six sibling issues will depend on `TestVault`'s seeded shape. Changing
"3 items in `production`" later breaks all of them at once. Mitigation: tests assert against
`v.item_ids` and `v.env_id`, never against literal ids or counts; additions go through
`seed_item`/`seed_project` inside the test that needs them, never by editing `unlocked_vault()`.
Warning sign: a sibling PR that modifies `test_support/mod.rs` — that should trigger a look.

**Risk: `TempDir` dropped early.** `TestVault` owns the `TempDir`; if a test destructures it away, the
sqlite file vanishes mid-run and failures look like DB corruption. Mitigation: documented on the
struct field; helpers all take `&TestVault`, never move out of it.

**Risk: a test leaks a secret value into CI logs.** Tests handle real plaintext by design. Mitigation:
fixture values are obvious dummies (`test-master-password-1`, `dummy-secret-*`); no test prints an
item value on the success path, and assertion failure messages compare booleans/ids rather than
dumping decrypted payloads.

**Assumptions that, if wrong, invalidate this plan:** (a) the Tauri lib compiles and its tests run
under `cargo test` on Linux without a display server — verified locally, must be re-verified in CI at
Phase 1; (b) `cargo-llvm-cov` handles the bundled `libsqlite3-sys` C code and the `staticlib`/`cdylib`
crate types without extra configuration — if it does not, the fallback is `--lib`-only coverage with
the bin targets covered by test count rather than percentage; (c) no sibling issue's fix changes
`resolve_environment`'s signature before Phase 1 merges — if one does, this plan's `resolve_scope`
tests are written against the new signature instead, not the old.

### 6.3 Rollback

Every phase is a self-contained PR containing only test code plus two pure refactors. Reverting any
phase restores the previous state with no data-model, schema or API-contract implication. The only
irreversible-feeling step is deleting the Postman collection; it remains recoverable from git history
at `1641f97:src-tauri/tests/crypt-env-api.postman_collection.json`.

### 6.4 Explicitly out of scope

Fix behaviour for #7, #8, #9, #10, #12, #13; frontend (TypeScript/Vitest) tests; network doubles for
the relay/share handlers; TUI tests; property-based or fuzz testing; and any performance benchmark.
