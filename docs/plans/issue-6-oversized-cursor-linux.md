# Issue #6 — Mouse cursor renders extremely large inside the app (Linux)

Status: plan (no code written)
Severity: low — cosmetic / UX. No security, data-integrity, or availability impact.
Root cause: **UNCONFIRMED.** This plan is diagnosis-first by construction.
Scope if a fix turns out to be warranted: `src-tauri/tauri.conf.json` (`bundle.linux`), `src-tauri/src/lib.rs` (`run()`, pre-GTK-init only), `README.md` (Linux section, ~L161–L192). No frontend code.
Related: #5 (Linux window controls missing in `src/components/WindowChrome.tsx`) — same platform, possibly same PR. See §5.

---

## 1. Objective

This issue reports a symptom with no established cause. Committing to a fix now would be committing to a cause we have not proven. The objective is therefore **staged**: Phase A produces a verdict, and only a specific verdict unlocks Phase B.

### Phase A — Diagnosis (mandatory, always runs)

Done when **all** of the following exist as a comment on issue #6:

1. **An evidence block** with every field in §3.1 filled in (platform, session type, DE, GTK/WebKitGTK versions, cursor env vars, screenshot). No field left as "unknown" without a stated reason.
2. **The H3 discriminator answered**: does *text* inside the window render at the correct size, yes or no? (One observation, cheapest check, splits the hypothesis space in half. See §3.2, step D1.)
3. **A control-app comparison**: the same screenshot taken of an unrelated GTK3 application in the same session, showing whether its cursor is oversized too.
4. **A native-Linux reproduction result**: run on a real X11 *and* a real Wayland session outside WSL. Reproduces / does not reproduce, with screenshot.
5. **A written verdict** naming exactly one of H1–H5 (§3.2) as the cause, or explicitly stating that the evidence is insufficient and what is still missing.

Phase A is complete even if the verdict is "not our bug". That is a success, not a failure — see §4.

### Phase B — Fix (conditional; only if the Phase A verdict is H3, H4, or H5)

Acceptance criterion, measurable:

> With the app built from this branch and launched with **no cursor-related environment overrides**, the cursor bitmap rendered inside the CryptEnv window is the same pixel size as the cursor rendered over the desktop background immediately outside the window. Verified by screenshot comparison, measuring the cursor's bounding box in pixels in both positions, at **1x and 2x display scaling**, on **X11 and Wayland**, on a native (non-WSL) Linux desktop. Tolerance: exact match at 1x; ≤1px difference at 2x (rounding).
>
> Additionally, a machine whose desktop *already* has a correctly configured non-default cursor size (e.g. `cursor-size=32`) must still see 32px inside the app. A fix that hard-pins a size is a regression, not a fix.

If the verdict is H1 or H2, Phase B is **not** entered. The outcome is §4.3 (documentation + close), not code.

---

## 2. What is being mitigated

**Checkable statement:**

> After this issue is closed, a user launching CryptEnv on Linux either (a) sees a cursor matching their desktop's, or (b) can find, in the project's own Linux documentation, the one-line explanation and workaround for why they do not.

**Honest severity assessment.** This is purely cosmetic. It does not:
- expose, corrupt, or leak any secret;
- affect encryption, the vault DB, the local API, MCP, or relay sharing;
- prevent any operation — every click still lands where the pointer hotspot is;
- have any attack surface. A cursor bitmap is drawn by the compositor/toolkit; the app cannot read it and nothing crosses a trust boundary.

**Why fix it anyway.** Two reasons, both non-technical:

