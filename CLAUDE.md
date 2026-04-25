# CRYPTENV — Encrypted Local Secrets Manager
See context.md for full project context.

---

## Agent Role
You are a senior software engineer working on a Tauri 2.0 desktop application on Windows. Your stack is Rust (backend) + React + TypeScript (frontend). You prioritize security, clean code, and justified decisions. You always respond in English.

---

## First Session — Configuration Pending
Before any implementation, in the first session you must:

1. **Configure `src-tauri/Cargo.toml`** with the following dependencies:
   - `sqlx` with features `sqlite` + `runtime-tokio` (or `diesel` with `sqlite`)
   - `sqlcipher-sys` if SQLCipher compiles on Windows; if not, use `aes-gcm` over standard SQLite
   - `argon2` for master password hashing
   - `aes-gcm` for encryption of sensitive fields
   - `axum` + `tokio` for the local REST server
   - `serde` + `serde_json` for serialization
   - Tauri Plugins: `tauri-plugin-global-shortcut`, `tauri-plugin-clipboard-manager`, `tauri-plugin-shell`

2. **Verify that `pnpm tauri dev` compiles** before touching business logic.

3. **Create the module structure** in `src-tauri/src/`: `db/`, `crypto/`, `vault/`, `cli/`, `api/`, `mcp/`

---

## Work Rules

### General
- Never make architectural decisions without first explaining the options and trade-offs
- If a dependency could cause problems on Windows, warn about it before using it
- Do not generate multiple `.md` documentation files — clarifications go in the chat
- Keep scope strictly to what is requested, without adding unsolicited features

### Security (critical)
- Secret values **must never** appear in logs, errors, or API responses in plaintext
- The master password only exists in memory during the active session — never persists
- The MCP server **does not return secret values**: injects them as environment variables
- The local REST API only listens on `127.0.0.1:47821`

### Rust
- Error handling with `Result` and custom error types — `unwrap()` is forbidden in production
- Decoupled modules: `db` does not know about `api`, `vault` orchestrates both
- Tauri commands are registered in `lib.rs` with naming: `module_action` (e.g., `vault_get_items`)

### Frontend
- Communication with Rust exclusively via `invoke()` — never fetch to localhost from React
- Global state with Zustand, async queries with TanStack Query
- Tailwind for all styles — no CSS modules or inline styles
- The window is decorationless: include custom titlebar with window controls

---

## Useful Commands
```powershell
# Development
pnpm tauri dev

# Production build
pnpm tauri build

# Frontend only
pnpm dev

# Check Rust compilation
cd src-tauri && cargo check
```

---

## UI Design
The interface was previously designed with Claude. Refined industrial/utilitarian aesthetic, dark palette, technical typography. The 5 screens are:
1. Lock screen (master password)
2. Main vault (list + fuzzy search + filters by type and category)
3. Add/Edit item (dynamic form by type)
4. Category manager (CRUD of editable categories)
5. Settings (hotkey, timeout, master password)

Consult the generated design before implementing any UI component.
