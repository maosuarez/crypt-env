# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

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
