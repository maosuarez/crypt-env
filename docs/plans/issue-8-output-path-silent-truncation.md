# Issue #8 — Silent truncation: explicit `output_path` is written with no existence check, backup or confirmation

Status: plan only. No code written.
Scope: the three filesystem sinks that accept a caller-supplied write target
(`POST /fill`, `POST /environments/:id/example`, `POST /environments/:id/inject`)
plus the `TempEnvFile` RAII guard that sits under one of them.

---

## 0. Corrections to the issue text

Two claims in the issue need adjusting before planning against them.

1. **`commands/inject.rs` does not write files.** The issue's consumer list implies
   the CLI `inject` subcommand is a fourth caller. It is not:
   `src-tauri/src/bin/crypt-env/commands/inject.rs` resolves one secret and prints a
   shell assignment to stdout (`format_assignment`), never touching the filesystem.
   Only `commands/fill.rs` sends an `output_path` (line 41). The CLI change surface
   is therefore one file, not two.

2. **The Tauri path never supplies a caller path.** `project::environment_inject`
   (`src-tauri/src/project/mod.rs:408-413`) calls `inject_environment(.., id, None, None)`.
   The GUI can only ever write to `environment.paths[]` — paths a human previously
   configured. That distinction is load-bearing for the policy below (§4.4): the
   dangerous input is a path the *caller* invented, not a path the *owner* saved.

---

## 1. Objective (definition of done)

### 1.1 The shared helper

A new leaf module **`src-tauri/src/envfile/mod.rs`** (declared `mod envfile;` in
`src-tauri/src/lib.rs`) is the single place where crypt-env decides whether it is
allowed to write to a path. It depends on `std` only — not on `db`, `vault`,
`crypto`, `project` or `api`. Public surface:

```rust
pub enum Target { Absent, Managed(String), Foreign }   // Foreign carries NO content
pub enum FileMode { Private0600, Inherit }
pub struct WriteOptions { pub overwrite: bool, pub mode: FileMode }
pub struct Committed { pub pre_existed: bool, pub backup: Option<PathBuf> }
pub enum EnvFileError { TargetExists(PathBuf), BackupExists(PathBuf), Io(PathBuf, std::io::ErrorKind) }

pub fn marker_line(project: &str, environment: &str) -> String;
pub fn inspect(path: &Path) -> Result<Target, EnvFileError>;
pub fn commit(path: &Path, content: &str, marker: &str, opts: &WriteOptions)
    -> Result<Committed, EnvFileError>;
```

`EnvFileError` is a custom error type implementing `std::error::Error` (CLAUDE.md
Rust rules). No `unwrap()` anywhere in the module. `Io` stores an
`std::io::ErrorKind`, not the `io::Error`, so the rendered message can never
carry an OS string beyond the path the caller already supplied.

### 1.2 The exact policy

For every resolved write target, in this order:

1. `inspect(path)` classifies it: `Absent` / `Managed` (first non-empty line,
   trimmed, starts with the exact ASCII prefix `# crypt-env:`) / `Foreign`.
2. Policy, keyed on where the path came from:
   - **Caller-supplied** (`output_path`, or a name derived from `output_dir`):
     `Foreign` ⇒ **refuse**. `Absent` and `Managed` ⇒ proceed.
   - **Owner-configured** (`environment.paths[]`): all three ⇒ proceed, but a
     `Foreign` target is reported back to the caller in `unmanaged_paths`.
3. Refusal is evaluated for **all** targets *before* any secret is decrypted and
   before any byte is written. A refused request performs zero writes.
4. `overwrite: true` converts a refusal into: copy the victim to `<path>.bak`
   (0o600 on Unix, refuse if `<path>.bak` already exists), then write.
5. `commit` guarantees the marker is line 1 of every file crypt-env writes, and
   is idempotent (never duplicated on a re-write).
6. A `.bak` is created only when a `Foreign` target is about to be modified.
   Never for `Absent`, never for `Managed` — crypt-env never writes its own
   secret-bearing content into a second file.

### 1.3 Status codes and error body

- Refusal ⇒ **`409 Conflict`**, reusing the existing `ErrorBody { error, code }`
  with `code: "TARGET_EXISTS"`.
- `<path>.bak` already present ⇒ **`409 Conflict`**, `code: "BACKUP_EXISTS"`.
- The existing `422 VALIDATION_ERROR` for `"environment has no paths configured"`
  is untouched.
- The `error` string names the offending path(s) and the remedy, and contains
  **nothing derived from the victim file's contents** — no excerpt, no length,
  no hash, no mtime. Enforced structurally by `Target::Foreign` being a unit
  variant: the content is never in scope at the point the error is built.

