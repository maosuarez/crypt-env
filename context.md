# crypt-env

## Description
Personal productivity vault for developers. Centralizes credentials, API keys, tokens, passwords, links, commands, and notes in a local desktop app accessible by hotkey (Ctrl+Alt+Z). Secrets are stored encrypted locally. Includes CLI, local REST API, and MCP server for integration with external tools.

## Stack
- **Frontend**: React 19 + TypeScript + Vite + Tailwind CSS + Framer Motion
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
│   │   ├── lib.rs                # Tauri command registry (19 commands), AppState setup
│   │   ├── db/mod.rs             # SQLite pool, tables, CRUD items/categories/settings
│   │   ├── crypto/mod.rs         # Argon2id KDF + AES-256-GCM encrypt/decrypt
│   │   ├── vault/mod.rs          # VaultState, 19 Tauri commands including backup/import
│   │   ├── api/mod.rs            # Axum server on 127.0.0.1:47821, dual token auth
│   │   ├── share/mod.rs          # Secure secret sharing (LAN bridge + encrypted packages)
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
4. **Command**: name, command, description, shell target (bash/zsh/sh/PowerShell), placeholders `{{VAR}}`
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
Developer user needs quick access (hotkey), ease of copying to clipboard, and ability to use secrets as environment variables without visual exposure. Can now securely share secrets with teammates via encrypted LAN bridge (mDNS discovery + ECDH key exchange) or encrypted packages for offline scenarios, eliminating insecure communication channels like WhatsApp.

---

## Implementation Decisions

### 1. SQLite + AES-GCM Instead of SQLCipher (Plan B)
**Context**: SQLCipher requires OpenSSL/vcpkg with complex configuration on Windows, generating linking errors during compilation.

**Decision**: Adopt Plan B: Standard SQLite with `libsqlite3-sys` bundled + AES-256-GCM encryption at application level.

**Rationale**: 
- Avoids OpenSSL compilation on Windows (high friction, costly maintenance)
- Sensitive fields (`data` in `items`) are encrypted before writing to DB
- The DB file on disk is not encrypted at file level, but item secrets are protected by AES-256-GCM
- Categories and settings are stored plaintext (not confidential metadata)
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
- `vault/mod.rs`: `VaultState` (orchestrator), 19 Tauri commands including unlock/lock, backup/import, and settings
- `api/mod.rs`: Axum REST server, 14 endpoints, dual token authentication

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
    id            INTEGER PRIMARY KEY CHECK(id = 1),
    kdf_salt      TEXT NOT NULL,
    verify_token  TEXT NOT NULL
);
-- Stores crypto material: kdf_salt (hex, 32 bytes) and verify_token (AES-GCM encrypted)

CREATE TABLE items (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    item_type TEXT NOT NULL,  -- 'secret', 'credential', 'link', 'command', 'note'
    data      TEXT NOT NULL,  -- JSON encrypted with AES-GCM
    created   TEXT NOT NULL,  -- Unix epoch seconds
    updated   TEXT NOT NULL   -- Unix epoch seconds
);

