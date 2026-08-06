# Issue #10 — MCP: unify the environment-scope parameter name on `environment_id`

**Type:** bug (interface naming) · **Surface:** MCP server only (`src-tauri/src/bin/crypt-env-mcp.rs`) · **Status:** plan, not implemented

---

## 1. Objective

`crypt_env_inject_environment` and `crypt_env_generate_example_env` are the only two MCP
tools that name the environment identifier `id`. Every other environment-scoped tool names
it `environment_id`. This plan makes `environment_id` the single advertised name for
"an environment identifier" across the whole tool list, keeps `id` working as an
**unadvertised** alias for the 1.0.x line only, and adds a regression test over the static
tool-list JSON so the divergence cannot come back.

### Definition of done

**Schemas changed (exactly two).** In `tool_definitions()` (`src-tauri/src/bin/crypt-env-mcp.rs:195`):

| Tool | Schema at | Change |
|---|---|---|
| `crypt_env_inject_environment` | `:519-531` | property `id` → `environment_id`; `project`, `environment`, top-level `description` re-worded to the canonical strings |
| `crypt_env_generate_example_env` | `:533-545` | identical change |

No other tool's schema is touched. `crypt_env_get_item`, `crypt_env_update_item`,
`crypt_env_delete_item`, `crypt_env_update_category`, `crypt_env_delete_category` and
`crypt_env_share_workspace_send` keep their bare `id` — there it means an *item*,
*category* or *workspace* id and is correct.

**Canonical strings** (already byte-identical across all 13 conforming tools — verified at
`:205/229/243/257/278/343/361/379/397/411/463/569/690`; the implementation copies them
verbatim, it does not paraphrase):

```
environment_id → "Environment ID (scope). Provide this, or both 'project' and 'environment'."
project        → "Project name (case-insensitive). Used with 'environment' when 'environment_id' is not given."
environment    → "Environment name within the project (case-insensitive), e.g. production, local, test. Used with 'project'."
```

**Resolver behaviour** (`resolve_environment_id`, `:1845`), in strict order:

1. `args["environment_id"]` as `i64` → resolve, no deprecation flag.
2. else `args["id"]` as `i64` → resolve, **flag as deprecated** (not silent — see below).
3. else `project` + `environment` name pair → `fetch_projects()` + case-insensitive match (unchanged).
4. else → `Err(tool_err("required: 'environment_id' (environment id), or 'project' + 'environment' (names)"))`.

The alias is **not silent**: the resolver returns a small struct rather than a bare `i64`, and
both call sites append a one-line deprecation notice to the *successful* tool response text.
The MCP protocol gives no other warning channel, and the response text is what the model
actually reads — a silent alias would keep teaching the wrong name indefinitely.

```rust
/// Outcome of resolving an environment identifier from MCP tool args.
struct ResolvedEnvironment {
    id: i64,
    /// True when the caller used the deprecated `id` key instead of `environment_id`.
    /// Callers surface this to the model; remove together with the alias in 1.1.0.
    via_deprecated_id: bool,
}
```

Notice text (fixed string, contains no identifiers and no secret material):
`note: parameter 'id' is deprecated on this tool; use 'environment_id' instead.`

**Deprecation window.** The alias is accepted for the whole **1.0.x** line and **removed in
1.1.0**. Marked in-source with `// DEPRECATED(remove in 1.1.0): environment `id` alias, issue #10`
on the branch, and recorded in `CHANGELOG.md` under both the 1.0.2 entry (alias added,
schema renamed) and — when it lands — the 1.1.0 entry (alias removed). Rationale for a short
window in §4.

**Error strings changed (two).**
- `:1856` → `"required: 'environment_id' (environment id), or 'project' + 'environment' (names)"`
- `:2736` → `"... Use crypt_env_inject_environment with a specific 'environment_id'."`

**Docs changed.**
- `docs/reference.md:245` and `:246` — parameter column `id` → `environment_id`; drop the
  trailing "Note: uses `id`, not `environment_id` like every other scoped tool above — see Notes".
- `docs/reference.md:274` — the "**Scope-parameter naming is inconsistent across tools**"
  note is replaced by a resolved note stating that all environment-scoped tools now use
  `environment_id`, that `id` is a temporary undocumented alias, and when it is removed.
- `CHANGELOG.md` — new `## [1.0.2]` **Changed** entry.
- `context.md:742` needs **no** change (lists tool names only, no parameters).
- `CLAUDE.md` needs no change.

