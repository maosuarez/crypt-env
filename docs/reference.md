# crypt-env Reference

## REST API

Endpoint: `https://127.0.0.1:47821`

Authentication: Header `X-Vault-Token` containing either a session token (from POST /unlock, has TTL) or a static MCP token (stored in database, no expiry). Token verification uses constant-time comparison. Rate limiting enforced on /unlock: 5 attempts per 60-second window.

| Method | Endpoint | Auth | Description |
|--------|----------|------|-------------|
| POST | /unlock | none | Derives AES-GCM key from master password + Argon2 salt, generates 16-byte session token with configurable TTL |
| GET | /health | none | Returns version, vault_locked bool, item_count, mcp_token_configured |
| GET | /items | token | List items (redacted — no secret values). Query params: `type`, `category`, `search` |
| POST | /items | token | Create item. Validates: name (req, max 255), type (one of: secret/credential/link/note/command), value (req non-empty). Encrypts with AES-GCM before storing. Returns 422 on validation failure |
| GET | /items/:id | token | Get single item metadata (redacted) |
| PUT | /items/:id | token | Update item. Merges — omitted fields keep existing values including secret fields |
| DELETE | /items/:id | token | Delete item. Returns 204 |
| POST | /items/:id/reveal | token | Returns plaintext secret value. Requires `{"confirm": true}` in body. Logs access to stderr |
| GET | /categories | token | List categories (id, name, color, description) |
| POST | /categories | token | Create category. Validates name (req, max 100) and color (req). Generates random hex cid |
| PUT | /categories/:id | token | Update category fields. Passing `description: ""` clears it |
| DELETE | /categories/:id | token | Delete category. Returns 204 |
| GET | /commands | token | List items of type "command" with extracted `{{VAR}}` placeholders |
| GET | /commands/:id | token | Get single command with placeholders |
| GET | /settings | token | Get auto_lock_timeout (minutes) and hotkey |
| PUT | /settings | token | Update auto_lock_timeout and/or hotkey |
| POST | /fill | token | Fill a .env template with real values. If `output_path` given: writes to disk with RAII TempEnvFile guard (zeros + deletes on error), returns stats only — no secret in response. If no output_path: returns filled content inline |
| POST | /share/listen | token | Start LAN share session as sender. Registers mDNS, returns `pairing_code` |
| POST | /share/connect | token | Connect as receiver using pairing_code. Returns ECDH fingerprint |
| POST | /share/confirm | token | Confirm (or reject) fingerprint. Both sides must call this |
| GET | /share/status | token | Returns session state, fingerprint, direction, received_names |
| DELETE | /share/session | token | Cancel active share session |
| POST | /share/export | token | Export items as AES-256-GCM encrypted `.vault` file. Returns passphrase in response |
| POST | /share/import | token | Import from `.vault` file using passphrase |
| GET | /workspaces | token | List workspaces with vars |
| POST | /workspaces | token | Create or update workspace (upsert by id=0 = create). Sets paths and vars atomically |
| DELETE | /workspaces/:id | token | Delete workspace and vars |
| POST | /workspaces/:id/inject | token | Decrypt vault items for workspace vars and write to all configured .env paths. Only updates/appends keys found in inject_map |
| POST | /relay/send | token | Encrypt selected items with Argon2id-derived key and upload to Supabase relay. Returns code + passphrase. Requires relay_supabase_url and relay_supabase_anon_key in settings |
| POST | /relay/receive | token | Download from Supabase relay, decrypt with key+passphrase, import items. Burns after read (best-effort delete) |

### Notes

`decrypt_all_items` decrypts the entire vault on every authenticated request (no caching, no index), making every GET /items a full decryption pass — O(n) per request regardless of filters.

`handle_delete_item` and `handle_update_item` each acquire the vault lock twice: once to read/verify existence and once to commit changes, with full AES-GCM re-encryption of the item held between acquisitions.

Session token design uses a single `token_expires` slot (Instant monotonic), allowing only one active session per vault — concurrent client connections with different tokens will collide.

`/health` returns `item_count` without authentication, leaking the vault's size to unauthenticated callers; the value is only meaningful after unlock so the endpoint reveals whether the vault is currently unlocked.

