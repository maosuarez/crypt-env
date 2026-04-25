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

### Prerequisites

- [Rust](https://rustup.rs/) (MSVC toolchain on Windows)
- [Node.js](https://nodejs.org/) LTS
- [pnpm](https://pnpm.io/)
- Windows: Microsoft C++ Build Tools + WebView2 Runtime

### Install & Run

```bash
git clone https://github.com/maosuarez/crypt-env.git
cd crypt-env
pnpm install
pnpm tauri dev
```

First Rust build takes 3–8 minutes. Subsequent builds are fast.

### Build for Production

```bash
pnpm tauri build
```

---

## 🖱️ CLI Usage

```bash
# Fill a .env file with secrets from the vault
crypt-env --fill .env

# Set a secret as an environment variable in the current shell
eval $(crypt-env set OPENAI_API_KEY)          # bash/zsh
crypt-env set OPENAI_API_KEY | Invoke-Expression  # PowerShell

# List saved commands
crypt-env cmd --list

# Get help for a specific command
crypt-env cmd deploy --help

# Run a command resolving its placeholders
crypt-env cmd deploy --HOST=production --PORT=8080

# Search items without exposing values
crypt-env search openai
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

### Available MCP tools

| Tool | Description |
|------|-------------|
| `vault_list_items` | List items filtered by type/category — no secrets exposed |
| `vault_get_item` | Get item metadata without the secret value |
| `vault_generate_env` | Generate a `.env` file for a list of keys — values never in response |
| `vault_inject_env` | Inject a secret directly as an env var in the client process |
| `vault_add_item` | Add a new item to the vault |
| `vault_update_settings` | Update app settings (not master password) |
| `vault_list_commands` | List available commands with their placeholders |
| `vault_run_command` | Resolve a command's placeholders and return it |

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
- [x] CLI (`vault` binary)
- [x] Local REST API (Axum, port 47821)
- [x] MCP server (JSON-RPC 2.0 over stdio)
- [ ] Rate limiting on `/unlock` endpoint
- [ ] macOS & Linux support
- [ ] Import from password managers
- [ ] Encrypted backup

---

## 📄 License

MIT © [Mao Suárez](https://github.com/maosuarez)

---

> Built because sharing API keys over WhatsApp is not a security strategy.
