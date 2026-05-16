# 🔐 CryptEnv

> A local-first, encrypted secrets vault for developers — accessible from anywhere with a hotkey. Integrate with AI agents via MCP to inject secrets as environment variables without exposing plaintext values.

![License](https://img.shields.io/badge/license-MIT-green)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue)
![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202.0-orange)
![Status](https://img.shields.io/badge/status-active%20development-yellow)

CryptEnv is a desktop application that centralizes your API keys, credentials, commands, links and notes in a single encrypted local vault. No cloud. No subscriptions. No secrets in your WhatsApp chats.

### Why CryptEnv?

CryptEnv's MCP server integrates with AI coding agents (Claude Code, Claude Desktop, etc.) to enable secure automation workflows. Instead of sharing API keys with tools—where they appear in plain text in logs, prompts, or responses—CryptEnv injects secrets directly as environment variables. The AI agent never sees the actual values, only uses them safely within subprocess calls.

---

## ✨ Features

- **Instant access** — press `Ctrl+Alt+Z` from any app to open the vault
- **5 item types** — API Keys, Credentials (user+pass+URL), Links, Commands, Notes
- **Fuzzy search** — find anything in milliseconds
- **Editable categories** — organize your vault your way
- **Clipboard-first** — copy any secret in one click
- **Export formats** — generate `.env`, `export VAR=val` or `$env:VAR = "val"` from any secret
- **Command runner** — store terminal commands with `{{placeholders}}` and fill them on the fly
- **Windows Hello biometric unlock** — unlock vault with fingerprint or face recognition (Windows 10/11)
- **Auto-lock** — vault locks automatically after configurable inactivity timeout
- **Backup & restore** — export encrypted `.cenvbak` backups and restore with merge or replace modes
- **Import from password managers** — import from `.env` files, Bitwarden, 1Password, or generic CSV
- **Change master password** — update vault password and re-encrypt all items atomically
- **Vault wipe** — permanently delete all vault data
- **CLI** — manage your vault from the terminal with `crypt-env` commands
- **Local REST API** — integrate with your own tools at `127.0.0.1:47821`
- **MCP server** — let AI agents (like Claude) inject secrets as environment variables without seeing them in plain text

---

## 🔐 Windows Hello Biometric Unlock

On Windows 10/11, CryptEnv can unlock your vault using Windows Hello (fingerprint, face, or PIN) instead of typing your master password every time. The master password is stored encrypted with DPAPI (Windows Data Protection API), scoped to your user account and machine.

### How it works

1. **Enrollment**: Go to Settings → Biometric Unlock → ENABLE. Enter your master password and perform Windows Hello authentication once to enroll.
2. **Unlock**: On the lock screen, click "UNLOCK WITH WINDOWS HELLO" and authenticate with your biometric method.
3. **Security**: Your master password is protected by Windows DPAPI and only accessible if you pass Windows Hello verification. The password is never stored in plaintext.

### Disabling

In Settings → Biometric Unlock → DISABLE. This immediately clears the stored DPAPI blob; biometric unlock is no longer available until you re-enroll.

**Note**: Biometric unlock is Windows-only and requires Windows Hello to be configured on your device. On macOS and Linux, the option is not available.

---

## 🔒 Security

- **AES-256-GCM** encryption for all sensitive fields
- **Argon2id** for master password hashing — never stored in plaintext
- **SQLite** local database — your data never leaves your machine
- **Zeroized keys** — encryption key is wiped from memory on lock
- **Timing-safe** token comparison to prevent brute-force attacks
- **Strict CSP** on the Tauri webview
- **MCP never exposes secret values** — injects them directly as environment variables

---

## 🖥️ Screenshots

![Promotion](/docs/images/imagen_readme.png "Promotion")

---

## 🚀 Getting Started

### Platform Support

- **Windows 10/11** — fully supported and tested
- **macOS** — in progress (UI/backend functional, installer/signing in development)
- **Linux** — in progress (UI/backend functional, installer in development)

### Prerequisites

- [Rust](https://rustup.rs/) (MSVC toolchain on Windows)
- [Node.js](https://nodejs.org/) LTS
- [pnpm](https://pnpm.io/)
- **Windows**: Microsoft C++ Build Tools + WebView2 Runtime
- **macOS**: Xcode Command Line Tools (`xcode-select --install`)
- **Linux**: `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`

### Install & Run

```bash
git clone https://github.com/maosuarez/crypt-env.git
cd crypt-env
pnpm install
pnpm tauri dev
```

First Rust build takes 3–8 minutes. Subsequent builds are fast. For detailed build instructions and distribution guides, see [docs/building.md](docs/building.md).

### Build for Production

```bash
pnpm tauri build
```

This produces platform-specific installers:
- **Windows**: `.msi` and `.exe` installers
- **macOS**: `.dmg` disk image
- **Linux**: `.deb`, `.rpm`, and `.AppImage` packages

See [docs/building.md](docs/building.md) for distribution instructions.

---

## 🖱️ CLI Usage

The `crypt-env` CLI binary connects to the running desktop app (via REST API on `127.0.0.1:47821`) and provides terminal access to your vault. Make sure the CryptEnv app is running and unlocked before using these commands.

### Command Reference

#### `crypt-env add [KEY=value] | [--file .env] | [$VARNAME]`

Add a new secret to the vault. Choose one input method:

```bash
# Add a key-value secret (quoted for safety)
crypt-env add "OPENAI_API_KEY=sk-..."

# Add from .env file (reads KEY=value lines)
crypt-env add --file .env.local

# Add by referencing an existing environment variable
crypt-env add $DATABASE_URL
```

#### `crypt-env set KEY [KEY2 ...]`

Print a shell assignment to set the secret as an environment variable. Does not expose the secret value in the terminal output—only prints the assignment syntax.

```bash
# bash/zsh: eval to set the variable in current shell
eval $(crypt-env set OPENAI_API_KEY)

# PowerShell: pipe to Invoke-Expression
crypt-env set OPENAI_API_KEY | Invoke-Expression

# Output example: export OPENAI_API_KEY='[REDACTED]'
# (actual value is injected directly, not displayed)
```

#### `crypt-env inject KEY [KEY2 ...]`

Print a shell assignment suitable for direct injection (alternative to `set`).

```bash
crypt-env inject MY_API_KEY
# Output: $env:MY_API_KEY = "[REDACTED]"
```

#### `crypt-env fill TEMPLATE [-o OUTPUT]`

Fill a `.env.example` template with secrets from your vault, creating a `.env` file.

```bash
# Fill .env.example and output to .env
crypt-env fill .env.example -o .env

# Fill and print to stdout (no file written)
crypt-env fill .env.example
```

Each `KEY=` placeholder is replaced with the corresponding secret value from the vault.

#### `crypt-env search TERM [--type TYPE] [--category CATEGORY]`

Search your vault without exposing secret values. Results show item name, type, and category—not the secret itself.

```bash
# Search for items matching "api"
crypt-env search api

# Filter by type (key, credential, link, command, note)
crypt-env search github --type credential

# Filter by category
crypt-env search --category "work" api
```

#### `crypt-env list`

List all saved commands in your vault (without their contents).

```bash
crypt-env list
```

Output shows command name, description, and placeholder variables.

#### `crypt-env exec NAME [--VAR=value ...]`

Execute a saved command, resolving `{{placeholder}}` variables.

```bash
# Run a saved command named "deploy"
crypt-env exec deploy --HOST=production --PORT=8080

# The placeholders are replaced and command is returned
# (does not execute the command, just resolves it)
```

#### `crypt-env cmd [SUBCOMMAND]`

Manage saved commands.

```bash
# List all commands
crypt-env cmd list

# Get help for a command
crypt-env cmd deploy --help

# View details of a command
crypt-env cmd show deploy
```

#### `crypt-env memory [--name NAME] [--description DESC] [--command CMD]`

Save a new command interactively to your vault.

```bash
# Interactive mode: prompts for name, description, and command
crypt-env memory

# Non-interactive mode: create command with all fields
crypt-env memory --name "deploy" \
  --description "Deploy to production" \
  --command "cd ~/project && git push && docker-compose up -d"
```

#### `crypt-env doctor`

Check app health and connectivity. Verifies the REST API is running and accessible.

```bash
crypt-env doctor
# Output: API is running ✓
#         Vault is unlocked ✓
#         All systems nominal
```

#### `crypt-env share send <ITEM_IDS>...`

Start a secure LAN sharing session as sender. Generates a 6-digit pairing code.

```bash
# Share multiple items via LAN bridge
crypt-env share send api_key_1 api_key_2

# Output:
# Pairing code: 123456 (expires in 5 minutes)
# Waiting for peer to connect...
# Peer fingerprint: a1b2c3d4
# Confirm match? (y/n)
```

#### `crypt-env share receive`

Start a secure LAN sharing session as receiver. Requires pairing code from sender.

```bash
crypt-env share receive 123456

# Output:
# Connecting to peer...
# Peer fingerprint: a1b2c3d4
# Confirm match? (y/n)
# Receiving items...
# Imported 2 items successfully
```

#### `crypt-env share export [ITEM_IDS] -o file`

Export items as an encrypted, self-contained `.vault` package with a random passphrase.

```bash
# Export specific items
crypt-env share export api_key_1 api_key_2 -o secrets.vault

# Output:
# Exported 2 items to secrets.vault
# Passphrase: xK9pL2mN (save this and share separately!)
```

#### `crypt-env share import -f file`

Import items from an encrypted `.vault` package.

```bash
crypt-env share import -f secrets.vault

# Prompts for passphrase (will not echo to terminal)
# Output: Imported 2 items successfully
```

### Examples

```bash
# Add a secret and immediately use it
crypt-env add "GITHUB_TOKEN=ghp_..."
eval $(crypt-env set GITHUB_TOKEN)

# Fill a template before running a script
crypt-env fill .env.example -o .env
source .env
./deploy.sh

# Search for database credentials (shows match, not the password)
crypt-env search database
# Output: Found: prod_db_password (credential, category: databases)

# Run a deployment command with parameters
crypt-env exec deploy --ENV=staging --REGION=us-west-2
```

---

## 🔗 Secret Sharing

CryptEnv supports **two secure methods** for sharing secrets with teammates:

### LAN Bridge (Local Network, Real-Time)

For users on the same network, CryptEnv uses encrypted mDNS discovery and X25519 ECDH key exchange to establish a secure channel. No passphrases to manage, just a 6-digit code.

**Flow**:
1. Sender runs: `crypt-env share send api_key_1 api_key_2`
   - Generates pairing code (e.g., `123456`), valid for 5 minutes
   - Displays peer fingerprint after receiver connects

2. Receiver runs: `crypt-env share receive 123456`
   - Discovers sender via mDNS
   - Both parties exchange public keys and verify fingerprint
   - Receiver confirms fingerprint match with sender's code
   - Items are transferred encrypted over TCP

3. Session auto-destroys after completion or inactivity

**Security**:
- X25519 ECDH for key exchange (industry-standard, post-quantum resistant)
- Fingerprint (first 8 hex chars of SHA-256(sender_pub || receiver_pub)) prevents MITM attacks
- AES-256-GCM encryption on all messages
- 6-digit pairing code is short (brute-forceable in ~2 seconds if attacker has network access, but code expires in 5 minutes)

### Encrypted Package (Offline, Portable)

For scenarios where real-time sharing isn't possible (different networks, email transfer, USB drive), export items as a self-contained encrypted `.vault` file.

**Flow**:
1. Sender runs: `crypt-env share export api_key_1 api_key_2 -o secrets.vault`
   - Generates random 12-character passphrase
   - Encrypts items using Argon2id(m=32768, t=2, p=2) KDF from passphrase
   - Shows passphrase once (save it!)
   - Creates portable `.vault` file

2. Sender shares the `.vault` file via any channel (email, Slack, USB, etc.)
   - **Passphrase must be shared separately** (via phone call, signal, etc.)

3. Receiver runs: `crypt-env share import -f secrets.vault`
   - Prompts for passphrase (does not echo)
   - Decrypts and imports items into vault
   - Logs import in audit trail

**Security**:
- AES-256-GCM encryption (same as vault encryption)
- Argon2id with high memory cost (32768 KiB) makes brute-forcing expensive
- Passphrase never stored, only used during encryption/decryption
- `.vault` file is self-contained and portable (can be stored offline)

### Audit Trail

All share operations (send, receive, import, export) are logged in the vault's `share_log` database table:
- Mode (LAN bridge or encrypted package)
- Direction (sent or received)
- Item IDs shared
- Peer fingerprint (LAN mode) or timestamp
- Exact timestamp (ISO 8601)

---

## 💾 Backup & Restore

CryptEnv lets you export your vault to encrypted `.cenvbak` files for backup and recovery.

### Export (Backup)

From the main vault, go to Settings → Backup & Restore → EXPORT BACKUP.

1. Specify a file path (e.g., `C:\Users\you\Desktop\vault-backup`). The `.cenvbak` extension is appended automatically.
2. Click EXPORT BACKUP. All items and categories are encrypted and written to the file.
3. The backup preserves your vault's encryption key material — you only need your original master password to restore.

**Security**: Your backup is as secure as your vault. Items remain AES-256-GCM encrypted with the same key.

### Restore (Import)

From Settings → Backup & Restore → RESTORE BACKUP.

1. Select a `.cenvbak` file.
2. Enter the master password that was in use when the backup was created.
3. Choose **MERGE** or **REPLACE**:
   - **MERGE**: Adds items from the backup to your existing vault. Duplicates are inserted as new entries.
   - **REPLACE**: Wipes your entire vault and restores it to the backup's state. This cannot be undone.
4. Click MERGE INTO VAULT or REPLACE VAULT to proceed.

**Returns**: The number of items successfully restored.

---

## 📥 Import from Password Manager

Migrate secrets from other password managers or `.env` files into your CryptEnv vault.

From Settings → Data → IMPORT FROM PASSWORD MANAGER.

### Supported Formats

- **ENV file (`.env`)** — standard KEY=VALUE pairs
- **Bitwarden CSV** — exported from Bitwarden's vault
- **1Password CSV** — exported from 1Password
- **Generic CSV** — auto-detected columns (name, value, username, password, url, notes)

### Workflow

1. **Select format** — choose the import format that matches your source file.
2. **Choose file** — select your `.env` or `.csv` file from disk.
3. **Preview & select items** — review parsed items and choose which ones to import. Deselect any items you don't want.
4. **Import** — items are re-encrypted with your vault key and added. Items with duplicate names are skipped.

**Returns**: The count of items successfully imported, and the number of items skipped (duplicates).

---

## 🔑 Change Master Password

Your master password can be changed at any time without losing access to your vault.

From Settings → Security → CHANGE.

1. Enter your **current master password**.
2. Enter your **new password** (minimum 8 characters).
3. Confirm the new password.
4. Click CONFIRM CHANGE.

Behind the scenes, all vault items are re-encrypted with the new password's derived key in a single atomic transaction. If the change succeeds, the new password applies immediately.

---

## 🗑️ Vault Wipe

Permanently delete all vault data (items, categories, settings, and crypto material).

From Settings → Data → WIPE ALL DATA.

1. Click WIPE.
2. Confirm in the dialog. This action **cannot be undone**.

After wiping, your vault is reset to the initial state. On next launch, you will be prompted to create a new master password.

---

## 🤖 MCP Integration

CryptEnv exposes a **stdio-based MCP server** (JSON-RPC 2.0) that AI agents can use to interact with your vault securely.

### Configure Claude Desktop

`crypt-env-mcp` is a **stdio subprocess** — Claude Desktop launches it directly. It is **not** an HTTP server and must **not** be configured with a `url` field.

Locate your Claude Desktop config at `%APPDATA%\Claude\claude_desktop_config.json` and add:

```json
{
  "mcpServers": {
    "cryptenv": {
      "command": "C:\\full\\path\\to\\crypt-env-mcp.exe"
    }
  }
}
```

Replace the path with the actual location of your `crypt-env-mcp.exe` binary. If you built from source, it is at:

```
src-tauri\target\release\crypt-env-mcp.exe
```

**Prerequisites before using the MCP tools:**
1. Generate an MCP token once: open CryptEnv → Settings → Integrations → Generate MCP Token
2. Start the CryptEnv app and unlock the vault — `crypt-env-mcp` connects to the local REST API at `127.0.0.1:47821`
3. Restart Claude Desktop after editing the config

### Available MCP Tools

| Tool | Input | Output | Description |
|------|-------|--------|-------------|
| `crypt_env_doctor` | — | Health status | Check app connectivity and vault state |
| `crypt_env_list_items` | `type`, `category` (optional) | Item metadata (no values) | List secrets filtered by type and/or category |
| `crypt_env_get_item` | `item_id` | Item name, type, category | Get item details without exposing the secret value |
| `crypt_env_search_items` | `query`, `type`, `category` (optional) | Matching item metadata | Search vault without revealing secret contents |
| `crypt_env_add_item` | `name`, `type`, `value`, `category`, `notes` | Confirmation | Add a new secret to the vault |
| `crypt_env_generate_env` | `items: [KEY, ...]` | Shell export statements | Generate `.env` syntax for a list of keys (secret values injected inline, not exposed in response) |
| `crypt_env_inject_env` | `items: [KEY, ...]` | Shell assignment code | Inject secrets directly as environment variables in the current process |
| `crypt_env_fill_env` | `template_path`, `output_path` | File path confirmation | Fill a `.env.example` template with vault secrets and save to `.env` |
| `crypt_env_update_settings` | `timeout`, `theme`, etc. | Updated settings | Modify app settings (not master password) |
| `crypt_env_list_commands` | — | Command list with placeholders | List all saved commands in the vault |
| `crypt_env_run_command` | `command_name`, `variables: {VAR=value, ...}` | `{ exit_code, stdout, stderr }` | Execute command with resolved `{{placeholder}}` variables; returns exit status and output (secrets never in response) |
| `crypt_env_list_categories` | — | Category list with metadata | List all categories (id, name, color, description) |
| `crypt_env_create_category` | `name`, `color`, `description` (optional) | `{ id, name, color, description }` | Create a new category in the vault |
| `crypt_env_update_category` | `id`, `name`, `color`, `description` (optional) | `{ id, name, color, description }` | Update an existing category; omitted fields keep their current values. Pass `description=""` to clear it |
| `crypt_env_delete_category` | `id` | `{ deleted: true }` | Delete a category by id |
| `crypt_env_update_item` | `id`, plus any fields to update (`name`, `value`, `url`, `username`, `password`, `title`, `description`, `notes`, `content`, `command`, `shell`, `categories`) | `{ updated_at }` | Update an existing vault item; only provided fields change, omitted fields (including secrets) are preserved |
| `crypt_env_delete_item` | `id` | `{ deleted: true, id }` | Permanently delete an item from the vault |
| `crypt_env_share_listen` | `item_ids: [...]` | `{ pairing_code, expires_in }` | Start LAN send session; returns pairing code for peer to connect |
| `crypt_env_share_connect` | `pairing_code` | `{ fingerprint }` | Start LAN receive session; returns sender's fingerprint for verification |
| `crypt_env_share_confirm` | `confirmed: bool` | `{ status }` | Confirm fingerprint match; session begins if true |
| `crypt_env_share_cancel` | — | `{ cancelled }` | Cancel active share session |
| `crypt_env_share_status` | — | `{ state, progress }` | Poll current share session status |
| `crypt_env_share_export` | `item_ids: [...]` | `{ ciphertext, salt, nonce, passphrase }` | Export items as encrypted package; **passphrase must be shown to user, never logged** |
| `crypt_env_share_import` | `package: { ... }, passphrase` | `{ imported_count }` | Import items from encrypted package |

### Typical MCP Workflow

1. **Health check** — `crypt_env_doctor` to verify API is running
2. **List & search** — `crypt_env_list_items` or `crypt_env_search_items` to find the secret you need
3. **Inject secrets** — `crypt_env_inject_env` or `crypt_env_generate_env` to set environment variables for a subprocess
4. **Execute tasks** — Run your subprocess with the injected environment (secret values never appear in Claude's context)

Example Claude workflow:
```
Claude: "I'll help you deploy. Let me check what secrets are available..."
→ Calls: crypt_env_list_items(type="credential", category="deployment")
← Returns: [prod_db_password (credential), aws_access_key (credential), ...]

Claude: "I found your deployment credentials. Injecting them now..."
→ Calls: crypt_env_inject_env(items=["prod_db_password", "aws_access_key"])
← Returns: export statements that set environment variables

Claude: "Now executing deployment with secrets safely in environment..."
→ Subprocess runs with DATABASE_PASSWORD and AWS_ACCESS_KEY in env
← Claude never sees the actual secret values
```

### MCP Secret Sharing Workflow

LLM agents can orchestrate secure secret sharing with teammates:

```
User: "Share my API keys with alice@example.com"

Claude: "I'll initiate a secure sharing session..."
→ Calls: crypt_env_share_listen(item_ids=["openai_api_key", "stripe_key"])
← Returns: { pairing_code: "123456", expires_in: 300 }

Claude: "I've generated pairing code 123456. Tell Alice to run: crypt-env share receive 123456"
[Alice runs the command on her machine]

Claude: "Now confirming fingerprint..."
→ Calls: crypt_env_share_confirm(confirmed=true)
← Returns: { status: "confirmed" }

Claude: "Items transferred securely. Alice's vault now contains your shared keys."

---

User: "Create a portable share package for my team"

Claude: "I'll export your secrets as an encrypted package..."
→ Calls: crypt_env_share_export(item_ids=["api_key_1", "api_key_2"])
← Returns: { ciphertext: "...", salt: "...", nonce: "...", passphrase: "xK9pL2mN" }

Claude: "Exported successfully! Share the file and this passphrase separately:
   File: share_package.vault (can email/upload to cloud)
   Passphrase: xK9pL2mN (share via Slack/call/secure message)"
```

---

## 🔌 REST API

CryptEnv runs a local HTTPS API at `127.0.0.1:47821` (localhost only). This is useful for custom integrations, scripts, or tools that want to interact with your vault without using the CLI or GUI.

**TLS**: The API uses a self-signed certificate generated automatically on first launch. The cert is stored at `%APPDATA%\com.maosuarez.cryptenv\tls\cert.pem`. For `curl` testing, pass `--cacert` with that path or use `-k` (insecure, for local dev only):
```bash
curl --cacert "%APPDATA%\com.maosuarez.cryptenv\tls\cert.pem" https://127.0.0.1:47821/health
# or (local dev only):
curl -k https://127.0.0.1:47821/health
```

**Authentication**: All endpoints (except `/health`) require the `X-Vault-Token` header with your MCP token. Generate this in CryptEnv → Settings → Integrations → Generate MCP Token.

### Endpoints

#### `GET /health`

Health check — no authentication required.

```bash
curl https://127.0.0.1:47821/health
# Response: { "status": "ok", "version": "0.1.0" }
```

#### `POST /unlock`

Unlock the vault with the master password. Rate-limited to 5 attempts per 60 seconds.

```bash
curl -X POST https://127.0.0.1:47821/unlock \
  -H "Content-Type: application/json" \
  -d '{"master_password": "your_master_password"}'
# Response: { "token": "session_token..." }
```

**Rate Limit Headers**:
- `Retry-After: 45` — seconds until next attempt allowed (on 429 response)

#### `POST /fill`

Fill a `.env.example` template with secrets from the vault.

```bash
curl -X POST https://127.0.0.1:47821/fill \
  -H "X-Vault-Token: your_mcp_token" \
  -H "Content-Type: application/json" \
  -d '{
    "template": "API_KEY=\nDATABASE_URL=",
    "output_path": ".env"
  }'
# Response: { "success": true, "path": ".env" }
```

#### `GET /items`

List all items in the vault (no secret values exposed).

```bash
curl https://127.0.0.1:47821/items \
  -H "X-Vault-Token: your_mcp_token"
# Response: [
#   { "id": "uuid1", "name": "OPENAI_API_KEY", "type": "key", "category": "api" },
#   { "id": "uuid2", "name": "prod_db", "type": "credential", "category": "database" },
#   ...
# ]
```

Query parameters:
- `type=key` — filter by item type (key, credential, link, command, note)
- `category=api` — filter by category
- `search=openai` — search by name

#### `POST /items`

Create a new item in the vault.

```bash
curl -X POST https://127.0.0.1:47821/items \
  -H "X-Vault-Token: your_mcp_token" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "STRIPE_API_KEY",
    "type": "key",
    "value": "sk_live_...",
    "category": "payment",
    "notes": "Stripe production key"
  }'
# Response: { "id": "new_uuid", "created_at": "2026-05-10T..." }
```

#### `GET /items/:id`

Get a specific item's metadata (not the secret value).

```bash
curl https://127.0.0.1:47821/items/uuid1 \
  -H "X-Vault-Token: your_mcp_token"
# Response: {
#   "id": "uuid1",
#   "name": "OPENAI_API_KEY",
#   "type": "key",
#   "category": "api",
#   "notes": "OpenAI production key",
#   "created_at": "2026-04-15T..."
# }
```

#### `GET /items/:id/reveal`

Reveal the secret value of an item (use with caution — logs this action).

```bash
curl https://127.0.0.1:47821/items/uuid1/reveal \
  -H "X-Vault-Token: your_mcp_token"
# Response: { "value": "sk_live_..." }
```

#### `PUT /items/:id`

Update an item. **Server-side partial merge**: only fields present in the request body are updated; omitted fields (including secret values) are preserved from the existing item.

```bash
curl -X PUT https://127.0.0.1:47821/items/uuid1 \
  -H "X-Vault-Token: your_mcp_token" \
  -H "Content-Type: application/json" \
  -d '{ "name": "new_name", "category": "new_category" }'
# Response: { "updated_at": "2026-05-10T..." }
```

**Field semantics**:
- All fields (except `categories`) are optional; omitted fields keep their current values
- `categories`: absent = keep existing, `[]` = clear all categories, `[...]` = replace with new list
- Secret values (`value`, `password`, `content`, etc.) can be updated by including them; if omitted, existing encrypted values are preserved

#### `DELETE /items/:id`

Delete an item from the vault.

```bash
curl -X DELETE https://127.0.0.1:47821/items/uuid1 \
  -H "X-Vault-Token: your_mcp_token"
# Response: { "deleted": true }
```

#### `GET /categories`

List all categories with metadata (id, name, color, description).

```bash
curl https://127.0.0.1:47821/categories \
  -H "X-Vault-Token: your_mcp_token"
# Response: [
#   { "id": "api", "name": "API Keys", "color": "#FF5733", "description": "Third-party API credentials" },
#   { "id": "db", "name": "Database", "color": "#33FF57", "description": "Database credentials" },
#   ...
# ]
```

#### `POST /categories`

Create a new category.

```bash
curl -X POST https://127.0.0.1:47821/categories \
  -H "X-Vault-Token: your_mcp_token" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Cloud Providers",
    "color": "#FF33FF",
    "description": "AWS, GCP, Azure credentials"
  }'
# Response: { "id": "cloud", "name": "Cloud Providers", "color": "#FF33FF", "description": "AWS, GCP, Azure credentials" }
```

#### `PUT /categories/:id`

Update an existing category. **Server-side partial merge**: only provided fields change.

```bash
curl -X PUT https://127.0.0.1:47821/categories/api \
  -H "X-Vault-Token: your_mcp_token" \
  -H "Content-Type: application/json" \
  -d '{ "description": "Updated description" }'
# Response: { "id": "api", "name": "API Keys", "color": "#FF5733", "description": "Updated description" }
```

Pass `description=""` (empty string) to clear the description field.

#### `GET /commands/:id`

Get a specific command's details (name, description, placeholders).

```bash
curl https://127.0.0.1:47821/commands/123 \
  -H "X-Vault-Token: your_mcp_token"
# Response: {
#   "id": 123,
#   "name": "deploy",
#   "description": "Deploy to production",
#   "command": "cd ~/project && deploy.sh --env={{ENV}} --region={{REGION}}",
#   "placeholders": ["{{ENV}}", "{{REGION}}"]
# }
```

**Note**: To execute a saved command with resolved placeholders, use the MCP tool `crypt_env_run_command` instead. The REST API is read-only for commands.

#### `GET /settings`

Get current app settings.

```bash
curl https://127.0.0.1:47821/settings \
  -H "X-Vault-Token: your_mcp_token"
# Response: {
#   "timeout_minutes": 5,
#   "hotkey": "Ctrl+Alt+Z",
#   "auto_lock": true,
#   "theme": "dark"
# }
```

#### `PUT /settings`

Update app settings.

```bash
curl -X PUT https://127.0.0.1:47821/settings \
  -H "X-Vault-Token: your_mcp_token" \
  -H "Content-Type: application/json" \
  -d '{ "timeout_minutes": 10, "theme": "light" }'
# Response: { "updated_at": "2026-05-10T..." }
```

---

## 🗂️ Project Structure

```
crypt-env/
├── src/                    # React + TypeScript frontend
│   ├── components/         # UI components (Lock, MainVault, AddItem, Settings...)
│   ├── store/              # Zustand global state
│   ├── hooks/              # Tauri invoke() wrappers
│   └── types/              # Shared TypeScript types
├── src-tauri/              # Rust backend
│   ├── src/
│   │   ├── bin/            # Standalone binaries
│   │   │   ├── crypt-env.rs    # CLI binary for vault operations
│   │   │   └── crypt-env-mcp.rs # MCP server (stdio, JSON-RPC 2.0)
│   │   ├── crypto/         # AES-256-GCM + Argon2id
│   │   ├── db/             # SQLite layer
│   │   ├── vault/          # Business logic (CRUD)
│   │   ├── api/            # Axum REST server (port 47821)
│   │   ├── cli/            # CLI command definitions (used by vault binary)
│   │   ├── mcp/            # MCP request handlers (used by crypt-env-mcp binary)
│   │   ├── lib.rs          # Shared library code
│   │   └── main.rs         # Tauri desktop app
│   ├── Cargo.toml
│   └── tauri.conf.json
├── CLAUDE.md               # Claude Code agent instructions
├── context.md              # Full technical context
├── CONTRIBUTING.md         # Contribution guidelines
├── SECURITY.md             # Security policy
├── CHANGELOG.md            # Release notes
└── README.md
```

---

## 🤝 Contributing

Contributions are welcome! CryptEnv is actively developed and there's a lot of ground to cover.

### How to contribute

1. Fork the repository
2. Create a feature branch: `git checkout -b feat/your-feature`
3. Make your changes
4. Open a Pull Request with a clear description of what you changed and why

### Good first issues

- [ ] macOS & Linux testing and fixes
- [ ] Dark/light theme toggle
- [ ] Browser extension for auto-filling credentials
- [ ] Vault item sharing via QR code (local network only)

### Guidelines

- Keep PRs focused — one feature or fix per PR
- Follow existing code conventions (Rust: snake_case, React: PascalCase)
- Security-related changes must include a description of the threat model
- No new cloud dependencies — this project is intentionally local-first

---

## 📋 Roadmap

- [x] Encrypted local vault (AES-256-GCM + Argon2id)
- [x] 5 item types (Key, Credential, Link, Command, Note)
- [x] Desktop UI with global hotkey
- [x] Clipboard integration
- [x] Export to `.env` / shell formats
- [x] Editable categories
- [x] Auto-lock timeout
- [x] CLI (`crypt-env` binary)
- [x] Local REST API (Axum, port 47821)
- [x] MCP server (JSON-RPC 2.0 over stdio)
- [x] Rate limiting on `/unlock` endpoint (5 attempts per 60s)
- [x] Windows Hello biometric unlock (fingerprint, face, PIN)
- [x] Import from password managers (ENV, Bitwarden CSV, 1Password CSV)
- [x] Encrypted backup & restore (`.cenvbak` format)
- [x] Change master password (atomic re-encryption)
- [x] Vault wipe (permanent data deletion)
- [ ] macOS & Linux support (in progress)

---

## 📄 License

MIT © [Mao Suárez](https://github.com/maosuarez)

---

> Built because sharing API keys over WhatsApp is not a security strategy.