**Tests (new file section — the MCP binary currently has zero tests).** A
`#[cfg(test)] mod tests` block at the end of `src-tauri/src/bin/crypt-env-mcp.rs`, run by
`cargo test --bin crypt-env-mcp` (and by a plain `cargo test`). Five assertions:

1. `every_environment_scoped_tool_declares_environment_id` — iterate `tool_definitions()`;
   treat a tool as environment-scoped iff its `inputSchema.properties` contains **both**
   `project` and `environment` (structurally exact today: 15 tools — the 13 conforming plus
   the 2 outliers; `crypt_env_inject_env_by_name` is excluded because it uses `project_path`,
   not `project`). For each: assert `properties.environment_id` **exists** and
   `properties.id` **does not exist**. This is the literal check the issue asks for.
2. `environment_scope_descriptions_are_canonical` — for the same set, assert the three
   description strings equal the canonical strings above, byte for byte.
3. `id_tools_keep_bare_id` — assert `crypt_env_get_item`, `crypt_env_update_item`,
   `crypt_env_delete_item`, `crypt_env_update_category`, `crypt_env_delete_category` still
   declare `id` and do **not** declare `environment_id`. Guards against an over-eager
   find-and-replace.
4. `resolve_environment_id_prefers_canonical_key` — three pure cases, **no network**:
   `{"environment_id": 7}` → `Ok(id: 7, via_deprecated_id: false)`;
   `{"id": 7}` → `Ok(id: 7, via_deprecated_id: true)`;
   `{"environment_id": 7, "id": 9}` → `Ok(id: 7, via_deprecated_id: false)`.
   Safe because `fetch_projects()` is only reached on the name-pair path; `token` is unused
   on these branches, so `""` is passed.
5. `resolve_environment_id_missing_scope_names_the_canonical_key` — `{}` → `Err`, and the
   error text contains `environment_id` and does **not** contain the substring `'id' (environment id)`.

Done means: `cargo test --bin crypt-env-mcp` passes, `cargo clippy` clean, and
`grep -n "'id'" src-tauri/src/bin/crypt-env-mcp.rs` returns no hit whose subject is an
environment.

---

## 2. What is being mitigated

**Risk removed:** an LLM caller that infers the parameter name from the 13-tool majority
passes `environment_id` to the two outliers. Today that key is ignored, and one of two
things happens:

- *Loud, recoverable:* no `project`/`environment` present → the call fails with
  `required: 'id' ...`, which contradicts the name the model just learned from every other
  tool. Wasted turns, model likely retries with the same wrong key.
- *Silent, not recoverable:* `project` and `environment` **are** also present → the resolver
  ignores `environment_id` entirely and resolves a **different environment by name**.
  `crypt_env_inject_environment` then writes that environment's full decrypted variable set
  to disk. The caller is never told its explicit identifier was discarded. This is the case
  that matters: a wrong-environment secret dump with no error.

A second, narrower hazard is closed by the same change: because `id` is simultaneously the
*correct* name for item ids and category ids on five neighbouring tools, an id copied from
`crypt_env_get_item` and passed as `id` to `crypt_env_inject_environment` is accepted as an
environment id today. After this change, `id` is no longer advertised anywhere on these two
tools, so no model is taught the collision; during the 1.0.x alias window the behaviour is
unchanged but the response now says the key is deprecated, and from 1.1.0 the call fails loudly.

**Checkable statement:** after this change, for every tool in the advertised
`tools/list` payload, the property named `id` never denotes an environment, and every tool
that accepts an environment scope declares `environment_id` with identical wording — asserted
by tests 1–3, which fail the build if either invariant is violated.

**Explicitly not mitigated:** the pre-existing "one call can dump a whole environment to an
arbitrary path" concern documented at `docs/reference.md:266`. This plan does not widen or
narrow that surface; it only fixes which key selects the environment.

---

## 3. Implementation steps

Ordered. Each step is independently compilable; steps 1–3 are one commit, 4 is a second, 5 a third.

### Step 1 — resolver (`src-tauri/src/bin/crypt-env-mcp.rs:1842-1884`)

1.1 Add `struct ResolvedEnvironment { id: i64, via_deprecated_id: bool }` immediately above
`resolve_environment_id` (private to the binary; no `#[derive]` needed beyond `Debug` for
test assertions).