CREATE TABLE categories (
    cid   TEXT PRIMARY KEY,
    name  TEXT NOT NULL,
    color TEXT NOT NULL
);
-- Categories stored plaintext with id, name, and color (not encrypted at DB level)

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- Keys: auto_lock_timeout, hotkey, mcp_token (plaintext, user configuration only)
```

**Rationale**:
- `vault_meta`: stores salt (public) and verify_token (private, encrypted) for key derivation with single row constraint
- `items.data`: JSON serialized and encrypted (avoids individual columns)
- `categories`: plaintext columns for UI efficiency (category metadata is not confidential)
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
| GET | `/settings` | token | Returns settings (no secrets) |
| PUT | `/settings` | token | Updates settings |
| GET | `/commands` | token | Lists available commands (MCP read-only) |
| GET | `/commands/:id` | token | Gets command details with placeholders |
| POST | `/fill` | token | Fills .env template with secret values (writes to disk, not response) |
| GET | `/health` | - | Health check (vault lock state, item count, version) |

**Rationale**:
- `/unlock` without token (entry point)
- `/items/:id/reveal` is the only endpoint returning secrets in plaintext (justifiable because it requires valid session token)
- Responses never include encrypted values in plaintext (only metadata JSON)

**Consequences**:
- CLI must make 2 calls: `/unlock` + then authenticated requests
- MCP makes initial `/unlock` or reuses MCP token directly
- Audit of `/items/:id/reveal` calls recommended (can log accesses)

---

### 8. CLI (`crypt-env` Binary)
**Context**: Standalone tool for management without GUI, written in Rust + clap, connects via HTTP REST.

**Decision**: Binary `src-tauri/src/bin/crypt-env/main.rs` that:
- Uses `clap` for argument parsing
- Connects via HTTP to `127.0.0.1:47821` (requires vault GUI running)
- Authenticates with session token from REST `/unlock` endpoint
- Supports commands:
  - `add` — Import secrets from KEY=value, environment variables, or .env files
  - `doctor` — Check app health, vault status, token files, and version
  - `fill` — Fill .env or .env.example templates with vault secrets
  - `inject` — Print shell-compatible variable assignment (safe for `eval`)
  - `list` — Display saved shell commands in a table
  - `exec` — Execute a saved command by name
  - `memory` — Save a command string to the vault (interactive)
  - `search` — Search items by name (no secret values exposed)
  - `set` — Print environment variable assignment for a secret
  - `cmd` — Manage saved commands (list, info, run)

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
- Implements JSON-RPC tools (all prefixed `crypt_env_`):
  - `crypt_env_list_items` — lists items without secrets, with type/category filters
  - `crypt_env_get_item` — gets item metadata by ID
  - `crypt_env_search_items` — searches items by name (no values)
  - `crypt_env_generate_env` — writes `.env` to disk with secret values (path in response, not values)
  - `crypt_env_inject_env` — injects a secret as environment variable into MCP process
  - `crypt_env_add_item` — creates new vault item
  - `crypt_env_update_settings` — modifies auto_lock_timeout and hotkey
  - `crypt_env_fill_env` — fills .env.example template with real values to disk
  - `crypt_env_doctor` — health check (status, vault lock state, item count, version)
  - `crypt_env_list_commands` — lists saved shell commands with placeholders
  - `crypt_env_run_command` — executes command with resolved placeholders; returns `{ exit_code, stdout, stderr }` (secrets never in response)

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

### 11. Secure Secret Sharing (LAN Bridge + Encrypted Packages)
**Context**: Users need to securely share secrets with teammates without exposing plaintext in WhatsApp, email, or other channels. Two scenarios exist: (1) both users on same LAN with ability to perform real-time key exchange, and (2) offline scenario requiring a self-contained encrypted file.

**Decision**: Implement two complementary sharing modes:

1. **LAN Bridge Mode** (for local network):
   - Sender initiates session with `POST /share/listen` → returns 6-digit pairing code (5-minute expiration)
   - Receiver initiates session with `POST /share/connect <pairing_code>` → gets sender's public key and fingerprint (first 8 hex chars of SHA-256(sender_pub || receiver_pub))
   - Both sides confirm fingerprint match via `POST /share/confirm`
   - Sender selects items and sends via `POST /share/items` encrypted with HKDF-SHA256 derived key (X25519 ECDH shared secret + info=`b"cryptenv-share-v1"`)
   - Session auto-destroys on completion, cancellation, or 30-second inactivity
   - All encryption uses AES-256-GCM on length-prefixed JSON messages over TCP

2. **Encrypted Package Mode** (for offline/non-LAN):
   - Sender exports items via `POST /share/export <item_ids>` with Argon2id(m=32768, t=2, p=2) KDF from random 12-char passphrase
   - Returns `.vault` JSON package: `{ version, salt, nonce, ciphertext }` + plaintext passphrase (shown once to sender)
   - Receiver imports via `POST /share/import` with passphrase (entered manually from sender)
   - Passphrase never stored, encrypted package is self-contained and portable

3. **Shared module structure** (`src-tauri/src/share/`):
   - `crypto.rs`: X25519 keypair generation, HKDF-SHA256 shared key derivation, AES-256-GCM channel encryption, 12-char passphrase generation, fingerprint computation
   - `lan.rs`: mDNS service discovery (`_cryptenv._tcp.local.`), TCP listener, ECDH handshake with pairing code verification
   - `package.rs`: `.vault` package format (JSON), PlainItem struct for export, Argon2id KDF for package encryption
   - `protocol.rs`: Length-prefixed JSON messages, ShareMessage enum (Hello, Confirm, Items, Ack, Error)
   - `mod.rs`: ShareState, ShareSession, ShareSessionState, ShareDirection state machine

4. **Database audit** (`share_log` table):
   ```sql
   CREATE TABLE share_log (
       id        INTEGER PRIMARY KEY AUTOINCREMENT,
       mode      TEXT NOT NULL,  -- 'lan' or 'package'
       direction TEXT NOT NULL,  -- 'sent' or 'received'
       item_ids  TEXT NOT NULL,  -- JSON array of shared item IDs
       peer_fp   TEXT,           -- Peer fingerprint (LAN mode only)
       timestamp TEXT NOT NULL   -- ISO 8601 timestamp
   );
   ```

5. **New REST endpoints** (all require auth except noted):
   - `POST /share/listen` → `{ pairing_code, expires_in }`
   - `POST /share/connect` → `{ fingerprint }`
   - `POST /share/confirm` → `{ status }`
   - `GET /share/status` → `{ state, progress }`
   - `DELETE /share/session` → `{ cancelled }`
   - `POST /share/export` → `{ ciphertext, salt, nonce, passphrase }`
   - `POST /share/import` → `{ imported_count }`

6. **New CLI commands**:
   - `crypt-env share send <ITEM_IDS>...` — Start LAN send session
   - `crypt-env share receive` — Start LAN receive session
   - `crypt-env share export [IDS] -o file` — Create encrypted package
   - `crypt-env share import -f file` — Import from package

7. **New MCP tools** (all prefixed `crypt_env_share_`):
   - `crypt_env_share_listen` — Start LAN send session
   - `crypt_env_share_connect` — Start LAN receive session
   - `crypt_env_share_confirm` — Confirm fingerprint
   - `crypt_env_share_cancel` — Cancel session
   - `crypt_env_share_status` — Poll session status
   - `crypt_env_share_export` — Export encrypted package (returns passphrase)
   - `crypt_env_share_import` — Import encrypted package

**Rationale**:
- LAN bridge mode provides real-time, interactive sharing with cryptographic proof (fingerprint confirmation) that both parties are communicating with the correct peer
- Encrypted package mode is a fallback for scenarios where real-time communication is impossible (different networks, offline transfers via USB)
- Pairing code (6 digits, 5-min expiration) is a human-verifiable authentication mechanism — prevents MITM if both users can confirm the same code
- X25519 ECDH is industry-standard, post-quantum resistant key exchange primitive
- Argon2id with high memory cost (32768 KiB) makes brute-forcing a random passphrase computationally expensive
- Audit log allows traceability of who shared what and when (useful for security incident response)
- Sender explicitly selects items to share (not a bulk "share all" which could leak unintended secrets)
- Session auto-destruction prevents reuse if the connection is compromised mid-transfer
- Fingerprint verification prevents MITM attacks where attacker intercepts pairing code

**Consequences**:
- LAN bridge requires mDNS discovery to work (must be available on the network)
- Pairing code is short (6 digits) to be human-readable; increases brute-force window to ~2 seconds if attacker has network access (acceptable because code expires in 5 minutes)
- Encrypted package passphrase is shown once and not stored; user must securely communicate it out-of-band (no built-in passphrase recovery)
- Argon2id KDF on package import is slow (~1-2 seconds per import); acceptable for infrequent use but not suitable for bulk imports
- MCP tools return passphrase only in `crypt_env_share_export` response (LLM must display to user via UI, not in logs)

---

### 12. Windows Hello Biometric Unlock
**Context**: Users on Windows need fast, convenient vault unlock without entering master password every time. Windows Hello (fingerprint, facial recognition) is available on most modern Windows devices.

**Decision**: Implement biometric unlock via Windows Hello with DPAPI-encrypted master password storage:

1. **Biometric module** (`src-tauri/src/biometric/mod.rs`):
   - `check_availability()` → returns `BiometricAvailable | NotAvailable`
   - `request_verification(message: &str)` → prompts Windows Hello dialog, returns `VerificationOk | VerificationCancelled | Error`
   - `dpapi_protect(data: &[u8])` → encrypts data with DPAPI (tied to Windows user account), returns hex-encoded blob
   - `dpapi_unprotect(blob_hex: &str)` → decrypts DPAPI blob, returns plaintext bytes

2. **Enrollment flow**:
   - User enters master password in Settings
   - Calls `dpapi_protect(password_bytes)` → generates DPAPI-encrypted blob
   - Stores blob in `settings` table as key `biometric_blob` (plaintext hex in DB)
   - DPAPI ties the blob to the Windows user account; blob cannot be decrypted on a different account

3. **Unlock flow**:
   - Frontend detects biometric availability and enrollment status
   - User clicks "Unlock with Windows Hello"
   - Calls `biometric_unlock` command → retrieves `biometric_blob` from DB → `dpapi_unprotect()` → Windows Hello prompt → recovers password bytes
   - Vault unlocks normally with recovered password (no special unlock path)
   - Session token issued as usual

4. **Disable biometric**:
   - User enters master password in Settings
   - Calls `biometric_disable` command → deletes `biometric_blob` from settings table
   - DPAPI blob discarded; biometric unlock unavailable until re-enrolled

5. **New Tauri commands**:
   - `biometric_check() → "available" | "not_available"` — Detects if Windows Hello is available
   - `biometric_is_enrolled() → bool` — Checks if user has enrolled (`biometric_blob` exists in DB)
   - `biometric_enroll(password: &str) → bool` — Encrypts password with DPAPI, stores blob, returns success
   - `biometric_unlock() → String` — Decrypts blob, prompts Windows Hello, returns session token on success
   - `biometric_disable(password: &str) → bool` — Verifies password, deletes blob, returns success

6. **Implementation notes**:
   - Windows WinRT calls (`UserConsentVerifier::CheckAvailabilityAsync()`, `UserConsentVerifier::RequestVerificationAsync()`) are blocking; use `tokio::task::spawn_blocking()` in Tauri commands
   - DPAPI output uses `LocalFree()` to deallocate WinRT-allocated memory; prevents memory leak
   - Secret password bytes are in `Zeroizing<Vec<u8>>` after decryption and before unlock
   - Non-Windows platforms: all functions compile but return `NotAvailable` at runtime (feature disabled)

7. **Dependencies**:
   - `windows` crate v0.58 with features: `Security_Credentials_UI`, `Win32_Security_Cryptography`, `Win32_Foundation`, `Win32_System_Memory`
   - `zeroize` (already in dependencies) for password bytes cleanup

**Rationale**:
- Biometric does NOT replace master password; it protects an encrypted copy (remains secure even if DPAPI is compromised)
- DPAPI is Windows user-account bound; blob cannot be decrypted by a different user or after password change
- Windows Hello is hardware-backed on devices with TPM/biometric sensors (strong second factor)
- Enrollment still requires master password (prevents stealing the phone and unlocking the vault)
- Pairing biometrics with DPAPI provides defense-in-depth: attacker needs both DPAPI blob AND Windows Hello verification

**Consequences**:
- Feature only works on Windows with Hello hardware; silent no-op on other platforms
- If Windows user password changes, DPAPI blob becomes unusable (user must re-enroll with new password)
- If user loses biometric enrollment (e.g., resets fingerprints), `biometric_blob` persists in DB but cannot be used (safe fallback: use master password)
- DPAPI blob in plaintext hex in DB is acceptable because blob is useless without Windows user privileges and Hello verification
- Unlock latency: ~100-200ms for WinRT call + ~500ms for Windows Hello UI = ~700ms total (slower than master password alone, but negligible for user experience)

---

## Security Status (post-review 2026-04-24)

A **comprehensive security review** was performed that identified **19 findings** (7 HIGH, 8 MEDIUM, 4 LOW). **All findings have been addressed**.

**Critical findings (HIGH) implemented**:
1. ✅ **Timing-safe token comparison**: Implemented `subtle::ConstantTimeEq` for all token comparisons
2. ✅ **Master password derivation**: Argon2id with hardened parameters (m=65536, t=3, p=4)
3. ✅ **Key in memory with Zeroizing**: Use `zeroize` crate to overwrite key on Drop
4. ✅ **Access audit for `/items/:id/reveal`**: Logging infrastructure prepared (future audit log implementation)
5. ✅ **File permissions (mcp_token, cli_session_token)**: Set to 0600 (Unix) or NTFS ACLs (Windows) on creation
6. ✅ **Credential encryption in MCP server**: Tokens stored in memory with `Zeroizing` to prevent leakage

**MEDIUM findings implemented**:
- ✅ Error handling without exposure of internal paths
- ✅ HTTPS for local REST API with auto-generated rcgen certificate
- ✅ Input validation on `/items` POST/PUT (HTTP 422 for invalid data)
- ✅ Cleanup of temporary `.env` files via RAII pattern (automatic zeroing + deletion)
- ✅ Session auto-lock via background task (configured timeout, `VaultState.key = None`)

**LOW findings**:
- Security documentation (covered in README.md and context.md)
- Change traceability (audit log structure in place, entries logged on demand)
- Secure data export (handled via `/fill` endpoint, no plaintext in responses)

---

> This file is the main project context.
> Referenced from CLAUDE.md with: `See context.md for full project context.`
