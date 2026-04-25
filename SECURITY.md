# Security Policy

Thank you for helping keep DevVault secure. This document outlines our security model, supported versions, and how to report vulnerabilities.

---

## Supported Versions

| Version | Status | End of Support |
|---------|--------|-----------------|
| 0.1.x   | Active | TBD             |

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

### Private Disclosure

1. Email a detailed description to: **security@maosuarez.dev**
2. Include:
   - Affected component(s) and version(s)
   - Description of the vulnerability and its impact
   - Steps to reproduce (if applicable)
   - Suggested fix (if you have one)
3. Allow up to 72 hours for an initial response
4. We will coordinate a fix with you privately and credit you in the release notes

### Scope

**In scope for security reports:**
- Cryptographic implementation flaws (key generation, encryption, hashing)
- Plaintext exposure of secrets (in logs, responses, temporary files)
- Authentication/authorization bypass in REST API or MCP server
- Data corruption or loss scenarios
- Memory safety issues in Rust code
- Privilege escalation or sandbox escapes in the desktop app

**Out of scope (not our responsibility):**
- Physical access to the machine (attacker with shell access can read anything)
- OS-level compromise (kernel rootkits, malware)
- Password strength validation (user responsibility)
- Social engineering (phishing, credential harvesting)
- Third-party dependency vulnerabilities (we respond to critical CVEs promptly)

---

## Security Design

### Core Principles

DevVault is a **local-first vault** — no cloud, no network dependencies (except optional local REST API).

### Encryption & Key Derivation

- **Master password hashing**: Argon2id (configurable time/space cost)
- **Encryption cipher**: AES-256-GCM (AEAD, authenticated)
- **Sensitive data structures**: Zeroized on drop to prevent plaintext residue in memory

### Vault Storage

- **SQLite database** — local file, encrypted sensitive columns
- **Master key derivation** — Argon2id(master_password, salt) → AES key (in memory during session)
- **No plaintext storage** — all secrets encrypted with AES-256-GCM at rest

### MCP Server & REST API

- **MCP server** — stdio-based subprocess (no network listening)
  - Communicates with the Tauri app via local REST API
  - Never returns secret values in responses — injects as environment variables only
  - Token-authenticated (MCP token stored in app data directory, user-generated in Settings)

- **REST API** — Axum server at `127.0.0.1:47821` (localhost only)
  - Requires MCP token in `Authorization: Bearer <token>` header
  - `/unlock` endpoint locked behind rate limiting (planned: prevent brute-force)
  - No session persistence — each request stateless

### Desktop App (Tauri)

- **Hotkey handling** — global shortcut (`Ctrl+Alt+Z`) with OS-level focus detection
- **Auto-lock** — vault locks after configurable timeout (5 min default)
- **Clipboard** — copied secrets auto-clear after 30 seconds (configurable)
- **WebView CSP** — strict Content Security Policy to mitigate XSS

### Threat Model

**We protect against:**
- Accidental plaintext exposure (encryption, sanitized logs)
- Timing-based attacks (constant-time token comparison)
- Brute-force password guessing (Argon2id cost, rate limiting)
- Memory disclosure (zeroize, locked pages where possible)

**We do NOT protect against:**
- Local privilege escalation (assumes single trusted user per machine)
- OS-level malware or rootkits
- Side-channel attacks (timing, power analysis, etc.)

---

## Security Updates

### Policy

- Critical vulnerabilities (auth bypass, data exposure): hotfix released within 72 hours
- High severity (cryptographic flaws, privilege escalation): patch release within 1 week
- Medium severity (DoS, information disclosure): minor version within 2 weeks
- Low severity (hardening, code quality): next scheduled release

### Notification

Security updates are announced via:
- GitHub Releases with `[SECURITY]` tag
- Project email notifications (if you have filed an issue)

---

## Dependencies

DevVault depends on trusted crates for cryptography:

- **argon2** — OWASP recommended password hashing
- **aes-gcm** — NIST approved AEAD cipher
- **sqlx** — SQL query builder (not a web framework, minimal attack surface)
- **tauri** — desktop framework with built-in webview sandboxing

We monitor **RustSec Advisory Database** for known vulnerabilities in dependencies. Run:

```bash
cargo audit
```

to check your local build.

---

## Questions?

If you have questions about the security model or a potential vulnerability you're unsure about, email **security@maosuarez.dev** — we're happy to discuss.