Exact message, one path:
```
refusing to overwrite existing file not managed by crypt-env: /tmp/victim.yaml
 — pass overwrite: true to replace it (a .bak copy of the current contents is kept first)
```
Multiple paths (inject): same sentence with a comma-separated list.

### 1.4 Verifiable acceptance criteria

Done when all of the following hold:

- **AC1** — `test_gate_rejects_foreign_target_all_endpoints`: a table-driven test
  writes a victim file, then calls each of `/fill`, `/environments/:id/example`,
  `/environments/:id/inject` with `output_path` pointing at it, and asserts for
  **all three**: HTTP `409`, `code == "TARGET_EXISTS"`, and the victim file's
  bytes are **identical** to before the call (`assert_eq!` on the raw `Vec<u8>`).
- **AC2** — `test_gate_rejection_leaks_no_secret_to_disk`: an environment with
  three configured paths plus one `Foreign` caller-supplied `output_path`; after
  the 409, a recursive walk of the tempdir asserts the known secret value string
  appears in **no file**, and that each of the three configured paths is still
  absent/unchanged.
- **AC3** — `test_error_body_excludes_victim_contents`: the victim contains a
  unique sentinel; the 409 response body does not contain it.
- **AC4** — `test_overwrite_creates_bak_with_original_bytes`: with
  `overwrite: true`, the target holds the new content and `<path>.bak` holds the
  original bytes byte-for-byte; on Unix `<path>.bak` is mode `0o600`.
- **AC5** — `test_marker_survives_inject_merge_roundtrip`: two consecutive
  injects leave exactly one marker line, at line 1, with keys updated in place.
- **AC6** — `test_temp_env_file_drop_preserves_preexisting_file`: dropping a
  `TempEnvFile` without `persist()` over a pre-existing path leaves the path
  **existing** (not `remove_file`d) and containing no plaintext.
- **AC7** — `cargo clippy -- -D warnings` clean; no `unwrap()` in `envfile/`.

Full test list in §5.

---

## 2. What is being mitigated

A checkable statement of the removed risk:

> **Before:** any authenticated caller of the local API — in practice an LLM agent
> through `crypt_env_fill_env` / `crypt_env_generate_example_env` /
> `crypt_env_inject_environment`, or a human typing `.env` where `.env.example`
> was meant — can name an arbitrary absolute path and crypt-env will destroy
> whatever is there. `/fill` and `/example` truncate it; `/inject` is worse, it
> *appends plaintext secret values* to it and leaves the rest intact, so a
> git-tracked file can absorb production credentials with no visible failure. In
> every case the response is `200 OK` and no copy of the destroyed content exists.

> **After:** a write to a path that exists and does not carry the crypt-env marker
> is refused with `409 TARGET_EXISTS` before any decryption occurs, naming the
> path. The only way past it is an explicit `overwrite: true`, which first
> preserves the original bytes in `<path>.bak`. The destructive outcome remains
> reachable — it must, it is a legitimate operation — but it is now
> **opt-in, named, and recoverable**, and it is never the result of a single
> mistyped path.

