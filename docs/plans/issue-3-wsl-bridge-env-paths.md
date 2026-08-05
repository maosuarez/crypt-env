# Issue #3 — WSL bridge: reach project `.env` files across the Windows/WSL boundary (Phase 1)

Status: plan (no code written)
Scope: new `src-tauri/src/wsl/`, `src-tauri/src/lib.rs`, `src-tauri/src/project/mod.rs` (one command signature), `src/components/ProjectManager.tsx`
Related: **#7** (path traversal via environment name), **#8** (silent truncation of `output_path`) — both touch the same write path; see §5, which is the highest-value part of this document. Also #11 (test conventions).
Label: enhancement. **Not a bug fix, not a security fix.**

> **Stale name correction.** The issue names the picker `workspace_pick_env_path`. It was renamed during the projects/environments migration and is now **`project_pick_env_path`**, at `src-tauri/src/project/mod.rs:416`, registered in `lib.rs:213`. Every reference below uses the current name.

---

## 1. Objective

Definition of done — Phase 1 only. Each item is independently checkable. Phase 2 is explicitly **not** committed (§6).

1. **`wsl_list_distros()` exists and is honest about absence.** New Tauri command in a new `src-tauri/src/wsl/` module, registered in `lib.rs`. Returns `Ok(Vec<String>)` of installed distro names. Returns `Ok(vec![])` — **never `Err`** — when `wsl.exe` is not on `PATH` (`io::ErrorKind::NotFound`), when WSL is installed with zero distros, and on every non-Windows build target. Returns `Err` only for a timeout or a genuinely unparseable response.
2. **The parser is pure and covered.** `wsl::parse_distro_list(&[u8]) -> Vec<String>` takes raw child stdout bytes and returns names. Covered by **10 unit tests** (T1–T10, §7.1) in an in-file `#[cfg(test)] mod tests`, over fixtures that include a real captured UTF-16LE-with-BOM sample, a BOM-less UTF-16LE sample, UTF-8 (the `WSL_UTF8=1` path), UTF-8-with-BOM, empty output, CRLF/trailing-blank-line noise, and a name containing a space.
3. **The command cannot hang the UI.** `wsl_list_distros` completes or errors within a hard 10 s budget, implemented with `tokio::process::Command` + `kill_on_drop(true)` inside `tokio::time::timeout`. It never acquires the `SharedState` mutex, so it cannot block unlock, save, or inject.
4. **A "Browse WSL" affordance exists in the environment path picker.** In the PATHS block of the environment editor (`src/components/ProjectManager.tsx:1138-1177`, next to the existing browse button at L1163): a control that lists distros, and on selection opens the existing native dialog **pre-seeded** at `\\wsl.localhost\<distro>\home\<user>\` (or `\\wsl.localhost\<distro>\home\` when the user directory cannot be determined unambiguously — see §3.4). The control does not render on non-Windows platforms, and does not render when the distro list is empty.
5. **Seeding is verified, not assumed.** The seed directory is probed for existence before the dialog is opened. If the probe fails (stopped distro, unsupported UNC form, WSL not running), the user sees an actionable message and falls back to the manual path input that already exists at `ProjectManager.tsx:1155` — the dialog is not opened at a wrong location.
6. **Manual verification passed on real hardware.** All of M1–M9 (§7.2) checked off, including the issue's own gate: `std::fs::write` (used by `project::inject_environment`, `src-tauri/src/project/mod.rs:357`) actually writes correctly to a real `\\wsl.localhost\...` path, and re-injection preserves unrelated keys already in that file.
7. **The UNC requirements in §5 have been communicated to #7 and #8** and appear in their plans. Phase 1 is **not closeable** while `inject_environment` still does `std::fs::read_to_string(path).unwrap_or_default()` at `src-tauri/src/project/mod.rs:328` — see §3.6, this is a data-loss gate, not a nicety.
8. **Nothing else changed.** No new Cargo dependency. No change to the API bind (`127.0.0.1:47821`). No Linux/macOS compilation target added. No DB schema change, no migration, no persisted state. `cargo check` clean on Linux (the maintainer's `cargo check` host) and on Windows.

---

## 2. What is being mitigated

**Checkable statement of the removed friction:**

> After this change, a Windows user whose project lives inside a WSL distro can attach that project's `.env` to an environment by clicking a distro name and picking the file in the normal file dialog — without knowing the UNC syntax, without knowing their distro's registration name, and without silently attaching a path that does not currently resolve.

This is an **enhancement**, and the baseline it improves on is not "impossible" — it is "undiscoverable". Today the manual path input at `ProjectManager.tsx:1155` already accepts any string, and Windows already resolves `\\wsl.localhost\Ubuntu\home\me\app\.env`. A user who knows that can type it and inject works. So the honest accounting is:

| | Before | After |
|---|---|---|
| Attach a WSL `.env` at all | Possible, if you know the UNC form and your distro's exact registered name | Two clicks, no knowledge required |
| Discover that it is possible | Nothing in the UI suggests it | A visible "WSL" control appears exactly on machines that have WSL |
| Typo in distro name / distro not started | Path is stored, inject fails later (or worse, §3.6) | Probe fails at pick time, before the path is stored |
| No WSL installed / not on Windows | — | Affordance absent; no dead UI, no error |

**Who this actually helps.** Windows developers who run WSL2. That is a narrow slice of any general user base, but this project is Windows-first by construction — NSIS is the primary bundle target, biometric unlock is Windows Hello (`src-tauri/src/biometric/mod.rs:20`), and the documented dev loop is PowerShell. Among Windows developers who keep `.env` files in a secrets manager, keeping the actual source tree in WSL is the common case, not the exotic one. The maintainer is the archetypal user: per `CLAUDE.local.md`, this repository itself lives at `/home/maosuarez/Programas/crypt-env` inside WSL2 Ubuntu and is built from Windows against `\\wsl.localhost\Ubuntu\...`. That matters operationally, because item 6 of §1 is a **manual** gate that cannot be automated in CI — and there is real hardware to run it on.

**Cost proportionality.** One new module of roughly 150 lines including tests, one command, one optional argument on an existing command, one UI control, zero new dependencies, zero persisted state, and a rollback that is three file reverts (§8). For a feature that removes a daily papercut for the person maintaining it, this is proportionate. If it were expensive, the correct answer would be a README paragraph (§4, alternative A) — and that alternative is not a strawman.

**Explicitly NOT mitigated:**
- Running the CLI, MCP server, or API client from inside WSL. That is Phase 2, uncommitted (§6).
- Any secret ever crossing the boundary by a route other than a file the user explicitly chose.
- The stopped-distro silent-overwrite failure, which is **#8's** helper to fix (§3.6, §5).

---

## 3. Implementation steps

Ordered. Steps 1–3 are Rust-only and independently testable; step 4 is the UI; steps 5–6 are gates.

### 3.1 Step 1 — New module `src-tauri/src/wsl/mod.rs`

Declared in `lib.rs` alongside the existing `mod` list (`api`, `biometric`, `cli`, `crypto`, `db`, `mcp`, `project`, `share`, `tls`, `vault`). A dedicated module — not a helper inside `project` — for two reasons: the command name `wsl_list_distros` mandated by the issue only satisfies the `module_action` convention if `wsl` is a module, and the module must stay decoupled (it knows nothing about `db`, `vault`, `api`, or `SharedState`; `project` does not call into it; the frontend composes the two).

Public surface — three functions, one of them a command:

- `pub fn parse_distro_list(bytes: &[u8]) -> Vec<String>` — pure, no I/O, no `unwrap`. Decode → split → trim → drop empties. Decoding order:
  1. UTF-8 BOM (`EF BB BF`) → strip, decode UTF-8 lossy.
  2. UTF-16LE BOM (`FF FE`) → strip, decode via `String::from_utf16_lossy` over `chunks_exact(2).map(u16::from_le_bytes)`.
  3. No BOM, but length is even **and** every second byte in the first 32 bytes is `0x00` → treat as BOM-less UTF-16LE, same decode. (This case is real: some `wsl.exe` builds omit the BOM on `--quiet`.)
  4. Otherwise → UTF-8 lossy.
  Then `lines()`, `trim()` (this also disposes of `\r` from CRLF and of any stray NUL), drop empty lines. No filtering of names (see §4, decision D6).
- `pub fn unc_root(distro: &str) -> String` — pure. Returns `\\wsl.localhost\<distro>\`. A sibling `pub fn unc_root_legacy(distro: &str) -> String` returns `\\wsl$\<distro>\`. Both testable on Linux.
- `#[tauri::command] pub async fn wsl_list_distros() -> Result<Vec<String>, String>` — see step 2.
- `#[tauri::command] pub async fn wsl_distro_home(distro: String) -> Result<String, String>` — see step 3.