`TempEnvFile` zeros output via `std::fs::write` which passes through OS page cache; on SSDs with wear leveling, overwritten data may persist in flash cells indefinitely — acceptable for most threat models but not forensic-grade.

`relay_delete` after receive is best-effort (error ignored), so relay payloads remain accessible to anyone with code+passphrase until the 24-hour TTL expires if deletion fails.

CORS guard accepts `Origin: null`, correctly matching local file:// and Tauri webviews, but also matches any sandboxed iframe — minimal practical impact but violates defense-in-depth.

---

## CLI

Command: `crypt-env`

| Command | Subcommand / Flags | Description |
|---------|-------------------|-------------|
| `add` | `KEY=value` | Add a secret from KEY=value literal |
| `add` | `$VARNAME` | Read value from system environment variable |
| `add` | `--file [PATH]` | Bulk-import from .env file (uses dotenvy). Defaults to `./.env`. Detects duplicates via GET /items before writing |
| `add` | `--credential` | Store as credential type instead of secret |
| `add` | `--note` | Store as note type |
| `add` | `--name NAME` | Override the stored key name |
| `add` | `--force` | Skip confirmation on duplicate keys |
| `doctor` | — | Check app health, vault lock state, token files, version |
| `fill` | `[PATH]` | Fill .env or .env.example with vault secrets. If .env.example: creates sibling .env. Preserves comments and blank lines. Warns on not-found keys |
| `inject` | `NAME [--shell TYPE]` | Prints shell assignment to stdout (safe for eval). Supported: pwsh, bash, zsh, sh. Prints verify hint to stderr |
| `list` | — | List saved commands in a table |
| `exec` | `NAME [ARGS]` | Execute a saved command by name |
| `memory` | — | Save a command string interactively |
| `search` | `QUERY` | Search items by name/title. Prints table of ID, TYPE, NAME, CATEGORIES. No values shown |
| `set` | `NAME` | Print export/env assignment for a secret (stdout) |
| `cmd` | `list/info/run` | Manage saved commands (list, get info, run) |
| `share send` | `ITEM_IDS...` | Start LAN share as sender. Polls for peer, shows fingerprint, prompts confirmation |
| `share receive` | — | Connect as receiver. Prompts pairing code, shows fingerprint, prompts confirmation |
| `share export` | `ITEM_IDS -o OUTPUT` | Export items as encrypted .vault file. Displays passphrase once |
| `share import` | `-f FILE` | Import from .vault file. Prompts passphrase via rpassword (no echo) |
| `category list` | — | List all categories |
| `category create` | `NAME COLOR [DESC]` | Create a new category |
| `category edit` | `ID [fields]` | Edit category by ID |
| `category delete` | `ID` | Delete category by ID |
| `tui` | — | Launch interactive TUI |
| `workspace list` | — | List workspaces (ID, Name, Template, Paths, Var count) |
| `workspace inject` | `--id ID` or `--name NAME` | Inject workspace vars into configured .env paths |
| `workspace delete` | `--id ID` | Delete workspace by ID |
| `relay send` | `--items 1,2,3` | Send items via internet relay. Prints code + passphrase once |
| `relay receive` | `--code CODE --passphrase PASS` | Receive items via relay |
| `sync` | `[--example PATH] [--env PATH] [--dry-run]` | Add new variables from .env.example into .env without overwriting existing. Fills from vault when found |

### Notes

`add --file` loads the entire item list via GET /items to detect duplicates, then POSTs each item sequentially — N+1 HTTP requests for a large .env file, with no batch-create optimization.

`share send` polls GET /share/status with `sleep(1s)` in a loop for up to 300 iterations (5 min) for fingerprint, then another 600 iterations (10 min) for peer acceptance — blocking the terminal indefinitely if the peer crashes or never connects.

`relay receive` accepts `--passphrase` as a CLI argument, which appears in shell history and `ps` output; all other secrets use `rpassword` (hidden prompt) — relay breaks this pattern.

`inject` prints the value to stdout embedded in a shell assignment; on multi-user systems the value is momentarily visible in `/proc/self/fd/1` on Linux and similar process introspection on other OSes.

`sync` appends new lines via `std::fs::OpenOptions::append` — if the .env file lacks a trailing newline, the first appended key appears on the same line as the last existing key.