1.2 Change the signature to
`fn resolve_environment_id(args: &serde_json::Value, token: &str) -> Result<ResolvedEnvironment, serde_json::Value>`.
Keep the name — it is accurate and renaming it churns two call sites plus the doc comment for
no gain.

1.3 Replace the first branch (`:1846-1848`) with the two-key lookup, canonical key first,
alias second, alias flagged. Do **not** collapse them into
`.get("environment_id").or_else(|| args.get("id"))` — that form cannot distinguish which key
was used and therefore cannot drive the deprecation notice.

1.4 Update the `Err` at `:1855-1858` to the new message.

1.5 Update the doc comment at `:1842-1844`: it currently says args are "shaped like
`crypt_env_inject_environment`'s schema: `id` directly". Rewrite to name `environment_id`
as canonical, `id` as deprecated-until-1.1.0, and add the
`// DEPRECATED(remove in 1.1.0): environment 'id' alias, issue #10` marker on the alias branch.
`unwrap()` is not introduced anywhere; all lookups stay `Option`-chained per CLAUDE.md.

### Step 2 — call sites

2.1 `tool_inject_environment` (`:1886-1890`) — destructure the struct; keep the local named
`environment_id` so the `/environments/{environment_id}/inject` format string at `:1901` and
the 404 message at `:1916` are untouched.

2.2 `tool_generate_example_env` (`:1929-1933`) — same, for `/environments/{id}/example` at `:1944`.

2.3 In both, when `via_deprecated_id` is true, append the fixed notice line to the success
text. Both functions end in the same shape (`tool_ok(pretty)` / `tool_ok(text)`); append to
the string passed to `tool_ok`, not to the parsed JSON, so the API payload shape is unchanged.
Do **not** append on the error paths — error text is already terminal and the deprecation is
not the cause. The notice is a compile-time constant string: no argument interpolation, so no
path, key, or value can leak into it (CLAUDE.md security rule).

### Step 3 — schemas and the stale error string

3.1 `:519-531` (`crypt_env_inject_environment`) — rename the property, swap in the three
canonical description strings, and re-word the tool-level `description` at `:520` from
"Identify by environment id, or by 'project' + 'environment' names." to
"Requires scope: 'environment_id', or both 'project' and 'environment'." — matching the
phrasing already used at `:198`. Leave `output_path` / `output_dir` alone. Do not add a
`"required"` array (see §4).

3.2 `:533-545` (`crypt_env_generate_example_env`) — identical treatment at `:534`; keep the
"Never reads or returns secret values" sentence, it is load-bearing.

3.3 `:2736` — the ambiguity error inside the `crypt_env_inject_env_by_name` path still tells
the caller to use `crypt_env_inject_environment` "with a specific 'id'". Change to
`'environment_id'`. This string is why the bug survives even for callers who read errors carefully.

3.4 Sweep: `grep -n "'id'" src-tauri/src/bin/crypt-env-mcp.rs` and confirm every remaining hit
refers to an item, category, or workspace id.

### Step 4 — tests (`src-tauri/src/bin/crypt-env-mcp.rs`, new trailing `#[cfg(test)] mod tests`)

4.1 Placement rationale: `crypt-env-mcp` is a `[[bin]]` (`src-tauri/Cargo.toml:106-107`), so
`tool_definitions()` and `resolve_environment_id()` are **not** reachable from
`src-tauri/tests/*.rs` — an integration test there can only spawn the binary. An in-file
`#[cfg(test)] mod tests` is the only zero-restructuring option and needs no new dev-dependency
(`serde_json` is already a normal dep; no `tokio`, no `tempfile`, no process spawn, no network).

4.2 Write a `fn environment_scoped_tools() -> Vec<&serde_json::Value>` test helper applying
the `has "project" && has "environment"` predicate, and a
`const CANON_ENVIRONMENT_ID_DESC/CANON_PROJECT_DESC/CANON_ENVIRONMENT_DESC`. Assert the helper
returns exactly 15 tools — a bare count check that fails loudly if someone adds a scoped tool
without the canonical shape, which is the failure mode this whole issue is about.

4.3 Implement tests 1–5 from §1. Failure messages must name the offending tool
(`assert!(..., "tool {name} declares a bare 'id' for an environment")`) — a bare
`assert_eq!` on a 15-element set is unactionable.

