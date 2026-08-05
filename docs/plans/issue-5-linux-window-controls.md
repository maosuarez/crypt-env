# Issue #5 — Linux: window has no close/minimize buttons

Status: plan only. No code written yet.
Scope owner: frontend chrome (`src/components/WindowChrome.tsx`) + Linux verification path.
Related: issue #6 (oversized cursor on Linux) — same platform, same verification session, **not fixed here**.

---

## 1. Objective

**Definition of done (measurable):**

1. On a Linux build produced from this branch (`.deb` installed, or `pnpm tauri build` output run directly), the window can be **minimized** and **closed** from controls rendered inside the app's titlebar. Both are reachable by mouse and by keyboard (`Tab` focus + `Enter`/`Space`).
2. The window remains **draggable** by the titlebar spacer (`data-tauri-drag-region`) on Linux — explicitly checked, because this is the one behaviour that `decorations: false` can silently break under Wayland.
3. Verified on **two** desktop configurations:
   - GNOME on Wayland (Ubuntu 24.04 default session), and
   - a non-GNOME / X11 session (KDE Plasma X11 or Xfce).
4. **No regression on Windows**, verified by the maintainer's normal loop (`pnpm tauri dev` from PowerShell against the WSL path, per `CLAUDE.local.md`): minimize works, close works, LOCK button still present and correctly hidden on the lock screen, wordmark still centred, drag still works.
5. **No change of behaviour on macOS**: macOS renders exactly what it renders today (no controls). This is a deliberate no-op — see §4.3. The macOS `dmg` job in `.github/workflows/release.yml` still builds green.
6. `pnpm build` (tsc + vite) and `cargo check` pass; no new dependency added to `package.json` or `src-tauri/Cargo.toml`.

**Explicitly NOT in the definition of done:** maximize/restore. See §4.4 — the issue's "Expected" section names it, and this plan defers it on purpose rather than silently dropping it.

---

## 2. What is being mitigated

Checkable statement of the current defect:

- `src-tauri/tauri.conf.json` sets `app.windows[0].decorations: false`. The OS therefore draws **no** titlebar, on any platform.
- `src/components/WindowChrome.tsx` renders the replacement minimize/close buttons behind `platform() === 'windows'` (line 46, `{isWindows && (...)}`).
- Consequence: on Linux there is **no way to close or minimize the window from within the application at all**. The user's only exits are the window manager's own affordances (`Alt+F4`, `Super+H`, right-click on a taskbar entry, `pkill`) — none of which are discoverable, and several of which are absent on minimal WMs.
- `bundle.targets` is `["nsis", "dmg", "deb"]` and `.github/workflows/release.yml` has a working `build-linux` job on `ubuntu-latest` that uploads a `.deb`. Linux is a **shipped target**, not hypothetical: one of three shipped bundles ships a window the user cannot close.
- The same gate also excludes macOS (`dmg`, also shipped). That is a real hole, tracked separately — see §4.3.

Verification that the defect is real: run the Linux build, confirm the titlebar shows only the `CRYPTENV` wordmark and (post-unlock) the `LOCK` button, with empty space where the Windows build has two buttons.

---

## 3. Decision

**Option (A): extend the existing custom chrome to Linux. Keep `decorations: false`. Right-aligned, Windows-order controls. No desktop-preference probing.**

Reasoning:

- `CLAUDE.md` states the window is decorationless with a custom titlebar. Option (A) is the only option that keeps that invariant; option (B) contradicts a documented project decision for one platform.
- It is the smallest correct diff: one gate condition, in one file, with buttons that already exist and already work.
- It keeps a single visual identity for an industrial/utilitarian app whose whole chrome is deliberately non-native.
- The cost of (A) is convention divergence (GNOME users expect close at the far right — which we already satisfy — and no minimize button at all in default GNOME, which we are *adding*, not removing). This is an acceptable, reversible cosmetic divergence. It is not a functional defect.