`fill` writes output via `std::fs::write` (non-atomic) — process crash mid-write leaves the file truncated with no atomic rename recovery.

---

## TUI

Command: `crypt-env tui`

Screens: Unlock (master password entry), Main (item list), Detail (item metadata + controls), Help (keybinding reference), Confirm (destructive operation confirmation).

| Key | Screen | Action |
|-----|--------|--------|
| (type) | Unlock | Enter master password |
| Enter | Unlock | Submit password, transition to Main |
| ↑ / k | Main | Move selection up |
| ↓ / j | Main | Move selection down |
| / | Main | Enter fuzzy search mode |
| Esc | Main (search) | Exit search mode, clear filter |
| Enter | Main | Open Detail view for selected item |
| r | Main | Refresh item list from vault |
| ? | Main | Open Help screen |
| q | Main | Quit |
| v | Detail | Reveal/hide secret value |
| c | Detail | Copy secret value to clipboard |
| d | Detail | Show Confirm screen for delete |
| Esc / q | Detail | Return to Main |
| y / Enter | Confirm | Confirm destructive action |
| n / Esc | Confirm | Cancel |
| Esc / q | Help | Return to previous screen |
| Ctrl+C | Any | Quit immediately |

### Notes

TUI uses crossterm raw mode without panic hook or RAII guard — if the process panics mid-render, the terminal remains in raw mode and is unusable until manual `stty sane` or shell restart.

Copy-to-clipboard (`c`) relies on `tauri-plugin-clipboard-manager`, a GUI plugin designed for Tauri's webview context — clipboard integration in a standalone TUI binary is uncertain and may silently fail or panic.

Reveal (`v`) calls GET /items/:id/reveal on the REST API for every toggle, generating server-side stderr logs — repeated toggling produces spam without debounce or client-side caching.

Fuzzy search operates on the in-memory item list loaded at Main screen entry — no re-fetch on search, so changes made elsewhere (API, CLI) are not reflected until `r` is pressed.

TUI has no auto-lock timeout despite the setting existing in vault config — the vault remains unlocked indefinitely if the user leaves the TUI open, bypassing the `auto_lock_timeout` setting entirely.

Detail view includes `detail_scroll` field in App state but scroll rendering is not confirmed functional from the source code — long secrets may be silently clipped without visual indication.

---

## MCP

Tool namespace: `crypt-env` (invoked as `crypt_env_*`)

Protocol: JSON-RPC 2.0 over stdio (protocol version 2024-11-05)

Authentication: Automatic — MCP server reads the REST API session token from disk on startup and reuses it for all tool invocations.

