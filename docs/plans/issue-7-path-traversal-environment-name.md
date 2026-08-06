# Issue #7 — Path traversal via environment name escapes `output_dir` on `/fill`, `/example` and inject

Status: plan (no code written)
Labels: `bug`, `security` — the only `security`-labelled issue of the current set. Treat it as the highest-priority item of the batch and do not bundle it with unrelated refactors.
Scope: new `src-tauri/src/fsguard/mod.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/project/mod.rs`, `src-tauri/src/api/mod.rs`, `src/components/ProjectManager.tsx` (mirror only), new `src-tauri/tests/path_containment.rs`
Related: #8 (silent truncation on explicit `output_path` — same three sinks, shares the helper module), #11 (test harness — consume it), #12 (case-insensitive uniqueness — owns all row renaming), #3 (WSL/UNC paths are supported on purpose)

---

## 1. Objective

Definition of done. Each item is independently checkable; none of them is "code was written".

1. **No write reachable through an environment `name` can land outside the resolved `output_dir`.** For `/fill`, `/environments/{}/example` and `project::inject_environment`'s default-filename branch, the byte-level write target is always a direct child of `canonicalize(output_dir)`. Verified by the table-driven containment test in §5 over **at least 17 malicious names** covering: `../` runs, `..\` runs, bare `..` and `.`, POSIX-absolute, Windows-absolute, UNC (`\\wsl.localhost\...`), verbatim (`\\?\C:\...`), drive-relative (`C:foo`), NUL byte, percent-encoded (`%2e%2e%2f`) and overlong-UTF-8-looking variants, Unicode look-alikes (U+2025 `‥`, fullwidth `．．`), Windows reserved device names (`CON`, `NUL`, `COM1`, `LPT1`), trailing dot / trailing space, NTFS ADS (`env:stream`), over-length (>64), and empty / whitespace-only.
2. **Both layers exist and each is sufficient on its own.** Disabling layer 1 (`validate_environment_name`) must leave the containment tests green; disabling layer 2 (`fsguard::resolve_within`) must leave the validation tests green. This is stated as an acceptance property because it is the whole point of belt-and-braces — a plan that only reroutes the payload through one gate has not fixed the bug.
3. **`/fill` writes no secret bytes anywhere on rejection.** After the whole malicious table is run against `/fill` with a vault item whose plaintext is a known 32-byte canary, a recursive scan of the tempdir root *and its parent* finds zero occurrences of that canary and zero files created outside the base. Rejection happens **before** decryption, not merely before the write.
4. **No directory-creation amplification.** After any rejected request, no directory named `.env...` (or any other attacker-named directory) exists under `output_dir`. `create_dir_all` is only ever called on the caller-supplied base, never on a path containing an interpolated name.
5. **Reported path == actual path.** `FillResponse.path` and `ExampleResponse.path` return the *resolved* target (post-canonicalization), not the pre-resolution joined string. Today they disagree — that discrepancy is what makes the repro in the issue look like a success to the caller.
6. **The choke point is `project::save_environment`, not the HTTP handler.** Creating an environment named `../../../tmp/pwned` fails identically through `POST /environments`, the Tauri command `environment_save` (GUI), a `.cryptenv-proj` template imported via `project_import`, and the CLI. Verified by calling `save_environment` directly in a test, with no HTTP involved.
7. **Error semantics are caller-facing, not server-facing.** Both layers return `422 UNPROCESSABLE_ENTITY` / `VALIDATION_ERROR` (layer 1) and `422` / `PATH_NOT_CONTAINED` (layer 2). Neither response nor log line echoes the offending name, the resolved absolute path, or any file content. Today layer-2-class failures surface as `500 INTERNAL_ERROR`, which mislabels a caller error as a server fault.
8. **One helper, four call sites' worth of coverage.** All three sinks call the same `fsguard::resolve_within`. A grep for `format!("{dir}/.env` outside `fsguard` returns nothing.

---

## 2. What is being mitigated

**Checkable statement of the removed exposure:**

> After this change, a stored environment `name` can no longer influence *which directory* a write lands in. The only thing a name can decide is the filename inside the directory the caller explicitly asked for, and that filename is a single path component with no separators, no parent references, and no platform-special meaning.