**Rejected: reading the desktop's button layout.** GNOME's `org.gnome.desktop.wm.preferences button-layout` is the only authoritative source for button order/side, it is not readable from the webview, and reading it would require a new Rust command (`window_button_layout` or similar), a gsettings/dconf dependency at runtime, a fallback path for every non-GNOME desktop that does not publish an equivalent key, and per-desktop layout code in `WindowChrome.tsx`. That is a large amount of machinery to move two buttons a few pixels in an application that already refuses to look native anywhere. **Over-engineering — do not build it.** If a user complains about order after shipping, revisit with real feedback.

---

## 4. Implementation steps

Ordered. Each step is independently checkable.

### 4.1 — Confirm permissions (no change expected)

File: `/home/maosuarez/Programas/crypt-env/src-tauri/capabilities/default.json`

Already granted:

```
core:window:allow-minimize
core:window:allow-close
core:window:allow-start-dragging
core:window:allow-is-maximized
```

Capabilities in this project are **not** platform-scoped (no `"platforms"` key), so the Windows-working permissions apply verbatim on Linux. **No permission additions are required for this plan.**

Only if maximize were adopted (it is not — §4.4) would `core:window:allow-maximize`, `core:window:allow-unmaximize` and/or `core:window:allow-toggle-maximize` need to be added here. Note `core:window:allow-is-maximized` is already present but is a getter only and grants nothing on its own.

### 4.2 — Rewrite the platform gate in `WindowChrome.tsx`

File: `/home/maosuarez/Programas/crypt-env/src/components/WindowChrome.tsx`

Replace the boolean `isWindows` state + `useEffect` with a **platform-derived config object**, computed once. Rationale for an object rather than a second boolean: the question this component actually asks is "what chrome does this platform want?", and adding platforms as booleans (`isWindows`, `isLinux`, `isMac`) produces exactly the tangle that caused this bug — a check that silently means "not-Windows = nothing". An object makes the per-platform answer explicit and makes a future macOS entry (§4.3) a data change, not a logic change.

Shape (illustrative, adjust naming to taste):

```ts
type Chrome = { showControls: boolean };

function chromeFor(os: string): Chrome {
  switch (os) {
    case 'windows':
    case 'linux':
      return { showControls: true };
    default:            // macos and anything else: unchanged behaviour
      return { showControls: false };
  }
}
```

Consume it with a **lazy `useState` initializer**, not a `useEffect`:

```ts
const [chrome] = useState(() => chromeFor(platform()));
```

Notes:
- `platform()` from `@tauri-apps/plugin-os` is **synchronous** in Tauri v2, so the current `useEffect` + `setState` is unnecessary indirection and causes a one-frame render with no controls. Removing it is in scope because we are editing those exact lines; do not extend the cleanup beyond this component.
- Prefer the lazy initializer over a module-scope `const` so the call happens at first render inside the Tauri webview rather than at import time. (`getCurrentWindow()` is already at module scope, so module scope would also work — the initializer is simply the safer of two acceptable choices.)
- Change the render gate from `{isWindows && (...)}` to `{chrome.showControls && (...)}`. **Nothing inside the button block changes**: same markup, same order (minimize then close), same right alignment, same Tailwind classes, same inline SVGs.

### 4.3 — Do NOT include macOS in this change

The `platform() === 'windows'` gate leaves macOS with no controls either, and `dmg` is a shipped target. This plan **deliberately does not fix macOS**, for two reasons:

1. `CLAUDE.md`: "Keep scope strictly to what is requested." Issue #5 is titled and scoped to Linux.
2. macOS is not a free ride-along. Its convention is traffic lights on the **left**, and the idiomatic Tauri approach there is not a hand-drawn right-aligned control cluster but `titleBarStyle: "Overlay"` with `hiddenTitle`, which keeps native traffic lights while allowing custom content — a different mechanism, a config change, and a layout change (left padding for the lights, wordmark re-centring). That is its own decision with its own trade-offs and its own verification hardware.

**Action:** open a sibling issue ("macOS: window has no close/minimize buttons") referencing this plan, so the hole is tracked rather than forgotten. Do not implement it in this PR.

### 4.4 — Maximize / restore: explicitly deferred, not dropped

The issue's Expected section says "close/minimize (and maximize)". Position:

