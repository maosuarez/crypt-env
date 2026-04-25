# crypt-env

## Description
Personal productivity vault for developers. Centralizes credentials, API keys, tokens, passwords, links, commands, and notes in a local desktop app accessible by hotkey (Ctrl+Alt+Z). Secrets are stored encrypted locally. Includes CLI, local REST API, and MCP server for integration with external tools.

## Stack
- **Frontend**: React 18 + TypeScript + Vite + Tailwind CSS + Framer Motion
- **Backend (Rust)**: Tauri 2.0, Axum (local REST), Tokio
- **Database**: SQLite with `libsqlite3-sys` bundled (no SQLCipher; see Decision #1)
- **Encryption**: AES-256-GCM for sensitive fields, Argon2id for master password, `subtle::ConstantTimeEq` for timing-safe comparisons
- **CLI**: Terminal interface for item management without opening the GUI (binary `crypt-env`)
- **MCP**: Model Context Protocol server for secure secret queries (binary `crypt-env-mcp`)
- **REST API**: Axum on `127.0.0.1:47821` with dual authentication (session token + MCP token)
- **Target OS**: Windows (development), multi-platform in the future
- **Package manager**: pnpm

## Architecture
```
crypt-env/
├── src/                          # React frontend
│   ├── components/               # UI components by screen
│   ├── store/                    # Global state with Zustand
│   ├── hooks/                    # Custom hooks for Tauri invoke()
│   └── types/                    # Shared TypeScript types
├── src-tauri/
│   ├── src/
│   │   ├── main.rs               # Tauri entrypoint (initializes lib)
│   │   ├── lib.rs                # Tauri command registry (10+), AppState setup
│   │   ├── db/mod.rs             # SQLite pool, tables, CRUD items/categories/settings
│   │   ├── crypto/mod.rs         # Argon2id KDF + AES-256-GCM encrypt/decrypt
│   │   ├── vault/mod.rs          # VaultState, 10+ Tauri commands, change_password, wipe
│   │   ├── api/mod.rs            # Axum server on 127.0.0.1:47821, dual token auth
│   │   ├── cli/mod.rs            # CLI module (stub)
│   │   ├── mcp/mod.rs            # MCP module (stub)
│   │   └── bin/
│   │       ├── crypt-env.rs      # CLI standalone (clap), connects via HTTP to API
│   │       └── crypt-env-mcp.rs  # MCP JSON-RPC 2.0 server over stdio
│   ├── Cargo.toml                # Rust dependencies
│   └── tauri.conf.json           # Window config, permissions, hotkey
```

**Communication**:
- Frontend → Tauri `invoke()` → registered Rust commands
- CLI (`crypt-env`) → HTTP REST to `127.0.0.1:47821` with session/MCP token
- MCP (`crypt-env-mcp`) → HTTP REST to `127.0.0.1:47821` with MCP token

## Vault Item Types
1. **Secret / API Key**: name, encrypted value, category, notes. Export as `.env` / `export` / `$env:`
2. **Credential**: site name, URL, username, encrypted password, notes
3. **Link**: title, URL, description, category
4. **Command**: name, command, description, shell target (bash/zsh/PowerShell/cmd), placeholders `{{VAR}}`
5. **Note**: title, free-form content, category

## Security — Decisions Made
- **Master password** derived with Argon2id (m=65536, t=3, p=4), never stored in plaintext
- **Sensitive values** encrypted with AES-256-GCM before writing to SQLite
- **Timing-safe comparisons** using `subtle::ConstantTimeEq` for tokens and verify_token
- **Key in memory** stored in `Zeroizing<[u8;32]>` which automatically overwrites on Drop
- **MCP does not expose values directly**: injects secrets as environment variables in the client process, without returning them as text
- **Local REST API** listens only on `127.0.0.1:47821`, never on external interfaces
- **Dual authentication**: session tokens (with expiration) + MCP token (static, stored in `%APPDATA%`)
- **Window auto-locks** after configurable timeout, setting `VaultState.key = None`

## Conventions
- Code language: English (variables, functions, comments)
- Agent response language: English
- Rust naming: snake_case. React/TS naming: camelCase, PascalCase for components
- Tauri commands (`invoke`) are named with module prefix: `vault_get_items`, `crypto_unlock`, etc.
- Do not use `unwrap()` in production — handle errors with `Result` and custom error types
- Tailwind for styles, no CSS modules or styled-components

## Constraints
- Dependencies in `src-tauri/Cargo.toml` are **pending**: configured in Claude Code's first session
- Do not implement cloud synchronization in this version
- Do not assume SQLCipher compiles frictionlessly on Windows — have Plan B (AES-GCM over standard SQLite)
- The window is **decorationless** (no OS titlebar), with custom titlebar in React
- Do not keep master password in memory longer than necessary to unlock
- MCP is read-only — does not allow creating or modifying items

## Business Context
Developer user currently sharing credentials insecurely over WhatsApp. Needs quick access (hotkey), ease of copying to clipboard, and ability to use secrets as environment variables without visual exposure. Strictly personal and local usage.

---

## Implementation Decisions

### 1. SQLite + AES-GCM Instead of SQLCipher (Plan B)
**Context**: SQLCipher requires OpenSSL/vcpkg with complex configuration on Windows, generating linking errors during compilation.

**Decision**: Adopt Plan B: Standard SQLite with `libsqlite3-sys` bundled + AES-256-GCM encryption at application level.

**Rationale**: 
- Avoids OpenSSL compilation on Windows (high friction, costly maintenance)
- All sensitive fields (`data` in `items`, `data` in `categories`) are encrypted before writing to DB
- The DB file on disk is not encrypted at file level, but the most sensitive data is protected by AES-256-GCM
- Allows future integration with larger-scale databases

**Consequences**:
- If the `vault.db` file is accessed directly without running the application, data remains encrypted at field level
- Assumes physical control of the machine (local Windows, single user) — not a defense against direct memory attacks
- The AES key derived only exists in memory during the active session

---

### 2. Decoupled Rust Module Structure
**Context**: Need to separate responsibilities between crypto, persistence, API, and business logic.

**Decision**: 
- `crypto/mod.rs`: Argon2id KDF, AES-256-GCM encrypt/decrypt, key management in `Zeroizing`
- `db/mod.rs`: SQLite pool, DDL of tables, CRUD of items/categories/settings (does not know about `api`, `vault`)
- `vault/mod.rs`: `VaultState` (orchestrator), 10+ Tauri commands, unlock/lock operations
- `api/mod.rs`: Axum REST server, endpoints, token authentication

**Rationale**: Each module has a clear responsibility. `vault` orchestrates between `crypto` and `db` without them knowing each other.

**Consequences**: 
- The REST API (`api/mod.rs`) also uses the same underlying modules
- CLI and MCP communicate with the backend via HTTP REST; no direct Rust linkage

---

### 3. MCP Token Storage in File
**Context**: MCP server needs token to authenticate calls to the REST API; requires persistence between sessions (no expiration).

**Decision**: 
- MCP Token: 32 bytes randomly generated with `rand::thread_rng()`, saved in `vault_meta.mcp_token` (DB)
- Redundant copy in `%APPDATA%\com.maosuarez.cryptenv\mcp_token` (plaintext file)
- Generated only once with `vault_generate_mcp_token` when MCP is started for the first time
- No expiration, valid while the vault is unlocked

**Rationale**: 
- Allows MCP server to read its token without need to unlock interactively
- File in `%APPDATA%` avoids having to read from DB each time
- Token verification in REST API uses `subtle::ConstantTimeEq`

**Consequences**:
- The `mcp_token` file in `%APPDATA%` needs restrictive permissions (ideally 0600, on Windows: owner only)
- If that file is compromised, anyone can make calls to MCP

---

### 4. Database Schema (SQLite in `%APPDATA%`)
**Context**: Need to store encrypted items, categories, crypto metadata, and settings.

**Decision**: 4 tables in `vault.db` located at `%APPDATA%\com.maosuarez.cryptenv\vault.db`:

```sql
CREATE TABLE vault_meta (
    id    INTEGER PRIMARY KEY,
    key   TEXT NOT NULL UNIQUE,
    value TEXT NOT NULL
);
-- Contains: kdf_salt (hex, 32 bytes), verify_token (hex, encrypted with AES-GCM)

CREATE TABLE items (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    item_type  TEXT NOT NULL,  -- 'secret', 'credential', 'link', 'command', 'note'
    data       TEXT NOT NULL,  -- JSON encrypted with AES-GCM
    created_at TEXT NOT NULL,  -- ISO 8601
    updated_at TEXT NOT NULL   -- ISO 8601
);

CREATE TABLE categories (
    id    INTEGER PRIMARY KEY AUTOINCREMENT,
    data  TEXT NOT NULL  -- JSON array encrypted
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- Keys: auto_lock_minutes, hotkey, mcp_token
```

**Rationale**:
- `vault_meta`: stores salt (public) and verify_token (private, encrypted) for key derivation
- `items.data`: JSON serialized and encrypted (avoids individual columns)
- `categories.data`: single JSON array, encrypted (simple CRUD)
- `settings`: plaintext (contains no secrets, only user configuration)

**Consequences**:
- The `items` table grows indefinitely; indexing by `id` and `item_type` recommended for future searches
- Encrypted JSON requires deserialization post-decryption in the application

---

### 5. Unlock Flow and Key Management in Session
**Context**: The AES key must only exist in memory during the active session; must be destroyed when locking.

**Decision**:

1. **First initialization** (`init_vault_crypto`):
   - Generates 32-byte `salt` with `rand::thread_rng()`
   - Derives AES key with Argon2id(m=65536, t=3, p=4) from password + salt
   - Encrypts `b"vault_ok_v1"` as `verify_token` with AES-256-GCM
   - Saves `salt` and `verify_token` in `vault_meta`
   - Saves key in `VaultState.key` as `Zeroizing<[u8;32]>`

2. **Unlock** (`unlock_vault_crypto`):
   - Reads `salt` and `verify_token` from `vault_meta`
   - Re-derives key with Argon2id
   - Attempts to decrypt `verify_token` → if OK, password is correct
   - Saves key in `VaultState.key`
   - Generates session token (32 bytes hex)
   - Returns token to client

3. **Lock**:
   - Sets `VaultState.key = None`
   - The `Zeroizing` automatically overwrites the 32 bytes on Drop

**Rationale**:
- `Zeroizing` is mandatory to prevent the key from persisting on heap between sessions
- Argon2id with high parameters (m=65536) makes brute-force very costly
- `verify_token` allows detecting incorrect password without decrypting all items

**Consequences**:
- Unlock time is ~200-500ms (by design, Argon2id is slow)
- If the process is abruptly killed, the key may not be overwritten (defense against DMA attacks is not possible in Windows user-mode)

---

### 6. REST Authentication: Session Token vs MCP Token
**Context**: REST API must authenticate requests; session tokens expire, MCP token is persistent.

**Decision**:

- **Session token**: 32 bytes hex, generated on `/unlock`, valid for `auto_lock_minutes` (Instant + Duration on server)
  - Header: `X-Vault-Token: <hex32>`
  - Expires automatically
  - Used by CLI and frontend (via Tauri `invoke`)

- **MCP token**: 32 bytes hex, generated once, no expiration
  - Header: `X-Vault-Token: <hex32>` (same header)
  - Constant-time verification with `subtle::ConstantTimeEq`
  - Used only by MCP server
  - Allows MCP to function without explicit unlock interface

**Rationale**: 
- Two separate channels: session (ephemeral, UI) vs MCP (persistent, backend)
- MCP can function without GUI interface
- Expiration prevents reuse of exfiltrated tokens

**Consequences**:
- Server must maintain `HashMap<String, Instant>` of active tokens
- Periodic cleanup of expired tokens recommended

---

### 7. Implemented REST Endpoints
**Context**: REST API on `127.0.0.1:47821` as unified interface for CLI, MCP, and Tauri.

**Decision**: Implement RESTful endpoints with dual authentication:

| Method | Route | Auth | Description |
|--------|-------|------|-------------|
| POST | `/unlock` | - | Validates password, returns session token |
| GET | `/items` | token | Lists items (no sensitive fields) |
| POST | `/items` | token | Creates new item |
| GET | `/items/:id` | token | Gets item (no encrypted values) |
| PUT | `/items/:id` | token | Updates item |
| DELETE | `/items/:id` | token | Deletes item |
| POST | `/items/:id/reveal` | token | Decrypts and returns sensitive value (only endpoint that does this) |
| GET | `/categories` | token | Lists categories |
| POST | `/categories` | token | Creates/updates categories |
| GET | `/settings` | token | Returns settings (no secrets) |
| PUT | `/settings` | token | Updates settings |
| GET | `/commands` | token | Lists available commands (MCP read-only) |

**Rationale**:
- `/unlock` without token (entry point)
- `/items/:id/reveal` is the only endpoint returning secrets in plaintext (justifiable because it requires valid session token)
- Responses never include encrypted values in plaintext (only metadata JSON)

**Consequences**:
- CLI must make 2 calls: `/unlock` + then authenticated requests
- MCP makes initial `/unlock` or reuses MCP token directly
- Audit of `/items/:id/reveal` calls recommended (can log accesses)

---

### 8. CLI (`vault` Binary)
**Context**: Standalone tool for management without GUI, written in Rust + clap, connects via HTTP REST.

**Decision**: Binary `src-tauri/src/bin/crypt-env.rs` that:
- Uses `clap` for argument parsing
- Connects via HTTP to `127.0.0.1:47821` (if vault GUI is running) or starts API server locally
- Stores session token in `%APPDATA%\com.maosuarez.cryptenv\cli_session_token` (with expiration)
- Supports commands:
  - `vault unlock` — requests master password, saves token
  - `vault lock` — invalidates session
  - `vault list [--type TYPE]` — lists items without secrets
  - `vault get <name>` — shows metadata
  - `vault set <name>` — copies value to clipboard (requires `/reveal`)
  - `vault fill <name>` — exports as `export VAR=value` (for `eval`)
  - `vault cmd <name>` — executes saved command
  - `vault add` — interactive assistant

**Rationale**: Decoupled CLI from REST server allows independent control; token storage avoids re-authentication.

**Consequences**:
- Token in `cli_session_token` file needs restrictive permissions (0600)
- If API server is inactive, CLI should be able to start it (possible future feature)

---

### 9. MCP Server (`crypt-env-mcp` Binary)
**Context**: Model Context Protocol server for AI agent integration, communication via JSON-RPC 2.0 over stdio.

**Decision**: Binary `src-tauri/src/bin/crypt-env-mcp.rs` that:
- Reads MCP token from `%APPDATA%\com.maosuarez.cryptenv\mcp_token`
- Connects via HTTP REST to `127.0.0.1:47821`
- Implements JSON-RPC tools:
  - `vault_list_items` — lists without secrets
  - `vault_get_item` — gets by ID/name
  - `vault_generate_env` — writes `.env` to `%TEMP%` (RAII pending)
  - `vault_inject_env` — set_var in current process (validation `[A-Z0-9_]+`)
  - `vault_add_item` — creates item
  - `vault_update_settings` — modifies settings
  - `vault_list_commands` — lists commands
  - `vault_run_command` — executes shell command

**Rationale**:
- Standard MCP protocol allows integration with any compatible client
- Does not return secrets in plaintext, only injects as environment variables
- Persistent MCP token allows functioning without explicit unlock interface

**Consequences**:
- If `mcp_token` is compromised, MCP can be accessed remotely (if listening on network, outside current scope)
- `vault_inject_env` requires strict name validation (prevent injection)

---

### 10. File Location on Windows
**Context**: Need to store DB, tokens, configuration persistently and securely.

**Decision**: Use `%APPDATA%\com.maosuarez.cryptenv\` as base directory:

```
%APPDATA%\com.maosuarez.cryptenv\
├── vault.db                    # SQLite DB (AES-GCM encryption at field level)
├── mcp_token                   # MCP token (plaintext, permissions 0600)
├── cli_session_token           # CLI session token (plaintext, permissions 0600)
└── logs/                        # (future) Access audit
```

**Rationale**: 
- `%APPDATA%` is standard for user data on Windows (roameable on domain)
- Subdirectory `com.maosuarez.cryptenv` prevents conflicts with other applications
- Token in file rather than memory-only facilitates access by CLI/MCP without GUI server

**Consequences**:
- If user account is compromised, tokens are also compromised
- Encryption at OS level (NTFS EFS) optional but not implemented

---

## Security Status (post-review 2026-04-24)

A **comprehensive security review** was performed that identified **19 findings** (7 HIGH, 8 MEDIUM, 4 LOW). 

**Critical findings (HIGH) implemented**:
1. ✅ **Timing-safe token comparison**: Implemented `subtle::ConstantTimeEq` for all token comparisons
2. ✅ **Master password derivation**: Argon2id with hardened parameters (m=65536, t=3, p=4)
3. ✅ **Key in memory with Zeroizing**: Use `zeroize` crate to overwrite key on Drop

**Critical findings (HIGH) pending implementation**:
4. ⏳ **Rate limiting on `/unlock`**: Prevent brute-force of master password
5. ⏳ **Access audit for `/items/:id/reveal`**: Log who accesses which secrets and when
6. ⏳ **File permissions (mcp_token, cli_session_token)**: Configure 0600 on creation
7. ⏳ **Credential encryption in MCP server**: Keep tokens in memory with Zeroizing

**MEDIUM findings implemented**:
- ✅ Error handling without exposure of internal paths

**MEDIUM findings pending**:
- ⏳ HTTPS for local REST API (mkcert)
- ⏳ Input validation on `/items` POST/PUT
- ⏳ Cleanup of temporary `.env` generated by `vault_generate_env` (RAII)
- ⏳ Session timeout not yet implemented (structure only)

**LOW findings**:
- Security documentation
- Change traceability (audit log)
- Secure data export

---

> This file is the main project context.
> Referenced from CLAUDE.md with: `See context.md for full project context.`