In-file `#[cfg(test)] mod tests` per the convention #11 establishes. These tests need **no** database and therefore do **not** consume #11's `test_support` harness — they are pure-function tests, exactly the category #11 puts in-file. Nothing is invented here; nothing is duplicated from there.

### 3.2 Step 2 — `wsl_list_distros()`

Command body, Windows arm:

- Spawn `wsl.exe --list --quiet` with `tokio::process::Command`, `.env("WSL_UTF8", "1")`, `.kill_on_drop(true)`, stdout+stderr piped, and — because this is a GUI process — `CREATE_NO_WINDOW` (`0x0800_0000`) via `std::os::windows::process::CommandExt::creation_flags`, so no console flashes on screen.
- Wrap the `output()` future in `tokio::time::timeout(Duration::from_secs(10), …)`.
- Map results: spawn error with `ErrorKind::NotFound` → `Ok(vec![])`. Any other spawn error → `Err` with the `io::Error` message (no path or secret content — there is none here). Non-zero exit status → `Ok(vec![])` (WSL present but reporting no installation; `wsl --list` on a WSL-less-but-stubbed system exits non-zero with a "no installed distributions" message that is localized and therefore not worth parsing). Timeout → `Err("WSL did not respond within 10s")`.
- Success → `parse_distro_list(&output.stdout)`.