- `tauri.conf.json` sets `"resizable": false`. With that, maximize is a no-op or a WM-refused request on most platforms — a button that visibly does nothing is worse than no button.
- Making it meaningful requires `"resizable": true`, which turns a fixed 560×700 industrial layout into a responsive one. Every screen (`ProjectManager`, `GlobalSecrets`, item forms, footer nav) would need to be re-checked at large sizes. That is a UI-layout project, not a window-controls fix.
- Maximize is also absent on Windows today, so adding it on Linux would create the platform inconsistency the issue is complaining about, in the opposite direction.

**Decision: maximize is out of scope for issue #5.** Record this as a comment on the issue when the PR opens ("minimize + close shipped; maximize deferred, requires `resizable: true` and a responsive-layout pass — filed as #NN"), so the requirement is visibly deferred rather than quietly unmet. If maximize is later adopted, it needs: `resizable: true`, `core:window:allow-toggle-maximize` in `capabilities/default.json`, a third button, and a maximized/restored icon state.

### 4.5 — Accessibility

Keep the existing pattern exactly: each button is a real `<button>` with both `title` and `aria-label` ("Minimize", "Close"). They are therefore already in the tab order and activate on `Enter`/`Space`.

One gap worth closing while in the file: the buttons currently express focus only through `hover:` classes, so keyboard focus is invisible. Add Tailwind `focus-visible:` styling consistent with the existing palette tokens (e.g. `focus-visible:outline focus-visible:outline-1 focus-visible:outline-bd` or an equivalent already used elsewhere in `src/components/ui/`). Tailwind classes only — **no inline styles, no CSS modules** (`CLAUDE.md`). If the reviewer considers this scope creep, it can be split; it is two class strings on buttons this PR already touches.

Tab order after the change on Linux: `LOCK` → `Minimize` → `Close`, matching Windows.

### 4.6 — Verify drag on Linux (this is a real risk, not a formality)

`data-tauri-drag-region` on the spacer `<div>` (line 29) maps to `start_dragging`, which is already permitted. On X11 this is reliable. On **Wayland** with `decorations: false`, `start_dragging` depends on the compositor honouring an `xdg_toplevel.move` with a valid input serial; there are known Tauri/wry reports of the drag not starting or ending immediately on some compositors.

During the Linux session, explicitly test: press and hold on empty titlebar area, move — does the window follow? Then:

- **Works:** nothing to do.
- **Fails:** do *not* patch it inside this PR silently. File it as a separate issue, and treat it as the trigger to reconsider option (B) for Linux (native decorations give drag for free). A window with working buttons but no drag is still a materially better state than today, so it does not block this PR.

Also note (observed behaviour, do not change): `win.close()` terminates the process, which also drops the `Ctrl+Alt+Z` global show/hide shortcut registered in `src-tauri/src/lib.rs`. That is identical to Windows behaviour today. Out of scope.

### 4.7 — No Rust changes

No new Tauri command, no `invoke()` helper, no `src-tauri/src/lib.rs` edit, no `Cargo.toml` change. `tauri-plugin-os` is already a dependency and `@tauri-apps/plugin-os` is already in `package.json`. If a future decision reverses §3 and requires reading the desktop button layout, that command must follow the `module_action` naming rule (e.g. `window_button_layout`) and be registered in the `invoke_handler!` list in `src-tauri/src/lib.rs` — but this plan does not add one.

---

## 5. Verification — the hard part

**The maintainer has no routine Linux run path.** `CLAUDE.local.md` documents the loop as: edit in WSL, build and run natively on Windows against `\\wsl.localhost\...`. `cargo check`/`clippy` in WSL prove compilation, not window behaviour. **"It compiles in WSL" is not verification of this fix.**

### 5.1 What CI gives us today

