# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

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

### Security Notes

- Master password is never persisted — exists only in memory during active session
- MCP server never returns secret values in plain text — injects directly as environment variables
- REST API is localhost-only (`127.0.0.1:47821`) and requires MCP token authentication
- All secret fields encrypted at rest in SQLite database
- Sensitive data structures use `zeroize` to prevent accidental plaintext leaks

### Known Limitations

- Windows only (macOS and Linux support planned)
- Single-user per vault file
- No encrypted cloud backup (intentional — local-first design)
- No import from password managers yet

[Unreleased]: https://github.com/maosuarez/crypt-env/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/maosuarez/crypt-env/releases/tag/v0.1.0