Non-Windows arm: `#[cfg(not(target_os = "windows"))] { Ok(Vec::new()) }`. The command is registered unconditionally in `lib.rs` on all targets — only its body is `cfg`-gated. Rationale in §4, decision D5.

No `unwrap()`, no `expect()`. No `SharedState`, no vault key, no logging of anything beyond distro names (which are not secrets, but are also not logged — nothing in this module logs).

### 3.3 Step 3 — `wsl_distro_home(distro)`

Returns the directory the dialog should be seeded at, or an `Err` the UI can show verbatim.

1. Reject a `distro` argument containing `\`, `/`, `..`, or a NUL. It is going straight into a path; it comes from a list we produced, but the command is invokable with anything.
2. Build `\\wsl.localhost\<distro>\home\`. Probe with `std::path::Path::new(&p).is_dir()`, itself wrapped in the same 10 s `spawn_blocking` + `timeout` shape (a UNC probe against a cold distro blocks; see §4, decision D4).
3. If that fails, retry once with the legacy `\\wsl$\<distro>\home\` form. If that also fails → `Err("cannot reach \\\\wsl.localhost\\<distro> — is the distro running?")`.
4. On success, `read_dir` the `home` directory. If it yields **exactly one** entry and that entry is a directory, return that child path (`…\home\<user>\`). Otherwise return `…\home\`. No guessing from the Windows username — see §4, decision D3.
5. Never propagate the raw `io::Error` for the `read_dir` step; a failure there is not fatal, it just means we return `…\home\`.

**Documented side effect:** touching `\\wsl.localhost\<distro>\…` starts the distro if it is stopped. Clicking "Browse WSL" can therefore boot a WSL VM, taking several seconds and consuming memory. This is inherent to the UNC bridge, not to our implementation, and it is why the whole path is behind an explicit user click rather than being probed eagerly when the environment editor opens. It must be stated in the button's tooltip.

### 3.4 Step 4 — Seed the existing dialog

Change `project_pick_env_path` (`src-tauri/src/project/mod.rs:416`) from `()` to:

```
pub async fn project_pick_env_path(start_dir: Option<String>) -> Result<Option<String>, String>
```

and, when `start_dir` is `Some`, call `.set_directory(dir)` on the `rfd::FileDialog` builder inside the existing `spawn_blocking`. Everything else about the command is unchanged. The existing frontend call site (`ProjectManager.tsx:732`, `invoke('project_pick_env_path')` with no arguments) keeps working: Tauri deserializes a missing argument into `None`.

This is the **only** one of the five `rfd::FileDialog` call sites that gains WSL awareness. The other four — `project_export` (`project/mod.rs:496`), `project_import` (`project/mod.rs:513`), and the two in `src-tauri/src/vault/share_commands.rs:166,188` — are left alone. Rationale in §4, decision D7.

Frontend, `src/components/ProjectManager.tsx`:

- Add `const [isWindows] = useState(() => platform() === 'windows')` using `@tauri-apps/plugin-os`, mirroring the existing pattern in `src/components/WindowChrome.tsx:3,15`. Nothing new is added to Cargo or package.json.
- On mount of the environment editor, when `isWindows`, `invoke<string[]>('wsl_list_distros')` once and hold the result in local component state. Not in the Zustand store (`src/store/projectStore.ts`) — it is ephemeral machine state, not vault state, and nothing else needs it. Not a TanStack Query either; a single fire-and-forget on an editor that is already mounted per-session is enough, and adding a query key for it is ceremony.
- Render, in the PATHS row at `ProjectManager.tsx:1154-1176` beside the existing browse button (L1163), a "WSL" button that is present only when `isWindows && distros.length > 0`. With one distro it acts directly; with several it opens a small inline list (the existing panel/select styling in this file, Tailwind classes only, no new component library).
- Click handler: set a busy state on the button → `await invoke<string>('wsl_distro_home', { distro })` → on success `await invoke<string|null>('project_pick_env_path', { startDir: home })` and reuse the exact existing result handling from `handlePickEnvPath` (`ProjectManager.tsx:730-744`, including the `folderNameFromPath` project-name inference) → on failure `showToast(String(e), 'error')` and leave focus in the manual input. Best implemented by extracting the shared tail of `handlePickEnvPath` into a small local helper rather than copying it.
- While busy: the button shows a spinner/disabled state. This is the whole "what the UI shows meanwhile" answer — the list fetch is silent and speculative, and the only user-visible wait is behind an explicit click.

### 3.5 Step 5 — Register and check

`src-tauri/src/lib.rs`: add `mod wsl;`, add `wsl::{wsl_distro_home, wsl_list_distros}` to the `use` block (near the `project::{…}` import at L32), add both names to the `invoke_handler!` list (near L213, next to `project_pick_env_path`). Then `cargo check` on Linux and `cargo check` / `pnpm tauri dev` on Windows per `CLAUDE.local.md`.

### 3.6 Step 6 — The gate that is not ours to write

`inject_environment` reads each target with `std::fs::read_to_string(path).unwrap_or_default()` (`src-tauri/src/project/mod.rs:328`) and then writes the merged result with `std::fs::write` (L357). It does not call `create_dir_all`. `unwrap_or_default()` treats *any* read failure as "the file is empty".

For local paths that is merely sloppy. For a `\\wsl.localhost\...` path it is a data-loss path, and it is the **most likely real-world failure of this feature**: the distro is stopped, the read fails, the existing `.env` — which may hold dozens of keys this environment does not manage — is treated as empty, and the write replaces it with only this environment's keys. Silently. Success is reported.

**#8 owns the write helper.** This plan does not write it. What this plan does:

- States the requirement: the read must distinguish `ErrorKind::NotFound` (legitimate: create a new file) from every other error kind (`PermissionDenied`, `NotConnected`, the various Windows network errors a dead 9p mount produces) — which must abort that path with an error, not proceed.
- Declares the sequencing gate: **Phase 1 is not closeable while L328 stands as written.** If #8 lands first, this is free. If #8 slips, this plan lands the minimal three-line `match e.kind()` guard at L328 itself and #8 subsumes it later. Shipping "Browse WSL" on top of a silent-overwrite path is not acceptable, and merging the feature while pretending the gate is someone else's problem is exactly how it would happen.

---

## 4. Trade-offs and alternatives considered

**A. Do nothing; document the UNC path in the README.** *This is the strongest competitor and deserves a straight answer.* Cost: zero. Users can already type `\\wsl.localhost\Ubuntu\home\me\app\.env` into the existing input at `ProjectManager.tsx:1155`, and it already works end-to-end. Rejected because the delta is discoverability and pre-flight validation, not capability: nothing in the UI hints the boundary can be crossed, users must know their distro's *registered* name (which is often not what they call it), and a typo or a stopped distro currently surfaces as a confusing later failure — or, per §3.6, as silent data loss. The feature is convenience, and it should be sold as convenience. If the implementation cost were meaningfully higher than §2's estimate, A would win.

**B. Ship the Phase 2 CLI-inside-WSL instead, and skip the path picker.** Strength: it is the architecturally correct answer — a client on the Linux side talking to the vault beats poking Linux files through a network share, and it is what Docker Desktop actually does. Rejected for now: three unanswered blocking questions (§6), at least one of which (TLS trust distribution) has no design at all today, and one of which (`127.0.0.1` reachability) may be unanswerable without loosening the API bind, which `CLAUDE.md` marks critical. Deferring is not a judgement that B is worse — it is that B's scope is unknown and Phase 1's is not.

**C. `--list --verbose` (as the issue literally specifies) vs `--list --quiet`.** *Deviating from the issue here.* `--list --verbose` output is UTF-16LE, column-aligned, carries a `*` default marker, and is **localized** — both the header row and the `Running`/`Stopped` state strings are translated on non-English Windows, so any parser keying on those strings is broken for a large fraction of users, and a column-offset parser breaks on the translated header widths. `--list --quiet` emits one bare name per line: no header, no marker, no localized text, no columns. The only things lost are the running-state and the default-distro flag. Neither is needed: the picker only needs names, and *selecting* a distro starts it anyway (§3.3), so displaying state would be decoration that goes stale the moment it is read. Reading state to gray out stopped distros would be actively wrong — they are perfectly selectable. Decision: `--list --quiet`, no state. This is scope reduction relative to the issue and should be called out at review.

Also considered: `--list --running` (only started distros — hides exactly the distro the user wants to start) and parsing the registry under `HKCU\Software\Microsoft\Windows\CurrentVersion\Lxss` (no subprocess, no encoding problem, instant — but it is an undocumented implementation detail of WSL that Microsoft can change without notice, and it would need a Windows-registry crate). Both rejected.

**D1. `WSL_UTF8=1` + defensive decode, vs decode-only.** Setting `WSL_UTF8=1` in the child environment makes recent `wsl.exe` builds (WSL 0.64+, broadly Win11 and updated Win10) emit UTF-8, which sidesteps the whole problem. It is ignored — harmlessly — by older builds. So we set it *and* keep the full BOM/heuristic decoder (§3.1), because "recent enough" cannot be assumed and a wrong guess produces a distro list full of NUL bytes. Belt and braces, and the belt costs one `.env()` call.

**D2. Hand-rolled UTF-16LE decode vs `encoding_rs`.** Hand-rolled, ~10 lines over `chunks_exact(2)` + `String::from_utf16_lossy`. `encoding_rs` is an excellent, widely-deployed, pure-Rust crate with no Windows-specific risk — this is not a warning about it, it is a scope argument: we decode exactly two known encodings from one known producer, and `CLAUDE.md`'s rule is to justify dependencies, which for this one comes out negative. **Nothing is added to `Cargo.toml` by this plan.** (`CLAUDE.md`'s Windows-dependency warning rule is therefore not triggered; if review disagrees and wants `encoding_rs`, note that it is `no_std`-capable, has no build script beyond a trivial one, and compiles cleanly on `x86_64-pc-windows-msvc` — the warning would be "none known".)

**D3. `\\wsl.localhost\` vs `\\wsl$\`.** Emit `\\wsl.localhost\` as primary. It is the current form, preferred on Win10 21H2+ and Win11, and is what `CLAUDE.local.md` documents for this very repository. `\\wsl$\` is the legacy form; it still resolves on modern builds as an alias, and it is the *only* form that resolves on pre-21H2 builds where `\\wsl.localhost\` does not exist. Rather than detect the Windows build number — brittle, and the mapping is not clean — §3.3 probes `\\wsl.localhost\` and falls back to `\\wsl$\` once. Two filesystem probes on a click is cheap; version sniffing that silently picks wrong is not. Stored paths therefore normally carry the `.localhost` form, and on old builds carry `\\wsl$\`; both are opaque strings to everything downstream.

**D4. Guessing `home\<user>\` from the Windows username, vs enumerating.** The Windows username and the WSL username are unrelated and frequently differ. Guessing yields a nonexistent directory, `rfd`'s `set_directory` then silently no-ops, and the dialog opens somewhere arbitrary — the worst outcome, because it looks like the feature is broken rather than unavailable. Enumerating `\\wsl.localhost\<distro>\home\` and descending only when there is exactly one child is reliable and self-limiting (multi-user distros stop at `home\`, which is still a useful seed). Its cost is the distro-start side effect, stated in §3.3 and surfaced in the tooltip. Rejected third option: parse `/etc/passwd` through the share — more I/O, more parsing, same side effect, no better answer.

**D5. Async/timeout strategy.** `tokio::process::Command` + `kill_on_drop(true)` inside `tokio::time::timeout`, not `spawn_blocking` + `std::process::Command`. `spawn_blocking` is the idiom already used in this file for `rfd` (`project/mod.rs:417,495,512`) and would have been the consistent choice — but a `timeout` around a `JoinHandle` does not cancel the blocked thread: it returns while the thread stays parked on a hung `wsl.exe`, leaking a blocking-pool slot for as long as the child lives. `tokio::process` cancels for real and kills the child on drop. `tokio` is already `features = ["full"]` in `src-tauri/Cargo.toml:46`, so `process` is available with no manifest change. The one place `spawn_blocking` *is* still correct is the `is_dir()` UNC probe in §3.3 — there is no async filesystem primitive for it, and the leak window there is bounded by the SMB client's own timeout rather than by a process we control. Accepted, and stated rather than hidden.

**D6. `std::process::Command` (via `tokio::process`) vs adding `tauri-plugin-shell`.** `CLAUDE.md`'s first-session dependency list mentions `tauri-plugin-shell`, but it is **not** in the actual manifest — `src-tauri/Cargo.toml:23-27` lists `opener`, `global-shortcut`, `clipboard-manager`, `os`, `updater` only. Keep it that way. The shell plugin's purpose is to let the *webview* spawn processes; adding it to run one command from Rust would grant a broad new capability to the frontend of a secrets manager in exchange for nothing, since Rust can spawn processes natively. This is a security argument, not a preference: fewer webview capabilities is strictly better here, and the plugin would also need capability entries that someone later widens. Rejected.

**D7. Filtering `docker-desktop` / `docker-desktop-data` out of the list.** Tempting — they are WSL distros nobody wants to browse, and `docker-desktop-data` in particular has nothing useful in `/home`. Rejected: it hardcodes one vendor's naming into our parser, it is a guess about intent, and it breaks the moment Docker renames anything. Show what WSL reports. Users recognise their own distros.

**D8. Non-Windows behaviour: `Ok(vec![])` vs an `Err("unsupported")` vs `cfg`-ing the command out of `invoke_handler`.** `tauri.conf.json:37` targets `["nsis", "dmg", "deb"]`, so this ships on macOS and Linux. `cfg`-ing the registration list is rejected — conditional `invoke_handler!` entries are easy to get subtly wrong and produce a runtime "command not found" that only appears on one platform. `Err` is rejected because it forces the frontend to distinguish "no WSL" from "broken", and the frontend's response to both is identical: hide the button. `Ok(vec![])` collapses non-Windows, WSL-not-installed, and WSL-with-zero-distros into one signal — *there is nothing to browse* — which is exactly the frontend's decision variable. The `platform() === 'windows'` check on the frontend is then a pure optimisation (skip a pointless IPC round trip), not a correctness requirement.

**D9. Extending `project_pick_env_path` with `start_dir` vs a new `wsl_pick_env_path(distro)`.** A dedicated WSL picker would duplicate the dialog configuration (title, filters) and drag `rfd` concerns into the `wsl` module, coupling it to something it has no business knowing. Worse, it would collapse two distinguishable outcomes — "could not reach the distro" and "user cancelled the dialog" — into one `Option`, and the UI wants to say different things about them. Splitting into `wsl_distro_home` (reachability) + `project_pick_env_path(start_dir)` (selection) keeps one dialog code path and gives the frontend two distinct failures. Cost: two IPC calls per click instead of one. Irrelevant at human latency.

**D10. Does `rfd`'s `set_directory` accept a UNC path?** `rfd` 0.14's Windows backend drives the native `IFileDialog`, and `set_directory` resolves the path via `SHCreateItemFromParsingName` before `SetFolder`/`SetDefaultFolder`. That API does accept UNC paths, and `rfd`'s own documentation notes the directory must exist. **This is a claim to verify (M3), not to assert** — the failure mode if it is wrong is quiet: `set_directory` no-ops and the dialog opens at the shell default, so the button appears to do nothing. Verification is the §3.3 existence probe plus M3 on real hardware. Fallback if M3 fails: keep `wsl_list_distros` and `wsl_distro_home`, drop the dialog seeding, and have the WSL button instead **insert the resolved UNC prefix into the manual path input** at `ProjectManager.tsx:1155` for the user to complete. That fallback delivers most of §2's value, needs no new Rust, and is a UI-only change — so the feature does not die on this uncertainty.

---

## 5. Cross-cutting: what #7's and #8's helpers must do about UNC paths

**This is the most important section of this plan.** #7 adds a containment check over `output_dir`-derived paths; #8 adds a no-clobber/marker/backup helper. Both sit directly on `inject_environment`'s write path (`src-tauri/src/project/mod.rs:268-364`), which is precisely the path a WSL `.env` travels. A naive implementation of either will break this feature — or, in the reverse direction, this feature will look like a way around a security control. Requirements, stated so neither happens:

1. **Explicit `environment.paths` entries are not `output_dir`-derived and must not be forced under any base directory.** #7's containment applies to the `output_dir` join at `project/mod.rs:286-289`, where an attacker-influenced environment *name* is concatenated into a filename. It must **not** be applied to `env.paths` (L280) or to an explicit `output_path` (L281-284): those are absolute paths the user picked in a native dialog, and a WSL `.env` is by definition outside every local base directory. Forcing containment there does not add security — the input is already trusted-by-selection — and it kills this feature outright.
2. **`Path::components()` on a UNC path starts with `Component::Prefix`, not `Component::RootDir`.** `\\wsl.localhost\Ubuntu\home\me\.env` yields `Prefix(PrefixComponent { kind: Prefix::UNC("wsl.localhost", "Ubuntu") })`, then `RootDir`, then normal components. A traversal check that scans for `ParentDir`/`CurDir` components is fine. One that assumes the first component is `Prefix::Disk(_)`, or that a path is "absolute" only if it starts with a drive letter, is not.
3. **`std::fs::canonicalize` on Windows returns verbatim paths, and rewrites the UNC prefix.** `\\wsl.localhost\Ubuntu\x` canonicalizes to `\\?\UNC\wsl.localhost\Ubuntu\x` — the leading `\\` is *replaced*, not extended. Therefore a check of the form `canonicalize(child).starts_with(base)` where `base` was not itself canonicalized **always fails** for UNC inputs. Canonicalize both sides or neither.
4. **`Prefix::UNC` and `Prefix::VerbatimUNC` are unequal values for the same location.** Never compare `Prefix` variants for equality, and never compare a canonicalized path against a non-canonicalized one component-wise.
5. **`canonicalize` requires the target to exist, and a UNC target to a stopped distro does not.** A helper that canonicalizes to decide whether a file already exists must treat a canonicalize failure as **unknown**, never as "does not exist, safe to create". This is #8's core case and it is the same root cause as §3.6.
6. **Verbatim (`\\?\`) paths disable OS-level path normalization — `..` is *not* resolved.** Any helper that canonicalizes a base and then joins untrusted components onto the result loses the protection it thought it had. Reject `..` lexically **before** joining; never rely on canonicalization to strip it.
7. **Case sensitivity is split across the boundary.** The UNC host and share components (`wsl.localhost`, the distro name) compare case-insensitively, like all Windows path prefixes. Everything after them lands on a Linux filesystem through 9p and is **case-sensitive** — `\\wsl.localhost\Ubuntu\home\Me` and `…\home\me` are different directories. A comparison helper that lowercases the whole path to compare is wrong on the tail; one that compares the whole path case-sensitively is wrong on the prefix. If comparison is unavoidable, split at the prefix.
8. **The read side needs the same care as the write side.** #8's helper must cover `read_to_string` at L328, not only `write` at L357 — see §3.6. Distinguishing `ErrorKind::NotFound` from every other error kind is the single required behaviour.
9. **`create_dir_all` is still not called anywhere on this path, and this plan does not add it.** Worth noting for #8: auto-creating parent directories across a 9p mount has different semantics (ownership, permissions) than locally, so if #8 adds it, that is a decision to make explicitly rather than inherit.

---

## 6. Phase 2 — deferred spike, not designed here

The issue is explicit that a CLI running inside WSL and talking back to the Windows vault is **not committed scope**. This plan does not design it. It records the three blocking questions, and how each gets answered — as a timeboxed investigation producing written answers and no committed code.

**Q1 — Does building `crypt-env` for Linux drag in Tauri/GTK?** Almost certainly yes: `[[bin]]` targets share package-level dependencies, so the `crypt-env` and `crypt-env-mcp` binaries in `src-tauri` link the same dependency graph as the library, `tauri` included. *How to answer:* in a clean container, `cargo build --bin crypt-env --target x86_64-unknown-linux-gnu` and observe whether `tauri`, `glib-sys`, `webkit2gtk-sys` compile. If they do, Phase 2 requires splitting the CLI into its own crate in a workspace — a substantial refactor that must be scoped separately, not smuggled in.

**Q2 — Is `127.0.0.1:47821` reachable from WSL2?** A configuration question, not a code question. Under default NAT networking, WSL2 is a separate network namespace: its `127.0.0.1` is its own loopback, not the Windows host's, so the answer is **no** — reaching Windows requires the host IP from `/etc/resolv.conf` (or `$(hostname).local`), and the server does not listen there. Under `networkingMode=mirrored` (`.wslconfig`, Win11 22H2+), host loopback is shared and the answer is **yes**. *How to answer:* `curl -k https://127.0.0.1:47821/health` from inside WSL under both modes, on real hardware. **Constraint that is not negotiable:** the bind stays `127.0.0.1` (`CLAUDE.md`, security-critical). Any Phase 2 design that begins with "just bind `0.0.0.0`" is rejected before it is written.

