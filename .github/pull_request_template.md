---
name: Pull Request
about: Submit a change to CryptEnv
---

## Summary
A concise description of what this PR does.

Closes #(issue number — delete if not applicable)

## Type of Change
- [ ] 🐛 Bug fix
- [ ] ✨ New feature
- [ ] 🔒 Security improvement
- [ ] ♻️ Refactor (no functional changes)
- [ ] 📖 Documentation
- [ ] 🧪 Tests

## What Changed
A brief description of the implementation approach and key decisions made.

## Testing
How did you test this change?
- [ ] `cargo check` passes
- [ ] `pnpm tauri dev` runs without errors
- [ ] Manual test of the affected feature
- [ ] Tested on: (Windows / macOS / Linux)

## Security Implications
Does this PR touch sensitive areas (crypto, API auth, MCP, secret handling)?
If yes, describe how the security model is maintained or improved.

## Screenshots
For UI changes, include before/after screenshots.

---
> PRs that touch `src-tauri/src/crypto/` require extra review.
> Do not include secrets, tokens, or real credentials anywhere in this PR.
