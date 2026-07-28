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
Industrial/utilitarian aesthetic with dark palette and technical typography. Navigation is footer-based (text links over icon rail per user preference). The main screens are:
1. **Lock Screen** — Master password entry + biometric unlock option
2. **Projects & Environments** — Primary landing page (project list → project detail with environments → environment editor for variable linking)
3. **Global Secrets** — Filtered view of reusable `isGlobal` items (footer link back to projects)
4. **Add/Edit Item** — Dynamic form by item type (secret, credential, link, command, note)
5. **Category Manager** — CRUD of editable categories
6. **Settings** — Master password, timeout, biometric, projects/environments management, internet relay config, backup/restore, import

Footer navigation links users between Projects, Global Secrets, Categories, and Settings. Decorationless window with custom React titlebar and window controls.

---

## Recent Features (Session 4+)

### 1. Projects & Environments
Hierarchical organization replacing flat Workspaces. Projects contain multiple typed Environments; every environment variable is now a real encrypted vault item.

**Core Architecture**:
- `src-tauri/src/project/mod.rs` — Project/environment business logic (shared by Tauri commands + HTTP API)
- `src-tauri/src/db/mod.rs` — Tables: `projects`, `environments`, `environment_vars`, `item_projects` (many-to-many), `project_categories`
- Database: `items.is_global` plaintext column tracks reusability across projects; `item_projects` tracks ownership

**Data Model**:
- **Project**: id, name, description, template, environments[], categories[] (tags via category names)
- **Environment**: id, project_id, name, is_default, paths[], vars[]
- **EnvironmentVar**: id, key, item_id (mandatory — no more literal key=value)
- **Item Ownership**: Every item either global (`is_global=true`) or owned by one or more projects. Deleting a project cascades to orphaned items; un-globaling multi-owner items forks them into independent copies.

**Tauri Commands**:
- `project_list() → Vec<Project>` — All projects with nested environments
- `project_save(project: ProjectInput) → Project` — Create or update
- `project_delete(id: i64)` — Delete with cascade logic
- `project_preview_delete(id: i64) → ProjectDeleteImpact` — Show impact before delete
- `environment_save(env: EnvironmentInput)` — Create or update environment
- `environment_delete(id: i64)` — Delete environment
- `environment_inject(id: i64) → InjectResult` — Write decrypted vars to configured paths
- `vault_create_project_item(project_id, key, item)` — Add variable to environment
- `vault_set_item_global(id, is_global) → GlobalToggleResult` — Make item global or fork if multi-owned
- `vault_get_item_owners(id) → Vec<ItemOwner>` — List projects that own this item

**Frontend**:
- New `ProjectManager.tsx` screen: project list → project detail with environment cards → environment editor with variable linking
- `GlobalSecrets.tsx` (renamed from MainVault.tsx): filtered to `isGlobal` items only, reachable via footer link
- New `src/components/itemFields/` module: extracted `ItemTypePicker`, `ItemTypeFields`, `emptyItemFields`, `validateItemFields` for shared per-type field logic
- Project categories reuse existing categories/TagInput; projects carry tags via `project_categories` join table

**Migration & Backward Compatibility**:
- One-time migration `vault::migrate_literal_vars_to_items()` (gated by `settings['migrated_literals_v1']`) runs on unlock: converts legacy literal-only environment vars to real encrypted vault items owned by their environment's project
- Pre-existing items with zero owners after backfill are promoted to global (surface in Global Secrets instead of disappearing)
- CLI `crypt-env project` replaces deleted `workspace` subcommand

**Example**: Create a "MyApp" project with "production" and "local" environments. Add DB_HOST, DB_PASSWORD as vault items. Link them to production environment. Click "Inject" to write to `.env.production`. Un-global DB_PASSWORD to make it project-specific.

---

### 2. Interactive TUI (`crypt-env tui`)
Terminal user interface for vault management without opening the GUI.

**Module & Command**:
- CLI subcommand: `crypt-env tui`
- Source: `src-tauri/src/bin/crypt-env/commands/tui.rs`
- Built with ratatui 0.29 + crossterm 0.28

**Screens**:
- Unlock (master password entry)
- Main (item list + fuzzy search with `/`)
- Item Detail (reveal/copy controls)
- Help (`?`)
- Confirm (for destructive operations)

**Keybindings**:
- Navigate: ↑↓ or jk
- Fuzzy search: `/`
- Detail view: Enter
- Reveal secret: v
- Copy to clipboard: c
- Delete item: d
- Refresh: r
- Help: ?
- Quit: q

---

### 3. Internet Relay Sharing
Secure secret sharing via encrypted relay (Supabase table) for users on different networks.

**Module & Commands**:
- `src-tauri/src/share/relay.rs` — Relay protocol implementation
- Tauri commands: `share_relay_send`, `share_relay_receive`

**Flow**:
1. **Sender**: Selects items → encrypts with AES-256-GCM → uploads to Supabase relay → receives `XXXX-XXXX` code + plaintext passphrase
2. **Receiver**: Enters code + passphrase → downloads encrypted payload → decrypts → imports items
3. **Security**: Burn-after-read, 24-hour TTL, Argon2id KDF from passphrase

**Setup**:
- Requires free Supabase project
- Run relay setup SQL (provided in docs)
- Store Supabase URL + anon key in Settings → Internet Sharing config

**Frontend**: New "INTERNET" tab in ShareModal, relay configuration in Settings