| Tool | Required | Optional | Description |
|------|----------|----------|-------------|
| `crypt_env_list_items` | — | `type`, `category` | List item metadata (no values). Filter by type or category |
| `crypt_env_get_item` | `id` | — | Get single item metadata (no value) |
| `crypt_env_search_items` | `query` | — | Search items by name. Returns metadata only |
| `crypt_env_add_item` | `type`, `name` | `value`, `category`, `notes`, `url`, `username` | Add item to vault. Value passes through MCP → REST in plaintext |
| `crypt_env_update_item` | `id` | `name`, `value`, `url`, `username`, `password`, `title`, `description`, `notes`, `content`, `command`, `shell`, `categories` | Update item. Omitted fields keep existing values server-side |
| `crypt_env_delete_item` | `id` | — | Permanently delete item |
| `crypt_env_generate_env` | `keys` | — | Write .env file to temp dir with real values for given key names. Returns path + count. Values never in response. Cleans up previous temp file on next call |
| `crypt_env_inject_env` | `key` | — | Inject one secret as env var into the MCP process via `std::env::set_var`. Does not return value |
| `crypt_env_fill_env` | `template`, `output_path` | — | Fill .env.example template and write to output_path. Values never in response |
| `crypt_env_import_env_file` | `path` | `category`, `overwrite` | Read .env file from disk, parse KEY=value pairs, import each as vault secret. Values never in MCP response |
| `crypt_env_update_settings` | — | `auto_lock_timeout`, `hotkey` | Update vault settings |
| `crypt_env_doctor` | — | — | Health check: app status, lock state, item count, token config |
| `crypt_env_list_commands` | — | — | List saved commands with placeholders |
| `crypt_env_run_command` | `name` | `params` | Resolve {{VAR}} placeholders and execute command via shell. Returns stdout/stderr (truncated 2000 chars) and exit code. Resolved command never returned |
| `crypt_env_share_listen` | `items` | — | Start LAN share session. Returns pairing_code |
| `crypt_env_share_connect` | `pairing_code` | — | Connect as receiver. Returns fingerprint |
| `crypt_env_share_confirm` | `confirmed` | — | Confirm/reject fingerprint (boolean) |
| `crypt_env_share_cancel` | — | — | Cancel active share session |
| `crypt_env_share_status` | — | — | Get session state, fingerprint, direction |
| `crypt_env_share_export` | `items`, `output_path` | — | Export as .vault package. Passphrase shown in response once |
| `crypt_env_share_import` | `path`, `passphrase` | — | Import from .vault package |
| `crypt_env_list_categories` | — | — | List categories |
| `crypt_env_create_category` | `name`, `color` | `description` | Create category |
| `crypt_env_update_category` | `id` | `name`, `color`, `description` | Update category. Empty string description clears it |
| `crypt_env_delete_category` | `id` | — | Delete category |
| `crypt_env_list_workspaces` | — | — | List workspaces with var count |
| `crypt_env_inject_workspace` | — | `id`, `name` | Inject workspace vars into configured .env. Identify by id or name |
| `crypt_env_list_workspaces_by_env` | — | — | Group workspaces by environment keyword (production/development/staging/other) |
| `crypt_env_inject_env_by_name` | `project_path`, `environment` | `output_path` | Find workspace matching project+environment and inject. Falls back to item name/category matching |
| `crypt_env_relay_send` | `item_ids` | — | Send via internet relay. Returns code + passphrase (show immediately, only once) |
| `crypt_env_relay_receive` | `code`, `passphrase` | — | Receive via internet relay |
| `crypt_env_list_mcp_servers` | — | `scope` | List registered MCP servers from Claude config. Scope: global/project/all. Env values never returned |
| `crypt_env_add_mcp_server` | `name`, `command` | `args`, `env`, `scope` | Add MCP server to Claude config. `env` stores KEY: "" placeholders — never real values |
| `crypt_env_update_mcp_server` | `name` | `command`, `args`, `env`, `scope` | Merge-update existing MCP server entry |
| `crypt_env_delete_mcp_server` | `name` | `scope` | Remove MCP server entry from Claude config |

### Notes

`crypt_env_add_item` with `value` parameter passes the secret plaintext through the LLM's tool call context — this is the only tool that breaks the "values never visible to LLM" design principle, and any agent/LLM host logging tool calls will capture the raw secret value permanently.

`crypt_env_generate_env` writes to `std::env::temp_dir()` with random filename but does NOT enforce restrictive permissions (unlike the API's TempEnvFile with 0o600) — on multi-user systems the temp .env is world-readable until cleanup or next call.

`crypt_env_inject_env` uses `std::env::set_var` which is marked `unsafe` in Rust with `#[allow(unused_unsafe)]` — calling set_var from a multi-threaded context is undefined behavior if any thread reads env vars concurrently; the MCP server's single-threaded main loop mitigates this in practice.

`crypt_env_run_command` substitutes {{VAR}} placeholders into shell commands without sanitization — if params contain shell metacharacters or a vault item's command template is user-controlled, this is a command injection vector.

`crypt_env_inject_env_by_name` uses heuristics (name prefix matching, category name matching) to find items for an environment — could match unintended items if naming conventions drift, with no explicit confirmation before writing.

MCP server is single-threaded with a blocking I/O loop (`stdin.lock().lines()`) — a slow or stalled request blocks all subsequent MCP tool calls and the LLM host may time out other concurrent requests.

`TEMP_FILES` cleanup in `crypt_env_generate_env` is best-effort (called at start of next call) — if the MCP process exits before another call, temp .env files are abandoned on disk.

MCP config management tools (`crypt_env_add/update/delete_mcp_server`) do not validate the `command` field for path traversal or injection — a crafted command string could write an adversarial entry to the Claude config files.