**Q3 — How does a WSL client come to trust the self-signed TLS certificate?** No mechanism exists today (`src-tauri/src/tls/`). *How to answer:* produce a written proposal covering where the cert is exported from, how it reaches the Linux trust store or a client-side pin, and what happens on regeneration. **Any proposal that copies a private key into the WSL filesystem is dead on arrival** — the WSL side is a different trust domain with different filesystem permissions.

**Gate:** Phase 2 is scheduled as committed work only once Q1, Q2, and Q3 have written answers. Until then it is not designed, not estimated, and not promised.

---

## 7. Verification

### 7.1 Automated — `parse_distro_list` and `unc_root`, in-file `#[cfg(test)] mod tests`

All pure; all run on Linux in CI; none need a DB, a vault, or #11's harness. A helper `fn utf16le(s: &str, bom: bool) -> Vec<u8>` builds synthetic fixtures inline (raw UTF-16 byte literals in source are unreadable), and **one** real capture from the maintainer's machine is committed as `src-tauri/tests/fixtures/wsl-list-quiet.bin` as ground truth for T9.

| # | Input | Expected |
|---|---|---|
| T1 | UTF-16LE **with** BOM, `"Ubuntu\r\ndocker-desktop\r\n"` | `["Ubuntu", "docker-desktop"]` |
| T2 | UTF-16LE **without** BOM, same content | same |
| T3 | UTF-8, no BOM (the `WSL_UTF8=1` path) | same |
| T4 | UTF-8 **with** BOM (`EF BB BF`) | same |
| T5 | Empty byte slice | `[]` |
| T6 | Only CRLFs and spaces | `[]` |
| T7 | Trailing blank lines, mixed `\n` / `\r\n` | names only, no empties |
| T8 | A name containing a space (`"openSUSE Leap 15.5"`) | preserved verbatim, not split |
| T9 | The committed real `wsl-list-quiet.bin` capture | the exact distro list on that machine |
| T10 | `unc_root("Ubuntu")` / `unc_root_legacy("Ubuntu")` | `\\wsl.localhost\Ubuntu\` / `\\wsl$\Ubuntu\` |

Not unit-testable, by construction: everything that touches a real UNC path or spawns `wsl.exe`. CI has no WSL. That is why §7.2 is a gate and not a suggestion.

### 7.2 Manual — the real gate (maintainer's Windows + WSL2 Ubuntu, per `CLAUDE.local.md`)

| # | Check | Pass condition |
|---|---|---|
| M1 | `wsl_list_distros` on the dev machine | Returns the real distro list; no console window flashes |
| M2 | Same build on a Windows machine/VM without WSL | `Ok([])`, WSL button absent, no error toast |
| M3 | Click WSL → pick distro | Native dialog opens **at** `\\wsl.localhost\Ubuntu\home\maosuarez\` — confirms D10 |
| M4 | Pick a `.env` there | Stored path is the UNC string; it appears in the PATHS list |
| M5 | **The issue's own gate:** inject to that path | `std::fs::write` succeeds; `cat` from inside WSL shows correct content and LF-only line endings (no CRLF conversion over 9p) |
| M6 | Pre-seed the target file with unrelated keys, re-inject | Unrelated keys survive — exercises the L328 read over UNC |
| M7 | `wsl --terminate Ubuntu`, then inject to a stored UNC path | Before §3.6's fix: reproduces the silent overwrite (**expected failure, documents the bug**). After: errors without writing |
| M8 | Force the timeout (e.g. cold-boot WSL then click immediately) | UI stays responsive, button shows busy, error after ≤10 s |
| M9 | Inspect the written file from inside WSL | Ownership/permissions usable by the Linux user; the file is not root-owned or unreadable |
| M10 | macOS or Linux build (`dmg`/`deb` targets) | No WSL button; no console errors from the absent-command path |

M7 is deliberately listed as a failure to *observe* before it is a check to *pass* — it is the evidence that §3.6's gate is real and not theoretical.

---

## 8. Rollback

Fully reversible, no data implications:

- Delete `src-tauri/src/wsl/`; remove `mod wsl;`, the `use`, and the two `invoke_handler` entries from `lib.rs`.
- Revert `project_pick_env_path`'s signature to `()` and drop the `set_directory` call.
- Remove the WSL button and its handler from `ProjectManager.tsx`.

No schema change, no migration, no settings key, no persisted state — nothing to undo in the vault. UNC paths already stored in `environments.paths` are plain strings and **remain valid and functional after a rollback**; the user simply loses the convenient way to enter new ones. The §3.6 read-error guard, if this plan ends up landing it, is *not* part of the rollback — it is a correctness fix that stands on its own.

---

## 9. Documentation

Per `CLAUDE.md`, no new `.md` files beyond this plan. On merge:

- `docs/reference.md` — add `wsl_list_distros` and `wsl_distro_home` to the Tauri command list, and note `project_pick_env_path`'s new optional `startDir` argument.
- A short "WSL paths" note in the user-facing docs stating the `\\wsl.localhost\` form, that browsing starts a stopped distro, and that a stopped distro must be running for inject to work.
