# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [1.0.1] - 2026-07-28

### Fixed

- **CRITICAL: Windows installer could silently wipe the user's entire PATH environment variable.** The NSIS installer's PATH-safety guard (added in a prior fix) inferred whether the registry value had been truncated by inspecting the *length of the value ReadRegStr returned* — but `ReadRegStr` itself silently truncates any value longer than NSIS's internal string cap with no error signal. A sufficiently long PATH could come back already truncated to something *under* the guard's threshold, pass the check as "safe", and then get written back verbatim — permanently destroying everything past the truncation point. Fixed by querying the registry directly via `RegQueryValueExW` (Win32 API, through NSIS's `System::Call`) to measure the *true* on-disk size of `HKCU\Environment\Path` before any bounded NSIS string variable is involved, so the decision to proceed is based on ground truth instead of a value that may already be mangled. Applies to both the install-time `AddToUserPath` and uninstall-time `RemoveFromUserPath` hooks (`src-tauri/nsis/installer_hooks.nsi`). Verified by compiling the installer script standalone with `makensis`.
- Removed `src-tauri/nsis/path_setup.nsh`, an unreferenced duplicate of the PATH-management logic that was not wired into the build (`tauri.conf.json` only points at `installer_hooks.nsi`) — its divergence from the real file was itself a hazard.

### Known Limitations (Carried Forward)

- This fix is Windows-only; the Linux (`deb`) and macOS (`dmg`) targets do not modify `PATH` programmatically at all (see `docs/building.md`'s manual install instructions), so they were never exposed to this bug class.

---

## [1.0.0] - 2026-07-28

### Changed

- **Major Architecture Refactor: Projects & Environments** — Replaced flat "Workspaces" model with hierarchical Projects containing typed Environments. Every environment variable is now a real vault item (not literal key=value). Projects are now the primary navigation landing page.
  - **Backend**: New tables `projects`, `environments`, `environment_vars`, `item_projects` (many-to-many), `project_categories`. Deleted `workspace/mod.rs`. Item ownership tracked explicitly; global items reusable across projects.
  - **Database**: Added `items.is_global` plaintext column (queryable without decryption). One-time migration `vault::migrate_literal_vars_to_items` converts legacy literal vars to real encrypted items on unlock.
  - **Deletion Logic**: Project deletion is transactional with preview of impact (environments deleted, items deleted vs orphaned). Un-globaling multi-owner items forks them into independent copies.
  - **Frontend**: New `ProjectManager.tsx` screen (project list → detail with environment cards → environment editor). `MainVault.tsx` renamed to `GlobalSecrets.tsx` (filtered to `isGlobal` items only, reachable via footer link). New `src/components/itemFields/` extracted shared per-type field components used by both ProjectManager and EditItem.
  - **Tauri Commands**: New `vault_create_project_item`, `vault_set_item_global`, `vault_get_item_owners`, `project_preview_delete`. Deleted `workspace_*` commands.
  - **CLI**: Replaced `crypt-env workspace` with `crypt-env project` (list/inject/delete/delete-env). Inject now works per-environment.
  - **MCP Server**: Renamed tools: `crypt_env_list_projects`, `crypt_env_inject_environment`, `crypt_env_list_environments_by_name`, `crypt_env_inject_env_by_name`. Legacy `crypt_env_share_workspace_send/receive` unchanged (out of scope, workspace-table-backed).
  - **Types**: `Workspace` → `Project` + `Environment`. `WorkspaceVar` → `EnvironmentVar`. `workspaceStore.ts` → `projectStore.ts`.
  - **Config**: Added `pnpm-workspace.yaml` for monorepo configuration (esbuild disabled).

- **Mandatory Project + Environment Scoping** — All interfaces that operate on vault items now require an explicit project + environment scope. Scope is a filter over linked variables, not access control.
  - **REST API**: Scoped endpoints (`GET`/`POST /items`, `GET /commands`, `POST /fill`, `POST /share/listen`, `POST /share/connect`, `POST /share/import`, `POST /relay/receive`) require either `environment_id` (i64) or both `project` and `environment` (case-insensitive names) as query params. Missing or unresolvable scope returns `422 VALIDATION_ERROR`. `GET`/`PUT`/`DELETE /items/:id` and `POST /items/:id/reveal` remain unscoped by design. New `POST /environments/:id/example` generates placeholder-only env files. `POST /environments/:id/inject` now takes a JSON body `{output_path, output_dir}`.
  - **CLI**: New shared scope resolver (`commands/scope.rs`) applied across all scoped commands. Resolution order per field: CLI flags (`--project`, `--env`) → `crypt-env.json` (searched upward from cwd) → cwd folder name + the project's default environment. Auto-creation of projects is restricted to `add`; every other command errors clearly if the project is missing. `--project`/`--env` added to `add`, `fill`, `inject`, `set`, `search`, `exec`, `list`, `cmd`, `sync`, `share send/receive`, `relay receive`, and `tui`.
  - **TUI**: Top bar shows the resolved project/environment; the item list is scoped to that environment's linked variables.
  - **MCP Server**: 14 tools gained matching `environment_id` / `project` / `environment` parameters in schema and docs, plus new `crypt_env_generate_example_env`.
  - **Docs**: `docs/reference.md` rewritten with the updated endpoint table, the scoping contract, the single-user vault access model, and known limitations.

### Fixed

- **`fill` no longer blanks unmatched keys** — When a key in the target `.env` is not found among the resolved environment's linked variables, the original line is now preserved intact and the key is reported as a warning. Previously unmatched keys were rewritten as `KEY=`, silently destroying existing values in plain (non-example) `.env` files.
- **Project name collision race** — Added a case-insensitive `UNIQUE` index on `projects.name` (`idx_projects_name_nocase`). Concurrent CLI auto-create now resolves deterministically: `POST /projects` returns `409 Conflict` on collision, and the loser re-fetches and reuses the winning project instead of failing. When `add` creates a project it re-reads the actual default environment name rather than assuming a literal.
- **Share/relay key hijack** — Imports from LAN share, internet relay, or `.vault` files no longer overwrite an existing variable link when an incoming item's name collides with a key already linked in the target environment. Collisions are skipped and surfaced via `skipped_keys` in `/share/status` and CLI warnings, preventing a peer from silently repointing the receiver's links with crafted item names.
- **Invisible imported items** — Items imported via `/share/import` and `/relay/receive` with a scope provided are now both owned by the project and linked into the environment (matching `add_item` / `POST /items`). Previously they were owned but never linked, making them invisible to scoped `GET /items` and `crypt_env` list queries. MCP `share_import` and `relay_receive` now build scoped query strings when calling those endpoints.

### Known Limitations (Carried Forward)

- Project export/import templates do not carry resolved secret values (by design)
- Backup/restore does not yet include projects/environments/item_projects tables (pre-existing gap, pre-planned)

---

## [0.1.0] - 2026-04-25

### Added

- **Encrypted local vault** — AES-256-GCM encryption for all sensitive fields, Argon2id for master password hashing
- **5 item types** — API Key, Credential (user+pass+URL), Link, Command (with placeholder resolution), Note
- **Desktop UI** — Industrial aesthetic, dark theme, 5 screens (Lock, Main Vault, Add/Edit Item, Category Manager, Settings)
- **Global hotkey** — Press `Ctrl+Alt+Z` from any app to toggle vault
- **Fuzzy search** — Find items instantly by name or content
- **Editable categories** — Organize vault items with user-defined categories
- **Clipboard integration** — Copy secrets in one click, auto-clear after timeout
- **Export formats** — Generate `.env`, bash `export VAR=val`, PowerShell `$env:VAR = "val"` format
- **Auto-lock timeout** — Vault locks automatically after configurable inactivity (5 min default)
- **CLI binary** (`crypt-env`) — Manage vault from terminal: fill `.env`, set env vars, run commands with placeholders, search items
- **Local REST API** — Axum server at `127.0.0.1:47821` for local integrations (locked by default, requires token)
- **MCP server** (`crypt-env-mcp`) — Stdio-based JSON-RPC 2.0 server for AI agents (Claude Code, Claude Desktop)
  - `vault_list_items` — List items by type/category (no secrets exposed)
  - `vault_get_item` — Get item metadata without secret values
  - `vault_generate_env` — Generate `.env` file (values never in response)
  - `vault_inject_env` — Inject secret as environment variable in client process
  - `vault_add_item` — Add new item to vault
  - `vault_update_settings` — Update app settings (not master password)
  - `vault_list_commands` — List available commands with placeholders
  - `vault_run_command` — Resolve command placeholders
- **Windows NSIS installer** — One-click install with PATH registration
- **Zeroized keys** — Encryption key wiped from memory on lock
- **Timing-safe token comparison** — Prevent brute-force attacks on unlock endpoint
- **Strict Content Security Policy** — Tauri webview hardening
- **Import from password managers** — Import secrets from `.env` files, Bitwarden CSV, and 1Password CSV
- **Encrypted backup & restore** — Export and restore full vault with encryption

### Security Notes

- Master password is never persisted — exists only in memory during active session
- MCP server never returns secret values in plain text — injects directly as environment variables
- REST API is localhost-only (`127.0.0.1:47821`) and requires MCP token authentication
- All secret fields encrypted at rest in SQLite database
- Sensitive data structures use `zeroize` to prevent accidental plaintext leaks

### Known Limitations

- Windows focus (macOS and Linux support in progress)
- Single-user per vault file
- No encrypted cloud backup (intentional — local-first design)

[Unreleased]: https://github.com/maosuarez/crypt-env/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/maosuarez/crypt-env/releases/tag/v0.1.0