`.github/workflows/release.yml` already has a `build-linux` job on `ubuntu-latest` that installs `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `patchelf` and runs `pnpm tauri build`, uploading `.deb` (and referencing AppImage paths, though `bundle.targets` does not currently list `appimage`).

But it triggers only on `push` of a `v*` tag or `workflow_dispatch`. **It does not gate PRs**, and CI is headless — it can produce a `.deb`, it can never click a button.

### 5.2 Getting a testable Linux artifact

Two workable routes, in order of preference:

1. **`workflow_dispatch` the existing release workflow from the branch** and download the `linux-artifacts` `.deb`. No new CI file, no new maintenance. Downside: it is a release workflow, so check it does not publish or tag when dispatched from a non-tag ref before using it this way.
2. **Build inside the target VM itself** (`git clone` or mount the repo, install the same apt deps, `pnpm install`, `pnpm tauri build --bundles deb`, or just `pnpm tauri dev`). Slowest first run, but it collapses build and test into one place and gives a live dev loop for iterating on the fix.

**Do not** build the Linux bundle in the WSL working copy: `CLAUDE.local.md` requires `pnpm install` to happen only on the Windows side for this repo, and a Linux-native `pnpm install` would replace `node_modules` with Linux binaries and break the Windows loop. If a WSL build is wanted anyway, do it in a **separate clone or git worktree** with its own `node_modules`. (`CARGO_TARGET_DIR` is already redirected per `CLAUDE.local.md`, so Rust artifacts do not collide.)

### 5.3 Where to actually click the buttons

| Environment | Verdict |
|---|---|
| **VM: Ubuntu 24.04, GNOME on Wayland** | **Required.** Representative of the default `.deb` audience. Install the `.deb`, launch from the app grid. |
| **VM/session: KDE Plasma X11 or Xfce** | **Required.** Second data point for a different WM and X11 instead of Wayland. Can be a second session installed in the same VM and chosen at the login screen. |
| **WSLg** | **Optional smoke test only.** It will launch the GTK/WebKit app and will prove the buttons invoke the right APIs, but it runs a Weston-based compositor with no real desktop shell — minimize semantics, drag behaviour and WM conventions are **not** representative. Never sufficient on its own. |
| **Headless container / Xvfb** | Not acceptable for this issue. No human can observe the window. |
| **CI** | Build-only. Proves the branch compiles and bundles on Linux; proves nothing about controls. |

### 5.4 Merge gate

**Linux verification is a merge blocker for this PR and cannot be satisfied from the Windows loop alone.** The PR description must state which distro, desktop and session type (Wayland/X11) were used, and must record the results of: minimize, close, keyboard activation, and drag (§4.6). Windows non-regression is verified separately in the normal loop.

### 5.5 Shared session with issue #6

Issue #6 (oversized cursor on Linux) needs the exact same scarce resource: a real Linux desktop session. Stand up the VM once and verify both. Whether they ship as one PR or two is a reviewer preference — one PR is defensible since both are Linux-only chrome fixes gated on one verification session; two keeps the revert surface minimal. Either way, **keep the commits separate**, and **do not design or implement #6's fix in this plan**.

---

## 6. Trade-offs / alternatives considered

### Option A — extend the custom chrome to Linux (**chosen**)

**Gains:** one consistent industrial look across Windows and Linux; upholds the `CLAUDE.md` decorationless directive; zero config change; zero new dependency; ~10 changed lines in one file; trivially revertible.

**Losses:**
- Ignores per-desktop button conventions. GNOME's default layout is close-only on the right (we match the side, and add a minimize GNOME would not draw); elementary OS puts close on the **left**; some KDE/older setups differ. Some users will find it slightly foreign — which is already true of every other pixel in this app.
- No native window menu on titlebar right-click.
- No native double-click-to-maximize affordance and no resize grips, since `decorations: false` also removes those. (Moot today: `resizable: false`.)
- Continues to rely on `data-tauri-drag-region` for movement, which is the fragile part on Wayland (§4.6).

### Option B — enable native decorations on Linux only (**rejected**)

Mechanism: `tauri.conf.json` has no per-platform window override, so this needs either a platform-conditional `WebviewWindowBuilder` at window creation in `src-tauri/src/lib.rs`, or `#[cfg(target_os = "linux")] window.set_decorations(true)` in the existing `.setup()` closure.