### Threat model, stated precisely

The name is **stored, reused, and cross-privilege**. `output_dir` and `output_path` are **per-request caller intent**. That asymmetry is the reason one is constrained here and the other is not:

| Input | Origin | Trust | Constrained by this issue? |
|---|---|---|---|
| `environments.name` | Persisted row. Written by GUI, HTTP, CLI, or an imported `.cryptenv-proj` template. Read back by *later, unrelated* requests. | Untrusted stored data | **Yes — both layers** |
| `output_dir` | Supplied by the caller of *this* request | Caller's own stated intent | No — it defines the base |
| `output_path` | Supplied by the caller of *this* request | Caller's own stated intent | No — issue #8 (no-clobber), not validation |
| `environments.paths[]` | Chosen by the user through `project_pick_env_path` (native file dialog), or set via `set_environment_paths` | User-chosen absolute paths, deliberately arbitrary (WSL/UNC targets are a supported feature — #3) | **No — explicitly out of scope, see §4/D6** |

The attack that this closes is therefore *not* "a caller writes where it asked to write". It is: **a name persisted once silently redirects somebody else's later write.** Concretely — an LLM agent holding the static MCP token (or anything that read that token out of the settings table / MCP config) creates an environment named `../../../tmp/pwned`; the next `/fill` that a human or another agent runs against that environment with a perfectly innocent `output_dir` writes **decrypted secret values** to `/tmp/pwned`. The response reports the innocent-looking path, so nothing signals it.

### Why the `..` currently resolves at all

`create_dir_all` is the amplifier, not an incidental detail. With `output_dir = /home/u/proj` and `name = "../../../tmp/pwned"`, the joined path is `/home/u/proj/.env.../../../tmp/pwned`. `create_dir_all` on its parent **creates the literal directory `.env...`**, which gives the following `..` segments something real to resolve against. Remove the interpolated segment from anything passed to `create_dir_all` (objective 4) and the primitive dies even without a prefix check.

`project::inject_environment` has no `create_dir_all` of its own and therefore fails in isolation — but a single prior `/fill` or `/example` with the same name creates exactly the directory it needs, after which inject writes through happily. The three sinks are one vulnerability, not three.

### Severity split across the sinks

| Sink | Location | Written content | Primitive |
|---|---|---|---|
| `/fill` | `api/mod.rs:1422-1448` | **Decrypted secret values** | Arbitrary-location **secret exfiltration to disk** |
| `/environments/{}/example` | `api/mod.rs:2195-2226` | Placeholder `KEY=` lines | Arbitrary file overwrite |
| `inject_environment` | `project/mod.rs:286-289`, write at `:357` | Env values (secrets, merged into existing content) | Arbitrary file overwrite + secret write |

`/fill` is the one that makes this a security issue rather than a robustness issue.

### Explicitly NOT mitigated (so nobody assumes otherwise)

- A caller that names a hostile `output_dir` or `output_path` directly. That is caller intent by design; #8 decides whether it needs confirmation / no-clobber.
- Writes to `environments.paths[]` (§4/D6).
- The static, non-rotating, non-scoped MCP token itself — every agent configured against the vault shares it. That is a separate authorization problem; this plan reduces its blast radius, it does not fix it.
- Symlink races inside `output_dir` — residual TOCTOU window, quantified honestly in §4/D3.

---

## 3. Implementation steps

Ordered. Each step compiles and is testable on its own. Steps 1–2 are the security fix; 3–5 are the wiring; 6–7 are hardening and UX.

### Step 1 — New leaf module `src-tauri/src/fsguard/mod.rs`

A dependency-free leaf: it imports only `std`. It must not know about `db`, `api`, `project`, `vault`, or Tauri. Register in `lib.rs` alongside the existing `pub mod` list (L4–13):

```rust
pub mod fsguard;
```

Public surface — exactly two items:

```rust
#[derive(Debug, PartialEq, Eq)]
pub enum ContainmentError {
    EmptyName,
    NotASingleComponent,   // separators, `.`, `..`, prefix/root, drive-relative
    NulByte,
    ReservedDeviceName,
    TrailingDotOrSpace,
    BaseUnusable(String),  // io error text from create/canonicalize on the BASE only
    Escapes,               // post-join prefix check failed (symlink / belt-and-braces)
}

/// Resolves `file_name` as a direct child of `base_dir`, guaranteeing the
/// result cannot be anywhere else. `file_name` is treated as a literal
/// filename: it is never decoded, unescaped, or normalized.
pub fn resolve_within(base_dir: &str, file_name: &str) -> Result<PathBuf, ContainmentError>;
```

`Display` impl on `ContainmentError` produces a message that **describes the rule, never echoes the input** (see §4/D5).

Algorithm, in this exact order:

1. Reject empty / whitespace-only `file_name`; reject any `'\0'`; reject any `char::is_control`.
2. **Lexical component check.** `Path::new(file_name).components()` must yield exactly one item and that item must be `Component::Normal`. This single check rejects `..` (`ParentDir`), `.` (`CurDir`), `/x` and `\x` (`RootDir` + extra), `C:\x` and `\\?\...` and `\\server\share\...` (`Prefix`), and every multi-segment form. It is a *lexical* check — it never touches the filesystem and never decodes anything, so `%2e%2e%2f` stays a literal filename and `‥` stays a literal character.
   - Belt: additionally reject a literal `'/'` or `'\\'` anywhere in the string. On Unix, `Path` does not treat `\` as a separator, so `..\..\x` parses as one `Normal` component — it would be *contained* on Linux but is a traversal on Windows. Rejecting both separators on both platforms keeps behaviour identical across targets and keeps a vault file portable between them.
3. **Windows-hostile-name check, applied on every platform** (a vault created on Linux can be opened on Windows, and this project's whole workflow crosses that boundary):
   - Reject the ASCII-case-insensitive reserved device names `CON PRN AUX NUL COM1..COM9 LPT1..LPT9`, both bare and with any extension.
   - Reject a trailing `.` or trailing space (Win32 silently strips them → two distinct names collide on one file, and `.env.prod.` writes to `.env.prod`).
   - Reject `:` (NTFS alternate data streams: `file.txt:hidden`) and the rest of the Win32-illegal set `< > " | ? *`.
4. **Base resolution.** `fs::create_dir_all(base_dir)` — this is the *only* `create_dir_all` in the whole flow, and its argument contains no interpolated name. Then `fs::canonicalize(base_dir)` → `real_base`. Any error here maps to `BaseUnusable`.
5. **Join and verify.** `let target = real_base.join(file_name);` then assert `target.starts_with(&real_base)`. `Path::starts_with` compares whole components, so it cannot be fooled by string-prefix tricks (`/base` vs `/base-evil`).
6. **Symlink post-check.** If `target` already exists, `fs::canonicalize(&target)` and re-assert `starts_with(&real_base)`; a pre-planted symlink pointing outside is rejected with `Escapes`. If it does not exist, step 4 already proved the parent is real.

Add `#[cfg(test)] mod tests` in-file for the pure-lexical cases (no I/O), keeping the integration test file for the filesystem-level invariants.

### Step 2 — Name validation at the single choke point

In `src-tauri/src/project/mod.rs`, add:

```rust
pub fn validate_environment_name(name: &str) -> Result<(), String>;
pub fn validate_project_name(name: &str) -> Result<(), String>;
```

Implemented as a hand-written character scan (not `regex` — see §4/D4), sharing a private `reject_filesystem_hostile(name)` core that applies exactly the step-1.3 rules plus separators, control chars and NUL.

- **Environment names — strict allowlist**, equivalent to `^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$`, i.e. first char alphanumeric ASCII; remaining chars from `[A-Za-z0-9._-]`; total length 1..=64; plus an explicit rejection of a trailing `.` or `-`. The leading-alphanumeric rule alone already makes `.`, `..`, `../x` and every dotfile-shaped name impossible. Rationale for the tight rule: environment names are machine identifiers (`production`, `local`, `staging-2`), they land in a filename, and they are the field with a proven exploit.
- **Project names — deny-list, deliberately laxer.** Reject separators, `..` as a whole name, control chars, NUL, the Win32-illegal set, reserved device names, leading/trailing dot or whitespace, and length > 128. **Allow** spaces and non-ASCII letters. Rationale and the compat argument for the asymmetry: §4/D2.

Call sites — `validate_environment_name` from `project::save_environment` (`project/mod.rs:172`), as the **first statement**, before `db.upsert_environment`; `validate_project_name` from `project::save_project` (`:147`), likewise. That single placement covers `POST /environments`, `POST /projects`, the Tauri commands `environment_save` (`:392`) and `project_save` (`:375`), the GUI, an imported `.cryptenv-proj` template (which flows back through `environment_save`), and the CLI. Do **not** put the check in `handle_save_environment` — that reproduces the exact defect being fixed, where the HTTP handler was the only gate.

`save_project`'s auto-created `"default"` environment (`:153`) already satisfies the rule; leave it as a literal, do not route it through validation.

### Step 3 — Sink 1: `api::handle_fill`

Two changes, in this order:

1. **Hoist the target resolution above the fill/decrypt block.** Immediately after `env` is resolved and the body is parsed — and before the loop that builds `filled` (`api/mod.rs:~1340-1417`) — compute the write target and validate it. This is what makes objective 3 true: on rejection no decryption has happened, so no plaintext ever exists in the process for that request, let alone on disk.
2. Replace the `write_target` block (`:1422-1448`):
   - `output_path` present → unchanged behaviour, passed through verbatim (issue #8's territory).
   - `output_dir` present → `fsguard::resolve_within(dir, &format!(".env.{}", env.name))?`. Note the *whole* filename including the `.env.` prefix goes through the helper, so the prefix cannot be used to smuggle anything either.
   - Delete the `create_dir_all(parent)` call at `:1439` — step 1.4 already created the base and nothing else may be created.
   - `TempEnvFile::create` receives the resolved `PathBuf`. Its RAII zero-fill-on-drop semantics are unchanged and still matter.
   - `FillResponse.path` reports the resolved path (objective 5).
   - Map `ContainmentError` → `422` / `PATH_NOT_CONTAINED`, except `BaseUnusable` → `500` / `INTERNAL_ERROR` (that one genuinely is a server/environment fault).

### Step 4 — Sink 2: `api::handle_environment_example`

Same edit at `:2195-2226` with `format!(".env.example.{}", env.name)`. Drop the `create_dir_all` at `:2207`. `ExampleResponse.path` reports the resolved path. Content here is placeholder keys only, so no decrypt-ordering concern — but keep the same code shape as step 3 so the two sinks stay diff-comparable.

### Step 5 — Sink 3: `project::inject_environment`

At `project/mod.rs:286-289`, the default-filename branch becomes `fsguard::resolve_within(dir, &format!(".env.{}", env.name))`, mapped into the function's existing `Result<_, String>` error type. Paths already configured on the environment and an explicit `output_path` continue to be pushed unchanged (§4/D6). No `create_dir_all` is added here — a resolved base always exists after step 1.4, and for configured paths the current "fail if the directory does not exist" behaviour is correct and should stay.

### Step 6 — Frontend mirror (UX only, never authoritative)

`src/components/ProjectManager.tsx:781` currently sends `envName.trim()` with no validation. Add an inline check mirroring the environment rule, disabling Save and showing the rule text on violation. Same for the project name field at `:646`. This is purely to avoid a pointless round-trip; the server stays the enforcement point and the frontend rule is documented in a comment as a mirror that may drift.

### Step 7 — Docs

Note the naming rule in the environment-creation section of the project docs, plus a one-line note that legacy names outside the rule keep working until edited. No new `.md` file beyond this plan.

### Not required

`src-tauri/src/bin/crypt-env-mcp.rs` needs **no change**. `tool_inject_environment` (`:1886`) and `tool_generate_example_env` (`:1929`) forward `output_path` / `output_dir` verbatim to the HTTP API, so they inherit enforcement automatically. Leaving MCP untouched is deliberate: a second enforcement point there would be a second thing to keep in sync.

---

## 4. Trade-offs / alternatives considered

### D1 — Two layers, or just one?

**Decision: both.**

Layer 2 alone would technically close the bug and needs no backward-compatibility story. Layer 1 alone would not, because legacy rows already in the database bypass it entirely. But layer 2 alone leaves a hostile name *storable*, and stored hostile data has a way of finding a fourth sink — the issue's own text predicts `projects.name` as the next one. Layer 1 alone is the classic mistake: it dies the moment somebody adds a sink that reads a row written before the validator shipped.

Cost of both: two places to look when a name is rejected, and two error codes. Accepted — that is a small readability tax for a control that survives a future maintainer deleting either half.

### D2 — Backward compatibility of the name charset

**Decision: validate on write only. No migration, no rename, no bulk rejection at read time.**

Existing installs may hold environment names with spaces, accents, or (in the worst case) an already-planted traversal payload. Three options:

| Option | Verdict |
|---|---|
| **Validate on write only** — legacy rows keep working, are contained by layer 2, and get corrected the first time somebody edits them | **Chosen.** No migration risk, no unopenable vault, no data loss. The security property is delivered entirely by layer 2 for legacy rows, which is exactly what layer 2 is for. |
| Migration that renames offending rows at unlock | Rejected *for this issue*. Renaming breaks name-based scope resolution silently — every CLI `--env`, MCP call, and saved script referencing the old name starts failing, and there is no way to tell the user which of their scripts to fix. Also duplicates machinery: **#12 already specifies a rename+audit mechanism** (`env_name_dedup_v1` settings key, deterministic suffixes, persisted report). If renaming is ever wanted, it belongs inside #12's pass, not as a second competing migration. |
| Reject legacy names at read time | Rejected. Turns a stored-data problem into "the user's vault stopped working" with no remediation path, and would break `/fill` for environments that are perfectly safe once contained. |

**Sequencing with #12:** ship #7 first. #7 touches no schema and runs no migration, so it is the cheaper and more urgent of the two, and it does not constrain #12's design. If #12 later extends its rename pass to also fix filesystem-hostile names, it inherits `validate_environment_name` as the predicate — one rule, one implementation.

**The asymmetry between the two rules** (strict allowlist for environments, deny-list for projects) is a deliberate compat trade: environment names are machine identifiers where a tight rule costs almost nothing, project names are human labels where `"My App"` is entirely normal and an ASCII-only allowlist would reject edits to real existing projects for no present security gain. Cost of the asymmetry: a reader must check which rule applies. Mitigated by both functions living side by side and sharing the hostile-character core. **`projects.name` is in scope** — the issue is right that it is the obvious next sink, and adding the gate now costs one function while adding it after a sink exists costs a migration.

### D3 — The containment algorithm

**Decision: lexical single-component check on the filename + `canonicalize` the base + component-wise prefix assert + existence-time symlink recheck.**

The two pure approaches each fail on their own:

- **`canonicalize` everything.** Requires the path to exist, which the target does not yet — you cannot canonicalize a file you are about to create. Canonicalizing the *parent* only works because the parent is the base, which is exactly the hybrid below.
- **`Path::components()` scan rejecting `Component::ParentDir` only.** Purely lexical, so it does not follow symlinks: a symlinked subdirectory inside `output_dir` still escapes. It also misses `Component::Prefix` on Windows.

The hybrid gets: no existence requirement for the target, symlink resolution for the base, component-wise (not string) prefix comparison, and no decoding of anything.

**Residual risk, stated honestly:** between the symlink recheck and `fs::write` there is a TOCTOU window in which an attacker with local write access inside `output_dir` can replace the target with a symlink pointing elsewhere. `std` offers no portable `O_NOFOLLOW` / `openat` to close it. Accepted because an attacker who can create files inside the user's chosen output directory already has local filesystem write access as that user — at which point they do not need this bug. If that assumption ever stops holding, revisit with `std::os::unix::fs::OpenOptionsExt` + `O_NOFOLLOW` on Unix and `FILE_FLAG_OPEN_REPARSE_POINT` on Windows.

**Windows specifics** the helper must get right, all covered by the step-1 order:
- `Component::Prefix` catches `C:\...`, `\\?\C:\...`, `\\server\share\...`, and `\\wsl.localhost\Ubuntu\...`.
- Drive-relative `C:foo` parses as `Prefix` + `Normal` → more than one component → rejected.
- `\\wsl.localhost\...` remains valid **as a base** — this project deliberately supports it (#3). `canonicalize` on Windows returns a verbatim `\\?\UNC\...` form; both sides of the prefix check are canonicalized so the comparison holds, but the path *displayed back to the user* should have the `\\?\` / `\\?\UNC\` prefix stripped for readability while the verbatim form is used for the actual write. Do not skip this: returning `\\?\UNC\wsl.localhost\...` in an API response is a support ticket.
- Reserved device names, trailing dots/spaces and ADS are name-level, not path-level, so `components()` cannot catch them — hence the explicit step-1.3 check.

### D4 — Sanitize instead of reject?

**Decision: reject. Never rewrite the caller's name.**

Sanitizing (`..` → `__`) is tempting because nothing ever fails. It is wrong here for a reason specific to this codebase: **the name is a lookup key**. `project::resolve_environment` (`project/mod.rs:214-237`) matches environments by case-insensitive name. Silently storing something other than what the caller sent means the caller's next `?environment=<name>` lookup misses, or — worse — two different requested names sanitize to the same stored name and a lookup resolves to the wrong environment. That is precisely the silent-wrong-environment failure mode #12 exists to eliminate; introducing a new source of it while fixing a security bug would be self-defeating.

There is in-repo precedent for sanitizing (`project_export`'s `safe_name`, `project/mod.rs:492`) — but that produces a *suggested filename for a save dialog*, a throwaway value nobody looks up later. Different problem, correctly solved differently.

Rejection also gives the user a real message. Sanitizing gives them an environment named `.....tmp.pwned` and no explanation.

**Regex vs hand-written scan:** `regex = "1"` is already a dependency (`Cargo.toml:69`), so a regex is available. Chosen against anyway: a `Regex` must either be recompiled per call or parked in a `OnceLock`, and — decisively — a regex match yields one boolean, so every violation produces the same message. The hand-written scan is ~30 lines, allocation-free, and says *which* rule was broken, which is the difference between a user fixing their name and filing a bug.

### D5 — Error semantics

**Decision: `422` at both layers, distinct codes, rule text only — never the offending value.**

- Layer 1 (save): `422 UNPROCESSABLE_ENTITY`, code `VALIDATION_ERROR`, message naming the field and the rule — e.g. `name: must be 1-64 chars, start with a letter or digit, and contain only letters, digits, '.', '_' or '-'`. Matches the existing shape at `api/mod.rs:2036`.
- Layer 2 (sink): `422`, code `PATH_NOT_CONTAINED`, message naming the *field* (`environment name` / `output_dir`) and nothing else. Not `500`: the current code maps every filesystem failure to `500 INTERNAL_ERROR` (`:1440`, `:2208`), which tells an operator to look at the server when the problem is the request. `BaseUnusable` is the one case that stays `500`.
- **No echo of the name or the resolved path** in either the response or any log line. Two reasons: the name is attacker-controlled text and would be written verbatim into logs (control characters, ANSI sequences, newline-forged log lines), and the resolved path can disclose directory structure. Log the environment `id` instead — it is stable, non-attacker-chosen, and enough to identify the row.
- **Never any file content** in an error, per CLAUDE.md. `/fill`'s existing discipline (write to disk, return stats only, never the filled content) is preserved unchanged.
- A layer-2 rejection firing at all means a legacy hostile name reached a sink. Emit one `warn`-level line with the environment id and the error variant — that is the signal a maintainer needs to know the second layer is doing real work.

Tauri commands return `Err(String)` with the same message text, since they have no status code.

### D6 — Are `environments.paths[]` in scope?

**Decision: explicitly out of scope. Stated here rather than left ambiguous.**

`set_environment_paths` stores arbitrary caller-supplied absolute paths and `inject_environment` writes to every one of them with no containment at all (`project/mod.rs:327-357`). That is a broader write surface than the bug being fixed. It stays unconstrained because these paths are the feature: the user picks them through a native file dialog (`project_pick_env_path`), they are supposed to point at real project checkouts anywhere on the machine — including `\\wsl.localhost\...` targets, which #3 exists to support — and there is no "base" to contain them against.

Their real risk is not traversal but **clobbering** — merging into and rewriting a file the user did not expect. That is exactly issue #8's question (confirmation / marker / no-clobber), and #8 should state whether configured paths are in *its* scope. This plan takes no position beyond: not here.

If a future issue does want to constrain them, the lever is at write time in `set_environment_paths` (reject non-absolute, reject paths containing unresolved `..`), not in `inject_environment`.

### D7 — Where the helper lives, and the boundary with #8

**Decision: new top-level leaf module `src-tauri/src/fsguard/`. One module, two responsibilities, shared with #8.**

Module-rule constraints from CLAUDE.md: `db` must not know about `api`; `vault` orchestrates. The helper is needed by both `api` and `project`, so it cannot live in either without creating a sideways dependency between two peers. It has zero dependencies of its own (`std` only), which makes a leaf module the honest shape. Alternatives rejected: putting it in `vault` (`vault` is the orchestrator, not a utility bag, and the CLI binaries would then pull in orchestration to resolve a path); putting it in `project` (`api` would depend on `project` for a pure path function — it already depends on `project` for business logic, so this would work, but it hides a general-purpose utility inside a domain module and the next consumer, `share` or `cli`, would look for it in the wrong place).

**Boundary with #8**, which proposes its own shared filesystem-write helper over the same three sinks. Two helpers competing over the same call sites is the worst outcome. The split:

| Concern | Owner | Signature shape |
|---|---|---|
| *Where* am I allowed to write — path algebra, containment, name legality | **#7**, `fsguard::resolve_within(base, name) -> Result<PathBuf, ContainmentError>` | Pure; the only I/O is creating and canonicalizing the base |
| *May* I write to this resolved path — existence, marker detection, backup, clobber confirmation | **#8**, `fsguard::guarded_write(path, content, opts) -> Result<_, _>` | Takes the `PathBuf` #7 produced |

They compose linearly: `let target = fsguard::resolve_within(dir, &name)?; fsguard::guarded_write(target, &content, opts)?;`. Whichever issue lands first creates `src-tauri/src/fsguard/mod.rs` and registers it in `lib.rs`; the second **adds a function to that module** and does not create a second one. Both plans must reference this table.

---

## 5. Tests

New file `src-tauri/tests/path_containment.rs`. Dev-deps already present (`tempfile = "3"`, `tokio` with `rt`/`macros`, `Cargo.toml:97-99`); **no new dependency is needed** — recursive directory walking is a ten-line `read_dir` helper, not a reason to add `walkdir`. HTTP-level cases consume #11's harness rather than standing up a second one; if #11 has not landed, the non-HTTP cases below still cover objectives 1, 2, 4 and 6, and the HTTP cases wait.

### T1 — The malicious table (drives objectives 1 and 2)

One `const MALICIOUS: &[&str]` shared by every case below, so a new entry is covered everywhere at once. Minimum 17 entries:

| # | Input | Class |
|---|---|---|
| 1 | `../../../tmp/pwned` | POSIX traversal (the issue's repro) |
| 2 | `..\..\..\Windows\Temp\pwned` | Windows traversal |
| 3 | `..` | bare parent |
| 4 | `.` | bare current |
| 5 | `/etc/cron.d/pwned` | POSIX absolute |
| 6 | `C:\Windows\Temp\pwned` | Windows absolute |
| 7 | `C:pwned` | Windows drive-relative |
| 8 | `\\wsl.localhost\Ubuntu\home\u\pwned` | UNC |
| 9 | `\\?\C:\pwned` | verbatim prefix |
| 10 | `prod\0evil` | NUL byte |
| 11 | `%2e%2e%2fpwned` | percent-encoded |
| 12 | `..%c0%af..%c0%afpwned` | overlong-UTF-8-looking |
| 13 | `‥/pwned` (U+2025) | Unicode look-alike |
| 14 | `．．／pwned` (fullwidth) | Unicode look-alike |
| 15 | `CON`, `NUL`, `COM1`, `LPT1` | reserved device names |
| 16 | `prod.` / `prod ` | trailing dot / space |
| 17 | `env:stream` | NTFS ADS |
| 18 | 65×`a` | over-length |
| 19 | `""`, `"   "` | empty / whitespace |

Note on 11–14: these must be treated as **literal filenames**, never decoded. The asserted outcome is "rejected by layer 1", and — if layer 1 is bypassed — "layer 2 either rejects or produces a path still inside the base". Both are acceptable; silently *decoding* them is not.

Assertions per entry:
- `project::validate_environment_name(input).is_err()`
- `fsguard::resolve_within(base, &format!(".env.{input}")).is_err()`, or if `Ok(p)`, then `p.starts_with(canonicalize(base))` and `p.parent() == Some(canonicalize(base))`.

### T2 — Filesystem invariant (objectives 1 and 4)

Layout: `tmp/` containing `tmp/base/` and `tmp/sibling/canary.txt` with known content. Run every entry through the three sinks. Then assert:
- `tmp/sibling/canary.txt` byte-identical to before;
- the recursive entry list of `tmp/` is identical to the pre-run snapshot except for files directly inside `tmp/base/`;
- no directory matching `.env*` exists under `tmp/base/` (kills the `create_dir_all` amplification specifically);
- the recursive entry list of `tmp/`'s parent is unchanged.

### T3 — `/fill` secret non-exfiltration (objective 3)

Seed a vault item whose plaintext is a 32-byte canary. Run the malicious table through `/fill` with `output_dir = tmp/base`. Assert: every response is `422`; a recursive byte-scan of `tmp/` and its parent finds **zero** occurrences of the canary; and the item was never decrypted on the rejected path (assert via the ordering — the check is before the fill loop — plus the byte-scan as the observable proof).

### T4 — Choke-point coverage (objective 6)

Call `project::save_environment` directly with `name = "../../../tmp/pwned"` against a `tempdir` vault and assert `Err`. No HTTP. This is the test that would have caught the original bug, where the HTTP handler was the only gate and the Tauri path had none.

### T5 — Layer independence (objective 2)

Two `#[cfg(test)]`-gated runs: with layer 1's call commented out, T2 and T3 still pass; with layer 2's call commented out, T1's validation assertions still pass. Implement as two feature-less test fns that call the layers directly rather than by mutating production code.

### T6 — Positive path (no over-rejection)

`production`, `local`, `staging-2`, `v1.2`, `a` (single char), 64×`a` all validate. `resolve_within(base, ".env.production")` returns exactly `canonicalize(base)/.env.production`, the file is created there, and `FillResponse.path` equals that resolved path (objective 5).

### T7 — Legacy-name containment

Insert a row with a hostile name **directly through `db::upsert_environment`**, bypassing validation — simulating a pre-fix vault. Assert `/fill`, `/example` and `inject_environment` all reject it at layer 2, and that the environment remains listable and deletable (no vault bricking, per D2).

### T8 — Error-shape assertions (objective 7)

For a rejected request: status is `422`, code is `VALIDATION_ERROR` or `PATH_NOT_CONTAINED`, and the response body contains **none of**: the offending name substring, an absolute path, `sqlite`, or any file content.

---

## 6. Rollback

Cheap and complete: the change is one additive leaf module plus pure-function guards at four call sites. No schema change, no migration, no data rewrite, nothing persisted. `git revert` of the commit restores prior behaviour exactly, and no vault opened under the new code is unopenable under the old.

Do **not** put either layer behind a runtime feature flag or a setting. A security control that can be switched off from the settings table is reachable by the same token that reaches the sinks, which makes it not a control.

The only user-visible regression risk is layer 1 rejecting an edit to a legacy name. Early warning sign: users reporting "I can't save my environment any more". Mitigation is already in the design — the error names the rule, layer 2 keeps the legacy row safe in the meantime, and renaming is the user's decision, not a migration's.

---

## 7. Open questions for the maintainer

1. **Environment name length cap of 64** — arbitrary but generous (`.env.` + 64 is well under `NAME_MAX`). Confirm no real environment name exceeds it before shipping.
2. **Project-name deny-list vs allowlist (D2).** The laxer project rule is a compat call. If existing project names are all ASCII-identifier-shaped in practice, the simpler answer is one rule for both — decide from the actual data.
3. **`\\?\` prefix stripping for display (D3).** Confirm the intended UX for a `\\wsl.localhost\...` base: show the stripped form and write the verbatim form, or show verbatim.
4. **#8 boundary (D7).** Both plans must agree the `fsguard` split before either lands, or the second one will fork the module.