1. **First-launch credibility.** An oversized cursor is visible in the first half-second, before the user has typed a master password. For an app whose entire pitch is "trust me with your secrets", looking broken on first launch is disproportionately expensive relative to the defect's actual size.
2. **Linux platform coverage.** `README.md` L161 already says Linux is "in progress". This is one of only two open Linux-platform issues (the other is #5). Clearing both — even if #6 clears as "environment artifact, documented" — moves Linux from "unknown state" to "known state", which is the actual blocker on promoting Linux support.

**Explicitly NOT in scope:**
- Cursor *shape* selection (which cursor is shown over which element). The Tailwind `cursor-pointer` utilities already in use (`WindowChrome.tsx`, `EditItem.tsx`, `Settings.tsx`, etc.) are correct and untouched.
- HiDPI layout polish generally. If the verdict is H3, the fix is the scale-factor mismatch that causes the cursor symptom, not a general HiDPI audit.
- Issue #5's missing Linux window controls.

---

## 3. Implementation steps

### 3.0 Prerequisite — a verifier (blocker)

`CLAUDE.local.md` documents the maintainer's dev loop as: edit in WSL, **build and run natively on Windows** over `\\wsl.localhost\...`. That loop cannot observe this bug and cannot verify a fix. Before anything below is actionable, one of these must exist:

- a native Linux desktop (bare metal or a VM with real X11 and Wayland sessions, GPU passthrough not required), **or**
- a Linux contributor willing to run steps 3.1–3.2 and paste the evidence block.

A container is **not** sufficient unless it is running a real compositor session — a headless/Xvfb container has no cursor theme and would fabricate an H2 result.

**This is a hard blocker on closing the issue.** Do not close #6 on reasoning alone; the whole point of this plan is that the cause is unconfirmed. If no verifier is available, the correct action is to label #6 `needs-linux-verifier` and stop.

### 3.1 Evidence to collect (before any code)

Run in the session where the bug reproduces, paste output into #6:

```
uname -a
echo "XDG_SESSION_TYPE=$XDG_SESSION_TYPE  WAYLAND_DISPLAY=$WAYLAND_DISPLAY  DISPLAY=$DISPLAY"
echo "XCURSOR_SIZE=$XCURSOR_SIZE  XCURSOR_THEME=$XCURSOR_THEME"
echo "GDK_SCALE=$GDK_SCALE  GDK_DPI_SCALE=$GDK_DPI_SCALE  GDK_BACKEND=$GDK_BACKEND"
echo "XDG_CURRENT_DESKTOP=$XDG_CURRENT_DESKTOP"
gsettings get org.gnome.desktop.interface cursor-theme
gsettings get org.gnome.desktop.interface cursor-size
gsettings get org.gnome.desktop.interface text-scaling-factor
pkg-config --modversion gtk+-3.0 webkit2gtk-4.1
ls -d /usr/share/icons/*/cursors 2>/dev/null
```

Plus, non-shell:
- **Screenshot** of the app window with the cursor visible *inside* it, and a second with the cursor just *outside* it on the desktop. Same screenshot if possible.
- **WSLg version** if applicable (`wsl.exe --version` from Windows).
- **Answer to D1** below.
- `getCurrentWindow().scaleFactor()` — read from the webview devtools console while the app is running; compare against what the compositor reports.

### 3.2 Diagnostic decision tree

Run in this order. Each step is chosen to eliminate the most hypotheses per unit of effort.

```
D1. Is TEXT inside the window also oversized?
    │
    ├─ YES ──> The whole surface is being scaled, not just the cursor.
    │           => H3 (scale-factor mismatch). Go to D4.
    │
    └─ NO  ──> Layout scale is correct; only the cursor bitmap is wrong.
                => H3 eliminated. Go to D2.

D2. Does an unrelated GTK3 app in the SAME session show the same oversized cursor?
    (control apps, in order of preference: gtk3-demo, gnome-text-editor, nautilus,
     any GTK3 app that is definitely not ours)
    │
    ├─ YES ──> Not our window. Environment-wide.
    │           => H1 or H2. Go to D3.
    │
    └─ NO  ──> Only CryptEnv's window is affected. Go to D5.

D3. Is a cursor theme actually installed and resolvable?
    (`ls /usr/share/icons/*/cursors` non-empty AND the theme named by
     `gsettings get org.gnome.desktop.interface cursor-theme` is among them)
    │
    ├─ NO  ──> Missing theme => fallback bitmap, commonly oversized.
    │           => H2 confirmed. Outcome §4.3 (document, close). NOT a code fix.
    │
    └─ YES ──> Theme resolves but size is not propagating to the client.
                Cross-check XCURSOR_SIZE / cursor-size / session type.
                Under WSLg/XWayland this is the known artifact.
                => H1 confirmed. Go to D6 (native repro) to be certain.

D4. (H3 path) Compare getCurrentWindow().scaleFactor() with the compositor's
    reported scale (e.g. `wlr-randr`, `xrandr --listmonitors`, or the DE's display panel).
    Also check GDK_SCALE / GDK_DPI_SCALE / text-scaling-factor for a
    fractional or doubled value.
    │
    ├─ MISMATCH ──> H3 confirmed. Phase B, fix branch B-H3 (§3.3).
    │
    └─ MATCH ─────> Contradiction with D1 (text oversized but scale correct).
                    Re-check D1 against a known reference; if it holds,
                    escalate as an upstream WebKitGTK rendering bug (§3.3, B-UP).

D5. (only-our-window path) Rule out the app itself:
      a. `grep -rn "cursor:" src/**/*.css`            -> already run: NO MATCHES.
      b. `grep -rniE "zoom|transform:\s*scale|image-rendering" src/`  -> check root
         containers in src/index.css, src/App.css, src/App.tsx.
      c. Any webview zoom level set via Tauri (`WebviewWindow::set_zoom`) -> grep
         src-tauri/src/ for `set_zoom`. Expected: none.
      d. Custom cursor image assets -> grep for `.cur`, `.ani`, `url(` in CSS.
    │
    ├─ SOMETHING FOUND ──> H4 confirmed. Phase B, fix branch B-H4 (§3.3). Trivial fix.
    │
    └─ NOTHING FOUND ────> Only remaining app-side candidate is the decorationless
                           window. Test H5: temporarily set "decorations": true in
                           tauri.conf.json, rebuild, observe. If the cursor
                           normalizes => H5. Otherwise => upstream (B-UP).

D6. DECISIVE TEST — reproduce on a native (non-WSL) Linux desktop,
    on BOTH X11 and Wayland, with a stock cursor theme installed.
    │
    ├─ DOES NOT REPRODUCE ──> Environment artifact. Outcome §4.3. Close #6.
    │                          Do NOT ship app code for this.
    │
    └─ REPRODUCES ──────────> Ours (or upstream's). Return to D1 with the
                              native-session evidence, which is now trustworthy.
```

### 3.3 Fix branches (Phase B only)

Entered **only** with a confirmed verdict from §3.2. Listed with cost, because the cheap-looking levers here are the dangerous ones.

**B-H4 — app-side CSS/zoom artifact.** Remove the offending rule. Smallest possible diff, no config change, no platform gating. Verify against §1 Phase B criterion. This branch is almost certainly not taken — the grep evidence already argues against it — but it is the only branch where the fix is unambiguous.

**B-H3 — scale-factor mismatch.** The window is fixed at 560x700 logical px with `resizable: false`. Investigate whether the fixed size interacts badly with a fractional compositor scale. Candidate fix is a `tauri.conf.json` window-config change or explicit scale handling, decided *after* D4 identifies which side reports the wrong number. Do **not** pre-commit to an approach here; D4's output determines it.

**B-H5 — CSD/decoration interaction.** If flipping `decorations` normalizes the cursor, the fix is not "turn decorations back on" (that would delete the custom titlebar the whole UI is built around). It is either a GTK hint set at window creation or an upstream report. Low probability; treat as a finding to file, not a redesign trigger.

**B-ENV — setting cursor env vars from Rust before GTK init** (`src-tauri/src/lib.rs`, `run()` at L64, before `tauri::Builder::default()` at L70). **Strongly discouraged.** Costs:
- It affects *every* Linux user, including the majority whose desktop is configured correctly. Overriding `XCURSOR_SIZE` for them is a regression that the Phase B acceptance criterion explicitly tests for.
- It is order-fragile: it only works if it lands before GTK reads the environment, which is an implicit ordering dependency with no compile-time guard.
- It papers over an environment problem inside a security-sensitive binary, for a cosmetic gain.
- If taken at all, it must be conditional (only set when the var is *unset*, never overwrite), and it must be commented with the issue number and the conditions under which it should be deleted.

**B-DESKTOP — `.desktop` launcher tweak via `bundle.linux.deb`** in `tauri.conf.json` (currently `"linux": { "deb": { "depends": [] } }`). Ships an env override only to users who installed the `.deb` and launch from the menu, leaving `pnpm tauri dev`, AppImage, and terminal launches untouched. Narrower blast radius than B-ENV, but also inconsistent coverage — the fix would apply in some launch paths and not others, which is its own support burden.

**B-UP — upstream report.** If the evidence points at WebKitGTK or Tauri's GTK integration, file upstream with the §3.1 evidence block, link it from #6, and take §4.3's documentation path locally in the meantime.

### 3.4 The wrong fix, stated plainly

**A CSS `cursor:` rule cannot fix an oversized system cursor bitmap.** `cursor: default`, `cursor: pointer`, a custom `cursor: url(...)` — these select *which* cursor is drawn. They do not control the size at which the toolkit rasterizes a themed cursor. Anyone reaching for `src/index.css` to fix this is fixing the wrong thing and will produce a change that looks plausible in review and does nothing on the affected machine (or worse, replaces a themed cursor with a hardcoded bitmap that ignores the user's theme entirely).

Corollary: CLAUDE.md's "Tailwind for all styles — no CSS modules or inline styles" rule is **not a blocker** on this issue, because no CSS-layer solution exists to be blocked. Mentioning the rule as a constraint here would be a category error. The rule stays intact either way.

### 3.5 Documentation deliverable (all outcomes)

Whatever the verdict, `README.md`'s Linux section (~L161–L192, alongside the existing dependency list) gains a short entry: the symptom, the confirmed cause, and the user-side workaround if applicable (e.g. exporting `XCURSOR_SIZE` / installing a cursor theme). One paragraph. This is the entire deliverable in the H1/H2 case and a footnote in the others.

---

## 4. Trade-offs / alternatives considered

### 4.1 Diagnose first vs. ship a plausible fix now

**Chosen: diagnose first.** The alternative — set `XCURSOR_SIZE` in `lib.rs` and close the issue — is faster and would probably make the reporter's screenshot look right. It is rejected because the issue body itself says the cause is unknown, and a fix aimed at an unverified cause in a shared code path is a permanent tax on every Linux user to resolve one possibly-environmental report. The cost of diagnosing is a few shell commands and one screenshot; the cost of a wrong fix is an override nobody will dare remove later.

**Cost of the chosen path:** #6 stays open longer, and it stays blocked on the §3.0 verifier, which the maintainer's documented Windows-based dev loop does not provide. That blocker is real and should be visible on the issue, not hidden inside a plan.

### 4.2 Where to intervene, if intervention is warranted

`lib.rs` env vars (B-ENV) are the most powerful and most invasive; the `.desktop` entry (B-DESKTOP) is narrower but covers only one launch path; documentation (§3.5) is weakest but has zero blast radius and cannot regress a correctly-configured desktop. Preference order is the reverse of power: documentation, then `.desktop`, then Rust env vars only with a confirmed app-side cause. The Phase B acceptance criterion's "must not break an already-correct 32px desktop" clause exists specifically to make B-ENV fail its own test if written carelessly.

### 4.3 Close as an environment issue — a legitimate outcome

If D6 shows no reproduction on native X11 and Wayland, **the correct resolution is to close #6 as an environment artifact** with the §3.1 evidence attached and the §3.5 README note added. WSLg/XWayland cursor-size propagation is a known, application-independent artifact, and `CLAUDE.local.md` explicitly documents WSLg as *not* the supported path for running this app — the supported Linux loop is a real Linux desktop or the Windows-native build.

This is a success. Shipping code into a security-sensitive binary to compensate for an unsupported development environment is worse than closing the issue with a documented explanation. The plan should not be read as biased toward finding a fix.

**Guard against over-correcting.** The WSLg hypothesis is the most likely one and it is the one most at risk of being *assumed* rather than *tested*. D2 and D6 are cheap and both must actually be run before H1 is written down as the verdict. "It's probably WSLg" is not a verdict.

### 4.4 Bundling with issue #5

#5 (missing Linux window controls) lives in `src/components/WindowChrome.tsx`, which gates controls on `isWindows` via `platform()` from `@tauri-apps/plugin-os`. Both issues are Linux-only and need the same §3.0 verifier and the same test session, so verifying them together is efficient.

They should **not** share a commit or a PR. #5 is a confirmed, well-understood frontend gating bug with a known fix; #6 may well produce no code at all. Coupling them means #5 waits on #6's diagnosis for no reason, and a reviewer gets a diff mixing a real fix with a speculative one. Share the test session, split the changes.

### 4.5 Rollback

Trivial by construction. Phase A produces only an issue comment. Phase B, in every branch, is a single-file change with no persistence, no schema, no migration, and no user-visible state — `git revert` fully restores prior behaviour. The README note in §3.5 is documentation-only. There is nothing here that requires a staged rollback plan.