Residual, explicitly *not* mitigated by this issue (see §4.4 and §6):
writing to a *wrong but owner-configured* path, writing to a path that does not
yet exist, and path traversal through `output_dir` (issue #7).

---

## 3. Implementation steps

Ordered. Each step compiles on its own.

### Step 1 — Create `src-tauri/src/envfile/mod.rs`

New leaf module. Declare `mod envfile;` in `src-tauri/src/lib.rs` alongside the
existing module declarations. Implement §1.1's surface:

- `MARKER_PREFIX: &str = "# crypt-env:"`.
- `marker_line(project, environment)` →
  `format!("# crypt-env: managed file (project: {project}, environment: {environment})")`.
  No timestamp, no version, nothing parsed on read (§4.3).
- `is_managed(content: &str) -> bool` — first non-empty line, `trim()`,
  `starts_with(MARKER_PREFIX)`. Nothing after the prefix is inspected.
- `inspect(path)`:
  - `std::fs::read_to_string(path)`:
    - `Err(e) if e.kind() == NotFound` ⇒ `Ok(Target::Absent)`
    - `Err(e)` ⇒ `Err(EnvFileError::Io(path, e.kind()))` — **this replaces
      `unwrap_or_default()` at `project/mod.rs:328`**, which today turns an
      EACCES/EISDIR into a silent "treat as empty, create new".
    - `Ok(s) if is_managed(&s)` ⇒ `Ok(Target::Managed(s))`
    - `Ok(_)` ⇒ `Ok(Target::Foreign)` — content deliberately dropped here.
  - Non-UTF-8 existing file ⇒ `InvalidData` ⇒ `Target::Foreign` (a binary file is
    by definition not one of ours; never truncate it silently).
- `commit(path, content, marker, opts)`:
  1. `let target = inspect(path)?;`
  2. If `target == Foreign && !opts.overwrite` ⇒ `Err(TargetExists(path))`.
  3. If `target == Foreign && opts.overwrite`: `bak = path.with_extension_appended(".bak")`;
     if `bak.try_exists()?` ⇒ `Err(BackupExists(bak))`; else `std::fs::copy(path, &bak)`
     and, `#[cfg(unix)]`, `set_permissions(&bak, 0o600)`.
  4. Prepend `marker` unless `content` already starts with `MARKER_PREFIX`
     (idempotence — inject's merge output already carries it, §4.3).
  5. Write via `OpenOptions`: `.write(true).create(true).truncate(true)`, and
     `#[cfg(unix)] .mode(0o600)` when `opts.mode == Private0600`. `mode()` only
     applies at creation; for an existing secret-bearing target additionally
     tighten to `0o600` if the current mode is group/other-readable (never loosen).
     `#[cfg(windows)]` skips all of this and inherits the directory ACL — the same
     reasoning already documented at `api/mod.rs:1241-1245`.
  6. Return `Committed { pre_existed, backup }`.
- Unit tests in the same file (§5.1).

### Step 2 — Fix `TempEnvFile` (`src-tauri/src/api/mod.rs:1227-1285`)

- Add field `pre_existed: bool`.
- Replace the body of `create` so it delegates to `envfile::commit` and records
  `pre_existed` from `Committed`. New signature:
  `fn create_guarded(path: PathBuf, content: &str, marker: &str, opts: &envfile::WriteOptions) -> Result<Self, envfile::EnvFileError>`.
  This removes the current write-then-`set_permissions` ordering, where a failed
  `set_permissions` returns `Err` *after* the target has already been truncated
  and leaves a secret file at umask permissions.
- `Drop`: keep the early return on `persisted`. When `!persisted`:
  - `pre_existed == false` ⇒ unchanged (zero-fill then `remove_file`).
  - `pre_existed == true` ⇒ zero-fill and truncate to length 0, **do not
    `remove_file`**. Rationale to put in the comment: by this point the original
    bytes are already gone (and preserved in `.bak` if the gate required one), so
    leaving our plaintext on disk is the worse of the two failures; but deleting a
    path we did not create would destroy the inode, its permissions and its
    existence, which callers may depend on.
  - Keep the existing note that a zero pass is defense-in-depth, not an erasure
    guarantee, on journaling/CoW filesystems.

### Step 3 — `/fill` (`src-tauri/src/api/mod.rs:1289-1460`)

- `FillBody` += `#[serde(default)] overwrite: bool`.
- `FillResponse` += `#[serde(skip_serializing_if = "Option::is_none")] backup: Option<String>`.
- After the existing `write_target` resolution (L1420-1427, untouched — that block
  is issue #7's, see §4.7) and after `create_dir_all`, replace
  `TempEnvFile::create` with `create_guarded(path, &filled, &marker, &WriteOptions { overwrite: body.overwrite, mode: Private0600 })`.
- Map `EnvFileError` → response: `TargetExists`/`BackupExists` ⇒
  `err_json(StatusCode::CONFLICT, …, "TARGET_EXISTS" | "BACKUP_EXISTS")`;
  `Io` ⇒ existing `500 INTERNAL_ERROR`.
- The marker needs project + environment names; `handle_fill` already resolves the
  environment for the fill itself — reuse it, do not re-query.

### Step 4 — `/environments/:id/example` (`src-tauri/src/api/mod.rs:2146-2230`)

- `ExampleBody` += `overwrite: bool`. `ExampleResponse` += `backup: Option<String>`.
- Replace the bare `std::fs::write` at L2218 with
  `envfile::commit(&path, &content, &marker, &WriteOptions { overwrite, mode: Inherit })`.
  `Inherit` (not `Private0600`) because this file exists to be committed and shared;
  see §4.6.
- Same 409 mapping. Keep the existing comment that the content holds no secret
  value, but correct it: it explains why no RAII wipe is needed, not why an
  existence check is unnecessary.

### Step 5 — `inject_environment` (`src-tauri/src/project/mod.rs:268-364`)

The largest change. Signature becomes:

```rust
pub async fn inject_environment(
    db: &VaultDb, vault_key: &[u8; 32], environment_id: i64,
    output_path: Option<String>, output_dir: Option<String>, overwrite: bool,
) -> Result<InjectResult, String>
```

Restructure into two phases, with the gate strictly before decryption:

1. **Resolve** — unchanged (L280-294), but track provenance:
   build `Vec<(String, PathOrigin)>` where `env.paths` ⇒ `Configured` and the
   `output_path` / `output_dir`-derived entry ⇒ `CallerSupplied`. `output_path`
   continues to be *appended* to the configured set, never to replace it.
2. **Gate (before any decryption)** — `envfile::inspect` every path. Collect every
   `CallerSupplied` + `Foreign` path into `refused`. If `!refused.is_empty() && !overwrite`
   ⇒ return `Err(...)` carrying the full list, mapped to 409 by the handler.
   Collect every `Configured` + `Foreign` path into `unmanaged` for the report.
   Cache each `Managed(content)` so the merge in phase 3 does not re-read (narrows,
   though does not close, the TOCTOU window).
3. **Decrypt** — the existing L296-322 block, now only reachable past the gate.
4. **Write** — per path, the existing merge (L332-354) against the cached content
   (`Absent`/`Foreign` ⇒ empty base), then `envfile::commit(...)` with
   `mode: Private0600`. The `#`-prefixed marker is skipped by the merge loop's
   existing `trimmed.starts_with('#')` guard at L334, so it round-trips untouched;
   `commit`'s idempotence check stops it being prepended twice.
   On a write error: stop, and return an error naming the failing path *and* the
   paths already written (see §4.5 — cross-file atomicity is not achievable).
- `InjectResult` += `unmanaged_paths: Vec<String>` and `backups: Vec<String>`.
- Do **not** add `create_dir_all` here (§4.7).

### Step 6 — Handlers and Tauri commands

- `api::handle_inject_environment` (L2100-2134): `InjectBody` += `overwrite: bool`;
  forward it; add the `409 TARGET_EXISTS` / `409 BACKUP_EXISTS` arms to the
  existing `match` before the catch-all 500.
- `project::environment_inject` (L408): signature becomes
  `environment_inject(state, id, overwrite: bool)`.
- New Tauri command `project::environment_inject_preview(state, id) -> InjectPreview`
  where `InjectPreview { paths: Vec<String>, foreign: Vec<String> }` — runs steps 1-2
  only, never decrypts, never writes. Naming follows the `module_action` convention;
  register both in `lib.rs` (import list at :31, handler list at :216).

### Step 7 — MCP (`src-tauri/src/bin/crypt-env-mcp.rs`)

- Add `"overwrite"` (boolean, not required) to the input schemas of
  `crypt_env_fill_env` (:343), `crypt_env_inject_environment` and
  `crypt_env_generate_example_env`. Description, verbatim:
  > `Destructive. Set true ONLY when the user has explicitly confirmed replacing the file at output_path. The previous contents are copied to <path>.bak first. Leave unset otherwise: the call will safely fail with a conflict naming the path if the target exists and was not created by crypt-env.`
- Forward it in `tool_inject_environment` (:1893) and `tool_generate_example_env`
  (:1936) exactly as `output_path` is forwarded today, plus the fill tool.
- Add a `409` arm alongside the existing `403`/`404` arms in each tool:
  `tool_err("target_exists: <server message>. Ask the user before retrying with overwrite=true.")`
  Advisory only — an agent can ignore it. It is not the control; the `.bak` is.
- Extend the three tool descriptions with one sentence: writes refuse to clobber
  files crypt-env did not create.

### Step 8 — CLI (`src-tauri/src/bin/crypt-env/commands/fill.rs`)

Add `--force` / `-f` to `FillArgs`; send `"overwrite": args.force` in the body at
:41. On 409, print the server message to stderr and exit non-zero. **No interactive
prompt** (§4.6).

### Step 9 — Frontend (`src/components/ProjectManager.tsx`)

- Inject button: call `environment_inject_preview` first; if `foreign` is non-empty,
  show a confirm modal listing those exact paths and stating that a `.bak` copy will
  be kept; on confirm call `environment_inject(id, true)`, otherwise `false`.
- After a successful inject, surface `unmanaged_paths` as a non-blocking warning.
- Update the TypeScript mirror of `InjectResult` (locate it by grepping `written:`
  under `src/`) with `unmanaged_paths` and `backups`, and add `InjectPreview`.
- Tailwind only, per CLAUDE.md; no new component library.

### Step 10 — Docs

`docs/reference.md`: the `overwrite` field on all three endpoints, the two new 409
codes, the marker line's exact format, the `.bak` convention, and a note to add
`*.bak` to `.gitignore`.

---

## 4. Trade-offs and alternatives considered

### 4.1 Where the shared helper lives

**DECISION — a new leaf module `src-tauri/src/envfile/`.**
The policy is pure filesystem behaviour with no knowledge of vaults, keys or HTTP.
As a leaf with a `std`-only dependency set it can be imported by `api`, `project`
and, later, the CLI, without creating a cycle and without violating CLAUDE.md's
`db`-knows-nothing-of-`api` rule. It is also the only shape that is unit-testable
without a database or a running server.

**ALTERNATIVE A — a `pub fn` inside `project`.** Works mechanically (`api` already
depends on `project`) and adds no module. Rejected: it makes `project` — a module
whose job is business logic over `db` — the owner of filesystem policy, so any
future CLI or TUI caller that just wants to write a file safely must drag in `db`
and `sqlx` behind it. Wrong direction for the dependency arrow.

**ALTERNATIVE B — inside `vault`.** `vault` is the designated orchestrator, so this
is not absurd. Rejected: `vault` is already the most-central module, and putting
file policy there invites exactly the coupling we want to avoid — the marker
containing vault state, the gate consulting the unlock status. Keeping the helper
ignorant of the vault is what makes it cheap to test.

**ALTERNATIVE C — duplicate the check at each of the three sinks.** Rejected
outright: three copies drifting apart is the mechanism that produced this bug.
`/fill` already got an RAII guard the other two never received.

**Trade-off accepted:** one more top-level module (7 in `src-tauri/src/` today, 8
after). Small, and its name states its scope.

### 4.2 `409` versus `422`

**DECISION — `409 Conflict`.** The request is well-formed and semantically valid;
what forbids it is the *current state of the filesystem*. The identical request
succeeds tomorrow if the user moves the file. That is the definition of 409, and
not of 422, which describes a body that is wrong independent of state.

**ALTERNATIVE — `422`, as the issue suggests.** Rejected for a second, practical
reason: `handle_inject_environment` already returns `422 VALIDATION_ERROR` for
`"environment has no paths configured"`. Reusing 422 would force any MCP client or
script to string-match the message to tell "you configured nothing" from "that file
is not yours" — two errors with opposite remedies.

**Sub-decision — reuse `ErrorBody { error, code }` rather than introducing a
richer conflict body carrying `paths: Vec<String>`.** The structured variant is
genuinely more machine-readable for the multi-path inject case, but it adds a
response shape to the public API surface for one rare error path, and the paths in
question are strings the caller itself just supplied. **Trade-off:** an agent must
read the path list out of prose. Acceptable — that is what agents are good at, and
`ErrorBody` stays the single error contract.

### 4.3 The marker

**DECISION — first line, exact ASCII prefix `# crypt-env:`, followed by
informational text that is never parsed:**
```
# crypt-env: managed file (project: myapp, environment: production)
```

- **Detection is prefix-only, not equality.** Renaming a project or environment
  therefore never invalidates a marker, and no "marker mismatch" error can exist.
- **No timestamp, no version.** A timestamp would make every inject dirty the file
  in git, which is intolerable for a committed `.env.example`, and would invite
  someone to start parsing it.
- **Survives inject's merge.** Verified against `project/mod.rs:334`: the merge loop
  `continue`s on any line whose trim starts with `#`, so the marker is copied into
  `lines` untouched and re-emitted by `lines.join("\n")`. `commit`'s idempotence
  check covers the other direction (never prepend a second one).
- **If the user deletes the marker**, the file reads as `Foreign` and the next write
  is refused until one `overwrite: true`, which restores it. This is the documented
  recovery path, not a bug — and it is the correct default, since a hand-edited file
  is exactly the kind we should hesitate over.
- **A planted marker defeats the gate.** Anyone able to write `# crypt-env:` into a
  file can make crypt-env overwrite it. Accepted: the threat model here is accident,
  not adversary. An adversary who can write to the target already has the ability to
  destroy it directly, and the API caller is already authenticated. Adversarial path
  control is issue #7's problem, not this one.

**Leakage — accepted and documented.** The marker puts the project and environment
names in plaintext into a file that may be committed to a public repository. These
names are not secrets: they already appear in generated filenames (`.env.production`),
in the API surface, and in the caller's own request. The marker never contains an
item name, a key, or a value. **Trade-off:** a company using internal codenames as
project names discloses one codename per generated file. Judged acceptable against
the alternative below.

**ALTERNATIVE — a bare `# crypt-env: managed file`, no names.** Zero leakage, and
detection would work identically. Rejected because the names are the entire human
value of the marker: a developer finding an unexpected `.env` in a repo learns which
environment produced it. Also, the names being in the file is what makes a future
"you are injecting production into a file marked staging" check possible at no extra
cost — explicitly out of scope here (§6), but cheap to keep the door open for.

**ALTERNATIVE — sidecar state (a manifest of paths crypt-env has written, in the
DB).** Rejected: it introduces state that goes stale the moment a user moves or
copies a file, requires a `db` dependency in what should be a leaf, and is invisible
to the human reading the file. The comment is self-describing and travels with the
file.

### 4.4 Which paths are gated

**DECISION — hard-gate caller-supplied paths only; report, do not block, on
owner-configured `environment.paths[]`.**

The alternative — gate everything — breaks every existing installation on upgrade:
every user's already-populated `.env` files lack the marker, so the first inject
after the update fails for all of them at once. The paths in `environment.paths[]`
were deliberately entered by the vault owner through the GUI; treating them as
already-consented is defensible, and they are not the vector this issue describes.
Since inject writes the marker on the way past, each such file self-heals on first
touch and appears in `unmanaged_paths` exactly once.

**Trade-off / residual risk, stated plainly:** a user who saves a *wrong* path into
`environment.paths[]` still gets it clobbered, with no 409. That case is mitigated
only by the GUI confirm dialog (Step 9), which is possible because that surface has
a human in front of it. The API/MCP surface, where the human is absent, is exactly
where the hard gate applies. If this residual case ever bites, the follow-up is a
one-time backfill that marks all configured paths, after which the exemption can be
removed — noted, not scheduled.

**ALTERNATIVE — gate everything immediately.** Strictly safer, and honest. Rejected
purely on upgrade blast radius: a security fix that makes the app fail on first use
for every existing user gets disabled, not adopted.

### 4.5 Partial writes on multi-path inject

**DECISION — all-or-nothing at the gate; abort-and-report on I/O failure.**
The gate evaluates every path before anything is decrypted, so a *policy* rejection
is guaranteed to leave zero bytes written and zero plaintext on disk (AC2). Genuine
I/O failure mid-loop (disk full, EACCES on path 3 of 4) cannot be made atomic —
there is no cross-file transaction across arbitrary user-chosen locations, and
faking one with temp files plus multi-rename is unreliable on Windows (§4.6). So:
stop at the first write error and return an error naming the failing path and the
paths already written.

**ALTERNATIVE — best-effort, write what you can, report failures in a 200 body.**
Rejected: a 200 whose body admits partial failure is the shape callers ignore, and
"some services got the new credentials" is a worse state than "the operation failed
loudly".

**Trade-off accepted:** partial writes remain possible, but only from real I/O
faults, never from the safety policy. That invariant is what the tests assert.

### 4.6 The `.bak` policy

**DECISION — back up only when about to modify a `Foreign` target; one `.bak` per
path; refuse if it already exists; `0o600` on Unix.**

This is a security trade-off, not a convenience. Points, each decided:

- **A `.bak` may hold sensitive content.** It is a copy of the *victim's* prior
  contents — unknown sensitivity, possibly someone else's secrets. We tighten it to
  `0o600` (never loosen), which is at least as strict as whatever the original had.
- **crypt-env never backs up its own output.** No `.bak` for `Absent` or `Managed`
  targets. That is what stops the feature from scattering second plaintext copies of
  our own secrets across the user's disk. The only `.bak` that can exist is one the
  caller explicitly asked to create by passing `overwrite: true`.
- **Never clobber an existing `.bak`.** Silently overwriting the previous backup
  would destroy exactly the artifact the feature promises. Refuse with 409
  `BACKUP_EXISTS`; the remedy (move or delete it) is trivial and loud.

**ALTERNATIVE — timestamped `<path>.bak.<epoch>`, never collides.** Genuinely
better for recoverability. Rejected: unbounded accumulation of possibly-secret files
inside the user's repository, with a high chance of being committed. One backup,
refuse to overwrite, user cleans up.

**ALTERNATIVE — back up only for `/example`, since it is the one most likely to be
mis-aimed.** Rejected as backwards: `/fill` and `/inject` overwriting a foreign file
are the *more* destructive operations. Giving the safe endpoint the safety net and
withholding it from the dangerous ones inverts the priority.

**Sub-decision — `FileMode::Inherit` for `/example`, `Private0600` for the other
two.** Forcing `0o600` on a `.env.example` whose entire purpose is to be committed
and shared would be surprising on a shared build machine, and the file provably
contains no value. **Trade-off:** one small enum instead of a uniform rule.

### 4.7 Creation semantics and Windows

**DECISION — create with the mode at open time (`OpenOptions` + `.mode(0o600)`),
not write-then-`chmod`. No temp-file-plus-rename.**

The current `create` (`api/mod.rs:1247-1254`) writes first and chmods second: there
is a window where a secret file exists at umask permissions, and if the chmod fails
it returns `Err` *after* the target is already truncated, with no guard armed.
`OpenOptions::mode` closes both.

**ALTERNATIVE — write to a sibling temp file, then `rename` over the target.**
The textbook answer, and it would make single-file replacement atomic. Rejected on
Windows grounds, which is the primary target per CLAUDE.md: `std::fs::rename` fails
when the destination exists on Windows, so this needs `MoveFileExW`/`ReplaceFileW`
through a new winapi dependency and a `cfg`-split implementation — precisely the
"dependency that could cause problems on Windows" CLAUDE.md tells us to flag. It
also drops the temp file inside the *user's* directory (same-volume requirement),
littering their repo if we crash, and a successful rename replaces the destination's
existing ACL/mode rather than preserving it. The boring option is correct here.

**Trade-off accepted:** replacement is not atomic. A crash mid-write leaves a
truncated file. The `.bak` (for foreign targets) and `TempEnvFile`'s repaired `Drop`
(for our own) are the recovery paths.

### 4.8 Exposing `overwrite` per surface

| Surface | Mechanism | Why |
|---|---|---|
| HTTP | `overwrite: bool`, `#[serde(default)] = false` | Wire-compatible: old clients omit it and get the *stricter* behaviour |
| MCP | opt-in boolean parameter, description marked destructive | The only lever available; advisory, backed by the `.bak` |
| CLI `fill` | `--force` / `-f` | Explicit, scriptable, visible in shell history |
| GUI | preview call + confirm modal naming the exact files | The one surface with a human to ask |

**Sub-decision — no interactive prompt in the CLI.** `crypt-env fill` runs in CI and
in pipes; a prompt deadlocks a non-TTY. **ALTERNATIVE:** prompt when
`stdout.is_terminal()`, `--force` otherwise. Rejected: behaviour that silently
differs between a terminal and a pipe is exactly the implicit magic CLAUDE.md
forbids. One flag, one behaviour.

**Upgrade friction, stated:** an existing script pointing `/fill` at a pre-existing
unmarked `.env` will fail with 409 on the first run after this ships. The user passes
`overwrite` once; the file gets a `.bak` and a marker; every subsequent run is clean.
This must be in the release notes.

### 4.9 Boundary with issue #7 (path traversal via `output_dir`)

The two issues meet in the same three functions, so the boundary must be explicit:

- **#7 governs path *construction*** — sanitising `env.name` before it is
  interpolated into `{output_dir}/.env.<name>`, so the derived filename cannot
  escape `output_dir`. Its edits land inside the `write_target` resolution blocks:
  `api/mod.rs:1420-1427`, `api/mod.rs:2195-2202`, `project/mod.rs:280-290`.
- **#8 (this plan) governs path *commit*** — what may be done to a resolved path,
  whatever its provenance. Its edits land strictly *after* those blocks.

Pipeline: `resolve/sanitise (#7)` → `envfile::inspect` + policy `(#8)` → `decrypt` →
`envfile::commit (#8)`. This plan therefore does not modify a single line owned by
#7, and vice versa; only the enclosing functions are shared. **Recommendation: merge
#7 first** (smaller, purely local) and rebase this one onto it.

Neither fix substitutes for the other: this gate does **not** stop traversal, because
a traversed path that does not yet exist classifies as `Absent` and is allowed.

**Related decision — do not add `create_dir_all` to `inject_environment`.** It is
conspicuously absent there (unlike `/fill` and `/example`), and adding it would
*widen* what inject may do to the filesystem while #7's confinement is still
unlanded. Out of scope. Only the `unwrap_or_default()` at `:328` is fixed here,
because we are replacing that read anyway and it currently converts a permissions
error into "the file is empty, create it".

---

## 5. Test plan

The repo has `src-tauri/tests/vault_integration.rs` (`#[tokio::test]` + `tempfile`,
dev-deps `tempfile = "3"` and `tokio` with `rt`/`macros`). **Issue #11's plan owns
the shared HTTP harness — consume it, do not build a second one.** The unit layer
below has no harness dependency and can land first.

### 5.1 Unit — `#[cfg(test)] mod tests` in `src-tauri/src/envfile/mod.rs` (10)

1. `inspect_absent_path_returns_absent`
2. `inspect_marked_file_returns_managed_with_content`
3. `inspect_unmarked_file_returns_foreign_without_content`
4. `inspect_unreadable_path_returns_io_error_not_absent` — the `unwrap_or_default()`
   regression guard (`#[cfg(unix)]`, chmod `000`)
5. `marker_detected_after_leading_blank_lines_and_ignores_trailing_text`
6. `commit_prepends_marker_and_is_idempotent_across_two_writes`
7. `commit_refuses_foreign_target_without_overwrite_and_leaves_bytes_identical`
8. `commit_with_overwrite_creates_bak_holding_original_bytes` (+ `0o600` on Unix)
9. `commit_refuses_when_bak_already_exists`
10. `commit_never_creates_bak_for_absent_or_managed_target`

### 5.2 Unit — in `src-tauri/src/api/mod.rs` (2)

11. `temp_env_file_drop_preserves_preexisting_file` — **AC6**: create victim,
    `create_guarded` without `persist()`, drop; assert the path still **exists**,
    is zero-length or zero-filled, and contains no plaintext.
12. `temp_env_file_drop_removes_file_it_created` — the unchanged branch still works.

### 5.3 Integration — new `src-tauri/tests/env_file_guard.rs` (7)

13. `gate_rejects_foreign_target_all_endpoints` — **AC1**, table-driven over
    `/fill`, `/environments/:id/example`, `/environments/:id/inject`; asserts 409,
    `code == "TARGET_EXISTS"`, and byte-identical victim, for all three.
14. `gate_rejection_leaks_no_secret_to_disk` — **AC2**.
15. `error_body_excludes_victim_contents` — **AC3**.
16. `overwrite_true_replaces_foreign_and_bak_holds_original` — **AC4**.
17. `marker_survives_inject_merge_roundtrip` — **AC5**.
18. `inject_configured_unmarked_path_is_written_and_reported_once` — the
    grandfathering rule of §4.4, including that the second inject no longer reports it.
19. `inject_partial_write_reports_written_and_failing_paths` — `#[cfg(unix)]`,
    read-only directory for the third of four paths.

### 5.4 Manual verification (the issue's repro, inverted)

```bash
echo "important config" > /tmp/victim.yaml
sha256sum /tmp/victim.yaml
curl -sk -X POST "$BASE/environments/1/example" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"output_path":"/tmp/victim.yaml"}'
# expect: HTTP 409, code TARGET_EXISTS, message naming /tmp/victim.yaml
sha256sum /tmp/victim.yaml    # unchanged
curl ... -d '{"output_path":"/tmp/victim.yaml","overwrite":true}'
# expect: 200; /tmp/victim.yaml.bak holds the original; new file line 1 is the marker
```

---

## 6. Risks, warning signs, and what is deliberately left open

| Risk | Early warning | Mitigation |
|---|---|---|
| Upgrade friction: existing scripts hit 409 on first run | Support reports of "fill stopped working" right after release | Release note with the exact one-time `overwrite` remedy; the 409 message states it inline |
| Agents reflexively retry with `overwrite: true` | `.bak` files appearing that the user did not expect | The MCP description tells the agent to ask; the `.bak` is the real control. **This is the weakest link in the design and should be stated as such** |
| `.bak` files committed to git | `git status` noise in user reports | Documented `.gitignore` line; single `.bak` per path, not accumulating |
| Marker in a committed `.env.example` discloses a project codename | User complaint | Documented in §4.3; a marker-content setting is the escape hatch if it ever matters, deliberately not built now |
| TOCTOU between `inspect` and `commit` | Not observable in practice | Narrowed by caching the inspected content; not closed. Not worth `O_EXCL` gymnastics for an accident-focused threat model — stated, not hidden |
| Grandfathered configured paths still clobber silently | A user reports losing a file the GUI wrote to | GUI confirm modal (Step 9); the follow-up backfill described in §4.4 |

**Explicitly out of scope**, and to be filed separately if wanted:
injecting one environment into a file marked as a different environment (the marker
now makes this detectable — that is the point of carrying the names); path
confinement for `output_dir` (issue #7); `create_dir_all` for inject; secure erase
guarantees on journaling/CoW filesystems.

**Rollback.** The change is additive and gated behind a request field that defaults
to `false`. Reverting means deleting `src-tauri/src/envfile/`, restoring the three
call sites and `TempEnvFile`, and dropping the four new response fields and one new
Tauri command. Files already carrying a marker stay valid — a `#` comment is inert
in every `.env` parser and in the inject merge loop. Cost: one commit, no data
migration.
