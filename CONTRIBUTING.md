# Contributing to CryptEnv

Thank you for your interest in contributing to CryptEnv! This is a local-first, security-focused tool built for developers — contributions of all kinds are welcome.

---

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [How to Contribute](#how-to-contribute)
- [Development Setup](#development-setup)
- [Project Structure](#project-structure)
- [Coding Conventions](#coding-conventions)
- [Security Contributions](#security-contributions)
- [Submitting a Pull Request](#submitting-a-pull-request)
- [Good First Issues](#good-first-issues)

---

## Code of Conduct

Be respectful. Be constructive. Focus on the code, not the person.
We welcome contributors of all experience levels.

---

## How to Contribute

There are many ways to contribute beyond writing code:

- 🐛 **Report bugs** — open an issue with steps to reproduce
- 💡 **Suggest features** — open a Discussion before building something big
- 🧪 **Test on macOS / Linux** — we develop on Windows; cross-platform fixes are highly valuable
- 📖 **Improve documentation** — typos, clarity, missing examples
- 🔒 **Security research** — see [Security Contributions](#security-contributions)
- 🎨 **UI/UX improvements** — the interface is functional but can always be better

---

## Development Setup

### Prerequisites

- [Rust](https://rustup.rs/) — MSVC toolchain on Windows, stable toolchain on macOS/Linux
- [Node.js](https://nodejs.org/) LTS
- [pnpm](https://pnpm.io/)
- Windows only: Microsoft C++ Build Tools + WebView2 Runtime

### Run locally

```bash
git clone https://github.com/maosuarez/crypt-env.git
cd crypt-env
pnpm install
pnpm tauri dev
```

First Rust build takes 3–8 minutes. Subsequent builds use cargo cache and are fast.

### Useful commands

```bash
# Frontend only (no Tauri window)
pnpm dev

# Check Rust compilation without building
cd src-tauri && cargo check

# Run Rust tests
cd src-tauri && cargo test

# Build production binary
pnpm tauri build
```

---

## Project Structure

```
crypt-env/
├── src/                    # React + TypeScript frontend
│   ├── components/         # One folder per screen/feature
│   ├── store/              # Zustand state management
│   ├── hooks/              # Tauri invoke() wrappers
│   └── types/              # Shared TypeScript interfaces
├── src-tauri/
│   ├── src/
│   │   ├── crypto/         # Encryption logic — touch carefully
│   │   ├── db/             # SQLite queries and migrations
│   │   ├── vault/          # Business logic (CRUD orchestration)
│   │   ├── api/            # Axum REST server (port 47821)
│   │   ├── cli/            # Terminal interface
│   │   └── mcp/            # MCP server
│   ├── Cargo.toml
│   └── tauri.conf.json
├── CLAUDE.md               # Instructions for Claude Code agent
├── context.md              # Full technical context and decisions
└── README.md
```

---

## Coding Conventions

### Rust (backend)
- **snake_case** for all identifiers
- No `unwrap()` in production code — use `Result` with typed errors
- Modules are decoupled: `db` has no knowledge of `api`, `vault` orchestrates both
- Tauri commands follow the naming pattern: `module_action` (e.g. `vault_get_items`, `crypto_unlock`)
- Use `zeroize` for any struct holding sensitive data

### TypeScript / React (frontend)
- **PascalCase** for components, **camelCase** for everything else
- Tailwind CSS only — no CSS modules, no inline styles, no styled-components
- All Rust communication via `invoke()` — never `fetch()` to localhost from React
- State: Zustand for global, `useState` for local form state
- Async data: TanStack Query

### Git
- Branch naming: `feat/description`, `fix/description`, `security/description`, `docs/description`
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/):
  ```
  feat(ui): add command placeholder editor
  fix(crypto): zeroize key on lock
  security(api): add rate limiting to /unlock endpoint
  docs(readme): add CLI usage examples
  ```
- One feature or fix per PR — keep PRs focused and reviewable

---

## Security Contributions

CryptEnv handles sensitive data. Security contributions are treated with extra care.

**Do not open a public issue for security vulnerabilities.**

Instead:
1. Email a description of the vulnerability to the maintainer (see GitHub profile)
2. Include: affected component, reproduction steps, potential impact, suggested fix if you have one
3. Allow up to 72 hours for an initial response
4. We will coordinate a fix and credit you in the release notes

For non-critical security improvements (hardening, code quality, missing validations), a regular PR is fine — just label it `security` and describe the threat model in the PR description.

---

## Submitting a Pull Request

1. **Open a Discussion or Issue first** for anything non-trivial — this avoids duplicate work and ensures alignment before you invest time coding
2. Fork the repository and create your branch from `main`
3. Make your changes following the conventions above
4. Test your changes:
   - `cargo check` passes
   - `pnpm tauri dev` runs without errors
   - Manual test of the affected feature
5. Open a PR with:
   - A clear title following Conventional Commits
   - Description of what changed and why
   - Screenshots or recordings for UI changes
   - Note any security implications if relevant

PRs that touch `src-tauri/src/crypto/` require extra review and must include a description of how the change maintains or improves the security model.

---

## Good First Issues

Look for issues labeled `good first issue` on GitHub. Some specific areas where help is welcome:

| Area | Task |
|------|------|
| Cross-platform | Test and fix on macOS |
| Cross-platform | Test and fix on Linux |
| CLI | Improve error messages and --help output |
| UI | Keyboard navigation throughout the vault |
| UI | Import from `.env` file into vault |
| Docs | Add screenshots to README |
| Docs | Document the REST API endpoints |
| Testing | Add Rust unit tests for crypto module |
| Feature | Import from 1Password / Bitwarden CSV |

---

## Questions?

Open a [Discussion](https://github.com/maosuarez/crypt-env/discussions) — we're happy to help you get started.