**Genuine strengths (no strawman):** every desktop's own button order, side, styling, window menu, snapping and drag come for free and stay correct forever, including on desktops nobody on this project has ever run. Zero frontend logic. Immune to the Wayland drag problem entirely.

**Why rejected:**
- **Double titlebar.** A GTK titlebar would sit directly above the 48px `WindowChrome` bar, which still holds the `CRYPTENV` wordmark and the `LOCK` button. Two stacked bars in a 700px-tall window is visibly wrong. The real cost is therefore not "flip a flag" — it is conditionally restructuring `WindowChrome` on Linux (slim it, or relocate `LOCK` into a screen header) and re-validating layout in `src/App.tsx`, whose root is a fixed `flex flex-col w-full h-full` column.
- Two visual identities for the product, contradicting the documented decorationless decision in `CLAUDE.md`.
- Larger blast radius: touches Rust startup, config, and the shared chrome component — versus one gate condition.

**When to revisit:** if §4.6 shows drag is broken on Wayland, or if user reports show the custom controls are genuinely disorienting on Linux. Option B is the designated fallback, not a dead end.

### Option C — read the desktop button-layout preference and mirror it (**rejected**)

Adds a Rust command to read `org.gnome.desktop.wm.preferences button-layout` (and equivalents), plus per-desktop rendering in `WindowChrome.tsx`. Rejected as over-engineering: substantial cross-desktop machinery, a new IPC surface, and a permanent maintenance obligation, all to reposition two buttons in an app that is intentionally non-native everywhere else. Revisit only on real user feedback, never speculatively.

### Option D — do nothing / document `Alt+F4` (**rejected**)

Zero effort, zero risk. Rejected: a shipped desktop app whose window cannot be closed from its own UI is broken, and `Alt+F4` is neither discoverable nor universal across WMs.

---

## 7. Risks

| Risk | Early warning | Mitigation |
|---|---|---|
| `data-tauri-drag-region` does not work under some Wayland compositors | Window does not follow the pointer during the §4.6 drag test | File separately; does not block this PR; escalate to option B if widespread |
| Tiling WMs (i3, sway, Hyprland) force-resize the window despite `resizable: false` | Layout looks stretched/cramped at non-560×700 sizes | Layout is flex-based and should tolerate it; if it does not, that is a separate responsive-layout issue, not a controls issue |
| Users expect close on the left (elementary OS) or expect no minimize (GNOME) | Post-release user reports | Cosmetic, single-line revert of the ordering; deliberately deferred until evidence exists |
| Verification is skipped because the VM is inconvenient | PR opened with only "compiles in WSL" evidence | §5.4 makes Linux verification an explicit merge blocker with required PR-description fields |
| macOS hole forgotten once #5 closes | Issue #5 closed with no macOS follow-up | §4.3 requires filing the sibling issue before merge |
| Maximize silently treated as delivered | Issue #5 closed without mentioning maximize | §4.4 requires an explicit deferral comment on the issue |

**Assumptions that would invalidate this plan if false:**
- `platform()` from `@tauri-apps/plugin-os` returns `'linux'` on the `.deb` build (confirm in the VM session — a wrong string means the gate silently fails again).
- Capabilities in `capabilities/default.json` are not platform-filtered (verified: no `"platforms"` key, so they apply on Linux).
- `decorations: false` is honoured on the target compositors (if a compositor forces server-side decorations, we land in option B's double-titlebar problem by accident and must handle it explicitly).

---

## 8. Rollback

Single-file frontend change, no schema, no persisted state, no IPC surface, no dependency. Rollback is `git revert` of the commit; the app returns to today's behaviour with no migration and no user-visible data impact. If the `focus-visible` styling (§4.5) is contested it can be reverted independently.

---

## 9. Documentation

`CLAUDE.md` already states the window is decorationless with a custom titlebar and window controls; option (A) makes the implementation match that statement on one more platform, so **no `CLAUDE.md` edit is required**. Per `CLAUDE.md` ("Do not generate multiple `.md` documentation files"), this plan file is the only document produced — no extra README or ADR. Remaining written output belongs in the PR description (verification evidence, §5.4) and in issue comments (§4.3 macOS follow-up, §4.4 maximize deferral).
