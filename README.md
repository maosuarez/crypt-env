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
- **Auto-lock** — vault locks automatically after configurable inactivity timeout
- **CLI** — manage your vault from the terminal with `crypt-env` commands
- **Local REST API** — integrate with your own tools at `127.0.0.1:47821`
- **MCP server** — let AI agents (like Claude) inject secrets as environment variables without seeing them in plain text

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

![Lock Screen](/docs/images/lockscreen.png "Lock Screen")

![Main View](/docs/images/mainscreen.png "Main View")

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
| `crypt_env_run_command` | `command_name`, `variables: {VAR=value, ...}` | Resolved command string | Resolve `{{placeholder}}` variables in a saved command |

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

---

## 🔌 REST API

CryptEnv runs a local HTTP API at `127.0.0.1:47821` (encrypted in transit). This is useful for custom integrations, scripts, or tools that want to interact with your vault without using the CLI or GUI.

**Authentication**: All endpoints (except `/health`) require the `X-MCP-Token` header with your MCP token. Generate this in CryptEnv → Settings → Integrations → Generate MCP Token.

### Endpoints

#### `GET /health`

Health check — no authentication required.

```bash
curl http://127.0.0.1:47821/health
# Response: { "status": "ok", "version": "0.2.0" }
```

#### `POST /unlock`

Unlock the vault with the master password. Rate-limited to 5 attempts per 60 seconds.

```bash
curl -X POST http://127.0.0.1:47821/unlock \
  -H "Content-Type: application/json" \
  -d '{"password": "your_master_password"}'
# Response: { "success": true, "token": "session_token..." }
```

**Rate Limit Headers**:
- `Retry-After: 45` — seconds until next attempt allowed (on 429 response)

#### `POST /fill`

Fill a `.env.example` template with secrets from the vault.

```bash
curl -X POST http://127.0.0.1:47821/fill \
  -H "X-MCP-Token: your_mcp_token" \
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
curl http://127.0.0.1:47821/items \
  -H "X-MCP-Token: your_mcp_token"
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
curl -X POST http://127.0.0.1:47821/items \
  -H "X-MCP-Token: your_mcp_token" \
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
curl http://127.0.0.1:47821/items/uuid1 \
  -H "X-MCP-Token: your_mcp_token"
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
curl http://127.0.0.1:47821/items/uuid1/reveal \
  -H "X-MCP-Token: your_mcp_token"
# Response: { "value": "sk_live_..." }
```

#### `PUT /items/:id`

Update an item.

```bash
curl -X PUT http://127.0.0.1:47821/items/uuid1 \
  -H "X-MCP-Token: your_mcp_token" \
  -H "Content-Type: application/json" \
  -d '{ "value": "new_value", "category": "new_category" }'
# Response: { "updated_at": "2026-05-10T..." }
```

#### `DELETE /items/:id`

Delete an item from the vault.

```bash
curl -X DELETE http://127.0.0.1:47821/items/uuid1 \
  -H "X-MCP-Token: your_mcp_token"
# Response: { "deleted": true }
```

#### `GET /categories`

List all categories in use.

```bash
curl http://127.0.0.1:47821/categories \
  -H "X-MCP-Token: your_mcp_token"
# Response: ["api", "database", "deployment", "personal"]
```

#### `POST /commands`

Execute a saved command with placeholder resolution.

```bash
curl -X POST http://127.0.0.1:47821/commands \
  -H "X-MCP-Token: your_mcp_token" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "deploy",
    "variables": { "ENV": "production", "REGION": "us-west-2" }
  }'
# Response: { "resolved": "cd ~/project && deploy.sh --env=production --region=us-west-2" }
```

#### `GET /settings`

Get current app settings.

```bash
curl http://127.0.0.1:47821/settings \
  -H "X-MCP-Token: your_mcp_token"
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
curl -X PUT http://127.0.0.1:47821/settings \
  -H "X-MCP-Token: your_mcp_token" \
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
- [ ] Import from 1Password / Bitwarden / `.env` files
- [ ] Browser extension for auto-filling credentials
- [ ] Encrypted backup & restore
- [ ] Vault item sharing via QR code (local network only)
- [ ] `crypt-env` CLI as a standalone installable binary

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
- [ ] macOS & Linux support (in progress)
- [ ] Import from password managers (ENV, Bitwarden CSV, 1Password CSV)
- [ ] Encrypted backup (`.cenvbak` format)

---

## 📄 License

MIT © [Mao Suárez](https://github.com/maosuarez)

---

> Built because sharing API keys over WhatsApp is not a security strategy.