4.4 **Dependency on issue #11.** #11 is planning a general test harness for this repo. Nothing
here blocks on it: these five tests are pure functions over a JSON literal, need no vault, no
`ApiState`, no temp dir, and no async runtime. If #11 lands first and establishes a shared
location for MCP tests, move the module there unchanged; if it lands second, it should adopt
this module as-is rather than duplicating it. Coordinate only to avoid two people writing the
same tool-list assertions.

### Step 5 — documentation

5.1 `docs/reference.md:245`, `:246` — parameter column and trailing note (see §1).

5.2 `docs/reference.md:274` — replace the inconsistency note with:
what the canonical name is, that `id` is accepted-but-unadvertised through 1.0.x, that it is
removed in 1.1.0, and a pointer to the test that enforces it. Keeping a note here (rather than
deleting the paragraph) is deliberate: readers of older transcripts need to know why they saw `id`.

5.3 `docs/reference.md:272` — the `crypt_env_inject_environment` note says "resolves environment
by ID or by project+environment names"; make "ID" read `environment_id` for consistency.

5.4 `CHANGELOG.md` — new `## [1.0.2]` section, `### Changed`, describing the rename, the silent
wrong-environment failure it removes, the alias, and its removal target. Note that `1.0.1`'s
entries stay untouched.

5.5 Do **not** create any additional `.md` file. CLAUDE.md forbids proliferating docs; this
plan file plus the two existing docs is the whole documentation footprint.

### Verification

```bash
cd src-tauri && cargo test --bin crypt-env-mcp && cargo clippy --bin crypt-env-mcp -- -D warnings
```
Manual smoke (Windows side, per CLAUDE.local.md): with the vault unlocked, call
`crypt_env_generate_example_env` with `{"environment_id": <n>}` (expect success, no notice),
then with `{"id": <n>}` (expect success **plus** the deprecation line), then with
`{"environment_id": <n>, "project": "X", "environment": "Y"}` where X/Y is a *different*
environment (expect the `environment_id` environment — the exact case that silently
mis-resolves today). Use `generate_example_env`, not `inject_environment`, for the manual
check: it writes placeholders only and never decrypts, so a mis-resolution during testing
cannot spill real secrets.

---

## 4. Trade-offs and alternatives considered

### 4.1 Deprecation window: alias for one release *(chosen)* vs. drop outright vs. keep forever

**Chosen: accept `id` through 1.0.x, remove in 1.1.0, never advertise it.**

- *Drop outright.* Genuinely attractive, and the strongest argument for it is real: while the
  alias lives, an item id mistakenly passed as `id` still resolves to an unrelated environment.
  Rejected because the cost of keeping it is one branch and one bool, and because the failure
  it prevents (an agent mid-task whose next call suddenly errors) is invisible to us and
  annoying to the user. The deprecation notice narrows the gap: the model is told the key is
  wrong on the very call that used it.
- *Keep forever.* Rejected. A permanent alias means `id` permanently means two different things
  in the same tool list, and dead compatibility branches never get removed once nobody remembers
  why they exist. The dated `DEPRECATED(remove in 1.1.0)` marker plus the CHANGELOG entry is
  the mechanism that makes removal actually happen.
- *Cost accepted:* one extra struct, one bool threaded through two call sites, and a mandatory
  follow-up commit in 1.1.0. If that follow-up is skipped, we are back at "keep forever" by
  neglect — that is the main risk of this choice and the reason the marker is dated rather than
  a vague "TODO".

Note the compat surface really is small: there is no MCP protocol versioning here, the tool list
is a static literal, and MCP clients re-fetch `tools/list` on connect. The only exposure is a
transcript already in flight — minutes, not releases. That is precisely why the window is one
minor version and not more.

### 4.2 Unify `resolve_environment_id` with `append_scope_params` *(rejected — scope creep)*

The two functions duplicate the case-insensitive project/environment fallback, and it is
tempting to extract one canonical scope parser. Rejected for this issue:

- They have genuinely different shapes and different *costs*. `append_scope_params` (`:798`)
  builds URL query params and is a pure string operation that pushes resolution to the API.
  `resolve_environment_id` (`:1845`) must produce a concrete `i64` because its two callers hit
  **path-param** routes (`POST /environments/:id/inject`, `POST /environments/:id/example` —
  `src-tauri/src/api/mod.rs:3085-3086`), so it performs an HTTP `fetch_projects()` round-trip.
  A single function returning "either an id or some query params" is a worse abstraction than
  the two honest ones.
- The bug is a naming bug. Merging two resolution strategies inside the fix means the diff is
  no longer reviewable as "one key renamed", and a regression in scope resolution is far more
  expensive than the inconsistency it would clean up.
- The duplication is small (~15 lines) and now covered by tests 1–2 at the schema level, which
  is where the divergence actually hurt.

If unification is still wanted, it belongs in a separate issue *after* §4.3 is decided — because
if the two outliers stop resolving client-side, `resolve_environment_id` disappears entirely and
there is nothing left to unify.

### 4.3 Push resolution server-side and delete the client-side resolver *(rejected here, worth a separate issue)*

The API already resolves scope from `environment_id` **or** `project`+`environment` via
`resolve_scope` / `project::resolve_environment` (`src-tauri/src/api/mod.rs:214-245`), using the
shared `EnvScopeQuery` extractor. If the two outlier tools forwarded the raw scope args instead
of pre-resolving, `resolve_environment_id` and its `fetch_projects()` round-trip both vanish, and
scope semantics would live in exactly one place (the API) instead of two.

**Cost, stated plainly:** `/environments/:id/inject` and `/environments/:id/example` are
**path-param** routes — `handle_inject_environment` and `handle_environment_example` take
`Path(id): Path<i64>` (`api/mod.rs:2100-2104`, `:2162-2166`). There is no id-less variant. Doing
this requires either (a) new routes (`POST /environments/inject` with `Query<EnvScopeQuery>`),
leaving two ways to call the same operation, or (b) changing the existing routes, which breaks the
CLI and any external caller of the local REST API. Both are API-surface changes needing their own
review, their own tests, and their own CHANGELOG entry — for a bug whose entire content is a
misspelled JSON key. Rejected as out of scope; recommended as a follow-up issue, with (a) plus a
deprecation of the path-param form as the likelier shape.

### 4.4 Add a `"required"` array to the two schemas *(rejected)*

Neither outlier schema declares `required` — resolution is entirely runtime. Adding
`"required": ["environment_id"]` would let the client validate, but it is **wrong**: the
`project`+`environment` pair is an equally valid way to identify the environment, and JSON Schema
`oneOf`/`anyOf` across property groups is exactly the kind of cleverness CLAUDE.md's simplicity
rule pushes back on — MCP clients vary in how much of it they enforce. The 13 conforming tools
also omit `required` for the same reason. Consistency wins; the runtime error message (now naming
`environment_id`) is the contract.

### 4.5 Test location: in-binary unit tests *(chosen)* vs. move `tool_definitions()` into `crypt_env_lib::mcp`

`src-tauri/src/mcp/mod.rs` exists and is empty (already declared at `src-tauri/src/lib.rs:9`), so
moving `tool_definitions()` there would cost nothing structurally and would make the tool list a
first-class library artifact testable from `src-tauri/tests/`, alongside `vault_integration.rs`.

Rejected for this pass: it moves ~600 lines of `json!` literal across a module boundary inside a
naming-bug fix, inflating the diff by an order of magnitude and burying the four lines that
actually matter. The in-binary `#[cfg(test)] mod tests` gets the identical assertions with a
~60-line diff and no restructuring. Recommended as a candidate for issue #11's harness work,
where a schema-move is on-topic and reviewable on its own merits.

### 4.6 Also rename the CLI's `--id` flag *(out of scope)*

`project inject --id` is the convention the MCP schema was derived from (see the comment at
`api/mod.rs:214-220`). It is **not** renamed here: a CLI flag is read by a human from `--help`
next to `--project`/`--environment`, not inferred by a model from twelve sibling schemas, so it
does not carry the failure mode this issue describes. Renaming it would be a user-visible breaking
change to an unrelated surface.

### 4.7 Echo the resolved environment name in the response *(noted, not done)*

A numeric id gives the caller no confirmation of *which* environment was touched. Echoing the
resolved project/environment name in the success payload would make a mis-resolution visible
immediately. It requires an API response-shape change on both endpoints, so it is a separate
issue — recorded here because it is the natural defence-in-depth companion to §2's silent
mis-resolution, and because it would make §4.1's residual risk near-zero.

---

## 5. Rollback

Single-file, additive-then-substitutive; revert the three commits in reverse order. Reverting
step 3 alone restores the old schemas while leaving the resolver accepting both keys — a safe
intermediate state, since the resolver is a strict superset of the old behaviour. No database
migration, no persisted state, no config, no frontend involvement. The only externally observable
artifact is the `tools/list` payload, which clients re-fetch on connect.
